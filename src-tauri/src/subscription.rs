use chrono::{DateTime, NaiveDate};
use serde_json::Value;

use crate::errors::{AppError, AppResult};
use crate::models::{Account, TokenHealth, Tokens, now_ts};
use crate::quota::{
    MAX_SUCCESS_BODY_BYTES, access_token_expires_soon, classify_refresh_error, is_quota_auth_error,
    read_bounded_body, read_bounded_error_body, refresh_access_token, sanitize_remote_message,
};

const PLAN_CODES: &[&str] = &[
    "subscription_plan",
    "subscriptionPlan",
    "plan_type",
    "planType",
    "sku",
];

const PLAN_NAMES: &[&str] = &[
    "plan",
    "plan_name",
    "planName",
    "plan_display_name",
    "product_name",
    "productName",
    "billing_plan",
    "billingPlan",
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

    // Try the remembered endpoint first, then each fallback once. Authentication,
    // rate limits and network failures stop the chain instead of multiplying requests.
    for endpoint in &endpoints {
        match fetch_subscription_endpoint(client, endpoint, tokens, account_id).await {
            Ok(info) => return Ok(info),
            Err(SubscriptionEndpointError::Fatal(error)) => return Err(error),
            Err(SubscriptionEndpointError::Unsupported(message)) => {
                log::debug!("Skipping subscription endpoint: {message}");
            }
        }
    }
    Err(AppError::msg(
        "Subscription auto-detect is unavailable for this account. Manual date was kept.",
    ))
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
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/159.0.0.0 Safari/537.36",
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
    let info = parse_subscription_payload(&payload, account_id);

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

fn parse_subscription_payload(payload: &Value, account_id: Option<&str>) -> SubscriptionInfo {
    let (plan, expires_at) =
        if let Some(accounts) = payload.get("accounts").and_then(Value::as_object) {
            let default = accounts.get("default");
            let entry = match account_id {
                Some(id) => accounts.get(id).or_else(|| {
                    default.filter(|entry| {
                        entry.pointer("/account/account_id").and_then(Value::as_str) == Some(id)
                    })
                }),
                None => default,
            };
            let account = entry.and_then(|entry| entry.get("account"));
            let entitlement = entry
                .and_then(|entry| entry.get("entitlement"))
                .filter(|value| {
                    value
                        .get("has_active_subscription")
                        .and_then(Value::as_bool)
                        != Some(false)
                });
            (
                entitlement
                    .and_then(direct_plan)
                    .or_else(|| account.and_then(direct_plan)),
                entitlement.and_then(direct_expiration),
            )
        } else {
            let subscription = payload.get("subscription");
            (
                subscription
                    .and_then(find_plan)
                    .or_else(|| find_plan(payload)),
                subscription
                    .and_then(find_expiration)
                    .or_else(|| find_expiration(payload)),
            )
        };
    SubscriptionInfo {
        plan,
        expires_at,
        fetched_at: now_ts(),
        endpoint_hint: None,
    }
}

fn direct_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let map = value.as_object()?;
    map.get(key).or_else(|| {
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    })
}

fn direct_plan(value: &Value) -> Option<String> {
    let text = |key: &&str| {
        direct_field(value, key)?
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let mut codes = PLAN_CODES.iter().filter_map(text);
    // Prefer a qualified product SKU when the same record also has a generic Pro code.
    let tier = codes
        .clone()
        .filter(|raw| !raw.eq_ignore_ascii_case("pro"))
        .find_map(pro_tier);
    tier.map(str::to_owned)
        .or_else(|| codes.next().map(plan_code_label))
        .or_else(|| {
            PLAN_NAMES
                .iter()
                .find_map(text)
                .map(|s| normalize_plan_label(s.to_owned()))
        })
}

fn find_plan(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => direct_plan(value).or_else(|| {
            map.iter()
                .filter(|(key, _)| !unrelated_subscription_field(key))
                .find_map(|(_, value)| find_plan(value))
        }),
        Value::Array(items) => items.iter().find_map(find_plan),
        _ => None,
    }
}

fn unrelated_subscription_field(key: &str) -> bool {
    matches!(
        key,
        "scheduled_plan_change"
            | "eligible_offers"
            | "last_active_subscription"
            | "eligible_promo_campaigns"
    )
}

fn direct_expiration(value: &Value) -> Option<i64> {
    EXPIRES_KEYS
        .iter()
        .find_map(|key| direct_field(value, key).and_then(value_to_timestamp))
}

