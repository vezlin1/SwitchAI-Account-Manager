use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app_state::{SharedState, lock_data};
use crate::errors::{AppError, AppResult};
use crate::models::{
    AccountProvider, QuotaInfo, QuotaWindow, TokenHealth, TokenHealthStatus, Tokens, now_ts,
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

#[derive(Debug, Clone)]
pub struct GeminiQuotaResult {
    pub quota: QuotaInfo,
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Tier {
    id: Option<String>,
    name: Option<String>,
    #[serde(default)]
    is_default: bool,
}

#[derive(Debug, Deserialize, Default)]
struct LoadCodeAssistResponse {
    #[serde(rename = "cloudaicompanionProject")]
    project_id: Option<String>,
    #[serde(rename = "currentTier")]
    current_tier: Option<Tier>,
    #[serde(rename = "paidTier")]
    paid_tier: Option<Tier>,
    #[serde(rename = "allowedTiers", default)]
    allowed_tiers: Vec<Tier>,
    #[serde(rename = "ineligibleTiers", default)]
    ineligible_tiers: Vec<Value>,
}

#[derive(Debug, Deserialize, Default)]
struct QuotaSummaryEnvelope {
    #[serde(default)]
    groups: Vec<QuotaSummaryGroup>,
    response: Option<QuotaSummaryBody>,
}

#[derive(Debug, Deserialize, Default)]
struct QuotaSummaryBody {
    #[serde(default)]
    groups: Vec<QuotaSummaryGroup>,
}

#[derive(Debug, Deserialize, Default)]
struct QuotaSummaryGroup {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    buckets: Vec<QuotaSummaryBucket>,
}

#[derive(Debug, Deserialize, Default)]
struct QuotaSummaryBucket {
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
    description: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RemainingQuota {
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AvailableModelsEnvelope {
    #[serde(default)]
    models: HashMap<String, AvailableModel>,
    response: Option<AvailableModelsBody>,
}

#[derive(Debug, Deserialize, Default)]
struct AvailableModelsBody {
    #[serde(default)]
    models: HashMap<String, AvailableModel>,
}

#[derive(Debug, Deserialize, Default)]
struct AvailableModel {
    #[serde(rename = "quotaInfo")]
    quota_info: Option<ModelQuotaInfo>,
}

#[derive(Debug, Deserialize, Default)]
struct ModelQuotaInfo {
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Debug, Clone)]
struct QuotaCandidate {
    remaining_fraction: f64,
    reset_at: Option<i64>,
    window_seconds: Option<i64>,
}

fn antigravity_request(client: &Client, url: &str, tokens: &Tokens) -> reqwest::RequestBuilder {
    client
        .post(url)
        .bearer_auth(&tokens.access_token)
        .header("Accept", "application/json")
        .header("User-Agent", "antigravity/1.18.3 windows/amd64")
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
    let retry_after_seconds = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
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
        Err(primary_err) => {
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
            .map_err(|_| primary_err)
        }
    }
}

fn tier_label(response: &LoadCodeAssistResponse) -> Option<String> {
    response
        .paid_tier
        .as_ref()
        .and_then(|tier| tier.name.clone().or_else(|| tier.id.clone()))
        .or_else(|| {
            response
                .ineligible_tiers
                .is_empty()
                .then(|| {
                    response
                        .current_tier
                        .as_ref()
                        .and_then(|tier| tier.name.clone().or_else(|| tier.id.clone()))
                })
                .flatten()
        })
        .or_else(|| {
            response
                .allowed_tiers
                .iter()
                .find(|tier| tier.is_default)
                .and_then(|tier| tier.name.clone().or_else(|| tier.id.clone()))
                .map(|label| format!("{label} (Restricted)"))
        })
}

async fn discover_project_and_tier(
    client: &Client,
    tokens: &Tokens,
) -> AppResult<(Option<String>, Option<String>)> {
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
    Ok((response.project_id.clone(), tier_label(&response)))
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

fn bucket_window_seconds(
    bucket: &QuotaSummaryBucket,
    group: &QuotaSummaryGroup,
    reset_at: Option<i64>,
    now: i64,
) -> Option<i64> {
    // 1. Check explicit bucket-level window and bucket_id
    let bucket_label = [
        bucket.bucket_id.as_deref(),
        bucket.window.as_deref(),
        bucket.display_name.as_deref(),
        bucket.description.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();

    if bucket_label.contains("week") || bucket_label.contains("7d") || bucket_label.contains("7 d")
    {
        return Some(WEEK_SECONDS);
    }
    if bucket_label.contains("5h")
        || bucket_label.contains("5 h")
        || bucket_label.contains("five hour")
    {
        return Some(FIVE_HOURS_SECONDS);
    }

    // 2. If reset_at is far in the future (> 5h + 15m), it is a weekly / multi-day window
    if let Some(reset_ts) = reset_at {
        let diff = reset_ts.saturating_sub(now);
        if diff > FIVE_HOURS_SECONDS + 15 * 60 {
            return Some(WEEK_SECONDS);
        }
        if diff > 0 && diff <= FIVE_HOURS_SECONDS + 15 * 60 {
            return Some(FIVE_HOURS_SECONDS);
        }
    }

    // 3. Fallback to group and bucket descriptive labels (like "session")
    let full_label = [
        bucket_label.as_str(),
        group.display_name.as_deref().unwrap_or(""),
        group.description.as_deref().unwrap_or(""),
    ]
    .join(" ")
    .to_ascii_lowercase();

    if full_label.contains("week") || full_label.contains("7d") {
        Some(WEEK_SECONDS)
    } else if full_label.contains("5h")
        || full_label.contains("5 h")
        || full_label.contains("five hour")
        || full_label.contains("session")
    {
        Some(FIVE_HOURS_SECONDS)
    } else {
        None
    }
}

fn candidate_to_window(candidate: Option<&QuotaCandidate>, fetched_at: i64) -> QuotaWindow {
    candidate.map_or_else(QuotaWindow::default, |candidate| QuotaWindow {
        used_percent: Some(
            ((1.0 - candidate.remaining_fraction.clamp(0.0, 1.0)) * 100.0).clamp(0.0, 100.0),
        ),
        limit_window_seconds: candidate.window_seconds,
        reset_at: candidate.reset_at,
        fetched_at: Some(fetched_at),
    })
}

fn strictest<'a>(
    candidates: impl Iterator<Item = &'a QuotaCandidate>,
) -> Option<&'a QuotaCandidate> {
    candidates.min_by(|left, right| {
        left.remaining_fraction
            .partial_cmp(&right.remaining_fraction)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn quota_from_summary(
    envelope: QuotaSummaryEnvelope,
    plan_type: Option<String>,
) -> Option<QuotaInfo> {
    let now = now_ts();
    let groups = if envelope.groups.is_empty() {
        envelope.response.unwrap_or_default().groups
    } else {
        envelope.groups
    };
    let mut candidates = Vec::new();
    for group in &groups {
        for bucket in &group.buckets {
            let Some(remaining_fraction) = bucket.remaining_fraction.or_else(|| {
                bucket
                    .remaining
                    .as_ref()
                    .and_then(|remaining| remaining.remaining_fraction)
            }) else {
                continue;
            };
            let reset_time = bucket.reset_time.as_deref().or_else(|| {
                bucket
                    .remaining
                    .as_ref()
                    .and_then(|remaining| remaining.reset_time.as_deref())
            });
            let reset_at = parse_reset_at(reset_time);
            candidates.push(QuotaCandidate {
                remaining_fraction,
                reset_at,
                window_seconds: bucket_window_seconds(bucket, group, reset_at, now),
            });
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let primary = strictest(
        candidates
            .iter()
            .filter(|candidate| candidate.window_seconds == Some(FIVE_HOURS_SECONDS)),
    )
    .or_else(|| {
        strictest(
            candidates
                .iter()
                .filter(|candidate| candidate.window_seconds.is_none()),
        )
    });
    let secondary = strictest(
        candidates
            .iter()
            .filter(|candidate| candidate.window_seconds == Some(WEEK_SECONDS)),
    );
    Some(QuotaInfo {
        plan_type,
        primary: candidate_to_window(primary, now),
        secondary: candidate_to_window(secondary, now),
        fetched_at: now,
    })
}

fn quota_from_available_models(
    envelope: AvailableModelsEnvelope,
    plan_type: Option<String>,
) -> Option<QuotaInfo> {
    let now = now_ts();
    let models = if envelope.models.is_empty() {
        envelope.response.unwrap_or_default().models
    } else {
        envelope.models
    };
    let candidates = models
        .into_values()
        .filter_map(|model| model.quota_info)
        .filter_map(|quota| {
            let reset_at = parse_reset_at(quota.reset_time.as_deref());
            let window_seconds = if let Some(reset_ts) = reset_at {
                let diff = reset_ts.saturating_sub(now);
                if diff > FIVE_HOURS_SECONDS + 15 * 60 {
                    Some(WEEK_SECONDS)
                } else if diff > 0 {
                    Some(FIVE_HOURS_SECONDS)
                } else {
                    None
                }
            } else {
                None
            };
            quota
                .remaining_fraction
                .map(|remaining_fraction| QuotaCandidate {
                    remaining_fraction,
                    reset_at,
                    window_seconds,
                })
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }
    let primary = strictest(
        candidates
            .iter()
            .filter(|candidate| candidate.window_seconds == Some(FIVE_HOURS_SECONDS)),
    )
    .or_else(|| {
        strictest(
            candidates
                .iter()
                .filter(|candidate| candidate.window_seconds.is_none()),
        )
    });
    let secondary = strictest(
        candidates
            .iter()
            .filter(|candidate| candidate.window_seconds == Some(WEEK_SECONDS)),
    );
    Some(QuotaInfo {
        plan_type,
        primary: candidate_to_window(primary, now),
        secondary: candidate_to_window(secondary, now),
        fetched_at: now,
    })
}

pub async fn fetch_gemini_quota(
    client: &Client,
    tokens: &Tokens,
    cached_project_id: Option<&str>,
) -> AppResult<GeminiQuotaResult> {
    if tokens.access_token.trim().is_empty() {
        return Err(AppError::msg(
            "Google access token is missing. Sign in again.",
        ));
    }

    let discovery = discover_project_and_tier(client, tokens).await;
    if let Err(error) = &discovery
        && google_reauth_required(error)
    {
        return Err(AppError::msg(format!(
            "Google authorization is no longer valid: {}",
            error.user_message()
        )));
    }
    let (discovered_project, plan_type) = discovery.unwrap_or((None, None));
    let project_id = cached_project_id.map(str::to_string).or(discovered_project);
    let request_body = project_id
        .as_ref()
        .map_or_else(|| json!({}), |project| json!({ "project": project }));

    let summary_result = post_cloud_code(
        client,
        tokens,
        "/v1internal:retrieveUserQuotaSummary",
        &request_body,
        "Antigravity quota summary failed",
    )
    .await;
    if let Ok(body) = summary_result {
        let envelope: QuotaSummaryEnvelope =
            serde_json::from_slice(&body).map_err(|source| AppError::Json {
                context: "Invalid Antigravity quota summary",
                source,
            })?;
        if let Some(quota) = quota_from_summary(envelope, plan_type.clone()) {
            return Ok(GeminiQuotaResult { quota, project_id });
        }
    }

    let available_body = post_cloud_code(
        client,
        tokens,
        "/v1internal:fetchAvailableModels",
        &request_body,
        "Antigravity model quota failed",
    )
    .await?;
    let envelope: AvailableModelsEnvelope =
        serde_json::from_slice(&available_body).map_err(|source| AppError::Json {
            context: "Invalid Antigravity model quota response",
            source,
        })?;
    let quota = quota_from_available_models(envelope, plan_type).ok_or_else(|| {
        AppError::msg(
            "Antigravity did not expose quota values for this Google account. The account remains available for switching.",
        )
    })?;
    Ok(GeminiQuotaResult { quota, project_id })
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
    let snapshot = {
        let data = lock_data(state)?;
        data.accounts
            .iter()
            .find(|account| account.id == account_id && account.provider == AccountProvider::Gemini)
            .cloned()
            .ok_or_else(|| AppError::msg("Antigravity account not found"))?
    };

    let token_result = fresh_google_tokens(
        &state.http_client,
        &snapshot.tokens,
        snapshot.token_expires_at,
    )
    .await;
    let network_result = match token_result {
        Ok(mut token_set) => {
            let mut quota_result = fetch_gemini_quota(
                &state.http_client,
                &token_set.tokens,
                snapshot.provider_project_id.as_deref(),
            )
            .await;
            if let Err(AppError::RemoteHttp { status: 401, .. }) = &quota_result
                && let Ok(new_token_set) =
                    refresh_google_access_token(&state.http_client, &snapshot.tokens).await
            {
                token_set = new_token_set;
                quota_result = fetch_gemini_quota(
                    &state.http_client,
                    &token_set.tokens,
                    snapshot.provider_project_id.as_deref(),
                )
                .await;
            }
            Ok((token_set, quota_result))
        }
        Err(error) => Err(error),
    };

    let mut data = lock_data(state)?;
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
            match quota_result {
                Ok(result) => {
                    if result.project_id.is_some() {
                        account.provider_project_id = result.project_id;
                    }
                    if let Some(plan_type) = result.quota.plan_type.as_ref() {
                        account.subscription_plan = Some(plan_type.clone());
                        account.subscription_detected_at = Some(now_ts());
                    }
                    account.quota = Some(result.quota);
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
    let previous_external = if active && refreshed_tokens {
        crate::gemini::read_antigravity_auth()?
    } else {
        None
    };
    if active && refreshed_tokens {
        crate::gemini::write_antigravity_account_auth(account)?;
    }
    if let Err(error) = commit_app_data(&mut data, next) {
        if active
            && refreshed_tokens
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
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_summary_maps_real_five_hour_and_weekly_buckets() {
        let envelope: QuotaSummaryEnvelope = serde_json::from_value(json!({
            "groups": [{
                "displayName": "Gemini Models",
                "buckets": [
                    {"bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.25, "resetTime": "2030-01-01T00:00:00Z"},
                    {"bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.60, "resetTime": "2030-01-07T00:00:00Z"}
                ]
            }, {
                "displayName": "Claude and GPT models",
                "buckets": [
                    {"bucketId": "3p-5h", "window": "5h", "remaining": {"remainingFraction": 0.10}},
                    {"bucketId": "3p-weekly", "window": "weekly", "remainingFraction": 0.80}
                ]
            }]
        }))
        .expect("summary");
        let quota = quota_from_summary(envelope, Some("Google AI Pro".to_string())).expect("quota");

        assert_eq!(quota.primary.limit_window_seconds, Some(FIVE_HOURS_SECONDS));
        assert_eq!(quota.primary.used_percent, Some(90.0));
        assert_eq!(quota.secondary.limit_window_seconds, Some(WEEK_SECONDS));
        assert_eq!(quota.secondary.used_percent, Some(40.0));
        assert_eq!(quota.plan_type.as_deref(), Some("Google AI Pro"));
    }

    #[test]
    fn available_models_fallback_uses_strictest_real_fraction() {
        let envelope: AvailableModelsEnvelope = serde_json::from_value(json!({
            "models": {
                "gemini-3-pro": {"quotaInfo": {"remainingFraction": 0.7}},
                "claude-sonnet": {"quotaInfo": {"remainingFraction": 0.2}}
            }
        }))
        .expect("models");
        let quota = quota_from_available_models(envelope, None).expect("quota");
        assert_eq!(quota.primary.used_percent, Some(80.0));
        assert_eq!(quota.primary.limit_window_seconds, None);
    }

    #[test]
    fn quota_summary_separates_multiday_models_from_5h_session_window() {
        let now = now_ts();
        let flash_reset = chrono::DateTime::from_timestamp(now + 3600, 0)
            .unwrap()
            .to_rfc3339();
        let claude_reset = chrono::DateTime::from_timestamp(now + 5 * 24 * 3600, 0)
            .unwrap()
            .to_rfc3339();

        let envelope: QuotaSummaryEnvelope = serde_json::from_value(json!({
            "groups": [{
                "displayName": "Session Quotas",
                "buckets": [
                    {"bucketId": "gemini-flash", "remainingFraction": 1.0, "resetTime": flash_reset},
                    {"bucketId": "claude-3-7-sonnet", "remainingFraction": 0.58, "resetTime": claude_reset}
                ]
            }]
        }))
        .expect("summary");
        let quota = quota_from_summary(envelope, Some("Google AI Pro".to_string())).expect("quota");

        // The 5-day Claude reset must NOT become the primary 5-hour window
        assert_eq!(quota.secondary.limit_window_seconds, Some(WEEK_SECONDS));
        assert!((quota.secondary.used_percent.unwrap() - 42.0).abs() < 0.1);
        assert_eq!(quota.primary.limit_window_seconds, Some(FIVE_HOURS_SECONDS));
        assert_eq!(quota.primary.used_percent, Some(0.0));
    }
}
