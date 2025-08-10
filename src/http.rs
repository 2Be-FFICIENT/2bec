//! HTTP client for the API: bearer auth, problem+json errors, rate-limit
//! headers and bounded retries on transient failures.

#![expect(dead_code, reason = "first caller is `auth status` via GET /v2/me")]

use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER, USER_AGENT};
use serde::de::DeserializeOwned;
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
}

impl Response {
    /// Deserialize the JSON body.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.body).context("API answered with unexpected JSON")
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
        let url = self.url(path);
        self.send_with_retry(|| self.inner.get(&url)).await
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

    /// Turn a response into `Ok` for 2xx or a typed `ApiError` otherwise.
    async fn classify(r: reqwest::Response) -> Result<Response, ApiError> {
        let status = r.status().as_u16();
        let rate_limit = parse_rate_limit(r.headers());
        let retry_after = header_u64(r.headers(), RETRY_AFTER.as_str());
        let body = r.text().await?;
        if (200..300).contains(&status) {
            Ok(Response {
                status,
                body,
                rate_limit,
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
