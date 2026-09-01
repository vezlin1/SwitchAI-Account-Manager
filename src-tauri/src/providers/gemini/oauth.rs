use reqwest::Client;
use serde::Deserialize;
use url::Url;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{Account, AccountProvider, AppData, TokenHealth, Tokens, now_ts};
use crate::token_utils::{extract_account_id, extract_email};

const DEFAULT_GOOGLE_OAUTH_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
// Google issues this as an installed-application credential. Native client
// secrets cannot be confidential and this value is also distributed with the
// Antigravity desktop client. Environment overrides make rotations recoverable.
const DEFAULT_GOOGLE_OAUTH_CLIENT_SECRET: &str = "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";
pub const GOOGLE_OAUTH_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
pub const GOOGLE_OAUTH_SCOPE: &str = "openid https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs";
pub const GOOGLE_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const MAX_GOOGLE_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_GOOGLE_ERROR_BYTES: usize = 64 * 1024;
const MAX_GOOGLE_ERROR_CHARS: usize = 320;

fn configured_value(name: &str, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

pub fn google_oauth_client_id() -> String {
    configured_value(
        "ANTIGRAVITY_OAUTH_CLIENT_ID",
        DEFAULT_GOOGLE_OAUTH_CLIENT_ID,
    )
}

fn google_oauth_client_secret() -> String {
    configured_value(
        "ANTIGRAVITY_OAUTH_CLIENT_SECRET",
        DEFAULT_GOOGLE_OAUTH_CLIENT_SECRET,
    )
}

pub fn build_google_authorize_url(
    state: &str,
    challenge: &str,
    login_hint: Option<&str>,
) -> AppResult<String> {
    let mut url = Url::parse(GOOGLE_OAUTH_AUTH_URL).map_err(|source| AppError::Url {
        context: "Failed to build Google OAuth URL",
        source,
    })?;
    let client_id = google_oauth_client_id();

    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", GOOGLE_REDIRECT_URI)
            .append_pair("response_type", "code")
            .append_pair("scope", GOOGLE_OAUTH_SCOPE)
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("access_type", "offline")
            .append_pair("include_granted_scopes", "true")
            .append_pair("prompt", "select_account consent");
        if let Some(login_hint) = login_hint.filter(|value| !value.trim().is_empty()) {
            query.append_pair("login_hint", login_hint);
        }
    }

    Ok(url.to_string())
}

#[derive(Debug, Clone)]
pub struct GoogleTokenSet {
    pub tokens: Tokens,
    pub expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: Option<String>,
    id_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GoogleUserInfoResponse {
    id: Option<String>,
    email: Option<String>,
}

async fn read_bounded_response(
    mut response: reqwest::Response,
    context: &'static str,
    limit: usize,
) -> AppResult<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|source| AppError::Http { context, source })?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(AppError::msg(format!(
                "{context} exceeded the {limit} byte response limit"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn sanitized_error(body: &[u8]) -> String {
    String::from_utf8_lossy(body)
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_GOOGLE_ERROR_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

async fn parse_google_token_response(
    response: reqwest::Response,
    context: &'static str,
    previous: Option<&Tokens>,
) -> AppResult<GoogleTokenSet> {
    let status = response.status();
    let retry_after_seconds = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    if !status.is_success() {
        let body = read_bounded_response(response, context, MAX_GOOGLE_ERROR_BYTES).await?;
        let details = sanitized_error(&body);
        return Err(AppError::RemoteHttp {
            context,
            status: status.as_u16(),
            retry_after_seconds,
            details: if details.is_empty() {
                "Google returned an empty error response".to_string()
            } else {
                details
            },
        });
    }

    let body = read_bounded_response(response, context, MAX_GOOGLE_RESPONSE_BYTES).await?;
    let parsed: GoogleTokenResponse =
        serde_json::from_slice(&body).map_err(|source| AppError::Json { context, source })?;
    let access_token = parsed
        .access_token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| AppError::msg("Google OAuth response is missing access_token"))?;
    let refresh_token = parsed
        .refresh_token
        .filter(|token| !token.trim().is_empty())
        .or_else(|| previous.map(|tokens| tokens.refresh_token.clone()))
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            AppError::msg(
                "Google did not return a refresh token. Revoke Antigravity access in your Google Account, then sign in again.",
            )
        })?;
    let id_token = parsed
        .id_token
        .filter(|token| !token.trim().is_empty())
        .or_else(|| previous.map(|tokens| tokens.id_token.clone()))
        .unwrap_or_default();
    let expires_in = parsed.expires_in.unwrap_or(3_600).clamp(60, 24 * 60 * 60);

    Ok(GoogleTokenSet {
        tokens: Tokens {
            id_token,
            access_token,
            refresh_token,
        },
        expires_at: now_ts().saturating_add(expires_in),
    })
}

pub async fn exchange_google_code_for_tokens(
    client: &Client,
    code: &str,
    code_verifier: &str,
) -> AppResult<GoogleTokenSet> {
    let client_id = google_oauth_client_id();
    let client_secret = google_oauth_client_secret();
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("code", code),
        ("code_verifier", code_verifier),
        ("redirect_uri", GOOGLE_REDIRECT_URI),
    ];

    let response = client
        .post(GOOGLE_OAUTH_TOKEN_URL)
        .header("Accept", "application/json")
        .header("User-Agent", "antigravity/1.18.3 windows/amd64")
        .form(&form)
        .send()
        .await
        .map_err(|source| AppError::Http {
            context: "Google OAuth token request failed",
            source,
        })?;

    parse_google_token_response(response, "Google OAuth exchange failed", None).await
}

