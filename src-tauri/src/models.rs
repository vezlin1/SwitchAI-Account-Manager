use chrono::Utc;

pub const APP_SCHEMA_VERSION: u32 = 12;

fn default_limits_base_url() -> String {
    "https://chatgpt.com/backend-api".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tokens {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Default)]
pub struct QuotaWindow {
    pub used_percent: Option<f64>,
    pub limit_window_seconds: Option<i64>,
    pub reset_at: Option<i64>,
    pub fetched_at: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct QuotaInfo {
    pub plan_type: Option<String>,
    pub primary: QuotaWindow,
    pub secondary: QuotaWindow,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TokenHealthStatus {
    #[default]
    Unknown,
    Healthy,
    Refreshed,
    NeedsRelogin,
    NetworkError,
    ServerError,
}

#[derive(Debug, Clone)]
pub struct TokenHealth {
    pub status: TokenHealthStatus,
    pub last_checked_at: Option<i64>,
    pub last_refreshed_at: Option<i64>,
    pub last_error: Option<String>,
}

impl Default for TokenHealth {
    fn default() -> Self {
        Self {
            status: TokenHealthStatus::Unknown,
            last_checked_at: None,
            last_refreshed_at: None,
            last_error: None,
        }
    }
}

impl TokenHealth {
    pub fn healthy() -> Self {
        Self {
            status: TokenHealthStatus::Healthy,
            last_checked_at: Some(now_ts()),
            last_refreshed_at: None,
            last_error: None,
        }
    }

    pub fn refreshed() -> Self {
        let now = now_ts();
        Self {
            status: TokenHealthStatus::Refreshed,
            last_checked_at: Some(now),
            last_refreshed_at: Some(now),
            last_error: None,
        }
    }

    pub fn needs_relogin(error: impl Into<String>) -> Self {
        Self {
            status: TokenHealthStatus::NeedsRelogin,
            last_checked_at: Some(now_ts()),
            last_refreshed_at: None,
            last_error: Some(error.into()),
        }
    }

    pub fn warning(status: TokenHealthStatus, error: impl Into<String>) -> Self {
        Self {
            status,
            last_checked_at: Some(now_ts()),
            last_refreshed_at: None,
            last_error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccountProvider {
    #[default]
    Codex,
    Gemini,
}

impl AccountProvider {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub provider: AccountProvider,
    pub email: Option<String>,
    pub account_id: Option<String>,
    // Provider-specific Google Cloud project discovered from Antigravity.
    // It is not a secret and avoids repeating project discovery on every refresh.
    pub provider_project_id: Option<String>,
    pub subscription_expires_at: Option<i64>,
    pub subscription_plan: Option<String>,
    pub subscription_detected_at: Option<i64>,
    pub subscription_checked_at: Option<i64>,
    pub subscription_next_check_at: Option<i64>,
    pub subscription_endpoint_hint: Option<String>,
    // Tokens are hydrated from the operating-system credential store and never
    // represented by the explicit IPC DTO layer.
    pub tokens: Tokens,
    // OAuth access-token expiry in Unix seconds. Older state leaves this empty
    // and refreshes once before the token is used.
    pub token_expires_at: Option<i64>,
    pub tokens_updated_at: Option<i64>,
    pub token_health: TokenHealth,
    pub quota: Option<QuotaInfo>,
    pub quota_next_refresh_at: Option<i64>,
    pub quota_refresh_failures: u32,
    pub created_at: i64,
    pub last_login_at: i64,
    // Quota and subscription failures are kept separate so one domain cannot
    // overwrite the status of another. `last_error` is the persisted legacy
    // quota error and is exposed through AccountIssuesDto, not directly.
    pub last_error: Option<String>,
    pub subscription_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppSettings {
    pub auto_refresh_enabled: bool,
    pub auto_refresh_interval_minutes: u64,
    pub close_to_tray: bool,
    pub skip_unsupported_region_refresh: bool,
    pub hidden_subscription_categories: Vec<String>,
    pub hidden_account_ids: Vec<String>,
    pub last_active_provider: Option<String>,
    pub gemini_switch_targets: Vec<String>,
    pub enabled_providers: Vec<String>,
    pub auto_check_updates: bool,
    pub ignored_update_version: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        let detected_targets = crate::gemini::detect_installed_antigravity_surfaces();
        let gemini_switch_targets = if detected_targets.is_empty() {
            vec!["antigravity".to_string()]
        } else {
            detected_targets
        };
        Self {
            auto_refresh_enabled: true,
            auto_refresh_interval_minutes: 15,
            close_to_tray: true,
            skip_unsupported_region_refresh: true,
            hidden_subscription_categories: Vec::new(),
            hidden_account_ids: Vec::new(),
            last_active_provider: Some("codex".to_string()),
            gemini_switch_targets,
            enabled_providers: vec!["codex".to_string(), "gemini".to_string()],
            auto_check_updates: true,
            ignored_update_version: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RefreshRunSummary {
    pub started_at: i64,
    pub finished_at: i64,
    pub succeeded: u32,
    pub failed: u32,
    pub failed_account_ids: Vec<String>,
    pub warnings: Vec<String>,
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.auto_refresh_interval_minutes = self.auto_refresh_interval_minutes.clamp(15, 1_440);
        const VALID_CATEGORIES: [&str; 10] = [
            "plus",
            "free",
            "pro-x20",
            "pro-x5",
            "google-ai-pro",
            "google-ai-ultra",
            "advanced",
            "ai-premium",
            "workspace",
            "developer",
        ];
        self.hidden_subscription_categories = VALID_CATEGORIES
            .into_iter()
            .filter(|candidate| {
                self.hidden_subscription_categories
                    .iter()
                    .any(|value| value.trim().eq_ignore_ascii_case(candidate))
            })
            .map(str::to_string)
            .collect();

        let mut hidden_account_ids = Vec::new();
        for account_id in std::mem::take(&mut self.hidden_account_ids) {
            let account_id = account_id.trim();
            if account_id.is_empty()
                || hidden_account_ids
                    .iter()
                    .any(|existing: &String| existing == account_id)
            {
                continue;
            }
            hidden_account_ids.push(account_id.to_string());
        }
        self.hidden_account_ids = hidden_account_ids;

        const VALID_PROVIDERS: [&str; 2] = ["codex", "gemini"];
        let mut normalized_providers = Vec::new();
        for provider in std::mem::take(&mut self.enabled_providers) {
            let lower = provider.trim().to_ascii_lowercase();
            if VALID_PROVIDERS.contains(&lower.as_str()) && !normalized_providers.contains(&lower) {
                normalized_providers.push(lower);
            }
        }
        if normalized_providers.is_empty() {
            normalized_providers = vec!["codex".to_string(), "gemini".to_string()];
        }
        self.enabled_providers = normalized_providers;

        if let Some(ref provider) = self.last_active_provider {
            let lower = provider.trim().to_ascii_lowercase();
            if self.enabled_providers.contains(&lower) {
                self.last_active_provider = Some(lower);
            } else {
                self.last_active_provider = Some(self.enabled_providers[0].clone());
            }
        } else {
            self.last_active_provider = Some(self.enabled_providers[0].clone());
        }

        const VALID_TARGETS: [&str; 3] = ["antigravity", "ide", "cli"];
        let mut normalized_targets = Vec::new();
        for target in std::mem::take(&mut self.gemini_switch_targets) {
            let target = target.trim().to_ascii_lowercase();
            if VALID_TARGETS.contains(&target.as_str()) && !normalized_targets.contains(&target) {
                normalized_targets.push(target);
            }
        }
        if normalized_targets.is_empty() {
            let detected = crate::gemini::detect_installed_antigravity_surfaces();
            normalized_targets = if detected.is_empty() {
                vec!["antigravity".to_string()]
            } else {
                detected
            };
        }
        self.gemini_switch_targets = normalized_targets;

        self
    }
}

#[derive(Debug, Clone)]
pub struct AutoRefreshStatus {
    pub enabled: bool,
    pub in_flight: bool,
    pub last_started_at: Option<i64>,
    pub last_finished_at: Option<i64>,
    pub last_error: Option<String>,
    pub next_run_at: Option<i64>,
    pub scheduled_accounts: u32,
    pub backed_off_accounts: u32,
    pub last_run: Option<RefreshRunSummary>,
}

impl AutoRefreshStatus {
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            enabled: settings.auto_refresh_enabled,
            in_flight: false,
            last_started_at: None,
            last_finished_at: None,
            last_error: None,
            next_run_at: if settings.auto_refresh_enabled {
                Some(now_ts() + (settings.auto_refresh_interval_minutes as i64 * 60))
            } else {
                None
            },
            scheduled_accounts: 0,
            backed_off_accounts: 0,
            last_run: None,
        }
    }
}

impl Default for AutoRefreshStatus {
    fn default() -> Self {
        Self::from_settings(&AppSettings::default())
    }
}

#[derive(Debug, Clone)]
pub struct AppData {
    // Runtime-only monotonic revision. Persistence deliberately omits it; a
    // restarted frontend cannot race responses from the previous process.
    pub revision: u64,
    pub schema_version: u32,
    pub accounts: Vec<Account>,
    pub active_account_id: Option<String>,
    pub active_gemini_account_id: Option<String>,
    pub limits_base_url: String,
    pub app_settings: AppSettings,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            revision: 0,
            schema_version: APP_SCHEMA_VERSION,
            accounts: Vec::new(),
            active_account_id: None,
            active_gemini_account_id: None,
            limits_base_url: default_limits_base_url(),
            app_settings: AppSettings::default(),
        }
    }
}

impl AppData {
    pub fn normalize_legacy(mut self) -> Self {
        if self.schema_version < APP_SCHEMA_VERSION {
            self.schema_version = APP_SCHEMA_VERSION;
        }
        for account in &mut self.accounts {
            if account.tokens_updated_at.is_none() {
                account.tokens_updated_at = Some(account.last_login_at);
            }
            if account.subscription_next_check_at.is_none() {
                account.subscription_next_check_at = match account.subscription_expires_at {
                    Some(expires_at) if expires_at > now_ts() => Some(expires_at),
                    Some(expires_at) => Some(
                        account
                            .subscription_checked_at
                            .unwrap_or(expires_at)
                            .max(expires_at)
                            .saturating_add(24 * 60 * 60),
                    ),
                    None => account
                        .subscription_checked_at
                        .map(|checked_at| checked_at.saturating_add(7 * 24 * 60 * 60)),
                };
            }
        }
        if self.limits_base_url.trim().is_empty() {
            self.limits_base_url = default_limits_base_url();
        }
        self.app_settings = self.app_settings.normalized();
        if self.active_account_id.as_ref().is_some_and(|active_id| {
            !self.accounts.iter().any(|account| {
                &account.id == active_id && account.provider == AccountProvider::Codex
            })
        }) {
            self.active_account_id = None;
        }
        if self
            .active_gemini_account_id
            .as_ref()
            .is_some_and(|active_id| {
                !self.accounts.iter().any(|account| {
                    &account.id == active_id && account.provider == AccountProvider::Gemini
                })
            })
        {
            self.active_gemini_account_id = None;
        }
        self
    }

    pub fn active_account_id_for_provider(&self, provider: AccountProvider) -> Option<&str> {
        match provider {
            AccountProvider::Codex => self.active_account_id.as_deref(),
            AccountProvider::Gemini => self.active_gemini_account_id.as_deref(),
        }
    }

    pub fn set_active_account_id_for_provider(
        &mut self,
        provider: AccountProvider,
        account_id: Option<String>,
    ) {
        match provider {
            AccountProvider::Codex => self.active_account_id = account_id,
            AccountProvider::Gemini => self.active_gemini_account_id = account_id,
        }
    }

    pub fn active_account_for_provider(&self, provider: AccountProvider) -> Option<&Account> {
        let active_id = self.active_account_id_for_provider(provider)?;
        self.accounts
            .iter()
            .find(|account| account.id == active_id && account.provider == provider)
    }
}

pub fn now_ts() -> i64 {
    Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::AppSettings;

    #[test]
    fn app_settings_normalize_hidden_subscription_categories() {
        let settings = AppSettings {
            auto_refresh_interval_minutes: 1,
            hidden_subscription_categories: vec![
                " FREE ".to_string(),
                "free".to_string(),
                "PRO-X5".to_string(),
                "Google-AI-Ultra".to_string(),
                "unknown".to_string(),
            ],
            hidden_account_ids: vec![
                " account-2 ".to_string(),
                "account-2".to_string(),
                "".to_string(),
                "account-1".to_string(),
            ],
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(
            settings.hidden_subscription_categories,
            vec![
                "free".to_string(),
                "pro-x5".to_string(),
                "google-ai-ultra".to_string()
            ]
        );
        assert_eq!(
            settings.hidden_account_ids,
            vec!["account-2".to_string(), "account-1".to_string()]
        );
        assert_eq!(settings.auto_refresh_interval_minutes, 15);
    }

    #[test]
    fn app_settings_normalizes_provider_and_gemini_targets() {
        let settings = AppSettings {
            last_active_provider: Some(" GEMINI ".to_string()),
            gemini_switch_targets: vec![
                "ANTIGRAVITY".to_string(),
                "ide".to_string(),
                "invalid".to_string(),
                "ide".to_string(),
            ],
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(settings.last_active_provider, Some("gemini".to_string()));
        assert_eq!(
            settings.gemini_switch_targets,
            vec!["antigravity".to_string(), "ide".to_string()]
        );

        let empty_targets = AppSettings {
            gemini_switch_targets: vec!["unknown".to_string()],
            ..AppSettings::default()
        }
        .normalized();
        assert!(!empty_targets.gemini_switch_targets.is_empty());
    }
}
