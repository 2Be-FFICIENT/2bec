//! Authentication: credential storage and token resolution.

pub mod oauth;
pub mod store;
pub mod token;

use anyhow::Result;
use std::fmt;

use crate::api::error::{ApiError, Problem};
use crate::config::Settings;
use store::{CredentialStore, StoredCredential};

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

/// Refresh state of a person login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthState {
    pub refresh_token: Option<String>,
    pub expires_at: u64,
    pub issuer: String,
}

/// A token together with its origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub token: String,
    pub source: TokenSource,
    /// Present for a person login; `None` for an API key or an override.
    pub oauth: Option<OAuthState>,
}

impl Credential {
    fn from_stored(stored: StoredCredential) -> Self {
        match stored {
            StoredCredential::ApiKey { token } => Self {
                token,
                source: TokenSource::Stored,
                oauth: None,
            },
            StoredCredential::OAuth {
                access_token,
                refresh_token,
                expires_at,
                issuer,
            } => Self {
                token: access_token,
                source: TokenSource::Stored,
                oauth: Some(OAuthState {
                    refresh_token,
                    expires_at,
                    issuer,
                }),
            },
        }
    }

    /// Whether the access token expires within `margin_secs`.
    pub fn expires_within(&self, margin_secs: u64) -> bool {
        self.oauth
            .as_ref()
            .is_some_and(|o| o.expires_at <= oauth::now_unix() + margin_secs)
    }
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
            oauth: None,
        }));
    }
    Ok(store.get(profile)?.map(Credential::from_stored))
}

/// Renew a person login whose access token is about to expire, persist the
/// rotated tokens and update `credential`. Failures are reported as a
/// warning: the command then runs with the old token and the API's own
/// `token-expired` answer tells the user to log in again.
pub async fn refresh_if_needed(
    credential: &mut Option<Credential>,
    settings: &Settings,
    store: &dyn CredentialStore,
    http: &reqwest::Client,
) {
    let Some(c) = credential.as_mut() else { return };
    if !c.expires_within(60) {
        return;
    }
    let Some(state) = c.oauth.as_ref() else {
        return;
    };
    let Some(refresh_token) = state.refresh_token.clone() else {
        eprintln!("warning: the login has expired and cannot be renewed; run `2bec auth login`");
        return;
    };
    let issuer = state.issuer.clone();
    match renew(http, settings, &issuer, &refresh_token).await {
        Ok(stored) => {
            if let Err(e) = store.set(&settings.profile, &stored) {
                eprintln!("warning: cannot save the renewed login: {e}");
            }
            *c = Credential::from_stored(stored);
        }
        Err(e) => eprintln!("warning: cannot renew the login ({e}); run `2bec auth login`"),
    }
}

async fn renew(
    http: &reqwest::Client,
    settings: &Settings,
    issuer: &str,
    refresh_token: &str,
) -> Result<StoredCredential> {
    let endpoints = oauth::discover(http, &settings.api_url, Some(issuer)).await?;
    let cfg = settings.login_config(false);
    let t = oauth::refresh(http, &endpoints, &cfg, refresh_token).await?;
    Ok(StoredCredential::OAuth {
        access_token: t.access_token,
        refresh_token: t.refresh_token,
        expires_at: t.expires_at,
        issuer: endpoints.issuer,
    })
}

/// The error for "no credential at all", shaped like the API's own
/// `api-key-required` answer so it exits with the auth code.
pub fn missing_credential() -> ApiError {
    ApiError::Auth(Box::new(Problem {
        type_: "urn:pegas:problem:api-key-required".into(),
        title: "Authentication required".into(),
        status: 401,
        ..Problem::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemStore(RefCell<HashMap<String, StoredCredential>>);

    impl MemStore {
        fn put(&self, profile: &str, token: &str) {
            self.0.borrow_mut().insert(
                profile.into(),
                StoredCredential::ApiKey {
                    token: token.into(),
                },
            );
        }
    }

    impl CredentialStore for MemStore {
        fn get(&self, profile: &str) -> Result<Option<StoredCredential>> {
            Ok(self.0.borrow().get(profile).cloned())
        }
        fn set(&self, profile: &str, credential: &StoredCredential) -> Result<()> {
            self.0
                .borrow_mut()
                .insert(profile.into(), credential.clone());
            Ok(())
        }
        fn remove(&self, profile: &str) -> Result<bool> {
            Ok(self.0.borrow_mut().remove(profile).is_some())
        }
        fn profiles(&self) -> Result<Vec<String>> {
            Ok(self.0.borrow().keys().cloned().collect())
        }
        fn describe(&self) -> String {
            "memory".into()
        }
    }

    #[test]
    fn override_wins_over_store() {
        let store = MemStore::default();
        store.put("default", "stored");
        let c = resolve(Some("flag"), "default", &store).unwrap().unwrap();
        assert_eq!(c.token, "flag");
        assert_eq!(c.source, TokenSource::Override);
    }

    #[test]
    fn blank_override_falls_back_to_store() {
        let store = MemStore::default();
        store.put("default", "stored");
        let c = resolve(Some("  "), "default", &store).unwrap().unwrap();
        assert_eq!(c.token, "stored");
        assert_eq!(c.source, TokenSource::Stored);
    }

    #[test]
    fn oauth_credentials_know_their_expiry() {
        let store = MemStore::default();
        store
            .set(
                "p",
                &StoredCredential::OAuth {
                    access_token: "acc".into(),
                    refresh_token: Some("ref".into()),
                    expires_at: oauth::now_unix() + 30,
                    issuer: "https://id".into(),
                },
            )
            .unwrap();
        let c = resolve(None, "p", &store).unwrap().unwrap();
        assert_eq!(c.token, "acc");
        assert!(c.expires_within(60));
        assert!(!c.expires_within(0));
        let k = resolve(Some("key"), "p", &store).unwrap().unwrap();
        assert!(!k.expires_within(u64::MAX / 2));
    }

    #[test]
    fn store_is_per_profile() {
        let store = MemStore::default();
        store.put("dev", "d");
        assert!(resolve(None, "prod", &store).unwrap().is_none());
        assert_eq!(resolve(None, "dev", &store).unwrap().unwrap().token, "d");
    }
}
