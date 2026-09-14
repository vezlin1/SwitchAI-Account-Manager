use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::prelude::*;
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app_state::SharedState;
use crate::atomic_file::write_atomic;
use crate::errors::{AppError, AppResult};

pub const DEFAULT_UPDATE_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDMwNTU1MDVDNDhENEI2QTQKUldTa3R0UklYRkJWTUxVb2d6dUUwUVAzdTZINU4rY0RGNFFUeEJIeWhuK2QyWXFSYkFJMkg4V3kK";

pub fn get_update_public_key() -> &'static str {
    option_env!("SWITCHAI_UPDATE_PUBLIC_KEY").unwrap_or(DEFAULT_UPDATE_PUBLIC_KEY)
}

pub const LATEST_MANIFEST_URL: &str =
    "https://github.com/vezlin1/SwitchAI-Account-Manager/releases/latest/download/latest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifestPlatform {
    pub signature: String,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default, rename = "pub_date")]
    pub release_date: Option<String>,
    pub platforms: HashMap<String, UpdateManifestPlatform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedUpdateMetadata {
    pub version: String,
    pub url: String,
    pub signature: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub public_key: String,
    #[serde(default)]
    pub ack_token: Option<String>,
}

pub struct UpdateDownloadRequest<'a> {
    pub download_url: &'a str,
    pub expected_size: Option<u64>,
    pub signature: &'a str,
    pub expected_sha256: Option<&'a str>,
    pub public_key: &'a str,
    pub version: &'a str,
}

pub fn current_platform_key() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows-x86_64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "darwin-aarch64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "darwin-x86_64"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "linux-x86_64"
    }
}

pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    let clean_current = current
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V');
    let clean_candidate = candidate
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V');
    match (
        semver::Version::parse(clean_current),
        semver::Version::parse(clean_candidate),
    ) {
        (Ok(cur), Ok(cand)) => cand > cur,
        _ => {
            // Safe numeric fallback if version string is non-standard (e.g. 1.1 or 1.1.0-build)
            let cur_parts: Vec<&str> = clean_current.split('.').collect();
            let cand_parts: Vec<&str> = clean_candidate.split('.').collect();
            if cur_parts.is_empty() || cand_parts.is_empty() {
                return false;
            }
            for (c_cur, c_cand) in cur_parts.iter().zip(cand_parts.iter()) {
                let n_cur = c_cur.parse::<u64>();
                let n_cand = c_cand.parse::<u64>();
                if let (Ok(nc), Ok(nd)) = (n_cur, n_cand) {
                    if nd > nc {
                        return true;
                    }
                    if nd < nc {
                        return false;
                    }
                }
            }
            cand_parts.len() > cur_parts.len()
        }
    }
}

pub fn get_exe_paths() -> AppResult<(PathBuf, PathBuf, PathBuf)> {
    let current_exe = std::env::current_exe()
        .map_err(|e| AppError::msg(format!("Cannot determine current executable path: {e}")))?;
    get_exe_paths_for(&current_exe)
}

fn get_exe_paths_for(current_exe: &Path) -> AppResult<(PathBuf, PathBuf, PathBuf)> {
    let file_name = current_exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("SwitchAI.exe");
    let old_exe = current_exe.with_file_name(format!("{file_name}.old"));
    let tmp_exe = current_exe.with_file_name(format!("{file_name}.update.tmp"));
    Ok((current_exe.to_path_buf(), old_exe, tmp_exe))
}

pub fn get_stage_metadata_path() -> AppResult<PathBuf> {
    let (current_exe, _, _) = get_exe_paths()?;
    get_stage_metadata_path_for(&current_exe)
}

fn get_stage_metadata_path_for(current_exe: &Path) -> AppResult<PathBuf> {
    let name = current_exe
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("SwitchAI.exe");
    Ok(current_exe.with_file_name(format!("{name}.update.json")))
}

fn get_update_helper_path_for(current_exe: &Path) -> AppResult<PathBuf> {
    let name = current_exe
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("SwitchAI.exe");
    Ok(current_exe.with_file_name(format!("{name}.update-helper.exe")))
}

fn get_update_ack_path_for(current_exe: &Path) -> AppResult<PathBuf> {
    let name = current_exe
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("SwitchAI.exe");
    Ok(current_exe.with_file_name(format!("{name}.update.ready")))
}

fn command_arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

pub fn is_supervised_update_launch() -> bool {
    command_arg("--update-ack-path").is_some() && command_arg("--update-ack-token").is_some()
}

