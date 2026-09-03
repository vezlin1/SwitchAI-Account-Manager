#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::process::Command;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::thread;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::models::{Account, AccountProvider, AppData, Tokens};

const ANTIGRAVITY_TARGET_NAME: &str = "gemini:antigravity";
const ANTIGRAVITY_USER_NAME: &str = "antigravity";
const MAX_WINDOWS_CREDENTIAL_BLOB_BYTES: usize = 2_560;

#[derive(Clone, PartialEq, Eq)]
pub struct AntigravityAuthSnapshot {
    pub tokens: Tokens,
    pub expires_at: Option<i64>,
    // Kept only in memory so a failed transaction can restore the exact blob
    // Antigravity wrote, including fields newer app versions may add.
    raw_credential_blob: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AntigravityTokenPayload {
    token: AntigravityTokenDetails,
    #[serde(default)]
    auth_method: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AntigravityTokenDetails {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expiry: Option<String>,
}

#[cfg(target_os = "windows")]
mod win_cred {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
        CredReadW, CredWriteW,
    };

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn read_generic_credential(target: &str) -> Result<Option<Vec<u8>>, String> {
        let target_wide = wide(target);
        let mut credential_ptr: *mut CREDENTIALW = null_mut();
        unsafe {
            let ok = CredReadW(
                target_wide.as_ptr(),
                CRED_TYPE_GENERIC,
                0,
                &mut credential_ptr,
            );
            if ok == 0 {
                let error = GetLastError();
                if error == ERROR_NOT_FOUND {
                    return Ok(None);
                }
                return Err(format!(
                    "Could not read the Antigravity Windows credential (error {error})"
                ));
            }
            if credential_ptr.is_null() {
                return Ok(None);
            }

            let credential = &*credential_ptr;
            let blob_len = credential.CredentialBlobSize as usize;
            let result = if blob_len == 0 {
                Vec::new()
            } else if credential.CredentialBlob.is_null() {
                CredFree(credential_ptr as *mut _);
                return Err("Antigravity credential has an invalid empty blob".to_string());
            } else {
                std::slice::from_raw_parts(credential.CredentialBlob, blob_len).to_vec()
            };
            CredFree(credential_ptr as *mut _);
            Ok(Some(result))
        }
    }

    pub fn write_generic_credential(target: &str, user: &str, blob: &[u8]) -> Result<(), String> {
        let mut target_wide = wide(target);
        let mut user_wide = wide(user);
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target_wide.as_mut_ptr(),
            Comment: null_mut(),
            LastWritten: windows_sys::Win32::Foundation::FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: null_mut(),
            TargetAlias: null_mut(),
            UserName: user_wide.as_mut_ptr(),
        };

        unsafe {
            let _ = CredDeleteW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0);
            if CredWriteW(&credential, 0) == 0 {
                return Err(format!(
                    "Could not update the Antigravity Windows credential (error {})",
                    GetLastError()
                ));
            }
        }
        Ok(())
    }

    pub fn delete_generic_credential(target: &str) -> Result<(), String> {
        let target_wide = wide(target);
        unsafe {
            if CredDeleteW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0) == 0 {
                let error = GetLastError();
                if error != ERROR_NOT_FOUND {
                    return Err(format!(
                        "Could not clear the Antigravity Windows credential (error {error})"
                    ));
                }
            }
        }
        Ok(())
    }
}

fn decode_credential_blob(bytes: Vec<u8>) -> AppResult<String> {
    if bytes.is_empty() {
        return Err(AppError::msg("Antigravity credential blob is empty"));
    }

    let is_utf16le = bytes.starts_with(&[0xFF, 0xFE])
        || (bytes.len() >= 2 && bytes.len().is_multiple_of(2) && bytes[1] == 0);

    let text = if is_utf16le {
        let slice = if bytes.starts_with(&[0xFF, 0xFE]) {
            &bytes[2..]
        } else {
            &bytes[..]
        };
        let u16_vec: Vec<u16> = slice
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16(&u16_vec).map_err(|error| {
            AppError::msg(format!(
                "Antigravity credential is not valid UTF-16: {error}"
            ))
        })?
    } else {
        let slice = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            &bytes[3..]
        } else {
            &bytes[..]
        };
        String::from_utf8(slice.to_vec()).map_err(|error| {
            AppError::msg(format!("Antigravity credential is not UTF-8: {error}"))
        })?
    };

    let text = text.trim_matches('\0').trim().to_string();

    if let Some(encoded) = text.strip_prefix("go-keyring-base64:") {
        let decoded = STANDARD.decode(encoded).map_err(|error| {
            AppError::msg(format!("Antigravity credential base64 is invalid: {error}"))
        })?;
        return decode_credential_blob(decoded);
    }
    Ok(text)
}

