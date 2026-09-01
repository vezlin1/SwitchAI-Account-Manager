use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use keyring::Entry;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::atomic_file::{backup_path, write_atomic, write_atomic_with_backup};
use crate::errors::{AppError, AppResult};
use crate::models::Tokens;

const CREDENTIAL_SERVICE: &str = "com.local.vgcodexaccountmanager.vault";
const MASTER_KEY_USERNAME: &str = "master-key";
const VAULT_FILE_NAME: &str = "secrets.vault.json";
const VAULT_VERSION: u32 = 1;
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultEnvelope {
    version: u32,
    nonce: String,
    ciphertext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultTokens {
    id_token: String,
    access_token: String,
    refresh_token: String,
}

type TokenVault = HashMap<String, VaultTokens>;

impl From<&Tokens> for VaultTokens {
    fn from(tokens: &Tokens) -> Self {
        Self {
            id_token: tokens.id_token.clone(),
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
        }
    }
}

impl From<VaultTokens> for Tokens {
    fn from(tokens: VaultTokens) -> Self {
        Self {
            id_token: tokens.id_token,
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
        }
    }
}

fn master_entry() -> AppResult<Entry> {
    Entry::new(CREDENTIAL_SERVICE, MASTER_KEY_USERNAME).map_err(|error| {
        AppError::msg(format!(
            "Protected credential storage is unavailable: {error}"
        ))
    })
}

fn load_master_key(create: bool) -> AppResult<Option<[u8; KEY_BYTES]>> {
    match master_entry()?.get_password() {
        Ok(encoded) => {
            let decoded = STANDARD
                .decode(encoded.trim_matches('\0').trim())
                .map_err(|error| {
                    AppError::msg(format!("Protected vault key is not valid base64: {error}"))
                })?;
            decoded
                .try_into()
                .map(Some)
                .map_err(|_| AppError::msg("Protected vault key has an unexpected length"))
        }
        Err(keyring::Error::NoEntry) if !create => Ok(None),
        Err(keyring::Error::NoEntry) => {
            let mut key = [0_u8; KEY_BYTES];
            rand::rng().fill_bytes(&mut key);
            master_entry()?
                .set_password(&STANDARD.encode(key))
                .map_err(|error| {
                    AppError::msg(format!("Failed to store protected vault key: {error}"))
                })?;
            Ok(Some(key))
        }
        Err(error) => Err(AppError::msg(format!(
            "Failed to read protected vault key: {error}"
        ))),
    }
}

fn vault_path() -> AppResult<PathBuf> {
    Ok(crate::storage::app_storage_dir()?.join(VAULT_FILE_NAME))
}

fn cipher(key: &[u8; KEY_BYTES]) -> AppResult<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key)
        .map_err(|_| AppError::msg("Failed to initialize protected token vault"))
}

fn decrypt_vault(path: &Path, key: &[u8; KEY_BYTES]) -> AppResult<TokenVault> {
    let text = fs::read_to_string(path).map_err(|source| AppError::Io {
        context: "Failed to read protected token vault",
        source,
    })?;
    let envelope: VaultEnvelope = serde_json::from_str(&text).map_err(|source| AppError::Json {
        context: "Failed to parse protected token vault",
        source,
    })?;
    if envelope.version != VAULT_VERSION {
        return Err(AppError::msg(format!(
            "Unsupported protected token vault version {}",
            envelope.version
        )));
    }
    let nonce = STANDARD
        .decode(envelope.nonce)
        .map_err(|error| AppError::msg(format!("Protected vault nonce is invalid: {error}")))?;
    let nonce: [u8; NONCE_BYTES] = nonce
        .try_into()
        .map_err(|_| AppError::msg("Protected vault nonce has an unexpected length"))?;
    let ciphertext = STANDARD.decode(envelope.ciphertext).map_err(|error| {
        AppError::msg(format!("Protected vault ciphertext is invalid: {error}"))
    })?;
    let plaintext = cipher(key)?
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| {
            AppError::msg(
                "Protected token vault authentication failed. The file or key may be damaged.",
            )
        })?;
    serde_json::from_slice(&plaintext).map_err(|source| AppError::Json {
        context: "Failed to decode protected token vault",
        source,
    })
}

fn read_vault() -> AppResult<TokenVault> {
    let path = vault_path()?;
    if !path.exists() {
        return Ok(TokenVault::new());
    }
    let key = load_master_key(false)?.ok_or_else(|| {
        AppError::msg("Protected token vault exists, but its operating-system key is missing")
    })?;
    match decrypt_vault(&path, &key) {
        Ok(vault) => Ok(vault),
        Err(primary_error) => {
            let backup = backup_path(&path)?;
            let restored = decrypt_vault(&backup, &key).map_err(|backup_error| {
                AppError::msg(format!(
                    "Protected vault recovery failed. Main file: {}; backup: {}",
                    primary_error.user_message(),
                    backup_error.user_message()
                ))
            })?;
            let backup_bytes = fs::read(&backup).map_err(|source| AppError::Io {
                context: "Failed to read protected vault backup",
                source,
            })?;
            write_atomic(&path, &backup_bytes, true)?;
            log::warn!("Recovered protected token vault from encrypted backup");
            Ok(restored)
        }
    }
}

fn write_vault(vault: &TokenVault) -> AppResult<()> {
    let key = load_master_key(true)?
        .ok_or_else(|| AppError::msg("Protected vault key could not be created"))?;
    let plaintext = serde_json::to_vec(vault).map_err(|source| AppError::Json {
        context: "Failed to encode protected token vault",
        source,
    })?;
    let mut nonce = [0_u8; NONCE_BYTES];
    rand::rng().fill_bytes(&mut nonce);
    let ciphertext = cipher(&key)?
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| AppError::msg("Failed to encrypt protected token vault"))?;
    let envelope = VaultEnvelope {
        version: VAULT_VERSION,
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    };
    let serialized = serde_json::to_vec(&envelope).map_err(|source| AppError::Json {
        context: "Failed to serialize protected token vault",
        source,
    })?;
    write_atomic_with_backup(&vault_path()?, &serialized, true)
}

pub fn load_all_tokens() -> AppResult<HashMap<String, Tokens>> {
    let mut vault = read_vault()?;
    Ok(vault
        .drain()
        .map(|(account_id, tokens)| (account_id, Tokens::from(tokens)))
        .collect())
}

pub fn store_tokens(account_id: &str, tokens: &Tokens) -> AppResult<()> {
    if tokens.access_token.trim().is_empty() && tokens.refresh_token.trim().is_empty() {
        return Err(AppError::msg(format!(
            "Refusing to store empty protected tokens for account {account_id}"
        )));
    }
    let mut vault = read_vault()?;
    vault.insert(account_id.to_string(), VaultTokens::from(tokens));
    write_vault(&vault)
}

pub fn delete_tokens(account_id: &str) -> AppResult<()> {
    let mut vault = read_vault()?;
    if vault.remove(account_id).is_some() {
        write_vault(&vault)?;
    }
    Ok(())
}

/// Removes every account credential owned by this application. The encrypted
/// vault is replaced with an empty authenticated document, then the old atomic
/// backup is deleted so it cannot retain credentials after a user-requested reset.
pub fn clear_all_tokens() -> AppResult<()> {
    let path = vault_path()?;
    let backup = backup_path(&path)?;
    if !path.exists() && !backup.exists() {
        return Ok(());
    }

    write_vault(&TokenVault::new())?;
    if backup.exists() {
        fs::remove_file(&backup).map_err(|source| AppError::Io {
            context: "Failed to remove protected token vault backup during reset",
            source,
        })?;
    }
    Ok(())
}
