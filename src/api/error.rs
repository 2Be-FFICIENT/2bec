//! Typed API failures and their exit codes.
//!
//! The API answers errors as RFC 9457 `application/problem+json`, with
//! `type` set to `urn:pegas:problem:<code>`. Commands branch on the code,
//! never on the human title, and every failure maps to a distinct process
//! exit code so scripts can tell auth problems from network ones.

#![expect(
    dead_code,
    reason = "constructed by the HTTP client once it classifies responses"
)]

use serde::Deserialize;
use std::fmt;
use std::process::ExitCode;

/// Prefix of every problem `type` the API emits.
const PROBLEM_URN: &str = "urn:pegas:problem:";

/// Exit codes, in the spirit of `gh` and sysexits: scripts can branch on them.
pub mod exit {
    /// Unclassified failure.
    pub const GENERIC: u8 = 1;
    /// Missing, invalid, expired or revoked credential, or missing scope.
    pub const AUTH: u8 = 3;
    /// Could not reach the API or read its answer.
    pub const NETWORK: u8 = 4;
    /// The API refused the request for another reason (4xx/5xx).
    pub const API: u8 = 5;
    /// Rate limited and retries exhausted.
    pub const RATE_LIMITED: u8 = 6;
}

/// Body of an `application/problem+json` answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    #[serde(rename = "type", default)]
    pub type_: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: u16,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub instance: Option<String>,
    #[serde(default)]
    pub trace_id: Option<String>,
    /// The scope the operation needed, filled in by the client on `scope-missing`.
    #[serde(skip)]
    pub required_scope: Option<&'static str>,
}

impl Problem {
    /// Parse a problem body; `None` when it is not problem+json.
    pub fn parse(body: &str) -> Option<Self> {
        let p: Self = serde_json::from_str(body).ok()?;
        (!p.type_.is_empty() || !p.title.is_empty()).then_some(p)
    }

    /// The short code after `urn:pegas:problem:`, or the full `type`.
    pub fn code(&self) -> &str {
        self.type_.strip_prefix(PROBLEM_URN).unwrap_or(&self.type_)
    }
}

/// Everything that can go wrong talking to the API.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// 401 or 403 with an auth-related problem code.
    #[error("{}", auth_message(.0))]
    Auth(Box<Problem>),
    /// 429 `rate-limited`; `retry_after` is in seconds when the API sent it.
    #[error("rate limited by the API{}", retry_hint(*.retry_after))]
    RateLimited {
        problem: Box<Problem>,
        retry_after: Option<u64>,
    },
    /// Any other problem+json answer.
    #[error("{}", problem_message(.0))]
    Problem(Box<Problem>),
    /// A failure status without a problem body.
    #[error("API answered HTTP {status}")]
    Status { status: u16, body: String },
    /// Transport failure, timeout or unreadable body.
    #[error("cannot reach the API: {0}")]
    Network(#[from] reqwest::Error),
    /// A 2xx body that does not match the expected shape.
    #[error("API answered with unexpected JSON: {0}")]
    Decode(#[from] serde_json::Error),
    /// Writing a downloaded body to disk failed.
    #[error("cannot write the downloaded file: {0}")]
    Io(std::io::Error),
}

impl ApiError {
    /// Name the scope a `scope-missing` answer was about, from the operation.
    pub fn with_scope_hint(mut self, method: &str, path: &str) -> Self {
        if let Self::Auth(p) = &mut self
            && p.code() == "scope-missing"
        {
            p.required_scope = crate::api::scopes::required(method, path);
        }
        self
    }

    /// Classify a non-success response.
    pub fn from_response(status: u16, retry_after: Option<u64>, body: &str) -> Self {
        let Some(problem) = Problem::parse(body) else {
            return Self::Status {
                status,
                body: body.to_string(),
            };
        };
        let problem = Box::new(problem);
        match problem.code() {
            "rate-limited" => Self::RateLimited {
                problem,
                retry_after,
            },
            code if is_auth_code(code) => Self::Auth(problem),
            _ => Self::Problem(problem),
        }
    }

    /// The process exit code this failure maps to.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Auth(_) => exit::AUTH,
            Self::RateLimited { .. } => exit::RATE_LIMITED,
            Self::Problem(_) | Self::Status { .. } | Self::Decode(_) => exit::API,
            Self::Io(_) => exit::GENERIC,
            Self::Network(_) => exit::NETWORK,
        }
    }

    /// The problem's `traceId`, for support requests.
    pub fn trace_id(&self) -> Option<&str> {
        match self {
            Self::Auth(p) | Self::Problem(p) | Self::RateLimited { problem: p, .. } => {
                p.trace_id.as_deref()
            }
            _ => None,
        }
    }

    /// Whether a retry may succeed without user action.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::RateLimited { .. } => true,
            Self::Problem(p) => p.code() == "api-keys-unavailable" || p.status >= 500,
            Self::Status { status, .. } => *status >= 500,
            Self::Network(e) => e.is_timeout() || e.is_connect(),
            Self::Auth(_) | Self::Decode(_) | Self::Io(_) => false,
        }
    }
}

