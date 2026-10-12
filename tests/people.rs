//! End-to-end tests of `2bec users`, `2bec profiles` and `2bec security`
//! against a mock API. No network.

mod common;

use assert_cmd::Command;
use common::IsolatedEnv;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const USER: &str = "11111111-1111-4111-8111-111111111111";
const PROFILE: &str = "22222222-2222-4222-8222-222222222222";
const GROUP: &str = "33333333-3333-4333-8333-333333333333";

fn user() -> serde_json::Value {
    serde_json::json!({
        "id": USER, "login": "alice", "displayName": "Alice Smith", "firstName": "Alice",
        "lastName": "Smith", "email": "alice@example.com", "jobTitle": "Ops", "phone": null,
        "mobile": "+33600000000", "roles": ["User Admin", "API Admin"], "accountAdmin": false,
        "status": "locked", "profile": {"id": PROFILE, "name": "Operations"}, "externalAuth": true,
        "lastSignInAt": "2026-10-01T10:00:00+00:00", "lastActivityAt": "2026-10-02T08:30:00+00:00"
    })
}

fn user_detail() -> serde_json::Value {
    let mut u = user();
    u["externalAuthOnly"] = true.into();
    u["twoFactorExempt"] = false.into();
    u["language"] = "fr".into();
    u["createdAt"] = "2025-01-15T09:00:00+00:00".into();
    u["lockedAt"] = "2026-10-03T07:00:00+00:00".into();
    u
}

fn profile_row() -> serde_json::Value {
    serde_json::json!({
        "id": PROFILE, "name": "Operations", "ssoDefault": true, "scenarioCount": 12,
        "memberCount": 3,
        "groups": [{"group": {"id": GROUP, "name": "Web"}, "access": true},
                   {"group": {"id": GROUP, "name": "Voice"}, "access": false}]
    })
}

fn profile_detail() -> serde_json::Value {
    serde_json::json!({
        "id": PROFILE, "name": "Operations", "ssoDefault": true, "scenarioCount": 12,
        "groups": [{"group": {"id": GROUP, "name": "Web"}, "access": true}],
        "members": [{"id": USER, "displayName": "Alice Smith", "login": "alice", "status": "locked"}]
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/users", "traceId": "trace123"
        }))
}

fn cmd(server: &MockServer, dir: &TempDir) -> Command {
    let mut c = Command::cargo_bin("2bec").unwrap();
    c.isolate_env()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .env("TWOBEC_TOKEN", KEY);
    c
}

#[tokio::test(flavor = "multi_thread")]
async fn users_list_sends_filters_and_follows_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/users"))
        .and(query_param("status", "locked"))
        .and(query_param("role", "User Admin"))
        .and(query_param("profile", PROFILE))
        .and(query_param("q", "smith"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [user()], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/users"))
        .and(query_param_is_missing("cursor"))
        .and(query_param_is_missing("status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [user()], "next": "c2", "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/users"))
        .and(query_param("cursor", "c2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [user()], "next": null, "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "users",
            "list",
            "--status",
            "locked",
            "--role",
            "User Admin",
            "--profile",
            PROFILE,
            "-q",
            "smith",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tLOGIN\tNAME\tEMAIL\tSTATUS\tROLES\tPROFILE\tLAST ACTIVITY\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{USER}\talice\tAlice Smith\talice@example.com\tlocked\tUser Admin, API Admin\tOperations\t2026-10-02 08:30\n"
        )));
    cmd(&server, &dir)
        .args(["users", "list", "--all", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"login\":\"alice\"").count(2));
    cmd(&server, &dir)
        .args(["users", "list", "--status", "gone"])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn users_get_prints_the_sign_in_details() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/users/{USER}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(user_detail()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/users/{PROFILE}")))
        .respond_with(problem("not-found", 404))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["users", "get", USER])
        .assert()
        .success()
        .stdout(predicate::str::contains("Login:          alice\n"))
        .stdout(predicate::str::contains(
            "Status:         locked since 2026-10-03 07:00\n",
        ))
        .stdout(predicate::str::contains(
            "Roles:          User Admin, API Admin\n",
        ))
        .stdout(predicate::str::contains(
            "Sign-in:        identity provider only\n",
        ))
        .stdout(predicate::str::contains(
            "Strong auth:    as the policy says\n",
        ))
        .stdout(predicate::str::contains("Language:       fr\n"));
    cmd(&server, &dir)
        .args(["users", "get", PROFILE])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("not-found"));
    cmd(&server, &dir)
        .args(["users", "get", "alice"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("user id must be a GUID"));
}

#[tokio::test(flavor = "multi_thread")]
async fn profiles_list_and_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/profiles"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [profile_row()], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/profiles/{PROFILE}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(profile_detail()))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["profiles", "list"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tNAME\tSSO DEFAULT\tGROUPS\tSCENARIOS\tUSERS\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{PROFILE}\tOperations\tyes\tWeb, [Voice]\t12\t3\n"
        )));
    cmd(&server, &dir)
        .args(["profiles", "get", PROFILE])
        .assert()
        .success()
        .stdout(predicate::str::contains("Name:         Operations\n"))
        .stdout(predicate::str::contains("Users:        1\n"))
        .stdout(predicate::str::contains(format!("{GROUP}\tWeb\topen\n")))
        .stdout(predicate::str::contains(format!(
            "{USER}\talice\tAlice Smith\tlocked\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn security_settings_prints_the_policy_and_names_the_scope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/security-settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "password": {"minLength": 12, "requireUpperAndLower": true, "requireDigit": true,
                         "specialCharCount": 1, "forbidReuse": true, "renewRegularly": true, "renewDays": 90},
            "twoFactor": {"enabled": true, "sms": true, "email": false, "users": false, "admins": true,
                          "trustedDeviceDays": 30},
            "userCount": 40, "twoFactorExemptCount": 2, "withoutMobileCount": 5, "withoutEmailCount": 0
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["security", "settings"])
        .assert()
        .success()
        .stdout(predicate::str::contains("  Minimum length:        12\n"))
        .stdout(predicate::str::contains(
            "  Renewal:               every 90 days\n",
        ))
        .stdout(predicate::str::contains("  Channels:              SMS\n"))
        .stdout(predicate::str::contains(
            "  Asked of:              administrators\n",
        ))
        .stdout(predicate::str::contains("  Without mobile:        5\n"));

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/security-settings"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    cmd(&server, &dir)
        .args(["security", "settings", "--json"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `users:read`",
        ));
}
