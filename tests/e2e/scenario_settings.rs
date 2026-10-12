//! End-to-end tests of `2bec scenarios settings|edit|rename-steps|run-variables|thresholds`
//! against a mock API. No network.

use crate::common::IsolatedEnv;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{body_json, header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const STEP1: &str = "11111111-1111-4111-8111-111111111111";
const STEP2: &str = "22222222-2222-4222-8222-222222222222";
const VERSION: &str = "44444444-4444-4444-8444-444444444444";
const BANDWIDTH: &str = "55555555-5555-4555-8555-555555555555";
const CATALOGUE: &str = "66666666-6666-4666-8666-666666666666";

fn settings() -> serde_json::Value {
    serde_json::json!({
        "id": SCENARIO, "name": "Login", "description": "Checked every 5 minutes", "catalogueId": null,
        "steps": [
            {"id": STEP1, "order": 1, "name": "Home", "measured": true},
            {"id": STEP2, "order": 2, "name": "Login", "measured": false}
        ],
        "runVariables": [{"name": "USER", "defaultValue": "alice"}, {"name": "ENV", "defaultValue": null}],
        "updatedAt": "2026-10-01T10:00:00+02:00"
    })
}

fn thresholds() -> serde_json::Value {
    serde_json::json!({
        "scenario": {"id": SCENARIO, "name": "Login", "kind": "transaction"},
        "versions": [{
            "id": VERSION, "begin": "2026-09-01T08:00:00+02:00", "end": null, "comment": "v12", "current": true,
            "steps": [{"id": STEP1, "name": "Step 1 - Home", "label": "Home", "order": 1}],
            "bandwidths": [{
                "id": BANDWIDTH, "bandwidth": "ADSL", "kiloBytesPerSecond": 1024, "preferred": true,
                "run": {"target": 3000.0, "client": 6000.0, "unacceptable": 12000.0},
                "steps": [{"target": 800.0, "client": 1500.0, "unacceptable": 3000.0}]
            }]
        }],
        "quality": {
            "completion": {"target": 0, "client": 1, "unacceptable": 5},
            "regularity": {"target": 10, "client": 15, "unacceptable": 20},
            "restoration": {"target": 30, "client": 60, "unacceptable": 90},
            "isDefault": true
        }
    })
}

fn computation() -> serde_json::Value {
    serde_json::json!({
        "from": "2026-10-01", "to": "2026-10-08",
        "settings": {"percentile": 0.85, "clientConstantSeconds": 1.0, "percentTarget": 0.8,
                     "percentUnacceptable": 1.25, "percentDuration": 1.0},
        "versions": [{"id": VERSION, "runs": 120, "bandwidths": [{
            "id": BANDWIDTH, "bandwidth": "ADSL",
            "run": {"proposed": {"target": 2400.0, "client": 3000.0, "unacceptable": 3750.0},
                    "keptCurrent": false, "samples": 120,
                    "currentBands": {"target": 100, "client": 15, "insufficient": 3, "unacceptable": 2},
                    "proposedBands": {"target": 90, "client": 20, "insufficient": 6, "unacceptable": 4}},
            "steps": [{"id": STEP1, "name": "Home", "percentile": 1700.0,
                       "proposal": {"proposed": {"target": 2160.0, "client": 2700.0, "unacceptable": 3375.0},
                                    "keptCurrent": false, "samples": 120, "currentBands": null, "proposedBands": null}}]
        }]}],
        "apply": {"bandwidths": [{
            "id": BANDWIDTH,
            "run": {"target": 2400.0, "client": 3000.0, "unacceptable": 3750.0},
            "steps": [{"id": STEP1, "thresholds": {"target": 2160.0, "client": 2700.0, "unacceptable": 3375.0}}]
        }], "computation": {"from": "2026-10-01", "to": "2026-10-08", "settings": null, "bandwidths": null}}
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/scenarios", "traceId": "trace123"
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
async fn settings_prints_steps_variables_and_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/settings")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1\"")
                .set_body_json(settings()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "settings", SCENARIO])
        .assert()
        .success()
        .stdout(predicate::str::contains("Name:         Login\n"))
        .stdout(predicate::str::contains("Catalogue id: -\n"))
        .stdout(predicate::str::contains("ETag:         \"v1\"\n"))
        .stdout(predicate::str::contains(format!("1\t{STEP1}\tHome\tyes\n")))
        .stdout(predicate::str::contains("USER\talice\n"))
        .stdout(predicate::str::contains("ENV\t-\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_json_carries_the_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/settings")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1\"")
                .set_body_json(settings()),
        )
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let out = cmd(&server, &dir)
        .args(["scenarios", "settings", SCENARIO, "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["etag"], "\"v1\"");
    assert_eq!(v["runVariables"][0]["name"], "USER");
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_sends_a_merge_patch_with_nulls_and_if_match() {
    let server = MockServer::start().await;
    let mut updated = settings();
    updated["name"] = "Login (prod)".into();
    updated["description"] = serde_json::Value::Null;
    updated["catalogueId"] = CATALOGUE.into();
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/scenarios/{SCENARIO}")))
        .and(header("content-type", "application/merge-patch+json"))
        .and(header("if-match", "\"v1\""))
        .and(body_json(serde_json::json!({
            "name": "Login (prod)", "description": null, "catalogueId": CATALOGUE
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v2\"")
                .set_body_json(updated),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "edit",
            SCENARIO,
            "--name",
            "Login (prod)",
            "--clear-description",
            "--catalogue-id",
            CATALOGUE,
            "--if-match",
            "\"v1\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Name:         Login (prod)\n"))
        .stdout(predicate::str::contains("Description:  -\n"))
        .stdout(predicate::str::contains(format!(
            "Catalogue id: {CATALOGUE}\n"
        )))
        .stdout(predicate::str::contains("ETag:         \"v2\"\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_without_a_change_does_not_call() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "edit", SCENARIO])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing to change"));
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_reports_staff_only_fields_and_stale_etags() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/scenarios/{SCENARIO}")))
        .and(header("if-match", "\"old\""))
        .respond_with(problem("precondition-failed", 412))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/scenarios/{SCENARIO}")))
        .respond_with(problem("staff-only-field", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "edit",
            SCENARIO,
            "--name",
            "X",
            "--if-match",
            "\"old\"",
        ])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("precondition-failed"));
    cmd(&server, &dir)
        .args(["scenarios", "edit", SCENARIO, "--name", "X"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("staff-only-field"));
}

#[tokio::test(flavor = "multi_thread")]
async fn rename_steps_puts_the_names_listed() {
    let server = MockServer::start().await;
    let mut updated = settings();
    updated["steps"][0]["name"] = "Open the home page".into();
    Mock::given(method("PUT"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/steps")))
        .and(body_json(serde_json::json!({
            "steps": [{"id": STEP1, "name": "Open the home page"}]
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v2\"")
                .set_body_json(updated),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "rename-steps",
            SCENARIO,
            &format!("{STEP1}=Open the home page"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "1\t{STEP1}\tOpen the home page\tyes\n"
        )));
    cmd(&server, &dir)
        .args(["scenarios", "rename-steps", SCENARIO, "nope=Home"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("step id must be a GUID"));
}

#[tokio::test(flavor = "multi_thread")]
async fn run_variables_replaces_the_list_or_clears_it() {
    let server = MockServer::start().await;
    let mut updated = settings();
    updated["runVariables"] = serde_json::json!([{"name": "ENV", "defaultValue": "prod"}]);
    Mock::given(method("PUT"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/run-variables")))
        .and(header("if-match", "\"v1\""))
        .and(body_json(serde_json::json!({
            "variables": [{"name": "ENV", "defaultValue": "prod"}, {"name": "USER", "defaultValue": null}]
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v2\"")
                .set_body_json(updated),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut cleared = settings();
    cleared["runVariables"] = serde_json::json!([]);
    Mock::given(method("PUT"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/run-variables")))
        .and(body_json(serde_json::json!({"variables": []})))
        .respond_with(ResponseTemplate::new(200).set_body_json(cleared))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "run-variables",
            SCENARIO,
            "ENV=prod",
            "USER",
            "--if-match",
            "\"v1\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ENV\tprod\n"));
    cmd(&server, &dir)
        .args(["scenarios", "run-variables", SCENARIO, "--clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Run variables: none\n"));
    cmd(&server, &dir)
        .args(["scenarios", "run-variables", SCENARIO, "SCHEDULER_X=1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("reserved"));
    cmd(&server, &dir)
        .args(["scenarios", "run-variables", SCENARIO])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--clear"));
}

#[tokio::test(flavor = "multi_thread")]
async fn thresholds_get_prints_each_bandwidth_and_step() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"t1\"")
                .set_body_json(thresholds()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "thresholds", "get", SCENARIO])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Scenario:    Login ({SCENARIO})\n"
        )))
        .stdout(predicate::str::contains("(platform defaults)"))
        .stdout(predicate::str::contains("ETag:        \"t1\"\n"))
        .stdout(predicate::str::contains(format!(
            "Version {VERSION} (current) from 2026-09-01 08:00: v12\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{BANDWIDTH}\tADSL (1024 kB/s) *\trun\t3.0s\t6.0s\t12.0s\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{STEP1}\t\tHome\t800ms\t1.5s\t3.0s\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn thresholds_get_refuses_a_social_scenario() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds")))
        .respond_with(problem("social-scenario", 409))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "thresholds", "get", SCENARIO])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("social-scenario"));
}

#[tokio::test(flavor = "multi_thread")]
async fn thresholds_compute_posts_the_period_and_prints_proposals() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds:compute")))
        .and(header_exists("idempotency-key"))
        .and(body_json(serde_json::json!({
            "from": "2026-10-01", "to": "2026-10-08",
            "settings": {"percentile": 0.9, "clientConstantSeconds": 1.0, "percentTarget": 0.8,
                         "percentUnacceptable": 1.25, "percentDuration": 1.0},
            "bandwidths": [BANDWIDTH]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(computation()))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "thresholds",
            "compute",
            SCENARIO,
            "--from",
            "2026-10-01",
            "--to",
            "2026-10-08",
            "--percentile",
            "0.9",
            "--bandwidth",
            BANDWIDTH,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Period:      2026-10-01 to 2026-10-08 (excluded)",
        ))
        .stdout(predicate::str::contains(format!(
            "Version {VERSION} (120 successful runs), bandwidth ADSL ({BANDWIDTH})\n"
        )))
        .stdout(predicate::str::contains(
            "run\t120\t-\ttarget 2.4s, client 3.0s, unacceptable 3.8s\t\n",
        ))
        .stdout(predicate::str::contains(
            "Home\t120\t1.7s\ttarget 2.2s, client 2.7s, unacceptable 3.4s\t\n",
        ))
        .stdout(predicate::str::contains(
            "Run bands:   now 100 target / 15 client / 3 insufficient / 2 unacceptable; proposed 90 / 20 / 6 / 4\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn thresholds_compute_apply_posts_the_computed_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds:compute")))
        .respond_with(ResponseTemplate::new(200).set_body_json(computation()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds:apply")))
        .and(body_json(serde_json::json!({"bandwidths": [{
            "id": BANDWIDTH,
            "run": {"target": 2400.0, "client": 3000.0, "unacceptable": 3750.0},
            "steps": [{"id": STEP1, "thresholds": {"target": 2160.0, "client": 2700.0, "unacceptable": 3375.0}}]
        }], "computation": {"from": "2026-10-01", "to": "2026-10-08"}})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"t2\"")
                .set_body_json(thresholds()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "thresholds",
            "compute",
            SCENARIO,
            "--from",
            "2026-10-01",
            "--to",
            "2026-10-08",
            "--apply",
            "--yes",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"etag\":\"\\\"t2\\\"\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn thresholds_compute_apply_needs_a_confirmation_off_a_terminal() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds:compute")))
        .respond_with(ResponseTemplate::new(200).set_body_json(computation()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds:apply")))
        .respond_with(ResponseTemplate::new(200).set_body_json(thresholds()))
        .expect(0)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "thresholds",
            "compute",
            SCENARIO,
            "--from",
            "2026-10-01",
            "--to",
            "2026-10-08",
            "--apply",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not confirmed; pass --yes"));
}

#[tokio::test(flavor = "multi_thread")]
async fn thresholds_compute_rejects_a_long_period_locally() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "thresholds",
            "compute",
            SCENARIO,
            "--from",
            "2026-07-01",
            "--to",
            "2026-10-01",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("62 days at most"));
}

#[tokio::test(flavor = "multi_thread")]
async fn thresholds_apply_reads_the_body_from_stdin_with_if_match() {
    let server = MockServer::start().await;
    let body = serde_json::json!({"bandwidths": [{
        "id": BANDWIDTH, "run": {"target": 3000, "client": 6000, "unacceptable": 12000}
    }]});
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds:apply")))
        .and(header("if-match", "\"t1\""))
        .and(body_json(serde_json::json!({"bandwidths": [{
            "id": BANDWIDTH, "run": {"target": 3000.0, "client": 6000.0, "unacceptable": 12000.0}, "steps": []
        }]})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"t2\"")
                .set_body_json(thresholds()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "thresholds",
            "apply",
            SCENARIO,
            "-",
            "--if-match",
            "\"t1\"",
        ])
        .write_stdin(body.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("ETag:        \"t2\"\n"));
    cmd(&server, &dir)
        .args([
            "scenarios",
            "thresholds",
            "apply",
            SCENARIO,
            "{\"bandwidths\":[]}",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("names no bandwidth"));
}

#[tokio::test(flavor = "multi_thread")]
async fn thresholds_apply_reports_a_conflict() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds:apply")))
        .respond_with(problem("thresholds-changed", 409))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "thresholds",
            "apply",
            SCENARIO,
            &format!("{{\"bandwidths\":[{{\"id\":\"{BANDWIDTH}\"}}]}}"),
        ])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("thresholds-changed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_names_the_scope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/thresholds:compute")))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/steps")))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "thresholds",
            "compute",
            SCENARIO,
            "--from",
            "2026-10-01",
            "--to",
            "2026-10-08",
        ])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `indicators:write`",
        ));
    cmd(&server, &dir)
        .args(["scenarios", "rename-steps", SCENARIO, &format!("{STEP1}=X")])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `scenarios:write`",
        ));
}
