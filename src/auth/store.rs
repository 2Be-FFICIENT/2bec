//! Persistent storage of API credentials, one per profile.
//!
//! `hosts.toml` next to `config.toml`, written with mode 0600, is the index
//! of the profiles that have a credential. By default it also holds the
//! secrets; with a [`SecretBackend`] (the OS keyring, behind the `keyring`
//! cargo feature) the secrets live there and the file keeps only the
//! non-secret fields and a `keyring = true` marker. The [`CredentialStore`]
//! trait keeps commands independent of the backend.

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
    /// Names of the profiles that have a credential, sorted.
    fn profiles(&self) -> Result<Vec<String>>;
    /// Human-readable location, shown by `auth status`.
    fn describe(&self) -> String;
}

/// Where the secret part of a credential is kept, when not in the file.
pub trait SecretBackend {
    /// The secret stored for `profile`, if any.
    fn get(&self, profile: &str) -> Result<Option<String>>;
    /// Store (or replace) the secret for `profile`.
    fn set(&self, profile: &str, secret: &str) -> Result<()>;
    /// Remove the secret for `profile`; a missing one is not an error.
    fn remove(&self, profile: &str) -> Result<()>;
    /// Human-readable name, shown by `auth status`.
    fn describe(&self) -> String;
}

/// The secret part of an entry, serialized as JSON into the backend.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Secret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Hosts {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    profiles: BTreeMap<String, Entry>,
}

/// On-disk shape: `token` for an API key, the other fields for OAuth.
/// With `keyring = true` the secret fields are absent and live in the
/// secret backend.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    keyring: bool,
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

    /// Split into the index part (kept in the file) and the secret part.
    fn split(self) -> (Self, Secret) {
        let secret = Secret {
            token: self.token,
            access_token: self.access_token,
            refresh_token: self.refresh_token,
        };
        let index = Self {
            keyring: true,
            expires_at: self.expires_at,
            issuer: self.issuer,
            ..Self::default()
        };
        (index, secret)
    }

    fn with_secret(mut self, secret: Secret) -> Self {
        self.token = secret.token;
        self.access_token = secret.access_token;
        self.refresh_token = secret.refresh_token;
        self
    }
}

/// Credential store indexed by a TOML file with owner-only permissions; the
/// secrets are in that file too unless a [`SecretBackend`] is attached.
pub struct FileStore {
    path: PathBuf,
    secrets: Option<Box<dyn SecretBackend>>,
}

