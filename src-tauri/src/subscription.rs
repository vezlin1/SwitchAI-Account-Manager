use chrono::{DateTime, NaiveDate};
use serde_json::Value;

use crate::errors::{AppError, AppResult};
use crate::models::{Account, TokenHealth, Tokens, now_ts};
use crate::quota::{
    MAX_SUCCESS_BODY_BYTES, access_token_expires_soon, classify_refresh_error, is_quota_auth_error,
    read_bounded_body, read_bounded_error_body, refresh_access_token, sanitize_remote_message,
};

const PLAN_KEYS: &[&str] = &[
    "plan_type",
    "planType",
    "plan",
    "plan_name",
    "planName",
    "subscription_plan",
    "subscriptionPlan",
    "product_name",
    "productName",
    "billing_plan",
    "billingPlan",
    "sku",
];

const EXPIRES_KEYS: &[&str] = &[
    "expires_at",
    "expiresAt",
    "expiration",
    "expiration_at",
    "expirationAt",
    "current_period_end",
    "currentPeriodEnd",
    "current_period_ends_at",
    "currentPeriodEndsAt",
    "period_end",
    "periodEnd",
    "renewal_date",
    "renewalDate",
    "next_invoice_date",
    "nextInvoiceDate",
    "next_billing_date",
    "nextBillingDate",
    "valid_until",
    "validUntil",
    "valid_to",
    "validTo",
    "end_date",
    "endDate",
];

#[derive(Debug, Clone)]
pub struct SubscriptionInfo {
    pub plan: Option<String>,
    pub expires_at: Option<i64>,
    pub fetched_at: i64,
    pub endpoint_hint: Option<String>,
}

pub const SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS: i64 = 24 * 60 * 60;
pub const SUBSCRIPTION_PLAN_ONLY_RECHECK_SECONDS: i64 = 7 * 24 * 60 * 60;
pub const SUBSCRIPTION_UNSUPPORTED_RECHECK_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone)]
struct SubscriptionEndpoint {
    hint: &'static str,
    url: String,
}

enum SubscriptionEndpointError {
    Unsupported(String),
    Fatal(AppError),
}

pub struct SubscriptionRefreshUpdate {
    pub subscription_result: AppResult<SubscriptionInfo>,
    pub refreshed_tokens: Option<Tokens>,
    pub token_health: Option<TokenHealth>,
}

pub async fn fetch_subscription_info(
    client: &reqwest::Client,
    base_url: &str,
    tokens: &Tokens,
    account_id: Option<&str>,
    preferred_endpoint: Option<&str>,
) -> AppResult<SubscriptionInfo> {
    if tokens.access_token.trim().is_empty() {
        return Err(AppError::msg("Missing access_token"));
    }

    let endpoints = subscription_endpoints(base_url, account_id, preferred_endpoint);
    if endpoints.is_empty() {
        return Err(AppError::msg("No subscription endpoints available"));
    }

    // If a preferred endpoint was provided, try it first
    if preferred_endpoint.is_some() {
        match fetch_subscription_endpoint(client, &endpoints[0], tokens, account_id).await {
            Ok(info) => return Ok(info),
            Err(SubscriptionEndpointError::Fatal(error)) => return Err(error),
            Err(SubscriptionEndpointError::Unsupported(_)) => {}
        }
    }

    // Probe candidates concurrently for fast auto-detection
    let mut tasks = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        let client = client.clone();
        let tokens = tokens.clone();
        let account_id = account_id.map(ToOwned::to_owned);
        tasks.push(tauri::async_runtime::spawn(async move {
            let res =
                fetch_subscription_endpoint(&client, &endpoint, &tokens, account_id.as_deref())
                    .await;
            (endpoint.hint, res)
        }));
    }

    let mut errors = Vec::new();
    let mut fatal_error = None;
    for task in tasks {
        match task.await {
            Ok((_hint, Ok(info))) => return Ok(info),
            Ok((_hint, Err(SubscriptionEndpointError::Fatal(error)))) => {
                if fatal_error.is_none() {
                    fatal_error = Some(error);
                }
            }
            Ok((_hint, Err(SubscriptionEndpointError::Unsupported(error)))) => {
                errors.push(error);
            }
            Err(e) => errors.push(e.to_string()),
        }
    }

    if let Some(error) = fatal_error {
        return Err(error);
    }

    if !errors.is_empty() {
        return Err(AppError::msg(
            "Subscription auto-detect is unavailable for this account. Manual date was kept.",
        ));
    }

    Err(AppError::msg("Subscription details not found"))
}

