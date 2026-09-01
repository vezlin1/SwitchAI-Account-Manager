use serde::{Deserialize, Serialize};

use crate::models::{
    APP_SCHEMA_VERSION, Account, AppData, AppSettings, QuotaInfo, QuotaWindow, TokenHealth,
    TokenHealthStatus, Tokens,
};

fn default_schema_version() -> u32 {
    APP_SCHEMA_VERSION
}

fn default_limits_base_url() -> String {
    "https://chatgpt.com/backend-api".to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PersistedAccountProvider {
    #[default]
    Codex,
    Gemini,
}

impl From<crate::models::AccountProvider> for PersistedAccountProvider {
    fn from(provider: crate::models::AccountProvider) -> Self {
        match provider {
            crate::models::AccountProvider::Codex => Self::Codex,
            crate::models::AccountProvider::Gemini => Self::Gemini,
        }
    }
}

impl From<PersistedAccountProvider> for crate::models::AccountProvider {
    fn from(provider: PersistedAccountProvider) -> Self {
        match provider {
            PersistedAccountProvider::Codex => Self::Codex,
            PersistedAccountProvider::Gemini => Self::Gemini,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedAppData {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub accounts: Vec<PersistedAccount>,
    #[serde(default)]
    pub active_account_id: Option<String>,
    #[serde(default)]
    pub active_gemini_account_id: Option<String>,
    #[serde(default = "default_limits_base_url")]
    pub limits_base_url: String,
    #[serde(default)]
    app_settings: PersistedAppSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedAccount {
    pub id: String,
    #[serde(default)]
    pub provider: PersistedAccountProvider,
    pub email: Option<String>,
    pub account_id: Option<String>,
    #[serde(default)]
    pub provider_project_id: Option<String>,
    pub subscription_expires_at: Option<i64>,
    #[serde(default)]
    pub subscription_plan: Option<String>,
    #[serde(default)]
    pub subscription_detected_at: Option<i64>,
    #[serde(default)]
    pub subscription_checked_at: Option<i64>,
    #[serde(default)]
    pub subscription_next_check_at: Option<i64>,
    #[serde(default)]
    pub subscription_endpoint_hint: Option<String>,
    // Legacy state files may still carry plaintext tokens. They are accepted only
    // for migration and are never written back to state.json.
    #[serde(default, skip_serializing)]
    tokens: PersistedTokens,
    #[serde(default)]
    pub token_expires_at: Option<i64>,
    #[serde(default)]
    pub tokens_updated_at: Option<i64>,
    #[serde(default)]
    token_health: PersistedTokenHealth,
    quota: Option<PersistedQuotaInfo>,
    #[serde(default)]
    pub quota_next_refresh_at: Option<i64>,
    #[serde(default)]
    pub quota_refresh_failures: u32,
    pub created_at: i64,
    pub last_login_at: i64,
    #[serde(default, alias = "lastError")]
    pub quota_error: Option<String>,
    #[serde(default)]
    pub subscription_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedTokens {
    id_token: String,
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedQuotaWindow {
    used_percent: Option<f64>,
    limit_window_seconds: Option<i64>,
    reset_at: Option<i64>,
    fetched_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedQuotaInfo {
    plan_type: Option<String>,
    primary: PersistedQuotaWindow,
    secondary: PersistedQuotaWindow,
    fetched_at: i64,
}

fn default_skip_unsupported_region_refresh() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedAppSettings {
    auto_refresh_enabled: bool,
    auto_refresh_interval_minutes: u64,
    close_to_tray: bool,
    #[serde(default = "default_skip_unsupported_region_refresh")]
    skip_unsupported_region_refresh: bool,
    #[serde(default)]
    hidden_subscription_categories: Vec<String>,
    #[serde(default)]
    hidden_account_ids: Vec<String>,
    #[serde(default)]
    last_active_provider: Option<String>,
    #[serde(default = "default_gemini_switch_targets")]
    gemini_switch_targets: Vec<String>,
}

fn default_gemini_switch_targets() -> Vec<String> {
    let detected = crate::gemini::detect_installed_antigravity_surfaces();
    if detected.is_empty() {
        vec!["antigravity".to_string()]
    } else {
        detected
    }
}

impl Default for PersistedAppSettings {
    fn default() -> Self {
        Self::from(&AppSettings::default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum PersistedTokenHealthStatus {
    #[default]
    Unknown,
    Healthy,
    Refreshed,
    NeedsRelogin,
    NetworkError,
    ServerError,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedTokenHealth {
    status: PersistedTokenHealthStatus,
    last_checked_at: Option<i64>,
    last_refreshed_at: Option<i64>,
    last_error: Option<String>,
}

impl From<&Tokens> for PersistedTokens {
    fn from(tokens: &Tokens) -> Self {
        Self {
            id_token: tokens.id_token.clone(),
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
        }
    }
}

impl From<PersistedTokens> for Tokens {
    fn from(tokens: PersistedTokens) -> Self {
        Self {
            id_token: tokens.id_token,
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
        }
    }
}

impl From<&QuotaWindow> for PersistedQuotaWindow {
    fn from(window: &QuotaWindow) -> Self {
        Self {
            used_percent: window.used_percent,
            limit_window_seconds: window.limit_window_seconds,
            reset_at: window.reset_at,
            fetched_at: window.fetched_at,
        }
    }
}

impl From<PersistedQuotaWindow> for QuotaWindow {
    fn from(window: PersistedQuotaWindow) -> Self {
        Self {
            used_percent: window.used_percent,
            limit_window_seconds: window.limit_window_seconds,
            reset_at: window.reset_at,
            fetched_at: window.fetched_at,
        }
    }
}

impl From<&QuotaInfo> for PersistedQuotaInfo {
    fn from(quota: &QuotaInfo) -> Self {
        Self {
            plan_type: quota.plan_type.clone(),
            primary: PersistedQuotaWindow::from(&quota.primary),
            secondary: PersistedQuotaWindow::from(&quota.secondary),
            fetched_at: quota.fetched_at,
        }
    }
}

impl From<PersistedQuotaInfo> for QuotaInfo {
    fn from(quota: PersistedQuotaInfo) -> Self {
        Self {
            plan_type: quota.plan_type,
            primary: QuotaWindow::from(quota.primary),
            secondary: QuotaWindow::from(quota.secondary),
            fetched_at: quota.fetched_at,
        }
    }
}

impl From<&AppSettings> for PersistedAppSettings {
    fn from(settings: &AppSettings) -> Self {
        Self {
            auto_refresh_enabled: settings.auto_refresh_enabled,
            auto_refresh_interval_minutes: settings.auto_refresh_interval_minutes,
            close_to_tray: settings.close_to_tray,
            skip_unsupported_region_refresh: settings.skip_unsupported_region_refresh,
            hidden_subscription_categories: settings.hidden_subscription_categories.clone(),
            hidden_account_ids: settings.hidden_account_ids.clone(),
            last_active_provider: settings.last_active_provider.clone(),
            gemini_switch_targets: settings.gemini_switch_targets.clone(),
        }
    }
}

impl From<PersistedAppSettings> for AppSettings {
    fn from(settings: PersistedAppSettings) -> Self {
        Self {
            auto_refresh_enabled: settings.auto_refresh_enabled,
            auto_refresh_interval_minutes: settings.auto_refresh_interval_minutes,
            close_to_tray: settings.close_to_tray,
            skip_unsupported_region_refresh: settings.skip_unsupported_region_refresh,
            hidden_subscription_categories: settings.hidden_subscription_categories,
            hidden_account_ids: settings.hidden_account_ids,
            last_active_provider: settings.last_active_provider,
            gemini_switch_targets: settings.gemini_switch_targets,
        }
    }
}

impl From<&TokenHealthStatus> for PersistedTokenHealthStatus {
    fn from(status: &TokenHealthStatus) -> Self {
        match status {
            TokenHealthStatus::Unknown => Self::Unknown,
            TokenHealthStatus::Healthy => Self::Healthy,
            TokenHealthStatus::Refreshed => Self::Refreshed,
            TokenHealthStatus::NeedsRelogin => Self::NeedsRelogin,
            TokenHealthStatus::NetworkError => Self::NetworkError,
            TokenHealthStatus::ServerError => Self::ServerError,
        }
    }
}

impl From<PersistedTokenHealthStatus> for TokenHealthStatus {
    fn from(status: PersistedTokenHealthStatus) -> Self {
        match status {
            PersistedTokenHealthStatus::Unknown => Self::Unknown,
            PersistedTokenHealthStatus::Healthy => Self::Healthy,
            PersistedTokenHealthStatus::Refreshed => Self::Refreshed,
            PersistedTokenHealthStatus::NeedsRelogin => Self::NeedsRelogin,
            PersistedTokenHealthStatus::NetworkError => Self::NetworkError,
            PersistedTokenHealthStatus::ServerError => Self::ServerError,
        }
    }
}

impl From<&TokenHealth> for PersistedTokenHealth {
    fn from(health: &TokenHealth) -> Self {
        Self {
            status: PersistedTokenHealthStatus::from(&health.status),
            last_checked_at: health.last_checked_at,
            last_refreshed_at: health.last_refreshed_at,
            last_error: health.last_error.clone(),
        }
    }
}

impl From<PersistedTokenHealth> for TokenHealth {
    fn from(health: PersistedTokenHealth) -> Self {
        Self {
            status: TokenHealthStatus::from(health.status),
            last_checked_at: health.last_checked_at,
            last_refreshed_at: health.last_refreshed_at,
            last_error: health.last_error,
        }
    }
}

impl From<&Account> for PersistedAccount {
    fn from(account: &Account) -> Self {
        Self {
            id: account.id.clone(),
            provider: PersistedAccountProvider::from(account.provider),
            email: account.email.clone(),
            account_id: account.account_id.clone(),
            provider_project_id: account.provider_project_id.clone(),
            subscription_expires_at: account.subscription_expires_at,
            subscription_plan: account.subscription_plan.clone(),
            subscription_detected_at: account.subscription_detected_at,
            subscription_checked_at: account.subscription_checked_at,
            subscription_next_check_at: account.subscription_next_check_at,
            subscription_endpoint_hint: account.subscription_endpoint_hint.clone(),
            tokens: PersistedTokens::from(&account.tokens),
            token_expires_at: account.token_expires_at,
            tokens_updated_at: account.tokens_updated_at,
            token_health: PersistedTokenHealth::from(&account.token_health),
            quota: account.quota.as_ref().map(PersistedQuotaInfo::from),
            quota_next_refresh_at: account.quota_next_refresh_at,
            quota_refresh_failures: account.quota_refresh_failures,
            created_at: account.created_at,
            last_login_at: account.last_login_at,
            quota_error: account.last_error.clone(),
            subscription_error: account.subscription_error.clone(),
        }
    }
}

impl From<&AppData> for PersistedAppData {
    fn from(data: &AppData) -> Self {
        Self {
            schema_version: data.schema_version,
            accounts: data.accounts.iter().map(PersistedAccount::from).collect(),
            active_account_id: data.active_account_id.clone(),
            active_gemini_account_id: data.active_gemini_account_id.clone(),
            limits_base_url: data.limits_base_url.clone(),
            app_settings: PersistedAppSettings::from(&data.app_settings),
        }
    }
}

impl From<PersistedAccount> for Account {
    fn from(account: PersistedAccount) -> Self {
        Self {
            id: account.id,
            provider: crate::models::AccountProvider::from(account.provider),
            email: account.email,
            account_id: account.account_id,
            provider_project_id: account.provider_project_id,
            subscription_expires_at: account.subscription_expires_at,
            subscription_plan: account.subscription_plan,
            subscription_detected_at: account.subscription_detected_at,
            subscription_checked_at: account.subscription_checked_at,
            subscription_next_check_at: account.subscription_next_check_at,
            subscription_endpoint_hint: account.subscription_endpoint_hint,
            tokens: Tokens::from(account.tokens),
            token_expires_at: account.token_expires_at,
            tokens_updated_at: account.tokens_updated_at,
            token_health: TokenHealth::from(account.token_health),
            quota: account.quota.map(QuotaInfo::from),
            quota_next_refresh_at: account.quota_next_refresh_at,
            quota_refresh_failures: account.quota_refresh_failures,
            created_at: account.created_at,
            last_login_at: account.last_login_at,
            last_error: account.quota_error,
            subscription_error: account.subscription_error,
        }
    }
}

impl From<PersistedAppData> for AppData {
    fn from(data: PersistedAppData) -> Self {
        Self {
            revision: 0,
            schema_version: data.schema_version,
            accounts: data.accounts.into_iter().map(Account::from).collect(),
            active_account_id: data.active_account_id,
            active_gemini_account_id: data.active_gemini_account_id,
            limits_base_url: data.limits_base_url,
            app_settings: AppSettings::from(data.app_settings),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{PersistedAccount, PersistedAppData};
    use crate::models::{Account, AppData, TokenHealth, Tokens};

    fn account_with_tokens() -> Account {
        Account {
            id: "account-1".to_string(),
            provider: crate::models::AccountProvider::Codex,
            email: Some("user@example.com".to_string()),
            account_id: Some("openai-1".to_string()),
            provider_project_id: None,
            subscription_expires_at: Some(2_000),
            subscription_plan: Some("Plus".to_string()),
            subscription_detected_at: Some(1_000),
            subscription_checked_at: Some(1_000),
            subscription_next_check_at: Some(2_000),
            subscription_endpoint_hint: Some("account-payments".to_string()),
            tokens: Tokens {
                id_token: "secret-id".to_string(),
                access_token: "secret-access".to_string(),
                refresh_token: "secret-refresh".to_string(),
            },
            token_expires_at: None,
            tokens_updated_at: Some(1_000),
            token_health: TokenHealth::healthy(),
            quota: None,
            quota_next_refresh_at: None,
            quota_refresh_failures: 0,
            created_at: 1_000,
            last_login_at: 1_000,
            last_error: None,
            subscription_error: None,
        }
    }

    #[test]
    fn persisted_json_excludes_secrets() {
        let mut data = AppData::default();
        data.accounts.push(account_with_tokens());
        data.active_account_id = Some("codex-account".to_string());

        let value = serde_json::to_value(PersistedAppData::from(&data))
            .expect("serialize persisted app data");

        let serialized = value.to_string();
        assert!(!serialized.contains("secret-access"));
        assert!(!serialized.contains("refreshToken"));
        assert!(!serialized.contains("\"tokens\""));
        assert_eq!(value["schemaVersion"], 12);
        assert_eq!(value["activeAccountId"], "codex-account");
    }

    #[test]
    fn legacy_plaintext_tokens_remain_deserialize_only() {
        let json = json!({
            "schemaVersion": 8,
            "accounts": [{
                "id": "account-1",
                "email": "user@example.com",
                "accountId": "openai-1",
                "subscriptionExpiresAt": null,
                "subscriptionPlan": null,
                "subscriptionDetectedAt": null,
                "subscriptionCheckedAt": null,
                "subscriptionNextCheckAt": null,
                "subscriptionEndpointHint": null,
                "tokens": {
                    "idToken": "legacy-id",
                    "accessToken": "legacy-access",
                    "refreshToken": "legacy-refresh"
                },
                "tokensUpdatedAt": null,
                "tokenHealth": {
                    "status": "unknown",
                    "lastCheckedAt": null,
                    "lastRefreshedAt": null,
                    "lastError": null
                },
                "quota": null,
                "quotaNextRefreshAt": null,
                "quotaRefreshFailures": 0,
                "createdAt": 1,
                "lastLoginAt": 1,
                "lastError": null
            }],
            "activeAccountId": null,
            "limitsBaseUrl": "https://chatgpt.com/backend-api",
            "appSettings": {
                "autoRefreshEnabled": true,
                "autoRefreshIntervalMinutes": 15,
                "closeToTray": true,
                "hiddenSubscriptionCategories": [],
                "hiddenAccountIds": []
            }
        });

        let persisted: PersistedAppData =
            serde_json::from_value(json).expect("parse legacy persisted state");
        assert_eq!(persisted.accounts[0].tokens.access_token, "legacy-access");

        let reserialized = serde_json::to_string(&persisted).expect("reserialize persisted state");
        assert!(!reserialized.contains("legacy-access"));
        assert!(!reserialized.contains("\"tokens\""));
    }

    #[test]
    fn schema_eight_last_error_migrates_to_quota_error() {
        let json = json!({
            "schemaVersion": 8,
            "accounts": [{
                "id": "account-1",
                "email": null,
                "accountId": null,
                "subscriptionExpiresAt": null,
                "quota": null,
                "createdAt": 1,
                "lastLoginAt": 1,
                "lastError": "legacy quota failure"
            }],
            "activeAccountId": null,
            "appSettings": {
                "autoRefreshEnabled": true,
                "autoRefreshIntervalMinutes": 15,
                "closeToTray": true
            }
        });

        let persisted: PersistedAppData =
            serde_json::from_value(json).expect("parse schema eight state");
        let data = AppData::from(persisted).normalize_legacy();

        assert_eq!(data.schema_version, crate::models::APP_SCHEMA_VERSION);
        assert_eq!(
            data.accounts[0].last_error.as_deref(),
            Some("legacy quota failure")
        );
        assert!(data.accounts[0].subscription_error.is_none());
    }

    #[test]
    fn persisted_account_round_trips_public_fields() {
        let account = account_with_tokens();
        let persisted = PersistedAccount::from(&account);
        let restored = crate::models::Account::from(persisted);

        assert_eq!(restored.id, account.id);
        assert_eq!(restored.email, account.email);
        assert_eq!(restored.tokens.access_token, "secret-access");
    }
}
