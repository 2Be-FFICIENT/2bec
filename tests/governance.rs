//! End-to-end tests of `2bec audit`, `2bec comments` and `2bec tickets`
//! against a mock API. No network.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const USER: &str = "11111111-1111-4111-8111-111111111111";
const SCENARIO: &str = "22222222-2222-4222-8222-222222222222";
const RUN: &str = "33333333-3333-4333-8333-333333333333";
const COMMENT: &str = "44444444-4444-4444-8444-444444444444";

fn scenario() -> serde_json::Value {
    serde_json::json!({"id": SCENARIO, "name": "Login", "kind": "transaction"})
}

fn entry(id: i64, outcome: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "occurredAt": "2026-10-01T10:00:00+00:00", "area": "tags",
        "action": "tag.delete", "actor": "alice", "actorIsStaff": id == 2, "source": "api",
        "targetType": "tag", "targetId": "7", "targetLabel": "release",
        "parameters": {"force": true}, "comment": "cleanup", "outcome": outcome,
        "error": if outcome == "failed" { "in use".into() } else { serde_json::Value::Null },
        "reversible": false, "revertsEntryId": null, "revertedByEntryId": null, "batchId": null,
        "scenarios": [scenario()]
    })
}

fn comment() -> serde_json::Value {
    serde_json::json!({
        "id": COMMENT, "text": "False\n  positive,  ignore", "author": "bob",
        "publishedAt": "2026-10-02T08:00:00+00:00", "scenario": scenario(),
        "run": {"id": RUN, "startedAt": "2026-10-02T07:55:00+00:00"}, "runStatus": "failed"
    })
}

fn ticket() -> serde_json::Value {
    serde_json::json!({
        "id": 7, "mergedIds": [8], "title": "Login fails since Monday", "subject": null,
        "status": "open", "kind": "Robot", "createdAt": "2026-10-01T10:00:00+00:00",
        "updatedAt": "2026-10-02T10:00:00+00:00", "summary": "The robot cannot log in.",
        "resolution": null, "scenarios": [scenario()], "keywords": ["login"],
        "incidentStartedAt": "2026-09-29T06:00:00+00:00"
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/tickets", "traceId": "trace123"
        }))
}

fn cmd(server: &MockServer, dir: &TempDir) -> Command {
    let mut c = Command::cargo_bin("2bec").unwrap();
    c.env_clear()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .env("TWOBEC_API_URL", server.uri())
        .env("TWOBEC_TOKEN", KEY);
    c
}

#[tokio::test(flavor = "multi_thread")]
async fn audit_list_sends_filters_and_shows_outcomes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/audit-entries"))
        .and(query_param("period", "last7d"))
        .and(query_param("area", "tags"))
        .and(query_param("action", "tag.delete"))
        .and(query_param("actor", USER))
        .and(query_param("outcome", "failed"))
        .and(query_param("source", "api"))
        .and(query_param("q", "release"))
        .and(query_param("limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [entry(1, "failed"), entry(2, "succeeded")], "next": null, "total": 12
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "audit",
            "list",
            "--period",
            "last7d",
            "--area",
            "tags",
            "--action",
            "tag.delete",
            "--actor",
            USER,
            "--outcome",
            "failed",
            "--source",
            "api",
            "-q",
            "release",
            "--limit",
            "10",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tAT\tAREA\tACTION\tACTOR\tSOURCE\tTARGET\tOUTCOME\n",
        ))
        .stdout(predicate::str::contains(
            "1\t2026-10-01 10:00\ttags\ttag.delete\talice\tapi\ttag release\tfailed: in use\n",
        ))
        .stdout(predicate::str::contains(
            "2\t2026-10-01 10:00\ttags\ttag.delete\talice (staff)\t",
        ))
        .stderr(predicate::str::contains("Showing 2 of 12 entries"));
    cmd(&server, &dir)
        .args(["audit", "list", "--area", "beta"])
        .assert()
        .code(2);
    cmd(&server, &dir)
        .args(["audit", "list", "--actor", "alice"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--actor must be a GUID"));
}

#[tokio::test(flavor = "multi_thread")]
async fn audit_get_prints_before_and_after() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/audit-entries/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "entry": entry(42, "succeeded"), "before": {"name": "release"}, "after": null
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/audit-entries/43"))
        .respond_with(problem("not-found", 404))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["audit", "get", "42"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Entry:      42\n"))
        .stdout(predicate::str::contains("Actor:      alice (api)\n"))
        .stdout(predicate::str::contains("Target:     tag release\n"))
        .stdout(predicate::str::contains("Scenarios:  Login\n"))
        .stdout(predicate::str::contains(
            "Parameters:\n{\n  \"force\": true\n}\n",
        ))
        .stdout(predicate::str::contains(
            "Before:\n{\n  \"name\": \"release\"\n}\n",
        ))
        .stdout(predicate::str::contains("After:").not());
    cmd(&server, &dir)
        .args(["audit", "get", "43"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("not-found"));
    cmd(&server, &dir)
        .args(["audit", "get", USER])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "entry id must be a positive number",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn comments_list_sends_scenarios_and_flattens_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/comments"))
        .and(query_param("scenario", SCENARIO))
        .and(query_param("outcome", "failed"))
        .and(query_param("q", "positive"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [comment()], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/comments"))
        .and(query_param_is_missing("scenario"))
        .respond_with(problem("scope-missing", 403))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "comments",
            "list",
            "--scenario",
            SCENARIO,
            "--outcome",
            "failed",
            "-q",
            "positive",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tPUBLISHED\tAUTHOR\tSCENARIO\tRUN\tSTATUS\tTEXT\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{COMMENT}\t2026-10-02 08:00\tbob\tLogin\t{RUN}\tfailed\tFalse positive, ignore\n"
        )));
    cmd(&server, &dir)
        .args(["comments", "list"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `runs:read`",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn tickets_list_and_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/tickets"))
        .and(query_param("view", "open"))
        .and(query_param("kind", "Robot"))
        .and(query_param("scenario", SCENARIO))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [ticket()], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/tickets/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ticket": ticket(),
            "messages": [
                {"at": "2026-10-01T10:00:00+00:00", "author": "customer", "text": "It fails."},
                {"at": "2026-10-02T10:00:00+00:00", "author": "support", "text": "Looking."}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "tickets",
            "list",
            "--view",
            "open",
            "--kind",
            "Robot",
            "--scenario",
            SCENARIO,
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tSTATUS\tKIND\tCREATED\tUPDATED\tSCENARIOS\tTITLE\n",
        ))
        .stdout(predicate::str::contains(
            "7\topen\tRobot\t2026-10-01 10:00\t2026-10-02 10:00\tLogin\tLogin fails since Monday\n",
        ));
    cmd(&server, &dir)
        .args(["tickets", "get", "7"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Merged:     8\n"))
        .stdout(predicate::str::contains(
            "Incident:   since 2026-09-29 06:00\n",
        ))
        .stdout(predicate::str::contains("Keywords:   login\n"))
        .stdout(predicate::str::contains(
            "Summary:\nThe robot cannot log in.\n",
        ))
        .stdout(predicate::str::contains(
            "[2026-10-02 10:00] support:\nLooking.\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn tickets_report_the_space_switch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/tickets"))
        .respond_with(problem("tickets-not-enabled", 403))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["tickets", "list"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("tickets-not-enabled"));
    cmd(&server, &dir)
        .args(["tickets", "get", "zero"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "request id must be a positive number",
        ));
}