fn parse_expiry(expiry: Option<&str>) -> Option<i64> {
    expiry
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp())
}

fn parse_antigravity_payload(text: &str) -> AppResult<AntigravityAuthSnapshot> {
    let payload: AntigravityTokenPayload =
        serde_json::from_str(text).map_err(|source| AppError::Json {
            context: "Failed to parse Antigravity credential",
            source,
        })?;
    let access_token = payload.token.access_token.trim().to_string();
    let refresh_token = payload.token.refresh_token.unwrap_or_default();
    if access_token.is_empty() && refresh_token.trim().is_empty() {
        return Err(AppError::msg(
            "Antigravity credential does not contain OAuth tokens",
        ));
    }
    Ok(AntigravityAuthSnapshot {
        tokens: Tokens {
            access_token,
            refresh_token,
            id_token: String::new(),
        },
        expires_at: parse_expiry(payload.token.expiry.as_deref()),
        raw_credential_blob: None,
    })
}

fn serialize_antigravity_payload(tokens: &Tokens, expires_at: i64) -> AppResult<Vec<u8>> {
    if tokens.access_token.trim().is_empty() || tokens.refresh_token.trim().is_empty() {
        return Err(AppError::msg(
            "Antigravity requires both an access token and a refresh token",
        ));
    }
    let expiry = DateTime::<Utc>::from_timestamp(expires_at, 0)
        .ok_or_else(|| AppError::msg("Antigravity token expiry is outside the supported range"))?
        .to_rfc3339_opts(SecondsFormat::Micros, true);
    let payload = AntigravityTokenPayload {
        token: AntigravityTokenDetails {
            access_token: tokens.access_token.clone(),
            token_type: Some("Bearer".to_string()),
            refresh_token: Some(tokens.refresh_token.clone()),
            expiry: Some(expiry),
        },
        auth_method: Some("consumer".to_string()),
    };
    let bytes = serde_json::to_vec(&payload).map_err(|source| AppError::Json {
        context: "Failed to serialize Antigravity credential",
        source,
    })?;
    if bytes.len() > MAX_WINDOWS_CREDENTIAL_BLOB_BYTES {
        return Err(AppError::msg(
            "Antigravity credential is too large for Windows Credential Manager",
        ));
    }
    Ok(bytes)
}

#[cfg(target_os = "macos")]
fn macos_keyring_entry() -> AppResult<keyring::Entry> {
    keyring::Entry::new(ANTIGRAVITY_TARGET_NAME, ANTIGRAVITY_USER_NAME)
        .map_err(|error| AppError::msg(format!("macOS Keychain is unavailable: {error}")))
}

