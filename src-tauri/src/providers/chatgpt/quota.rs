use serde_json::Value;

use crate::errors::{AppError, AppResult};
use crate::models::{
    Account, QuotaInfo, QuotaWindow, TokenHealth, TokenHealthStatus, Tokens, now_ts,
};
use crate::token_utils::token_exp;

const ACCESS_TOKEN_REFRESH_MARGIN_SECONDS: i64 = 10 * 60;

pub(crate) const MAX_SUCCESS_BODY_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_OAUTH_SUCCESS_BODY_BYTES: usize = 256 * 1024;

pub(crate) fn sanitize_remote_message(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<i64> {
    let value = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if let Ok(seconds) = value.parse::<i64>() {
        return Some(seconds.max(0));
    }
    chrono::DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|date| date.timestamp().saturating_sub(now_ts()).max(0))
}

pub(crate) async fn read_bounded_body(
    mut response: reqwest::Response,
    context: &'static str,
    limit_bytes: usize,
) -> AppResult<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|source| AppError::Http { context, source })?
    {
        if body.len().saturating_add(chunk.len()) > limit_bytes {
            return Err(AppError::msg(format!(
                "{context} body exceeded {limit_bytes} byte limit"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(crate) async fn read_bounded_error_body(
    mut response: reqwest::Response,
    context: &'static str,
) -> AppResult<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|source| AppError::Http { context, source })?
    {
        let remaining = MAX_ERROR_BODY_BYTES.saturating_sub(body.len());
        if remaining == 0 {
            break;
        }
        let take = chunk.len().min(remaining);
        body.extend_from_slice(&chunk[..take]);
    }
    Ok(body)
}

pub struct QuotaRefreshUpdate {
    pub quota_result: AppResult<QuotaInfo>,
    pub refreshed_tokens: Option<Tokens>,
    pub token_health: Option<TokenHealth>,
}

fn parse_quota_window(win: Option<&Value>) -> QuotaWindow {
    let Some(win) = win else {
        return QuotaWindow::default();
    };

    QuotaWindow {
        used_percent: win.get("used_percent").and_then(Value::as_f64),
        limit_window_seconds: win.get("limit_window_seconds").and_then(Value::as_i64),
        reset_at: win.get("reset_at").and_then(Value::as_i64),
        fetched_at: Some(now_ts()),
    }
}

pub async fn fetch_quota(
    client: &reqwest::Client,
    base_url: &str,
    tokens: &Tokens,
    account_id: Option<&str>,
) -> AppResult<QuotaInfo> {
    if tokens.access_token.trim().is_empty() {
        return Err(AppError::msg("Missing access_token"));
    }

    let base = base_url.trim_end_matches('/');
    let endpoint = if base.contains("/backend-api") {
        format!("{base}/wham/usage")
    } else {
        format!("{base}/api/codex/usage")
    };

    let mut request = client
        .get(endpoint)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {}", tokens.access_token))
        .header("User-Agent", "codex-cli");

    if let Some(account_id) = account_id
        && !account_id.trim().is_empty()
    {
        request = request.header("ChatGPT-Account-Id", account_id);
    }

    let response = request.send().await.map_err(|source| AppError::Http {
        context: "Quota request failed",
        source,
    })?;
    let status = response.status();
    let retry_after_seconds = parse_retry_after(response.headers());
    let body = if status.is_success() {
        read_bounded_body(response, "Quota response", MAX_SUCCESS_BODY_BYTES).await?
    } else {
        read_bounded_error_body(response, "Quota response").await?
    };
    let body = String::from_utf8_lossy(&body).into_owned();

    if !status.is_success() {
        return Err(AppError::RemoteHttp {
            context: "Quota request failed",
            status: status.as_u16(),
            retry_after_seconds,
            details: sanitize_remote_message(&body, 240),
        });
    }

    let payload: Value = serde_json::from_str(&body).map_err(|source| AppError::Json {
        context: "Invalid quota payload",
        source,
    })?;

    let rate_limit = payload.get("rate_limit");
    let primary = parse_quota_window(rate_limit.and_then(|v| v.get("primary_window")));
    let secondary = parse_quota_window(rate_limit.and_then(|v| v.get("secondary_window")));

    Ok(QuotaInfo {
        plan_type: payload
            .get("plan_type")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        primary,
        secondary,
        fetched_at: now_ts(),
    })
}

pub async fn refresh_access_token(client: &reqwest::Client, tokens: &Tokens) -> AppResult<Tokens> {
    if tokens.refresh_token.trim().is_empty() {
        return Err(AppError::msg("Missing refresh_token"));
    }

    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", crate::oauth::OAUTH_CLIENT_ID),
        ("refresh_token", tokens.refresh_token.as_str()),
    ];

    let response = client
        .post(format!("{}/oauth/token", crate::oauth::OAUTH_ISSUER))
        .header("Accept", "application/json")
        .header("User-Agent", "codex-cli")
        .form(&form)
        .send()
        .await
        .map_err(|source| AppError::Http {
            context: "OAuth refresh request failed",
            source,
        })?;

    let status = response.status();
    let body = if status.is_success() {
        read_bounded_body(
            response,
            "OAuth refresh response",
            MAX_OAUTH_SUCCESS_BODY_BYTES,
        )
        .await?
    } else {
        read_bounded_error_body(response, "OAuth refresh response").await?
    };
    let body = String::from_utf8_lossy(&body).into_owned();

    if !status.is_success() {
        return Err(AppError::msg(format!(
            "OAuth refresh failed ({status}): {}",
            sanitize_remote_message(&body, 240)
        )));
    }

    let payload: Value = serde_json::from_str(&body).map_err(|source| AppError::Json {
        context: "Invalid OAuth refresh payload",
        source,
    })?;
    let access_token = payload
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::msg("OAuth refresh payload missing access_token"))?
        .to_string();

    let id_token = payload
        .get("id_token")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| tokens.id_token.clone());
    let refresh_token = payload
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| tokens.refresh_token.clone());

    Ok(Tokens {
        id_token,
        access_token,
        refresh_token,
    })
}

