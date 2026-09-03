use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::app_state::{SharedState, lock_data, lock_flows};
use crate::codex::write_codex_auth;
use crate::dto::AccountDto;
use crate::errors::{AppError, AppResult};
use crate::models::{
    Account, AccountProvider, AppData, TokenHealth, TokenHealthStatus, Tokens, now_ts,
};
use crate::providers::gemini::oauth::{
    build_google_authorize_url, exchange_google_code_for_tokens, fetch_google_user_info,
    google_reauth_required, save_authenticated_gemini_account,
};
use crate::providers::gemini::quota::fetch_gemini_quota;
use crate::quota::fetch_quota;
use crate::storage::persist_app_data;
use crate::subscription::{fetch_subscription_info, next_subscription_check};
use crate::token_utils::{extract_account_id, extract_email};

pub const OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const OAUTH_ISSUER: &str = "https://auth.openai.com";
const OAUTH_SCOPE: &str = "openid profile email offline_access";
const OAUTH_ORIGINATOR: &str = "codex_cli_rs";
pub use crate::oauth::server::{
    CALLBACK_ADDR, MAX_OAUTH_ERROR_BODY_BYTES, MAX_OAUTH_SUCCESS_BODY_BYTES, OAUTH_REDIRECT_URI,
    ensure_callback_server, handle_callback_stream, html_escape, html_message,
    parse_callback_target, write_error_http_response, write_http_response,
    write_http_response_with_limit,
};
const ACTIVE_FLOW_MAX_AGE_SECS: i64 = 10 * 60;
const TERMINAL_FLOW_MAX_AGE_SECS: i64 = 2 * 60;
const MAX_OAUTH_ERROR_MESSAGE_CHARS: usize = 240;

