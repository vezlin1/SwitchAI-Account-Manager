use std::sync::Arc;

use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app_state::{SharedState, lock_data};
use crate::errors::{AppError, AppResult};
use crate::models::{
    Account, AccountProvider, QuotaInfo, QuotaWindow, QuotaWindows, TokenHealth, TokenHealthStatus,
    Tokens, now_ts,
};
use crate::oauth_gemini::{GoogleTokenSet, google_reauth_required, refresh_google_access_token};
use crate::refresh_service::AccountRefreshOutcome;
use crate::storage::commit_app_data;

const CLOUD_CODE_BASE_URL: &str = "https://daily-cloudcode-pa.googleapis.com";
const CLOUD_CODE_FALLBACK_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";
const CLOUD_CODE_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);
const TOKEN_REFRESH_SKEW_SECONDS: i64 = 15 * 60;
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_ERROR_BYTES: usize = 64 * 1024;
const FIVE_HOURS_SECONDS: i64 = 5 * 60 * 60;
const WEEK_SECONDS: i64 = 7 * 24 * 60 * 60;

#[derive(Debug)]
pub struct GeminiQuotaResult {
    pub discovery: Option<GeminiPlanCheck>,
    pub quota: AppResult<QuotaInfo>,
}

#[derive(Debug)]
pub struct GeminiPlanCheck {
    pub project_id: Option<String>,
    plan: Option<String>,
    checked_at: i64,
}

impl GeminiPlanCheck {
    pub fn apply(self, account: &mut Account) {
        if let Some(project) = self.project_id {
            account.provider_project_id = Some(project);
        }
        account.subscription_checked_at = Some(self.checked_at);
        account.subscription_detected_at = self.plan.as_ref().map(|_| self.checked_at);
        // The UI also uses the plan on the last quota snapshot. Keep the two
        // consistent even when the separate quota request fails.
        if let Some(quota) = &mut account.quota {
            quota.plan_type.clone_from(&self.plan);
        }
        account.subscription_plan = self.plan;
    }
}

#[derive(Debug, Deserialize, Default)]
struct Tier {
    id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct LoadCodeAssistResponse {
    #[serde(rename = "cloudaicompanionProject")]
    project_id: Option<String>,
    #[serde(rename = "currentTier")]
    current_tier: Option<Tier>,
    #[serde(rename = "paidTier")]
    paid_tier: Option<Tier>,
}

#[derive(Debug, Deserialize, Default)]
struct QuotaSummaryEnvelope {
    #[serde(flatten)]
    body: QuotaSummaryBody,
    response: Option<QuotaSummaryBody>,
}

#[derive(Debug, Deserialize, Default)]
struct QuotaSummaryBody {
    #[serde(default)]
    groups: Vec<QuotaSummaryGroup>,
    #[serde(default)]
    buckets: Vec<QuotaSummaryBucket>,
}

#[derive(Debug, Deserialize, Default)]
struct QuotaSummaryGroup {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(default)]
    buckets: Vec<QuotaSummaryBucket>,
}

#[derive(Debug, Deserialize, Default)]
struct QuotaSummaryBucket {
    #[serde(default)]
    disabled: bool,
    #[serde(rename = "bucketId")]
    bucket_id: Option<String>,
    window: Option<String>,
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    remaining: Option<RemainingQuota>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RemainingQuota {
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaModelGroup {
    Gemini,
    ClaudeAndGpt,
}

impl QuotaModelGroup {
    fn from_display_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "gemini models" | "gemini" => Some(Self::Gemini),
            "claude and gpt models" | "claude and gpt" => Some(Self::ClaudeAndGpt),
            _ => None,
        }
    }
}

