//! End-to-end tests of `2bec availability` against a mock API. No network.

use crate::common::IsolatedEnv;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const OTHER: &str = "44444444-4444-4444-8444-444444444444";
const TAG_KO: &str = "11111111-1111-4111-8111-111111111111";
const TAG_OK: &str = "22222222-2222-4222-8222-222222222222";

fn settings() -> serde_json::Value {
    serde_json::json!({
        "computingType": "removeNeutralized", "failureCountsFrom": 1, "tagRulesEnabled": false,
        "tagRules": {
            "failure": [{"id": TAG_KO, "name": "KO", "kind": "business"}],
            "success": [], "neutral": []
        },
        "editable": true, "isDefault": false
    })
}

fn targets() -> serde_json::Value {
    serde_json::json!({
        "tenant": null, "effective": 99.0, "default": 99.0,
        "scenarios": [
            {"scenario": {"id": SCENARIO, "name": "Login", "kind": "transaction"}, "own": 99.5, "effective": 99.5},
            {"scenario": {"id": OTHER, "name": "Checkout", "kind": "transaction"}, "own": null, "effective": 99.0}
        ]
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/availability-settings", "traceId": "trace123"
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
async fn settings_get_prints_the_rules_and_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/availability-settings"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"s1\"")
                .set_body_json(settings()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["availability", "settings", "get"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Neutralized runs:    removed from the figures\n",
        ))
        .stdout(predicate::str::contains(
            "Down from:           1 consecutive failure(s)\n",
        ))
        .stdout(predicate::str::contains("Tag rules:           off\n"))
        .stdout(predicate::str::contains(format!(
            "  Failure tags:      KO ({TAG_KO})\n"
        )))
        .stdout(predicate::str::contains("  Success tags:      -\n"))
        .stdout(predicate::str::contains("ETag:                \"s1\"\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_set_reads_then_puts_the_merged_rules_with_if_match() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/availability-settings"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"s1\"")
                .set_body_json(settings()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut saved = settings();
    saved["computingType"] = "ignoreNeutralized".into();
    saved["failureCountsFrom"] = 2.into();
    saved["tagRulesEnabled"] = true.into();
    saved["tagRules"]["success"] =
        serde_json::json!([{"id": TAG_OK, "name": "OK", "kind": "business"}]);
    Mock::given(method("PUT"))
        .and(path("/v2/availability-settings"))
        .and(header("if-match", "\"s1\""))
        .and(body_json(serde_json::json!({
            "computingType": "ignoreNeutralized", "failureCountsFrom": 2, "tagRulesEnabled": true,
            "tagRules": {"failure": [TAG_KO], "success": [TAG_OK], "neutral": []}
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"s2\"")
                .set_body_json(serde_json::json!({
                    "settings": saved, "rulesChanged": true, "recomputeScheduled": true
                })),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "availability",
            "settings",
            "set",
            "--computing-type",
            "ignoreNeutralized",
            "--failure-counts-from",
            "2",
            "--tag-rules",
            "true",
            "--success-tags",
            TAG_OK,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Neutralized runs:    ignored\n"))
        .stdout(predicate::str::contains("ETag:                \"s2\"\n"))
        .stdout(predicate::str::contains(
            "The past days are being recomputed with the new rules.\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_set_clears_a_rule_with_an_empty_list_and_honours_if_match() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/availability-settings"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"s1\"")
                .set_body_json(settings()),
        )
        .mount(&server)
        .await;
    let mut saved = settings();
    saved["tagRules"]["failure"] = serde_json::json!([]);
    Mock::given(method("PUT"))
        .and(path("/v2/availability-settings"))
        .and(header("if-match", "\"mine\""))
        .and(body_json(serde_json::json!({
            "computingType": "removeNeutralized", "failureCountsFrom": 1, "tagRulesEnabled": false,
            "tagRules": {"failure": [], "success": [], "neutral": []}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "settings": saved, "rulesChanged": true, "recomputeScheduled": false
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "availability",
            "settings",
            "set",
            "--failure-tags",
            "",
            "--if-match",
            "\"mine\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("  Failure tags:      -\n"))
        .stderr(predicate::str::contains("did not take the recompute"));
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_set_refuses_locked_rules_and_bad_flags() {
    let server = MockServer::start().await;
    let mut locked = settings();
    locked["editable"] = false.into();
    Mock::given(method("GET"))
        .and(path("/v2/availability-settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(locked))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/v2/availability-settings"))
        .respond_with(problem("rules-locked", 403))
        .expect(0)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "availability",
            "settings",
            "set",
            "--failure-counts-from",
            "3",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("locked"));
    cmd(&server, &dir)
        .args(["availability", "settings", "set"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing to change"));
    cmd(&server, &dir)
        .args([
            "availability",
            "settings",
            "set",
            "--computing-type",
            "drop",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--computing-type `drop` is not one of",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn targets_get_prints_the_space_and_scenarios() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/availability-targets"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"a1\"")
                .set_body_json(targets()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["availability", "targets", "get"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Space target: 99% (platform default 99%)\n",
        ))
        .stdout(predicate::str::contains("ETag:         \"a1\"\n"))
        .stdout(predicate::str::contains(format!(
            "{SCENARIO}\tLogin\t99.5%\t99.5%\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{OTHER}\tCheckout\t-\t99%\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn targets_set_sends_tenant_and_scenarios_with_if_match() {
    let server = MockServer::start().await;
    let mut after = targets();
    after["tenant"] = 99.9.into();
    after["effective"] = 99.9.into();
    after["scenarios"][0]["own"] = serde_json::Value::Null;
    after["scenarios"][0]["effective"] = 99.9.into();
    Mock::given(method("PUT"))
        .and(path("/v2/availability-targets"))
        .and(header("if-match", "\"a1\""))
        .and(body_json(serde_json::json!({
            "tenant": 99.9,
            "scenarios": [{"scenario": SCENARIO, "target": null}, {"scenario": OTHER, "target": 99.95}]
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"a2\"")
                .set_body_json(serde_json::json!({
                    "written": 3, "unchanged": 0, "failed": 0, "targets": after
                })),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "availability",
            "targets",
            "set",
            "--tenant",
            "99.9",
            "--scenario",
            &format!("{SCENARIO}=default"),
            "--scenario",
            &format!("{OTHER}=99.95"),
            "--if-match",
            "\"a1\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Written: 3, unchanged: 0, failed: 0\n",
        ))
        .stdout(predicate::str::contains("Space target: 99.9% (own)\n"))
        .stdout(predicate::str::contains("ETag:         \"a2\"\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn targets_set_sends_a_null_tenant_for_default() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v2/availability-targets"))
        .and(body_json(
            serde_json::json!({"tenant": null, "scenarios": []}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "written": 1, "unchanged": 0, "failed": 0, "targets": targets()
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "availability",
            "targets",
            "set",
            "--tenant",
            "default",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"written\":1"));
    cmd(&server, &dir)
        .args(["availability", "targets", "set", "--tenant", "50"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("above 50 and below 100"));
    cmd(&server, &dir)
        .args(["availability", "targets", "set"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "give --tenant, --scenario or both",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_names_the_scope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/availability-targets"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/v2/availability-targets"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["availability", "targets", "get"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `metrics:read`",
        ));
    cmd(&server, &dir)
        .args(["availability", "targets", "set", "--tenant", "99"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `indicators:write`",
        ));
}
