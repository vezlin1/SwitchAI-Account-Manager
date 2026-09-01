use serde::{Deserialize, Serialize};

use crate::models::{
    Account, AppData, AppSettings, AutoRefreshStatus, QuotaInfo, QuotaWindow, RefreshRunSummary,
    TokenHealth, TokenHealthStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountProviderDto {
    Codex,
    Gemini,
}

impl From<crate::models::AccountProvider> for AccountProviderDto {
    fn from(provider: crate::models::AccountProvider) -> Self {
        match provider {
            crate::models::AccountProvider::Codex => Self::Codex,
            crate::models::AccountProvider::Gemini => Self::Gemini,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataDto {
    pub revision: u64,
    pub accounts: Vec<AccountDto>,
    pub active_account_id: Option<String>,
    pub active_gemini_account_id: Option<String>,
    pub app_settings: AppSettingsDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    pub id: String,
    pub provider: AccountProviderDto,
    pub email: Option<String>,
    pub account_id: Option<String>,
    pub subscription_expires_at: Option<i64>,
    pub subscription_plan: Option<String>,
    pub subscription_detected_at: Option<i64>,
    pub subscription_checked_at: Option<i64>,
    pub token_health: TokenHealthDto,
    pub quota: Option<QuotaInfoDto>,
    pub created_at: i64,
    pub last_login_at: i64,
    pub issues: AccountIssuesDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountIssuesDto {
    pub quota: Option<String>,
    pub subscription: Option<String>,
}

fn default_skip_unsupported_region_refresh() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsDto {
    pub auto_refresh_enabled: bool,
    pub auto_refresh_interval_minutes: u64,
    pub close_to_tray: bool,
    #[serde(default = "default_skip_unsupported_region_refresh")]
    pub skip_unsupported_region_refresh: bool,
    #[serde(default)]
    pub hidden_subscription_categories: Vec<String>,
    #[serde(default)]
    pub hidden_account_ids: Vec<String>,
    #[serde(default)]
    pub last_active_provider: Option<String>,
    #[serde(default)]
    pub gemini_switch_targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravitySurfaceDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub installed: bool,
    pub running: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoRefreshStatusDto {
    pub enabled: bool,
    pub in_flight: bool,
    pub last_started_at: Option<i64>,
    pub last_finished_at: Option<i64>,
    pub last_error: Option<String>,
    pub next_run_at: Option<i64>,
    pub scheduled_accounts: u32,
    pub backed_off_accounts: u32,
    pub last_run: Option<RefreshRunSummaryDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRunSummaryDto {
    pub started_at: i64,
    pub finished_at: i64,
    pub succeeded: u32,
    pub failed: u32,
    pub failed_account_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSnapshotDto {
    pub revision: u64,
    pub account: AccountDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandWarningDto {
    pub code: String,
    pub domain: String,
    pub message: String,
    pub account_id: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateResultDto {
    pub state: AppDataDto,
    pub warnings: Vec<CommandWarningDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStatusDto {
    pub error: String,
    pub data_directory: String,
    pub state_path: String,
    pub backup_available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupStatusDto {
    pub mode: String,
    pub state: Option<AppDataDto>,
    pub warnings: Vec<String>,
    pub recovery: Option<RecoveryStatusDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindowDto {
    pub used_percent: Option<f64>,
    pub limit_window_seconds: Option<i64>,
    pub reset_at: Option<i64>,
    pub fetched_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaInfoDto {
    pub plan_type: Option<String>,
    pub primary: QuotaWindowDto,
    pub secondary: QuotaWindowDto,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenHealthStatusDto {
    Unknown,
    Healthy,
    Refreshed,
    NeedsRelogin,
    NetworkError,
    ServerError,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHealthDto {
    pub status: TokenHealthStatusDto,
    pub last_checked_at: Option<i64>,
    pub last_refreshed_at: Option<i64>,
    pub last_error: Option<String>,
}

impl From<&AppData> for AppDataDto {
    fn from(data: &AppData) -> Self {
        Self {
            revision: data.revision,
            accounts: data.accounts.iter().map(AccountDto::from).collect(),
            active_account_id: data.active_account_id.clone(),
            active_gemini_account_id: data.active_gemini_account_id.clone(),
            app_settings: AppSettingsDto::from(&data.app_settings),
        }
    }
}

impl From<&Account> for AccountDto {
    fn from(account: &Account) -> Self {
        Self {
            id: account.id.clone(),
            provider: AccountProviderDto::from(account.provider),
            email: account.email.clone(),
            account_id: account.account_id.clone(),
            subscription_expires_at: account.subscription_expires_at,
            subscription_plan: account.subscription_plan.clone(),
            subscription_detected_at: account.subscription_detected_at,
            subscription_checked_at: account.subscription_checked_at,
            token_health: TokenHealthDto::from(&account.token_health),
            quota: account.quota.as_ref().map(QuotaInfoDto::from),
            created_at: account.created_at,
            last_login_at: account.last_login_at,
            issues: AccountIssuesDto {
                quota: account.last_error.clone(),
                subscription: account.subscription_error.clone(),
            },
        }
    }
}

impl From<&AppSettings> for AppSettingsDto {
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

impl From<AppSettingsDto> for AppSettings {
    fn from(settings: AppSettingsDto) -> Self {
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

impl From<&AutoRefreshStatus> for AutoRefreshStatusDto {
    fn from(status: &AutoRefreshStatus) -> Self {
        Self {
            enabled: status.enabled,
            in_flight: status.in_flight,
            last_started_at: status.last_started_at,
            last_finished_at: status.last_finished_at,
            last_error: status.last_error.clone(),
            next_run_at: status.next_run_at,
            scheduled_accounts: status.scheduled_accounts,
            backed_off_accounts: status.backed_off_accounts,
            last_run: status.last_run.as_ref().map(RefreshRunSummaryDto::from),
        }
    }
}

impl From<&RefreshRunSummary> for RefreshRunSummaryDto {
    fn from(summary: &RefreshRunSummary) -> Self {
        Self {
            started_at: summary.started_at,
            finished_at: summary.finished_at,
            succeeded: summary.succeeded,
            failed: summary.failed,
            failed_account_ids: summary.failed_account_ids.clone(),
            warnings: summary.warnings.clone(),
        }
    }
}

impl From<&QuotaWindow> for QuotaWindowDto {
    fn from(window: &QuotaWindow) -> Self {
        Self {
            used_percent: window.used_percent,
            limit_window_seconds: window.limit_window_seconds,
            reset_at: window.reset_at,
            fetched_at: window.fetched_at,
        }
    }
}

impl From<&QuotaInfo> for QuotaInfoDto {
    fn from(quota: &QuotaInfo) -> Self {
        Self {
            plan_type: quota.plan_type.clone(),
            primary: QuotaWindowDto::from(&quota.primary),
            secondary: QuotaWindowDto::from(&quota.secondary),
            fetched_at: quota.fetched_at,
        }
    }
}

impl From<&TokenHealthStatus> for TokenHealthStatusDto {
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

impl From<&TokenHealth> for TokenHealthDto {
    fn from(health: &TokenHealth) -> Self {
        Self {
            status: TokenHealthStatusDto::from(&health.status),
            last_checked_at: health.last_checked_at,
            last_refreshed_at: health.last_refreshed_at,
            last_error: health.last_error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn ipc_app_data_excludes_tokens_and_internal_state() {
        let mut data = AppData::default();
        data.accounts.push(account_with_tokens());

        let serialized =
            serde_json::to_string(&super::AppDataDto::from(&data)).expect("serialize IPC app data");

        assert!(!serialized.contains("secret-access"));
        assert!(!serialized.contains("refreshToken"));
        assert!(!serialized.contains("\"tokens\""));
        assert!(!serialized.contains("schemaVersion"));
        assert!(!serialized.contains("limitsBaseUrl"));
        assert!(serialized.contains("\"accounts\""));
    }

    #[test]
    fn ipc_account_excludes_tokens_but_keeps_public_state() {
        let account = account_with_tokens();
        let serialized = serde_json::to_string(&super::AccountDto::from(&account))
            .expect("serialize IPC account");

        assert!(!serialized.contains("secret-id"));
        assert!(!serialized.contains("secret-access"));
        assert!(!serialized.contains("secret-refresh"));
        assert!(!serialized.contains("\"tokens\""));
        assert!(!serialized.contains("tokensUpdatedAt"));
        assert!(!serialized.contains("quotaNextRefreshAt"));
        assert!(!serialized.contains("subscriptionEndpointHint"));
        assert!(serialized.contains("\"id\":\"account-1\""));
        assert!(serialized.contains("\"tokenHealth\""));
    }
}