pub fn next_subscription_check(
    now: i64,
    previous_expires_at: Option<i64>,
    result: &AppResult<SubscriptionInfo>,
) -> i64 {
    match result {
        Ok(info) => match info.expires_at {
            Some(expires_at) if expires_at > now => expires_at,
            Some(_) => now.saturating_add(SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS),
            None if previous_expires_at.is_some_and(|expires_at| expires_at <= now) => {
                now.saturating_add(SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS)
            }
            None => now.saturating_add(SUBSCRIPTION_PLAN_ONLY_RECHECK_SECONDS),
        },
        Err(error) if error.retry_after_seconds().is_some() => now.saturating_add(
            error
                .retry_after_seconds()
                .unwrap_or(SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS)
                .clamp(60, SUBSCRIPTION_UNSUPPORTED_RECHECK_SECONDS),
        ),
        Err(error) if is_unsupported_subscription_error(&error.user_message()) => {
            now.saturating_add(SUBSCRIPTION_UNSUPPORTED_RECHECK_SECONDS)
        }
        Err(_) => now.saturating_add(SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS),
    }
}

fn is_unsupported_subscription_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("subscription auto-detect is unavailable")
}

pub async fn refresh_subscription_with_token_refresh(
    client: &reqwest::Client,
    base_url: &str,
    account: &Account,
) -> SubscriptionRefreshUpdate {
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
                return SubscriptionRefreshUpdate {
                    subscription_result: Err(AppError::msg(format!(
                        "Token refresh failed before subscription request: {message}"
                    ))),
                    refreshed_tokens: None,
                    token_health: Some(classify_refresh_error(&message)),
                };
            }
        }
    }

    let mut subscription_result = fetch_subscription_info(
        client,
        base_url,
        &current_tokens,
        account.account_id.as_deref(),
        account.subscription_endpoint_hint.as_deref(),
    )
    .await;

    if let Err(first_err) = &subscription_result
        && is_quota_auth_error(&first_err.user_message())
    {
        match refresh_access_token(client, &current_tokens).await {
            Ok(new_tokens) => {
                subscription_result = fetch_subscription_info(
                    client,
                    base_url,
                    &new_tokens,
                    account.account_id.as_deref(),
                    account.subscription_endpoint_hint.as_deref(),
                )
                .await;
                refreshed_tokens = Some(new_tokens);
                token_health = Some(TokenHealth::refreshed());
            }
            Err(refresh_err) => {
                let message = refresh_err.user_message();
                subscription_result = Err(AppError::msg(format!(
                    "Subscription request failed due to auth and token refresh failed: {message}"
                )));
                token_health = Some(classify_refresh_error(&message));
            }
        }
    }

    if subscription_result.is_ok() && token_health.is_none() {
        token_health = Some(TokenHealth::healthy());
    }

    SubscriptionRefreshUpdate {
        subscription_result,
        refreshed_tokens,
        token_health,
    }
}

pub async fn refresh_subscription_without_token_refresh(
    client: &reqwest::Client,
    base_url: &str,
    account: &Account,
) -> SubscriptionRefreshUpdate {
    let subscription_result = fetch_subscription_info(
        client,
        base_url,
        &account.tokens,
        account.account_id.as_deref(),
        account.subscription_endpoint_hint.as_deref(),
    )
    .await;
    let token_health = subscription_result.is_ok().then(TokenHealth::healthy);

    SubscriptionRefreshUpdate {
        subscription_result,
        refreshed_tokens: None,
        token_health,
    }
}

fn subscription_endpoints(
    base_url: &str,
    account_id: Option<&str>,
    preferred_endpoint: Option<&str>,
) -> Vec<SubscriptionEndpoint> {
    let base = base_url.trim_end_matches('/');
    let mut endpoints = Vec::new();

    if let Some(account_id) = account_id.filter(|value| !value.trim().is_empty()) {
        let encoded =
            url::form_urlencoded::byte_serialize(account_id.as_bytes()).collect::<String>();
        endpoints.push(SubscriptionEndpoint {
            hint: "account-payments",
            url: format!("{base}/payments/subscription?account_id={encoded}"),
        });
        endpoints.push(SubscriptionEndpoint {
            hint: "account-subscription",
            url: format!("{base}/accounts/{encoded}/subscription"),
        });
    }

    endpoints.push(SubscriptionEndpoint {
        hint: "payments",
        url: format!("{base}/payments/subscription"),
    });
    endpoints.push(SubscriptionEndpoint {
        hint: "accounts-check",
        url: format!("{base}/accounts/check/v4-2023-04-27"),
    });

    if let Some(preferred_endpoint) = preferred_endpoint
        && let Some(index) = endpoints
            .iter()
            .position(|endpoint| endpoint.hint == preferred_endpoint)
    {
        endpoints.rotate_left(index);
    }
    endpoints
}

