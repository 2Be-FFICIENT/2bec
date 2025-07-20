//! Thin HTTP backend wrapping `reqwest`.

use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use std::time::{Duration, Instant};

/// Default production base URL of the 2Be-FFICIENT API.
pub const DEFAULT_API_URL: &str = "https://api.2befficient.com";

/// `User-Agent` sent with every request; the API groups usage by product token.
const USER_AGENT_VALUE: &str = concat!("2bec/", env!("CARGO_PKG_VERSION"));

pub struct Client {
    inner: reqwest::Client,
    api_url: String,
}

#[derive(Debug)]
pub struct Ping {
    pub url: String,
    pub status: u16,
    pub latency_ms: u128,
    pub body: String,
}

impl Client {
    pub fn new(api_url: &str, timeout_secs: u64, token: Option<&str>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        if let Some(t) = token {
            let mut v = HeaderValue::from_str(&format!("Bearer {t}"))
                .context("token contains invalid header characters")?;
            v.set_sensitive(true);
            headers.insert(AUTHORIZATION, v);
        }
        let inner = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .default_headers(headers)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            inner,
            api_url: api_url.trim_end_matches('/').to_string(),
        })
    }

    /// Resolve a path against the API URL; absolute URLs are passed through.
    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}/{}", self.api_url, path.trim_start_matches('/'))
        }
    }

    /// Probe the API root (`GET /v2`), which answers anonymously.
    pub async fn ping(&self) -> Result<Ping> {
        let url = self.url("v2");
        let start = Instant::now();
        let r = self
            .inner
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;
        let latency_ms = start.elapsed().as_millis();
        let status = r.status().as_u16();
        let body = r.text().await.context("failed to read response body")?;
        Ok(Ping {
            url,
            status,
            latency_ms,
            body,
        })
    }
}