fn antigravity_request(client: &Client, url: &str, tokens: &Tokens) -> reqwest::RequestBuilder {
    client
        .post(url)
        .bearer_auth(&tokens.access_token)
        .header("Accept", "application/json")
        // The Hub summary includes first-party and partner quota groups.
        .header("User-Agent", super::USER_AGENT)
        .header(
            "X-Goog-Api-Client",
            "google-cloud-sdk vscode_cloudshelleditor/0.1",
        )
        .header(
            "Client-Metadata",
            r#"{"ideType":"ANTIGRAVITY","platform":"WINDOWS","pluginType":"GEMINI"}"#,
        )
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
        .take(320)
        .collect::<String>()
        .trim()
        .to_string()
}

async fn post_cloud_code_to_url(
    client: &Client,
    tokens: &Tokens,
    base_url: &str,
    path: &'static str,
    body: &Value,
    context: &'static str,
) -> AppResult<Vec<u8>> {
    let response = antigravity_request(client, &format!("{base_url}{path}"), tokens)
        .timeout(CLOUD_CODE_REQUEST_TIMEOUT)
        .json(body)
        .send()
        .await
        .map_err(|source| AppError::Http { context, source })?;

    let status = response.status();
    let retry_after_seconds = crate::quota::parse_retry_after(response.headers());
    if status.is_success() {
        return read_bounded_response(response, context, MAX_RESPONSE_BYTES).await;
    }
    let error_body = read_bounded_response(response, context, MAX_ERROR_BYTES).await?;
    Err(AppError::RemoteHttp {
        context,
        status: status.as_u16(),
        retry_after_seconds,
        details: sanitized_error(&error_body),
    })
}

async fn post_cloud_code(
    client: &Client,
    tokens: &Tokens,
    path: &'static str,
    body: &Value,
    context: &'static str,
) -> AppResult<Vec<u8>> {
    match post_cloud_code_to_url(client, tokens, CLOUD_CODE_BASE_URL, path, body, context).await {
        Ok(bytes) => Ok(bytes),
        Err(primary_err) if should_try_fallback_host(&primary_err) => {
            // Antigravity language server runs against daily-cloudcode-pa.googleapis.com.
            // If the primary daily endpoint is unreachable or errors, fallback to legacy cloudcode-pa.googleapis.com.
            post_cloud_code_to_url(
                client,
                tokens,
                CLOUD_CODE_FALLBACK_BASE_URL,
                path,
                body,
                context,
            )
            .await
        }
        Err(error) => Err(error),
    }
}

fn tier_label(response: &LoadCodeAssistResponse) -> Option<String> {
    // allowedTiers are onboarding options. Only paidTier and currentTier
    // describe the account's assigned tier.
    response
        .paid_tier
        .iter()
        .chain(&response.current_tier)
        .find_map(|tier| {
            let name = tier
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty());
            let id = tier
                .id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty());
            match (id, name) {
                (Some(id), Some("Antigravity") | None) => Some(id.to_owned()),
                (_, label) => label.map(str::to_owned),
            }
        })
}

async fn discover_project_and_tier(client: &Client, tokens: &Tokens) -> AppResult<GeminiPlanCheck> {
    let body = post_cloud_code(
        client,
        tokens,
        "/v1internal:loadCodeAssist",
        &json!({ "metadata": { "ideType": "ANTIGRAVITY" } }),
        "Antigravity account discovery failed",
    )
    .await?;
    let response: LoadCodeAssistResponse =
        serde_json::from_slice(&body).map_err(|source| AppError::Json {
            context: "Invalid Antigravity account response",
            source,
        })?;
    Ok(GeminiPlanCheck {
        plan: tier_label(&response),
        project_id: response.project_id,
        checked_at: now_ts(),
    })
}

fn parse_reset_at(value: Option<&str>) -> Option<i64> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp())
        .or_else(|| value.parse::<i64>().ok())
}

impl QuotaSummaryBucket {
    fn family(&self, named_group: Option<QuotaModelGroup>) -> Option<QuotaModelGroup> {
        match self.bucket_id.as_deref() {
            Some("gemini-5h" | "gemini-weekly") => Some(QuotaModelGroup::Gemini),
            Some("3p-5h" | "3p-weekly") => Some(QuotaModelGroup::ClaudeAndGpt),
            // Other identified pools, e.g. an individual model's weekly limit,
            // do not represent the shared limit even under the same group name.
            Some(id) if id.starts_with("gemini-") || id.starts_with("3p-") => None,
            _ => named_group,
        }
    }

