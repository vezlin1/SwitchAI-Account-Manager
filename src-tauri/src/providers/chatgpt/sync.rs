use std::fs;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::process::Command;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::thread;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::atomic_file::write_atomic;
use crate::errors::{AppError, AppResult};
use crate::models::{AccountProvider, AppData, TokenHealth, Tokens, now_ts};
use crate::storage::commit_app_data;
use crate::token_utils::{extract_account_id, extract_email};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(target_os = "windows")]
const CHATGPT_PROCESS_IMAGE: &str = "ChatGPT.exe";
#[cfg(target_os = "windows")]
const CHATGPT_APP_TARGET: &str = r"shell:AppsFolder\OpenAI.Codex_2p2nqsd0c76g0!App";
#[cfg(target_os = "windows")]
const CHATGPT_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(target_os = "windows")]
const CHATGPT_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(100);

fn codex_auth_path() -> AppResult<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| AppError::msg("Cannot determine user home directory"))?;
    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).map_err(|source| AppError::Io {
        context: "Failed to create .codex directory",
        source,
    })?;
    Ok(codex_dir.join("auth.json"))
}

#[cfg(test)]
fn codex_auth_path_at(root: &Path) -> PathBuf {
    root.join(".codex").join("auth.json")
}

#[derive(Debug, Clone)]
pub struct CodexAuthSnapshot {
    pub tokens: Tokens,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub last_refresh_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CodexAuthFile {
    #[serde(default)]
    tokens: Option<CodexAuthTokens>,
    #[serde(default)]
    last_refresh: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct CodexAuthTokens {
    #[serde(default)]
    id_token: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    account_id: Option<String>,
}

fn parse_last_refresh(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::String(text) => DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|date| date.timestamp()),
        Value::Number(number) => number.as_i64(),
        _ => None,
    }
}

fn read_codex_auth_at(path: &Path) -> AppResult<Option<CodexAuthSnapshot>> {
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path).map_err(|source| AppError::Io {
        context: "Failed to read auth.json",
        source,
    })?;
    let parsed: CodexAuthFile = serde_json::from_str(&text).map_err(|source| AppError::Json {
        context: "Failed to parse auth.json",
        source,
    })?;

    let Some(auth_tokens) = parsed.tokens else {
        return Ok(None);
    };

    if auth_tokens.access_token.trim().is_empty() || auth_tokens.refresh_token.trim().is_empty() {
        return Ok(None);
    }

    let tokens = Tokens {
        id_token: auth_tokens.id_token,
        access_token: auth_tokens.access_token,
        refresh_token: auth_tokens.refresh_token,
    };
    let account_id = auth_tokens
        .account_id
        .filter(|value| !value.trim().is_empty())
        .or_else(|| extract_account_id(&tokens.id_token));
    let email = extract_email(&tokens.id_token);
    let last_refresh_at = parse_last_refresh(parsed.last_refresh.as_ref());

    Ok(Some(CodexAuthSnapshot {
        tokens,
        account_id,
        email,
        last_refresh_at,
    }))
}

pub fn read_codex_auth() -> AppResult<Option<CodexAuthSnapshot>> {
    let path = codex_auth_path()?;
    read_codex_auth_at(&path)
}

fn tokens_differ(left: &Tokens, right: &Tokens) -> bool {
    left.access_token != right.access_token
        || left.refresh_token != right.refresh_token
        || left.id_token != right.id_token
}

pub fn reconcile_codex_auth(data: &mut AppData) -> AppResult<bool> {
    let Some(snapshot) = read_codex_auth()? else {
        return Ok(false);
    };

    Ok(reconcile_codex_auth_from_snapshot(data, Some(&snapshot)))
}

pub fn reconcile_codex_auth_transactionally(data: &mut AppData) -> AppResult<bool> {
    reconcile_codex_auth_with_snapshot(data, read_codex_auth()?.as_ref())
}

/// Transactional reconciliation against an auth snapshot the caller already
/// read. Sharing one read keeps a single refresh from reconciling against one
/// version of auth.json and then deciding ownership against another.
pub fn reconcile_codex_auth_with_snapshot(
    data: &mut AppData,
    snapshot: Option<&CodexAuthSnapshot>,
) -> AppResult<bool> {
    let mut next = data.clone();
    if !reconcile_codex_auth_from_snapshot(&mut next, snapshot) {
        return Ok(false);
    }
    commit_app_data(data, next)?;
    Ok(true)
}