pub fn acknowledge_update_startup() -> AppResult<()> {
    if !is_supervised_update_launch() {
        return Ok(());
    }
    let current_exe = get_exe_paths()?.0;
    let expected_path = get_update_ack_path_for(&current_exe)?;
    let supplied_path = PathBuf::from(
        command_arg("--update-ack-path")
            .ok_or_else(|| AppError::msg("Update acknowledgement path is missing"))?,
    );
    if supplied_path != expected_path {
        return Err(AppError::msg("Update acknowledgement path is invalid"));
    }
    let token = command_arg("--update-ack-token")
        .ok_or_else(|| AppError::msg("Update acknowledgement token is missing"))?;
    if token.len() < 16 || token.len() > 128 {
        return Err(AppError::msg("Update acknowledgement token is invalid"));
    }
    write_atomic(&expected_path, token.as_bytes(), false)
}

pub fn read_staged_update_metadata() -> AppResult<Option<StagedUpdateMetadata>> {
    let (current_exe, _, _) = get_exe_paths()?;
    read_staged_update_metadata_for(&current_exe)
}

pub fn read_staged_update_metadata_for(
    current_exe: &Path,
) -> AppResult<Option<StagedUpdateMetadata>> {
    let metadata_path = get_stage_metadata_path_for(current_exe)?;
    if !metadata_path.exists() {
        return Ok(None);
    }
    let (_, _, tmp_path) = get_exe_paths_for(current_exe)?;
    let metadata: StagedUpdateMetadata = match std::fs::read(&metadata_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(value) if tmp_path.exists() => value,
        _ => {
            let _ = std::fs::remove_file(&metadata_path);
            let _ = std::fs::remove_file(&tmp_path);
            return Ok(None);
        }
    };
    let bytes = std::fs::read(&tmp_path)
        .map_err(|error| AppError::msg(format!("Failed to read staged update: {error}")))?;
    if !is_newer_version(env!("CARGO_PKG_VERSION"), &metadata.version)
        || metadata
            .size
            .is_some_and(|expected_size| expected_size != bytes.len() as u64)
        || verify_payload_integrity(
            &bytes,
            metadata.sha256.as_deref(),
            &metadata.signature,
            // The signing key is part of the application build, not trusted
            // input from the sidecar file. Otherwise an attacker who can
            // replace both the staged binary and metadata could supply their
            // own public key and make the candidate appear valid.
            get_update_public_key(),
        )
        .is_err()
    {
        let _ = std::fs::remove_file(&metadata_path);
        let _ = std::fs::remove_file(&tmp_path);
        return Ok(None);
    }
    Ok(Some(metadata))
}

pub fn check_directory_write_permissions() -> AppResult<()> {
    let (exe_path, _, _) = get_exe_paths()?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| AppError::msg("Executable directory not found"))?;
    let probe_file = exe_dir.join(format!(".switchai_write_test_{}.tmp", uuid::Uuid::new_v4()));

    match std::fs::write(&probe_file, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe_file);
            Ok(())
        }
        Err(err) => Err(AppError::msg(format!(
            "Application directory is write-protected: {}. Please run SwitchAI from a writable folder or install the update manually.",
            err
        ))),
    }
}

pub async fn check_for_updates(
    client: &reqwest::Client,
    current_version: &str,
) -> AppResult<Option<UpdateManifest>> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    let url = format!("{LATEST_MANIFEST_URL}?t={timestamp}");

    let response = client
        .get(&url)
        .header("User-Agent", "SwitchAI-Portable-Updater")
        .send()
        .await
        .map_err(|source| AppError::Http {
            context: "Failed to fetch latest update manifest from GitHub CDN",
            source,
        })?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::msg(
            "Update manifest is unavailable for the latest release. Please download the update manually from GitHub Releases.",
        ));
    }

    if !response.status().is_success() {
        return Err(AppError::msg(format!(
            "GitHub CDN returned HTTP status {} when fetching update manifest",
            response.status()
        )));
    }

    let manifest: UpdateManifest = response
        .json()
        .await
        .map_err(|e| AppError::msg(format!("Failed to parse update manifest JSON: {e}")))?;

    if is_newer_version(current_version, &manifest.version) {
        Ok(Some(manifest))
    } else {
        Ok(None)
    }
}

fn normalize_minisign_pubkey(pubkey_input: &str) -> AppResult<PublicKey> {
    let trimmed = pubkey_input.trim();
    if trimmed.starts_with("untrusted comment:") {
        let lines: Vec<&str> = trimmed.lines().collect();
        let pubkey_part = if lines.len() >= 2 {
            lines[1].trim()
        } else {
            trimmed
        };
        return PublicKey::from_base64(pubkey_part)
            .map_err(|e| AppError::msg(format!("Failed to parse minisign public key: {e}")));
    }
    if let Some(decoded_str) = BASE64_STANDARD
        .decode(trimmed)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|s| s.starts_with("untrusted comment:"))
    {
        let lines: Vec<&str> = decoded_str.lines().collect();
        let pubkey_part = if lines.len() >= 2 {
            lines[1].trim()
        } else {
            decoded_str.trim()
        };
        return PublicKey::from_base64(pubkey_part)
            .map_err(|e| AppError::msg(format!("Failed to parse minisign public key: {e}")));
    }
    PublicKey::from_base64(trimmed)
        .map_err(|e| AppError::msg(format!("Failed to parse minisign public key: {e}")))
}

