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

pub trait KeyringBackend: Send + Sync {
    fn get_password(&self, service: &str, username: &str) -> Result<String, keyring::Error>;
    fn set_password(
        &self,
        service: &str,
        username: &str,
        password: &str,
    ) -> Result<(), keyring::Error>;
    #[allow(dead_code)]
    fn delete_password(&self, service: &str, username: &str) -> Result<(), keyring::Error>;
}

pub struct OsKeyring;

impl KeyringBackend for OsKeyring {
    fn get_password(&self, service: &str, username: &str) -> Result<String, keyring::Error> {
        Entry::new(service, username)?.get_password()
    }

    fn set_password(
        &self,
        service: &str,
        username: &str,
        password: &str,
    ) -> Result<(), keyring::Error> {
        Entry::new(service, username)?.set_password(password)
    }

    fn delete_password(&self, service: &str, username: &str) -> Result<(), keyring::Error> {
        Entry::new(service, username)?.delete_credential()
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub struct InMemoryKeyring {
    entries: std::sync::Mutex<HashMap<(String, String), String>>,
}

impl InMemoryKeyring {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }
}

impl KeyringBackend for InMemoryKeyring {
    fn get_password(&self, service: &str, username: &str) -> Result<String, keyring::Error> {
        let entries = self.entries.lock().map_err(|_| {
            keyring::Error::PlatformFailure(std::io::Error::other("lock poisoned").into())
        })?;
        entries
            .get(&(service.to_string(), username.to_string()))
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }

    fn set_password(
        &self,
        service: &str,
        username: &str,
        password: &str,
    ) -> Result<(), keyring::Error> {
        let mut entries = self.entries.lock().map_err(|_| {
            keyring::Error::PlatformFailure(std::io::Error::other("lock poisoned").into())
        })?;
        entries.insert(
            (service.to_string(), username.to_string()),
            password.to_string(),
        );
        Ok(())
    }

    fn delete_password(&self, service: &str, username: &str) -> Result<(), keyring::Error> {
        let mut entries = self.entries.lock().map_err(|_| {
            keyring::Error::PlatformFailure(std::io::Error::other("lock poisoned").into())
        })?;
        if entries
            .remove(&(service.to_string(), username.to_string()))
            .is_some()
        {
            Ok(())
        } else {
            Err(keyring::Error::NoEntry)
        }
    }
}

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

pub struct SecretStore<B: KeyringBackend = OsKeyring> {
    backend: B,
    vault_path: PathBuf,
}

impl<B: KeyringBackend> SecretStore<B> {
    pub fn new(backend: B, vault_path: PathBuf) -> Self {
        Self {
            backend,
            vault_path,
        }
    }

