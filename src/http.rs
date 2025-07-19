//! Thin HTTP backend wrapping `reqwest`.

use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use std::time::{Duration, Instant};

pub struct Client {
    inner: reqwest::Client,
    base_url: String,
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

#[derive(Debug)]
pub struct Ping {
    pub url: String,
    pub status: u16,
    pub latency_ms: u128,
}

impl Client {
    pub fn new(base_url: &str, timeout_secs: u64, token: Option<&str>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(concat!("twobec/", env!("CARGO_PKG_VERSION"))),
        );
        if let Some(t) = token {
            let v = HeaderValue::from_str(&format!("Bearer {t}"))
                .context("token contains invalid header characters")?;
            headers.insert(AUTHORIZATION, v);
        }
        let inner = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .default_headers(headers)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            inner,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    /// Resolve a path against the base URL; absolute URLs are passed through.
    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}/{}", self.base_url, path.trim_start_matches('/'))
        }
    }

    pub async fn get(&self, path: &str) -> Result<Response> {
        let url = self.url(path);
        let r = self
            .inner
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;
        Self::into_response(r).await
    }

    pub async fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<Response> {
        let url = self.url(path);
        let r = self
            .inner
            .post(&url)
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {url} failed"))?;
        Self::into_response(r).await
    }

    pub async fn ping(&self) -> Result<Ping> {
        let url = self.url("");
        let start = Instant::now();
        let r = self
            .inner
            .head(&url)
            .send()
            .await
            .with_context(|| format!("HEAD {url} failed"))?;
        Ok(Ping {
            url,
            status: r.status().as_u16(),
            latency_ms: start.elapsed().as_millis(),
        })
    }

    async fn into_response(r: reqwest::Response) -> Result<Response> {
        let status = r.status().as_u16();
        let body = r.text().await.context("failed to read response body")?;
        Ok(Response { status, body })
    }
}
