use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::atomic_file::{backup_path, write_atomic, write_atomic_with_backup};
use crate::errors::{AppError, AppResult};
use crate::models::{AppData, TokenHealth, Tokens, now_ts};
use crate::persisted::PersistedAppData;
use crate::secret_store;

const APP_STORAGE_DIR_NAME: &str = "VGCodexAccountManager";
const LEGACY_STORAGE_DIR_NAME: &str = "CodexAccountManagerLite";
const STATE_FILE_NAME: &str = "state.json";

fn base_data_dir() -> AppResult<PathBuf> {
    #[cfg(test)]
    {
        return Ok(std::env::temp_dir().join("switchai-test-app-data"));
    }
    #[allow(unreachable_code)]
    {
        dirs::data_local_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| AppError::msg("Cannot determine data directory"))
    }
}

pub fn app_storage_dir() -> AppResult<PathBuf> {
    let base = base_data_dir()?;
    let dir = base.join(APP_STORAGE_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|source| AppError::Io {
        context: "Failed to create data directory",
        source,
    })?;
    Ok(dir)
}

#[cfg(test)]
pub(crate) fn app_storage_dir_at(root: &Path) -> AppResult<PathBuf> {
    let dir = root.join(APP_STORAGE_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|source| AppError::Io {
        context: "Failed to create data directory",
        source,
    })?;
    Ok(dir)
}

#[cfg(test)]
pub(crate) fn app_storage_file_at(root: &Path) -> AppResult<PathBuf> {
    let path = app_storage_dir_at(root)?.join(STATE_FILE_NAME);
    migrate_legacy_storage_from(&path, root)?;
    Ok(path)
}

pub fn app_storage_file() -> AppResult<PathBuf> {
    let path = app_storage_dir()?.join(STATE_FILE_NAME);
    migrate_legacy_storage_if_needed(&path)?;
    Ok(path)
}

fn migrate_legacy_storage_if_needed(path: &Path) -> AppResult<()> {
    migrate_legacy_storage_from(path, &base_data_dir()?)
}

fn migrate_legacy_storage_from(path: &Path, base: &Path) -> AppResult<()> {
    if path.exists() {
        return Ok(());
    }

    let legacy_state = base.join(LEGACY_STORAGE_DIR_NAME).join(STATE_FILE_NAME);

    if !legacy_state.exists() {
        return Ok(());
    }

    let text = fs::read_to_string(&legacy_state).map_err(|source| AppError::Io {
        context: "Failed to read legacy state file",
        source,
    })?;
    write_atomic_with_backup(path, text.as_bytes(), true)?;
    log::info!("Migrated legacy state file to {}", path.display());
    Ok(())
}

fn read_persisted_state(path: &Path) -> AppResult<(PersistedAppData, String)> {
    let text = fs::read_to_string(path).map_err(|source| AppError::Io {
        context: "Failed to read state file",
        source,
    })?;
    let sanitized = text.trim_start_matches('\u{feff}');
    let parsed =
        serde_json::from_str::<PersistedAppData>(sanitized).map_err(|source| AppError::Json {
            context: "Failed to parse state file",
            source,
        })?;
    Ok((parsed, text))
}

fn quarantine_path_with_marker(path: &Path, marker: &str) -> AppResult<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::msg("State file has no valid file name"))?;
    Ok(path.with_file_name(format!(
        "{file_name}.{marker}-{}-{}",
        now_ts(),
        Uuid::new_v4()
    )))
}

fn quarantine_path(path: &Path) -> AppResult<PathBuf> {
    quarantine_path_with_marker(path, "corrupt")
}

fn quarantine_file_if_exists(path: &Path, marker: &str) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let quarantined = quarantine_path_with_marker(path, marker)?;
    fs::rename(path, &quarantined).map_err(|source| AppError::Io {
        context: "Failed to quarantine application state file",
        source,
    })?;
    Ok(())
}