pub async fn refresh_quota_with_token_refresh(
    client: &reqwest::Client,
    base_url: &str,
    account: &Account,
) -> QuotaRefreshUpdate {
    let mut current_tokens = account.tokens.clone();
    let mut refreshed_tokens: Option<Tokens> = None;
    let mut token_health: Option<TokenHealth> = None;

    if access_token_expires_soon(&current_tokens) {
        match refresh_access_token(client, &current_tokens).await {
            Ok(new_tokens) => {
                current_tokens = new_tokens.clone();
                refreshed_tokens = Some(new_tokens);
                token_health = Some(TokenHealth::refreshed());
            }
            Err(refresh_err) => {
                let message = refresh_err.user_message();
                return QuotaRefreshUpdate {
                    quota_result: Err(AppError::msg(format!(
                        "Token refresh failed before quota request: {message}"
                    ))),
                    refreshed_tokens: None,
                    token_health: Some(classify_refresh_error(&message)),
                };
            }
        }
    }

    let mut quota_result = fetch_quota(
        client,
        base_url,
        &current_tokens,
        account.account_id.as_deref(),
    )
    .await;

    if let Err(first_err) = &quota_result
        && is_quota_auth_error(&first_err.user_message())
    {
        match refresh_access_token(client, &current_tokens).await {
            Ok(new_tokens) => {
                quota_result =
                    fetch_quota(client, base_url, &new_tokens, account.account_id.as_deref()).await;
                refreshed_tokens = Some(new_tokens);
                token_health = Some(TokenHealth::refreshed());
            }
            Err(refresh_err) => {
                let message = refresh_err.user_message();
                quota_result = Err(AppError::msg(format!(
                    "Quota request failed due to auth and token refresh failed: {message}"
                )));
                token_health = Some(classify_refresh_error(&message));
            }
        }
    }

    if quota_result.is_ok() && token_health.is_none() {
        token_health = Some(TokenHealth::healthy());
    }

    QuotaRefreshUpdate {
        quota_result,
        refreshed_tokens,
        token_health,
    }
}

pub async fn refresh_quota_without_token_refresh(
    client: &reqwest::Client,
    base_url: &str,
    account: &Account,
) -> QuotaRefreshUpdate {
    let quota_result = fetch_quota(
        client,
        base_url,
        &account.tokens,
        account.account_id.as_deref(),
    )
    .await;
    let token_health = quota_result.is_ok().then(TokenHealth::healthy);

    QuotaRefreshUpdate {
        quota_result,
        refreshed_tokens: None,
        token_health,
    }
}

