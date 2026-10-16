//! API keys: one encrypted store, its master key, a client per server.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ai_pool::{AiClient, ExposeSecret, KeyMetadata, KeyStatus, MasterKeyProvider, ResolvedKey};
use ai_pool::{SecretBox, VaultError};
use mareader_settings::Provider;
use mareader_study::{Failure, Why};
use tokio::sync::{Mutex, OnceCell};

use crate::board::{KeyRow, KeyState};

/// The keychain entry's service; the app's bundle id.
pub const KEYCHAIN_SERVICE: &str = "com.codewiththiha.mareader";
const STORE: &str = "keys.sqlite";
const MASTER_FILE: &str = "master.key";
/// Marks a keychain key, so a locked keychain isn't replaced by a file.
const IN_KEYCHAIN: &str = "keychain";

/// The master key: a file if one exists, else the keychain, else a new file.
pub fn master_key(
    root: &Path,
    keychain: impl FnOnce() -> Result<ResolvedKey, VaultError>,
) -> Result<SecretBox<[u8; 32]>, VaultError> {
    let file = root.join(MASTER_FILE);
    let marker = root.join(IN_KEYCHAIN);
    if file.exists() {
        return MasterKeyProvider::File(file).resolve().map(|k| k.key);
    }
    if marker.exists() {
        return keychain().map(|k| k.key);
    }
    match keychain() {
        Ok(k) => {
            // Without the marker a locked keychain later looks like none.
            let _ = std::fs::create_dir_all(root);
            let _ = std::fs::write(&marker, "The master key is in the keychain.\n");
            Ok(k.key)
        }
        Err(_) => MasterKeyProvider::File(file).resolve().map(|k| k.key),
    }
}

fn os_keychain() -> Result<ResolvedKey, VaultError> {
    MasterKeyProvider::os_keychain(KEYCHAIN_SERVICE).resolve()
}

/// Clients by provider and address, built once each.
pub struct Clients {
    root: PathBuf,
    /// Off, the master key stays in a file: for tests and portable setups.
    keychain: bool,
    /// Kept once found; a locked keychain is asked again next time.
    master: OnceCell<SecretBox<[u8; 32]>>,
    built: Mutex<HashMap<(Provider, String, bool), Arc<AiClient>>>,
}

impl Clients {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            keychain: true,
            master: OnceCell::new(),
            built: Mutex::new(HashMap::new()),
        }
    }

    /// Keeps the master key in a file, never the keychain.
    pub fn without_keychain(mut self) -> Self {
        self.keychain = false;
        self
    }

    async fn master(&self) -> Result<&SecretBox<[u8; 32]>, Failure> {
        let (root, keychain) = (self.root.clone(), self.keychain);
        let resolve = || async move {
            let task = tokio::task::spawn_blocking(move || match keychain {
                true => master_key(&root, os_keychain),
                false => master_key(&root, || Err(VaultError::Keychain("off".into()))),
            });
            match task.await {
                Ok(Ok(key)) => Ok(key),
                Ok(Err(VaultError::KeychainLocked(m))) => Err(Failure::detailed(Why::Locked, m)),
                Ok(Err(e)) => Err(Failure::detailed(Why::Setup, e.to_string())),
                Err(e) => Err(Failure::detailed(Why::Setup, e.to_string())),
            }
        };
        self.master.get_or_try_init(resolve).await
    }

    /// `p`'s client at `url`: with its stored keys, or keyless.
    pub async fn client(
        &self,
        p: Provider,
        url: &str,
        keyed: bool,
    ) -> Result<Arc<AiClient>, Failure> {
        let url = checked(url)?;
        let mut built = self.built.lock().await;
        let id = (p, url.to_owned(), keyed);
        if let Some(c) = built.get(&id) {
            return Ok(Arc::clone(c));
        }
        let local = p.local() || url.contains("://localhost") || url.contains("://127.");
        let mut b = AiClient::builder()
            .default_url(url)
            .request_timeout(Duration::from_secs(if local { 180 } else { 60 }))
            .connect_timeout(Duration::from_secs(if local { 3 } else { 10 }));
        if p == Provider::OpenRouter {
            b = b
                .header("HTTP-Referer", "https://github.com/codewiththiha/mareader2")
                .header("X-Title", "Mareader");
        }
        b = match keyed {
            true => {
                let master = self.master().await?;
                let provider = MasterKeyProvider::custom_from_slice(master.expose_secret())
                    .map_err(|e| Failure::detailed(Why::Setup, e.to_string()))?;
                b.key_scope(p.scope())
                    .sqlite_store(self.root.join(STORE))
                    .master_key_provider(provider)
            }
            false => b.keyless(),
        };
        let client = Arc::new(b.build().await.map_err(|e| crate::compat::failure(&e))?);
        built.insert(id, Arc::clone(&client));
        Ok(client)
    }

    /// The client to ask with; a custom server without keys goes keyless.
    pub async fn asking(&self, p: Provider, url: &str) -> Result<Arc<AiClient>, Failure> {
        if !p.takes_keys() {
            return self.client(p, url, false).await;
        }
        let keyed = self.client(p, url, true).await?;
        if p == Provider::Custom
            && keyed
                .manager()
                .list_keys()
                .await
                .is_ok_and(|k| k.is_empty())
        {
            return self.client(p, url, false).await;
        }
        Ok(keyed)
    }
}

