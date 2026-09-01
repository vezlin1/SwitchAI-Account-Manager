use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::app_state::{SharedState, lock_data};
use crate::codex::{
    clear_codex_auth, read_codex_auth, reconcile_codex_auth_transactionally,
    reconcile_codex_auth_with_snapshot, write_codex_auth, write_codex_auth_if_changed,
};
use crate::errors::{AppError, AppResult};
use crate::models::{Account, AccountProvider, AppData, TokenHealth, Tokens, now_ts};
use crate::quota::{
    QuotaRefreshUpdate, is_quota_auth_error, is_refresh_token_reuse_error,
    refresh_quota_with_token_refresh, refresh_quota_without_token_refresh,
};
use crate::storage::persist_app_data;
use crate::subscription::{
    SubscriptionInfo, SubscriptionRefreshUpdate, fetch_subscription_info, next_subscription_check,
    refresh_subscription_with_token_refresh, refresh_subscription_without_token_refresh,
};

type ActiveAuth = Option<(Tokens, Option<String>)>;

pub struct RefreshService {
    state: Arc<SharedState>,
}

#[derive(Debug, Clone)]
pub struct SubscriptionRefreshResult {
    pub state: AppData,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaRefreshWarning {
    TokenUpdateSkipped,
}

#[derive(Debug, Clone)]
pub struct QuotaRefreshOutcome {
    pub succeeded: bool,
    pub warning: Option<QuotaRefreshWarning>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AccountRefreshOutcome {
    pub succeeded: bool,
    pub warnings: Vec<String>,
}

struct TokenUpdateDecision {
    allowed: bool,
    warning: Option<String>,
}

struct AccountSnapshot {
    base_url: String,
    account: Account,
    ownership: CredentialOwnership,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct CredentialOwnership {
    codex: bool,
}

impl CredentialOwnership {
    fn is_external(self) -> bool {
        self.codex
    }
}

type ReconciledAccountSnapshot = (Option<(String, Account)>, Option<String>);

impl RefreshService {
    pub fn new(state: Arc<SharedState>) -> Self {
        Self { state }
    }

    pub async fn refresh_account_subscription(
        &self,
        account_id: &str,
    ) -> AppResult<SubscriptionRefreshResult> {
        let snapshot = self
            .snapshot_account(account_id)?
            .ok_or_else(|| AppError::msg("Account not found"))?;

        let skip_unsupported = lock_data(&self.state)?
            .app_settings
            .skip_unsupported_region_refresh;
        if skip_unsupported && snapshot.account.provider == AccountProvider::Codex {
            let reachability =
                crate::auto_refresh::get_chatgpt_reachability(&self.state, false).await;
            if !reachability.is_available() {
                let msg = reachability.user_summary();
                log::info!("Skipping ChatGPT subscription refresh for account {account_id}: {msg}");
                return Ok(SubscriptionRefreshResult {
                    state: lock_data(&self.state)?.clone(),
                    warning: Some(format!("Subscription refresh skipped: {msg}")),
                });
            }
        }

        let mut warnings = snapshot.warnings.clone();
        let old_refresh_token = snapshot.account.tokens.refresh_token.clone();
        let mut update = self
            .refresh_subscription_for_owner(
                &snapshot.base_url,
                &snapshot.account,
                snapshot.ownership,
            )
            .await;
        let mut source_snapshot = snapshot.account.clone();

        if should_retry_subscription_after_reconcile(&update) {
            let (retry, warning) =
                self.refreshed_snapshot_after_reconcile(account_id, &snapshot.account)?;
            if let Some(warning) = warning {
                warnings.push(warning);
            }
            if let Some((retry_base_url, retry_snapshot)) = retry {
                update = self
                    .refresh_subscription_for_owner(
                        &retry_base_url,
                        &retry_snapshot,
                        snapshot.ownership,
                    )
                    .await;
                source_snapshot = retry_snapshot;
            }
        }

        let mut result = self.commit_subscription_update(
            account_id,
            &source_snapshot,
            &old_refresh_token,
            update,
            snapshot.ownership,
        )?;
        if let Some(warning) = result.warning.take() {
            warnings.push(warning);
        }
        result.warning = (!warnings.is_empty()).then(|| warnings.join("; "));
        Ok(result)
    }

    pub async fn refresh_account_quota(&self, account_id: &str) -> AppResult<QuotaRefreshOutcome> {
        let snapshot = self
            .snapshot_account(account_id)?
            .ok_or_else(|| AppError::msg("Account not found"))?;

        let skip_unsupported = lock_data(&self.state)?
            .app_settings
            .skip_unsupported_region_refresh;
        if skip_unsupported && snapshot.account.provider == AccountProvider::Codex {
            let reachability =
                crate::auto_refresh::get_chatgpt_reachability(&self.state, false).await;
            if !reachability.is_available() {
                let msg = reachability.user_summary();
                log::info!("Skipping ChatGPT quota refresh for account {account_id}: {msg}");
                return Ok(QuotaRefreshOutcome {
                    succeeded: false,
                    warning: None,
                    warnings: vec![format!("Quota refresh skipped: {msg}")],
                });
            }
        }

        let mut warnings = snapshot.warnings.clone();
        let old_refresh_token = snapshot.account.tokens.refresh_token.clone();
        let mut update = self
            .refresh_quota_for_owner(&snapshot.base_url, &snapshot.account, snapshot.ownership)
            .await;
        let mut source_snapshot = snapshot.account.clone();

        if should_retry_after_reconcile(&update) {
            let (retry, warning) =
                self.refreshed_snapshot_after_reconcile(account_id, &snapshot.account)?;
            if let Some(warning) = warning {
                warnings.push(warning);
            }
            if let Some((retry_base_url, retry_snapshot)) = retry {
                update = self
                    .refresh_quota_for_owner(&retry_base_url, &retry_snapshot, snapshot.ownership)
                    .await;
                source_snapshot = retry_snapshot;
            }
        }

        let quota_succeeded = update.quota_result.is_ok();
        let ownership = snapshot.ownership;
        let mut data = lock_data(&self.state)?;
        let decision = self.allow_token_update(&data, account_id, &source_snapshot, ownership);
        let allow_token_update = decision.allowed;
        let skipped_tokens = skipped_token_update(
            allow_token_update,
            update.refreshed_tokens.as_ref(),
            &source_snapshot,
        );
        let mut has_explicit_skip_warning = false;
        if skipped_tokens && let Some(warning) = decision.warning {
            has_explicit_skip_warning = true;
            warnings.push(warning);
        }
        let mut next = data.clone();
        let active_auth = apply_quota_update(
            &data,
            &mut next,
            account_id,
            &source_snapshot,
            &old_refresh_token,
            update,
            None,
            allow_token_update,
        )?;
        if let Some(warning) = commit_update(&mut data, next, active_auth)? {
            warnings.push(warning);
        }

        Ok(QuotaRefreshOutcome {
            succeeded: quota_succeeded,
            warning: (!has_explicit_skip_warning)
                .then(|| quota_warning_for_cas(skipped_tokens, quota_succeeded))
                .flatten(),
            warnings,
        })
    }

