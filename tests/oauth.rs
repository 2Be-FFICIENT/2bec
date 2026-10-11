//! End-to-end tests of the browser login against a mock identity server
//! and a mock API. The "browser" is a plain HTTP request to the loopback
//! callback. No network.

use assert_cmd::cargo::cargo_bin;
use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use tempfile::TempDir;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn me_body(caller_name: &str) -> serde_json::Value {
    serde_json::json!({
        "caller": "person",
        "tenant": {"id": "t1", "name": "Acme"},
        "key": null,
        "person": {"id": "u1", "name": caller_name, "email": "ada@example.com",
                   "roles": ["Admin"], "scopes": ["runs:read"], "client": "2bec"},
        "rateLimit": {"limit": 600, "remaining": 599, "resetAt": "2026-01-01T00:01:00+00:00"}
    })
}

/// Mount discovery documents pointing the identity server at `server` itself.
async fn mount_discovery(server: &MockServer) {
    let base = server.uri();
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-protected-resource"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "resource": base, "authorization_servers": [format!("{base}/id/")],
            "scopes_supported": ["pegas-api:read", "pegas-api:write"]
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/id/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": format!("{base}/id"),
            "authorization_endpoint": format!("{base}/id/connect/authorize"),
            "token_endpoint": format!("{base}/id/connect/token"),
            "revocation_endpoint": format!("{base}/id/connect/revocation"),
            "device_authorization_endpoint": format!("{base}/id/connect/deviceauthorization")
        })))
        .mount(server)
        .await;
}

fn hosts_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("hosts.toml")
}

fn read_hosts(dir: &TempDir) -> toml::Value {
    toml::from_str(&std::fs::read_to_string(hosts_path(dir)).unwrap()).unwrap()
}

/// Pull one query parameter out of a URL.
fn query_param(url: &str, name: &str) -> String {
    let q = url.split_once('?').map_or("", |(_, q)| q);
    q.split('&')
        .find_map(|p| p.strip_prefix(&format!("{name}=")))
        .unwrap_or_else(|| panic!("no {name} in {url}"))
        .replace("%3A", ":")
        .replace("%2F", "/")
}