/// Startup-only reconciliation. Applies an already-read auth snapshot to `data`
/// and reports whether `data` changed. Never reads or writes auth.json itself.
pub fn reconcile_codex_auth_from_snapshot(
    data: &mut AppData,
    snapshot: Option<&CodexAuthSnapshot>,
) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };

    let snapshot_account_id = snapshot
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let matched_index = match snapshot_account_id {
        Some(snapshot_account_id) => data.accounts.iter().position(|account| {
            account.provider == AccountProvider::Codex
                && account.account_id.as_deref().map(str::trim) == Some(snapshot_account_id)
        }),
        None => data.accounts.iter().position(|account| {
            account.provider == AccountProvider::Codex
                && account
                    .email
                    .as_deref()
                    .zip(snapshot.email.as_deref())
                    .is_some_and(|(left, right)| left.trim().eq_ignore_ascii_case(right.trim()))
        }),
    };

    let Some(index) = matched_index else {
        // An unknown external account_id or email means the manager must not keep a
        // different account selected.
        if (snapshot_account_id.is_some() || snapshot.email.is_some())
            && data.active_account_id.is_some()
        {
            data.active_account_id = None;
            return true;
        }
        return false;
    };

    let mut changed = false;
    let active_id = data.active_account_id.clone();

    if active_id.as_deref() != Some(data.accounts[index].id.as_str()) {
        data.active_account_id = Some(data.accounts[index].id.clone());
        changed = true;
    }

    let account = &mut data.accounts[index];
    if account.account_id.is_none()
        && let Some(account_id) = snapshot_account_id
    {
        account.account_id = Some(account_id.to_string());
        changed = true;
    }
    if account.email.is_none()
        && let Some(email) = snapshot.email.as_deref()
        && !email.trim().is_empty()
    {
        account.email = Some(email.to_string());
        changed = true;
    }
    let snapshot_ts = snapshot.last_refresh_at.unwrap_or_else(now_ts);
    let account_ts = account.tokens_updated_at.unwrap_or(account.last_login_at);
    if snapshot_ts >= account_ts && tokens_differ(&account.tokens, &snapshot.tokens) {
        account.tokens = snapshot.tokens.clone();
        account.tokens_updated_at = Some(snapshot_ts);
        account.token_health = TokenHealth::healthy();
        account.last_error = None;
        changed = true;
    }

    changed
}

/// Reads auth.json once at startup, reconciles the saved accounts, and persists
/// only when the snapshot changed anything. auth.json is never written here.
pub fn reconcile_codex_auth_at_startup(data: &mut AppData) -> AppResult<bool> {
    let snapshot = read_codex_auth()?;

    let Some(snapshot) = snapshot else {
        return Ok(false);
    };

    let mut next = data.clone();
    if !reconcile_codex_auth_from_snapshot(&mut next, Some(&snapshot)) {
        return Ok(false);
    }
    commit_app_data(data, next)?;
    Ok(true)
}

fn existing_auth_document(path: &Path) -> AppResult<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = fs::read_to_string(path).map_err(|source| AppError::Io {
        context: "Failed to read auth.json before updating it",
        source,
    })?;
    match serde_json::from_str::<Value>(&text) {
        Ok(value @ Value::Object(_)) => Ok(value),
        Ok(_) => Err(AppError::msg(
            "auth.json is not a JSON object; refusing to overwrite it",
        )),
        Err(source) => Err(AppError::Json {
            context: "Failed to parse auth.json before updating it",
            source,
        }),
    }
}