    pub async fn refresh_single_account(
        &self,
        account_id: &str,
    ) -> AppResult<AccountRefreshOutcome> {
        let Some(snapshot) = self.snapshot_account(account_id)? else {
            return Ok(AccountRefreshOutcome {
                succeeded: true,
                warnings: Vec::new(),
            });
        };

        let skip_unsupported = lock_data(&self.state)?
            .app_settings
            .skip_unsupported_region_refresh;
        if skip_unsupported && snapshot.account.provider == AccountProvider::Codex {
            let reachability =
                crate::auto_refresh::get_chatgpt_reachability(&self.state, false).await;
            if !reachability.is_available() {
                let msg = reachability.user_summary();
                log::info!("Skipping ChatGPT refresh for account {account_id}: {msg}");
                return Ok(AccountRefreshOutcome {
                    succeeded: false,
                    warnings: vec![format!("Refresh skipped: {msg}")],
                });
            }
        }

        let mut warnings = snapshot.warnings.clone();
        let old_refresh_token = snapshot.account.tokens.refresh_token.clone();
        let mut update = self
            .refresh_quota_for_owner(&snapshot.base_url, &snapshot.account, snapshot.ownership)
            .await;
        let mut source_snapshot = snapshot.account.clone();
        let mut effective_base_url = snapshot.base_url.clone();

        if should_retry_after_reconcile(&update) {
            let (retry, _) =
                self.refreshed_snapshot_after_reconcile(account_id, &snapshot.account)?;
            if let Some((retry_base_url, retry_snapshot)) = retry {
                update = self
                    .refresh_quota_for_owner(&retry_base_url, &retry_snapshot, snapshot.ownership)
                    .await;
                source_snapshot = retry_snapshot;
                effective_base_url = retry_base_url;
            }
        }

        let refresh_succeeded = update.quota_result.is_ok();
        let subscription_result =
            if refresh_succeeded && subscription_refresh_due(&snapshot.account, now_ts()) {
                let tokens = update
                    .refreshed_tokens
                    .as_ref()
                    .unwrap_or(&source_snapshot.tokens);
                Some(
                    fetch_subscription_info(
                        &self.state.http_client,
                        &effective_base_url,
                        tokens,
                        source_snapshot.account_id.as_deref(),
                        source_snapshot.subscription_endpoint_hint.as_deref(),
                    )
                    .await,
                )
            } else {
                None
            };

        let ownership = snapshot.ownership;
        let mut data = lock_data(&self.state)?;
        let decision = self.allow_token_update(&data, account_id, &source_snapshot, ownership);
        let allow_token_update = decision.allowed;
        if skipped_token_update(
            allow_token_update,
            update.refreshed_tokens.as_ref(),
            &source_snapshot,
        ) {
            log::warn!(
                "Refresh for {account_id}: account tokens changed during refresh; keeping newer tokens"
            );
            warnings.push(decision.warning.unwrap_or_else(|| {
                "Token update skipped: account tokens changed during refresh".to_string()
            }));
        } else if !allow_token_update {
            log::debug!(
                "Refresh for {account_id}: credential commit was blocked, but the refresh produced no new tokens"
            );
        }
        let mut next = data.clone();
        let active_auth = apply_quota_update(
            &data,
            &mut next,
            account_id,
            &source_snapshot,
            &old_refresh_token,
            update,
            subscription_result,
            allow_token_update,
        )?;
        if let Some(warning) = commit_update(&mut data, next, active_auth)? {
            warnings.push(warning);
        }

        Ok(AccountRefreshOutcome {
            succeeded: refresh_succeeded,
            warnings,
        })
    }

    fn snapshot_account(&self, account_id: &str) -> AppResult<Option<AccountSnapshot>> {
        let mut data = lock_data(&self.state)?;
        let mut warnings = Vec::new();
        // One read backs both the reconciliation below and the ownership
        // decision, so a refresh cannot reconcile against one version of
        // auth.json and then commit against another.
        let (codex_auth, codex_auth_readable) = match read_codex_auth() {
            Ok(codex_auth) => (codex_auth, true),
            Err(error) => {
                let message = format!("auth.json could not be read: {}", error.user_message());
                log::warn!("{message}");
                warnings.push(message);
                (None, false)
            }
        };
        if let Err(error) = reconcile_codex_auth_with_snapshot(&mut data, codex_auth.as_ref()) {
            let message = format!(
                "auth.json reconciliation was skipped: {}",
                error.user_message()
            );
            log::warn!("{message}");
            warnings.push(message);
        }
        // An unreadable file is treated as still externally owned; only a file
        // we could read and found empty hands ownership back to the manager.
        let codex_auth_present = codex_auth.is_some() || !codex_auth_readable;
        let base_url = data.limits_base_url.clone();
        let active_account = data.active_account_id.as_ref().and_then(|active_id| {
            data.accounts
                .iter()
                .find(|account| &account.id == active_id)
                .cloned()
        });
        Ok(data
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .cloned()
            .map(|account| {
                let ownership = CredentialOwnership {
                    codex: codex_owns_account(
                        &account,
                        active_account.as_ref(),
                        codex_auth_present,
                    ),
                };
                AccountSnapshot {
                    base_url,
                    account,
                    ownership,
                    warnings,
                }
            }))
    }

    fn refreshed_snapshot_after_reconcile(
        &self,
        account_id: &str,
        previous: &Account,
    ) -> AppResult<ReconciledAccountSnapshot> {
        let mut data = lock_data(&self.state)?;
        let mut warnings = Vec::new();
        if let Err(error) = reconcile_codex_auth_transactionally(&mut data) {
            let message = format!(
                "auth.json reconciliation retry was skipped: {}",
                error.user_message()
            );
            log::warn!("{message}");
            warnings.push(message);
        }
        let base_url = data.limits_base_url.clone();
        let snapshot = data
            .accounts
            .iter()
            .find(|account| {
                account.id == account_id && tokens_differ(&account.tokens, &previous.tokens)
            })
            .cloned()
            .map(|account| (base_url, account));
        Ok((
            snapshot,
            (!warnings.is_empty()).then(|| warnings.join("; ")),
        ))
    }

    async fn refresh_quota_for_owner(
        &self,
        base_url: &str,
        account: &Account,
        ownership: CredentialOwnership,
    ) -> QuotaRefreshUpdate {
        if ownership.is_external() {
            refresh_quota_without_token_refresh(&self.state.http_client, base_url, account).await
        } else {
            refresh_quota_with_token_refresh(&self.state.http_client, base_url, account).await
        }
    }

    async fn refresh_subscription_for_owner(
        &self,
        base_url: &str,
        account: &Account,
        ownership: CredentialOwnership,
    ) -> SubscriptionRefreshUpdate {
        if ownership.is_external() {
            refresh_subscription_without_token_refresh(&self.state.http_client, base_url, account)
                .await
        } else {
            refresh_subscription_with_token_refresh(&self.state.http_client, base_url, account)
                .await
        }
    }