fn find_expiration(value: &Value) -> Option<i64> {
    match value {
        Value::Object(map) => direct_expiration(value).or_else(|| {
            map.iter()
                .filter(|(key, _)| !unrelated_subscription_field(key))
                .find_map(|(_, field)| find_expiration(field))
        }),
        Value::Array(items) => items.iter().find_map(find_expiration),
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

fn pro_tier(raw: &str) -> Option<&'static str> {
    let compact = raw
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-', ' ', '$'], "");
    let product = compact
        .strip_prefix("chatgpt")
        .or_else(|| compact.strip_prefix("codex"))
        .unwrap_or(&compact);
    match product.strip_suffix("plan").unwrap_or(product) {
        "prolite" | "pro100" | "pro5x" | "prox5" => Some("Pro 100"),
        "pro" | "pro200" | "pro20x" | "prox20" => Some("Pro 200"),
        "promax" | "pro500" => Some("Pro 500"),
        _ => None,
    }
}

pub(crate) fn plan_code_label(raw: &str) -> String {
    pro_tier(raw)
        .map(str::to_owned)
        .unwrap_or_else(|| normalize_plan_label(raw.to_owned()))
}

// Older saves collapsed all Pro tiers into the display name "Pro".
// Only refine that label if the saved usage response still has a Pro plan code.
pub(crate) fn saved_plan_label(plan: Option<&str>, usage_code: Option<&str>) -> Option<String> {
    match plan {
        Some(raw) if raw.eq_ignore_ascii_case("pro") => {
            Some(usage_code.and_then(pro_tier).unwrap_or("Pro").into())
        }
        Some(raw) => Some(pro_tier(raw).unwrap_or(raw).to_owned()),
        None => usage_code.map(plan_code_label),
    }
}

fn normalize_plan_label(raw: String) -> String {
    let lower = raw.to_ascii_lowercase();
    if !raw.trim().eq_ignore_ascii_case("pro")
        && let Some(tier) = pro_tier(&raw)
    {
        return tier.to_owned();
    }

    if lower.contains("enterprise") {
        return "Enterprise".to_string();
    }
    if lower.contains("business") {
        return "Business".to_string();
    }
    if lower.contains("team") {
        return "Team".to_string();
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

    #[test]
    fn subscription_stops_after_success_and_does_not_repeat_failed_preferred_endpoint() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        use std::time::{Duration, Instant};
        for first_status in [200, 404, 429] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let stop_server = Arc::clone(&stop);
            let server = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut requests = Vec::new();
                while !stop_server.load(Ordering::SeqCst) && Instant::now() < deadline {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // Accepted sockets can inherit nonblocking mode on Windows.
                            stream.set_nonblocking(false).unwrap();
                            stream
                                .set_read_timeout(Some(Duration::from_secs(1)))
                                .unwrap();
                            let mut buffer = [0; 4096];
                            let size = stream.read(&mut buffer).unwrap();
                            let request = String::from_utf8_lossy(&buffer[..size]);
                            requests.push(request.lines().next().unwrap().to_string());
                            let status = if requests.len() == 1 {
                                first_status
                            } else {
                                200
                            };
                            let body = if status == 200 {
                                r#"{"plan_type":"pro"}"#
                            } else {
                                "{}"
                            };
                            write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nContent-Type: application/json\r\nRetry-After: 300\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        Err(error) => panic!("mock listener: {error}"),
                    }
                }
                requests
            });
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap();
            let tokens = crate::models::Tokens {
                access_token: "test".into(),
                refresh_token: String::new(),
                id_token: String::new(),
            };
            let result = tauri::async_runtime::block_on(super::fetch_subscription_info(
                &client,
                &format!("http://{address}"),
                &tokens,
                Some("test-account"),
                Some("account-payments"),
            ));
            stop.store(true, Ordering::SeqCst);
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), if first_status == 404 { 2 } else { 1 });
            assert_eq!(
                requests
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                requests.len()
            );
            if first_status == 429 {
                assert_eq!(result.unwrap_err().retry_after_seconds(), Some(300));
            } else {
                assert_eq!(result.unwrap().plan.as_deref(), Some("Pro 200"));
            }
        }
    }

    #[test]
    fn old_pro_labels_only_gain_a_tier_from_identifiable_saved_data() {
        for (plan, usage, expected) in [
            (Some("Pro"), Some("promax"), Some("Pro 500")),
            (Some("Pro"), Some("pro"), Some("Pro 200")),
            (Some("Pro"), None, Some("Pro")),
            (Some("Pro"), Some("free"), Some("Pro")),
            (Some("Pro x5"), Some("pro"), Some("Pro 100")),
            (None, Some("prolite"), Some("Pro 100")),
            (Some("Free"), Some("pro"), Some("Free")),
            (None, None, None),
        ] {
            assert_eq!(super::saved_plan_label(plan, usage).as_deref(), expected);
        }
    }

    #[test]
    fn reads_the_active_entitlement_of_the_requested_workspace() {
        let payload = json!({"accounts": {
            "default": {"account": {"account_id": "another", "plan_type": "prolite"}},
            "selected": {
                "account": {"plan_type": "pro", "plan_display_name": "Pro"},
                "entitlement": {"has_active_subscription": true, "subscription_plan": "chatgptpromax",
                    "expires_at": 1900000000, "scheduled_plan_change": {"plan_type": "prolite", "expires_at": 1800000000}}
            }
        }});
        let info = parse_subscription_payload(&payload, Some("selected"));
        assert_eq!(info.plan.as_deref(), Some("Pro 500"));
        assert_eq!(info.expires_at, Some(1900000000));
        assert_eq!(
            parse_subscription_payload(&payload, Some("missing")).plan,
            None
        );
        assert_eq!(
            parse_subscription_payload(&payload, Some("another"))
                .plan
                .as_deref(),
            Some("Pro 100")
        );
        let inactive = json!({"accounts": {"selected": {
            "account": {"plan_type": "free"},
            "entitlement": {"has_active_subscription": false, "subscription_plan": "chatgptpro", "expires_at": 1900000000}
        }}});
        let info = parse_subscription_payload(&inactive, Some("selected"));
        assert_eq!(info.plan.as_deref(), Some("Free"));
        assert_eq!(info.expires_at, None);
    }

    #[test]
    fn recognizes_pro_codes_and_active_skus_without_inferring_from_prices_or_offers() {
        for (code, label) in [
            ("prolite", "Pro 100"),
            ("pro", "Pro 200"),
            ("promax", "Pro 500"),
            ("chatgptprolite", "Pro 100"),
            ("chatgptpro", "Pro 200"),
            ("chatgptpromax", "Pro 500"),
            ("pro_100", "Pro 100"),
            ("Pro 200", "Pro 200"),
            ("Pro 500", "Pro 500"),
            ("self_serve_business_prolite", "Business"),
        ] {
            assert_eq!(super::plan_code_label(code), label);
            assert_eq!(
                parse_subscription_payload(&json!({"plan_type": code}), None)
                    .plan
                    .as_deref(),
                Some(label)
            );
        }
        let info = parse_subscription_payload(
            &json!({"subscription": {"plan_type": "pro", "sku": "chatgptpromax"}}),
            None,
        );
        assert_eq!(info.plan.as_deref(), Some("Pro 500"));
        let info = parse_subscription_payload(
            &json!({"plan_name": "Pro", "amount": 500, "eligible_offers": [{"sku": "chatgptpromax"}]}),
            None,
        );
        assert_eq!(info.plan.as_deref(), Some("Pro"));
    }

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

        let info = parse_subscription_payload(&payload, None);

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
    fn translates_legacy_pro_product_names() {
        assert_eq!(
            normalize_plan_label("chatgpt_pro_5x_plan".into()),
            "Pro 100"
        );
        assert_eq!(
            normalize_plan_label("chatgpt_pro_x20_plan".into()),
            "Pro 200"
        );
        assert_eq!(normalize_plan_label("pro".into()), "Pro");
    }

    #[test]
    fn subscription_schedule_uses_expiry_and_long_lived_caches() {
        let now = 1_000_000;
        let mut future = parse_subscription_payload(
            &json!({
                "plan": "Plus",
                "expires_at": now + 5_000
            }),
            None,
        );
        future.fetched_at = now;
        assert_eq!(next_subscription_check(now, None, &Ok(future)), now + 5_000);

        let plan_only = parse_subscription_payload(&json!({ "plan": "Free" }), None);
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
        let plan_only = parse_subscription_payload(&json!({ "plan": "Plus" }), None);

        assert_eq!(
            next_subscription_check(now, Some(now - 1), &Ok(plan_only)),
            now + SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS
        );
    }
}