fn load_app_data_from(path: &Path) -> AppResult<AppData> {
    if !path.exists() {
        let backup = backup_path(path)?;
        if !backup.exists() {
            return Ok(AppData::default());
        }

        let (restored, backup_text) = read_persisted_state(&backup)?;
        write_atomic(path, backup_text.as_bytes(), true)?;
        log::warn!("Restored missing state file from {}", backup.display());
        return Ok(AppData::from(restored).normalize_legacy());
    }

    match read_persisted_state(path) {
        Ok((data, _)) => Ok(AppData::from(data).normalize_legacy()),
        Err(primary_error) => {
            let backup = backup_path(path)?;
            let (restored, backup_text) =
                read_persisted_state(&backup).map_err(|backup_error| {
                    AppError::msg(format!(
                        "State recovery failed. Main file: {}; backup: {}",
                        primary_error.user_message(),
                        backup_error.user_message()
                    ))
                })?;
            let quarantined = quarantine_path(path)?;
            fs::rename(path, &quarantined).map_err(|source| AppError::Io {
                context: "Failed to preserve corrupted state file",
                source,
            })?;
            write_atomic(path, backup_text.as_bytes(), true)?;
            log::warn!(
                "Recovered state from {}. Corrupted state preserved at {}",
                backup.display(),
                quarantined.display()
            );
            Ok(AppData::from(restored).normalize_legacy())
        }
    }
}

fn tokens_present(tokens: &Tokens) -> bool {
    !tokens.id_token.trim().is_empty()
        || !tokens.access_token.trim().is_empty()
        || !tokens.refresh_token.trim().is_empty()
}

fn contains_legacy_quota_history(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|value| {
            value
                .get("accounts")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|accounts| {
                    accounts
                        .iter()
                        .any(|account| account.get("quotaHistory").is_some())
                })
        })
}

fn contains_legacy_plaintext_tokens(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|value| {
            value
                .get("accounts")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|accounts| {
                    accounts.iter().any(|account| {
                        account
                            .get("tokens")
                            .and_then(serde_json::Value::as_object)
                            .is_some_and(|tokens| {
                                ["idToken", "accessToken", "refreshToken"]
                                    .into_iter()
                                    .filter_map(|key| tokens.get(key))
                                    .filter_map(serde_json::Value::as_str)
                                    .any(|token| !token.trim().is_empty())
                            })
                    })
                })
        })
}

fn recover_legacy_tokens_from_backup(path: &Path, data: &mut AppData) -> AppResult<bool> {
    if !contains_legacy_plaintext_tokens(path) {
        return Ok(false);
    }
    let (persisted, _) = read_persisted_state(path)?;
    let backup_data = AppData::from(persisted);
    let mut recovered = false;
    for account in &mut data.accounts {
        if tokens_present(&account.tokens) {
            continue;
        }
        if let Some(backup_account) = backup_data
            .accounts
            .iter()
            .find(|candidate| candidate.id == account.id && tokens_present(&candidate.tokens))
        {
            account.tokens = backup_account.tokens.clone();
            recovered = true;
        }
    }
    Ok(recovered)
}

fn hydrate_protected_tokens(data: &mut AppData) -> AppResult<bool> {
    let has_legacy_tokens = data
        .accounts
        .iter()
        .any(|account| tokens_present(&account.tokens));

    if has_legacy_tokens {
        // Migrate all credentials before the plaintext state is touched. A partial
        // migration must never make the original file unusable.
        for account in &data.accounts {
            if tokens_present(&account.tokens) {
                secret_store::store_tokens(&account.id, &account.tokens)?;
            }
        }
    }

    let mut vault_tokens = secret_store::load_all_tokens();

    for account in &mut data.accounts {
        if tokens_present(&account.tokens) {
            continue;
        }
        match &mut vault_tokens {
            Ok(tokens_map) => match tokens_map.remove(&account.id) {
                Some(tokens) => account.tokens = tokens,
                None => {
                    let message = "Protected tokens are missing. Sign in to this account again.";
                    account.token_health = TokenHealth::needs_relogin(message);
                    account.last_error = Some(message.to_string());
                }
            },
            Err(error) => {
                let message = error.user_message();
                account.token_health = TokenHealth::needs_relogin(message.clone());
                account.last_error = Some(message);
            }
        }
    }

    Ok(has_legacy_tokens)
}

fn write_serialized_state(path: &Path, data: &AppData, with_backup: bool) -> AppResult<()> {
    let persisted = PersistedAppData::from(data);
    let text = serde_json::to_string_pretty(&persisted).map_err(|source| AppError::Json {
        context: "Failed to serialize state",
        source,
    })?;
    if with_backup {
        write_atomic_with_backup(path, text.as_bytes(), true)
    } else {
        write_atomic(path, text.as_bytes(), true)
    }
}