fn is_auth_code(code: &str) -> bool {
    matches!(
        code,
        "api-key-required"
            | "api-key-unknown"
            | "api-key-expired"
            | "api-key-revoked"
            | "token-invalid"
            | "token-expired"
            | "scope-missing"
            | "staff-not-allowed"
    )
}

/// One actionable line per auth code.
fn auth_message(p: &Problem) -> String {
    match p.code() {
        "api-key-required" => "no credential: run `2bec auth login`".into(),
        "api-key-unknown" => "API key is not recognized: check it or run `2bec auth login`".into(),
        "api-key-expired" => {
            "API key has expired: create a new one and run `2bec auth login`".into()
        }
        "api-key-revoked" => {
            "API key was revoked: create a new one and run `2bec auth login`".into()
        }
        "token-invalid" => "access token is invalid for this API: run `2bec auth login`".into(),
        "token-expired" => "access token has expired: run `2bec auth login`".into(),
        "scope-missing" => match p.required_scope {
            Some(scope) => {
                format!("credential lacks the scope `{scope}`: ask an API Admin for a key with it")
            }
            None => "credential lacks the scope this command needs".into(),
        },
        "staff-not-allowed" => "staff accounts cannot use the API: use an API key".into(),
        other => format!("authentication failed ({other})"),
    }
}

fn problem_message(p: &Problem) -> String {
    match &p.detail {
        Some(d) if !d.is_empty() => format!("{} ({})", d, p.code()),
        _ => format!("{} ({})", p.title, p.code()),
    }
}

fn retry_hint(retry_after: Option<u64>) -> String {
    match retry_after {
        Some(s) => format!(", retry in {s}s"),
        None => String::new(),
    }
}

/// Map any error reaching `main` to an exit code: typed API errors keep
/// theirs, everything else is generic.
pub fn exit_code_for(err: &anyhow::Error) -> ExitCode {
    let code = err
        .downcast_ref::<ApiError>()
        .map_or(exit::GENERIC, ApiError::exit_code);
    ExitCode::from(code)
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.status, self.code())?;
        if let Some(d) = &self.detail {
            write!(f, ": {d}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(code: &str, status: u16) -> String {
        format!(
            r#"{{"type":"urn:pegas:problem:{code}","title":"T","status":{status},"detail":"D","instance":"/v2/me","traceId":"abc"}}"#
        )
    }

    #[test]
    fn auth_codes_map_to_auth_exit() {
        for code in ["api-key-unknown", "token-expired", "scope-missing"] {
            let e = ApiError::from_response(401, None, &body(code, 401));
            assert!(matches!(e, ApiError::Auth(_)), "{code}");
            assert_eq!(e.exit_code(), exit::AUTH);
            assert_eq!(e.trace_id(), Some("abc"));
            assert!(!e.is_transient());
        }
    }

    #[test]
    fn auth_messages_are_actionable() {
        let e = ApiError::from_response(401, None, &body("api-key-revoked", 401));
        assert_eq!(
            e.to_string(),
            "API key was revoked: create a new one and run `2bec auth login`"
        );
    }

    #[test]
    fn rate_limited_keeps_retry_after() {
        let e = ApiError::from_response(429, Some(7), &body("rate-limited", 429));
        assert_eq!(e.exit_code(), exit::RATE_LIMITED);
        assert!(e.is_transient());
        assert_eq!(e.to_string(), "rate limited by the API, retry in 7s");
    }

    #[test]
    fn other_problems_use_detail_and_code() {
        let e = ApiError::from_response(400, None, &body("invalid-query", 400));
        assert_eq!(e.exit_code(), exit::API);
        assert_eq!(e.to_string(), "D (invalid-query)");
        assert!(!e.is_transient());
    }

    #[test]
    fn key_store_outage_is_transient() {
        let e = ApiError::from_response(503, None, &body("api-keys-unavailable", 503));
        assert!(e.is_transient());
        assert_eq!(e.exit_code(), exit::API);
    }

    #[test]
    fn non_problem_bodies_fall_back_to_status() {
        let e = ApiError::from_response(502, None, "<html>bad gateway</html>");
        assert!(matches!(e, ApiError::Status { status: 502, .. }));
        assert!(e.is_transient());
        assert_eq!(e.to_string(), "API answered HTTP 502");
        assert_eq!(e.trace_id(), None);
    }

    #[test]
    fn problem_code_strips_urn() {
        let p = Problem::parse(&body("not-found", 404)).unwrap();
        assert_eq!(p.code(), "not-found");
        assert_eq!(p.to_string(), "404 not-found: D");
        assert!(Problem::parse("{}").is_none());
        assert!(Problem::parse("[]").is_none());
    }

    #[test]
    fn anyhow_wrapping_keeps_exit_code() {
        let e: anyhow::Error =
            ApiError::from_response(401, None, &body("token-invalid", 401)).into();
        assert_eq!(exit_code_for(&e), ExitCode::from(exit::AUTH));
        assert_eq!(
            exit_code_for(&anyhow::anyhow!("x")),
            ExitCode::from(exit::GENERIC)
        );
    }
}