async fn fetch_subscription_endpoint(
    client: &reqwest::Client,
    endpoint: &SubscriptionEndpoint,
    tokens: &Tokens,
    account_id: Option<&str>,
) -> Result<SubscriptionInfo, SubscriptionEndpointError> {
    let mut request = client
        .get(&endpoint.url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {}", tokens.access_token))
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.7977.65 Safari/537.36",
        );

    if let Some(account_id) = account_id
        && !account_id.trim().is_empty()
    {
        request = request.header("ChatGPT-Account-Id", account_id);
    }

    let response = request.send().await.map_err(|source| {
        SubscriptionEndpointError::Fatal(AppError::Http {
            context: "Subscription request failed",
            source,
        })
    })?;
    let status = response.status();
    let retry_after_seconds = retry_after_seconds(response.headers());
    let body = if status.is_success() {
        read_bounded_body(response, "Subscription response", MAX_SUCCESS_BODY_BYTES)
            .await
            .map_err(SubscriptionEndpointError::Fatal)?
    } else {
        read_bounded_error_body(response, "Subscription response")
            .await
            .map_err(SubscriptionEndpointError::Fatal)?
    };
    let body = String::from_utf8_lossy(&body).into_owned();

    if !status.is_success() {
        let details = sanitize_remote_message(&body, 180);
        if matches!(status.as_u16(), 403 | 404) {
            return Err(SubscriptionEndpointError::Unsupported(format!(
                "{} returned {status}",
                endpoint.hint
            )));
        }
        return Err(SubscriptionEndpointError::Fatal(AppError::RemoteHttp {
            context: "Subscription request failed",
            status: status.as_u16(),
            retry_after_seconds,
            details,
        }));
    }

    let payload: Value = serde_json::from_str(&body).map_err(|source| {
        SubscriptionEndpointError::Fatal(AppError::Json {
            context: "Invalid subscription payload",
            source,
        })
    })?;
    let info = parse_subscription_payload(&payload);

    if info.plan.is_none() && info.expires_at.is_none() {
        return Err(SubscriptionEndpointError::Unsupported(format!(
            "{} payload did not include plan or expiration fields",
            endpoint.hint
        )));
    }

    Ok(SubscriptionInfo {
        endpoint_hint: Some(endpoint.hint.to_string()),
        ..info
    })
}

fn retry_after_seconds(headers: &reqwest::header::HeaderMap) -> Option<i64> {
    let value = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if let Ok(seconds) = value.parse::<i64>() {
        return Some(seconds.max(0));
    }
    DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|date| date.timestamp().saturating_sub(now_ts()).max(0))
}

fn parse_subscription_payload(payload: &Value) -> SubscriptionInfo {
    SubscriptionInfo {
        plan: find_string_by_keys(payload, PLAN_KEYS).map(normalize_plan_label),
        expires_at: find_timestamp_by_keys(payload, EXPIRES_KEYS),
        fetched_at: now_ts(),
        endpoint_hint: None,
    }
}

fn find_string_by_keys(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (key, field) in map {
                if keys
                    .iter()
                    .any(|candidate| key.eq_ignore_ascii_case(candidate))
                    && let Some(text) = field.as_str().map(str::trim)
                    && !text.is_empty()
                {
                    return Some(text.to_string());
                }
            }

            map.values()
                .find_map(|field| find_string_by_keys(field, keys))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_string_by_keys(item, keys)),
        _ => None,
    }
}

fn find_timestamp_by_keys(value: &Value, keys: &[&str]) -> Option<i64> {
    match value {
        Value::Object(map) => {
            for (key, field) in map {
                if keys
                    .iter()
                    .any(|candidate| key.eq_ignore_ascii_case(candidate))
                    && let Some(timestamp) = value_to_timestamp(field)
                {
                    return Some(timestamp);
                }
            }

            map.values()
                .find_map(|field| find_timestamp_by_keys(field, keys))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_timestamp_by_keys(item, keys)),
        _ => None,
    }
}

fn value_to_timestamp(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().and_then(normalize_timestamp),
        Value::String(text) => string_to_timestamp(text),
        _ => None,
    }
}

fn string_to_timestamp(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = trimmed.parse::<i64>() {
        return normalize_timestamp(value);
    }

    if let Ok(value) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(value.timestamp());
    }

    if let Ok(value) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return value
            .and_hms_opt(23, 59, 59)
            .map(|date_time| date_time.and_utc().timestamp());
    }

    None
}