    fn duration(&self) -> Option<i64> {
        // Time until reset cannot distinguish a weekly limit in its last hour
        // from a five-hour limit.
        let labels = [
            self.window.as_deref(),
            self.bucket_id.as_deref(),
            self.display_name.as_deref(),
        ];
        for label in labels.into_iter().flatten() {
            let label = label.to_ascii_lowercase();
            if label.contains("week") || label == "7d" {
                return Some(WEEK_SECONDS);
            }
            if label.contains("5h") || label.contains("5 h") || label.contains("five hour") {
                return Some(FIVE_HOURS_SECONDS);
            }
        }
        None
    }

    fn window(&self, fetched_at: i64, duration: i64) -> Option<QuotaWindow> {
        if self.disabled {
            return None;
        }
        let fraction = self
            .remaining_fraction
            .or_else(|| self.remaining.as_ref()?.remaining_fraction)?;
        if !(0.0..=1.0).contains(&fraction) {
            return None;
        }
        let reset = self
            .reset_time
            .as_deref()
            .or_else(|| self.remaining.as_ref()?.reset_time.as_deref());
        Some(QuotaWindow {
            used_percent: Some((1.0 - fraction) * 100.0),
            limit_window_seconds: Some(duration),
            reset_at: parse_reset_at(reset),
            fetched_at: Some(fetched_at),
        })
    }
}

fn quota_from_summary(
    envelope: QuotaSummaryEnvelope,
    plan_type: Option<String>,
) -> Option<QuotaInfo> {
    let now = now_ts();
    let mut gemini = QuotaWindows::<QuotaWindow>::default();
    let mut partners = QuotaWindows::<QuotaWindow>::default();
    let mut reported = false;
    for body in [Some(envelope.body), envelope.response]
        .into_iter()
        .flatten()
    {
        // Prefer grouped records over the deprecated top-level buckets.
        let flat = QuotaSummaryGroup {
            buckets: body.buckets,
            ..Default::default()
        };
        for group in body.groups.iter().chain(std::iter::once(&flat)) {
            let family = group
                .display_name
                .as_deref()
                .and_then(QuotaModelGroup::from_display_name);
            for bucket in &group.buckets {
                let windows = match bucket.family(family) {
                    Some(QuotaModelGroup::Gemini) => &mut gemini,
                    Some(QuotaModelGroup::ClaudeAndGpt) => &mut partners,
                    None => continue,
                };
                let Some(duration) = bucket.duration() else {
                    continue;
                };
                let window = if duration == WEEK_SECONDS {
                    &mut windows.secondary
                } else {
                    &mut windows.primary
                };
                if window.used_percent.is_some() {
                    continue;
                }
                let Some(record) = bucket.window(now, duration) else {
                    continue;
                };
                *window = record;
                reported = true;
            }
        }
    }
    reported.then_some(QuotaInfo {
        plan_type,
        primary: gemini.primary,
        secondary: gemini.secondary,
        claude_and_gpt: Some(partners),
        fetched_at: now,
    })
}

fn method_unavailable(error: &AppError) -> bool {
    if matches!(error, AppError::RemoteHttp { details, .. } if details.to_ascii_lowercase().contains("project"))
    {
        return false;
    }
    matches!(
        error,
        AppError::RemoteHttp {
            status: 404 | 405 | 501,
            ..
        }
    )
}

fn should_try_fallback_host(error: &AppError) -> bool {
    method_unavailable(error)
        || matches!(error, AppError::Http { source, .. } if source.is_connect() || source.is_timeout())
        || matches!(
            error,
            AppError::RemoteHttp {
                status: 502..=504,
                retry_after_seconds: None,
                ..
            }
        )
}

