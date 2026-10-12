//! Secrets in the operating system keyring (Secret Service on Linux,
//! Keychain on macOS, Credential Manager on Windows), one entry per
//! profile under the service name `2bec`.

use anyhow::{Context, Result};

use super::store::SecretBackend;

/// Service name the entries are filed under.
const SERVICE: &str = "2bec";

/// The OS keyring as a [`SecretBackend`].
#[derive(Debug, Default)]
pub struct Keyring;

fn entry(profile: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, profile).context("cannot open the OS keyring")
}

impl SecretBackend for Keyring {
    fn get(&self, profile: &str) -> Result<Option<String>> {
        match entry(profile)?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e).context("cannot read the credential from the OS keyring"),
        }
    }

    fn set(&self, profile: &str, secret: &str) -> Result<()> {
        entry(profile)?
            .set_password(secret)
            .context("cannot write the credential to the OS keyring")
    }

    fn remove(&self, profile: &str) -> Result<()> {
        match entry(profile)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e).context("cannot remove the credential from the OS keyring"),
        }
    }

    fn describe(&self) -> String {
        "OS keyring".to_string()
    }
}
