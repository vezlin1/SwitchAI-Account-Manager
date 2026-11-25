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

#[cfg(test)]
#[derive(Default)]
pub struct InMemoryKeyring {
    entries: std::sync::Mutex<HashMap<(String, String), String>>,
}

#[cfg(test)]
impl InMemoryKeyring {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
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
        // Do not treat inaccessible files (or dangling symlinks) as a fresh vault.
        // A missing primary can still have a recoverable encrypted backup.
        match fs::symlink_metadata(&self.vault_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let backup = backup_path(&self.vault_path)?;
                match fs::symlink_metadata(&backup) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(TokenVault::new());
                    }
                    Err(source) => {
                        return Err(AppError::Io {
                            context: "Failed to inspect protected vault backup",
                            source,
                        });
                    }
                    Ok(_) => {}
                }
            }
            Err(source) => {
                return Err(AppError::Io {
                    context: "Failed to inspect protected token vault",
                    source,
                });
            }
            Ok(_) => {}
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

    fn test_tokens(id: &str) -> Tokens {
        Tokens {
            id_token: format!("test-id-{id}"),
            access_token: format!("test-access-{id}"),
            refresh_token: format!("test-refresh-{id}"),
        }
    }

    fn assert_tokens(tokens: &Tokens, id: &str) {
        let expected = test_tokens(id);
        assert_eq!(tokens.id_token, expected.id_token);
        assert_eq!(tokens.access_token, expected.access_token);
        assert_eq!(tokens.refresh_token, expected.refresh_token);
    }

    fn leave_only_backup(store: &SecretStore<InMemoryKeyring>) -> (PathBuf, Vec<u8>) {
        store.store_tokens("acc-1", &test_tokens("1")).unwrap();
        store.store_tokens("acc-2", &test_tokens("2")).unwrap();
        let backup = backup_path(&store.vault_path).unwrap();
        fs::copy(&store.vault_path, &backup).unwrap();
        let bytes = fs::read(&backup).unwrap();
        fs::remove_file(&store.vault_path).unwrap();
        (backup, bytes)
    }

    #[test]
    fn missing_primary_recovers_backup_without_changing_encrypted_bytes() {
        let (store, dir) = test_store();
        let (backup, bytes) = leave_only_backup(&store);

        let loaded = store.load_all_tokens().expect("recover missing primary");
        assert_eq!(loaded.len(), 2);
        assert_tokens(&loaded["acc-1"], "1");
        assert_tokens(&loaded["acc-2"], "2");
        assert_eq!(fs::read(&store.vault_path).unwrap(), bytes);
        assert_eq!(fs::read(&backup).unwrap(), bytes);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_with_missing_primary_preserves_other_accounts_and_recovered_backup() {
        let (store, dir) = test_store();
        let (backup, bytes) = leave_only_backup(&store);

        // The write itself must recover before merging the updated account.
        store
            .store_tokens("acc-1", &test_tokens("updated"))
            .unwrap();
        let loaded = store.load_all_tokens().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_tokens(&loaded["acc-1"], "updated");
        assert_tokens(&loaded["acc-2"], "2");
        assert_eq!(fs::read(&backup).unwrap(), bytes);

        store.store_tokens("acc-3", &test_tokens("3")).unwrap();
        let loaded = store.load_all_tokens().unwrap();
        assert_eq!(loaded.len(), 3);
        assert_tokens(&loaded["acc-1"], "updated");
        assert_tokens(&loaded["acc-2"], "2");
        assert_tokens(&loaded["acc-3"], "3");
        let key = store.load_master_key(false).unwrap().unwrap();
        let previous = store.decrypt_vault(&backup, &key).unwrap();
        assert_eq!(previous.len(), 2);
        assert_tokens(&Tokens::from(previous["acc-1"].clone()), "updated");
        assert_tokens(&Tokens::from(previous["acc-2"].clone()), "2");

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_backup_with_missing_primary_blocks_reads_and_writes() {
        let (store, dir) = test_store();
        let (backup, bytes) = leave_only_backup(&store);
        let original_key = store.load_master_key(false).unwrap();
        let mut tampered: VaultEnvelope = serde_json::from_slice(&bytes).unwrap();
        let mut ciphertext = STANDARD.decode(&tampered.ciphertext).unwrap();
        ciphertext[0] ^= 1;
        tampered.ciphertext = STANDARD.encode(ciphertext);

        for invalid in [
            b"invalid JSON".to_vec(),
            serde_json::to_vec(&tampered).unwrap(),
        ] {
            fs::write(&backup, &invalid).unwrap();
            assert!(store.load_all_tokens().is_err());
            assert!(store.store_tokens("new", &test_tokens("new")).is_err());
            assert!(store.delete_tokens("acc-1").is_err());
            assert!(!store.vault_path.try_exists().unwrap());
            assert_eq!(fs::read(&backup).unwrap(), invalid);
            assert_eq!(store.load_master_key(false).unwrap(), original_key);
        }

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn backup_without_key_cannot_create_a_fresh_vault_or_key() {
        let (store, dir) = test_store();
        let (backup, bytes) = leave_only_backup(&store);
        store
            .backend
            .delete_password(CREDENTIAL_SERVICE, MASTER_KEY_USERNAME)
            .unwrap();

        assert!(
            store
                .load_all_tokens()
                .unwrap_err()
                .user_message()
                .contains("key is missing")
        );
        assert!(store.store_tokens("new", &test_tokens("new")).is_err());
        assert!(store.delete_tokens("acc-1").is_err());
        assert_eq!(store.load_master_key(false).unwrap(), None);
        assert!(!store.vault_path.try_exists().unwrap());
        assert_eq!(fs::read(&backup).unwrap(), bytes);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unreadable_backup_path_cannot_create_a_fresh_vault_or_key() {
        let (store, dir) = test_store();
        let backup = backup_path(&store.vault_path).unwrap();
        // A directory is deterministically unreadable as a vault on every platform.
        fs::create_dir(&backup).unwrap();
        let preserved = backup.join("preserved");
        fs::write(&preserved, b"test data").unwrap();

        assert!(store.load_all_tokens().is_err());
        assert!(store.store_tokens("new", &test_tokens("new")).is_err());
        assert_eq!(store.load_master_key(false).unwrap(), None);
        // Also exercise the read failure with an existing key.
        let key = store.load_master_key(true).unwrap();
        assert!(store.load_all_tokens().is_err());
        assert!(store.store_tokens("new", &test_tokens("new")).is_err());
        assert_eq!(store.load_master_key(false).unwrap(), key);
        assert!(!store.vault_path.try_exists().unwrap());
        assert_eq!(fs::read(&preserved).unwrap(), b"test data");

        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn locked_backup_with_missing_primary_fails_closed_until_readable() {
        use std::os::windows::fs::OpenOptionsExt;

        let (store, dir) = test_store();
        let (backup, bytes) = leave_only_backup(&store);
        let key = store.load_master_key(false).unwrap();
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&backup)
            .unwrap();

        assert!(store.load_all_tokens().is_err());
        assert!(store.store_tokens("new", &test_tokens("new")).is_err());
        assert!(!store.vault_path.try_exists().unwrap());
        assert_eq!(store.load_master_key(false).unwrap(), key);
        drop(locked);
        assert_eq!(fs::read(&backup).unwrap(), bytes);
        let loaded = store.load_all_tokens().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_tokens(&loaded["acc-1"], "1");
        assert_tokens(&loaded["acc-2"], "2");

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn healthy_primary_takes_precedence_over_stale_or_invalid_backup() {
        let (store, dir) = test_store();
        store.store_tokens("acc-1", &test_tokens("1")).unwrap();
        store
            .store_tokens("acc-1", &test_tokens("updated"))
            .unwrap();
        let primary = fs::read(&store.vault_path).unwrap();
        let backup = backup_path(&store.vault_path).unwrap();
        let stale = fs::read(&backup).unwrap();

        for backup_bytes in [stale, b"invalid backup".to_vec()] {
            fs::write(&backup, &backup_bytes).unwrap();
            let loaded = store.load_all_tokens().unwrap();
            assert_eq!(loaded.len(), 1);
            assert_tokens(&loaded["acc-1"], "updated");
            assert_eq!(fs::read(&store.vault_path).unwrap(), primary);
            assert_eq!(fs::read(&backup).unwrap(), backup_bytes);
        }

        fs::remove_dir_all(dir).unwrap();
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