    fn load_master_key(&self, create: bool) -> AppResult<Option<[u8; KEY_BYTES]>> {
        match self
            .backend
            .get_password(CREDENTIAL_SERVICE, MASTER_KEY_USERNAME)
        {
            Ok(encoded) => {
                let decoded =
                    STANDARD
                        .decode(encoded.trim_matches('\0').trim())
                        .map_err(|error| {
                            AppError::msg(format!(
                                "Protected vault key is not valid base64: {error}"
                            ))
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
                self.backend
                    .set_password(
                        CREDENTIAL_SERVICE,
                        MASTER_KEY_USERNAME,
                        &STANDARD.encode(key),
                    )
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

    fn cipher(&self, key: &[u8; KEY_BYTES]) -> AppResult<Aes256Gcm> {
        Aes256Gcm::new_from_slice(key)
            .map_err(|_| AppError::msg("Failed to initialize protected token vault"))
    }

    fn decrypt_vault(&self, path: &Path, key: &[u8; KEY_BYTES]) -> AppResult<TokenVault> {
        let text = fs::read_to_string(path).map_err(|source| AppError::Io {
            context: "Failed to read protected token vault",
            source,
        })?;
        let envelope: VaultEnvelope =
            serde_json::from_str(&text).map_err(|source| AppError::Json {
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
        let plaintext = self
            .cipher(key)?
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

    fn read_vault(&self) -> AppResult<TokenVault> {
        if !self.vault_path.exists() {
            return Ok(TokenVault::new());
        }
        let key = self.load_master_key(false)?.ok_or_else(|| {
            AppError::msg("Protected token vault exists, but its operating-system key is missing")
        })?;
        match self.decrypt_vault(&self.vault_path, &key) {
            Ok(vault) => Ok(vault),
            Err(primary_error) => {
                let backup = backup_path(&self.vault_path)?;
                let restored = self.decrypt_vault(&backup, &key).map_err(|backup_error| {
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
                write_atomic(&self.vault_path, &backup_bytes, true)?;
                log::warn!("Recovered protected token vault from encrypted backup");
                Ok(restored)
            }
        }
    }

    fn write_vault(&self, vault: &TokenVault) -> AppResult<()> {
        let key = self
            .load_master_key(true)?
            .ok_or_else(|| AppError::msg("Protected vault key could not be created"))?;
        let plaintext = serde_json::to_vec(vault).map_err(|source| AppError::Json {
            context: "Failed to encode protected token vault",
            source,
        })?;
        let mut nonce = [0_u8; NONCE_BYTES];
        rand::rng().fill_bytes(&mut nonce);
        let ciphertext = self
            .cipher(&key)?
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
        write_atomic_with_backup(&self.vault_path, &serialized, true)
    }

    pub fn load_all_tokens(&self) -> AppResult<HashMap<String, Tokens>> {
        let mut vault = self.read_vault()?;
        Ok(vault
            .drain()
            .map(|(account_id, tokens)| (account_id, Tokens::from(tokens)))
            .collect())
    }

    pub fn store_tokens(&self, account_id: &str, tokens: &Tokens) -> AppResult<()> {
        if tokens.access_token.trim().is_empty() && tokens.refresh_token.trim().is_empty() {
            return Err(AppError::msg(format!(
                "Refusing to store empty protected tokens for account {account_id}"
            )));
        }
        let mut vault = self.read_vault()?;
        vault.insert(account_id.to_string(), VaultTokens::from(tokens));
        self.write_vault(&vault)
    }

    pub fn delete_tokens(&self, account_id: &str) -> AppResult<()> {
        let mut vault = self.read_vault()?;
        if vault.remove(account_id).is_some() {
            self.write_vault(&vault)?;
        }
        Ok(())
    }

    pub fn clear_all_tokens(&self) -> AppResult<()> {
        let backup = backup_path(&self.vault_path)?;
        if !self.vault_path.exists() && !backup.exists() {
            return Ok(());
        }

        self.write_vault(&TokenVault::new())?;
        if backup.exists() {
            fs::remove_file(&backup).map_err(|source| AppError::Io {
                context: "Failed to remove protected token vault backup during reset",
                source,
            })?;
        }
        Ok(())
    }
}

fn vault_path() -> AppResult<PathBuf> {
    Ok(crate::storage::app_storage_dir()?.join(VAULT_FILE_NAME))
}

fn default_store() -> AppResult<SecretStore<OsKeyring>> {
    Ok(SecretStore::new(OsKeyring, vault_path()?))
}

pub fn load_all_tokens() -> AppResult<HashMap<String, Tokens>> {
    default_store()?.load_all_tokens()
}

pub fn store_tokens(account_id: &str, tokens: &Tokens) -> AppResult<()> {
    default_store()?.store_tokens(account_id, tokens)
}

pub fn delete_tokens(account_id: &str) -> AppResult<()> {
    default_store()?.delete_tokens(account_id)
}

pub fn clear_all_tokens() -> AppResult<()> {
    default_store()?.clear_all_tokens()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_store() -> (SecretStore<InMemoryKeyring>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("vg-secret-store-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create test dir");
        let vault_file = dir.join("secrets.vault.json");
        let store = SecretStore::new(InMemoryKeyring::new(), vault_file);
        (store, dir)
    }

    #[test]
    fn stores_loads_and_deletes_tokens_in_memory() {
        let (store, dir) = test_store();

        let initial = store.load_all_tokens().expect("load empty tokens");
        assert!(initial.is_empty());

        let tokens = Tokens {
            id_token: "id-token-1".to_string(),
            access_token: "access-token-1".to_string(),
            refresh_token: "refresh-token-1".to_string(),
        };

        store.store_tokens("acc-1", &tokens).expect("store tokens");

        let loaded = store.load_all_tokens().expect("load tokens");
        assert_eq!(loaded.len(), 1);
        let stored = loaded.get("acc-1").expect("acc-1 present");
        assert_eq!(stored.access_token, "access-token-1");
        assert_eq!(stored.refresh_token, "refresh-token-1");

        store.delete_tokens("acc-1").expect("delete tokens");
        let after_delete = store.load_all_tokens().expect("load after delete");
        assert!(after_delete.is_empty());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn refuses_empty_tokens() {
        let (store, dir) = test_store();
        let empty_tokens = Tokens {
            id_token: "".to_string(),
            access_token: "".to_string(),
            refresh_token: "".to_string(),
        };
        let result = store.store_tokens("empty", &empty_tokens);
        assert!(result.is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn recovers_from_backup_on_corruption() {
        let (store, dir) = test_store();

        let tokens1 = Tokens {
            id_token: "id-1".to_string(),
            access_token: "acc-1".to_string(),
            refresh_token: "ref-1".to_string(),
        };
        let tokens2 = Tokens {
            id_token: "id-2".to_string(),
            access_token: "acc-2".to_string(),
            refresh_token: "ref-2".to_string(),
        };

        store.store_tokens("acc-1", &tokens1).expect("store 1");
        store.store_tokens("acc-2", &tokens2).expect("store 2");

        // Corrupt main vault file
        fs::write(&store.vault_path, b"corrupted payload").expect("corrupt vault");

        // Should recover from backup
        let loaded = store.load_all_tokens().expect("load after corrupt");
        assert!(loaded.contains_key("acc-1"));

        fs::remove_dir_all(dir).ok();
    }
}