#[derive(Debug, Clone)]
pub enum OauthFlowStatus {
    WaitingCallback,
    Exchanging,
    Completed,
    Cancelled,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct OauthFlow {
    pub id: String,
    pub provider: AccountProvider,
    pub state: String,
    pub code_verifier: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub terminal_at: Option<i64>,
    pub authorization_url: String,
    pub callback_url: Option<String>,
    pub target_account_id: Option<String>,
    pub result_account_id: Option<String>,
    pub status: OauthFlowStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OauthStartResponse {
    pub flow_id: String,
    pub authorization_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OauthFlowResponse {
    pub flow_id: String,
    pub authorization_url: String,
    pub callback_url: Option<String>,
    pub created_at: i64,
    pub status: String,
    pub error: Option<String>,
    pub account: Option<AccountDto>,
}

fn random_urlsafe(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn build_pkce() -> (String, String) {
    let verifier = random_urlsafe(64);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn build_authorize_url(state: &str, challenge: &str) -> AppResult<String> {
    let mut url =
        Url::parse(&format!("{OAUTH_ISSUER}/oauth/authorize")).map_err(|source| AppError::Url {
            context: "Failed to build OAuth URL",
            source,
        })?;

    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", OAUTH_CLIENT_ID)
        .append_pair("redirect_uri", OAUTH_REDIRECT_URI)
        .append_pair("scope", OAUTH_SCOPE)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("originator", OAUTH_ORIGINATOR)
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true");

    Ok(url.to_string())
}

pub fn build_oauth_flow(
    provider: AccountProvider,
    target_account_id: Option<String>,
    login_hint: Option<&str>,
) -> AppResult<(OauthFlow, OauthStartResponse)> {
    let (code_verifier, code_challenge) = build_pkce();
    let flow_state = random_urlsafe(32);
    let auth_url = match provider {
        AccountProvider::Codex => build_authorize_url(&flow_state, &code_challenge)?,
        AccountProvider::Gemini => {
            build_google_authorize_url(&flow_state, &code_challenge, login_hint)?
        }
    };
    let flow_id = Uuid::new_v4().to_string();

    let created_at = now_ts();
    let flow = OauthFlow {
        id: flow_id.clone(),
        provider,
        state: flow_state,
        code_verifier,
        created_at,
        updated_at: created_at,
        terminal_at: None,
        authorization_url: auth_url.clone(),
        callback_url: None,
        target_account_id,
        result_account_id: None,
        status: OauthFlowStatus::WaitingCallback,
    };

    let response = OauthStartResponse {
        flow_id,
        authorization_url: auth_url,
    };

    Ok((flow, response))
}

async fn read_bounded_response(
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

async fn read_bounded_error_body(
    mut response: reqwest::Response,
    context: &'static str,
) -> AppResult<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|source| AppError::Http { context, source })?
    {
        let remaining = MAX_OAUTH_ERROR_BODY_BYTES.saturating_sub(body.len());
        if remaining == 0 {
            break;
        }
        let take = chunk.len().min(remaining);
        body.extend_from_slice(&chunk[..take]);
    }
    Ok(body)
}

fn sanitize_oauth_error(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_OAUTH_ERROR_MESSAGE_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

async fn exchange_code_for_tokens(
    client: &reqwest::Client,
    code: &str,
    code_verifier: &str,
) -> AppResult<Tokens> {
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", OAUTH_CLIENT_ID),
        ("code", code),
        ("code_verifier", code_verifier),
        ("redirect_uri", OAUTH_REDIRECT_URI),
    ];

    let response = client
        .post(format!("{OAUTH_ISSUER}/oauth/token"))
        .header("Accept", "application/json")
        .header("User-Agent", "codex-cli")
        .form(&form)
        .send()
        .await
        .map_err(|source| AppError::Http {
            context: "OAuth token request failed",
            source,
        })?;

    let status = response.status();
    if !status.is_success() {
        let details = read_bounded_error_body(response, "OAuth error response")
            .await
            .map(|raw| sanitize_oauth_error(&raw))?;
        return Err(AppError::msg(format!(
            "OAuth exchange failed ({status}): {details}"
        )));
    }

    let body = read_bounded_response(
        response,
        "OAuth token response",
        MAX_OAUTH_SUCCESS_BODY_BYTES,
    )
    .await?;
    let payload: Value = serde_json::from_slice(&body).map_err(|source| AppError::Json {
        context: "Invalid OAuth payload",
        source,
    })?;
    let access_token = payload
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::msg("OAuth payload missing access_token"))?
        .to_string();

    Ok(Tokens {
        id_token: payload
            .get("id_token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        access_token,
        refresh_token: payload
            .get("refresh_token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn upsert_account(
    data: &mut AppData,
    tokens: Tokens,
    email: Option<String>,
    account_id: Option<String>,
) -> Account {
    let now = now_ts();
    let existing = data.accounts.iter().position(|account| {
        account.provider == AccountProvider::Codex
            && (account_id.is_some() && account.account_id == account_id
                || email.is_some() && account.email == email)
    });

    if let Some(index) = existing {
        let account = &mut data.accounts[index];
        account.tokens = tokens;
        account.tokens_updated_at = Some(now);
        account.token_health = TokenHealth::healthy();
        account.quota_next_refresh_at = None;
        account.quota_refresh_failures = 0;
        account.email = email;
        account.account_id = account_id;
        account.last_login_at = now;
        account.last_error = None;
        return account.clone();
    }

    let account = Account {
        id: Uuid::new_v4().to_string(),
        provider: crate::models::AccountProvider::Codex,
        email,
        account_id,
        provider_project_id: None,
        subscription_expires_at: None,
        subscription_plan: None,
        subscription_detected_at: None,
        subscription_checked_at: None,
        subscription_next_check_at: None,
        subscription_endpoint_hint: None,
        tokens,
        token_expires_at: None,
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
    account
}

fn account_identity_matches(
    expected: &Account,
    email: Option<&str>,
    account_id: Option<&str>,
) -> bool {
    if let (Some(expected_id), Some(actual_id)) = (
        expected
            .account_id
            .as_deref()
            .filter(|value| !value.trim().is_empty()),
        account_id.filter(|value| !value.trim().is_empty()),
    ) {
        return expected_id == actual_id;
    }

    expected
        .email
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .zip(email.filter(|value| !value.trim().is_empty()))
        .is_some_and(|(expected_email, actual_email)| {
            expected_email.eq_ignore_ascii_case(actual_email)
        })
}

fn authenticated_account_label(email: Option<&str>, account_id: Option<&str>) -> String {
    email
        .filter(|value| !value.trim().is_empty())
        .or_else(|| account_id.filter(|value| !value.trim().is_empty()))
        .unwrap_or("unknown account")
        .to_string()
}

pub fn save_authenticated_account(
    data: &mut AppData,
    tokens: Tokens,
    email: Option<String>,
    account_id: Option<String>,
    target_account_id: Option<&str>,
) -> AppResult<Account> {
    if let Some(target_account_id) = target_account_id {
        let target = data
            .accounts
            .iter_mut()
            .find(|account| {
                account.id == target_account_id && account.provider == AccountProvider::Codex
            })
            .ok_or_else(|| AppError::msg("Codex re-login target account no longer exists"))?;

        if !account_identity_matches(target, email.as_deref(), account_id.as_deref()) {
            return Err(AppError::msg(format!(
                "Re-login account mismatch: expected {}, but OAuth authenticated {}. No account was changed.",
                authenticated_account_label(target.email.as_deref(), target.account_id.as_deref()),
                authenticated_account_label(email.as_deref(), account_id.as_deref())
            )));
        }

        let now = now_ts();
        target.tokens = tokens;
        target.tokens_updated_at = Some(now);
        target.token_health = TokenHealth::healthy();
        target.quota_next_refresh_at = None;
        target.quota_refresh_failures = 0;
        if email.is_some() {
            target.email = email;
        }
        if account_id.is_some() {
            target.account_id = account_id;
        }
        target.last_login_at = now;
        target.last_error = None;
        return Ok(target.clone());
    }

    Ok(upsert_account(data, tokens, email, account_id))
}

fn flow_status_text(status: &OauthFlowStatus) -> String {
    match status {
        OauthFlowStatus::WaitingCallback => "waiting_callback",
        OauthFlowStatus::Exchanging => "exchanging",
        OauthFlowStatus::Completed => "completed",
        OauthFlowStatus::Cancelled => "cancelled",
        OauthFlowStatus::Error(_) => "error",
    }
    .to_string()
}

fn flow_is_active(status: &OauthFlowStatus) -> bool {
    matches!(
        status,
        OauthFlowStatus::WaitingCallback | OauthFlowStatus::Exchanging
    )
}

fn flow_is_terminal(status: &OauthFlowStatus) -> bool {
    matches!(
        status,
        OauthFlowStatus::Completed | OauthFlowStatus::Cancelled | OauthFlowStatus::Error(_)
    )
}

pub fn flow_to_response(flow: &OauthFlow, data: &AppData) -> OauthFlowResponse {
    let account = flow
        .result_account_id
        .as_ref()
        .and_then(|id| data.accounts.iter().find(|account| &account.id == id))
        .map(AccountDto::from);
    let error = match &flow.status {
        OauthFlowStatus::Error(error) => Some(error.clone()),
        _ => None,
    };

    OauthFlowResponse {
        flow_id: flow.id.clone(),
        authorization_url: flow.authorization_url.clone(),
        callback_url: flow.callback_url.clone(),
        created_at: flow.created_at,
        status: flow_status_text(&flow.status),
        error,
        account,
    }
}

pub async fn complete_oauth_code(
    shared: &Arc<SharedState>,
    flow_id: &str,
    code: &str,
    callback_url: Option<String>,
) -> AppResult<Account> {
    let (code_verifier, target_account_id, flow_provider) = {
        let mut flows = lock_flows(shared)?;
        let flow = flows
            .get_mut(flow_id)
            .ok_or_else(|| AppError::msg("OAuth flow not found"))?;
        if flow_is_terminal(&flow.status) {
            return Err(AppError::msg("OAuth flow is already finished"));
        }
        flow.status = OauthFlowStatus::Exchanging;
        flow.updated_at = now_ts();
        if let Some(callback_url) = &callback_url {
            flow.callback_url = Some(callback_url.clone());
        }
        (
            flow.code_verifier.clone(),
            flow.target_account_id.clone(),
            flow.provider,
        )
    };

    async {
        let (tokens, token_expires_at) = if flow_provider == AccountProvider::Gemini {
            let token_set =
                exchange_google_code_for_tokens(&shared.http_client, code, &code_verifier).await?;
            (token_set.tokens, Some(token_set.expires_at))
        } else {
            (
                exchange_code_for_tokens(&shared.http_client, code, &code_verifier).await?,
                None,
            )
        };

        let (email, account_id) = if flow_provider == AccountProvider::Gemini {
            fetch_google_user_info(&shared.http_client, &tokens).await?
        } else {
            (
                extract_email(&tokens.id_token),
                extract_account_id(&tokens.id_token),
            )
        };

        if let Some(target_account_id) = target_account_id.as_deref() {
            let data = lock_data(shared)?;
            let target = data
                .accounts
                .iter()
                .find(|account| account.id == target_account_id)
                .ok_or_else(|| AppError::msg("Re-login target account no longer exists"))?;
            if !account_identity_matches(target, email.as_deref(), account_id.as_deref()) {
                return Err(AppError::msg(format!(
                    "Re-login account mismatch: expected {}, but OAuth authenticated {}. No account was changed.",
                    authenticated_account_label(
                        target.email.as_deref(),
                        target.account_id.as_deref()
                    ),
                    authenticated_account_label(email.as_deref(), account_id.as_deref())
                )));
            }
        }

        let mut gemini_project_id = None;
        let (quota_result, subscription_result) = if flow_provider == AccountProvider::Gemini {
            let quota = fetch_gemini_quota(&shared.http_client, &tokens, None)
                .await
                .map(|result| {
                    gemini_project_id = result.project_id;
                    result.quota
                });
            (quota, Err(AppError::msg("Not applicable")))
        } else {
            let limits_base_url = {
                let data = lock_data(shared)?;
                data.limits_base_url.clone()
            };
            let quota_result = fetch_quota(
                &shared.http_client,
                &limits_base_url,
                &tokens,
                account_id.as_deref(),
            )
            .await;
            let preferred_subscription_endpoint = {
                let data = lock_data(shared)?;
                target_account_id
                    .as_deref()
                    .and_then(|target_id| data.accounts.iter().find(|account| account.id == target_id))
                    .and_then(|account| account.subscription_endpoint_hint.clone())
            };
            let subscription_result = fetch_subscription_info(
                &shared.http_client,
                &limits_base_url,
                &tokens,
                account_id.as_deref(),
                preferred_subscription_endpoint.as_deref(),
            )
            .await;
            (quota_result, subscription_result)
        };

        let account = {
            let mut flows = lock_flows(shared)?;
            let flow = flows
                .get_mut(flow_id)
                .ok_or_else(|| AppError::msg("OAuth flow not found"))?;
            if matches!(flow.status, OauthFlowStatus::Cancelled) {
                return Err(AppError::msg("OAuth flow was cancelled"));
            }

            let outcome = (|| -> AppResult<Account> {
                let mut data = lock_data(shared)?;
                let mut next = data.clone();
                let account = if flow_provider == AccountProvider::Gemini {
                    save_authenticated_gemini_account(
                        &mut next,
                        tokens,
                        token_expires_at,
                        email,
                        account_id,
                        gemini_project_id,
                        target_account_id.as_deref(),
                    )?
                } else {
                    save_authenticated_account(
                        &mut next,
                        tokens,
                        email,
                        account_id,
                        target_account_id.as_deref(),
                    )?
                };
                let account_mut = next
                    .accounts
                    .iter_mut()
                    .find(|entry| entry.id == account.id)
                    .ok_or_else(|| AppError::msg("Account disappeared during OAuth completion"))?;

                match quota_result {
                    Ok(quota) => {
                        if let Some(plan_type) = quota.plan_type.as_ref() {
                            account_mut.subscription_plan = Some(plan_type.clone());
                            account_mut.subscription_detected_at = Some(now_ts());
                        }
                        account_mut.quota = Some(quota);
                        account_mut.last_error = None;
                    }
                    Err(err) => {
                        if flow_provider == AccountProvider::Gemini && google_reauth_required(&err)
                        {
                            account_mut.token_health =
                                TokenHealth::needs_relogin(err.user_message());
                        } else if flow_provider == AccountProvider::Gemini
                            && matches!(err, AppError::RemoteHttp { status: 500..=599, .. })
                        {
                            account_mut.token_health = TokenHealth::warning(
                                TokenHealthStatus::ServerError,
                                err.user_message(),
                            );
                        }
                        account_mut.last_error = Some(err.user_message());
                    }
                }

                if flow_provider == AccountProvider::Codex {
                    let subscription_checked_at = now_ts();
                    account_mut.subscription_checked_at = Some(subscription_checked_at);
                    account_mut.subscription_next_check_at = Some(next_subscription_check(
                        subscription_checked_at,
                        account_mut.subscription_expires_at,
                        &subscription_result,
                    ));
                    match subscription_result {
                        Ok(info) => {
                            if let Some(plan) = info.plan {
                                account_mut.subscription_plan = Some(plan);
                            }
                            if let Some(expires_at) = info.expires_at {
                                account_mut.subscription_expires_at = Some(expires_at);
                            }
                            if let Some(endpoint_hint) = info.endpoint_hint {
                                account_mut.subscription_endpoint_hint = Some(endpoint_hint);
                            }
                            account_mut.subscription_detected_at = Some(info.fetched_at);
                        }
                        Err(error) => log::warn!(
                            "Subscription auto-detect failed after OAuth for {}: {}",
                            account_mut.email.as_deref().unwrap_or(&account_mut.id),
                            error.user_message()
                        ),
                    }
                }

                let updated = account_mut.clone();
                let updates_codex_auth = flow_provider == AccountProvider::Codex
                    && next.active_account_id.as_deref() == Some(updated.id.as_str());
                let updates_antigravity_auth = flow_provider == AccountProvider::Gemini
                    && next.active_gemini_account_id.as_deref() == Some(updated.id.as_str());
                let previous_antigravity_auth = if updates_antigravity_auth {
                    crate::gemini::read_antigravity_auth()?
                } else {
                    None
                };
                if updates_codex_auth {
                    write_codex_auth(&updated.tokens, updated.account_id.as_deref())?;
                }
                if updates_antigravity_auth {
                    crate::gemini::write_antigravity_account_auth(&updated)?;
                }

                if let Err(error) = persist_app_data(&data, &next) {
                    if updates_codex_auth
                        && let Some(previous_active) = data.accounts.iter().find(|entry| {
                            data.active_account_id.as_deref() == Some(entry.id.as_str())
                        })
                        && let Err(rollback_error) = write_codex_auth(
                            &previous_active.tokens,
                            previous_active.account_id.as_deref(),
                        )
                    {
                        return Err(AppError::msg(format!(
                            "{}; failed to restore previous credentials: {}",
                            error.user_message(),
                            rollback_error.user_message()
                        )));
                    }
                    if updates_antigravity_auth
                        && let Err(rollback_error) = crate::gemini::restore_antigravity_auth(
                            previous_antigravity_auth.as_ref(),
                        )
                    {
                        return Err(AppError::msg(format!(
                            "{}; failed to restore previous Antigravity credentials: {}",
                            error.user_message(),
                            rollback_error.user_message()
                        )));
                    }
                    return Err(error);
                }
                next.revision = data.revision.saturating_add(1);
                *data = next;
                Ok(updated)
            })();

            match &outcome {
                Ok(updated) => {
                    flow.status = OauthFlowStatus::Completed;
                    flow.result_account_id = Some(updated.id.clone());
                }
                Err(error) => {
                    flow.status = OauthFlowStatus::Error(error.user_message());
                }
            }
            flow.updated_at = now_ts();
            flow.terminal_at = Some(flow.updated_at);
            outcome?
        };

        crate::tray_dashboard::emit_state_changed(shared, "accounts", vec![account.id.clone()]);
        crate::auto_refresh::notify_schedule_changed(shared);
        crate::tray_dashboard::refresh_dashboard(shared);
        Ok(account)
    }
    .await
}

pub fn cancel_oauth_flow(shared: &Arc<SharedState>, flow_id: &str) -> AppResult<()> {
    let mut flows = lock_flows(shared)?;
    let Some(flow) = flows.get_mut(flow_id) else {
        return Err(AppError::msg("OAuth flow not found"));
    };

    if flow_is_terminal(&flow.status) {
        return Err(AppError::msg("OAuth flow is already finished"));
    }

    flow.status = OauthFlowStatus::Cancelled;
    flow.updated_at = now_ts();
    flow.terminal_at = Some(flow.updated_at);
    Ok(())
}

pub fn prune_oauth_flows(shared: &Arc<SharedState>, now: i64) -> usize {
    let stale_ids = lock_flows(shared).map_or(Vec::new(), |mut flows| {
        let stale_ids: Vec<String> = flows
            .iter()
            .filter(|(_, flow)| {
                if flow_is_active(&flow.status) {
                    now.saturating_sub(flow.created_at) > ACTIVE_FLOW_MAX_AGE_SECS
                } else {
                    let terminal_at = flow.terminal_at.unwrap_or(flow.updated_at);
                    now.saturating_sub(terminal_at) > TERMINAL_FLOW_MAX_AGE_SECS
                }
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &stale_ids {
            flows.remove(id);
        }
        stale_ids
    });
    stale_ids.len()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::models::AppData;
    use crate::models::{Account, AccountProvider, TokenHealth, Tokens};

    use super::{
        OauthFlow, OauthFlowStatus, account_identity_matches, cancel_oauth_flow,
        complete_oauth_code, prune_oauth_flows, save_authenticated_account,
    };

    fn account(account_id: Option<&str>, email: Option<&str>) -> Account {
        Account {
            id: "target".to_string(),
            provider: crate::models::AccountProvider::Codex,
            email: email.map(ToOwned::to_owned),
            account_id: account_id.map(ToOwned::to_owned),
            provider_project_id: None,
            subscription_expires_at: None,
            subscription_plan: None,
            subscription_detected_at: None,
            subscription_checked_at: None,
            subscription_next_check_at: None,
            subscription_endpoint_hint: None,
            tokens: Tokens::default(),
            token_expires_at: None,
            tokens_updated_at: None,
            token_health: TokenHealth::default(),
            quota: None,
            quota_next_refresh_at: None,
            quota_refresh_failures: 0,
            created_at: 0,
            last_login_at: 0,
            last_error: None,
            subscription_error: None,
        }
    }

    fn flow(id: &str, status: OauthFlowStatus, created_at: i64) -> OauthFlow {
        OauthFlow {
            id: id.to_string(),
            provider: AccountProvider::Codex,
            state: format!("state-{id}"),
            code_verifier: "verifier".to_string(),
            created_at,
            updated_at: created_at,
            terminal_at: None,
            authorization_url: "https://auth.openai.com/oauth/authorize".to_string(),
            callback_url: None,
            target_account_id: None,
            result_account_id: None,
            status,
        }
    }

    #[test]
    fn relogin_identity_prefers_stable_account_id() {
        let target = account(Some("account-a"), Some("same@example.com"));

        assert!(account_identity_matches(
            &target,
            Some("changed@example.com"),
            Some("account-a")
        ));
        assert!(!account_identity_matches(
            &target,
            Some("same@example.com"),
            Some("account-b")
        ));
    }

    #[test]
    fn relogin_identity_falls_back_to_case_insensitive_email() {
        let target = account(None, Some("User@Example.com"));

        assert!(account_identity_matches(
            &target,
            Some("user@example.com"),
            None
        ));
        assert!(!account_identity_matches(
            &target,
            Some("other@example.com"),
            None
        ));
    }

    #[test]
    fn relogin_mismatch_does_not_create_or_modify_accounts() {
        let target = account(Some("account-a"), Some("a@example.com"));
        let original_tokens = target.tokens.clone();
        let mut data = AppData {
            accounts: vec![target],
            ..AppData::default()
        };

        let result = save_authenticated_account(
            &mut data,
            Tokens {
                id_token: "new-id".to_string(),
                access_token: "new-access".to_string(),
                refresh_token: "new-refresh".to_string(),
            },
            Some("b@example.com".to_string()),
            Some("account-b".to_string()),
            Some("target"),
        );

        assert!(result.is_err());
        assert_eq!(data.accounts.len(), 1);
        assert_eq!(
            data.accounts[0].tokens.access_token,
            original_tokens.access_token
        );
    }

    #[test]
    fn relogin_preserves_subscription_error_from_concurrent_update() {
        let mut target = account(Some("account-a"), Some("a@example.com"));
        target.subscription_error = Some("previous subscription failure".to_string());
        let mut data = AppData {
            accounts: vec![target],
            ..AppData::default()
        };

        save_authenticated_account(
            &mut data,
            Tokens {
                id_token: "new-id".to_string(),
                access_token: "new-access".to_string(),
                refresh_token: "new-refresh".to_string(),
            },
            Some("a@example.com".to_string()),
            Some("account-a".to_string()),
            Some("target"),
        )
        .expect("relogin succeeds");

        assert_eq!(
            data.accounts[0].subscription_error.as_deref(),
            Some("previous subscription failure")
        );
    }

    #[test]
    fn cancelled_flow_rejects_late_completion_without_exchange() {
        let state = Arc::new(
            crate::app_state::SharedState::new_with_startup_error(AppData::default(), None)
                .expect("create state"),
        );
        state.flows.lock().expect("lock flows").insert(
            "flow-1".to_string(),
            flow("flow-1", OauthFlowStatus::Cancelled, 0),
        );

        let result = tauri::async_runtime::block_on(complete_oauth_code(
            &state,
            "flow-1",
            "late-code",
            None,
        ));

        let error = result.expect_err("cancelled flow must fail fast");
        assert!(error.user_message().contains("already finished"));
        assert!(matches!(
            state
                .flows
                .lock()
                .expect("lock flows")
                .get("flow-1")
                .expect("flow exists")
                .status,
            OauthFlowStatus::Cancelled
        ));
    }

    #[test]
    fn cancel_marks_active_flow_but_rejects_terminal_or_unknown() {
        let state = Arc::new(
            crate::app_state::SharedState::new_with_startup_error(AppData::default(), None)
                .expect("create state"),
        );
        let mut flows = state.flows.lock().expect("lock flows");
        flows.insert(
            "active".to_string(),
            flow("active", OauthFlowStatus::WaitingCallback, 0),
        );
        flows.insert(
            "done".to_string(),
            flow("done", OauthFlowStatus::Completed, 0),
        );
        drop(flows);

        cancel_oauth_flow(&state, "active").expect("cancel active flow");
        let cancelled = state
            .flows
            .lock()
            .expect("lock flows")
            .get("active")
            .expect("flow exists")
            .clone();
        assert!(matches!(cancelled.status, OauthFlowStatus::Cancelled));
        assert!(cancelled.terminal_at.is_some());
        assert!(cancel_oauth_flow(&state, "active").is_err());
        assert!(cancel_oauth_flow(&state, "done").is_err());
        assert!(cancel_oauth_flow(&state, "missing").is_err());
    }

    #[test]
    fn prune_removes_stale_active_and_terminal_flows_only() {
        let state = Arc::new(
            crate::app_state::SharedState::new_with_startup_error(AppData::default(), None)
                .expect("create state"),
        );
        let now = 1_000_000;
        let mut flows = state.flows.lock().expect("lock flows");
        flows.insert(
            "old-active".to_string(),
            flow(
                "old-active",
                OauthFlowStatus::WaitingCallback,
                now - (10 * 60) - 1,
            ),
        );
        flows.insert(
            "recent-active".to_string(),
            flow("recent-active", OauthFlowStatus::WaitingCallback, now - 1),
        );
        flows.insert(
            "old-done".to_string(),
            flow("old-done", OauthFlowStatus::Completed, now - (2 * 60) - 1),
        );
        flows.insert(
            "recent-error".to_string(),
            flow(
                "recent-error",
                OauthFlowStatus::Error("failed".to_string()),
                now - 1,
            ),
        );
        let mut recent_terminal = flow(
            "recent-terminal",
            OauthFlowStatus::Completed,
            now - (2 * 60) - 1,
        );
        recent_terminal.terminal_at = Some(now - 1);
        flows.insert("recent-terminal".to_string(), recent_terminal);
        drop(flows);

        let pruned = prune_oauth_flows(&state, now);

        assert_eq!(pruned, 2);
        let remaining = state.flows.lock().expect("lock flows");
        assert!(remaining.contains_key("recent-active"));
        assert!(remaining.contains_key("recent-error"));
        assert!(remaining.contains_key("recent-terminal"));
        assert!(!remaining.contains_key("old-active"));
        assert!(!remaining.contains_key("old-done"));
    }

    #[test]
    fn prune_uses_terminal_time_not_creation_time_for_finished_flows() {
        let state = Arc::new(
            crate::app_state::SharedState::new_with_startup_error(AppData::default(), None)
                .expect("create state"),
        );
        let now = 1_000_000;
        let mut old_created = flow(
            "old-created",
            OauthFlowStatus::Completed,
            now - (2 * 60) - 1,
        );
        old_created.updated_at = now - 1;
        old_created.terminal_at = Some(now - 1);
        let mut recent_created = flow(
            "recent-created",
            OauthFlowStatus::Error("failed".to_string()),
            now - 1,
        );
        recent_created.terminal_at = Some(now - (2 * 60) - 1);
        state
            .flows
            .lock()
            .expect("lock flows")
            .insert("old-created".to_string(), old_created);
        state
            .flows
            .lock()
            .expect("lock flows")
            .insert("recent-created".to_string(), recent_created);

        assert_eq!(prune_oauth_flows(&state, now), 1);
        let remaining = state.flows.lock().expect("lock flows");
        assert!(remaining.contains_key("old-created"));
        assert!(!remaining.contains_key("recent-created"));
    }
}
