//! End-to-end tests of `2bec planning` against a mock API. No network.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{body_json, header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const MATE: &str = "44444444-4444-4444-8444-444444444444";
const EVENT: &str = "55555555-5555-4555-8555-555555555555";

fn scenario_ref(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "name": name, "kind": "transaction"})
}

fn pause_row(paused: bool) -> serde_json::Value {
    serde_json::json!({
        "scenario": scenario_ref(SCENARIO, "Login"), "paused": paused,
        "pausedSince": if paused { serde_json::Value::from("2026-10-12T08:00:00+02:00") } else { serde_json::Value::Null },
        "lastResumedAt": "2026-10-01T09:00:00+02:00",
        "sharedWith": [scenario_ref(MATE, "Login mobile")], "hiddenSharedWith": 1
    })
}

fn event() -> serde_json::Value {
    serde_json::json!({
        "id": EVENT, "scope": "scenario", "category": "release", "expectedImpact": "performance",
        "title": "Release 4.2", "description": "New checkout", "start": "2026-10-12T10:00:00+02:00",
        "end": null, "instant": true, "externalReference": "CHG-42", "externalUrl": "https://tickets.example/CHG-42",
        "labels": ["checkout"], "source": "api", "scenarios": [scenario_ref(SCENARIO, "Login")],
        "hiddenScenarios": 0, "attachments": 0, "createdBy": "ops key",
        "createdAt": "2026-10-12T09:00:00+02:00", "updatedBy": null, "updatedAt": null
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/events", "traceId": "trace123"
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
async fn timeline_sends_filters_and_prints_items() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/timeline"))
        .and(query_param("scenario", SCENARIO))
        .and(query_param("kind", "incident"))
        .and(query_param("incidentMinMinutes", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "period": {"from": "2026-10-05T00:00:00+02:00", "to": "2026-10-19T00:00:00+02:00"},
            "items": [
                {"key": "i1", "kind": "incident", "scope": "scenario", "scenarios": [scenario_ref(SCENARIO, "Login")],
                 "hiddenScenarios": 0, "start": "2026-10-11T08:00:00+02:00", "end": "2026-10-11T09:02:00+02:00",
                 "instant": false, "allDay": false, "ongoing": false, "title": "Login down 1h02m", "summary": null,
                 "severity": "down", "category": null, "expectedImpact": null, "labels": [], "actor": null,
                 "eventId": null, "interruptionIds": null, "runId": "r1", "failedRuns": 12, "comments": 1},
                {"key": "p1", "kind": "monitoringPause", "scope": "scenario",
                 "scenarios": [scenario_ref(SCENARIO, "Login"), scenario_ref(MATE, "Login mobile")],
                 "hiddenScenarios": 1, "start": "2026-10-12T08:00:00+02:00", "end": null,
                 "instant": false, "allDay": false, "ongoing": true, "title": "Monitoring paused", "summary": null,
                 "severity": "info", "category": null, "expectedImpact": null, "labels": [], "actor": "ops",
                 "eventId": null, "interruptionIds": null, "runId": null, "failedRuns": null, "comments": null}
            ],
            "unavailable": ["scriptVersion"], "skipped": [], "truncated": false
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "planning", "timeline", "--scenario", SCENARIO, "--kind", "INCIDENT", "--incident-min-minutes", "0",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Range:  2026-10-05 00:00 to 2026-10-19 00:00"))
        .stdout(predicate::str::contains(
            "2026-10-11 08:00\t2026-10-11 09:02\tincident\tdown\tscenario\tLogin\tLogin down 1h02m\tr1\n",
        ))
        .stdout(predicate::str::contains(
            "2026-10-12 08:00\tongoing\tmonitoringPause\tinfo\tscenario\tLogin +1 (1 hidden)\tMonitoring paused\t-\n",
        ))
        .stderr(predicate::str::contains("warning: scriptVersion items could not be read"));

    cmd(&server, &dir)
        .args(["planning", "timeline", "--kind", "holiday"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--kind `holiday` is not one of"));
}

#[tokio::test(flavor = "multi_thread")]
async fn pauses_lists_paused_scenarios_and_all() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pauses"))
        .and(query_param("all", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [pause_row(true), {
                "scenario": scenario_ref(MATE, "Pay"), "paused": false, "pausedSince": null,
                "lastResumedAt": null, "sharedWith": [], "hiddenSharedWith": 0
            }],
            "next": null, "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["planning", "pauses", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{SCENARIO}\tLogin\tyes\t2026-10-12 08:00\t2026-10-01 09:00\tLogin mobile, 1 hidden\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{MATE}\tPay\tno\t-\t-\t-\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_needs_confirmation_then_acknowledges() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}:pause")))
        .and(header_exists("idempotency-key"))
        .and(body_json(serde_json::json!({"acknowledge": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "action": "pause", "at": "2026-10-12T08:00:00+02:00",
            "changed": [pause_row(true)], "unchanged": [], "hiddenChanged": 1,
            "interruptions": [], "warnings": ["Alerts are silenced while paused"]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["planning", "pause", SCENARIO])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not confirmed; pass --yes"));
    cmd(&server, &dir)
        .args(["planning", "pause", SCENARIO, "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Paused at 2026-10-12 08:00: 1 scenario(s) changed, 0 already so, 1 hidden mate(s) moved too",
        ))
        .stdout(predicate::str::contains(format!("{SCENARIO}\tLogin\tyes\t")))
        .stderr(predicate::str::contains("warning: Alerts are silenced while paused"));
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_sends_the_comment_and_prints_the_interruption() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}:resume")))
        .and(body_json(serde_json::json!({"comment": "Release done"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "action": "resume", "at": "2026-10-12T09:00:00+02:00",
            "changed": [pause_row(false)], "unchanged": [], "hiddenChanged": 0,
            "interruptions": [{"scenarios": [scenario_ref(SCENARIO, "Login"), scenario_ref(MATE, "Login mobile")],
                "start": "2026-10-12T08:00:00+02:00", "end": "2026-10-12T09:00:00+02:00", "comment": "Release done"}],
            "warnings": []
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "planning",
            "resume",
            SCENARIO,
            "--comment",
            " Release done ",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Resumed at 2026-10-12 09:00: 1 scenario(s) changed, 0 already so",
        ))
        .stdout(predicate::str::contains(
            "2026-10-12 08:00\t2026-10-12 09:00\tLogin, Login mobile\tRelease done\n",
        ));

    cmd(&server, &dir)
        .args([
            "planning",
            "resume",
            SCENARIO,
            "--comment",
            &"x".repeat(501),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--comment is 500 characters at most",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_of_a_social_scenario_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}:resume")))
        .respond_with(problem("social-scenario", 409))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["planning", "resume", SCENARIO])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("social-scenario"));
}

#[tokio::test(flavor = "multi_thread")]
async fn events_list_sends_filters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/events"))
        .and(query_param("period", "lastMonth"))
        .and(query_param("category", "release"))
        .and(query_param("impact", "availability"))
        .and(query_param("q", "outage"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [event()], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "planning",
            "events",
            "list",
            "--period",
            "lastmonth",
            "--category",
            "release",
            "--impact",
            "AVAILABILITY",
            "-q",
            " outage ",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{EVENT}\t2026-10-12 10:00\t-\trelease\tperformance\tscenario\tLogin\tRelease 4.2\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn events_get_prints_details_and_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/events/{EVENT}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"e1\"")
                .set_body_json(event()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["planning", "events", "get", EVENT])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "When:         2026-10-12 10:00 (instant)",
        ))
        .stdout(predicate::str::contains("Scope:        scenario (Login)"))
        .stdout(predicate::str::contains(
            "Reference:    CHG-42 https://tickets.example/CHG-42",
        ))
        .stdout(predicate::str::contains("Labels:       checkout"))
        .stdout(predicate::str::contains(
            "Created:      2026-10-12 09:00 by ops key",
        ))
        .stdout(predicate::str::contains("ETag:         \"e1\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn events_create_builds_the_body_from_flags() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/events"))
        .and(header_exists("idempotency-key"))
        .and(body_json(serde_json::json!({
            "category": "release", "expectedImpact": "performance", "title": "Release 4.2",
            "description": "New checkout", "start": "2026-10-12T10:00:00+02:00",
            "externalReference": "CHG-42", "externalUrl": "https://tickets.example/CHG-42",
            "labels": ["checkout"], "scenarios": [SCENARIO]
        })))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"e1\"")
                .set_body_json(event()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "planning",
            "events",
            "create",
            "--title",
            " Release 4.2 ",
            "--start",
            "2026-10-12T10:00:00+02:00",
            "--category",
            "release",
            "--impact",
            "Performance",
            "--description",
            "New checkout",
            "--ref",
            "CHG-42",
            "--url",
            "https://tickets.example/CHG-42",
            "--label",
            "checkout",
            "--label",
            " ",
            "--scenario",
            SCENARIO,
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Created event {EVENT}")))
        .stdout(predicate::str::contains("ETag:         \"e1\""));

    cmd(&server, &dir)
        .args([
            "planning", "events", "create", "--title", "x", "--start", "now", "--impact", "huge",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--impact `huge` is not one of"));
}

#[tokio::test(flavor = "multi_thread")]
async fn events_edit_clears_with_empty_values_and_sends_if_match() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/events/{EVENT}")))
        .and(header("content-type", "application/merge-patch+json"))
        .and(header("if-match", "\"e1\""))
        .and(body_json(serde_json::json!({
            "end": "2026-10-12T11:00:00+02:00", "description": null, "externalReference": null,
            "labels": ["a", "b"]
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"e2\"")
                .set_body_json(event()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/events/{EVENT}")))
        .respond_with(problem("event-read-only", 403))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "planning",
            "events",
            "edit",
            EVENT,
            "--end",
            "2026-10-12T11:00:00+02:00",
            "--description",
            "",
            "--ref",
            "",
            "--label",
            "a",
            "--label",
            "b",
            "--if-match",
            "\"e1\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETag:         \"e2\""));
    cmd(&server, &dir)
        .args(["planning", "events", "edit", EVENT])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing to change"));
    cmd(&server, &dir)
        .args(["planning", "events", "rm", EVENT])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("event-read-only"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_names_planning_read() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pauses"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["planning", "pauses"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `planning:read`",
        ));
}