fn write_codex_auth_at(path: &Path, tokens: &Tokens, account_id: Option<&str>) -> AppResult<()> {
    let mut data = existing_auth_document(path)?;
    let mut token_document = match data.get("tokens") {
        Some(Value::Object(tokens)) => tokens.clone(),
        _ => serde_json::Map::new(),
    };
    token_document.retain(|key, _| {
        !matches!(
            key.as_str(),
            "id_token" | "access_token" | "refresh_token" | "account_id"
        )
    });
    token_document.insert("id_token".to_string(), json!(tokens.id_token));
    token_document.insert("access_token".to_string(), json!(tokens.access_token));
    token_document.insert("refresh_token".to_string(), json!(tokens.refresh_token));
    token_document.insert(
        "account_id".to_string(),
        account_id
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
    );

    let data = data
        .as_object_mut()
        .ok_or_else(|| AppError::msg("auth.json document is not an object"))?;
    data.insert("tokens".to_string(), Value::Object(token_document));
    data.insert("last_refresh".to_string(), json!(Utc::now().to_rfc3339()));
    data.entry("OPENAI_API_KEY".to_string())
        .or_insert(Value::Null);

    let text = serde_json::to_string_pretty(data).map_err(|source| AppError::Json {
        context: "Failed to serialize auth.json",
        source,
    })?;
    write_atomic(path, text.as_bytes(), true)
}

pub fn write_codex_auth(tokens: &Tokens, account_id: Option<&str>) -> AppResult<()> {
    let path = codex_auth_path()?;
    write_codex_auth_at(&path, tokens, account_id)
}

/// The credential fields exactly as auth.json stores them, without the id_token
/// fallbacks `read_codex_auth_at` applies. A mirror check has to compare what a
/// write would actually replace, not a reconstructed view of it.
fn stored_auth_credentials(path: &Path) -> AppResult<Option<(Tokens, Option<String>)>> {
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path).map_err(|source| AppError::Io {
        context: "Failed to read auth.json before updating it",
        source,
    })?;
    let parsed: CodexAuthFile = serde_json::from_str(&text).map_err(|source| AppError::Json {
        context: "Failed to parse auth.json before updating it",
        source,
    })?;

    Ok(parsed.tokens.map(|stored| {
        (
            Tokens {
                id_token: stored.id_token,
                access_token: stored.access_token,
                refresh_token: stored.refresh_token,
            },
            stored.account_id,
        )
    }))
}

fn write_codex_auth_if_changed_at(
    path: &Path,
    tokens: &Tokens,
    account_id: Option<&str>,
) -> AppResult<bool> {
    if let Some((stored_tokens, stored_account_id)) = stored_auth_credentials(path)?
        && !tokens_differ(&stored_tokens, tokens)
        && stored_account_id.as_deref() == account_id
    {
        return Ok(false);
    }

    write_codex_auth_at(path, tokens, account_id)?;
    Ok(true)
}

/// Mirrors `tokens` into auth.json only when the file does not already hold
/// exactly them, reporting whether it wrote. Every refresh of the active
/// account re-mirrors credentials that usually did not change, and rewriting
/// bumps `last_refresh` underneath a Codex process reading the same file for no
/// gain. Like the unconditional write, a corrupt document fails closed.
pub fn write_codex_auth_if_changed(tokens: &Tokens, account_id: Option<&str>) -> AppResult<bool> {
    let path = codex_auth_path()?;
    write_codex_auth_if_changed_at(&path, tokens, account_id)
}

fn clear_codex_auth_at(path: &Path) -> AppResult<()> {
    let mut data = existing_auth_document(path)?;
    let data = data
        .as_object_mut()
        .ok_or_else(|| AppError::msg("auth.json document is not an object"))?;
    data.insert("tokens".to_string(), Value::Null);
    data.insert("last_refresh".to_string(), json!(Utc::now().to_rfc3339()));
    data.entry("OPENAI_API_KEY".to_string())
        .or_insert(Value::Null);

    let text = serde_json::to_string_pretty(data).map_err(|source| AppError::Json {
        context: "Failed to serialize cleared auth.json",
        source,
    })?;
    write_atomic(path, text.as_bytes(), true)
}

pub fn clear_codex_auth() -> AppResult<()> {
    let path = codex_auth_path()?;
    clear_codex_auth_at(&path)
}