fn discovery_due(account: &crate::models::Account, force: bool, now: i64) -> bool {
    force
        || account.provider_project_id.is_none()
        || account
            .subscription_checked_at
            .is_none_or(|checked| checked > now || now - checked >= 6 * 60 * 60)
}

pub async fn fetch_gemini_quota(
    client: &Client,
    tokens: &Tokens,
    cached_project_id: Option<&str>,
) -> GeminiQuotaResult {
    fetch_gemini_quota_with_cache(client, tokens, cached_project_id, None, true).await
}

async fn fetch_gemini_quota_with_cache(
    client: &Client,
    tokens: &Tokens,
    cached_project_id: Option<&str>,
    cached_plan: Option<&str>,
    discover: bool,
) -> GeminiQuotaResult {
    let mut discovery = None;
    let quota = async {
        if tokens.access_token.trim().is_empty() {
            return Err(AppError::msg(
                "Google access token is missing. Sign in again.",
            ));
        }
        if discover {
            match discover_project_and_tier(client, tokens).await {
                Ok(check) => discovery = Some(check),
                Err(error) if method_unavailable(&error) => {}
                Err(error) => return Err(error),
            }
        }
        let project_id = discovery
            .as_ref()
            .and_then(|check| check.project_id.as_deref())
            .or(cached_project_id);
        let plan = match &discovery {
            Some(check) => check.plan.as_deref(),
            None => cached_plan,
        };
        fetch_quota_summary(client, tokens, project_id, plan).await
    }
    .await;
    GeminiQuotaResult { discovery, quota }
}

async fn fetch_quota_summary(
    client: &Client,
    tokens: &Tokens,
    project_id: Option<&str>,
    plan: Option<&str>,
) -> AppResult<QuotaInfo> {
    let request_body =
        project_id.map_or_else(|| json!({}), |project| json!({ "project": project }));
    let body = post_cloud_code(
        client,
        tokens,
        "/v1internal:retrieveUserQuotaSummary",
        &request_body,
        "Antigravity quota summary failed",
    )
    .await?;
    let envelope = serde_json::from_slice(&body).map_err(|source| AppError::Json {
        context: "Invalid Antigravity quota summary",
        source,
    })?;
    quota_from_summary(envelope, plan.map(str::to_owned)).ok_or_else(|| {
        AppError::msg("Google did not report five-hour or weekly limits for these model groups.")
    })
}

async fn fetch_account_quota(
    client: &Client,
    tokens: &Tokens,
    account: &crate::models::Account,
    force: bool,
) -> GeminiQuotaResult {
    let discover = discovery_due(account, force, now_ts());
    let result = fetch_gemini_quota_with_cache(
        client,
        tokens,
        account.provider_project_id.as_deref(),
        account.subscription_plan.as_deref(),
        discover,
    )
    .await;
    // A stale project is retried once with fresh discovery; rate limits and auth errors never enter this path.
    if !discover
        && matches!(&result.quota, Err(AppError::RemoteHttp { status: 400 | 404, details, .. })
        if details.to_ascii_lowercase().contains("project"))
    {
        return fetch_gemini_quota_with_cache(
            client,
            tokens,
            None,
            account.subscription_plan.as_deref(),
            true,
        )
        .await;
    }
    result
}

pub async fn fresh_google_tokens(
    client: &Client,
    tokens: &Tokens,
    expires_at: Option<i64>,
) -> AppResult<GoogleTokenSet> {
    if !tokens.access_token.trim().is_empty()
        && expires_at.is_some_and(|expiry| expiry > now_ts() + TOKEN_REFRESH_SKEW_SECONDS)
    {
        return Ok(GoogleTokenSet {
            tokens: tokens.clone(),
            expires_at: expires_at.expect("checked above"),
        });
    }
    refresh_google_access_token(client, tokens).await
}

