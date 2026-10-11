//! Command implementations, one module per resource.

pub mod alerts;
pub mod auth;
pub mod metrics;
pub mod runs;
pub mod scenarios;
pub mod webhooks;

use anyhow::Result;

use crate::auth::store::FileStore;
use crate::auth::{Credential, missing_credential};
use crate::config::Settings;
use crate::http::Client;

/// A plain `reqwest` client with the tool's `User-Agent` and timeout.
pub fn plain_http(timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .user_agent(concat!("2bec/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {e}"))
}

/// A JSON body argument: a literal, `-` for stdin, or `@path` for a file.
pub fn read_json_body(arg: &str) -> Result<serde_json::Value> {
    let text = runs::read_text(arg)?;
    if text.trim().is_empty() {
        anyhow::bail!("the body is empty");
    }
    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("the body is not valid JSON: {e}"))
}

/// Stream `path` into `target`: stdout for `-`, else a temp file renamed
/// into place on success so a failed download leaves nothing behind.
pub async fn download_to(client: &Client, path: &str, target: &str) -> Result<()> {
    if target == "-" {
        let mut stdout = std::io::stdout().lock();
        client.download(path, &mut stdout).await?;
        return Ok(());
    }
    let tmp = format!("{target}.part");
    let mut file =
        std::fs::File::create(&tmp).map_err(|e| anyhow::anyhow!("cannot create {tmp}: {e}"))?;
    let result = client.download(path, &mut file).await;
    drop(file);
    match result {
        Ok((_, bytes)) => {
            std::fs::rename(&tmp, target)
                .map_err(|e| anyhow::anyhow!("cannot write {target}: {e}"))?;
            eprintln!("Saved {target} ({bytes} bytes)");
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.into())
        }
    }
}

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

    /// Unauthenticated client for the identity server and discovery.
    pub fn plain_http(&self) -> Result<reqwest::Client> {
        plain_http(self.timeout)
    }

    /// The credential, or the same error the API gives when none is sent.
    pub fn require_credential(&self) -> Result<&Credential> {
        self.credential
            .as_ref()
            .ok_or_else(|| missing_credential().into())
    }
}
