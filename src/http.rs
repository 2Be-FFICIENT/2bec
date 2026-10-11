//! HTTP client for the API: bearer auth, problem+json errors, rate-limit
//! headers and bounded retries on transient failures.

#![expect(
    dead_code,
    reason = "writes and downloads are used by the next runs commands"
)]

use anyhow::{Context, Result};
use reqwest::header::{
    AUTHORIZATION, CONTENT_TYPE, ETAG, HeaderMap, HeaderValue, IF_MATCH, LOCATION, RETRY_AFTER,
    USER_AGENT,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::Write;
use std::time::{Duration, Instant};

use crate::api::error::ApiError;

/// Default production base URL of the 2Be-FFICIENT API.
pub const DEFAULT_API_URL: &str = "https://api.2befficient.com";

/// `User-Agent` sent with every request; the API groups usage by product token.
const USER_AGENT_VALUE: &str = concat!("2bec/", env!("CARGO_PKG_VERSION"));

/// Attempts per request, including the first one.
const MAX_ATTEMPTS: u32 = 3;
/// Longest pause between attempts, whatever `Retry-After` says.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

pub struct Client {
    inner: reqwest::Client,
    api_url: String,
}

/// Rate-limit state the API reports on every authorized answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    /// Seconds until the window resets.
    pub reset_secs: u64,
}

/// A successful answer with the headers commands care about.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: String,
    pub rate_limit: Option<RateLimit>,
    /// `ETag` of the answered resource, when the route versions it.
    pub etag: Option<String>,
    /// `Location` of a created resource.
    pub location: Option<String>,
}

impl Response {
    /// Deserialize the JSON body.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, ApiError> {
        Ok(serde_json::from_str(&self.body)?)
    }
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

    /// `GET` a path, retrying transient failures, and classify the answer.
    pub async fn get(&self, path: &str) -> Result<Response, ApiError> {
        self.get_with(path, &[]).await
    }

    /// `GET` with query parameters (URL-encoded by reqwest).
    pub async fn get_with(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Response, ApiError> {
        let url = self.url(path);
        self.send_with_retry(|| self.inner.get(&url).query(query))
            .await
            .map_err(|e| e.with_scope_hint("GET", path))
    }

    async fn send_with_retry<F>(&self, build: F) -> Result<Response, ApiError>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let mut attempt = 1;
        loop {
            let err = match build().send().await {
                Ok(r) => match Self::classify(r).await {
                    Ok(resp) => return Ok(resp),
                    Err(e) => e,
                },
                Err(e) => ApiError::Network(e),
            };
            if attempt >= MAX_ATTEMPTS || !err.is_transient() {
                return Err(err);
            }
            tokio::time::sleep(backoff(attempt, &err)).await;
            attempt += 1;
        }
    }

    /// `POST` a JSON body. Writes are retried only when `idempotency_key` is
    /// set, since the API then replays the first answer instead of acting twice.
    pub async fn post_json<B: Serialize>(
        &self,
        path: &str,
        body: &B,
        write: &WriteOptions,
    ) -> Result<Response, ApiError> {
        let url = self.url(path);
        self.send_write(write, || self.inner.post(&url).json(body))
            .await
            .map_err(|e| e.with_scope_hint("POST", path))
    }

    /// `PATCH` with a JSON merge patch body (`application/merge-patch+json`).
    pub async fn patch_merge<B: Serialize>(
        &self,
        path: &str,
        body: &B,
        write: &WriteOptions,
    ) -> Result<Response, ApiError> {
        let url = self.url(path);
        let raw = serde_json::to_vec(body)?;
        self.send_write(write, || {
            self.inner
                .patch(&url)
                .header(CONTENT_TYPE, "application/merge-patch+json")
                .body(raw.clone())
        })
        .await
        .map_err(|e| e.with_scope_hint("PATCH", path))
    }

    /// `DELETE` a resource.
    pub async fn delete(&self, path: &str, write: &WriteOptions) -> Result<Response, ApiError> {
        let url = self.url(path);
        self.send_write(write, || self.inner.delete(&url))
            .await
            .map_err(|e| e.with_scope_hint("DELETE", path))
    }

    async fn send_write<F>(&self, write: &WriteOptions, build: F) -> Result<Response, ApiError>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let decorate = || {
            let mut b = build();
            if let Some(k) = &write.idempotency_key {
                b = b.header("Idempotency-Key", k);
            }
            if let Some(m) = &write.if_match {
                b = b.header(IF_MATCH, m);
            }
            b
        };
        if write.idempotency_key.is_some() {
            self.send_with_retry(decorate).await
        } else {
            match decorate().send().await {
                Ok(r) => Self::classify(r).await,
                Err(e) => Err(ApiError::Network(e)),
            }
        }
    }

    /// `GET` a binary resource and stream it into `sink`. Returns the
    /// `Content-Type` and the number of bytes written.
    pub async fn download(
        &self,
        path: &str,
        sink: &mut dyn Write,
    ) -> Result<(Option<String>, u64), ApiError> {
        self.download_with(path, &[], sink).await
    }

    /// `download` with query parameters (URL-encoded by reqwest).
    pub async fn download_with(
        &self,
        path: &str,
        query: &[(&str, String)],
        sink: &mut dyn Write,
    ) -> Result<(Option<String>, u64), ApiError> {
        let url = self.url(path);
        let mut r = self.inner.get(&url).query(query).send().await?;
        let status = r.status().as_u16();
        if !(200..300).contains(&status) {
            let retry_after = header_u64(r.headers(), RETRY_AFTER.as_str());
            let body = r.text().await?;
            return Err(
                ApiError::from_response(status, retry_after, &body).with_scope_hint("GET", path)
            );
        }
        let content_type = header_string(r.headers(), CONTENT_TYPE.as_str());
        let mut written = 0u64;
        while let Some(chunk) = r.chunk().await? {
            sink.write_all(&chunk).map_err(ApiError::Io)?;
            written += chunk.len() as u64;
        }
        sink.flush().map_err(ApiError::Io)?;
        Ok((content_type, written))
    }

    /// Turn a response into `Ok` for 2xx or a typed `ApiError` otherwise.
    async fn classify(r: reqwest::Response) -> Result<Response, ApiError> {
        let status = r.status().as_u16();
        let rate_limit = parse_rate_limit(r.headers());
        let retry_after = header_u64(r.headers(), RETRY_AFTER.as_str());
        let etag = header_string(r.headers(), ETAG.as_str());
        let location = header_string(r.headers(), LOCATION.as_str());
        let body = r.text().await?;
        if (200..300).contains(&status) {
            Ok(Response {
                status,
                body,
                rate_limit,
                etag,
                location,
            })
        } else {
            Err(ApiError::from_response(status, retry_after, &body))
        }
    }

    /// Probe the API root (`GET /v2`), which answers anonymously.
    pub async fn ping(&self) -> Result<Ping, ApiError> {
        let url = self.url("v2");
        let start = Instant::now();
        let r = self.inner.get(&url).send().await?;
        let latency_ms = start.elapsed().as_millis();
        let status = r.status().as_u16();
        let body = r.text().await?;
        Ok(Ping {
            url,
            status,
            latency_ms,
            body,
        })
    }
}