    fn commit_subscription_update(
        &self,
        account_id: &str,
        source_snapshot: &Account,
        old_refresh_token: &str,
        update: SubscriptionRefreshUpdate,
        ownership: CredentialOwnership,
    ) -> AppResult<SubscriptionRefreshResult> {
        let mut data = lock_data(&self.state)?;
        let decision = self.allow_token_update(&data, account_id, source_snapshot, ownership);
        let mut warnings = Vec::new();
        let allow_token_update = decision.allowed;
        if skipped_token_update(
            allow_token_update,
            update.refreshed_tokens.as_ref(),
            source_snapshot,
        ) {
            warnings.push(decision.warning.unwrap_or_else(|| {
                "Token update skipped: account tokens changed during refresh".to_string()
            }));
        }
        let mut next = data.clone();
        let (active_auth, error) = apply_subscription_update(
            &data,
            &mut next,
            account_id,
            source_snapshot,
            old_refresh_token,
            update,
            allow_token_update,
        )?;
        if let Some(warning) = commit_update(&mut data, next, active_auth)? {
            warnings.push(warning);
        }
        let mut result = subscription_refresh_result(data.clone(), error);
        if let Some(warning) = result.warning.take() {
            warnings.push(warning);
        }
        result.warning = (!warnings.is_empty()).then(|| warnings.join("; "));
        Ok(result)
    }