#[cfg(target_os = "windows")]
fn is_chatgpt_running() -> AppResult<bool> {
    let output = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq ChatGPT.exe", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|source| AppError::Io {
            context: "Failed to check whether ChatGPT is still running",
            source,
        })?;

    if !output.status.success() {
        return Err(AppError::msg(format!(
            "Failed to check whether ChatGPT is still running: {}",
            output.status
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .to_ascii_lowercase()
        .contains(&CHATGPT_PROCESS_IMAGE.to_ascii_lowercase()))
}

#[cfg(target_os = "windows")]
fn wait_for_chatgpt_to_exit() -> AppResult<()> {
    let deadline = Instant::now() + CHATGPT_EXIT_TIMEOUT;

    while is_chatgpt_running()? {
        if Instant::now() >= deadline {
            return Err(AppError::msg(
                "ChatGPT did not close within 5 seconds. Close it manually, then open ChatGPT to use the selected account.",
            ));
        }
        thread::sleep(CHATGPT_EXIT_POLL_INTERVAL);
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub fn restart_codex_process() -> AppResult<()> {
    if !is_chatgpt_running()? {
        return Ok(());
    }

    let status = Command::new("taskkill")
        .args(["/IM", CHATGPT_PROCESS_IMAGE, "/F", "/T"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|source| AppError::Io {
            context: "Failed to stop ChatGPT process",
            source,
        })?;

    if !status.success() {
        log::info!(
            "taskkill returned {status} for {CHATGPT_PROCESS_IMAGE}; continuing because ChatGPT may already be closed"
        );
    }

    wait_for_chatgpt_to_exit()?;
    crate::shell::open_target(CHATGPT_APP_TARGET, "Failed to launch ChatGPT")
}

#[cfg(target_os = "macos")]
fn is_chatgpt_running() -> AppResult<bool> {
    let output = Command::new("pgrep")
        .arg("-x")
        .arg("ChatGPT")
        .output()
        .map_err(|source| AppError::Io {
            context: "Failed to check whether ChatGPT is still running",
            source,
        })?;
    Ok(output.status.success())
}

#[cfg(target_os = "macos")]
fn wait_for_chatgpt_to_exit() -> AppResult<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while is_chatgpt_running()? {
        if Instant::now() >= deadline {
            return Err(AppError::msg(
                "ChatGPT did not close within 5 seconds. Close it manually, then open ChatGPT to use the selected account.",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn restart_codex_process() -> AppResult<()> {
    if !is_chatgpt_running()? {
        return Ok(());
    }

    let _ = Command::new("osascript")
        .args(["-e", "tell application \"ChatGPT\" to quit"])
        .output();

    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut exited = false;
    while Instant::now() < deadline {
        if !is_chatgpt_running()? {
            exited = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !exited && is_chatgpt_running()? {
        let _ = Command::new("pkill").arg("-x").arg("ChatGPT").output();
        wait_for_chatgpt_to_exit()?;
    }

    Command::new("open")
        .arg("-a")
        .arg("ChatGPT")
        .spawn()
        .map_err(|source| AppError::Io {
            context: "Failed to launch ChatGPT",
            source,
        })?;
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn restart_codex_process() -> AppResult<()> {
    Err(AppError::msg(
        "Automatic Codex restart is currently only supported on Windows and macOS.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Account, AppData, TokenHealth, Tokens};

    fn account(id: &str, account_id: Option<&str>, email: Option<&str>, ts: i64) -> Account {
        Account {
            id: id.to_string(),
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
            tokens: Tokens {
                id_token: format!("id-{id}"),
                access_token: format!("access-{id}"),
                refresh_token: format!("refresh-{id}"),
            },
            token_expires_at: None,
            tokens_updated_at: Some(ts),
            token_health: TokenHealth::default(),
            quota: None,
            quota_next_refresh_at: None,
            quota_refresh_failures: 0,
            created_at: ts,
            last_login_at: ts,
            last_error: None,
            subscription_error: None,
        }
    }

    fn snapshot(
        account_id: Option<&str>,
        email: Option<&str>,
        last_refresh_at: i64,
    ) -> CodexAuthSnapshot {
        CodexAuthSnapshot {
            tokens: Tokens {
                id_token: "snapshot-id".to_string(),
                access_token: "snapshot-access".to_string(),
                refresh_token: "snapshot-refresh".to_string(),
            },
            account_id: account_id.map(ToOwned::to_owned),
            email: email.map(ToOwned::to_owned),
            last_refresh_at: Some(last_refresh_at),
        }
    }

    #[test]
    fn startup_reconcile_selects_known_account_by_exact_account_id() {
        let mut data = AppData {
            accounts: vec![
                account("one", Some("id-one"), Some("one@example.com"), 100),
                account("two", Some("id-two"), Some("two@example.com"), 100),
            ],
            active_account_id: None,
            ..AppData::default()
        };

        let changed = reconcile_codex_auth_from_snapshot(
            &mut data,
            Some(&snapshot(Some("id-two"), Some("one@example.com"), 200)),
        );

        assert!(changed);
        assert_eq!(data.active_account_id.as_deref(), Some("two"));
        assert_eq!(data.accounts[1].tokens.access_token, "snapshot-access");
        assert_eq!(data.accounts[0].tokens.access_token, "access-one");
    }

    #[test]
    fn startup_reconcile_uses_email_fallback_only_without_account_id() {
        let mut data = AppData {
            accounts: vec![
                account("one", Some("id-one"), Some("one@example.com"), 100),
                account("two", None, Some("two@example.com"), 100),
            ],
            active_account_id: Some("one".to_string()),
            ..AppData::default()
        };

        // Unknown account_id must not fall back to the email match.
        let changed = reconcile_codex_auth_from_snapshot(
            &mut data,
            Some(&snapshot(Some("unknown"), Some("two@example.com"), 200)),
        );
        assert!(changed);
        assert_eq!(data.active_account_id, None);
        assert_eq!(data.accounts[1].tokens.access_token, "access-two");

        // Without account_id the email match selects and refreshes the account.
        let changed = reconcile_codex_auth_from_snapshot(
            &mut data,
            Some(&snapshot(None, Some("TWO@example.com"), 200)),
        );
        assert!(changed);
        assert_eq!(data.active_account_id.as_deref(), Some("two"));
        assert_eq!(data.accounts[1].tokens.access_token, "snapshot-access");

        // Whitespace account_id is treated as absent.
        let mut whitespace = AppData {
            accounts: vec![account("two", None, Some("two@example.com"), 100)],
            active_account_id: None,
            ..AppData::default()
        };
        let changed = reconcile_codex_auth_from_snapshot(
            &mut whitespace,
            Some(&snapshot(Some("   "), Some("two@example.com"), 200)),
        );
        assert!(changed);
        assert_eq!(whitespace.active_account_id.as_deref(), Some("two"));
    }

    #[test]
    fn startup_reconcile_unknown_account_id_clears_active_selection() {
        let mut data = AppData {
            accounts: vec![account("one", Some("id-one"), None, 100)],
            active_account_id: Some("one".to_string()),
            ..AppData::default()
        };

        let changed = reconcile_codex_auth_from_snapshot(
            &mut data,
            Some(&snapshot(Some("missing"), None, 200)),
        );

        assert!(changed);
        assert_eq!(data.active_account_id, None);
        assert_eq!(data.accounts[0].tokens.access_token, "access-one");

        data.active_account_id = None;
        assert!(!reconcile_codex_auth_from_snapshot(
            &mut data,
            Some(&snapshot(Some("missing"), None, 200)),
        ));
    }

    #[test]
    fn startup_reconcile_does_not_overwrite_fresher_account_tokens() {
        let mut data = AppData {
            accounts: vec![account("one", Some("id-one"), None, 300)],
            active_account_id: Some("one".to_string()),
            ..AppData::default()
        };

        let changed = reconcile_codex_auth_from_snapshot(
            &mut data,
            Some(&snapshot(Some("id-one"), None, 200)),
        );

        assert!(!changed);
        assert_eq!(data.accounts[0].tokens.access_token, "access-one");
        assert_eq!(data.accounts[0].tokens_updated_at, Some(300));
        assert_eq!(data.active_account_id.as_deref(), Some("one"));
    }

    #[test]
    fn startup_reconcile_updates_tokens_when_snapshot_is_not_older() {
        let mut data = AppData {
            accounts: vec![account("one", Some("id-one"), None, 200)],
            active_account_id: Some("one".to_string()),
            ..AppData::default()
        };

        let changed = reconcile_codex_auth_from_snapshot(
            &mut data,
            Some(&snapshot(Some("id-one"), None, 200)),
        );

        assert!(changed);
        assert_eq!(data.accounts[0].tokens.access_token, "snapshot-access");
        assert_eq!(data.accounts[0].tokens_updated_at, Some(200));
    }

    #[test]
    fn startup_reconcile_backfills_missing_email_without_overwriting_older_tokens() {
        let mut data = AppData {
            accounts: vec![account("one", Some("id-one"), None, 300)],
            active_account_id: Some("one".to_string()),
            ..AppData::default()
        };

        let changed = reconcile_codex_auth_from_snapshot(
            &mut data,
            Some(&snapshot(Some("id-one"), Some("one@example.com"), 200)),
        );

        assert!(changed);
        assert_eq!(data.accounts[0].email.as_deref(), Some("one@example.com"));
        assert_eq!(data.accounts[0].tokens.access_token, "access-one");
        assert_eq!(data.accounts[0].tokens_updated_at, Some(300));
    }

    #[test]
    fn startup_reconcile_updates_only_first_account_for_duplicate_email() {
        let mut data = AppData {
            accounts: vec![
                account("first", None, Some("shared@example.com"), 100),
                account("second", None, Some("shared@example.com"), 100),
            ],
            active_account_id: None,
            ..AppData::default()
        };

        let changed = reconcile_codex_auth_from_snapshot(
            &mut data,
            Some(&snapshot(None, Some("shared@example.com"), 200)),
        );

        assert!(changed);
        assert_eq!(data.active_account_id.as_deref(), Some("first"));
        assert_eq!(data.accounts[0].tokens.access_token, "snapshot-access");
        assert_eq!(data.accounts[1].tokens.access_token, "access-second");
    }

    #[test]
    fn startup_reconcile_missing_snapshot_leaves_state_unchanged() {
        let mut data = AppData {
            accounts: vec![account("one", Some("id-one"), None, 100)],
            active_account_id: Some("one".to_string()),
            ..AppData::default()
        };

        assert!(!reconcile_codex_auth_from_snapshot(&mut data, None));
        assert_eq!(data.active_account_id.as_deref(), Some("one"));
        assert_eq!(data.accounts[0].tokens.access_token, "access-one");
    }

    #[test]
    fn write_codex_auth_preserves_api_key_unknown_fields_and_token_scopes() {
        let dir =
            std::env::temp_dir().join(format!("vg-codex-auth-write-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create test directory");
        let path = codex_auth_path_at(&dir);
        std::fs::create_dir_all(path.parent().expect("auth parent")).expect("create codex dir");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "OPENAI_API_KEY": "sk-keep-me",
                "customSetting": { "enabled": true },
                "tokens": { "scopes": ["chatgpt"], "deviceId": "device-1" }
            }))
            .expect("serialize fixture"),
        )
        .expect("write auth fixture");

        let tokens = Tokens {
            id_token: "new-id".to_string(),
            access_token: "new-access".to_string(),
            refresh_token: "new-refresh".to_string(),
        };
        write_codex_auth_at(&path, &tokens, Some("openai-1")).expect("write auth");

        let value: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read auth")).expect("parse auth");
        assert_eq!(value["OPENAI_API_KEY"], "sk-keep-me");
        assert_eq!(value["customSetting"]["enabled"], true);
        assert_eq!(value["tokens"]["scopes"][0], "chatgpt");
        assert_eq!(value["tokens"]["deviceId"], "device-1");
        assert_eq!(value["tokens"]["access_token"], "new-access");
        assert_eq!(value["tokens"]["account_id"], "openai-1");
        std::fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn clear_codex_auth_preserves_api_key_and_unknown_fields() {
        let dir =
            std::env::temp_dir().join(format!("vg-codex-auth-clear-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create test directory");
        let path = codex_auth_path_at(&dir);
        std::fs::create_dir_all(path.parent().expect("auth parent")).expect("create codex dir");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "OPENAI_API_KEY": "sk-keep-me",
                "customSetting": "keep",
                "tokens": { "scopes": ["chatgpt"] }
            }))
            .expect("serialize fixture"),
        )
        .expect("write auth fixture");

        clear_codex_auth_at(&path).expect("clear auth");

        let value: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read auth")).expect("parse auth");
        assert_eq!(value["OPENAI_API_KEY"], "sk-keep-me");
        assert_eq!(value["customSetting"], "keep");
        assert!(value["tokens"].is_null());
        assert!(value.get("last_refresh").is_some());
        std::fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn mirroring_rewrites_auth_json_only_when_credentials_change() {
        let dir =
            std::env::temp_dir().join(format!("vg-codex-auth-mirror-{}", uuid::Uuid::new_v4()));
        let path = codex_auth_path_at(&dir);
        std::fs::create_dir_all(path.parent().expect("auth parent")).expect("create codex dir");

        let tokens = Tokens {
            id_token: "id-1".to_string(),
            access_token: "access-1".to_string(),
            refresh_token: "refresh-1".to_string(),
        };
        let last_refresh = |path: &Path| -> String {
            let value: Value = serde_json::from_slice(&std::fs::read(path).expect("read auth"))
                .expect("parse auth");
            value["last_refresh"].as_str().expect("last_refresh").into()
        };

        // A missing file is always created.
        assert!(
            write_codex_auth_if_changed_at(&path, &tokens, Some("openai-1")).expect("mirror auth")
        );
        let first_refresh = last_refresh(&path);

        // Re-mirroring the same credentials must leave the file untouched, so a
        // Codex process reading it does not see a pointless last_refresh bump.
        assert!(
            !write_codex_auth_if_changed_at(&path, &tokens, Some("openai-1")).expect("mirror auth")
        );
        assert_eq!(last_refresh(&path), first_refresh);

        // A different account_id for the same tokens still has to be written.
        assert!(
            write_codex_auth_if_changed_at(&path, &tokens, Some("openai-2")).expect("mirror auth")
        );

        let rotated = Tokens {
            access_token: "access-2".to_string(),
            ..tokens.clone()
        };
        assert!(
            write_codex_auth_if_changed_at(&path, &rotated, Some("openai-2")).expect("mirror auth")
        );
        let value: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read auth")).expect("parse auth");
        assert_eq!(value["tokens"]["access_token"], "access-2");

        // Cleared credentials are restored rather than skipped.
        clear_codex_auth_at(&path).expect("clear auth");
        assert!(
            write_codex_auth_if_changed_at(&path, &rotated, Some("openai-2")).expect("mirror auth")
        );

        std::fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn mirroring_fails_closed_on_unreadable_document() {
        let dir = std::env::temp_dir().join(format!(
            "vg-codex-auth-mirror-corrupt-{}",
            uuid::Uuid::new_v4()
        ));
        let path = codex_auth_path_at(&dir);
        std::fs::create_dir_all(path.parent().expect("auth parent")).expect("create codex dir");
        std::fs::write(&path, b"{broken").expect("write corrupt auth");

        let tokens = Tokens {
            id_token: "id-1".to_string(),
            access_token: "access-1".to_string(),
            refresh_token: "refresh-1".to_string(),
        };
        let error = write_codex_auth_if_changed_at(&path, &tokens, Some("openai-1"))
            .expect_err("mirror must fail");
        assert!(error.user_message().contains("Failed to parse auth.json"));
        assert_eq!(std::fs::read(&path).expect("read auth"), b"{broken");
        std::fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn write_codex_auth_fails_closed_on_unreadable_document() {
        let dir =
            std::env::temp_dir().join(format!("vg-codex-auth-corrupt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create test directory");
        let path = codex_auth_path_at(&dir);
        std::fs::create_dir_all(path.parent().expect("auth parent")).expect("create codex dir");
        std::fs::write(&path, b"{broken").expect("write corrupt auth");

        let tokens = Tokens {
            id_token: "new-id".to_string(),
            access_token: "new-access".to_string(),
            refresh_token: "new-refresh".to_string(),
        };
        let error =
            write_codex_auth_at(&path, &tokens, Some("openai-1")).expect_err("write must fail");
        assert!(error.user_message().contains("Failed to parse auth.json"));
        assert_eq!(std::fs::read(&path).expect("read auth"), b"{broken");
        std::fs::remove_dir_all(dir).expect("remove test directory");
    }
}