pub fn load_app_data() -> AppResult<AppData> {
    let path = app_storage_file()?;
    let base = base_data_dir()?;
    let migrated_from_legacy = migrated_legacy_source_at(&path, &base);
    let mut data = load_app_data_from(&path)?;
    let schema_needs_upgrade = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("schemaVersion")
                .and_then(serde_json::Value::as_u64)
        })
        .is_some_and(|version| version < u64::from(crate::models::APP_SCHEMA_VERSION));
    let had_legacy_quota_history = contains_legacy_quota_history(&path);
    let backup = backup_path(&path)?;
    let backup_had_plaintext_tokens = contains_legacy_plaintext_tokens(&backup);
    let recovered_backup_tokens = recover_legacy_tokens_from_backup(&backup, &mut data)?;
    let migrated_tokens = hydrate_protected_tokens(&mut data)? || recovered_backup_tokens;
    // Reaching this point means the authoritative state parsed and its protected
    // credentials hydrated successfully. A leftover legacy source can therefore
    // be retired even when a prior run already wrote a token-free v9 state.
    let mut retire_legacy = migrated_from_legacy.is_some();
    if migrated_tokens
        || backup_had_plaintext_tokens
        || had_legacy_quota_history
        || schema_needs_upgrade
    {
        // A plaintext backup is removed only after the protected vault was
        // hydrated and the new token-free primary state was durably written.
        write_serialized_state(&path, &data, false)?;
        if (migrated_tokens || backup_had_plaintext_tokens || had_legacy_quota_history)
            && backup.exists()
        {
            if let Err(error) = write_serialized_state(&backup, &data, false) {
                log::warn!(
                    "Could not sanitize state backup during migration: {}",
                    error.user_message()
                );
            }
            retire_legacy = true;
        }
        if migrated_tokens {
            log::info!("Migrated account tokens to protected operating-system storage");
        }
        if backup_had_plaintext_tokens {
            log::info!("Removed a legacy plaintext-token state backup");
        }
        if had_legacy_quota_history {
            log::info!("Removed unused quota history from persisted state");
        }
        if schema_needs_upgrade {
            log::info!(
                "Persisted application state schema v{} migration",
                crate::models::APP_SCHEMA_VERSION
            );
        }
    }
    if migrated_from_legacy.is_some() {
        // The legacy source is only retired once the new token-free state and its
        // plaintext backup are durably gone. A failed vault migration leaves the
        // original legacy files untouched so the next launch can retry.
        finalize_legacy_migration(&path, &base, retire_legacy)?;
    }
    Ok(data)
}

fn migrated_legacy_source_at(_path: &Path, base: &Path) -> Option<PathBuf> {
    let legacy_state = base.join(LEGACY_STORAGE_DIR_NAME).join(STATE_FILE_NAME);
    legacy_state.exists().then_some(legacy_state)
}

fn finalize_legacy_migration(path: &Path, base: &Path, retire_legacy: bool) -> AppResult<()> {
    let Some(legacy_state) = migrated_legacy_source_at(path, base) else {
        return Ok(());
    };
    if !retire_legacy {
        return Ok(());
    }
    let legacy_backup = backup_path(&legacy_state)?;
    quarantine_file_if_exists(&legacy_state, "legacy")?;
    quarantine_file_if_exists(&legacy_backup, "legacy")?;
    log::info!("Retired legacy state files after successful durable migration");
    Ok(())
}

fn rollback_secrets(changed_secrets: &[(String, Option<Tokens>)]) -> Vec<String> {
    let mut failures = Vec::new();
    for (account_id, previous) in changed_secrets.iter().rev() {
        let result = match previous {
            Some(tokens) if tokens_present(tokens) => {
                secret_store::store_tokens(account_id, tokens)
            }
            _ => secret_store::delete_tokens(account_id),
        };
        if let Err(error) = result {
            failures.push(format!("{account_id}: {}", error.user_message()));
        }
    }
    failures
}

fn with_rollback_error(primary: AppError, failures: Vec<String>) -> AppError {
    if failures.is_empty() {
        return primary;
    }
    AppError::msg(format!(
        "{}; failed to roll back protected tokens: {}",
        primary.user_message(),
        failures.join("; ")
    ))
}

