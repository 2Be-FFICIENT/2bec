//! End-to-end tests of `2bec runs` against a mock API. No network.

use crate::common::IsolatedEnv;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{body_json, header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const RUN: &str = "11111111-1111-4111-8111-111111111111";
const RUN2: &str = "22222222-2222-4222-8222-222222222222";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const COMMENT: &str = "44444444-4444-4444-8444-444444444444";
const REQUEST: &str = "55555555-5555-4555-8555-555555555555";

fn scenario() -> serde_json::Value {
    serde_json::json!({"id": SCENARIO, "name": "Login", "kind": "transaction"})
}

fn run_item(id: &str, status: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "scenario": scenario(), "startedAt": "2026-10-11T08:00:00+02:00",
        "status": status, "statusCode": if status == "ok" {0} else {100},
        "performance": "target", "durationMs": 1500,
        "failedStep": if status == "ok" { serde_json::Value::Null } else {
            serde_json::json!({"id": null, "name": "login", "label": "Login", "order": 2}) },
        "replay": false, "tags": ["prod"], "notObservedReason": null
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/runs", "traceId": "trace123"
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
async fn list_sends_filters_and_prints_tsv_in_a_pipe() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/runs"))
        .and(query_param("status", "failed"))
        .and(query_param("period", "today"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [run_item(RUN, "failed")], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["runs", "list", "--status", "FAILED", "--period", "today"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tSTARTED\tSCENARIO\tSTATUS\tDURATION\tFAILED STEP\tTAGS\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{RUN}\t2026-10-11 08:00\tLogin\tfailed\t1.5s\tLogin\tprod\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_all_follows_cursors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/runs"))
        .and(query_param("limit", "200"))
        .and(query_param("cursor", "MjAw"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [run_item(RUN2, "ok")], "next": null, "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/runs"))
        .and(query_param("limit", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [run_item(RUN, "failed")], "next": "MjAw", "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let out = cmd(&server, &dir)
        .args(["runs", "list", "--all", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let ids: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [RUN, RUN2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_rejects_bad_filters_locally() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["runs", "list", "--status", "broken"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--status `broken` is not one of"));
    cmd(&server, &dir)
        .args(["runs", "get", "not-a-guid"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("must be a GUID"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_prints_the_run_and_maps_not_found() {
    let server = MockServer::start().await;
    let mut run = run_item(RUN, "failed");
    run["endedAt"] = "2026-10-11T08:00:02+02:00".into();
    run["voiceCall"] = false.into();
    run["steps"] = serde_json::json!([{"id": "s1", "name": "home", "label": "Home", "order": 1,
        "measured": true, "durationMs": 700, "serverMs": 100, "bytes": 2048, "performance": "target"}]);
    run["failure"] = serde_json::json!({"failedStep": {"id": null, "name": "login", "label": "Login", "order": 2},
        "failedAction": "click", "failedActionId": null, "failedStepStartedAt": null,
        "failedStepDurationMs": null, "inPlannedInterruption": false, "description": "Button missing", "confidence": 4});
    run["artifacts"] = serde_json::json!(["video", "screenshot"]);
    run["commentCount"] = 1.into();
    Mock::given(method("GET"))
        .and(path(format!("/v2/runs/{RUN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(run))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/runs/{RUN2}")))
        .respond_with(problem("not-found", 404))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["runs", "get", RUN])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status:      failed"))
        .stdout(predicate::str::contains("Description: Button missing"))
        .stdout(predicate::str::contains("Artifacts:   video, screenshot"))
        .stdout(predicate::str::contains(
            "1\tHome\t700ms\t100ms\t2048\ttarget",
        ));
    cmd(&server, &dir)
        .args(["runs", "get", RUN2])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("(not-found)"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_is_an_auth_failure() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/runs"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["runs", "list"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("lacks the scope"));
}

#[tokio::test(flavor = "multi_thread")]
async fn download_writes_the_file_atomically() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/runs/{RUN}/artifacts/screenshot")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "image/png")
                .set_body_bytes(b"PNGDATA".to_vec()),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/runs/{RUN}/artifacts/wav")))
        .respond_with(problem("no-artifact", 404))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("shot.png");

    cmd(&server, &dir)
        .args(["runs", "download", RUN, "screenshot", "-o"])
        .arg(&target)
        .assert()
        .success()
        .stderr(predicate::str::contains("(7 bytes)"));
    assert_eq!(std::fs::read(&target).unwrap(), b"PNGDATA");

    let missing = dir.path().join("audio.wav");
    cmd(&server, &dir)
        .args(["runs", "download", RUN, "wav", "-o"])
        .arg(&missing)
        .assert()
        .code(5);
    assert!(!missing.exists());
    assert!(!dir.path().join("audio.wav.part").exists());

    cmd(&server, &dir)
        .args(["runs", "download", RUN, "screenshot", "-o", "-"])
        .assert()
        .success()
        .stdout("PNGDATA");
}

#[tokio::test(flavor = "multi_thread")]
async fn comment_add_is_idempotent_and_edit_sends_if_match() {
    let server = MockServer::start().await;
    let comment = serde_json::json!({"id": COMMENT, "author": "ci", "at": "2026-10-11T09:00:00+02:00", "text": "hello"});
    Mock::given(method("POST"))
        .and(path(format!("/v2/runs/{RUN}/comments")))
        .and(header_exists("Idempotency-Key"))
        .and(body_json(serde_json::json!({"text": "hello"})))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("ETag", "W/\"abc\"")
                .set_body_json(comment.clone()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/runs/{RUN}/comments/{COMMENT}")))
        .and(header("If-Match", "W/\"abc\""))
        .and(header("content-type", "application/merge-patch+json"))
        .and(body_json(serde_json::json!({"text": "edited"})))
        .respond_with(ResponseTemplate::new(200).insert_header("ETag", "W/\"def\"").set_body_json(
            serde_json::json!({"id": COMMENT, "author": "ci", "at": "2026-10-11T09:00:00+02:00", "text": "edited"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/runs/{RUN}/comments/{COMMENT}")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let out = cmd(&server, &dir)
        .args(["runs", "comments", "add", RUN, "-", "--json"])
        .write_stdin("hello\n")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["etag"], "W/\"abc\"");

    cmd(&server, &dir)
        .args([
            "runs",
            "comments",
            "edit",
            RUN,
            COMMENT,
            "edited",
            "--if-match",
            "W/\"abc\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETag:    W/\"def\""));

    cmd(&server, &dir)
        .args(["runs", "comments", "rm", RUN, COMMENT])
        .assert()
        .success()
        .stderr(predicate::str::contains("Deleted comment"));

    cmd(&server, &dir)
        .args(["runs", "comments", "add", RUN, ""])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("empty"));
}

#[tokio::test(flavor = "multi_thread")]
async fn trigger_waits_until_the_request_is_done() {
    let server = MockServer::start().await;
    let request = |stage: &str| {
        serde_json::json!({"id": REQUEST, "scenario": scenario(), "requestedAt": "2026-10-11T10:00:00+02:00",
            "requestedBy": "ci", "variables": {"USER": "bob"}, "comment": "smoke", "outcome": "accepted",
            "error": null, "stage": stage, "warnings": []})
    };
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/run-requests")))
        .and(header_exists("Idempotency-Key"))
        .and(body_json(
            serde_json::json!({"variables": {"USER": "bob"}, "comment": "smoke"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(request("sleeping")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/run-requests/{REQUEST}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(request("done")))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "runs",
            "trigger",
            SCENARIO,
            "--var",
            "USER=bob",
            "--comment",
            "smoke",
            "--wait",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Requested run"))
        .stdout(predicate::str::contains("accepted (sleeping)"))
        .stdout(predicate::str::contains("Final: accepted (done)"));
}

#[tokio::test(flavor = "multi_thread")]
async fn trigger_quota_is_reported_as_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/run-requests")))
        .respond_with(problem("run-quota", 429))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["runs", "trigger", SCENARIO])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("(run-quota)"));
}
