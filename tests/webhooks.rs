//! End-to-end tests of `2bec webhooks` against a mock API. No network.

mod common;

use assert_cmd::Command;
use common::IsolatedEnv;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{
    body_json, body_partial_json, header, header_exists, method, path, query_param,
    query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const HOOK: &str = "88888888-8888-4888-8888-888888888888";
const DELIVERY: &str = "99999999-9999-4999-8999-999999999999";
const API_KEY: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

fn webhook() -> serde_json::Value {
    serde_json::json!({
        "id": HOOK, "url": "https://ops.example.com/2be", "events": ["run.failed", "run.recovered"],
        "description": "Ops bridge", "active": true, "state": "active",
        "disabledReason": null, "disabledAt": null,
        "apiKey": {"id": API_KEY, "name": "ops"},
        "lastDeliveryAt": "2026-10-12T09:00:00+02:00", "lastDeliveryStatus": 200,
        "previousSecretUntil": null,
        "createdAt": "2026-10-01T10:00:00+02:00", "updatedAt": null
    })
}

fn delivery(status: &str) -> serde_json::Value {
    serde_json::json!({
        "id": DELIVERY, "event": "run.failed", "occurredAt": "2026-10-12T08:59:00+02:00",
        "status": status, "attempts": 2,
        "nextAttemptAt": if status == "pending" { serde_json::Value::from("2026-10-12T09:10:00+02:00") } else { serde_json::Value::Null },
        "lastAttemptAt": "2026-10-12T09:05:00+02:00",
        "responseStatus": if status == "delivered" { 200 } else { 0 },
        "error": if status == "delivered" { serde_json::Value::Null } else { "connection refused".into() },
        "durationMs": 120,
        "deliveredAt": if status == "delivered" { serde_json::Value::from("2026-10-12T09:05:00+02:00") } else { serde_json::Value::Null },
        "createdAt": "2026-10-12T08:59:30+02:00"
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/webhooks", "traceId": "trace123"
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
async fn list_prints_state_key_and_last_delivery() {
    let server = MockServer::start().await;
    let mut disabled = webhook();
    disabled["id"] = "77777777-7777-4777-8777-777777777777".into();
    disabled["state"] = "disabled".into();
    disabled["disabledReason"] = "failing".into();
    disabled["lastDeliveryStatus"] = 0.into();
    Mock::given(method("GET"))
        .and(path("/v2/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [webhook(), disabled], "next": null, "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "list"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tURL\tSTATE\tEVENTS\tKEY\tLAST DELIVERY\tSTATUS\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{HOOK}\thttps://ops.example.com/2be\tactive\trun.failed,run.recovered\tops\t2026-10-12 09:00\t200\n"
        )))
        .stdout(predicate::str::contains(
            "\tdisabled (failing)\trun.failed,run.recovered\tops\t2026-10-12 09:00\tunreachable\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_prints_details_and_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/webhooks/{HOOK}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v3\"")
                .set_body_json(webhook()),
        )
        .expect(2)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "get", HOOK])
        .assert()
        .success()
        .stdout(predicate::str::contains("State:         active"))
        .stdout(predicate::str::contains(
            "Events:        run.failed, run.recovered",
        ))
        .stdout(predicate::str::contains(format!(
            "Reads with:    ops ({API_KEY})"
        )))
        .stdout(predicate::str::contains(
            "Last delivery: 2026-10-12 09:00 (200)",
        ))
        .stdout(predicate::str::contains("ETag:          \"v3\""))
        .stdout(predicate::str::contains("Secret").not());

    let out = cmd(&server, &dir)
        .args(["webhooks", "get", HOOK, "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["etag"], "\"v3\"");
    assert_eq!(v["url"], "https://ops.example.com/2be");
}

#[tokio::test(flavor = "multi_thread")]
async fn events_lists_the_catalog() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/webhook-events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [{"code": "run.failed", "scope": "runs:read", "description": "A run failed."}],
            "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "events"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "run.failed\truns:read\tA run failed.\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_sends_a_canonical_body_and_prints_the_secret_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/webhooks"))
        .and(header_exists("idempotency-key"))
        .and(body_json(serde_json::json!({
            "url": "https://ops.example.com/2be",
            "events": ["run.failed", "run.recovered"],
            "description": "Ops bridge",
            "active": false
        })))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"v1\"")
                .set_body_json(serde_json::json!({"webhook": webhook(), "secret": "whsec_abc"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "webhooks",
            "create",
            "--url",
            " https://ops.example.com/2be ",
            "--event",
            "run.recovered",
            "--event",
            "run.failed",
            "--description",
            " Ops bridge ",
            "--inactive",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETag:          \"v1\""))
        .stdout(predicate::str::contains("Secret:        whsec_abc"))
        .stderr(predicate::str::contains("shown this once"));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_rejects_bad_input_locally() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "webhooks",
            "create",
            "--url",
            "https://localhost/h",
            "--event",
            "run.failed",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--url must reach a public host"));
    cmd(&server, &dir)
        .args([
            "webhooks",
            "create",
            "--url",
            "https://e.example.com/h",
            "--event",
            "run.boom",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--event `run.boom` is not one of"));
    cmd(&server, &dir)
        .args(["webhooks", "create", "--url", "https://e.example.com/h"])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn create_past_the_limit_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/webhooks"))
        .respond_with(problem("limit-reached", 409))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "webhooks",
            "create",
            "--url",
            "https://e.example.com/h",
            "--event",
            "run.failed",
        ])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("limit-reached"));
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_sends_a_merge_patch_with_if_match() {
    let server = MockServer::start().await;
    let mut updated = webhook();
    updated["active"] = false.into();
    updated["state"] = "inactive".into();
    updated["description"] = serde_json::Value::Null;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/webhooks/{HOOK}")))
        .and(header("content-type", "application/merge-patch+json"))
        .and(header("if-match", "\"v3\""))
        .and(body_json(serde_json::json!({
            "active": false, "description": null, "events": ["incident.opened"]
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v4\"")
                .set_body_json(updated),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "webhooks",
            "edit",
            HOOK,
            "--active",
            "false",
            "--description",
            "",
            "--event",
            "incident.opened",
            "--if-match",
            "\"v3\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("State:         inactive"))
        .stdout(predicate::str::contains("Description:   -"))
        .stdout(predicate::str::contains("ETag:          \"v4\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_without_a_change_does_not_call() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "edit", HOOK])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing to change"));
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_sends_if_match_and_reports() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/webhooks/{HOOK}")))
        .and(header("if-match", "\"v3\""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "rm", HOOK, "--if-match", "\"v3\""])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Deleted webhook {HOOK}")));
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_prints_the_new_secret_and_the_grace() {
    let server = MockServer::start().await;
    let mut rotated = webhook();
    rotated["previousSecretUntil"] = "2026-10-13T10:00:00+02:00".into();
    Mock::given(method("POST"))
        .and(path(format!("/v2/webhooks/{HOOK}:rotate-secret")))
        .and(header_exists("idempotency-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v5\"")
                .set_body_json(serde_json::json!({"webhook": rotated, "secret": "whsec_new"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let out = cmd(&server, &dir)
        .args(["webhooks", "rotate", HOOK, "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["secret"], "whsec_new");
    assert_eq!(v["etag"], "\"v5\"");
    assert_eq!(v["previousSecretUntil"], "2026-10-13T10:00:00+02:00");
}

#[tokio::test(flavor = "multi_thread")]
async fn ping_prints_the_attempt() {
    let server = MockServer::start().await;
    let mut d = delivery("delivered");
    d["event"] = "ping".into();
    d["attempts"] = 1.into();
    Mock::given(method("POST"))
        .and(path(format!("/v2/webhooks/{HOOK}:ping")))
        .respond_with(ResponseTemplate::new(200).set_body_json(d))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "ping", HOOK])
        .assert()
        .success()
        .stdout(predicate::str::contains("Event:         ping"))
        .stdout(predicate::str::contains("Status:        delivered"))
        .stdout(predicate::str::contains(
            "Last attempt:  2026-10-12 09:05 answered 200 in 120 ms",
        ))
        .stdout(predicate::str::contains("Delivered:     2026-10-12 09:05"));
}

#[tokio::test(flavor = "multi_thread")]
async fn deliveries_list_filters_and_shows_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/webhooks/{HOOK}/deliveries")))
        .and(query_param("status", "failed"))
        .and(query_param("limit", "50"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [delivery("failed")], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "deliveries", "list", HOOK, "--status", "FAILED"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{DELIVERY}\trun.failed\t2026-10-12 08:59\tfailed\t2\t2026-10-12 09:05\tconnection refused\t-\n"
        )));

    cmd(&server, &dir)
        .args(["webhooks", "deliveries", "list", HOOK, "--status", "lost"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--status `lost` is not one of"));
}

#[tokio::test(flavor = "multi_thread")]
async fn deliveries_get_prints_the_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/webhooks/{HOOK}/deliveries/{DELIVERY}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "delivery": delivery("pending"),
            "body": {"id": "e1", "type": "run.failed", "at": "2026-10-12T08:59:00+02:00",
                "webhook": HOOK, "data": {"id": "r1", "status": "failed"},
                "links": {"resource": "/v2/runs/r1"}}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "deliveries", "get", HOOK, DELIVERY])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Attempts:      2, next at 2026-10-12 09:10",
        ))
        .stdout(predicate::str::contains(
            "Error:         connection refused",
        ))
        .stdout(predicate::str::contains(
            "Body:\n{\n  \"id\": \"e1\",\n  \"type\": \"run.failed\"",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn redeliver_queues_and_prints() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v2/webhooks/{HOOK}/deliveries/{DELIVERY}:redeliver"
        )))
        .and(body_partial_json(serde_json::json!({})))
        .respond_with(ResponseTemplate::new(202).set_body_json(delivery("pending")))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "deliveries", "redeliver", HOOK, DELIVERY])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Queued delivery {DELIVERY} again"
        )))
        .stdout(predicate::str::contains("Status:        pending"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_names_webhooks_write() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/webhooks"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["webhooks", "list"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `webhooks:write`",
        ));
}