pub fn is_unsupported_region_error(error: &str) -> bool {
    let text = error.to_ascii_lowercase();
    text.contains("unsupported_country")
        || text.contains("unsupported country")
        || text.contains("country not supported")
        || text.contains("not available in your country")
        || text.contains("unsupported region")
        || text.contains("cf-ray")
        || text.contains("cloudflare")
        || text.contains("1020")
}

pub fn is_quota_auth_error(error: &str) -> bool {
    let text = error.to_ascii_lowercase();
    if is_unsupported_region_error(&text) {
        return false;
    }
    text.contains("(401")
        || text.contains("(403")
        || text.contains("unauthorized")
        || text.contains("invalid_token")
        || text.contains("expired")
}

pub fn is_refresh_token_reuse_error(error: &str) -> bool {
    let text = error.to_ascii_lowercase();
    text.contains("already been used")
        || text.contains("used to generate a new access token")
        || text.contains("refresh token has already been used")
        || (text.contains("refresh token") && text.contains("reuse"))
}

pub(crate) fn access_token_expires_soon(tokens: &Tokens) -> bool {
    token_exp(&tokens.access_token).is_some_and(|exp| {
        let refresh_at = exp.saturating_sub(ACCESS_TOKEN_REFRESH_MARGIN_SECONDS);
        now_ts() >= refresh_at
    })
}

pub(crate) fn classify_refresh_error(message: &str) -> TokenHealth {
    let text = message.to_ascii_lowercase();

    if is_unsupported_region_error(&text)
        || text.contains("request failed")
        || text.contains("timed out")
        || text.contains("dns")
        || text.contains("connection")
        || text.contains("network")
        || text.contains("blocked")
    {
        return TokenHealth::warning(TokenHealthStatus::NetworkError, message.to_string());
    }

    if text.contains("invalid_grant")
        || text.contains("invalid refresh")
        || text.contains("refresh token")
        || text.contains("revoked")
        || text.contains("expired")
        || text.contains("unauthorized")
        || text.contains("(400")
        || text.contains("(401")
        || text.contains("(403")
    {
        return TokenHealth::needs_relogin(message.to_string());
    }

    TokenHealth::warning(TokenHealthStatus::ServerError, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        classify_refresh_error, is_quota_auth_error, is_refresh_token_reuse_error,
        is_unsupported_region_error,
    };
    use crate::models::TokenHealthStatus;

    #[test]
    fn detects_refresh_token_reuse_errors() {
        assert!(is_refresh_token_reuse_error(
            "Your refresh token has already been used to generate a new access token"
        ));
        assert!(is_refresh_token_reuse_error(
            "OAuth refresh failed (400 Bad Request): refresh token reuse detected"
        ));
    }

    #[test]
    fn detects_unsupported_region_strings() {
        assert!(is_unsupported_region_error(
            "OAuth refresh failed (403): unsupported_country"
        ));
        assert!(is_unsupported_region_error(
            "Service is not available in your country"
        ));
        assert!(is_unsupported_region_error(
            "Cloudflare error 1020 access denied"
        ));
        assert!(!is_unsupported_region_error(
            "OAuth refresh failed: invalid_grant"
        ));
    }

    #[test]
    fn classifies_unsupported_region_as_network_warning() {
        let health =
            classify_refresh_error("OAuth refresh failed (403 Forbidden): unsupported_country");
        assert_eq!(health.status, TokenHealthStatus::NetworkError);

        let health_cf = classify_refresh_error("Cloudflare 1020 access denied");
        assert_eq!(health_cf.status, TokenHealthStatus::NetworkError);

        let health_timeout = classify_refresh_error("error sending request for url: timed out");
        assert_eq!(health_timeout.status, TokenHealthStatus::NetworkError);
    }

    #[test]
    fn distinguishes_quota_auth_error_from_geo_block() {
        assert!(!is_quota_auth_error(
            "Quota request failed (403): unsupported_country"
        ));
        assert!(is_quota_auth_error(
            "Quota request failed (401): unauthorized"
        ));
    }
}