#[tokio::test(flavor = "multi_thread")]
async fn browser_login_exchanges_the_code_and_stores_the_tokens() {
    let server = MockServer::start().await;
    mount_discovery(&server).await;
    Mock::given(method("POST"))
        .and(path("/id/connect/token"))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=thecode"))
        .and(body_string_contains("client_id=2bec"))
        .and(body_string_contains("code_verifier="))
        .and(body_string_contains(
            "resource=https%3A%2F%2Fapi.2befficient.com",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ACCESS1", "refresh_token": "REFRESH1",
            "expires_in": 3600, "token_type": "Bearer"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/me"))
        .and(header("authorization", "Bearer ACCESS1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me_body("Ada")))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let mut child = Command::new(cargo_bin!("2bec"))
        .env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .args(["auth", "login", "--no-browser"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Read stderr until the authorize URL and the "Waiting" line appear.
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut url = String::new();
    let mut seen_waiting = false;
    let mut line = String::new();
    while !seen_waiting && stderr.read_line(&mut line).unwrap() > 0 {
        let t = line.trim();
        if t.starts_with("http") {
            url = t.to_string();
        }
        if t.starts_with("Waiting") {
            seen_waiting = true;
        }
        line.clear();
    }
    assert!(
        url.starts_with(&format!("{}/id/connect/authorize?", server.uri())),
        "{url}"
    );
    assert_eq!(query_param(&url, "code_challenge_method"), "S256");
    assert_eq!(query_param(&url, "client_id"), "2bec");
    let redirect = query_param(&url, "redirect_uri");
    let state = query_param(&url, "state");
    assert!(redirect.starts_with("http://127.0.0.1:"), "{redirect}");

    // Play the browser: hit the loopback callback with the code and state.
    let authority = redirect
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap();
    let mut sock = TcpStream::connect(authority).unwrap();
    write!(
        sock,
        "GET /callback?code=thecode&state={state} HTTP/1.1\r\nHost: {authority}\r\n\r\n"
    )
    .unwrap();
    let mut answer = String::new();
    sock.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.contains("Logged in"));

    let status = child.wait().unwrap();
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    assert!(status.success(), "{out}");
    assert!(out.contains("Logged in to Acme as Ada"), "{out}");

    let hosts = read_hosts(&dir);
    let entry = &hosts["profiles"]["default"];
    assert_eq!(entry["access_token"].as_str(), Some("ACCESS1"));
    assert_eq!(entry["refresh_token"].as_str(), Some("REFRESH1"));
    assert_eq!(
        entry["issuer"].as_str(),
        Some(format!("{}/id", server.uri()).as_str())
    );
    assert!(entry.get("token").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_login_is_renewed_before_the_command_runs() {
    let server = MockServer::start().await;
    mount_discovery(&server).await;
    Mock::given(method("POST"))
        .and(path("/id/connect/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=OLDREFRESH"))
        .and(body_string_contains("client_id=2bec"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ACCESS2", "refresh_token": "REFRESH2", "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    std::fs::write(
        hosts_path(&dir),
        format!(
            "[profiles.default]\naccess_token = \"OLD\"\nrefresh_token = \"OLDREFRESH\"\n\
             expires_at = 1\nissuer = \"{}/id\"\n",
            server.uri()
        ),
    )
    .unwrap();

    assert_cmd::Command::cargo_bin("2bec")
        .unwrap()
        .env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout("ACCESS2\n");

    let hosts = read_hosts(&dir);
    assert_eq!(
        hosts["profiles"]["default"]["refresh_token"].as_str(),
        Some("REFRESH2")
    );
    assert!(
        hosts["profiles"]["default"]["expires_at"]
            .as_integer()
            .unwrap()
            > 1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_renewal_warns_and_keeps_going() {
    let server = MockServer::start().await;
    mount_discovery(&server).await;
    Mock::given(method("POST"))
        .and(path("/id/connect/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_grant", "error_description": "refresh token revoked"
        })))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    std::fs::write(
        hosts_path(&dir),
        format!(
            "[profiles.default]\naccess_token = \"OLD\"\nrefresh_token = \"R\"\n\
             expires_at = 1\nissuer = \"{}/id\"\n",
            server.uri()
        ),
    )
    .unwrap();

    assert_cmd::Command::cargo_bin("2bec")
        .unwrap()
        .env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout("OLD\n")
        .stderr(predicates::str::contains("invalid_grant"))
        .stderr(predicates::str::contains("run `2bec auth login`"));
}

#[tokio::test(flavor = "multi_thread")]
async fn logout_revokes_the_refresh_token() {
    let server = MockServer::start().await;
    mount_discovery(&server).await;
    Mock::given(method("POST"))
        .and(path("/id/connect/revocation"))
        .and(body_string_contains("token=REF"))
        .and(body_string_contains("token_type_hint=refresh_token"))
        .and(body_string_contains("client_id=2bec"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    std::fs::write(
        hosts_path(&dir),
        format!(
            "[profiles.default]\naccess_token = \"A\"\nrefresh_token = \"REF\"\n\
             expires_at = 9999999999\nissuer = \"{}/id\"\n",
            server.uri()
        ),
    )
    .unwrap();

    assert_cmd::Command::cargo_bin("2bec")
        .unwrap()
        .env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .args(["auth", "logout"])
        .assert()
        .success()
        .stderr(predicates::str::contains("Revoked the login"))
        .stdout(predicates::str::contains("Removed the credential"));
    assert!(
        !hosts_path(&dir).exists()
            || !std::fs::read_to_string(hosts_path(&dir))
                .unwrap()
                .contains("REF")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn status_shows_the_person_login() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/me"))
        .and(header("authorization", "Bearer A"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me_body("Ada")))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    std::fs::write(
        hosts_path(&dir),
        format!(
            "[profiles.default]\naccess_token = \"A\"\nrefresh_token = \"REF\"\n\
             expires_at = 9999999999\nissuer = \"{}/id\"\n",
            server.uri()
        ),
    )
    .unwrap();

    assert_cmd::Command::cargo_bin("2bec")
        .unwrap()
        .env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Token kind:   person login (OAuth)",
        ))
        .stdout(predicates::str::contains("renewed automatically"))
        .stdout(predicates::str::contains(
            "Caller:       Ada <ada@example.com>",
        ))
        .stdout(predicates::str::contains("Client:       2bec"));
}

fn device_authorization(interval: u64) -> serde_json::Value {
    serde_json::json!({
        "device_code": "DEV1", "user_code": "123-456",
        "verification_uri": "https://id.example/device",
        "verification_uri_complete": "https://id.example/device?userCode=123456",
        "expires_in": 300, "interval": interval
    })
}

fn oauth_error(code: &str) -> ResponseTemplate {
    ResponseTemplate::new(400).set_body_json(serde_json::json!({"error": code}))
}

#[tokio::test(flavor = "multi_thread")]
async fn device_login_polls_until_approved_and_stores_the_tokens() {
    let server = MockServer::start().await;
    mount_discovery(&server).await;
    Mock::given(method("POST"))
        .and(path("/id/connect/deviceauthorization"))
        .and(body_string_contains("client_id=2bec"))
        .and(body_string_contains(
            "scope=pegas-api%3Aread+pegas-api%3Awrite+offline_access",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(device_authorization(1)))
        .expect(1)
        .mount(&server)
        .await;
    // First poll: pending. Second: slow down. Third: the tokens.
    let poll = || {
        Mock::given(method("POST"))
            .and(path("/id/connect/token"))
            .and(body_string_contains(
                "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code",
            ))
            .and(body_string_contains("device_code=DEV1"))
            .and(body_string_contains("client_id=2bec"))
            .and(body_string_contains(
                "resource=https%3A%2F%2Fapi.2befficient.com",
            ))
    };
    poll()
        .respond_with(oauth_error("authorization_pending"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    poll()
        .respond_with(oauth_error("slow_down"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    poll()
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ACCESS9", "refresh_token": "REFRESH9",
            "expires_in": 3600, "token_type": "Bearer"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/me"))
        .and(header("authorization", "Bearer ACCESS9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me_body("Ada")))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let started = std::time::Instant::now();
    Command::new(cargo_bin!("2bec"))
        .env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .args(["auth", "login", "--device"])
        .stdin(Stdio::null())
        .assert()
        .success()
        .stderr(predicate::str::contains("https://id.example/device\n"))
        .stderr(predicate::str::contains("Code: 123-456\n"))
        .stderr(predicate::str::contains(
            "(or open https://id.example/device?userCode=123456)",
        ))
        .stdout(predicate::str::contains("Logged in to Acme as Ada"));
    // 1 s before each poll, plus the 5 s slow_down asks for.
    assert!(started.elapsed() >= std::time::Duration::from_secs(8));

    let entry = &read_hosts(&dir)["profiles"]["default"];
    assert_eq!(entry["access_token"].as_str(), Some("ACCESS9"));
    assert_eq!(entry["refresh_token"].as_str(), Some("REFRESH9"));
}

#[tokio::test(flavor = "multi_thread")]
async fn device_login_declined_in_the_browser_fails() {
    let server = MockServer::start().await;
    mount_discovery(&server).await;
    Mock::given(method("POST"))
        .and(path("/id/connect/deviceauthorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(device_authorization(1)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/id/connect/token"))
        .respond_with(oauth_error("access_denied"))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    Command::new(cargo_bin!("2bec"))
        .env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .args(["auth", "login", "--device"])
        .stdin(Stdio::null())
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "the login was declined in the browser",
        ));
    assert!(!hosts_path(&dir).exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn device_login_explains_a_client_without_the_grant() {
    let server = MockServer::start().await;
    mount_discovery(&server).await;
    Mock::given(method("POST"))
        .and(path("/id/connect/deviceauthorization"))
        .respond_with(oauth_error("unauthorized_client"))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    Command::new(cargo_bin!("2bec"))
        .env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .args(["auth", "login", "--device"])
        .stdin(Stdio::null())
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "unauthorized_client (the 2bec client is not allowed the device grant)",
        ));
}