pub async fn refresh_google_access_token(
    client: &Client,
    tokens: &Tokens,
) -> AppResult<GoogleTokenSet> {
    if tokens.refresh_token.trim().is_empty() {
        return Err(AppError::msg(
            "Google refresh token is missing. Sign in with Google again.",
        ));
    }
    let client_id = google_oauth_client_id();
    let client_secret = google_oauth_client_secret();
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("refresh_token", tokens.refresh_token.as_str()),
    ];
    let response = client
        .post(GOOGLE_OAUTH_TOKEN_URL)
        .header("Accept", "application/json")
        .header("User-Agent", "antigravity/1.18.3 windows/amd64")
        .form(&form)
        .send()
        .await
        .map_err(|source| AppError::Http {
            context: "Google OAuth refresh request failed",
            source,
        })?;

    parse_google_token_response(response, "Google OAuth refresh failed", Some(tokens)).await
}

pub fn google_reauth_required(error: &AppError) -> bool {
    match error {
        AppError::RemoteHttp { status: 401, .. } => true,
        AppError::RemoteHttp {
            status: 400,
            details,
            ..
        } => {
            let details = details.to_ascii_lowercase();
            details.contains("invalid_grant")
                || details.contains("invalid_token")
                || details.contains("unauthorized")
                || details.contains("revoked")
        }
        AppError::Message(message) => {
            let message = message.to_ascii_lowercase();
            message.contains("refresh token is missing") || message.contains("sign in with google")
        }
        _ => false,
    }
}

pub async fn fetch_google_user_info(
    client: &Client,
    tokens: &Tokens,
) -> AppResult<(Option<String>, Option<String>)> {
    let mut email = extract_email(&tokens.id_token);
    let mut account_id = extract_account_id(&tokens.id_token);

    if email.is_some() && account_id.is_some() {
        return Ok((email, account_id));
    }

    let response = client
        .get(GOOGLE_USERINFO_URL)
        .bearer_auth(&tokens.access_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|source| AppError::Http {
            context: "Google account lookup failed",
            source,
        })?;
    let status = response.status();
    if !status.is_success() {
        let body = read_bounded_response(
            response,
            "Google account lookup failed",
            MAX_GOOGLE_ERROR_BYTES,
        )
        .await?;
        return Err(AppError::RemoteHttp {
            context: "Google account lookup failed",
            status: status.as_u16(),
            retry_after_seconds: None,
            details: sanitized_error(&body),
        });
    }
    let body = read_bounded_response(
        response,
        "Google account response",
        MAX_GOOGLE_RESPONSE_BYTES,
    )
    .await?;
    let user_info: GoogleUserInfoResponse =
        serde_json::from_slice(&body).map_err(|source| AppError::Json {
            context: "Invalid Google account response",
            source,
        })?;
    if email.is_none() {
        email = user_info.email.filter(|value| !value.trim().is_empty());
    }
    if account_id.is_none() {
        account_id = user_info.id.filter(|value| !value.trim().is_empty());
    }
    if email.is_none() && account_id.is_none() {
        return Err(AppError::msg(
            "Google sign-in succeeded, but the account identity was not returned.",
        ));
    }

    Ok((email, account_id))
}

fn identity_matches(account: &Account, email: Option<&str>, account_id: Option<&str>) -> bool {
    if let (Some(expected), Some(actual)) = (
        account
            .account_id
            .as_deref()
            .filter(|value| !value.is_empty()),
        account_id.filter(|value| !value.is_empty()),
    ) {
        return expected == actual;
    }
    account
        .email
        .as_deref()
        .zip(email)
        .is_some_and(|(expected, actual)| expected.eq_ignore_ascii_case(actual))
}