impl FileStore {
    /// Store living in `dir` (the 2bec config directory), secrets in the file.
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            path: dir.join(STORE_FILE),
            secrets: None,
        }
    }

    /// Keep the secrets in `backend` and only the index in the file.
    #[cfg_attr(not(feature = "keyring"), allow(dead_code))]
    pub fn with_secrets(mut self, backend: Box<dyn SecretBackend>) -> Self {
        self.secrets = Some(backend);
        self
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
        let Some(entry) = self.load()?.profiles.remove(profile) else {
            return Ok(None);
        };
        // An entry written by the other backend is still readable, so a
        // change of `credential_store` keeps the logins until the next one.
        if !entry.keyring {
            return Ok(entry.into_credential());
        }
        let Some(backend) = &self.secrets else {
            anyhow::bail!(
                "the credential for profile `{profile}` is in the OS keyring, which this build \
                 of 2bec cannot read; set `credential_store = \"file\"` and log in again"
            );
        };
        let Some(json) = backend.get(profile)? else {
            return Ok(None);
        };
        let secret: Secret =
            serde_json::from_str(&json).context("the secret in the keyring is not valid JSON")?;
        Ok(entry.with_secret(secret).into_credential())
    }

    fn set(&self, profile: &str, credential: &StoredCredential) -> Result<()> {
        let mut hosts = self.load()?;
        let entry = match &self.secrets {
            None => Entry::from_credential(credential),
            Some(backend) => {
                let (index, secret) = Entry::from_credential(credential).split();
                let json = serde_json::to_string(&secret).context("cannot serialize the secret")?;
                backend.set(profile, &json)?;
                index
            }
        };
        hosts.profiles.insert(profile.to_string(), entry);
        self.save(&hosts)
    }

    fn remove(&self, profile: &str) -> Result<bool> {
        let mut hosts = self.load()?;
        let Some(entry) = hosts.profiles.remove(profile) else {
            return Ok(false);
        };
        if entry.keyring
            && let Some(backend) = &self.secrets
        {
            backend.remove(profile)?;
        }
        self.save(&hosts)?;
        Ok(true)
    }

    fn profiles(&self) -> Result<Vec<String>> {
        Ok(self.load()?.profiles.into_keys().collect())
    }

    fn describe(&self) -> String {
        match &self.secrets {
            None => self.path.display().to_string(),
            Some(b) => format!("{} (index in {})", b.describe(), self.path.display()),
        }
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
        assert_eq!(store.profiles().unwrap(), ["dev", "prod"]);
        assert!(store.remove("prod").unwrap());
        assert_eq!(store.get("prod").unwrap(), None);
        assert_eq!(store.profiles().unwrap(), ["dev"]);
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

    /// A secret backend in memory, standing in for the OS keyring.
    #[derive(Default)]
    struct MemSecrets(std::cell::RefCell<BTreeMap<String, String>>);

    impl SecretBackend for MemSecrets {
        fn get(&self, profile: &str) -> Result<Option<String>> {
            Ok(self.0.borrow().get(profile).cloned())
        }
        fn set(&self, profile: &str, secret: &str) -> Result<()> {
            self.0.borrow_mut().insert(profile.into(), secret.into());
            Ok(())
        }
        fn remove(&self, profile: &str) -> Result<()> {
            self.0.borrow_mut().remove(profile);
            Ok(())
        }
        fn describe(&self) -> String {
            "memory".into()
        }
    }

    #[test]
    fn secret_backend_keeps_the_secrets_out_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = std::rc::Rc::new(MemSecrets::default());
        struct Shared(std::rc::Rc<MemSecrets>);
        impl SecretBackend for Shared {
            fn get(&self, p: &str) -> Result<Option<String>> {
                self.0.get(p)
            }
            fn set(&self, p: &str, s: &str) -> Result<()> {
                self.0.set(p, s)
            }
            fn remove(&self, p: &str) -> Result<()> {
                self.0.remove(p)
            }
            fn describe(&self) -> String {
                self.0.describe()
            }
        }
        let store = FileStore::in_dir(dir.path()).with_secrets(Box::new(Shared(secrets.clone())));
        let oauth = StoredCredential::OAuth {
            access_token: "acc".into(),
            refresh_token: Some("ref".into()),
            expires_at: 1_800_000_000,
            issuer: "https://id.example".into(),
        };
        store.set("prod", &oauth).unwrap();
        store.set("dev", &key("2bef_dev_x")).unwrap();

        let text = std::fs::read_to_string(dir.path().join(STORE_FILE)).unwrap();
        assert!(
            !text.contains("acc") && !text.contains("ref") && !text.contains("2bef_dev_x"),
            "{text}"
        );
        assert!(text.contains("keyring = true"), "{text}");
        assert!(text.contains("issuer = \"https://id.example\""), "{text}");
        assert_eq!(
            secrets.get("prod").unwrap().as_deref(),
            Some(r#"{"access_token":"acc","refresh_token":"ref"}"#)
        );
        assert_eq!(store.get("prod").unwrap(), Some(oauth));
        assert_eq!(store.get("dev").unwrap(), Some(key("2bef_dev_x")));
        assert_eq!(store.profiles().unwrap(), ["dev", "prod"]);
        assert_eq!(
            store.describe(),
            format!(
                "memory (index in {})",
                dir.path().join(STORE_FILE).display()
            )
        );

        assert!(store.remove("prod").unwrap());
        assert_eq!(secrets.get("prod").unwrap(), None);
        assert_eq!(store.get("prod").unwrap(), None);
        assert!(!store.remove("prod").unwrap());
    }

    #[test]
    fn entries_of_the_other_backend_stay_readable() {
        let dir = tempfile::tempdir().unwrap();
        // Written with the secrets in the file, read with a backend attached.
        FileStore::in_dir(dir.path())
            .set("prod", &key("2bef_live_x"))
            .unwrap();
        let with_backend =
            FileStore::in_dir(dir.path()).with_secrets(Box::new(MemSecrets::default()));
        assert_eq!(with_backend.get("prod").unwrap(), Some(key("2bef_live_x")));
        // Written to the backend, read by a build without one: a clear error.
        with_backend.set("prod", &key("2bef_live_y")).unwrap();
        let err = FileStore::in_dir(dir.path()).get("prod").unwrap_err();
        assert!(err.to_string().contains("OS keyring"), "{err}");
        // A missing secret behind a marker reads as logged out.
        with_backend.set("gone", &key("2bef_live_z")).unwrap();
        let fresh = FileStore::in_dir(dir.path()).with_secrets(Box::new(MemSecrets::default()));
        assert_eq!(fresh.get("gone").unwrap(), None);
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let (dir, store) = temp_store();
        std::fs::write(dir.path().join(STORE_FILE), "not = [toml").unwrap();
        let err = store.get("prod").unwrap_err();
        assert!(err.to_string().contains("invalid credential store"));
    }
}