pub(crate) fn persist_app_data_at(current: &AppData, next: &AppData, path: &Path) -> AppResult<()> {
    let mut changed_secrets: Vec<(String, Option<Tokens>)> = Vec::new();

    for next_account in &next.accounts {
        if !tokens_present(&next_account.tokens) {
            continue;
        }
        let previous = current
            .accounts
            .iter()
            .find(|account| account.id == next_account.id)
            .map(|account| account.tokens.clone());
        if previous.as_ref() == Some(&next_account.tokens) {
            continue;
        }

        if let Err(error) = secret_store::store_tokens(&next_account.id, &next_account.tokens) {
            let failures = rollback_secrets(&changed_secrets);
            return Err(with_rollback_error(error, failures));
        }
        changed_secrets.push((next_account.id.clone(), previous));
    }

    if let Err(error) = write_serialized_state(path, next, true) {
        let failures = rollback_secrets(&changed_secrets);
        return Err(with_rollback_error(error, failures));
    }

    for removed in current.accounts.iter().filter(|account| {
        !next
            .accounts
            .iter()
            .any(|candidate| candidate.id == account.id)
    }) {
        if let Err(error) = secret_store::delete_tokens(&removed.id) {
            // The state no longer references this credential. An orphan is safer than
            // rolling back a successfully committed account deletion.
            log::warn!(
                "Could not remove orphaned protected tokens for account {}: {}",
                removed.id,
                error.user_message()
            );
        }
    }

    Ok(())
}

pub fn persist_app_data(current: &AppData, next: &AppData) -> AppResult<()> {
    let path = app_storage_file()?;
    persist_app_data_at(current, next, &path)
}

pub fn commit_app_data(current: &mut AppData, next: AppData) -> AppResult<()> {
    persist_app_data(current, &next)?;
    let mut next = next;
    next.revision = current.revision.saturating_add(1);
    *current = next;
    Ok(())
}

pub(crate) fn commit_state_data_at(
    state: &std::sync::Arc<crate::app_state::SharedState>,
    next: AppData,
    path: &Path,
) -> AppResult<AppData> {
    let _commit_guard = state
        .commit_gate
        .lock()
        .map_err(|_| AppError::msg("State commit gate poisoned"))?;

    let current_snapshot = {
        let data = crate::app_state::lock_data(state)?;
        data.clone()
    };

    persist_app_data_at(&current_snapshot, &next, path)?;

    let committed_state = {
        let mut data = crate::app_state::lock_data(state)?;
        let mut next = next;
        next.revision = data.revision.saturating_add(1);
        *data = next.clone();
        next
    };

    crate::tray_dashboard::refresh_dashboard_and_alerts(state);
    Ok(committed_state)
}

pub fn commit_state_data(
    state: &std::sync::Arc<crate::app_state::SharedState>,
    next: AppData,
) -> AppResult<AppData> {
    let path = app_storage_file()?;
    commit_state_data_at(state, next, &path)
}

#[derive(Debug, Clone)]
pub struct RecoveryStatus {
    pub data_directory: PathBuf,
    pub state_path: PathBuf,
    pub backup_path: PathBuf,
    pub backup_available: bool,
}

pub fn recovery_status() -> AppResult<RecoveryStatus> {
    let data_directory = app_storage_dir()?;
    let state_path = data_directory.join(STATE_FILE_NAME);
    let backup_path = backup_path(&state_path)?;
    let backup_available = backup_path.is_file();
    Ok(RecoveryStatus {
        data_directory,
        state_path,
        backup_path,
        backup_available,
    })
}

fn restore_app_data_backup_from(path: &Path) -> AppResult<AppData> {
    let backup = backup_path(path)?;
    let (restored, backup_text) = read_persisted_state(&backup).map_err(|error| {
        AppError::msg(format!(
            "State backup cannot be restored: {}",
            error.user_message()
        ))
    })?;
    quarantine_file_if_exists(path, "corrupt")?;
    write_atomic(path, backup_text.as_bytes(), true)?;
    log::warn!(
        "Restored state from backup {}; previous primary state preserved",
        backup.display()
    );
    Ok(AppData::from(restored).normalize_legacy())
}

pub fn restore_app_data_backup() -> AppResult<AppData> {
    let status = recovery_status()?;
    restore_app_data_backup_from(&status.state_path)?;
    load_app_data()
}

