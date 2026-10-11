//! Authentication: credential storage and token resolution.

pub mod store;

use anyhow::Result;
use std::fmt;

use store::CredentialStore;

/// Where the active token came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    /// `--token` flag or `TWOBEC_TOKEN` environment variable.
    Override,
    /// The credential store (`auth login`).
    Stored,
}

impl fmt::Display for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Override => f.write_str("--token / TWOBEC_TOKEN"),
            Self::Stored => f.write_str("credential store"),
        }
    }
}

/// A token together with its origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub token: String,
    pub source: TokenSource,
}

/// Pick the token for `profile`: an explicit override wins over the store.
/// Blank overrides are ignored so `TWOBEC_TOKEN=` behaves like unset.
pub fn resolve(
    override_token: Option<&str>,
    profile: &str,
    store: &dyn CredentialStore,
) -> Result<Option<Credential>> {
    if let Some(t) = override_token.map(str::trim).filter(|t| !t.is_empty()) {
        return Ok(Some(Credential {
            token: t.to_string(),
            source: TokenSource::Override,
        }));
    }
    Ok(store.get(profile)?.map(|token| Credential {
        token,
        source: TokenSource::Stored,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemStore(RefCell<HashMap<String, String>>);

    impl CredentialStore for MemStore {
        fn get(&self, profile: &str) -> Result<Option<String>> {
            Ok(self.0.borrow().get(profile).cloned())
        }
        fn set(&self, profile: &str, token: &str) -> Result<()> {
            self.0.borrow_mut().insert(profile.into(), token.into());
            Ok(())
        }
        fn remove(&self, profile: &str) -> Result<bool> {
            Ok(self.0.borrow_mut().remove(profile).is_some())
        }
        fn describe(&self) -> String {
            "memory".into()
        }
    }

    #[test]
    fn override_wins_over_store() {
        let store = MemStore::default();
        store.set("default", "stored").unwrap();
        let c = resolve(Some("flag"), "default", &store).unwrap().unwrap();
        assert_eq!(c.token, "flag");
        assert_eq!(c.source, TokenSource::Override);
    }

    #[test]
    fn blank_override_falls_back_to_store() {
        let store = MemStore::default();
        store.set("default", "stored").unwrap();
        let c = resolve(Some("  "), "default", &store).unwrap().unwrap();
        assert_eq!(c.token, "stored");
        assert_eq!(c.source, TokenSource::Stored);
    }

    #[test]
    fn store_is_per_profile() {
        let store = MemStore::default();
        store.set("dev", "d").unwrap();
        assert!(resolve(None, "prod", &store).unwrap().is_none());
        assert_eq!(resolve(None, "dev", &store).unwrap().unwrap().token, "d");
    }
}
