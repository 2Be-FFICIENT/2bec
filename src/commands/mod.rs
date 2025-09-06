//! Command implementations, one module per resource.

pub mod auth;

use anyhow::Result;

use crate::auth::store::FileStore;
use crate::auth::{Credential, missing_credential};
use crate::config::Settings;
use crate::http::Client;

/// Everything a command needs: resolved settings, the credential store and
/// the credential picked for this invocation.
pub struct Context {
    pub settings: Settings,
    pub store: FileStore,
    pub credential: Option<Credential>,
    pub timeout: u64,
}

impl Context {
    /// HTTP client authenticated with the resolved credential, if any.
    pub fn client(&self) -> Result<Client> {
        self.client_with(self.credential.as_ref().map(|c| c.token.as_str()))
    }

    /// HTTP client authenticated with an explicit token (or none).
    pub fn client_with(&self, token: Option<&str>) -> Result<Client> {
        Client::new(&self.settings.api_url, self.timeout, token)
    }

    /// The credential, or the same error the API gives when none is sent.
    pub fn require_credential(&self) -> Result<&Credential> {
        self.credential
            .as_ref()
            .ok_or_else(|| missing_credential().into())
    }
}