pub fn read_antigravity_auth() -> AppResult<Option<AntigravityAuthSnapshot>> {
    #[cfg(target_os = "windows")]
    {
        match win_cred::read_generic_credential(ANTIGRAVITY_TARGET_NAME) {
            Ok(Some(bytes)) => {
                let raw_credential_blob = bytes.clone();
                decode_credential_blob(bytes)
                    .and_then(|text| parse_antigravity_payload(&text))
                    .map(|mut snapshot| {
                        snapshot.raw_credential_blob = Some(raw_credential_blob);
                        Some(snapshot)
                    })
            }
            Ok(None) => Ok(None),
            Err(error) => Err(AppError::msg(error)),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let entry = macos_keyring_entry()?;
        match entry.get_password() {
            Ok(password) => {
                let raw_bytes = password.as_bytes().to_vec();
                decode_credential_blob(raw_bytes.clone())
                    .and_then(|text| parse_antigravity_payload(&text))
                    .map(|mut snapshot| {
                        snapshot.raw_credential_blob = Some(raw_bytes);
                        Some(snapshot)
                    })
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AppError::msg(format!(
                "Could not read Antigravity macOS keychain item: {error}"
            ))),
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Ok(None)
    }
}

pub fn write_antigravity_auth(tokens: &Tokens, expires_at: i64) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        let payload = serialize_antigravity_payload(tokens, expires_at)?;
        win_cred::write_generic_credential(ANTIGRAVITY_TARGET_NAME, ANTIGRAVITY_USER_NAME, &payload)
            .map_err(AppError::msg)
    }
    #[cfg(target_os = "macos")]
    {
        let payload = serialize_antigravity_payload(tokens, expires_at)?;
        let entry = macos_keyring_entry()?;
        let text = String::from_utf8(payload).map_err(|error| {
            AppError::msg(format!("Antigravity payload is not valid UTF-8: {error}"))
        })?;
        entry.set_password(&text).map_err(|error| {
            AppError::msg(format!(
                "Could not update Antigravity macOS keychain item: {error}"
            ))
        })
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (tokens, expires_at);
        Err(AppError::msg(
            "Antigravity account switching is currently supported only on Windows and macOS",
        ))
    }
}

pub fn write_antigravity_account_auth(account: &Account) -> AppResult<()> {
    if account.provider != AccountProvider::Gemini {
        return Err(AppError::msg(
            "Selected account is not an Antigravity account",
        ));
    }
    let expires_at = account.token_expires_at.ok_or_else(|| {
        AppError::msg("Google token expiry is unknown. Refresh or re-login before switching.")
    })?;
    write_antigravity_auth(&account.tokens, expires_at)
}

pub fn clear_antigravity_auth() -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        win_cred::delete_generic_credential(ANTIGRAVITY_TARGET_NAME).map_err(AppError::msg)
    }
    #[cfg(target_os = "macos")]
    {
        let entry = macos_keyring_entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AppError::msg(format!(
                "Could not clear Antigravity macOS keychain item: {error}"
            ))),
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(AppError::msg(
            "Antigravity account switching is currently supported only on Windows and macOS",
        ))
    }
}

pub fn restore_antigravity_auth(snapshot: Option<&AntigravityAuthSnapshot>) -> AppResult<()> {
    match snapshot {
        Some(snapshot) => {
            #[cfg(target_os = "windows")]
            if let Some(raw) = snapshot.raw_credential_blob.as_deref() {
                return win_cred::write_generic_credential(
                    ANTIGRAVITY_TARGET_NAME,
                    ANTIGRAVITY_USER_NAME,
                    raw,
                )
                .map_err(AppError::msg);
            }
            #[cfg(target_os = "macos")]
            if let Some(raw) = snapshot.raw_credential_blob.as_deref() {
                let entry = macos_keyring_entry()?;
                let text = decode_credential_blob(raw.to_vec())?;
                return entry.set_password(&text).map_err(|error| {
                    AppError::msg(format!(
                        "Could not restore Antigravity macOS keychain item: {error}"
                    ))
                });
            }
            let expires_at = snapshot.expires_at.ok_or_else(|| {
                AppError::msg(
                    "Cannot restore a synthesized Antigravity credential without its original expiry",
                )
            })?;
            write_antigravity_auth(&snapshot.tokens, expires_at)
        }
        None => clear_antigravity_auth(),
    }
}