fn normalize_minisign_signature(signature_input: &str) -> AppResult<Signature> {
    let trimmed = signature_input.trim();
    if trimmed.starts_with("untrusted comment:") {
        return Signature::decode(trimmed).map_err(|e| {
            AppError::msg(format!("Failed to decode minisign signature format: {e}"))
        });
    }
    if let Some(decoded_str) = BASE64_STANDARD
        .decode(trimmed)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|s| s.starts_with("untrusted comment:"))
    {
        return Signature::decode(&decoded_str).map_err(|e| {
            AppError::msg(format!("Failed to decode minisign signature format: {e}"))
        });
    }
    Signature::decode(trimmed)
        .map_err(|e| AppError::msg(format!("Failed to decode minisign signature format: {e}")))
}

pub fn verify_minisign_signature(
    payload: &[u8],
    signature_input: &str,
    pubkey_input: &str,
) -> AppResult<()> {
    let pubkey = normalize_minisign_pubkey(pubkey_input)?;
    let sig = normalize_minisign_signature(signature_input)?;

    pubkey
        .verify(payload, &sig, false)
        .map_err(|e| AppError::msg(format!("Cryptographic signature verification failed: {e}")))?;

    Ok(())
}

pub fn verify_payload_integrity(
    payload: &[u8],
    expected_sha256: Option<&str>,
    signature_input: &str,
    pubkey_input: &str,
) -> AppResult<()> {
    if let Some(expected) = expected_sha256 {
        let digest = Sha256::digest(payload);
        let computed = digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if !computed.eq_ignore_ascii_case(expected.trim()) {
            return Err(AppError::msg(format!(
                "SHA-256 checksum mismatch: expected {}, got {}",
                expected.trim(),
                computed
            )));
        }
    }

    verify_minisign_signature(payload, signature_input, pubkey_input)
}

pub fn cleanup_stale_update_files() {
    if is_after_update_launch() {
        return;
    }
    if let Ok((_exe_path, _old_path, tmp_path)) = get_exe_paths() {
        let staged_is_valid = read_staged_update_metadata().ok().flatten().is_some();
        if !staged_is_valid {
            if let Ok(metadata_path) = get_stage_metadata_path() {
                let _ = std::fs::remove_file(metadata_path);
            }
            let _ = std::fs::remove_file(tmp_path);
        }
        if let Ok(current_exe) = std::env::current_exe() {
            if let Ok(helper_path) = get_update_helper_path_for(&current_exe) {
                let _ = std::fs::remove_file(helper_path);
            }
            if let Ok(ack_path) = get_update_ack_path_for(&current_exe) {
                let _ = std::fs::remove_file(ack_path);
            }
        }
    }
}

pub fn is_after_update_launch() -> bool {
    std::env::args().any(|argument| argument == "--after-update")
}

/// Removes the previous executable only after the replacement has loaded its
/// state and completed Tauri setup. A failed startup leaves the backup in place
/// for manual recovery instead of deleting the only known-good binary early.
pub fn confirm_update_startup() {
    if !is_after_update_launch() {
        return;
    }
    if let Ok((_exe_path, old_path, _tmp_path)) = get_exe_paths()
        && old_path.exists()
    {
        let _ = std::fs::remove_file(old_path);
    }
    if let Ok(metadata_path) = get_stage_metadata_path() {
        let _ = std::fs::remove_file(metadata_path);
    }
}