#[allow(clippy::too_many_arguments)]
pub fn save_authenticated_gemini_account(
    data: &mut AppData,
    tokens: Tokens,
    token_expires_at: Option<i64>,
    email: Option<String>,
    account_id: Option<String>,
    provider_project_id: Option<String>,
    target_account_id: Option<&str>,
) -> AppResult<Account> {
    let now = now_ts();

    if let Some(target_id) = target_account_id {
        let account = data
            .accounts
            .iter_mut()
            .find(|entry| entry.id == target_id && entry.provider == AccountProvider::Gemini)
            .ok_or_else(|| AppError::msg("Google re-login target account was not found"))?;
        if !identity_matches(account, email.as_deref(), account_id.as_deref()) {
            return Err(AppError::msg(
                "Google authenticated a different account. The saved Antigravity account was not changed.",
            ));
        }
        account.tokens = tokens;
        account.token_expires_at = token_expires_at;
        account.tokens_updated_at = Some(now);
        account.token_health = TokenHealth::refreshed();
        if email.is_some() {
            account.email = email;
        }
        if account_id.is_some() {
            account.account_id = account_id;
        }
        if provider_project_id.is_some() {
            account.provider_project_id = provider_project_id;
        }
        account.quota_next_refresh_at = None;
        account.quota_refresh_failures = 0;
        account.last_login_at = now;
        account.last_error = None;
        return Ok(account.clone());
    }

    if let Some(existing) = data.accounts.iter_mut().find(|entry| {
        entry.provider == AccountProvider::Gemini
            && ((account_id.is_some() && entry.account_id == account_id)
                || (email.is_some()
                    && entry
                        .email
                        .as_deref()
                        .zip(email.as_deref())
                        .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))))
    }) {
        existing.tokens = tokens;
        existing.token_expires_at = token_expires_at;
        existing.tokens_updated_at = Some(now);
        existing.token_health = TokenHealth::refreshed();
        if email.is_some() {
            existing.email = email;
        }
        if account_id.is_some() {
            existing.account_id = account_id;
        }
        if provider_project_id.is_some() {
            existing.provider_project_id = provider_project_id;
        }
        existing.quota_next_refresh_at = None;
        existing.quota_refresh_failures = 0;
        existing.last_login_at = now;
        existing.last_error = None;
        return Ok(existing.clone());
    }

    let account = Account {
        id: Uuid::new_v4().to_string(),
        provider: AccountProvider::Gemini,
        email,
        account_id,
        provider_project_id,
        subscription_expires_at: None,
        subscription_plan: None,
        subscription_detected_at: None,
        subscription_checked_at: None,
        subscription_next_check_at: None,
        subscription_endpoint_hint: None,
        tokens,
        token_expires_at,
        tokens_updated_at: Some(now),
        token_health: TokenHealth::healthy(),
        quota: None,
        quota_next_refresh_at: None,
        quota_refresh_failures: 0,
        created_at: now,
        last_login_at: now,
        last_error: None,
        subscription_error: None,
    };

    data.accounts.push(account.clone());
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_google_account_does_not_change_antigravity_selection() {
        let mut data = AppData::default();
        let account = save_authenticated_gemini_account(
            &mut data,
            Tokens {
                id_token: "id".to_string(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
            },
            Some(now_ts() + 3_600),
            Some("user@example.com".to_string()),
            Some("google-user".to_string()),
            None,
            None,
        )
        .expect("save account");

        assert_eq!(account.provider, AccountProvider::Gemini);
        assert_eq!(data.accounts.len(), 1);
        assert_eq!(data.active_gemini_account_id, None);
    }

    #[test]
    fn google_authorize_url_uses_antigravity_scopes_pkce_and_account_picker() {
        let url = build_google_authorize_url("state", "challenge", Some("user@example.com"))
            .expect("build URL");
        let parsed = Url::parse(&url).expect("parse URL");
        let query = parsed
            .query_pairs()
            .into_owned()
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(query.get("state").map(String::as_str), Some("state"));
        assert_eq!(
            query.get("code_challenge").map(String::as_str),
            Some("challenge")
        );
        assert_eq!(
            query.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(
            query.get("login_hint").map(String::as_str),
            Some("user@example.com")
        );
        assert!(
            query
                .get("prompt")
                .is_some_and(|value| value.contains("select_account"))
        );
        let scope = query.get("scope").expect("scope");
        assert!(scope.contains("auth/cloud-platform"));
        assert!(scope.contains("auth/cclog"));
        assert!(scope.contains("auth/experimentsandconfigs"));
    }
}
