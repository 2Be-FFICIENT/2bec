//! Persistent storage of API credentials, one per profile.
//!
//! The default backend keeps tokens in `hosts.toml` next to `config.toml`,
//! written with mode 0600. The [`CredentialStore`] trait keeps commands
//! independent of the backend so an OS keyring can be added later.

#![expect(
    dead_code,
    reason = "the write side is wired by `auth login` / `auth logout`"
)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// File name of the credential store inside the config directory.
pub const STORE_FILE: &str = "hosts.toml";

/// Backend-agnostic access to stored tokens, keyed by profile name.
pub trait CredentialStore {
    /// The stored token for `profile`, if any.
    fn get(&self, profile: &str) -> Result<Option<String>>;
    /// Store (or replace) the token for `profile`.
    fn set(&self, profile: &str, token: &str) -> Result<()>;
    /// Remove the token for `profile`; returns whether one existed.
    fn remove(&self, profile: &str) -> Result<bool>;
    /// Human-readable location, shown by `auth status`.
    fn describe(&self) -> String;
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Hosts {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    profiles: BTreeMap<String, Entry>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    token: String,
}

/// Credential store backed by a TOML file with owner-only permissions.
#[derive(Debug, Clone)]
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// Store living in `dir` (the 2bec config directory).
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            path: dir.join(STORE_FILE),
        }
    }

    fn load(&self) -> Result<Hosts> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Hosts::default()),
            Err(e) => {
                return Err(e).with_context(|| format!("cannot read {}", self.path.display()));
            }
        };
        toml::from_str(&text)
            .with_context(|| format!("invalid credential store {}", self.path.display()))
    }

    fn save(&self, hosts: &Hosts) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let text = toml::to_string(hosts).context("cannot serialize credential store")?;
        // Write to a sibling temp file with 0600, then rename over the target so
        // the secret never exists on disk with broader permissions.
        let tmp = self.path.with_extension("toml.tmp");
        write_private(&tmp, text.as_bytes())?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("cannot write {}", self.path.display()))
    }
}

impl CredentialStore for FileStore {
    fn get(&self, profile: &str) -> Result<Option<String>> {
        Ok(self.load()?.profiles.remove(profile).map(|e| e.token))
    }

    fn set(&self, profile: &str, token: &str) -> Result<()> {
        let mut hosts = self.load()?;
        hosts.profiles.insert(
            profile.to_string(),
            Entry {
                token: token.to_string(),
            },
        );
        self.save(&hosts)
    }

    fn remove(&self, profile: &str) -> Result<bool> {
        let mut hosts = self.load()?;
        let existed = hosts.profiles.remove(profile).is_some();
        if existed {
            self.save(&hosts)?;
        }
        Ok(existed)
    }

    fn describe(&self) -> String {
        self.path.display().to_string()
    }
}

/// Create or truncate `path` with mode 0600 and write `bytes` to it.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("cannot create {}", path.display()))?;
    #[cfg(unix)]
    {
        // The mode above only applies on creation; tighten an existing file too.
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("cannot set permissions on {}", path.display()))?;
    }
    file.write_all(bytes)
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, FileStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::in_dir(dir.path());
        (dir, store)
    }

    #[test]
    fn missing_file_has_no_token() {
        let (_d, store) = temp_store();
        assert_eq!(store.get("default").unwrap(), None);
        assert!(!store.remove("default").unwrap());
    }

    #[test]
    fn set_get_remove_round_trip() {
        let (_d, store) = temp_store();
        store.set("prod", "2bef_live_abc").unwrap();
        store.set("dev", "2bef_dev_xyz").unwrap();
        assert_eq!(store.get("prod").unwrap().as_deref(), Some("2bef_live_abc"));
        assert_eq!(store.get("dev").unwrap().as_deref(), Some("2bef_dev_xyz"));
        assert!(store.remove("prod").unwrap());
        assert_eq!(store.get("prod").unwrap(), None);
        assert_eq!(store.get("dev").unwrap().as_deref(), Some("2bef_dev_xyz"));
    }

    #[test]
    fn set_replaces_existing_token() {
        let (_d, store) = temp_store();
        store.set("prod", "old").unwrap();
        store.set("prod", "new").unwrap();
        assert_eq!(store.get("prod").unwrap().as_deref(), Some("new"));
    }

    #[cfg(unix)]
    #[test]
    fn file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, store) = temp_store();
        store.set("prod", "secret").unwrap();
        let mode = std::fs::metadata(dir.path().join(STORE_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(!dir.path().join("hosts.toml.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let (dir, store) = temp_store();
        std::fs::write(dir.path().join(STORE_FILE), "not = [toml").unwrap();
        let err = store.get("prod").unwrap_err();
        assert!(err.to_string().contains("invalid credential store"));
    }
}
