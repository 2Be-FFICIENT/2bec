//! End-to-end tests of `2bec incidents` and `2bec interruptions` against a
//! mock API. No network.

mod common;

use assert_cmd::Command;
use common::IsolatedEnv;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{body_json, header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const MATE: &str = "44444444-4444-4444-8444-444444444444";
const INCIDENT: &str = "55555555-5555-4555-8555-555555555555";
const RULE: &str = "66666666-6666-4666-8666-666666666666";

fn scenario_ref(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "name": name, "kind": "transaction"})
}

fn incident_row() -> serde_json::Value {
    serde_json::json!({
        "id": INCIDENT, "scenario": scenario_ref(SCENARIO, "Login"), "start": "2026-10-11T08:00:00+02:00",
        "end": null, "ongoing": true, "durationSeconds": 3720.0, "failedStep": "Pay", "failedRuns": 12,
        "firstRun": {"id": INCIDENT, "startedAt": "2026-10-11T08:00:00+02:00"},
        "lastRun": {"id": MATE, "startedAt": "2026-10-11T09:00:00+02:00"}
    })
}

fn rule() -> serde_json::Value {
    serde_json::json!({
        "id": RULE, "kind": "weekly", "state": "rule", "exclusion": true, "comment": "Weekend batch",
        "spec": {"kind": "weekly", "weekly": {"startDay": 6, "startTime": "22:00", "endDay": 7, "endTime": "06:00"}},
        "scenarios": [scenario_ref(SCENARIO, "Login"), scenario_ref(MATE, "Login mobile")], "hiddenScenarios": 1,
        "validFrom": "2026-09-01T00:00:00+02:00", "validUntil": null, "applied": true, "replaces": null,
        "occurrences": [{"start": "2026-10-17T22:00:00+02:00", "end": "2026-10-18T06:00:00+02:00"}]
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/interruptions", "traceId": "trace123"
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
async fn incidents_list_sends_filters_and_prints_rows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/incidents"))
        .and(query_param("scenario", SCENARIO))
        .and(query_param("period", "lastMonth"))
        .and(query_param("ongoing", "true"))
        .and(query_param("minDuration", "600"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [incident_row()], "next": null, "total": 4
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "incidents",
            "list",
            "--scenario",
            SCENARIO,
            "--period",
            "lastmonth",
            "--ongoing",
            "--min-duration",
            "600",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{INCIDENT}\tLogin\t2026-10-11 08:00\tongoing\t1h02m\tPay\t12\n"
        )))
        .stderr(predicate::str::contains("Showing 1 of 4 incidents"));
}

#[tokio::test(flavor = "multi_thread")]
async fn incidents_get_prints_failure_and_runs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/incidents/{INCIDENT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": INCIDENT, "scenario": scenario_ref(SCENARIO, "Login"), "start": "2026-10-11T08:00:00+02:00",
            "end": "2026-10-11T09:02:00+02:00", "ongoing": false, "durationSeconds": 3720, "failedStep": "Pay", "failedRuns": 2,
            "runs": [
                {"id": INCIDENT, "startedAt": "2026-10-11T08:00:00+02:00", "status": "failed", "failedStep": "Pay"},
                {"id": MATE, "startedAt": "2026-10-11T09:02:00+02:00", "status": "ok", "failedStep": null}
            ],
            "failure": {"failedStep": {"id": "s4", "name": "Step 4 - Pay", "label": "Pay", "order": 4},
                "failedAction": "click #pay", "failedActionId": "a1", "failedStepStartedAt": "2026-10-11T08:00:10+02:00",
                "failedStepDurationMs": 30000, "inPlannedInterruption": false,
                "description": "Pay button missing", "confidence": 4},
            "alertsSent": 3
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["incidents", "get", INCIDENT])
        .assert()
        .success()
        .stdout(predicate::str::contains("End:          2026-10-11 09:02"))
        .stdout(predicate::str::contains("Duration:     1h02m"))
        .stdout(predicate::str::contains("Alerts sent:  3"))
        .stdout(predicate::str::contains(
            "  Description: Pay button missing",
        ))
        .stdout(predicate::str::contains("  Confidence:  4/5"))
        .stdout(predicate::str::contains(format!(
            "{MATE}\t2026-10-11 09:02\tok\t-\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn interruptions_list_sends_filters_and_describes_specs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/interruptions"))
        .and(query_param("state", "rule"))
        .and(query_param("include", "occurrences"))
        .and(query_param("from", "2026-10-01T00:00:00Z"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [rule(), {
                "id": INCIDENT, "kind": "date", "state": "upcoming", "exclusion": true, "comment": null,
                "spec": {"kind": "date", "date": {"start": "2026-10-12T22:00:00+02:00", "end": "2026-10-13T02:00:00+02:00"}},
                "scenarios": [scenario_ref(SCENARIO, "Login")], "hiddenScenarios": 0,
                "validFrom": null, "validUntil": null, "applied": false, "replaces": null, "occurrences": null
            }],
            "next": null, "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "interruptions", "list", "--state", "RULE", "--occurrences", "--from", "2026-10-01T00:00:00Z",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{RULE}\tweekly\trule\tweekly Sat 22:00 to Sun 06:00\tLogin +1 (1 hidden)\tWeekend batch\t2026-10-17 22:00\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{INCIDENT}\tdate\tupcoming\t2026-10-12 22:00 to 2026-10-13 02:00\tLogin\t-\t-\n"
        )));

    cmd(&server, &dir)
        .args(["interruptions", "list", "--state", "later"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--state `later` is not one of"));
}

#[tokio::test(flavor = "multi_thread")]
async fn interruptions_get_prints_details_occurrences_and_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/interruptions/{RULE}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"i1\"")
                .set_body_json(rule()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["interruptions", "get", RULE])
        .assert()
        .success()
        .stdout(predicate::str::contains("Kind:         weekly (rule)"))
        .stdout(predicate::str::contains(
            "When:         weekly Sat 22:00 to Sun 06:00",
        ))
        .stdout(predicate::str::contains(
            "Valid:        from 2026-09-01 00:00 until now (has applied)",
        ))
        .stdout(predicate::str::contains(format!(
            "Scenarios:    Login ({SCENARIO}), Login mobile ({MATE}), 1 hidden"
        )))
        .stdout(predicate::str::contains("ETag:         \"i1\""))
        .stdout(predicate::str::contains(
            "2026-10-17 22:00\t2026-10-18 06:00\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn interruptions_create_window_from_flags_and_reports_refusals() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/interruptions"))
        .and(header_exists("idempotency-key"))
        .and(body_json(serde_json::json!({
            "scenarios": [SCENARIO, MATE],
            "spec": {"kind": "date", "date": {"start": "2026-10-12T22:00:00Z", "end": "2026-10-13T02:00:00Z"}},
            "comment": "Maintenance"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "interruptions": [rule()],
            "failed": [{"scenarios": [scenario_ref(MATE, "Login mobile")], "reason": "calendar locked"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "interruptions",
            "create",
            "--scenario",
            SCENARIO,
            "--scenario",
            MATE,
            "--start",
            "2026-10-12T22:00:00Z",
            "--end",
            "2026-10-13T02:00:00Z",
            "--comment",
            " Maintenance ",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: refused for Login mobile: calendar locked",
        ))
        .stderr(predicate::str::contains(
            "Planned 1 interruption(s), one per script",
        ))
        .stdout(predicate::str::contains(format!("Interruption: {RULE}")));

    cmd(&server, &dir)
        .args(["interruptions", "create", "--scenario", SCENARIO])
        .assert()
        .code(2);
    cmd(&server, &dir)
        .args([
            "interruptions",
            "create",
            "--scenario",
            SCENARIO,
            "--spec",
            r#"{"kind": "daily"}"#,
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "a `daily` spec needs a `daily` object",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn interruptions_create_spec_refused_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/interruptions"))
        .respond_with(problem("refused", 422))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "interruptions", "create", "--scenario", SCENARIO, "--spec",
            r#"{"kind": "daily", "daily": {"weekdays": [1], "bankHolidays": false, "hours": [{"start": "02:00", "end": "03:00"}]}}"#,
            "--starts-at", "2026-09-01T00:00:00Z",
        ])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("refused"));
}

#[tokio::test(flavor = "multi_thread")]
async fn interruptions_edit_sends_a_merge_patch_with_if_match() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/interruptions/{RULE}")))
        .and(header("content-type", "application/merge-patch+json"))
        .and(header("if-match", "\"i1\""))
        .and(body_json(serde_json::json!({"comment": null})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"i2\"")
                .set_body_json(rule()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "interruptions",
            "edit",
            RULE,
            "--comment",
            "",
            "--if-match",
            "\"i1\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETag:         \"i2\""));
    cmd(&server, &dir)
        .args(["interruptions", "edit", RULE])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing to change"));
}

#[tokio::test(flavor = "multi_thread")]
async fn interruptions_end_handles_both_answers() {
    let server = MockServer::start().await;
    let mut ended = rule();
    ended["state"] = "ended".into();
    ended["validUntil"] = "2026-10-12T10:00:00+02:00".into();
    Mock::given(method("POST"))
        .and(path(format!("/v2/interruptions/{RULE}:end")))
        .respond_with(ResponseTemplate::new(200).set_body_json(ended))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/interruptions/{INCIDENT}:end")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/interruptions/{MATE}:end")))
        .respond_with(problem("interruption-not-ongoing", 409))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["interruptions", "end", RULE])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Ended interruption {RULE}"
        )))
        .stdout(predicate::str::contains("Kind:         weekly (ended)"))
        .stdout(predicate::str::contains("until 2026-10-12 10:00"));
    cmd(&server, &dir)
        .args(["interruptions", "end", INCIDENT])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Rule {INCIDENT} never applied: deleted"
        )));
    cmd(&server, &dir)
        .args(["interruptions", "end", MATE])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("interruption-not-ongoing"));
}

#[tokio::test(flavor = "multi_thread")]
async fn interruptions_restore_and_delete() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/interruptions/{RULE}:restore")))
        .respond_with(ResponseTemplate::new(200).set_body_json(rule()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/interruptions/{RULE}")))
        .and(query_param("erasePast", "true"))
        .and(header("if-match", "\"i1\""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["interruptions", "restore", RULE])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Restored rule {RULE}")));
    cmd(&server, &dir)
        .args([
            "interruptions",
            "rm",
            RULE,
            "--erase-past",
            "--if-match",
            "\"i1\"",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Deleted interruption {RULE}"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_names_the_right_scopes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/incidents"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/interruptions"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["incidents", "list"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `metrics:read`",
        ));
    cmd(&server, &dir)
        .args([
            "interruptions",
            "create",
            "--scenario",
            SCENARIO,
            "--start",
            "2026-10-12T22:00:00Z",
            "--end",
            "2026-10-13T02:00:00Z",
        ])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `monitoring:write`",
        ));
}
