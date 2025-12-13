//! OAuth 2.0 login for a person: authorization code with PKCE on a loopback
//! redirect (RFC 8252), refresh and revocation.
//!
//! The authorization server is discovered from the API's protected-resource
//! metadata, then from its own `openid-configuration`. The `resource`
//! parameter (RFC 8707) names the API so the token's audience is right.

use anyhow::{Context as _, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Client id registered for this tool in the identity server.
pub const DEFAULT_CLIENT_ID: &str = "2bec";

/// Scopes a person token needs on the public API.
pub const SCOPES: &str = "pegas-api:read pegas-api:write offline_access";

/// How long to wait for the browser to come back.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// Endpoints of the authorization server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub issuer: String,
    pub authorization: String,
    pub token: String,
    pub revocation: Option<String>,
}

/// What the token endpoint hands back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds when the access token stops working.
    pub expires_at: u64,
}

/// Settings of one login: where to go and what to ask for.
#[derive(Debug, Clone)]
pub struct LoginConfig {
    pub client_id: String,
    /// `resource` sent to the identity server; the API's audience.
    pub resource: String,
    /// Print the URL instead of opening the browser.
    pub no_browser: bool,
}

#[derive(Deserialize)]
struct ProtectedResource {
    #[serde(default)]
    authorization_servers: Vec<String>,
}

#[derive(Deserialize)]
struct OpenIdConfiguration {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    revocation_endpoint: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Deserialize, Default)]
struct OAuthError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Find the authorization server: `issuer` when given, else the API's
/// `/.well-known/oauth-protected-resource`.
pub async fn discover(
    http: &reqwest::Client,
    api_url: &str,
    issuer: Option<&str>,
) -> Result<Endpoints> {
    let issuer = match issuer {
        Some(i) => i.trim_end_matches('/').to_string(),
        None => {
            let url = format!(
                "{}/.well-known/oauth-protected-resource",
                api_url.trim_end_matches('/')
            );
            let meta: ProtectedResource = http
                .get(&url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .with_context(|| format!("cannot read {url}"))?
                .json()
                .await
                .with_context(|| format!("{url} is not protected-resource metadata"))?;
            meta.authorization_servers
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("{url} names no authorization server"))?
                .trim_end_matches('/')
                .to_string()
        }
    };
    let url = format!("{issuer}/.well-known/openid-configuration");
    let cfg: OpenIdConfiguration = http
        .get(&url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("cannot read {url}"))?
        .json()
        .await
        .with_context(|| format!("{url} is not an OpenID configuration"))?;
    Ok(Endpoints {
        issuer: cfg.issuer,
        authorization: cfg.authorization_endpoint,
        token: cfg.token_endpoint,
        revocation: cfg.revocation_endpoint,
    })
}

/// Run the browser login and return the tokens.
pub async fn login(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    cfg: &LoginConfig,
) -> Result<Tokens> {
    let verifier = random_urlsafe(32)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_urlsafe(16)?;

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("cannot open a local port for the browser callback")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let url = authorize_url(endpoints, cfg, &redirect_uri, &challenge, &state);
    if cfg.no_browser {
        eprintln!("Open this URL in a browser to log in:\n\n  {url}\n");
    } else {
        eprintln!("Opening your browser to log in; if nothing happens, open:\n\n  {url}\n");
        if let Err(e) = open::that_detached(&url) {
            eprintln!("warning: cannot open the browser ({e})");
        }
    }
    eprintln!("Waiting for the login to complete...");

    let code = tokio::time::timeout(CALLBACK_TIMEOUT, wait_for_code(&listener, &state))
        .await
        .map_err(|_| {
            anyhow!(
                "no response from the browser after {}s",
                CALLBACK_TIMEOUT.as_secs()
            )
        })??;

    let params = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", cfg.client_id.as_str()),
        ("code_verifier", verifier.as_str()),
        ("resource", cfg.resource.as_str()),
    ];
    token_request(http, &endpoints.token, &params).await
}

/// Trade a refresh token for new tokens. The old refresh token is single
/// use, so the caller must persist the returned one.
pub async fn refresh(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    cfg: &LoginConfig,
    refresh_token: &str,
) -> Result<Tokens> {
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", cfg.client_id.as_str()),
        ("resource", cfg.resource.as_str()),
    ];
    token_request(http, &endpoints.token, &params).await
}

/// Revoke a refresh token so the login cannot be renewed any more.
pub async fn revoke(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    client_id: &str,
    refresh_token: &str,
) -> Result<()> {
    let Some(url) = &endpoints.revocation else {
        bail!("the identity server has no revocation endpoint");
    };
    let params = [
        ("token", refresh_token),
        ("token_type_hint", "refresh_token"),
        ("client_id", client_id),
    ];
    http.post(url)
        .form(&params)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("revocation at {url} failed"))?;
    Ok(())
}

fn authorize_url(
    endpoints: &Endpoints,
    cfg: &LoginConfig,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> String {
    let query = [
        ("response_type", "code"),
        ("client_id", cfg.client_id.as_str()),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPES),
        ("resource", cfg.resource.as_str()),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
    ];
    let qs: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect();
    let sep = if endpoints.authorization.contains('?') {
        '&'
    } else {
        '?'
    };
    format!("{}{sep}{}", endpoints.authorization, qs.join("&"))
}