pub async fn download_and_stage_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    client: &reqwest::Client,
    request: &UpdateDownloadRequest<'_>,
) -> AppResult<PathBuf> {
    check_directory_write_permissions()?;
    let (_, _, tmp_path) = get_exe_paths()?;

    if tmp_path.exists() {
        let _ = std::fs::remove_file(&tmp_path);
    }

    use tauri::Emitter;

    let mut response = client
        .get(request.download_url)
        .header("User-Agent", "SwitchAI-Portable-Updater")
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|source| AppError::Http {
            context: "Failed to download update payload",
            source,
        })?;

    if !response.status().is_success() {
        return Err(AppError::msg(format!(
            "Download failed with HTTP status {}",
            response.status()
        )));
    }

    let total_bytes = response.content_length().or(request.expected_size);
    let mut file = std::fs::File::create(&tmp_path).map_err(|e| {
        AppError::msg(format!(
            "Failed to create temporary file {}: {e}",
            tmp_path.display()
        ))
    })?;

    use std::io::Write;
    let mut downloaded: u64 = 0;
    let mut last_progress_emit = std::time::Instant::now();
    let mut hasher = Sha256::new();

    loop {
        let chunk = match response.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                drop(file);
                let _ = std::fs::remove_file(&tmp_path);
                return Err(AppError::msg(format!(
                    "Error while streaming download chunks: {e}"
                )));
            }
        };

        if let Err(e) = file.write_all(&chunk) {
            drop(file);
            let _ = std::fs::remove_file(&tmp_path);
            return Err(AppError::msg(format!(
                "Failed to write chunk to temporary file: {e}"
            )));
        }
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;

        if last_progress_emit.elapsed() >= std::time::Duration::from_millis(80)
            || downloaded == total_bytes.unwrap_or(0)
        {
            last_progress_emit = std::time::Instant::now();
            let percent = match total_bytes {
                Some(total) if total > 0 => ((downloaded as f64 / total as f64) * 100.0) as f32,
                _ => 0.0,
            };
            let _ = app.emit(
                "update://progress",
                crate::dto::UpdateProgressDto {
                    downloaded_bytes: downloaded,
                    total_bytes,
                    percent,
                },
            );
        }
    }

    if let Err(e) = file.flush() {
        drop(file);
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::msg(format!(
            "Failed to flush staged update file: {e}"
        )));
    }
    drop(file);

    if let Some(expected) = request.expected_size
        && downloaded != expected
    {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::msg(format!(
            "Downloaded update size mismatch: expected {expected} bytes, got {downloaded}"
        )));
    }

    // Verify SHA-256 checksum from streaming hasher
    if let Some(expected) = request.expected_sha256 {
        let computed = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if !computed.eq_ignore_ascii_case(expected.trim()) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(AppError::msg(format!(
                "SHA-256 checksum mismatch: expected {}, got {}",
                expected.trim(),
                computed
            )));
        }
    }

    // Read the staged file from disk to verify cryptographic signature on the actual written content
    let staged_bytes = std::fs::read(&tmp_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        AppError::msg(format!(
            "Failed to read staged update file for signature verification: {e}"
        ))
    })?;

    // Verify minisign cryptographic signature
    if let Err(err) =
        verify_minisign_signature(&staged_bytes, request.signature, request.public_key)
    {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    let metadata = StagedUpdateMetadata {
        version: request.version.to_string(),
        url: request.download_url.to_string(),
        signature: request.signature.to_string(),
        sha256: request.expected_sha256.map(str::to_string),
        size: request.expected_size.or(Some(downloaded)),
        public_key: request.public_key.to_string(),
        ack_token: Some(uuid::Uuid::new_v4().to_string()),
    };
    let metadata_path = get_stage_metadata_path()?;
    let metadata_bytes = serde_json::to_vec_pretty(&metadata).map_err(|error| {
        AppError::msg(format!("Failed to encode staged update metadata: {error}"))
    })?;
    if let Err(error) = write_atomic(&metadata_path, &metadata_bytes, false) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(error);
    }

    // Final 100% progress emit
    let _ = app.emit(
        "update://progress",
        crate::dto::UpdateProgressDto {
            downloaded_bytes: downloaded,
            total_bytes: Some(downloaded),
            percent: 100.0,
        },
    );

    log::info!(
        "Update successfully downloaded and verified at {:?}",
        tmp_path
    );
    Ok(tmp_path)
}