pub fn reconcile_antigravity_auth_at_startup(data: &mut AppData) -> AppResult<Option<String>> {
    let Some(snapshot) = read_antigravity_auth()? else {
        return Ok(None);
    };
    let snapshot_email = crate::token_utils::extract_email(&snapshot.tokens.id_token);
    let matching_id = data
        .accounts
        .iter()
        .find(|account| {
            account.provider == AccountProvider::Gemini
                && (((!snapshot.tokens.refresh_token.is_empty()
                    && account.tokens.refresh_token == snapshot.tokens.refresh_token)
                    || (!snapshot.tokens.access_token.is_empty()
                        && account.tokens.access_token == snapshot.tokens.access_token))
                    || (snapshot_email.is_some()
                        && account
                            .email
                            .as_deref()
                            .zip(snapshot_email.as_deref())
                            .is_some_and(|(l, r)| l.trim().eq_ignore_ascii_case(r.trim()))))
        })
        .map(|account| account.id.clone());

    let Some(matching_id) = matching_id else {
        if data.active_gemini_account_id.is_some() {
            let mut next = data.clone();
            next.active_gemini_account_id = None;
            crate::storage::commit_app_data(data, next)?;
        }
        return Ok(Some(
            "Antigravity is signed in with an account not yet managed here. Open the Gemini tab and choose Import current session."
                .to_string(),
        ));
    };

    let mut next = data.clone();
    let mut changed = next.active_gemini_account_id.as_deref() != Some(matching_id.as_str());
    let account = next
        .accounts
        .iter_mut()
        .find(|account| account.id == matching_id)
        .ok_or_else(|| AppError::msg("Matched Antigravity account disappeared"))?;
    let mut reconciled_tokens = snapshot.tokens;
    if reconciled_tokens.id_token.is_empty() {
        reconciled_tokens.id_token = account.tokens.id_token.clone();
    }
    if account.tokens != reconciled_tokens {
        account.tokens = reconciled_tokens;
        account.tokens_updated_at = Some(crate::models::now_ts());
        changed = true;
    }
    if snapshot.expires_at.is_some() && account.token_expires_at != snapshot.expires_at {
        account.token_expires_at = snapshot.expires_at;
        changed = true;
    }
    next.active_gemini_account_id = Some(matching_id);
    if changed {
        crate::storage::commit_app_data(data, next)?;
    }
    Ok(None)
}

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(target_os = "windows")]
const ANTIGRAVITY_PROCESS_IMAGE: &str = "Antigravity.exe";
#[cfg(target_os = "windows")]
const ANTIGRAVITY_IDE_PROCESS_IMAGE: &str = "Antigravity IDE.exe";
#[cfg(target_os = "windows")]
const ANTIGRAVITY_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(target_os = "windows")]
const ANTIGRAVITY_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn antigravity_executable_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let mut candidates = Vec::new();
        if let Some(local_appdata) = dirs::data_local_dir() {
            candidates.push(
                local_appdata
                    .join("Programs")
                    .join("antigravity")
                    .join("Antigravity.exe"),
            );
            candidates.push(
                local_appdata
                    .join("Programs")
                    .join("Antigravity")
                    .join("Antigravity.exe"),
            );
        }
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(root) = std::env::var(variable) {
                candidates.push(
                    PathBuf::from(root)
                        .join("Antigravity")
                        .join("Antigravity.exe"),
                );
            }
        }
        candidates.into_iter().find(|path| path.is_file())
    }
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![PathBuf::from("/Applications/Antigravity.app")];
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join("Applications").join("Antigravity.app"));
        }
        candidates.into_iter().find(|path| path.exists())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    None
}

pub fn antigravity_ide_executable_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let mut candidates = Vec::new();
        if let Some(local_appdata) = dirs::data_local_dir() {
            candidates.push(
                local_appdata
                    .join("Programs")
                    .join("Antigravity IDE")
                    .join("Antigravity IDE.exe"),
            );
            candidates.push(
                local_appdata
                    .join("Programs")
                    .join("antigravity-ide")
                    .join("Antigravity IDE.exe"),
            );
        }
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(root) = std::env::var(variable) {
                candidates.push(
                    PathBuf::from(root)
                        .join("Antigravity IDE")
                        .join("Antigravity IDE.exe"),
                );
            }
        }
        candidates.into_iter().find(|path| path.is_file())
    }
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![PathBuf::from("/Applications/Antigravity IDE.app")];
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join("Applications").join("Antigravity IDE.app"));
        }
        candidates.into_iter().find(|path| path.exists())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    None
}