/// Pause before retry `attempt`: `Retry-After` when given, else exponential
/// from one second, capped at [`MAX_BACKOFF`].
fn backoff(attempt: u32, err: &ApiError) -> Duration {
    let wanted = match err {
        ApiError::RateLimited {
            retry_after: Some(s),
            ..
        } => Duration::from_secs(*s),
        _ => Duration::from_secs(1 << (attempt - 1)),
    };
    wanted.min(MAX_BACKOFF)
}

/// Headers that control a write: `Idempotency-Key` makes the API replay the
/// first answer to a repeated request, `If-Match` guards against lost updates.
#[derive(Debug, Clone, Default)]
pub struct WriteOptions {
    pub idempotency_key: Option<String>,
    pub if_match: Option<String>,
}

impl WriteOptions {
    /// A fresh random idempotency key for a single logical write.
    pub fn idempotent() -> Self {
        Self {
            idempotency_key: Some(random_key()),
            if_match: None,
        }
    }
}

/// 32 hex characters from the OS random source, enough for an idempotency key.
fn random_key() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::with_capacity(32);
    for _ in 0..2 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as u64),
        );
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_string)
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

fn parse_rate_limit(headers: &HeaderMap) -> Option<RateLimit> {
    Some(RateLimit {
        limit: header_u64(headers, "ratelimit-limit")?,
        remaining: header_u64(headers, "ratelimit-remaining")?,
        reset_secs: header_u64(headers, "ratelimit-reset")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_headers_parse_together_or_not_at_all() {
        let mut h = HeaderMap::new();
        h.insert("RateLimit-Limit", HeaderValue::from_static("600"));
        h.insert("RateLimit-Remaining", HeaderValue::from_static("599"));
        assert_eq!(parse_rate_limit(&h), None);
        h.insert("RateLimit-Reset", HeaderValue::from_static(" 42"));
        assert_eq!(
            parse_rate_limit(&h),
            Some(RateLimit {
                limit: 600,
                remaining: 599,
                reset_secs: 42
            })
        );
    }

    #[test]
    fn random_keys_are_hex_and_distinct() {
        let a = random_key();
        let b = random_key();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn backoff_honours_retry_after_and_caps() {
        let limited = ApiError::RateLimited {
            problem: Box::default(),
            retry_after: Some(5),
        };
        assert_eq!(backoff(1, &limited), Duration::from_secs(5));
        let huge = ApiError::RateLimited {
            problem: Box::default(),
            retry_after: Some(600),
        };
        assert_eq!(backoff(1, &huge), MAX_BACKOFF);
        let status = ApiError::Status {
            status: 503,
            body: String::new(),
        };
        assert_eq!(backoff(1, &status), Duration::from_secs(1));
        assert_eq!(backoff(2, &status), Duration::from_secs(2));
    }
}