pub fn atomic_swap(current: &Path, backup: &Path, replacement: &Path) -> std::io::Result<PathBuf> {
    let mut actual_backup = backup.to_path_buf();
    if backup.exists() {
        let mut removed = false;
        for _ in 0..3 {
            if std::fs::remove_file(backup).is_ok() || !backup.exists() {
                removed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !removed && backup.exists() {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let ext = format!("old.{}", timestamp);
            actual_backup = backup.with_extension(ext);
        }
    }
    std::fs::rename(current, &actual_backup)?;
    if let Err(err) = std::fs::rename(replacement, current) {
        log::error!("Replacement rename failed: {err}. Attempting rollback...");
        let _ = std::fs::rename(&actual_backup, current);
        return Err(err);
    }
    Ok(actual_backup)
}

pub fn rollback_swap(exe_path: &Path, old_path: &Path, tmp_path: &Path) -> std::io::Result<()> {
    if !old_path.exists() {
        return Ok(());
    }
    // On Windows, rename fails if the destination already exists.
    // Move the failed new executable back to tmp_path (or delete it) so exe_path becomes free.
    if exe_path.exists() {
        let _ = std::fs::remove_file(tmp_path);
        if let Err(rename_err) = std::fs::rename(exe_path, tmp_path) {
            log::warn!("Could not move failed binary to tmp ({rename_err}), deleting instead");
            for _ in 0..3 {
                if std::fs::remove_file(exe_path).is_ok() || !exe_path.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    std::fs::rename(old_path, exe_path)?;
    Ok(())
}

fn ensure_no_active_oauth(state: &Arc<SharedState>) -> AppResult<()> {
    {
        let flows = crate::app_state::lock_flows(state)?;
        if flows.values().any(|flow| {
            matches!(
                flow.status,
                crate::oauth::OauthFlowStatus::WaitingCallback
                    | crate::oauth::OauthFlowStatus::Exchanging
            )
        }) {
            return Err(AppError::msg(
                "Cannot restart while an account authorization (OAuth) is in progress. Please finish or cancel it first.",
            ));
        }
    }
    Ok(())
}

struct PausedAutoRefresh {
    state: Arc<SharedState>,
    resume_on_drop: bool,
}

impl PausedAutoRefresh {
    fn new(state: &Arc<SharedState>) -> AppResult<Self> {
        crate::auto_refresh::stop(state)?;
        Ok(Self {
            state: Arc::clone(state),
            resume_on_drop: true,
        })
    }
}

impl Drop for PausedAutoRefresh {
    fn drop(&mut self) {
        if self.resume_on_drop {
            // `start` consults current settings, so a disabled scheduler stays disabled.
            if let Err(error) = crate::auto_refresh::start(&self.state) {
                log::error!(
                    "Failed to resume auto refresh after interrupted update: {}",
                    error.user_message()
                );
            }
        }
    }
}

fn configure_background_command(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
}

fn parse_update_helper_request() -> AppResult<Option<(u32, PathBuf)>> {
    let args: Vec<String> = std::env::args().collect();
    let Some(index) = args
        .iter()
        .position(|argument| argument == "--update-helper")
    else {
        return Ok(None);
    };
    let pid = args
        .get(index + 1)
        .ok_or_else(|| AppError::msg("Update helper parent PID is missing"))?
        .parse::<u32>()
        .map_err(|_| AppError::msg("Update helper parent PID is invalid"))?;
    let executable = args
        .get(index + 2)
        .ok_or_else(|| AppError::msg("Update helper executable path is missing"))?;
    let executable = PathBuf::from(executable);
    if !executable.is_absolute() {
        return Err(AppError::msg(
            "Update helper executable path must be absolute",
        ));
    }
    Ok(Some((pid, executable)))
}

fn rollback_and_relaunch_previous(
    executable: &Path,
    backup: &Path,
    temporary: &Path,
    metadata: &Path,
    acknowledgement: &Path,
) {
    let _ = std::fs::remove_file(acknowledgement);
    if let Err(error) = rollback_swap(executable, backup, temporary) {
        log::error!("Failed to restore previous executable after update failure: {error}");
        return;
    }
    let _ = std::fs::remove_file(metadata);
    let mut command = std::process::Command::new(executable);
    configure_background_command(&mut command);
    if let Err(error) = command.spawn() {
        log::error!("Failed to relaunch previous executable after rollback: {error}");
    }
}

fn relaunch_original(executable: &Path) {
    let mut command = std::process::Command::new(executable);
    configure_background_command(&mut command);
    if let Err(error) = command.spawn() {
        log::error!("Failed to relaunch SwitchAI after cancelling update: {error}");
    }
}

fn run_update_helper_inner(parent_pid: u32, executable: &Path) -> AppResult<()> {
    let helper_executable = std::env::current_exe()
        .map_err(|error| AppError::msg(format!("Cannot determine update helper path: {error}")))?;
    let expected_helper = get_update_helper_path_for(executable)?;
    let helper_matches = std::fs::canonicalize(&helper_executable)
        .ok()
        .zip(std::fs::canonicalize(&expected_helper).ok())
        .is_some_and(|(actual, expected)| actual == expected);
    if !helper_matches {
        return Err(AppError::msg(
            "Update helper path is not owned by this installation",
        ));
    }

    let (current_path, backup_path, temporary_path) = get_exe_paths_for(executable)?;
    let metadata_path = get_stage_metadata_path_for(executable)?;
    let acknowledgement_path = get_update_ack_path_for(executable)?;
    if current_path != executable || !current_path.exists() {
        return Err(AppError::msg("Update target executable is unavailable"));
    }
    if parent_pid != 0 && !wait_for_process_exit(parent_pid, 10_000) {
        return Err(AppError::msg(
            "The previous SwitchAI process did not exit in time; update was cancelled safely.",
        ));
    }

    let metadata = match read_staged_update_metadata_for(executable) {
        Ok(Some(metadata)) => metadata,
        Ok(None) => {
            relaunch_original(executable);
            return Err(AppError::msg(
                "No verified staged update is available; update was cancelled safely.",
            ));
        }
        Err(error) => {
            relaunch_original(executable);
            return Err(error);
        }
    };
    let ack_token = metadata
        .ack_token
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let _ = std::fs::remove_file(&acknowledgement_path);

    let backup_path = match atomic_swap(&current_path, &backup_path, &temporary_path) {
        Ok(actual_backup) => actual_backup,
        Err(error) => {
            relaunch_original(executable);
            return Err(AppError::msg(format!("Atomic file swap failed: {error}")));
        }
    };

    let mut candidate_command = std::process::Command::new(&current_path);
    candidate_command
        .arg("--after-update")
        .arg(parent_pid.to_string())
        .arg("--update-ack-path")
        .arg(&acknowledgement_path)
        .arg("--update-ack-token")
        .arg(&ack_token);
    configure_background_command(&mut candidate_command);
    let mut candidate = match candidate_command.spawn() {
        Ok(child) => child,
        Err(error) => {
            rollback_and_relaunch_previous(
                &current_path,
                &backup_path,
                &temporary_path,
                &metadata_path,
                &acknowledgement_path,
            );
            return Err(AppError::msg(format!(
                "Failed to launch updated binary: {error}"
            )));
        }
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let healthy = loop {
        if std::fs::read_to_string(&acknowledgement_path)
            .ok()
            .is_some_and(|value| value == ack_token)
        {
            let grace_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let survived_grace = loop {
                if candidate
                    .try_wait()
                    .map_err(|error| {
                        AppError::msg(format!("Failed to monitor updated binary: {error}"))
                    })?
                    .is_some()
                {
                    break false;
                }
                if std::time::Instant::now() >= grace_deadline {
                    break true;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            };
            break survived_grace;
        }
        if candidate
            .try_wait()
            .map_err(|error| AppError::msg(format!("Failed to monitor updated binary: {error}")))?
            .is_some()
        {
            break false;
        }
        if std::time::Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };

    if healthy {
        let _ = std::fs::remove_file(&backup_path);
        let _ = std::fs::remove_file(&metadata_path);
        let _ = std::fs::remove_file(&acknowledgement_path);
        return Ok(());
    }

    let _ = candidate.kill();
    let _ = candidate.wait();
    rollback_and_relaunch_previous(
        &current_path,
        &backup_path,
        &temporary_path,
        &metadata_path,
        &acknowledgement_path,
    );
    Err(AppError::msg(
        "Updated SwitchAI did not complete startup in time; the previous version was restored.",
    ))
}

/// Runs the updater helper before Tauri initializes. Returning true tells the
/// regular application entrypoint to stop before loading state or plugins.
pub fn run_update_helper() -> bool {
    let request = match parse_update_helper_request() {
        Ok(request) => request,
        Err(error) => {
            eprintln!("SwitchAI update helper: {}", error.user_message());
            return true;
        }
    };
    let Some((parent_pid, executable)) = request else {
        return false;
    };
    if let Err(error) = run_update_helper_inner(parent_pid, &executable) {
        eprintln!("SwitchAI update helper: {}", error.user_message());
    }
    true
}

pub async fn perform_atomic_swap_and_restart(
    app: &tauri::AppHandle,
    state: &Arc<SharedState>,
) -> AppResult<()> {
    ensure_no_active_oauth(state)?;

    // Lifecycle safety 2: stop background auto refresh
    let mut paused_refresh = PausedAutoRefresh::new(state)?;

    // Drain both provider queues in the same order as a combined manual refresh.
    let _codex_gate = state.refresh_codex_gate.lock().await;
    let _gemini_gate = state.refresh_gemini_gate.lock().await;

    let (exe_path, _old_path, tmp_path) = get_exe_paths()?;
    if !tmp_path.exists() || read_staged_update_metadata()?.is_none() {
        return Err(AppError::msg(
            "No verified staged update is available. Please download the update first.",
        ));
    }

    crate::storage::flush_refresh_metadata(state)?;
    let current_pid = std::process::id();
    let helper_path = get_update_helper_path_for(&exe_path)?;
    std::fs::copy(&exe_path, &helper_path).map_err(|error| {
        AppError::msg(format!(
            "Failed to create the isolated update helper: {error}"
        ))
    })?;
    let mut helper_command = std::process::Command::new(&helper_path);
    helper_command
        .arg("--update-helper")
        .arg(current_pid.to_string())
        .arg(&exe_path);
    configure_background_command(&mut helper_command);
    if let Err(e) = helper_command.spawn() {
        let _ = std::fs::remove_file(&helper_path);
        return Err(AppError::msg(format!(
            "Failed to launch isolated update helper: {e}"
        )));
    }

    state
        .is_quitting
        .store(true, std::sync::atomic::Ordering::SeqCst);
    paused_refresh.resume_on_drop = false;
    app.exit(0);
    Ok(())
}

#[cfg(windows)]
pub fn wait_for_process_exit(pid: u32, timeout_ms: u32) -> bool {
    if pid == 0 {
        return true;
    }
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, FALSE, pid);
        if handle.is_null() {
            return true;
        }
        let exited = WaitForSingleObject(handle, timeout_ms) != WAIT_TIMEOUT;
        CloseHandle(handle);
        exited
    }
}

#[cfg(not(windows))]
pub fn wait_for_process_exit(_pid: u32, _timeout_ms: u32) -> bool {
    true
}

pub fn handle_after_update_wait() {
    let args: Vec<String> = std::env::args().collect();
    let mut old_pid: Option<u32> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--after-update" && i + 1 < args.len() {
            if let Ok(pid) = args[i + 1].parse::<u32>() {
                old_pid = Some(pid);
            }
            break;
        }
        i += 1;
    }

    if let Some(pid) = old_pid {
        #[cfg(windows)]
        let _ = wait_for_process_exit(pid, 5000);

        #[cfg(not(windows))]
        {
            let _ = pid;
        }
    }

    // Supervised candidates keep the backup until the frontend calls
    // `acknowledge_update_startup`; legacy direct launches are confirmed from
    // the Tauri setup callback for compatibility with older releases.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn locked_old_backup_rolls_back_to_the_actual_previous_executable() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir =
            std::env::temp_dir().join(format!("switchai-locked-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let current = dir.join("app.exe");
        let backup = dir.join("app.exe.old");
        let staged = dir.join("app.exe.tmp");
        std::fs::write(&current, "v1").unwrap();
        std::fs::write(&backup, "v0").unwrap();
        std::fs::write(&staged, "v2").unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&backup)
            .unwrap();
        let actual = atomic_swap(&current, &backup, &staged).unwrap();
        assert_ne!(actual, backup);
        assert_eq!(std::fs::read_to_string(&actual).unwrap(), "v1");
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "v2");
        rollback_swap(&current, &actual, &staged).unwrap();
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "v1");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "v0");
        drop(lock);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn terminal_oauth_flows_do_not_block_updates() {
        let state = Arc::new(
            SharedState::new_with_startup_error(crate::models::AppData::default(), None).unwrap(),
        );
        assert!(ensure_no_active_oauth(&state).is_ok());
        for status in [
            crate::oauth::OauthFlowStatus::Completed,
            crate::oauth::OauthFlowStatus::Cancelled,
            crate::oauth::OauthFlowStatus::Error("failed".into()),
        ] {
            let (mut flow, _) =
                crate::oauth::build_oauth_flow(crate::models::AccountProvider::Codex, None, None)
                    .unwrap();
            flow.status = status;
            crate::app_state::lock_flows(&state)
                .unwrap()
                .insert(flow.id.clone(), flow);
        }
        assert!(ensure_no_active_oauth(&state).is_ok());
        for status in [
            crate::oauth::OauthFlowStatus::WaitingCallback,
            crate::oauth::OauthFlowStatus::Exchanging,
        ] {
            let (mut flow, _) =
                crate::oauth::build_oauth_flow(crate::models::AccountProvider::Codex, None, None)
                    .unwrap();
            flow.status = status;
            let id = flow.id.clone();
            crate::app_state::lock_flows(&state)
                .unwrap()
                .insert(id.clone(), flow);
            assert!(ensure_no_active_oauth(&state).is_err());
            crate::app_state::lock_flows(&state).unwrap().remove(&id);
        }
    }

    #[test]
    fn failed_update_resumes_auto_refresh_but_respects_disabled_setting() {
        for enabled in [true, false] {
            let mut data = crate::models::AppData::default();
            data.app_settings.auto_refresh_enabled = enabled;
            let state = Arc::new(SharedState::new_with_startup_error(data, None).unwrap());
            let fail_install = || -> AppResult<()> {
                let _pause = PausedAutoRefresh::new(&state)?;
                assert!(!crate::app_state::lock_auto_refresh(&state)?.status.enabled);
                Err(AppError::msg("Simulated failed executable swap"))
            };
            assert!(fail_install().is_err());
            let resumed = crate::app_state::lock_auto_refresh(&state)
                .unwrap()
                .status
                .enabled;
            crate::auto_refresh::stop(&state).unwrap();
            assert_eq!(resumed, enabled);
        }
    }

    #[test]
    fn successful_update_keeps_auto_refresh_stopped() {
        let state = Arc::new(
            SharedState::new_with_startup_error(crate::models::AppData::default(), None).unwrap(),
        );
        {
            let mut pause = PausedAutoRefresh::new(&state).unwrap();
            pause.resume_on_drop = false;
        }
        assert!(
            !crate::app_state::lock_auto_refresh(&state)
                .unwrap()
                .status
                .enabled
        );
    }

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("1.1.0", "1.2.0"));
        assert!(is_newer_version("1.1.0", "v1.2.0"));
        assert!(is_newer_version("v1.1.0", "1.1.1"));
        assert!(!is_newer_version("1.2.0", "1.1.0"));
        assert!(!is_newer_version("1.1.0", "1.1.0"));
        assert!(is_newer_version("1.1.0-alpha.1", "1.1.0"));
    }

    #[test]
    fn test_minisign_verify_flow() {
        let pubkey_b64 = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDMwNTU1MDVDNDhENEI2QTQKUldTa3R0UklYRkJWTUxVb2d6dUUwUVAzdTZINU4rY0RGNFFUeEJIeWhuK2QyWXFSYkFJMkg4V3kK";
        let sig_b64 = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTa3R0UklYRkJWTUkvVzVHa1pSS2dFYlplalMzaXlIV01sSzVIYiswVUhZUW5hUHIwRGFYSDJocGFxYklBeGhWMWJMUkZYQnNRYXhvd3AxMmU4Z2E5YlFEdGtJcDg3cFFJPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzg4Mzg2OTIzCWZpbGU6dGVzdF9wYXlsb2FkLmJpbgpMS0VncmZKVEg1R1JVUm9tMkYzZ0piVWFkWlc1RHZWWjJKY2k5d0U1YjFjS1dpMFFGQUlVTmh1cGcxYlFMeHdXb0Z2azBUaGdsbnJHaWJ2LzkvVXZEUT09Cg==";
        let data = b"hello world 123";

        let result = verify_payload_integrity(
            data,
            Some("d4223bf93e202505a6a501421a88d9fa43341f7757e217dd603ccdce157c13bd"),
            sig_b64,
            pubkey_b64,
        );
        assert!(result.is_ok(), "Verification failed: {:?}", result.err());

        // Test with raw ASCII format (decoded from base64)
        let raw_pubkey = String::from_utf8(BASE64_STANDARD.decode(pubkey_b64).unwrap()).unwrap();
        let raw_sig = String::from_utf8(BASE64_STANDARD.decode(sig_b64).unwrap()).unwrap();
        let raw_result = verify_payload_integrity(
            data,
            Some("d4223bf93e202505a6a501421a88d9fa43341f7757e217dd603ccdce157c13bd"),
            &raw_sig,
            &raw_pubkey,
        );
        assert!(
            raw_result.is_ok(),
            "Raw ASCII verification failed: {:?}",
            raw_result.err()
        );

        // Test with corrupted data
        let corrupted_data = b"hello world 124";
        assert!(verify_payload_integrity(corrupted_data, None, sig_b64, pubkey_b64).is_err());
    }

    #[test]
    fn test_atomic_swap_and_rollback() {
        let temp_dir =
            std::env::temp_dir().join(format!("switchai_swap_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let current = temp_dir.join("app.exe");
        let backup = temp_dir.join("app.exe.old");
        let replacement = temp_dir.join("app.exe.update.tmp");

        std::fs::write(&current, b"version 1").unwrap();
        std::fs::write(&replacement, b"version 2").unwrap();

        // Test successful swap
        atomic_swap(&current, &backup, &replacement).expect("swap should succeed");
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "version 2");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "version 1");
        assert!(!replacement.exists());

        // Test rollback when replacement doesn't exist
        let non_existent_replacement = temp_dir.join("does_not_exist.tmp");
        let swap_err = atomic_swap(&current, &backup, &non_existent_replacement);
        assert!(
            swap_err.is_err(),
            "Swap should fail when replacement missing"
        );
        // Rollback should have restored current!
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "version 2");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_rollback_after_failed_spawn() {
        let temp_dir =
            std::env::temp_dir().join(format!("switchai_rollback_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let exe_path = temp_dir.join("app.exe");
        let old_path = temp_dir.join("app.exe.old");
        let tmp_path = temp_dir.join("app.exe.update.tmp");

        std::fs::write(&exe_path, b"original_v1").unwrap();
        std::fs::write(&tmp_path, b"new_v2_broken").unwrap();

        // 1. Swap succeeds: exe_path becomes v2, old_path becomes v1
        atomic_swap(&exe_path, &old_path, &tmp_path).expect("swap should succeed");
        assert_eq!(std::fs::read_to_string(&exe_path).unwrap(), "new_v2_broken");
        assert_eq!(std::fs::read_to_string(&old_path).unwrap(), "original_v1");

        // 2. Rollback when spawn fails: exe_path must be safely restored to v1
        rollback_swap(&exe_path, &old_path, &tmp_path).expect("rollback should succeed");
        assert_eq!(std::fs::read_to_string(&exe_path).unwrap(), "original_v1");
        assert_eq!(std::fs::read_to_string(&tmp_path).unwrap(), "new_v2_broken");
        assert!(!old_path.exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
