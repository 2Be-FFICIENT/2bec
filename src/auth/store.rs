//! Persistent storage of API credentials, one per profile.
//!
//! The default backend keeps tokens in `hosts.toml` next to `config.toml`,
//! written with mode 0600. The [`CredentialStore`] trait keeps commands
//! independent of the backend so an OS keyring can be added later.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// File name of the credential store inside the config directory.
pub const STORE_FILE: &str = "hosts.toml";

/// What is kept for a profile: an API key, or a person's OAuth tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredCredential {
    ApiKey {
        token: String,
    },
    OAuth {
        access_token: String,
        refresh_token: Option<String>,
        /// Unix seconds when the access token expires.
        expires_at: u64,
        /// Authorization server the tokens came from.
        issuer: String,
    },
}

/// Backend-agnostic access to stored credentials, keyed by profile name.
pub trait CredentialStore {
    /// The stored credential for `profile`, if any.
    fn get(&self, profile: &str) -> Result<Option<StoredCredential>>;
    /// Store (or replace) the credential for `profile`.
    fn set(&self, profile: &str, credential: &StoredCredential) -> Result<()>;
    /// Remove the credential for `profile`; returns whether one existed.
    fn remove(&self, profile: &str) -> Result<bool>;
    /// Human-readable location, shown by `auth status`.
    fn describe(&self) -> String;
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Hosts {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    profiles: BTreeMap<String, Entry>,
}

/// On-disk shape: `token` for an API key, the other fields for OAuth.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    issuer: Option<String>,
}

impl Entry {
    fn from_credential(c: &StoredCredential) -> Self {
        match c {
            StoredCredential::ApiKey { token } => Self {
                token: Some(token.clone()),
                ..Self::default()
            },
            StoredCredential::OAuth {
                access_token,
                refresh_token,
                expires_at,
                issuer,
            } => Self {
                access_token: Some(access_token.clone()),
                refresh_token: refresh_token.clone(),
                expires_at: Some(*expires_at),
                issuer: Some(issuer.clone()),
                ..Self::default()
            },
        }
    }

    fn into_credential(self) -> Option<StoredCredential> {
        if let Some(token) = self.token {
            return Some(StoredCredential::ApiKey { token });
        }
        Some(StoredCredential::OAuth {
            access_token: self.access_token?,
            refresh_token: self.refresh_token,
            expires_at: self.expires_at.unwrap_or(0),
            issuer: self.issuer.unwrap_or_default(),
        })
    }
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
    fn get(&self, profile: &str) -> Result<Option<StoredCredential>> {
        Ok(self
            .load()?
            .profiles
            .remove(profile)
            .and_then(Entry::into_credential))
    }

    fn set(&self, profile: &str, credential: &StoredCredential) -> Result<()> {
        let mut hosts = self.load()?;
        hosts
            .profiles
            .insert(profile.to_string(), Entry::from_credential(credential));
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

    fn key(t: &str) -> StoredCredential {
        StoredCredential::ApiKey { token: t.into() }
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
        store.set("prod", &key("2bef_live_abc")).unwrap();
        store.set("dev", &key("2bef_dev_xyz")).unwrap();
        assert_eq!(store.get("prod").unwrap(), Some(key("2bef_live_abc")));
        assert_eq!(store.get("dev").unwrap(), Some(key("2bef_dev_xyz")));
        assert!(store.remove("prod").unwrap());
        assert_eq!(store.get("prod").unwrap(), None);
        assert_eq!(store.get("dev").unwrap(), Some(key("2bef_dev_xyz")));
    }

    #[test]
    fn set_replaces_existing_token() {
        let (_d, store) = temp_store();
        store.set("prod", &key("old")).unwrap();
        store.set("prod", &key("new")).unwrap();
        assert_eq!(store.get("prod").unwrap(), Some(key("new")));
    }

    #[test]
    fn oauth_tokens_round_trip_and_key_entries_stay_readable() {
        let (dir, store) = temp_store();
        let oauth = StoredCredential::OAuth {
            access_token: "acc".into(),
            refresh_token: Some("ref".into()),
            expires_at: 1_800_000_000,
            issuer: "https://id.example".into(),
        };
        store.set("prod", &oauth).unwrap();
        assert_eq!(store.get("prod").unwrap(), Some(oauth));
        let text = std::fs::read_to_string(dir.path().join(STORE_FILE)).unwrap();
        assert!(!text.contains("\ntoken = "), "{text}");
        assert!(text.contains("refresh_token = \"ref\""));
        // A file written by an older version holds only `token`.
        std::fs::write(
            dir.path().join(STORE_FILE),
            "[profiles.prod]\ntoken = \"2bef_live_x\"\n",
        )
        .unwrap();
        assert_eq!(store.get("prod").unwrap(), Some(key("2bef_live_x")));
    }

    #[cfg(unix)]
    #[test]
    fn file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, store) = temp_store();
        store.set("prod", &key("secret")).unwrap();
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