fn normalize_timestamp(value: i64) -> Option<i64> {
    if value <= 0 {
        return None;
    }

    Some(if value > 10_000_000_000 {
        value / 1_000
    } else {
        value
    })
}

fn normalize_plan_label(raw: String) -> String {
    let lower = raw.to_ascii_lowercase();
    let compact = lower.replace(['_', '-', ' '], "");

    if lower.contains("enterprise") {
        return "Enterprise".to_string();
    }
    if lower.contains("business") {
        return "Business".to_string();
    }
    if lower.contains("team") {
        return "Team".to_string();
    }
    if lower.contains("pro") {
        if compact.contains("x5") || compact.contains("5x") {
            return "Pro x5".to_string();
        }
        if compact.contains("x20") || compact.contains("20x") {
            return "Pro x20".to_string();
        }
        return "Pro x20".to_string();
    }
    if lower.contains("plus") {
        return "Plus".to_string();
    }
    if lower.contains("free") {
        return "Free".to_string();
    }

    raw.replace(['_', '-'], " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::errors::AppError;

    use super::{
        SUBSCRIPTION_PLAN_ONLY_RECHECK_SECONDS, SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS,
        SUBSCRIPTION_UNSUPPORTED_RECHECK_SECONDS, next_subscription_check, normalize_plan_label,
        parse_subscription_payload, string_to_timestamp,
    };

    #[test]
    fn parses_nested_plan_and_period_end() {
        let payload = json!({
            "account": {
                "subscription": {
                    "plan_type": "chatgpt_plus_plan",
                    "current_period_end": 1782863999000i64
                }
            }
        });

        let info = parse_subscription_payload(&payload);

        assert_eq!(info.plan.as_deref(), Some("Plus"));
        assert_eq!(info.expires_at, Some(1_782_863_999));
    }

    #[test]
    fn parses_iso_date_strings() {
        assert_eq!(
            string_to_timestamp("2026-06-30T21:00:00Z"),
            Some(1_782_853_200)
        );
        assert_eq!(string_to_timestamp("2026-06-30"), Some(1_782_863_999));
    }

    #[test]
    fn keeps_pro_multipliers_for_ui_filtering() {
        assert_eq!(normalize_plan_label("chatgpt_pro_5x_plan".into()), "Pro x5");
        assert_eq!(
            normalize_plan_label("chatgpt_pro_x20_plan".into()),
            "Pro x20"
        );
        assert_eq!(normalize_plan_label("pro".into()), "Pro x20");
    }

    #[test]
    fn subscription_schedule_uses_expiry_and_long_lived_caches() {
        let now = 1_000_000;
        let mut future = parse_subscription_payload(&json!({
            "plan": "Plus",
            "expires_at": now + 5_000
        }));
        future.fetched_at = now;
        assert_eq!(next_subscription_check(now, None, &Ok(future)), now + 5_000);

        let plan_only = parse_subscription_payload(&json!({ "plan": "Free" }));
        assert_eq!(
            next_subscription_check(now, None, &Ok(plan_only)),
            now + SUBSCRIPTION_PLAN_ONLY_RECHECK_SECONDS
        );

        let unsupported = Err(AppError::msg(
            "Subscription auto-detect is unavailable (403 Forbidden)",
        ));
        assert_eq!(
            next_subscription_check(now, None, &unsupported),
            now + SUBSCRIPTION_UNSUPPORTED_RECHECK_SECONDS
        );

        let transient = Err(AppError::msg("Subscription request timed out"));
        assert_eq!(
            next_subscription_check(now, None, &transient),
            now + SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS
        );

        let rate_limited = Err(AppError::RemoteHttp {
            context: "Subscription request failed",
            status: 429,
            retry_after_seconds: Some(600),
            details: "rate limited".to_string(),
        });
        assert_eq!(next_subscription_check(now, None, &rate_limited), now + 600);

        let wrapped_server_error = Err(AppError::msg(
            "Subscription details not found. Tried ChatGPT backend endpoints: Subscription request failed (500 Internal Server Error)",
        ));
        assert_eq!(
            next_subscription_check(now, None, &wrapped_server_error),
            now + SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS
        );
    }

    #[test]
    fn expired_known_subscription_keeps_daily_recheck_when_payload_has_no_date() {
        let now = 1_000_000;
        let plan_only = parse_subscription_payload(&json!({ "plan": "Plus" }));

        assert_eq!(
            next_subscription_check(now, Some(now - 1), &Ok(plan_only)),
            now + SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS
        );
    }
}
