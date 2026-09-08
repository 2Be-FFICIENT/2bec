//! End-to-end tests of `2bec variables` against a mock API. No network.

mod common;

use assert_cmd::Command;
use common::IsolatedEnv;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{body_json, header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const VAR: &str = "11111111-1111-4111-8111-111111111111";
const SECRET: &str = "22222222-2222-4222-8222-222222222222";

fn variable(id: &str, name: &str, secret: bool) -> serde_json::Value {
    serde_json::json!({
        "id": id, "name": name, "secret": secret,
        "value": if secret { serde_json::Value::Null } else { "smtp.example.com".into() },
        "comment": "Outgoing mail", "createdAt": "2026-01-01T00:00:00+00:00",
        "updatedAt": "2026-10-01T10:00:00+02:00", "updatedBy": "alice", "versions": 2,
        "envName": format!("P3_{name}")
    })
}

fn detail() -> serde_json::Value {
    let mut v = variable(VAR, "SMTP_HOST", false);
    v["history"] = serde_json::json!([
        {"at": "2026-10-01T10:00:00+02:00", "by": "alice", "value": "smtp.example.com"},
        {"at": "2026-01-01T00:00:00+00:00", "by": null, "value": "mail.example.com"}
    ]);
    v["readBy"] = serde_json::json!([{"kind": "hook", "login": "ops", "hook": "pagerduty"}]);
    v
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/variables", "traceId": "trace123"
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
async fn list_sends_filters_and_hides_secret_values() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/variables"))
        .and(query_param("secret", "true"))
        .and(query_param("q", "token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [variable(SECRET, "API_TOKEN", true)], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/variables"))
        .and(query_param_is_missing("secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [variable(VAR, "SMTP_HOST", false), variable(SECRET, "API_TOKEN", true)],
            "next": null, "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["variables", "list", "--secret", "-q", "token"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tNAME\tKIND\tVALUE\tVERSIONS\tUPDATED\tCOMMENT\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{SECRET}\tAPI_TOKEN\tsecret\t(secret)\t2\t2026-10-01 10:00\tOutgoing mail\n"
        )));
    cmd(&server, &dir)
        .args(["variables", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{VAR}\tSMTP_HOST\tplain\tsmtp.example.com\t"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_prints_history_readers_and_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/variables/{VAR}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v2\"")
                .set_body_json(detail()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["variables", "get", VAR])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Name:      SMTP_HOST (env P3_SMTP_HOST)\n",
        ))
        .stdout(predicate::str::contains("Value:     smtp.example.com\n"))
        .stdout(predicate::str::contains(
            "Updated:   2026-10-01 10:00 by alice\n",
        ))
        .stdout(predicate::str::contains("ETag:      \"v2\"\n"))
        .stdout(predicate::str::contains(
            "Read by:   hook pagerduty (ops)\n",
        ))
        .stdout(predicate::str::contains(
            "2026-01-01 00:00\t-\tmail.example.com\n",
        ));
    cmd(&server, &dir)
        .args(["variables", "get", "SMTP_HOST"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("variable id must be a GUID"));
}

#[tokio::test(flavor = "multi_thread")]
async fn set_creates_with_the_value_from_stdin() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v2/variables/API_TOKEN"))
        .and(body_json(
            serde_json::json!({"secret": true, "value": "s3cr3t"}),
        ))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"v1\"")
                .set_body_json(variable(SECRET, "API_TOKEN", true)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["variables", "set", "API_TOKEN", "-", "--secret"])
        .write_stdin("s3cr3t\n")
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Created variable API_TOKEN ({SECRET})"
        )))
        .stdout(predicate::str::contains("Kind:      secret\n"))
        .stdout(predicate::str::contains("Value:     (secret)\n"))
        .stdout(predicate::str::contains("ETag:      \"v1\"\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn set_changes_the_comment_only_with_if_match() {
    let server = MockServer::start().await;
    let mut updated = variable(VAR, "SMTP_HOST", false);
    updated["comment"] = serde_json::Value::Null;
    Mock::given(method("PUT"))
        .and(path("/v2/variables/SMTP_HOST"))
        .and(header("if-match", "\"v2\""))
        .and(body_json(serde_json::json!({"comment": null})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v3\"")
                .set_body_json(updated),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "variables",
            "set",
            "SMTP_HOST",
            "--comment",
            "",
            "--if-match",
            "\"v2\"",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Updated variable SMTP_HOST"))
        .stdout(predicate::str::contains("Comment:   -\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn set_rejects_bad_input_locally_and_reports_api_refusals() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v2/variables/API_TOKEN"))
        .respond_with(problem("variable-kind-fixed", 409))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["variables", "set", "smtp-host", "x"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("letters, digits and underscores"));
    cmd(&server, &dir)
        .args(["variables", "set", "SMTP_HOST"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing to write"));
    cmd(&server, &dir)
        .args(["variables", "set", "API_TOKEN", "x", "--secret"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("variable-kind-fixed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_sends_force_and_if_match() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v2/variables/OLD_TOKEN"))
        .and(query_param("force", "true"))
        .and(header("if-match", "\"v2\""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v2/variables/SMTP_HOST"))
        .and(query_param_is_missing("force"))
        .respond_with(problem("variable-in-use", 409))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "variables",
            "rm",
            "OLD_TOKEN",
            "--force",
            "--if-match",
            "\"v2\"",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Deleted variable OLD_TOKEN"));
    cmd(&server, &dir)
        .args(["variables", "delete", "SMTP_HOST"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("variable-in-use"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_names_the_scope() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v2/variables/SMTP_HOST"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["variables", "set", "SMTP_HOST", "x"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `scenarios:write`",
        ));
}