    /// Exact CAS for the refresh commit. The in-memory row must still match the
    /// network snapshot, and for an externally owned active account the
    /// corresponding auth.json must still hold the snapshot tokens. If a client
    /// replaced credentials between snapshot and commit, the stale refresh is
    /// not allowed to advance or overwrite them.
    fn allow_token_update(
        &self,
        data: &AppData,
        account_id: &str,
        source_snapshot: &Account,
        ownership: CredentialOwnership,
    ) -> TokenUpdateDecision {
        let codex_auth_tokens = if ownership.codex {
            match read_codex_auth() {
                Ok(snapshot) => snapshot.map(|auth| auth.tokens),
                Err(error) => {
                    let warning = format!(
                        "auth.json could not be read; token update skipped: {}",
                        error.user_message()
                    );
                    log::warn!("{warning}");
                    return TokenUpdateDecision {
                        allowed: false,
                        warning: Some(warning),
                    };
                }
            }
        } else {
            None
        };
        TokenUpdateDecision {
            allowed: refresh_may_update_tokens(
                data,
                account_id,
                source_snapshot,
                ownership.codex,
                codex_auth_tokens.as_ref(),
            ),
            warning: None,
        }
    }
}

pub(crate) fn active_account(data: &AppData) -> Option<Account> {
    let active_id = data.active_account_id.as_ref()?;
    data.accounts
        .iter()
        .find(|account| &account.id == active_id)
        .cloned()
}

pub(crate) fn write_account_auth(account: Option<&Account>) -> AppResult<()> {
    match account {
        Some(account) => write_codex_auth(&account.tokens, account.account_id.as_deref()),
        None => clear_codex_auth(),
    }
}

fn commit_update(
    current: &mut AppData,
    mut next: AppData,
    active_auth: ActiveAuth,
) -> AppResult<Option<String>> {
    let previous_active = active_account(current);
    let mut warning = None;
    // Only writes when auth.json does not already hold these credentials; the
    // rollback below stays unconditional so a restore always lands.
    if let Some((tokens, account_id)) = active_auth.as_ref()
        && let Err(error) = write_codex_auth_if_changed(tokens, account_id.as_deref())
    {
        let mut message = format!(
            "auth.json could not be updated; token update skipped: {}",
            error.user_message()
        );
        if let Err(rollback_error) = write_account_auth(previous_active.as_ref()) {
            message.push_str(&format!(
                "; failed to restore previous auth.json: {}",
                rollback_error.user_message()
            ));
        }
        log::warn!("{message}");
        restore_credential_state(current, &mut next);
        warning = Some(message);
    }
    if let Err(error) = persist_app_data(current, &next) {
        if warning.is_none() {
            let mut message = error.user_message();
            if active_auth.is_some()
                && let Err(rollback_error) = write_account_auth(previous_active.as_ref())
            {
                message.push_str(&format!(
                    "; failed to restore previous auth.json: {}",
                    rollback_error.user_message()
                ));
            }
            return Err(AppError::msg(message));
        }
        if let Some(auth_error) = warning {
            return Err(AppError::msg(format!(
                "{}; {}",
                error.user_message(),
                auth_error
            )));
        }
        return Err(error);
    }
    next.revision = current.revision.saturating_add(1);
    *current = next;
    Ok(warning)
}

fn restore_credential_state(current: &AppData, next: &mut AppData) {
    for next_account in &mut next.accounts {
        let Some(previous) = current
            .accounts
            .iter()
            .find(|account| account.id == next_account.id)
        else {
            continue;
        };
        next_account.tokens = previous.tokens.clone();
        next_account.tokens_updated_at = previous.tokens_updated_at;
        next_account.token_health = previous.token_health.clone();
    }
}

// Keeping the complete refresh snapshot explicit here makes the CAS and rollback
// boundary auditable; bundling these values would obscure which state is authoritative.
#[allow(clippy::too_many_arguments)]
fn apply_quota_update(
    current: &AppData,
    data: &mut AppData,
    account_id: &str,
    source_snapshot: &Account,
    old_refresh_token: &str,
    update: QuotaRefreshUpdate,
    subscription_result: Option<AppResult<SubscriptionInfo>>,
    allow_token_update: bool,
) -> AppResult<ActiveAuth> {
    let refreshed_tokens = allow_token_update
        .then_some(update.refreshed_tokens)
        .flatten();
    let token_health = allow_token_update.then_some(update.token_health).flatten();
    let checked_at = now_ts();

    {
        let account = data
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
            .ok_or_else(|| AppError::msg("Account disappeared during update"))?;

        if let Some(new_tokens) = refreshed_tokens.as_ref() {
            account.tokens = new_tokens.clone();
            account.tokens_updated_at = Some(checked_at);
        }
        if let Some(token_health) = token_health.as_ref() {
            account.token_health = token_health.clone();
        }

        match update.quota_result {
            Ok(quota) => {
                if let Some(plan_type) = quota.plan_type.as_ref() {
                    account.subscription_plan = Some(plan_type.clone());
                    account.subscription_detected_at = Some(checked_at);
                }
                account.quota = Some(quota);
                account.last_error = None;
                account.quota_refresh_failures = 0;
                let settings = current.app_settings.clone().normalized();
                let is_active = crate::auto_refresh::is_account_active_in_data(account, current);
                account.quota_next_refresh_at = Some(crate::auto_refresh::next_after_success(
                    account, &settings, is_active, checked_at, checked_at, true,
                ));
            }
            Err(err) => {
                account.last_error = Some(err.user_message());
            }
        }

        if let Some(result) = subscription_result {
            account.subscription_checked_at = Some(checked_at);
            account.subscription_next_check_at = Some(next_subscription_check(
                checked_at,
                account.subscription_expires_at,
                &result,
            ));
            account.subscription_error = result.as_ref().err().map(AppError::user_message);
            match result {
                Ok(info) => {
                    if let Some(plan) = info.plan {
                        account.subscription_plan = Some(plan);
                    }
                    if let Some(expires_at) = info.expires_at {
                        account.subscription_expires_at = Some(expires_at);
                    }
                    if let Some(endpoint_hint) = info.endpoint_hint {
                        account.subscription_endpoint_hint = Some(endpoint_hint);
                    }
                    account.subscription_detected_at = Some(info.fetched_at);
                }
                Err(error) => log::warn!(
                    "Automatic subscription refresh failed for {}: {}",
                    account.email.as_deref().unwrap_or(&account.id),
                    error.user_message()
                ),
            }
        }
    }

    if let Some(new_tokens) = refreshed_tokens.as_ref() {
        sync_related_account_tokens(
            current,
            data,
            account_id,
            source_snapshot,
            old_refresh_token,
            new_tokens,
            checked_at,
        );
    }

    let active_auth = if allow_token_update {
        active_auth_for_update(
            current,
            data,
            account_id,
            source_snapshot,
            old_refresh_token,
            refreshed_tokens.as_ref(),
        )
    } else {
        None
    };
    Ok(active_auth)
}

fn apply_subscription_update(
    current: &AppData,
    data: &mut AppData,
    account_id: &str,
    source_snapshot: &Account,
    old_refresh_token: &str,
    update: SubscriptionRefreshUpdate,
    allow_token_update: bool,
) -> AppResult<(ActiveAuth, Option<AppError>)> {
    let refreshed_tokens = allow_token_update
        .then_some(update.refreshed_tokens)
        .flatten();
    let token_health = allow_token_update.then_some(update.token_health).flatten();
    let checked_at = now_ts();
    let subscription_result = update.subscription_result;

    {
        let account = data
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
            .ok_or_else(|| AppError::msg("Account disappeared during subscription update"))?;

        if let Some(new_tokens) = refreshed_tokens.as_ref() {
            account.tokens = new_tokens.clone();
            account.tokens_updated_at = Some(checked_at);
        }
        if let Some(token_health) = token_health.as_ref() {
            account.token_health = token_health.clone();
        }

        account.subscription_checked_at = Some(checked_at);
        account.subscription_next_check_at = Some(next_subscription_check(
            checked_at,
            account.subscription_expires_at,
            &subscription_result,
        ));
        apply_subscription_result(account, &subscription_result);
        account.subscription_error = subscription_result
            .as_ref()
            .err()
            .map(AppError::user_message);
    }

    if let Some(new_tokens) = refreshed_tokens.as_ref() {
        sync_related_account_tokens(
            current,
            data,
            account_id,
            source_snapshot,
            old_refresh_token,
            new_tokens,
            checked_at,
        );
    }

    let active_auth = if allow_token_update {
        active_auth_for_update(
            current,
            data,
            account_id,
            source_snapshot,
            old_refresh_token,
            refreshed_tokens.as_ref(),
        )
    } else {
        None
    };
    let error = subscription_result.err();

    Ok((active_auth, error))
}

pub(crate) fn apply_subscription_result(
    account: &mut Account,
    result: &AppResult<SubscriptionInfo>,
) {
    let Ok(info) = result else {
        return;
    };

    if let Some(plan) = info.plan.as_ref() {
        account.subscription_plan = Some(plan.clone());
    }
    if let Some(expires_at) = info.expires_at {
        account.subscription_expires_at = Some(expires_at);
    }
    if let Some(endpoint_hint) = info.endpoint_hint.as_ref() {
        account.subscription_endpoint_hint = Some(endpoint_hint.clone());
    }
    account.subscription_detected_at = Some(info.fetched_at);
}

fn subscription_refresh_result(
    state: AppData,
    error: Option<AppError>,
) -> SubscriptionRefreshResult {
    SubscriptionRefreshResult {
        state,
        warning: error.map(|error| error.user_message()),
    }
}

fn sync_related_account_tokens(
    current: &AppData,
    data: &mut AppData,
    source_account_id: &str,
    source_snapshot: &Account,
    old_refresh_token: &str,
    new_tokens: &Tokens,
    checked_at: i64,
) {
    for account in &mut data.accounts {
        if account.id == source_account_id {
            continue;
        }
        let Some(current_account) = current
            .accounts
            .iter()
            .find(|candidate| candidate.id == account.id)
        else {
            continue;
        };
        if !matches_refresh_source(current_account, source_snapshot, old_refresh_token)
            || tokens_differ(&current_account.tokens, &account.tokens)
        {
            continue;
        }

        account.tokens = new_tokens.clone();
        account.tokens_updated_at = Some(checked_at);
        account.token_health = TokenHealth::refreshed();
        account.last_error = None;
    }
}

pub(crate) fn active_auth_for_update(
    current: &AppData,
    data: &AppData,
    source_account_id: &str,
    source_snapshot: &Account,
    old_refresh_token: &str,
    refreshed_tokens: Option<&Tokens>,
) -> ActiveAuth {
    let active_id = data.active_account_id.as_ref()?;
    let active = data
        .accounts
        .iter()
        .find(|account| &account.id == active_id)?;
    let current_active = current
        .accounts
        .iter()
        .find(|account| &account.id == active_id)?;

    let changed_by_other_writer = tokens_differ(&current_active.tokens, &active.tokens)
        && active.id != source_account_id
        && refreshed_tokens
            .is_none_or(|tokens| active.tokens.refresh_token != tokens.refresh_token);
    if changed_by_other_writer {
        return None;
    }

    let refreshed_match = refreshed_tokens.is_some_and(|tokens| {
        !tokens.refresh_token.trim().is_empty()
            && active.tokens.refresh_token == tokens.refresh_token
    });
    let active_matches = active.id == source_account_id
        || matches_refresh_source(active, source_snapshot, old_refresh_token)
        || refreshed_match;

    active_matches.then(|| (active.tokens.clone(), active.account_id.clone()))
}

pub(crate) fn matches_refresh_source(
    account: &Account,
    source_snapshot: &Account,
    old_refresh_token: &str,
) -> bool {
    if account.provider != source_snapshot.provider {
        return false;
    }
    source_snapshot
        .account_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .zip(account.account_id.as_deref())
        .is_some_and(|(left, right)| left == right)
        || source_snapshot
            .email
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .zip(account.email.as_deref())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
        || (!old_refresh_token.trim().is_empty()
            && account.tokens.refresh_token == old_refresh_token)
}

pub(crate) fn account_belongs_to_active_codex(account: &Account, active_account: &Account) -> bool {
    account.provider == AccountProvider::Codex
        && active_account.provider == AccountProvider::Codex
        && (account.id == active_account.id
            || matches_refresh_source(
                account,
                active_account,
                &active_account.tokens.refresh_token,
            ))
}

pub(crate) fn refresh_gate_key(account: &Account, active_account: Option<&Account>) -> String {
    if active_account
        .is_some_and(|active_account| account_belongs_to_active_codex(account, active_account))
    {
        return "codex-owned-active".to_string();
    }

    let provider = account.provider.as_str();
    if let Some(account_id) = account.account_id.as_deref().map(str::trim)
        && !account_id.is_empty()
    {
        return format!("{provider}:account:{account_id}");
    }

    if let Some(email) = account.email.as_deref().map(str::trim)
        && !email.is_empty()
    {
        return format!("{provider}:email:{}", email.to_ascii_lowercase());
    }

    if !account.tokens.refresh_token.trim().is_empty() {
        let digest = Sha256::digest(account.tokens.refresh_token.as_bytes());
        let fingerprint = digest[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        return format!("{provider}:refresh-token:{fingerprint}");
    }

    format!("{provider}:account-row:{}", account.id)
}

fn is_refresh_reuse_update(update: &QuotaRefreshUpdate) -> bool {
    update
        .quota_result
        .as_ref()
        .err()
        .map(AppError::user_message)
        .or_else(|| {
            update
                .token_health
                .as_ref()
                .and_then(|health| health.last_error.clone())
        })
        .is_some_and(|message| is_refresh_token_reuse_error(&message))
}

fn is_quota_auth_update(update: &QuotaRefreshUpdate) -> bool {
    update
        .quota_result
        .as_ref()
        .err()
        .is_some_and(|err| is_quota_auth_error(&err.user_message()))
}

fn should_retry_after_reconcile(update: &QuotaRefreshUpdate) -> bool {
    is_refresh_reuse_update(update) || is_quota_auth_update(update)
}

fn should_retry_subscription_after_reconcile(update: &SubscriptionRefreshUpdate) -> bool {
    update
        .subscription_result
        .as_ref()
        .err()
        .map(AppError::user_message)
        .is_some_and(|message| {
            is_refresh_token_reuse_error(&message) || is_quota_auth_error(&message)
        })
        || update
            .token_health
            .as_ref()
            .and_then(|health| health.last_error.clone())
            .is_some_and(|message| {
                is_refresh_token_reuse_error(&message) || is_quota_auth_error(&message)
            })
}

pub(crate) fn tokens_differ(left: &Tokens, right: &Tokens) -> bool {
    left.access_token != right.access_token
        || left.refresh_token != right.refresh_token
        || left.id_token != right.id_token
}

fn tokens_match_snapshot(data: &AppData, account_id: &str, snapshot: &Account) -> bool {
    data.accounts
        .iter()
        .find(|account| account.id == account_id)
        .is_some_and(|account| !tokens_differ(&account.tokens, &snapshot.tokens))
}

/// Pure decision helper backing the refresh commit CAS. Token advancement is
/// allowed only when the in-memory row still matches the network snapshot and,
/// for the Codex-owned active account mirrored into auth.json, the on-disk
/// credentials still match the snapshot too. A mismatch means an external
/// process replaced credentials after the snapshot; the stale refresh must not
/// overwrite them, while quota/subscription metadata may still be applied.
/// Credentials that are simply absent are not a mismatch: there is nothing on
/// disk to protect, so the refresh may advance and re-mirror auth.json.
pub(crate) fn refresh_may_update_tokens(
    data: &AppData,
    account_id: &str,
    source_snapshot: &Account,
    is_codex_owned: bool,
    external_auth_tokens: Option<&Tokens>,
) -> bool {
    if !tokens_match_snapshot(data, account_id, source_snapshot) {
        return false;
    }
    if !is_codex_owned {
        return true;
    }
    external_auth_tokens.is_none_or(|external| !tokens_differ(external, &source_snapshot.tokens))
}

/// Codex owns an account's credentials only while auth.json actually holds
/// them. Without credentials on disk the manager is the sole writer, so the
/// account has to refresh its own tokens; leaving it Codex-owned would freeze
/// them until the next manual re-login.
pub(crate) fn codex_owns_account(
    account: &Account,
    active_account: Option<&Account>,
    codex_auth_present: bool,
) -> bool {
    external_client_owns_account(account, active_account, codex_auth_present)
}

fn external_client_owns_account(
    account: &Account,
    active_account: Option<&Account>,
    auth_present: bool,
) -> bool {
    auth_present
        && active_account
            .is_some_and(|active_account| account_belongs_to_active_codex(account, active_account))
}

/// A blocked commit only loses work when the refresh actually produced
/// credentials that differ from the snapshot it started from. Quota refreshes
/// routinely return no new tokens at all — Codex-owned accounts never rotate
/// them here — and reporting those as a skipped token update tells the user
/// something was dropped when nothing was.
fn skipped_token_update(
    allow_token_update: bool,
    refreshed_tokens: Option<&Tokens>,
    source_snapshot: &Account,
) -> bool {
    !allow_token_update
        && refreshed_tokens.is_some_and(|tokens| tokens_differ(tokens, &source_snapshot.tokens))
}

fn quota_warning_for_cas(
    skipped_token_update: bool,
    quota_succeeded: bool,
) -> Option<QuotaRefreshWarning> {
    (skipped_token_update && quota_succeeded).then_some(QuotaRefreshWarning::TokenUpdateSkipped)
}

fn subscription_refresh_due(account: &Account, now: i64) -> bool {
    account
        .subscription_next_check_at
        .is_none_or(|next_check_at| next_check_at <= now)
}

#[cfg(test)]
mod tests {
    use crate::errors::AppError;
    use crate::models::{
        Account, AccountProvider, AppData, QuotaInfo, QuotaWindow, TokenHealth, TokenHealthStatus,
        Tokens,
    };
    use crate::quota::QuotaRefreshUpdate;
    use crate::subscription::{
        SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS, SUBSCRIPTION_UNSUPPORTED_RECHECK_SECONDS,
        SubscriptionInfo, SubscriptionRefreshUpdate,
    };

    use super::{
        account_belongs_to_active_codex, active_auth_for_update, apply_quota_update,
        apply_subscription_result, apply_subscription_update, codex_owns_account,
        matches_refresh_source, quota_warning_for_cas, refresh_gate_key, refresh_may_update_tokens,
        restore_credential_state, should_retry_after_reconcile,
        should_retry_subscription_after_reconcile, skipped_token_update, subscription_refresh_due,
        sync_related_account_tokens, tokens_differ,
    };

    fn account(id: &str) -> Account {
        Account {
            id: id.to_string(),
            provider: crate::models::AccountProvider::Codex,
            email: Some(format!("{id}@example.com")),
            account_id: Some(format!("openai-{id}")),
            provider_project_id: None,
            subscription_expires_at: Some(2_000_000_000),
            subscription_plan: Some("Plus".to_string()),
            subscription_detected_at: Some(1_900_000_000),
            subscription_checked_at: Some(1_900_000_000),
            subscription_next_check_at: Some(2_000_000_000),
            subscription_endpoint_hint: Some("account-payments".to_string()),
            tokens: Tokens {
                id_token: format!("id-{id}"),
                access_token: format!("access-{id}"),
                refresh_token: format!("refresh-{id}"),
            },
            token_expires_at: None,
            tokens_updated_at: Some(1_900_000_000),
            token_health: TokenHealth::healthy(),
            quota: None,
            quota_next_refresh_at: None,
            quota_refresh_failures: 0,
            created_at: 1_800_000_000,
            last_login_at: 1_900_000_000,
            last_error: None,
            subscription_error: None,
        }
    }

    fn quota_update(
        result: crate::errors::AppResult<QuotaInfo>,
        refreshed_tokens: Option<Tokens>,
    ) -> QuotaRefreshUpdate {
        QuotaRefreshUpdate {
            quota_result: result,
            refreshed_tokens: refreshed_tokens.clone(),
            token_health: refreshed_tokens.as_ref().map(|_| TokenHealth::refreshed()),
        }
    }

    #[test]
    fn subscription_lookup_failure_commits_state_and_becomes_warning() {
        let data = AppData {
            accounts: vec![account("source")],
            active_account_id: Some("source".to_string()),
            ..AppData::default()
        };
        let source = data.accounts[0].clone();
        let old_refresh_token = source.tokens.refresh_token.clone();
        let new_tokens = Tokens {
            id_token: "new-id".to_string(),
            access_token: "new-access".to_string(),
            refresh_token: "new-refresh".to_string(),
        };
        let update = SubscriptionRefreshUpdate {
            subscription_result: Err(AppError::msg(
                "Subscription auto-detect is unavailable for this account. Manual date was kept.",
            )),
            refreshed_tokens: Some(new_tokens.clone()),
            token_health: Some(TokenHealth::refreshed()),
        };

        let mut next = data.clone();
        let (active_auth, error) = apply_subscription_update(
            &data,
            &mut next,
            "source",
            &source,
            &old_refresh_token,
            update,
            true,
        )
        .expect("apply subscription update");

        assert!(active_auth.is_some());
        assert!(error.is_some());
        assert_eq!(next.accounts[0].tokens.refresh_token, "new-refresh");
        assert_eq!(
            next.accounts[0].token_health.status,
            TokenHealthStatus::Refreshed
        );
        assert!(next.accounts[0].subscription_checked_at.is_some());
        assert!(next.accounts[0].subscription_next_check_at.is_some());
        assert!(next.accounts[0].subscription_error.is_some());

        let result = super::subscription_refresh_result(next.clone(), error);
        assert!(result.warning.is_some());
        assert!(
            result
                .warning
                .as_deref()
                .unwrap()
                .contains("auto-detect is unavailable")
        );
        assert_eq!(result.state.accounts[0].tokens.refresh_token, "new-refresh");
    }

    #[test]
    fn successful_subscription_lookup_has_no_warning() {
        let data = AppData {
            accounts: vec![account("source")],
            active_account_id: Some("source".to_string()),
            ..AppData::default()
        };
        let source = data.accounts[0].clone();
        let update = SubscriptionRefreshUpdate {
            subscription_result: Ok(SubscriptionInfo {
                plan: Some("Pro x20".to_string()),
                expires_at: Some(2_100_000_000),
                fetched_at: 1_950_000_000,
                endpoint_hint: Some("payments".to_string()),
            }),
            refreshed_tokens: None,
            token_health: Some(TokenHealth::healthy()),
        };

        let mut next = data.clone();
        let (_, error) = apply_subscription_update(
            &data,
            &mut next,
            "source",
            &source,
            &source.tokens.refresh_token,
            update,
            true,
        )
        .expect("apply subscription update");

        assert!(error.is_none());
        assert_eq!(
            next.accounts[0].subscription_plan.as_deref(),
            Some("Pro x20")
        );
        assert_eq!(
            next.accounts[0].subscription_expires_at,
            Some(2_100_000_000)
        );
        assert_eq!(
            next.accounts[0].subscription_endpoint_hint.as_deref(),
            Some("payments")
        );
        assert!(next.accounts[0].subscription_error.is_none());
        assert!(
            super::subscription_refresh_result(next, error)
                .warning
                .is_none()
        );
    }

    #[test]
    fn subscription_errors_preserve_last_known_values() {
        let mut account = account("subscription");
        let previous_expiry = account.subscription_expires_at;
        let previous_detected_at = account.subscription_detected_at;
        let result = Err(AppError::msg("Subscription request timed out"));

        apply_subscription_result(&mut account, &result);

        assert_eq!(account.subscription_expires_at, previous_expiry);
        assert_eq!(account.subscription_detected_at, previous_detected_at);
        assert_eq!(account.subscription_plan.as_deref(), Some("Plus"));
    }

    #[test]
    fn refresh_source_matches_by_identity_or_old_token() {
        let source = account("source");
        let mut related = account("related");
        related.account_id = source.account_id.clone();
        assert!(matches_refresh_source(
            &related,
            &source,
            &source.tokens.refresh_token
        ));

        let mut email_related = account("email-related");
        email_related.account_id = None;
        email_related.email = Some("SOURCE@example.com".to_string());
        assert!(matches_refresh_source(
            &email_related,
            &source,
            &source.tokens.refresh_token
        ));

        let mut token_related = account("token-related");
        token_related.account_id = None;
        token_related.email = None;
        token_related.tokens.refresh_token = source.tokens.refresh_token.clone();
        assert!(matches_refresh_source(
            &token_related,
            &source,
            &source.tokens.refresh_token
        ));

        let unrelated = account("unrelated");
        assert!(!matches_refresh_source(
            &unrelated,
            &source,
            &source.tokens.refresh_token
        ));
    }

    #[test]
    fn refresh_identity_never_crosses_provider_boundaries() {
        let source = account("source");
        let mut google = source.clone();
        google.id = "google".to_string();
        google.provider = AccountProvider::Gemini;

        assert!(!matches_refresh_source(
            &google,
            &source,
            &source.tokens.refresh_token
        ));
        assert!(!account_belongs_to_active_codex(&google, &source));
        assert_ne!(
            refresh_gate_key(&google, None),
            refresh_gate_key(&source, None)
        );
    }

    #[test]
    fn related_account_tokens_are_synced_and_unrelated_are_untouched() {
        let source = account("source");
        let mut related = account("related");
        related.account_id = source.account_id.clone();
        let unrelated = account("unrelated");
        let new_tokens = Tokens {
            id_token: "new-id".to_string(),
            access_token: "new-access".to_string(),
            refresh_token: "new-refresh".to_string(),
        };
        let mut data = AppData {
            accounts: vec![source.clone(), related, unrelated],
            ..AppData::default()
        };

        let current = data.clone();
        sync_related_account_tokens(
            &current,
            &mut data,
            "source",
            &source,
            &source.tokens.refresh_token,
            &new_tokens,
            1_950_000_000,
        );

        assert_eq!(data.accounts[0].tokens.refresh_token, "refresh-source");
        assert_eq!(data.accounts[1].tokens.refresh_token, "new-refresh");
        assert_eq!(
            data.accounts[1].token_health.status,
            TokenHealthStatus::Refreshed
        );
        assert_eq!(data.accounts[2].tokens.refresh_token, "refresh-unrelated");
    }

    #[test]
    fn related_account_sync_skips_rows_changed_by_another_writer() {
        let source = account("source");
        let mut related = account("related");
        related.account_id = source.account_id.clone();
        let unrelated = account("unrelated");
        let new_tokens = Tokens {
            id_token: "new-id".to_string(),
            access_token: "new-access".to_string(),
            refresh_token: "new-refresh".to_string(),
        };
        let snapshot_data = AppData {
            accounts: vec![source.clone(), related.clone(), unrelated.clone()],
            ..AppData::default()
        };
        // Another writer refreshed this related row after the network snapshot.
        let mut current = snapshot_data.clone();
        current.accounts[1].tokens.refresh_token = "other-writer".to_string();
        let mut next = snapshot_data.clone();

        sync_related_account_tokens(
            &current,
            &mut next,
            "source",
            &source,
            &source.tokens.refresh_token,
            &new_tokens,
            1_950_000_000,
        );

        assert_eq!(next.accounts[1].tokens.refresh_token, "refresh-related");
        assert_eq!(
            next.accounts[1].token_health.status,
            TokenHealthStatus::Healthy
        );
    }

    #[test]
    fn snapshot_cas_detects_token_changes_since_snapshot() {
        let mut data = AppData {
            accounts: vec![account("source")],
            ..AppData::default()
        };
        let snapshot = data.accounts[0].clone();
        assert!(super::tokens_match_snapshot(&data, "source", &snapshot));

        data.accounts[0].tokens.refresh_token = "changed".to_string();
        assert!(!super::tokens_match_snapshot(&data, "source", &snapshot));
        assert!(!super::tokens_match_snapshot(&data, "missing", &snapshot));
    }

    #[test]
    fn cas_includes_external_auth_for_codex_owned_accounts() {
        let mut data = AppData {
            accounts: vec![account("source")],
            active_account_id: Some("source".to_string()),
            ..AppData::default()
        };
        let snapshot = data.accounts[0].clone();
        let snapshot_tokens = snapshot.tokens.clone();

        // External auth still holds the snapshot tokens -> refresh may advance.
        assert!(refresh_may_update_tokens(
            &data,
            "source",
            &snapshot,
            true,
            Some(&snapshot_tokens),
        ));

        // External auth replaced tokens -> stale refresh must not overwrite.
        let mut replaced = snapshot_tokens.clone();
        replaced.refresh_token = "external-refresh".to_string();
        assert!(!refresh_may_update_tokens(
            &data,
            "source",
            &snapshot,
            true,
            Some(&replaced),
        ));

        // No credentials on disk leaves nothing to protect, so the refresh may
        // advance and re-mirror auth.json instead of stalling forever.
        assert!(refresh_may_update_tokens(
            &data, "source", &snapshot, true, None,
        ));

        // Non-Codex-owned accounts are not gated by auth.json.
        assert!(refresh_may_update_tokens(
            &data, "source", &snapshot, false, None,
        ));
        assert!(refresh_may_update_tokens(
            &data,
            "source",
            &snapshot,
            false,
            Some(&replaced),
        ));

        // In-memory CAS still applies independently of the external file.
        data.accounts[0].tokens.refresh_token = "in-app-changed".to_string();
        assert!(!refresh_may_update_tokens(
            &data,
            "source",
            &snapshot,
            true,
            Some(&snapshot_tokens),
        ));
    }

    #[test]
    fn quota_cas_skip_is_partial_success_with_exact_warning() {
        assert_eq!(
            quota_warning_for_cas(true, true),
            Some(crate::refresh_service::QuotaRefreshWarning::TokenUpdateSkipped)
        );
        assert_eq!(quota_warning_for_cas(false, true), None);
        assert_eq!(quota_warning_for_cas(true, false), None);
    }

    #[test]
    fn blocked_commit_is_reported_only_when_new_tokens_were_dropped() {
        let source = account("source");
        let rotated = Tokens {
            id_token: "new-id".to_string(),
            access_token: "new-access".to_string(),
            refresh_token: "new-refresh".to_string(),
        };

        assert!(skipped_token_update(false, Some(&rotated), &source));
        // A quota refresh that rotated nothing has no update to lose.
        assert!(!skipped_token_update(false, None, &source));
        assert!(!skipped_token_update(
            false,
            Some(&source.tokens.clone()),
            &source
        ));
        assert!(!skipped_token_update(true, Some(&rotated), &source));
    }

    #[test]
    fn codex_owns_the_active_account_only_while_auth_json_holds_credentials() {
        let active = account("active");
        let other = account("other");

        assert!(codex_owns_account(&active, Some(&active), true));
        // auth.json holds nothing, so the manager must refresh these tokens
        // itself rather than waiting for Codex to rotate them.
        assert!(!codex_owns_account(&active, Some(&active), false));
        assert!(!codex_owns_account(&other, Some(&active), true));
        assert!(!codex_owns_account(&active, None, true));
    }

    #[test]
    fn quota_apply_suppresses_auth_write_when_cas_fails() {
        let data = AppData {
            accounts: vec![account("source")],
            active_account_id: Some("source".to_string()),
            ..AppData::default()
        };
        let source = data.accounts[0].clone();
        let quota = QuotaInfo {
            plan_type: Some("Plus".to_string()),
            primary: QuotaWindow::default(),
            secondary: QuotaWindow::default(),
            fetched_at: 1_950_000_000,
        };
        let new_tokens = Tokens {
            id_token: "new-id".to_string(),
            access_token: "new-access".to_string(),
            refresh_token: "new-refresh".to_string(),
        };

        let mut next = data.clone();
        let active_auth = apply_quota_update(
            &data,
            &mut next,
            "source",
            &source,
            &source.tokens.refresh_token,
            quota_update(Ok(quota), Some(new_tokens)),
            None,
            false,
        )
        .expect("apply quota update");

        // Quota metadata is applied, but tokens and the active auth write are
        // suppressed when the CAS fails.
        assert!(active_auth.is_none());
        assert!(next.accounts[0].quota.is_some());
        assert_eq!(next.accounts[0].tokens.refresh_token, "refresh-source");
        assert_eq!(
            next.accounts[0].token_health.status,
            TokenHealthStatus::Healthy
        );
    }

    #[test]
    fn auth_write_failure_fallback_restores_credentials_but_keeps_metadata() {
        let current = AppData {
            accounts: vec![account("source"), account("related")],
            ..AppData::default()
        };
        let mut next = current.clone();
        next.accounts[0].tokens.refresh_token = "advanced-source".to_string();
        next.accounts[0].token_health = TokenHealth::needs_relogin("stale health");
        next.accounts[0].quota = Some(QuotaInfo {
            plan_type: Some("Plus".to_string()),
            primary: QuotaWindow::default(),
            secondary: QuotaWindow::default(),
            fetched_at: 1_950_000_000,
        });
        next.accounts[1].tokens.refresh_token = "advanced-related".to_string();

        restore_credential_state(&current, &mut next);

        assert_eq!(
            next.accounts[0].tokens.refresh_token,
            current.accounts[0].tokens.refresh_token
        );
        assert_eq!(
            next.accounts[0].token_health.status,
            TokenHealthStatus::Healthy
        );
        assert_eq!(
            next.accounts[1].tokens.refresh_token,
            current.accounts[1].tokens.refresh_token
        );
        assert_eq!(
            next.accounts[0]
                .quota
                .as_ref()
                .map(|quota| quota.fetched_at),
            Some(1_950_000_000)
        );
    }

    #[test]
    fn active_auth_syncs_only_the_active_owner() {
        let source = account("source");
        let mut related = account("related");
        related.account_id = source.account_id.clone();
        let mut data = AppData {
            accounts: vec![source.clone(), related.clone()],
            active_account_id: Some("related".to_string()),
            ..AppData::default()
        };

        let auth = active_auth_for_update(
            &data,
            &data,
            "source",
            &source,
            &source.tokens.refresh_token,
            None,
        )
        .expect("active auth");
        assert_eq!(auth.0.refresh_token, "refresh-related");
        assert_eq!(auth.1.as_deref(), Some("openai-source"));

        data.active_account_id = Some("unrelated".to_string());
        assert!(
            active_auth_for_update(
                &data,
                &data,
                "source",
                &source,
                &source.tokens.refresh_token,
                None,
            )
            .is_none()
        );
    }

    #[test]
    fn quota_apply_sets_quota_and_clears_previous_error() {
        let mut data = AppData {
            accounts: vec![account("source")],
            ..AppData::default()
        };
        data.accounts[0].last_error = Some("previous failure".to_string());
        let source = data.accounts[0].clone();
        let quota = QuotaInfo {
            plan_type: Some("Plus".to_string()),
            primary: QuotaWindow::default(),
            secondary: QuotaWindow::default(),
            fetched_at: 1_950_000_000,
        };

        let mut next = data.clone();
        let active_auth = apply_quota_update(
            &data,
            &mut next,
            "source",
            &source,
            &source.tokens.refresh_token,
            quota_update(Ok(quota), None),
            None,
            true,
        )
        .expect("apply quota update");

        assert!(active_auth.is_none());
        assert!(next.accounts[0].quota.is_some());
        assert!(next.accounts[0].last_error.is_none());
        assert_eq!(next.accounts[0].subscription_plan.as_deref(), Some("Plus"));
    }

    #[test]
    fn quota_failure_records_error_and_marks_retry_after_reconcile() {
        let data = AppData {
            accounts: vec![account("source")],
            ..AppData::default()
        };
        let source = data.accounts[0].clone();
        let mut next = data.clone();

        let auth_error = quota_update(
            Err(AppError::msg("Quota request failed (401 Unauthorized)")),
            None,
        );
        assert!(should_retry_after_reconcile(&auth_error));
        apply_quota_update(
            &data,
            &mut next,
            "source",
            &source,
            &source.tokens.refresh_token,
            auth_error,
            None,
            true,
        )
        .expect("apply quota update");
        assert!(
            next.accounts[0]
                .last_error
                .as_deref()
                .unwrap()
                .contains("401")
        );

        let reuse_error = quota_update(
            Err(AppError::msg(
                "Your refresh token has already been used to generate a new access token",
            )),
            None,
        );
        assert!(should_retry_after_reconcile(&reuse_error));

        let transient = quota_update(
            Err(AppError::msg(
                "Quota request failed (500 Internal Server Error)",
            )),
            None,
        );
        assert!(!should_retry_after_reconcile(&transient));
    }

    #[test]
    fn subscription_reuse_or_auth_failure_triggers_reconcile_retry() {
        let reuse = SubscriptionRefreshUpdate {
            subscription_result: Err(AppError::msg(
                "Your refresh token has already been used to generate a new access token",
            )),
            refreshed_tokens: None,
            token_health: None,
        };
        assert!(should_retry_subscription_after_reconcile(&reuse));

        let auth = SubscriptionRefreshUpdate {
            subscription_result: Err(AppError::msg(
                "Subscription request failed (401 Unauthorized)",
            )),
            refreshed_tokens: None,
            token_health: None,
        };
        assert!(should_retry_subscription_after_reconcile(&auth));

        let health_reuse = SubscriptionRefreshUpdate {
            subscription_result: Err(AppError::msg("Subscription request failed (500)")),
            refreshed_tokens: None,
            token_health: Some(TokenHealth::warning(
                TokenHealthStatus::ServerError,
                "refresh token reuse detected",
            )),
        };
        assert!(should_retry_subscription_after_reconcile(&health_reuse));

        let transient = SubscriptionRefreshUpdate {
            subscription_result: Err(AppError::msg(
                "Subscription request failed (500 Internal Server Error)",
            )),
            refreshed_tokens: None,
            token_health: None,
        };
        assert!(!should_retry_subscription_after_reconcile(&transient));
    }

    #[test]
    fn subscription_schedule_is_driven_by_persisted_next_check() {
        let now = 1_000_000;
        let mut account = account("schedule");
        account.subscription_next_check_at = None;
        assert!(subscription_refresh_due(&account, now));
        account.subscription_next_check_at = Some(now + 1);
        assert!(!subscription_refresh_due(&account, now));
        assert!(subscription_refresh_due(&account, now + 1));
    }

    #[test]
    fn tokens_differ_detects_any_token_change() {
        let left = Tokens::default();
        let right = Tokens::default();
        assert!(!tokens_differ(&left, &right));

        let mut changed = right.clone();
        changed.id_token = "changed".to_string();
        assert!(tokens_differ(&left, &changed));
    }

    #[test]
    fn moved_subscription_schedule_constants_match_prior_behavior() {
        let now = 1_000_000;
        let plan_only = SubscriptionInfo {
            plan: Some("Free".to_string()),
            expires_at: None,
            fetched_at: now,
            endpoint_hint: None,
        };
        assert_eq!(
            crate::subscription::next_subscription_check(now, None, &Ok(plan_only)),
            now + 7 * 24 * 60 * 60
        );
        let unsupported = Err(AppError::msg(
            "Subscription auto-detect is unavailable (403 Forbidden)",
        ));
        assert_eq!(
            crate::subscription::next_subscription_check(now, None, &unsupported),
            now + SUBSCRIPTION_UNSUPPORTED_RECHECK_SECONDS
        );
        assert_eq!(SUBSCRIPTION_RECHECK_AFTER_EXPIRY_SECONDS, 24 * 60 * 60);
    }
}