/// Accept connections until one carries the authorization code for `state`.
async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (mut socket, _) = listener.accept().await.context("callback accept failed")?;
        let mut buf = vec![0u8; 8192];
        let n = socket.read(&mut buf).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        let Some(target) = request
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
        else {
            continue;
        };
        // Browsers also ask for /favicon.ico; only /callback matters.
        if !target.starts_with("/callback") {
            respond(&mut socket, 404, "Not found").await;
            continue;
        }
        let params = parse_query(target.split_once('?').map_or("", |(_, q)| q));
        let outcome = callback_outcome(&params, state);
        match &outcome {
            Ok(_) => {
                respond(
                    &mut socket,
                    200,
                    "Logged in. You can close this tab and return to the terminal.",
                )
                .await;
            }
            Err(e) => {
                respond(&mut socket, 400, &format!("Login failed: {e}")).await;
            }
        }
        return outcome;
    }
}

/// Extract the code from the callback parameters, checking state and errors.
fn callback_outcome(params: &[(String, String)], state: &str) -> Result<String> {
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    if let Some(err) = get("error") {
        let desc = get("error_description").unwrap_or("");
        bail!("{}", format!("{err} {desc}").trim());
    }
    if get("state") != Some(state) {
        bail!("state mismatch; the callback did not come from this login");
    }
    get("code")
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("the callback carries no code"))
}

async fn respond(socket: &mut tokio::net::TcpStream, status: u16, text: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>2bec</title>\
         <body style=\"font-family:sans-serif;margin:3em\"><h1>2bec</h1><p>{}</p></body>",
        html_escape(text)
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

async fn token_request(
    http: &reqwest::Client,
    url: &str,
    params: &[(&str, &str)],
) -> Result<Tokens> {
    let resp = http
        .post(url)
        .form(params)
        .send()
        .await
        .with_context(|| format!("token request to {url} failed"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .context("cannot read the token response")?;
    if !status.is_success() {
        let err: OAuthError = serde_json::from_str(&body).unwrap_or_default();
        let what = if err.error.is_empty() {
            format!("HTTP {status}")
        } else {
            err.error
        };
        match err.error_description {
            Some(d) if !d.is_empty() => {
                bail!("identity server refused the token request: {what} ({d})")
            }
            _ => bail!("identity server refused the token request: {what}"),
        }
    }
    let t: TokenResponse =
        serde_json::from_str(&body).context("token response is not the expected JSON")?;
    Ok(Tokens {
        access_token: t.access_token,
        refresh_token: t.refresh_token,
        expires_at: now_unix() + t.expires_in.unwrap_or(3600),
    })
}

/// Current time as Unix seconds.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn random_urlsafe(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| anyhow!("no secure random source: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Percent-encode for a query string (RFC 3986 unreserved set kept).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (urldecode(k), urldecode(v))
        })
        .collect()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc_7636_example() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn authorize_url_carries_every_parameter() {
        let e = Endpoints {
            issuer: "https://id.example".into(),
            authorization: "https://id.example/connect/authorize".into(),
            token: "https://id.example/connect/token".into(),
            revocation: None,
        };
        let cfg = LoginConfig {
            client_id: "2bec".into(),
            resource: "https://api.example".into(),
            no_browser: true,
        };
        let url = authorize_url(&e, &cfg, "http://127.0.0.1:4242/callback", "chal", "st");
        assert!(
            url.starts_with(
                "https://id.example/connect/authorize?response_type=code&client_id=2bec"
            )
        );
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A4242%2Fcallback"));
        assert!(url.contains("scope=pegas-api%3Aread%20pegas-api%3Awrite%20offline_access"));
        assert!(url.contains("resource=https%3A%2F%2Fapi.example"));
        assert!(url.contains("code_challenge=chal&code_challenge_method=S256&state=st"));
    }

    #[test]
    fn callback_checks_state_and_errors() {
        let ok = parse_query("code=abc&state=st");
        assert_eq!(callback_outcome(&ok, "st").unwrap(), "abc");
        let bad_state = parse_query("code=abc&state=other");
        assert!(callback_outcome(&bad_state, "st").is_err());
        let denied = parse_query("error=access_denied&error_description=User%20said%20no&state=st");
        let e = callback_outcome(&denied, "st").unwrap_err().to_string();
        assert_eq!(e, "access_denied User said no");
        let none = parse_query("state=st");
        assert!(callback_outcome(&none, "st").is_err());
    }

    #[test]
    fn url_coding_round_trips() {
        assert_eq!(urlencode("a b/c:d"), "a%20b%2Fc%3Ad");
        assert_eq!(urldecode("a%20b%2Fc%3Ad+e"), "a b/c:d e");
        assert_eq!(urldecode("%zz"), "%zz");
    }

    #[test]
    fn random_values_are_urlsafe_and_distinct() {
        let a = random_urlsafe(32).unwrap();
        let b = random_urlsafe(32).unwrap();
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
    }
}
