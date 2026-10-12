//! End-to-end tests of `2bec neutralizations` against a mock API. No network.

use crate::common::IsolatedEnv;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{
    body_json, body_partial_json, header_exists, method, path, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const OP: &str = "77777777-7777-4777-8777-777777777777";
const UNDO: &str = "88888888-8888-4888-8888-888888888888";
const CAP: &str = "99999999-9999-4999-8999-999999999999";

fn scenario_ref() -> serde_json::Value {
    serde_json::json!({"id": SCENARIO, "name": "Login", "kind": "transaction"})
}

fn operation(id: &str, state: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "kind": "neutralize", "state": state, "undoes": null, "undoneBy": null,
        "selection": {"scenario": [SCENARIO], "from": "2026-10-11T00:00:00Z", "to": "2026-10-12T00:00:00Z",
            "status": "failed", "weekdays": [6, 7], "dayStart": "22:00", "dayStop": "06:00"},
        "scenarios": [scenario_ref()], "hiddenScenarioCount": 0,
        "period": {"from": "2026-10-11T00:00:00+00:00", "to": "2026-10-12T00:00:00+00:00"},
        "comment": "CDN outage", "matched": 12, "applied": if state == "running" { 0 } else { 10 }, "skipped": 2,
        "createdAt": "2026-10-12T10:00:00+02:00",
        "completedAt": if state == "running" { serde_json::Value::Null } else { "2026-10-12T10:00:05+02:00".into() },
        "createdBy": "ops key", "origin": "pegas", "error": null, "canUndo": state == "applied"
    })
}