fn token_health_for_error(error: &AppError) -> TokenHealth {
    let message = error.user_message();
    if google_reauth_required(error)
        || message
            .to_ascii_lowercase()
            .contains("authorization is no longer valid")
    {
        TokenHealth::needs_relogin(message)
    } else {
        let status = match error {
            AppError::RemoteHttp {
                status: 500..=599, ..
            } => TokenHealthStatus::ServerError,
            _ => TokenHealthStatus::NetworkError,
        };
        TokenHealth::warning(status, message)
    }
}

pub async fn refresh_gemini_account(
    state: &Arc<SharedState>,
    account_id: &str,
) -> AppResult<AccountRefreshOutcome> {
    refresh_gemini_account_with_options(state, account_id, false, false).await
}

pub async fn refresh_gemini_account_with_options(
    state: &Arc<SharedState>,
    account_id: &str,
    force_discovery: bool,
    batch_metadata: bool,
) -> AppResult<AccountRefreshOutcome> {
    let _refreshing = crate::auto_refresh::AccountRefreshGuard::new(state, account_id);
    let mut warnings = Vec::new();
    let (snapshot, external_before) = {
        let mut data = lock_data(state)?;
        let external = match crate::gemini::read_antigravity_auth() {
            Ok(auth) => {
                crate::gemini::reconcile_antigravity_auth_with_snapshot(&mut data, auth.as_ref())?;
                auth
            }
            Err(error) => {
                warnings.push(format!(
                    "Local Antigravity session could not be read: {}",
                    error.user_message()
                ));
                None
            }
        };
        let account = data
            .accounts
            .iter()
            .find(|account| account.id == account_id && account.provider == AccountProvider::Gemini)
            .cloned()
            .ok_or_else(|| AppError::msg("Antigravity account not found"))?;
        (account, external)
    };

    let token_result = fresh_google_tokens(
        &state.http_client,
        &snapshot.tokens,
        snapshot.token_expires_at,
    )
    .await;
    let network_result = match token_result {
        Ok(mut token_set) => {
            let mut quota_result = fetch_account_quota(
                &state.http_client,
                &token_set.tokens,
                &snapshot,
                force_discovery,
            )
            .await;
            if matches!(
                &quota_result.quota,
                Err(AppError::RemoteHttp { status: 401, .. })
            ) && token_set.tokens == snapshot.tokens
            {
                match refresh_google_access_token(&state.http_client, &token_set.tokens).await {
                    Ok(new_token_set) => {
                        token_set = new_token_set;
                        let retry = fetch_account_quota(
                            &state.http_client,
                            &token_set.tokens,
                            &snapshot,
                            force_discovery,
                        )
                        .await;
                        quota_result.discovery = retry.discovery.or(quota_result.discovery);
                        quota_result.quota = retry.quota;
                    }
                    Err(error) => quota_result.quota = Err(error),
                }
            }
            Ok((token_set, quota_result))
        }
        Err(error) => Err(error),
    };

    let retry_after_seconds = match &network_result {
        Ok((_, result)) => result
            .quota
            .as_ref()
            .err()
            .and_then(AppError::retry_after_seconds),
        Err(error) => error.retry_after_seconds(),
    };

    let mut data = lock_data(state)?;
    let previous_external = match crate::gemini::read_antigravity_auth() {
        Ok(auth) => {
            crate::gemini::reconcile_antigravity_auth_with_snapshot(&mut data, auth.as_ref())?;
            auth
        }
        Err(error) => {
            warnings.push(format!(
                "Antigravity session was left unchanged: {}",
                error.user_message()
            ));
            None
        }
    };
    let external_unchanged = crate::gemini::antigravity_auth_unchanged(
        external_before.as_ref(),
        previous_external.as_ref(),
        &snapshot,
    );
    let current = data
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .cloned()
        .ok_or_else(|| AppError::msg("Antigravity account disappeared during refresh"))?;
    if current.tokens != snapshot.tokens || current.tokens_updated_at != snapshot.tokens_updated_at
    {
        return Ok(AccountRefreshOutcome {
            succeeded: false,
            retry_after_seconds: None,
            warnings: vec![
                "Antigravity credentials changed while quota was refreshing; the newer credentials were kept."
                    .to_string(),
            ],
        });
    }

    let settings = data.app_settings.clone().normalized();
    let is_active = data.active_gemini_account_id.as_deref() == Some(account_id);
    let mut next = data.clone();
    let account = next
        .accounts
        .iter_mut()
        .find(|account| account.id == account_id)
        .ok_or_else(|| AppError::msg("Antigravity account disappeared during refresh"))?;
    let mut refreshed_tokens = false;
    let succeeded;
    match network_result {
        Ok((token_set, quota_result)) => {
            refreshed_tokens = token_set.tokens != account.tokens
                || account.token_expires_at != Some(token_set.expires_at);
            if refreshed_tokens {
                account.tokens = token_set.tokens;
                account.token_expires_at = Some(token_set.expires_at);
                account.tokens_updated_at = Some(now_ts());
            }
            if let Some(check) = quota_result.discovery {
                check.apply(account);
            }
            match quota_result.quota {
                Ok(mut quota) => {
                    quota.plan_type.clone_from(&account.subscription_plan);
                    account.quota = Some(quota);
                    account.token_health = if refreshed_tokens {
                        TokenHealth::refreshed()
                    } else {
                        TokenHealth::healthy()
                    };
                    account.last_error = None;
                    account.quota_refresh_failures = 0;
                    account.quota_next_refresh_at = Some(crate::auto_refresh::next_after_success(
                        account,
                        &settings,
                        is_active,
                        now_ts(),
                        now_ts(),
                        true,
                    ));
                    succeeded = true;
                }
                Err(error) => {
                    account.token_health = token_health_for_error(&error);
                    account.last_error = Some(error.user_message());
                    succeeded = false;
                }
            }
        }
        Err(error) => {
            account.token_health = token_health_for_error(&error);
            account.last_error = Some(error.user_message());
            succeeded = false;
        }
    }

    let active = next.active_gemini_account_id.as_deref() == Some(account_id);
    let mut wrote_external = false;
    if active && refreshed_tokens && external_unchanged {
        match crate::gemini::write_antigravity_account_auth(account) {
            Ok(()) => wrote_external = true,
            Err(error) => warnings.push(format!(
                "Antigravity credentials could not be updated: {}",
                error.user_message()
            )),
        }
    }
    let written_tokens = account.tokens.clone();
    let commit = if batch_metadata && !wrote_external {
        crate::storage::commit_refresh_metadata(state, &mut data, next)
    } else {
        commit_app_data(&mut data, next)
    };
    if let Err(error) = commit {
        if wrote_external
            && crate::gemini::read_antigravity_auth()
                .ok()
                .flatten()
                .is_some_and(|auth| {
                    auth.tokens.access_token == written_tokens.access_token
                        && auth.tokens.refresh_token == written_tokens.refresh_token
                })
            && let Err(rollback_error) =
                crate::gemini::restore_antigravity_auth(previous_external.as_ref())
        {
            return Err(AppError::msg(format!(
                "{}; failed to restore the previous Antigravity credential: {}",
                error.user_message(),
                rollback_error.user_message()
            )));
        }
        return Err(error);
    }

    Ok(AccountRefreshOutcome {
        succeeded,
        retry_after_seconds,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_and_rate_limits_stop_fallback_requests() {
        let error = |status, retry_after_seconds| AppError::RemoteHttp {
            context: "test",
            status,
            retry_after_seconds,
            details: "test response".into(),
        };
        for status in [401, 403, 429] {
            assert!(!should_try_fallback_host(&error(status, Some(300))));
            assert!(!method_unavailable(&error(status, Some(300))));
        }
        assert!(method_unavailable(&error(404, None)));
        assert!(should_try_fallback_host(&error(503, None)));
        assert!(!should_try_fallback_host(&error(503, Some(300))));
        assert_eq!(error(429, Some(300)).retry_after_seconds(), Some(300));
    }

    #[test]
    fn plan_uses_assigned_tiers_and_preserves_provider_names() {
        let parse = |value| tier_label(&serde_json::from_value(value).unwrap());
        for (payload, expected) in [
            (
                json!({"currentTier":{"id":"free-tier","name":"Antigravity"}, "paidTier":{"id":"g1-pro-tier","name":"Google AI Pro"}}),
                Some("Google AI Pro"),
            ),
            (
                json!({"paidTier":{"id":"g1-pro-tier","name":" Google AI Pro (Trial) "}}),
                Some("Google AI Pro (Trial)"),
            ),
            (
                json!({"paidTier":{"id":"g1-ultra-tier","name":"  "}}),
                Some("g1-ultra-tier"),
            ),
            (
                json!({"paidTier":{}, "currentTier":{"id":"free-tier","name":"Antigravity"}, "ineligibleTiers":[{"tierId":"standard-tier"}]}),
                Some("free-tier"),
            ),
            (
                json!({"currentTier":{"name":"Enterprise Premium"}, "allowedTiers":null}),
                Some("Enterprise Premium"),
            ),
            (
                json!({"allowedTiers":[{"name":"Google AI Ultra","isDefault":true}], "ineligibleTiers":null}),
                None,
            ),
            (
                json!({"paidTier":{"id":" ","name":""},"currentTier":null}),
                None,
            ),
        ] {
            assert_eq!(parse(payload).as_deref(), expected);
        }
    }

    #[test]
    fn fresh_discovery_updates_the_plan_even_without_a_new_quota_snapshot() {
        let mut data = crate::models::AppData::default();
        let mut account = crate::oauth_gemini::save_authenticated_gemini_account(
            &mut data,
            Tokens::default(),
            None,
            Some("plan@example.com".into()),
            None,
            None,
            None,
        )
        .unwrap();
        account.quota = Some(QuotaInfo {
            fetched_at: 10,
            ..Default::default()
        });
        for (time, plan) in [(20, Some("Google AI Pro")), (30, None)] {
            GeminiPlanCheck {
                project_id: None,
                plan: plan.map(str::to_owned),
                checked_at: time,
            }
            .apply(&mut account);
            assert_eq!(account.subscription_plan.as_deref(), plan);
            assert_eq!(account.subscription_checked_at, Some(time));
            assert_eq!(account.subscription_detected_at, plan.map(|_| time));
            let quota = account.quota.as_ref().unwrap();
            assert_eq!(quota.plan_type.as_deref(), plan);
            assert_eq!(quota.fetched_at, 10);
        }
    }

    #[test]
    fn project_and_tier_cache_expires_and_manual_subscription_refresh_bypasses_it() {
        let now = now_ts();
        let mut data = crate::models::AppData::default();
        let mut account = crate::oauth_gemini::save_authenticated_gemini_account(
            &mut data,
            Tokens {
                access_token: "a".into(),
                refresh_token: "r".into(),
                id_token: String::new(),
            },
            Some(now + 3600),
            Some("cache@example.com".into()),
            None,
            Some("project".into()),
            None,
        )
        .unwrap();
        account.subscription_checked_at = Some(now - 60);
        assert!(!discovery_due(&account, false, now));
        assert!(discovery_due(&account, true, now));
        assert!(discovery_due(&account, false, now + 6 * 3600));
        account.provider_project_id = None;
        assert!(discovery_due(&account, false, now));
    }

    fn summary(value: Value) -> QuotaInfo {
        quota_from_summary(
            serde_json::from_value(value).unwrap(),
            Some("Google AI Pro".into()),
        )
        .unwrap()
    }

    #[test]
    fn quota_request_uses_hub_profile_for_first_party_windows() {
        let request = antigravity_request(&Client::new(), CLOUD_CODE_BASE_URL, &Tokens::default())
            .build()
            .unwrap();
        let agent = request.headers()[reqwest::header::USER_AGENT]
            .to_str()
            .unwrap();
        assert!(agent.starts_with("antigravity/hub/"));
    }

    #[test]
    fn hub_summary_keeps_each_percent_and_reset_in_its_own_group_and_period() {
        let partners = json!({"displayName": "Claude and GPT models", "buckets": [
            {"bucketId": "3p-weekly", "remainingFraction": 0.6040296, "resetTime": "2026-12-03T15:40:53Z"},
            {"bucketId": "3p-5h", "remainingFraction": 1.0}
        ]});
        let gemini = json!({"displayName": "Gemini Models", "buckets": [
            {"bucketId": "gemini-weekly", "remainingFraction": 0.5825124, "resetTime": "2026-12-03T10:44:58Z"},
            {"bucketId": "gemini-5h", "remainingFraction": 0.996712, "resetTime": "2026-11-27T16:19:27Z"}
        ]});
        let other = json!({"displayName": "Argon", "buckets": [
            {"bucketId": "gemini-argon-weekly", "window": "weekly", "remainingFraction": 0.01}
        ]});
        for groups in [
            json!([partners, gemini, other]),
            json!([other, gemini, partners]),
        ] {
            let quota = summary(json!({"groups": groups}));
            assert_eq!(quota.plan_type.as_deref(), Some("Google AI Pro"));
            let partners = quota.claude_and_gpt.unwrap();
            for (window, remaining, seconds, reset) in [
                (
                    quota.primary,
                    99.6712,
                    FIVE_HOURS_SECONDS,
                    Some("2026-11-27T16:19:27Z"),
                ),
                (
                    quota.secondary,
                    58.25124,
                    WEEK_SECONDS,
                    Some("2026-12-03T10:44:58Z"),
                ),
                (partners.primary, 100.0, FIVE_HOURS_SECONDS, None),
                (
                    partners.secondary,
                    60.40296,
                    WEEK_SECONDS,
                    Some("2026-12-03T15:40:53Z"),
                ),
            ] {
                assert!((100.0 - window.used_percent.unwrap() - remaining).abs() < 1e-8);
                assert_eq!(window.reset_at, parse_reset_at(reset));
                assert_eq!(window.limit_window_seconds, Some(seconds));
                assert_eq!(window.fetched_at, Some(quota.fetched_at));
            }
        }
    }

    #[test]
    fn legacy_partner_only_response_does_not_supply_gemini_limits() {
        let quota = summary(json!({"response": {"groups": [{"buckets": [
            {"bucketId": "3p-weekly", "remainingFraction": 0.60},
            {"bucketId": "3p-5h", "remainingFraction": 1.0}
        ]}]}}));
        assert!(quota.primary.used_percent.is_none() && quota.secondary.used_percent.is_none());
        let partners = quota.claude_and_gpt.unwrap();
        assert_eq!(partners.primary.used_percent, Some(0.0));
        assert!(partners.primary.reset_at.is_none());
        assert_eq!(partners.secondary.used_percent, Some(40.0));
    }

    #[test]
    fn named_windows_use_the_bucket_period_and_only_fill_gaps_from_flat_records() {
        let quota = summary(json!({
            "groups": [{"displayName": "Gemini Models", "buckets": [
                {"window": "weekly", "remaining": {"remainingFraction": 0.0, "resetTime": "1234"}},
                {"window": "5h", "disabled": true, "remainingFraction": 1.0}
            ]}],
            "buckets": [
                {"bucketId": "gemini-weekly", "remainingFraction": 0.8, "resetTime": "5678"},
                {"bucketId": "gemini-5h", "remainingFraction": 0.25},
                {"bucketId": "unknown", "window": "weekly", "remainingFraction": 0.01}
            ]
        }));
        assert_eq!(quota.primary.used_percent, Some(75.0));
        assert!(quota.primary.reset_at.is_none());
        assert_eq!(quota.secondary.used_percent, Some(100.0));
        assert_eq!(quota.secondary.limit_window_seconds, Some(WEEK_SECONDS));
        assert_eq!(quota.secondary.reset_at, Some(1234));
        assert!(
            quota
                .claude_and_gpt
                .unwrap()
                .secondary
                .used_percent
                .is_none()
        );
    }
}