pub fn start_fresh_app_data() -> AppResult<AppData> {
    let status = recovery_status()?;
    quarantine_file_if_exists(&status.state_path, "reset")?;
    quarantine_file_if_exists(&status.backup_path, "reset")?;
    secret_store::clear_all_tokens()?;
    log::warn!(
        "Started with fresh application state; previous state files were quarantined and the protected token vault was cleared"
    );
    Ok(AppData::default())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use uuid::Uuid;

    use super::{
        LEGACY_STORAGE_DIR_NAME, STATE_FILE_NAME, app_storage_dir_at, app_storage_file_at,
        contains_legacy_plaintext_tokens, contains_legacy_quota_history, finalize_legacy_migration,
        load_app_data_from, quarantine_file_if_exists, restore_app_data_backup_from,
        write_serialized_state,
    };
    use crate::atomic_file::backup_path;
    use crate::errors::AppResult;
    use crate::models::{AppData, TokenHealth, Tokens};
    use crate::persisted::{PersistedAccount, PersistedAppData};

    fn test_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("vg-storage-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    fn save_app_data_to(path: &std::path::Path, data: &AppData) -> crate::errors::AppResult<()> {
        let text =
            serde_json::to_string_pretty(&PersistedAppData::from(data)).map_err(|source| {
                crate::errors::AppError::Json {
                    context: "Failed to serialize test state",
                    source,
                }
            })?;
        crate::atomic_file::write_atomic_with_backup(path, text.as_bytes(), true)
    }

    fn account_with_tokens() -> crate::models::Account {
        crate::models::Account {
            id: "account-1".to_string(),
            provider: crate::models::AccountProvider::Codex,
            email: Some("user@example.com".to_string()),
            account_id: Some("openai-1".to_string()),
            provider_project_id: None,
            subscription_expires_at: None,
            subscription_plan: None,
            subscription_detected_at: None,
            subscription_checked_at: None,
            subscription_next_check_at: None,
            subscription_endpoint_hint: None,
            tokens: Tokens {
                id_token: "secret-id".to_string(),
                access_token: "secret-access".to_string(),
                refresh_token: "secret-refresh".to_string(),
            },
            token_expires_at: None,
            tokens_updated_at: Some(1),
            token_health: TokenHealth::healthy(),
            quota: None,
            quota_next_refresh_at: None,
            quota_refresh_failures: 0,
            created_at: 1,
            last_login_at: 1,
            last_error: None,
            subscription_error: None,
        }
    }

    #[test]
    fn recovers_corrupted_state_from_backup() {
        let dir = test_dir();
        let path = dir.join("state.json");
        let account = account_with_tokens();
        let first = AppData {
            accounts: vec![account.clone()],
            active_account_id: Some(account.id.clone()),
            ..AppData::default()
        };
        let mut second = first.clone();
        second.active_account_id = Some("newer".to_string());

        save_app_data_to(&path, &first).expect("save first state");
        save_app_data_to(&path, &second).expect("save second state");
        fs::write(&path, "{broken").expect("corrupt main state");

        let restored = load_app_data_from(&path).expect("recover state");
        assert_eq!(
            restored.active_account_id.as_deref(),
            Some(account.id.as_str())
        );
        assert!(
            fs::read_dir(&dir)
                .expect("list directory")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
        );
        assert!(backup_path(&path).expect("backup path").exists());
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn reports_when_main_and_backup_are_both_corrupt() {
        let dir = test_dir();
        let path = dir.join("state.json");
        fs::write(&path, "{broken").expect("write corrupt main");
        fs::write(backup_path(&path).expect("backup path"), "{also-broken")
            .expect("write corrupt backup");

        let error = load_app_data_from(&path).expect_err("recovery must fail");
        assert!(error.user_message().contains("State recovery failed"));
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn persistence_omits_secrets() {
        let dir = test_dir();
        let path = dir.join("state.json");
        let mut data = AppData::default();
        data.accounts.push(account_with_tokens());

        write_serialized_state(&path, &data, false).expect("persist state");
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read persisted state"))
                .expect("parse persisted state");

        let accounts = value.get("accounts").and_then(|value| value.as_array());
        assert_eq!(accounts.map(Vec::len), Some(1));
        assert!(!value.to_string().contains("secret-access"));
        assert!(!value.to_string().contains("\"tokens\""));
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn detects_legacy_quota_history_only_as_an_account_field() {
        let dir = test_dir();
        let path = dir.join("state.json");
        fs::write(
            &path,
            json!({
                "accounts": [{ "id": "account-1", "quotaHistory": [{ "fetchedAt": 1 }] }]
            })
            .to_string(),
        )
        .expect("write legacy quota history");
        assert!(contains_legacy_quota_history(&path));

        fs::write(
            &path,
            json!({
                "accounts": [{ "id": "account-1", "lastError": "quotaHistory" }]
            })
            .to_string(),
        )
        .expect("write state without quota history field");
        assert!(!contains_legacy_quota_history(&path));
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn detects_plaintext_tokens_in_a_legacy_backup() {
        let dir = test_dir();
        let path = dir.join("state.json.bak");
        fs::write(
            &path,
            json!({
                "accounts": [{
                    "id": "account-1",
                    "tokens": {
                        "idToken": "",
                        "accessToken": "legacy-access",
                        "refreshToken": "legacy-refresh"
                    }
                }]
            })
            .to_string(),
        )
        .expect("write legacy token backup");
        assert!(contains_legacy_plaintext_tokens(&path));

        fs::write(
            &path,
            json!({ "accounts": [{ "id": "account-1" }] }).to_string(),
        )
        .expect("write token-free backup");
        assert!(!contains_legacy_plaintext_tokens(&path));
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn legacy_plaintext_tokens_survive_parse_and_are_not_reserialized() {
        let dir = test_dir();
        let path = dir.join("state.json");
        fs::write(
            &path,
            json!({
                "schemaVersion": 7,
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
            })
            .to_string(),
        )
        .expect("write legacy state");

        let restored = load_app_data_from(&path).expect("parse legacy state");
        assert_eq!(restored.schema_version, crate::models::APP_SCHEMA_VERSION);
        assert_eq!(restored.accounts[0].tokens.access_token, "legacy-access");

        let reserialized = serde_json::to_string(&PersistedAppData::from(&restored))
            .expect("reserialize legacy state");
        assert!(!reserialized.contains("legacy-access"));
        assert!(!reserialized.contains("\"tokens\""));
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn persisted_account_dto_round_trips_runtime_account() {
        let account = account_with_tokens();
        let persisted = PersistedAccount::from(&account);
        let restored = crate::models::Account::from(persisted);

        assert_eq!(restored.id, account.id);
        assert_eq!(restored.tokens.access_token, "secret-access");
    }

    #[test]
    fn commit_bumps_revision_only_after_successful_persistence() {
        let dir = test_dir();
        let path = dir.join("state.json");
        let first = AppData::default();
        write_serialized_state(&path, &first, false).expect("persist first state");

        let mut current = first;
        let mut next = current.clone();
        next.active_account_id = Some("account-1".to_string());
        let next_original_revision = next.revision;
        commit_app_data_at(&mut current, next, &path).expect("commit state");
        assert_eq!(current.revision, 1);

        let serialized = serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(&path).expect("read committed state"),
        )
        .expect("parse committed state");
        assert!(serialized.get("revision").is_none());
        assert_eq!(next_original_revision, 0);
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn legacy_quarantine_happens_only_after_new_state_is_durable() {
        let dir = test_dir();
        let legacy_dir = dir.join(LEGACY_STORAGE_DIR_NAME);
        fs::create_dir_all(&legacy_dir).expect("create legacy directory");
        let legacy_state = legacy_dir.join(STATE_FILE_NAME);
        fs::write(&legacy_state, b"{\"schemaVersion\":8}").expect("write legacy state");
        fs::write(
            backup_path(&legacy_state).expect("legacy backup path"),
            b"{\"schemaVersion\":8}",
        )
        .expect("write legacy backup");

        let path = app_storage_file_at(&dir).expect("migrate legacy state");
        assert_eq!(
            path,
            app_storage_dir_at(&dir)
                .expect("app dir")
                .join(STATE_FILE_NAME)
        );
        assert!(legacy_state.exists());
        assert!(
            backup_path(&legacy_state)
                .expect("legacy backup path")
                .exists()
        );
        assert!(path.exists());
        assert!(!backup_path(&path).expect("new backup path").exists());

        write_serialized_state(&path, &AppData::default(), false).expect("write token-free state");
        finalize_legacy_migration(&path, &dir, true).expect("finalize migration");
        assert!(!legacy_state.exists());
        assert!(
            !backup_path(&legacy_state)
                .expect("legacy backup path")
                .exists()
        );
        assert!(
            fs::read_dir(&legacy_dir)
                .expect("list legacy directory")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains(".legacy-"))
        );
        let migrated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read migrated state"))
                .expect("parse migrated state");
        assert_eq!(migrated["schemaVersion"], crate::models::APP_SCHEMA_VERSION);
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn legacy_source_survives_failed_migration_finalize() {
        let dir = test_dir();
        let legacy_dir = dir.join(LEGACY_STORAGE_DIR_NAME);
        fs::create_dir_all(&legacy_dir).expect("create legacy directory");
        let legacy_state = legacy_dir.join(STATE_FILE_NAME);
        fs::write(&legacy_state, b"{\"schemaVersion\":8}").expect("write legacy state");
        fs::write(
            backup_path(&legacy_state).expect("legacy backup path"),
            b"{\"schemaVersion\":8}",
        )
        .expect("write legacy backup");
        let path = app_storage_file_at(&dir).expect("migrate legacy state");

        finalize_legacy_migration(&path, &dir, false).expect("skip legacy finalize");

        assert!(legacy_state.exists());
        assert!(
            backup_path(&legacy_state)
                .expect("legacy backup path")
                .exists()
        );
        assert!(
            fs::read_to_string(&path)
                .expect("read new state")
                .contains("schemaVersion")
        );
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn restore_backup_validates_then_quarantines_primary() {
        let dir = test_dir();
        let path = dir.join("state.json");
        fs::write(&path, "{broken").expect("write corrupt primary");
        fs::write(
            backup_path(&path).expect("backup path"),
            b"{\"schemaVersion\":9}",
        )
        .expect("write valid backup");

        let restored = restore_app_data_backup_from(&path).expect("restore from backup");
        assert_eq!(restored.schema_version, crate::models::APP_SCHEMA_VERSION);
        assert!(
            fs::read_dir(&dir)
                .expect("list directory")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
        );
        assert!(
            fs::read_to_string(&path)
                .expect("read restored state")
                .contains("\"schemaVersion\":9")
        );
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn start_fresh_quarantines_owned_state_without_touching_auth() {
        let dir = test_dir();
        let app_dir = app_storage_dir_at(&dir).expect("create app directory");
        let path = app_dir.join(STATE_FILE_NAME);
        fs::write(&path, b"old").expect("write old state");
        fs::write(backup_path(&path).expect("backup path"), b"old-backup")
            .expect("write old backup");
        let auth = dir.join("auth.json");
        fs::write(&auth, b"do-not-touch").expect("write auth fixture");

        let fresh = start_fresh_app_data_in(&dir).expect("start fresh");
        assert_eq!(fresh.active_account_id, None);
        assert!(fresh.accounts.is_empty());
        assert!(!path.exists());
        assert!(!backup_path(&path).expect("backup path").exists());
        assert_eq!(fs::read(&auth).expect("read auth fixture"), b"do-not-touch");
        assert_eq!(
            fs::read_dir(&app_dir).expect("list app directory").count(),
            2
        );
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    fn start_fresh_app_data_in(root: &std::path::Path) -> AppResult<AppData> {
        let dir = app_storage_dir_at(root)?;
        let state_path = dir.join(STATE_FILE_NAME);
        quarantine_file_if_exists(&state_path, "reset")?;
        quarantine_file_if_exists(&backup_path(&state_path)?, "reset")?;
        Ok(AppData::default())
    }

    fn commit_app_data_at(
        current: &mut AppData,
        next: AppData,
        path: &std::path::Path,
    ) -> AppResult<()> {
        write_serialized_state(path, &next, true)?;
        let mut next = next;
        next.revision = current.revision.saturating_add(1);
        *current = next;
        Ok(())
    }

    #[test]
    fn commit_state_data_updates_revision_and_state() {
        let dir = test_dir();
        let path = dir.join("state.json");
        let initial = AppData {
            revision: 3,
            ..AppData::default()
        };
        let state = std::sync::Arc::new(
            crate::app_state::SharedState::new_with_startup_error(initial, None).unwrap(),
        );

        let mut next = crate::app_state::lock_data(&state).unwrap().clone();
        next.app_settings.auto_check_updates = false;

        let committed =
            super::commit_state_data_at(&state, next, &path).expect("commit state data");
        assert_eq!(committed.revision, 4);
        assert!(!committed.app_settings.auto_check_updates);

        let in_state = crate::app_state::lock_data(&state).unwrap().clone();
        assert_eq!(in_state.revision, 4);
        assert!(!in_state.app_settings.auto_check_updates);
        let _ = fs::remove_dir_all(dir);
    }
}