fn preview(applicable: i64) -> serde_json::Value {
    serde_json::json!({
        "matched": 12, "applicable": applicable, "alreadyNeutralized": 2, "protected": 0, "cap": CAP,
        "firstRunAt": "2026-10-11T01:00:00+02:00", "lastRunAt": "2026-10-11T23:00:00+02:00",
        "commentRequired": false, "asynchronous": false,
        "scenarios": [{"scenario": scenario_ref(), "matched": 12, "applicable": applicable}],
        "statuses": [{"status": "failed", "count": 12}],
        "sample": [{"id": OP, "scenario": scenario_ref(), "startedAt": "2026-10-11T01:00:00+02:00",
            "status": "failed", "failedStep": "Pay", "outcome": "applied"}],
        "warnings": ["A planned interruption overlaps the period"]
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/neutralizations", "traceId": "trace123"
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
async fn list_sends_filters_and_prints_rows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/neutralizations"))
        .and(query_param("scenario", SCENARIO))
        .and(query_param("kind", "neutralize"))
        .and(query_param("state", "applied"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [operation(OP, "applied")], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "neutralizations",
            "list",
            "--scenario",
            SCENARIO,
            "--kind",
            "Neutralize",
            "--state",
            "APPLIED",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{OP}\tneutralize\tapplied\t2026-10-12 10:00\tops key\tLogin\t12\t10\tCDN outage\n"
        )));
    cmd(&server, &dir)
        .args(["neutralizations", "list", "--state", "pending"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--state `pending` is not one of"));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_prints_the_operation_and_waits_while_running() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/neutralizations/{OP}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(operation(OP, "running")))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/neutralizations/{OP}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"n1\"")
                .set_body_json(operation(OP, "applied")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["neutralizations", "get", OP, "--wait"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Kind:         neutralize (applied, from pegas)",
        ))
        .stdout(predicate::str::contains(
            "Filters:      status failed, weekdays 6,7, 22:00-06:00",
        ))
        .stdout(predicate::str::contains(
            "Runs:         12 matched, 10 applied, 2 skipped",
        ))
        .stdout(predicate::str::contains("Undoable:     yes"));
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_dry_run_prints_the_preview_and_writes_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/neutralizations"))
        .and(query_param("dryRun", "true"))
        .and(header_exists("idempotency-key"))
        .and(body_json(serde_json::json!({
            "selection": {"scenario": [SCENARIO], "from": "2026-10-11T00:00:00Z", "to": "2026-10-12T00:00:00Z",
                "status": "failed", "weekdays": [6, 7], "dayStart": "22:00", "dayStop": "06:00"},
            "comment": "CDN outage"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(preview(10)))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "neutralizations",
            "apply",
            "--from",
            "2026-10-11T00:00:00Z",
            "--to",
            "2026-10-12T00:00:00Z",
            "--scenario",
            SCENARIO,
            "--status",
            "failed",
            "--weekday",
            "6",
            "--weekday",
            "7",
            "--day-start",
            "22:00",
            "--day-stop",
            "06:00",
            "--comment",
            "CDN outage",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Would change:  10 of 12 matched run(s) (2 already neutralized, 0 protected)",
        ))
        .stdout(predicate::str::contains("By status:     12 failed"))
        .stdout(predicate::str::contains(format!(
            "{SCENARIO}\tLogin\t12\t10\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{OP}\tLogin\t2026-10-11 01:00\tfailed\tPay\tapplied\n"
        )))
        .stderr(predicate::str::contains(
            "warning: A planned interruption overlaps the period",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_without_a_terminal_needs_yes_and_then_skips_the_dry_run() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/neutralizations"))
        .and(query_param("dryRun", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(preview(10)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/neutralizations"))
        .and(query_param_is_missing("dryRun"))
        .and(body_json(serde_json::json!({
            "selection": {"from": "2026-10-11T00:00:00Z", "to": "2026-10-12T00:00:00Z"}
        })))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"n1\"")
                .set_body_json(operation(OP, "applied")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "neutralizations",
            "apply",
            "--from",
            "2026-10-11T00:00:00Z",
            "--to",
            "2026-10-12T00:00:00Z",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Would change:  10 of 12"))
        .stderr(predicate::str::contains("not confirmed; pass --yes"));
    cmd(&server, &dir)
        .args([
            "neutralizations",
            "apply",
            "--from",
            "2026-10-11T00:00:00Z",
            "--to",
            "2026-10-12T00:00:00Z",
            "--yes",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Applied operation {OP}")))
        .stdout(predicate::str::contains("ETag:         \"n1\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_with_nothing_applicable_stops_before_asking() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/neutralizations"))
        .and(query_param("dryRun", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(preview(0)))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "neutralizations",
            "apply",
            "--from",
            "2026-10-11T00:00:00Z",
            "--to",
            "2026-10-12T00:00:00Z",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Nothing to neutralize"));
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_with_yes_and_wait_polls_a_background_operation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/neutralizations"))
        .respond_with(
            ResponseTemplate::new(202)
                .insert_header("location", format!("/v2/neutralizations/{OP}"))
                .set_body_json(operation(OP, "running")),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/neutralizations/{OP}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(operation(OP, "applied")))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "neutralizations",
            "apply",
            "--from",
            "2026-10-11T00:00:00Z",
            "--to",
            "2026-10-12T00:00:00Z",
            "--yes",
            "--wait",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Started operation {OP}")))
        .stdout(predicate::str::contains(
            "Kind:         neutralize (applied, from pegas)",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_selection_changed_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/neutralizations"))
        .respond_with(problem("selection-changed", 409))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "neutralizations",
            "apply",
            "--from",
            "2026-10-11T00:00:00Z",
            "--to",
            "2026-10-12T00:00:00Z",
            "--yes",
        ])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("selection-changed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_sends_the_comment_and_reports() {
    let server = MockServer::start().await;
    let mut undo = operation(UNDO, "applied");
    undo["kind"] = "restore".into();
    undo["undoes"] = OP.into();
    undo["canUndo"] = false.into();
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/neutralizations/{OP}")))
        .and(query_param("comment", "Ours after all"))
        .and(header_exists("idempotency-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(undo))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/neutralizations/{UNDO}")))
        .respond_with(problem("neutralization-not-undoable", 409))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["neutralizations", "undo", OP, "--comment", "Ours after all"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Applied undo {UNDO} of operation {OP}"
        )))
        .stdout(predicate::str::contains(
            "Kind:         restore (applied, from pegas)",
        ))
        .stdout(predicate::str::contains(format!("Undoes:       {OP}")));
    cmd(&server, &dir)
        .args(["neutralizations", "undo", UNDO])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("neutralization-not-undoable"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_names_runs_write() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/neutralizations"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["neutralizations", "list"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `runs:write`",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_body_with_partial_json_matcher_keeps_the_period() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/neutralizations"))
        .and(body_partial_json(
            serde_json::json!({"selection": {"from": "2026-10-11T00:00:00Z"}}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(operation(OP, "applied")))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "neutralizations",
            "apply",
            "--from",
            "2026-10-11T00:00:00Z",
            "--to",
            "2026-10-12T00:00:00Z",
            "-y",
        ])
        .assert()
        .success();
}
