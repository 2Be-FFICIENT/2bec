//! End-to-end tests of `2bec auth` against a mock API. No network.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";

fn me_body() -> serde_json::Value {
    serde_json::json!({
        "caller": "key",
        "tenant": {"id": "t1", "name": "Acme"},
        "key": {
            "id": "k1", "name": "ci", "description": null, "prefix": "2bef_live_ab",
            "scopes": ["runs:read", "runs:write"], "profile": {"id": "p1", "name": "All"},
            "createdAt": "2026-01-01T00:00:00+00:00", "expiresAt": null, "lastUsedAt": null
        },
        "person": null,
        "rateLimit": {"limit": 600, "remaining": 599, "resetAt": "2026-01-01T00:01:00+00:00"}
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"),
            "title": "T", "status": status, "detail": "D",
            "instance": "/v2/me", "traceId": "trace123"
        }))
}

fn me_ok() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("RateLimit-Limit", "600")
        .insert_header("RateLimit-Remaining", "599")
        .insert_header("RateLimit-Reset", "42")
        .set_body_json(me_body())
}

/// A command pointed at `server` with an isolated config dir.
fn cmd(server: &MockServer, dir: &TempDir) -> Command {
    let mut c = Command::cargo_bin("2bec").unwrap();
    c.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri());
    c
}

fn stored_token(dir: &TempDir) -> Option<String> {
    let text = std::fs::read_to_string(dir.path().join("hosts.toml")).ok()?;
    let v: toml::Value = toml::from_str(&text).ok()?;
    v.get("profiles")?
        .get("default")?
        .get("token")?
        .as_str()
        .map(str::to_string)
}

#[tokio::test(flavor = "multi_thread")]
async fn login_verifies_and_stores_the_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/me"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .and(header(
            "user-agent",
            concat!("2bec/", env!("CARGO_PKG_VERSION")),
        ))
        .respond_with(me_ok())
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["auth", "login"])
        .write_stdin(format!("{KEY}\n"))
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Logged in to Acme as API key `ci`",
        ));

    assert_eq!(stored_token(&dir).as_deref(), Some(KEY));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("hosts.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn login_rejects_a_malformed_key_without_calling_the_api() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["auth", "login", "--with-token"])
        .write_stdin("2bef_live_short\n")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not a valid API key"));

    assert_eq!(server.received_requests().await.unwrap().len(), 0);
    assert!(stored_token(&dir).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn login_does_not_store_a_key_the_api_refuses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/me"))
        .respond_with(problem("api-key-revoked", 401))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["auth", "login", "-v"])
        .write_stdin(KEY)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("API key was revoked"))
        .stderr(predicate::str::contains("trace id: trace123"));

    assert!(stored_token(&dir).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn status_without_credential_exits_with_auth_code() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["auth", "status"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("run `2bec auth login`"));
    cmd(&server, &dir).args(["auth", "token"]).assert().code(3);
}

#[tokio::test(flavor = "multi_thread")]
async fn status_reports_the_stored_credential() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/me"))
        .respond_with(me_ok())
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    cmd(&server, &dir)
        .args(["auth", "login"])
        .write_stdin(KEY)
        .assert()
        .success();

    cmd(&server, &dir)
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Token source: credential store"))
        .stdout(predicate::str::contains("Token:        2bef_live_ab…"))
        .stdout(predicate::str::contains(
            "Scopes:       runs:read, runs:write",
        ))
        .stdout(predicate::str::contains("resets in 42s"))
        .stdout(predicate::str::contains(KEY).not());

    let out = cmd(&server, &dir)
        .args(["auth", "status", "--json", "--show-token"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["tokenSource"], "stored");
    assert_eq!(v["profile"], "default");
    assert_eq!(v["token"], KEY);
    assert_eq!(v["tenant"]["name"], "Acme");
}

#[tokio::test(flavor = "multi_thread")]
async fn env_token_overrides_the_store_and_token_prints_it() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("hosts.toml"),
        "[profiles.default]\ntoken = \"stored-token\"\n",
    )
    .unwrap();

    cmd(&server, &dir)
        .env("TWOBEC_TOKEN", "env-token")
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout("env-token\n");
    cmd(&server, &dir)
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout("stored-token\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn profiles_keep_separate_credentials() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("hosts.toml"),
        "[profiles.default]\ntoken = \"a\"\n[profiles.dev]\ntoken = \"b\"\n",
    )
    .unwrap();

    cmd(&server, &dir)
        .args(["--profile", "dev", "auth", "token"])
        .assert()
        .success()
        .stdout("b\n");
    cmd(&server, &dir)
        .args(["--profile", "dev", "auth", "logout"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Removed the credential for profile `dev`",
        ));
    cmd(&server, &dir)
        .args(["--profile", "dev", "auth", "logout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No credential stored"));
    cmd(&server, &dir)
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout("a\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_is_retried_then_reported() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/me"))
        .respond_with(problem("rate-limited", 429).insert_header("Retry-After", "0"))
        .expect(3)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .env("TWOBEC_TOKEN", KEY)
        .args(["auth", "status"])
        .assert()
        .code(6)
        .stderr(predicate::str::contains("rate limited by the API"));
}

#[tokio::test(flavor = "multi_thread")]
async fn transient_outage_is_retried_until_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/me"))
        .respond_with(problem("api-keys-unavailable", 503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/me"))
        .respond_with(me_ok())
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .env("TWOBEC_TOKEN", KEY)
        .args(["auth", "status"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_api_exits_with_network_code() {
    let dir = TempDir::new().unwrap();

    // Port 1 is reserved and never listening: the connection is refused.
    Command::cargo_bin("2bec")
        .unwrap()
        .env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", "http://127.0.0.1:1")
        .env("TWOBEC_TOKEN", KEY)
        .args(["auth", "status"])
        .assert()
        .code(4)
        .stderr(predicate::str::contains("cannot reach the API"));
}

#[tokio::test(flavor = "multi_thread")]
async fn ping_reports_the_service() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "2Be-FFICIENT API", "version": "v2"
        })))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .arg("ping")
        .assert()
        .success()
        .stdout(predicate::str::contains("HTTP 200 (2Be-FFICIENT API v2)"));
}