pub fn antigravity_cli_path() -> Option<PathBuf> {
    if let Some(home) = dirs::home_dir() {
        let state_dir = home.join(".gemini").join("antigravity");
        if state_dir.exists() {
            return Some(state_dir);
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            #[cfg(target_os = "windows")]
            {
                for ext in ["exe", "cmd", "bat"] {
                    let candidate = dir.join(format!("agy.{}", ext));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let candidate = dir.join("agy");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

pub fn detect_installed_antigravity_surfaces() -> Vec<String> {
    let mut detected = Vec::new();
    if antigravity_executable_path().is_some() {
        detected.push("antigravity".to_string());
    }
    if antigravity_ide_executable_path().is_some() {
        detected.push("ide".to_string());
    }
    if antigravity_cli_path().is_some() {
        detected.push("cli".to_string());
    }
    detected
}

#[cfg(target_os = "windows")]
pub fn check_running_antigravity_surfaces() -> (bool, bool) {
    let running =
        crate::process::is_named_process_running(&["Antigravity.exe", "Antigravity IDE.exe"]);
    (running[0], running[1])
}

#[cfg(target_os = "macos")]
pub fn check_running_antigravity_surfaces() -> (bool, bool) {
    let running = crate::process::is_named_process_running(&["Antigravity", "Antigravity IDE"]);
    (running[0], running[1])
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn check_running_antigravity_surfaces() -> (bool, bool) {
    (false, false)
}

pub fn get_antigravity_surfaces() -> Vec<crate::dto::AntigravitySurfaceDto> {
    let (app_running, ide_running) = check_running_antigravity_surfaces();

    let mut surfaces = Vec::new();

    let app_path = antigravity_executable_path();
    let app_installed = app_path.is_some();
    surfaces.push(crate::dto::AntigravitySurfaceDto {
        id: "antigravity".to_string(),
        name: "Antigravity".to_string(),
        description: "Desktop App".to_string(),
        installed: app_installed,
        running: app_running,
        path: app_path.map(|p| p.to_string_lossy().to_string()),
    });

    let ide_path = antigravity_ide_executable_path();
    let ide_installed = ide_path.is_some();
    surfaces.push(crate::dto::AntigravitySurfaceDto {
        id: "ide".to_string(),
        name: "Antigravity IDE".to_string(),
        description: "AI Code Editor".to_string(),
        installed: ide_installed,
        running: ide_running,
        path: ide_path.map(|p| p.to_string_lossy().to_string()),
    });

    let cli_path = antigravity_cli_path();
    let cli_installed = cli_path.is_some();
    surfaces.push(crate::dto::AntigravitySurfaceDto {
        id: "cli".to_string(),
        name: "Antigravity CLI".to_string(),
        description: "agy command-line".to_string(),
        installed: cli_installed,
        running: false,
        path: cli_path.map(|p| p.to_string_lossy().to_string()),
    });

    surfaces
}

#[cfg(target_os = "windows")]
pub fn is_antigravity_running() -> AppResult<bool> {
    crate::process::is_process_running("Antigravity.exe")
}

#[cfg(target_os = "macos")]
pub fn is_antigravity_running() -> AppResult<bool> {
    crate::process::is_process_running("Antigravity")
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn is_antigravity_running() -> AppResult<bool> {
    Ok(false)
}

#[cfg(target_os = "windows")]
fn wait_for_antigravity_to_exit() -> AppResult<()> {
    let deadline = Instant::now() + ANTIGRAVITY_EXIT_TIMEOUT;
    while is_antigravity_running()? {
        if Instant::now() >= deadline {
            return Err(AppError::msg(
                "Antigravity did not close within 5 seconds. Close it manually and open it again to use the selected account.",
            ));
        }
        thread::sleep(ANTIGRAVITY_EXIT_POLL_INTERVAL);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn wait_for_antigravity_to_exit() -> AppResult<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while is_antigravity_running()? {
        if Instant::now() >= deadline {
            return Err(AppError::msg(
                "Antigravity did not close within 5 seconds. Close it manually and open it again to use the selected account.",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn restart_antigravity_process() -> AppResult<()> {
    if !is_antigravity_running()? {
        return Ok(());
    }

    // Try graceful termination first so Electron/VS Code flushes workspaces safely
    let _ = Command::new("taskkill")
        .args(["/IM", ANTIGRAVITY_PROCESS_IMAGE])
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    let graceful_deadline = Instant::now() + Duration::from_millis(1500);
    let mut exited = false;
    while Instant::now() < graceful_deadline {
        if !is_antigravity_running()? {
            exited = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    if !exited && is_antigravity_running()? {
        let status = Command::new("taskkill")
            .args(["/IM", ANTIGRAVITY_PROCESS_IMAGE, "/F", "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|source| AppError::Io {
                context: "Failed to close Antigravity",
                source,
            })?;
        if !status.success() {
            return Err(AppError::msg(format!(
                "Antigravity could not be closed ({status})"
            )));
        }
        wait_for_antigravity_to_exit()?;
    }

    let executable = antigravity_executable_path().ok_or_else(|| {
        AppError::msg(
            "Antigravity executable was not found in LocalAppData or Program Files. Launch Antigravity manually to use the selected account.",
        )
    })?;
    crate::shell::open_target(
        &executable.to_string_lossy(),
        "Failed to launch Antigravity",
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn restart_antigravity_process() -> AppResult<()> {
    if !is_antigravity_running()? {
        return Ok(());
    }

    let _ = Command::new("osascript")
        .args(["-e", "tell application \"Antigravity\" to quit"])
        .output();

    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut exited = false;
    while Instant::now() < deadline {
        if !is_antigravity_running()? {
            exited = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !exited && is_antigravity_running()? {
        let _ = Command::new("pkill").arg("-x").arg("Antigravity").output();
        wait_for_antigravity_to_exit()?;
    }

    if let Some(app_path) = antigravity_executable_path() {
        Command::new("open")
            .arg("-a")
            .arg(app_path)
            .spawn()
            .map_err(|source| AppError::Io {
                context: "Failed to launch Antigravity",
                source,
            })?;
    } else {
        Command::new("open")
            .arg("-a")
            .arg("Antigravity")
            .spawn()
            .map_err(|source| AppError::Io {
                context: "Failed to launch Antigravity",
                source,
            })?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn restart_antigravity_process() -> AppResult<()> {
    Err(AppError::msg(
        "Automatic Antigravity restart is currently supported only on Windows and macOS",
    ))
}

#[cfg(target_os = "windows")]
pub fn is_antigravity_ide_running() -> AppResult<bool> {
    crate::process::is_process_running("Antigravity IDE.exe")
}

#[cfg(target_os = "macos")]
pub fn is_antigravity_ide_running() -> AppResult<bool> {
    crate::process::is_process_running("Antigravity IDE")
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn is_antigravity_ide_running() -> AppResult<bool> {
    Ok(false)
}

#[cfg(target_os = "windows")]
fn wait_for_antigravity_ide_to_exit() -> AppResult<()> {
    let deadline = Instant::now() + ANTIGRAVITY_EXIT_TIMEOUT;
    while is_antigravity_ide_running()? {
        if Instant::now() >= deadline {
            return Err(AppError::msg(
                "Antigravity IDE did not close within 5 seconds. Close it manually and open it again to use the selected account.",
            ));
        }
        thread::sleep(ANTIGRAVITY_EXIT_POLL_INTERVAL);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn wait_for_antigravity_ide_to_exit() -> AppResult<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while is_antigravity_ide_running()? {
        if Instant::now() >= deadline {
            return Err(AppError::msg(
                "Antigravity IDE did not close within 5 seconds. Close it manually and open it again to use the selected account.",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn restart_antigravity_ide_process() -> AppResult<()> {
    if !is_antigravity_ide_running()? {
        return Ok(());
    }

    let _ = Command::new("taskkill")
        .args(["/IM", ANTIGRAVITY_IDE_PROCESS_IMAGE])
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    let graceful_deadline = Instant::now() + Duration::from_millis(1500);
    let mut exited = false;
    while Instant::now() < graceful_deadline {
        if !is_antigravity_ide_running()? {
            exited = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    if !exited && is_antigravity_ide_running()? {
        let status = Command::new("taskkill")
            .args(["/IM", ANTIGRAVITY_IDE_PROCESS_IMAGE, "/F", "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|source| AppError::Io {
                context: "Failed to close Antigravity IDE",
                source,
            })?;
        if !status.success() {
            return Err(AppError::msg(format!(
                "Antigravity IDE could not be closed ({status})"
            )));
        }
        wait_for_antigravity_ide_to_exit()?;
    }

    let executable = antigravity_ide_executable_path().ok_or_else(|| {
        AppError::msg(
            "Antigravity IDE executable was not found in LocalAppData or Program Files. Launch Antigravity IDE manually to use the selected account.",
        )
    })?;
    crate::shell::open_target(
        &executable.to_string_lossy(),
        "Failed to launch Antigravity IDE",
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn restart_antigravity_ide_process() -> AppResult<()> {
    if !is_antigravity_ide_running()? {
        return Ok(());
    }

    let _ = Command::new("osascript")
        .args(["-e", "tell application \"Antigravity IDE\" to quit"])
        .output();

    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut exited = false;
    while Instant::now() < deadline {
        if !is_antigravity_ide_running()? {
            exited = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !exited && is_antigravity_ide_running()? {
        let _ = Command::new("pkill")
            .arg("-x")
            .arg("Antigravity IDE")
            .output();
        wait_for_antigravity_ide_to_exit()?;
    }

    if let Some(app_path) = antigravity_ide_executable_path() {
        Command::new("open")
            .arg("-a")
            .arg(app_path)
            .spawn()
            .map_err(|source| AppError::Io {
                context: "Failed to launch Antigravity IDE",
                source,
            })?;
    } else {
        Command::new("open")
            .arg("-a")
            .arg("Antigravity IDE")
            .spawn()
            .map_err(|source| AppError::Io {
                context: "Failed to launch Antigravity IDE",
                source,
            })?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn restart_antigravity_ide_process() -> AppResult<()> {
    Err(AppError::msg(
        "Automatic Antigravity IDE restart is currently supported only on Windows and macOS",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_payload_uses_consumer_auth_and_real_expiry() {
        let tokens = Tokens {
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
        };
        let bytes = serialize_antigravity_payload(&tokens, 1_900_000_000).expect("serialize");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        assert_eq!(value["auth_method"], "consumer");
        assert_eq!(value["token"]["token_type"], "Bearer");
        assert_eq!(value["token"]["expiry"], "2030-03-17T17:46:40.000000Z");
    }

    #[test]
    fn parses_antigravity_expiry_without_exposing_tokens() {
        let snapshot = parse_antigravity_payload(
            r#"{"token":{"access_token":"access","refresh_token":"refresh","expiry":"2030-03-17T17:46:40Z"},"auth_method":"consumer"}"#,
        )
        .expect("parse");
        assert_eq!(snapshot.expires_at, Some(1_900_000_000));
        assert_eq!(snapshot.tokens.refresh_token, "refresh");
    }

    #[test]
    fn decodes_utf8_utf16le_and_bom_credential_blobs() {
        let sample = r#"{"token":{"access_token":"acc","refresh_token":"ref"}}"#;

        // UTF-8
        assert_eq!(
            decode_credential_blob(sample.as_bytes().to_vec()).unwrap(),
            sample
        );

        // UTF-8 with BOM
        let mut utf8_bom = vec![0xEF, 0xBB, 0xBF];
        utf8_bom.extend_from_slice(sample.as_bytes());
        assert_eq!(decode_credential_blob(utf8_bom).unwrap(), sample);

        // UTF-16LE
        let utf16le: Vec<u8> = sample
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(decode_credential_blob(utf16le).unwrap(), sample);

        // Trailing nulls
        let mut with_null = sample.as_bytes().to_vec();
        with_null.push(0);
        with_null.push(0);
        assert_eq!(decode_credential_blob(with_null).unwrap(), sample);
    }
}