/// `url` if it can be asked: http or https, nothing else.
fn checked(url: &str) -> Result<&str, Failure> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Failure::detailed(
            Why::Setup,
            "the server has no address yet",
        ));
    }
    let scheme = url.split_once("://").map(|(s, _)| s.to_ascii_lowercase());
    match scheme.as_deref() {
        Some("http" | "https") => Ok(url),
        _ => Err(Failure::detailed(
            Why::Setup,
            "the address must start with http:// or https://",
        )),
    }
}

/// A key as the settings show it.
pub fn row(meta: KeyMetadata) -> KeyRow {
    let state = match meta.status {
        KeyStatus::Active => KeyState::Ready,
        KeyStatus::Cooldown { remaining_ms } => KeyState::Resting {
            secs: remaining_ms.div_ceil(1000),
        },
        KeyStatus::Banned { reason } => KeyState::Banned { reason },
    };
    KeyRow {
        id: meta.id,
        shown: meta.censored_key,
        state,
    }
}

/// A pasted key, trimmed; refused if it can't be one.
pub fn pasted(key: &str) -> Result<&str, Failure> {
    let key = key.trim();
    let one_word = !key.is_empty() && !key.chars().any(char::is_whitespace);
    if one_word && key.is_ascii() && key.len() <= 1_024 {
        Ok(key)
    } else {
        let why = "a key is one word of plain letters, digits and signs";
        Err(Failure::detailed(Why::Setup, why))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn fake(byte: u8) -> Result<ResolvedKey, VaultError> {
        Ok(ResolvedKey {
            key: SecretBox::new(Box::new([byte; 32])),
            fresh: false,
        })
    }

    fn locked() -> Result<ResolvedKey, VaultError> {
        Err(VaultError::KeychainLocked("locked".into()))
    }

    #[test]
    fn the_keychain_is_tried_first_and_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let first = master_key(dir.path(), || fake(7)).unwrap();
        assert_eq!(first.expose_secret(), &[7; 32]);
        assert!(dir.path().join(IN_KEYCHAIN).exists());
        // Locked later: reported, never swapped for a new file.
        let later = master_key(dir.path(), locked);
        assert!(matches!(later, Err(VaultError::KeychainLocked(_))));
        assert!(!dir.path().join(MASTER_FILE).exists());
    }

    #[test]
    fn without_a_keychain_a_private_file_holds_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let first = master_key(dir.path(), locked).unwrap();
        assert!(dir.path().join(MASTER_FILE).exists());
        let asked = Cell::new(false);
        let again = master_key(dir.path(), || {
            asked.set(true);
            fake(1)
        })
        .unwrap();
        assert_eq!(again.expose_secret(), first.expose_secret());
        assert!(!asked.get(), "the file wins once it exists");
    }

    #[test]
    fn addresses_and_keys_are_checked_before_use() {
        assert!(checked(" https://api.openai.com/v1/ ").is_ok());
        assert!(checked("HTTP://box:1234/v1").is_ok());
        assert!(checked("").is_err());
        assert!(checked("ftp://x").is_err());
        assert!(checked("localhost:11434").is_err());
        assert_eq!(pasted("  sk-abc  ").ok(), Some("sk-abc"));
        assert!(pasted("sk abc").is_err());
        assert!(pasted("").is_err());
        assert!(pasted("ключ").is_err());
    }
}
