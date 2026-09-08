//! End-to-end tests of `2bec alerts` against a mock API. No network.

mod common;

use assert_cmd::Command;
use common::IsolatedEnv;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{body_json, header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const ALERT: &str = "11111111-1111-4111-8111-111111111111";
const GROUP: &str = "22222222-2222-4222-8222-222222222222";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const STEP: &str = "44444444-4444-4444-8444-444444444444";
const PERSON: &str = "55555555-5555-4555-8555-555555555555";
const WINDOW: &str = "66666666-6666-4666-8666-666666666666";

fn scenario() -> serde_json::Value {
    serde_json::json!({"id": SCENARIO, "name": "Login", "kind": "transaction"})
}

fn alert(name: &str, active: bool) -> serde_json::Value {
    serde_json::json!({
        "id": ALERT, "name": name, "active": active,
        "group": {"id": GROUP, "name": "On call", "channel": "sms", "members": 2},
        "failure": {
            "notifyRestoration": true, "triggerMode": "ranks", "ranks": [1, 3], "every": 0,
            "tagMode": "none", "tags": [],
            "steps": [{"id": STEP, "scenario": scenario(), "label": "Login", "order": 2, "suspended": false, "stale": true}],
            "hiddenSteps": 1
        },
        "extraFailureRules": 0,
        "responseTime": null, "extraResponseTimeRules": 0,
        "social": [],
        "schedule": {"kind": "always", "quietPeriods": []},
        "health": {"noRecipient": false, "watchesNothing": false, "staleSteps": true, "duplicate": false, "unreachable": false},
        "mailSubject": null
    })
}

fn group() -> serde_json::Value {
    serde_json::json!({
        "id": GROUP, "name": "On call", "channel": "sms", "hookType": null, "deliverable": true,
        "alerts": 1,
        "members": [{"id": PERSON, "name": "Ada", "muted": false}]
    })
}

fn window(state: &str) -> serde_json::Value {
    serde_json::json!({
        "id": WINDOW, "start": "2026-11-02T02:00:00+01:00", "end": "2026-11-02T04:00:00+01:00",
        "state": state, "scope": "all", "scenarios": [], "hiddenTargets": 0,
        "scenarioPattern": null, "stepPattern": null, "comment": "DB upgrade",
        "createdBy": "Ada", "createdAt": "2026-10-12T10:00:00+02:00"
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/alerts", "traceId": "trace123"
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
async fn list_shows_what_each_alert_watches_and_its_issues() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/alerts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [alert("Checkout down", true)], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["alerts", "list"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tNAME\tACTIVE\tGROUP\tCHANNEL\tWATCHES\tISSUES\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{ALERT}\tCheckout down\tyes\tOn call\tsms\t2 steps\tstale steps\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_prints_the_rules_and_the_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/alerts/{ALERT}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"7\"")
                .set_body_json(alert("Checkout down", true)),
        )
        .expect(2)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["alerts", "get", ALERT])
        .assert()
        .success()
        .stdout(predicate::str::contains("Health:       stale steps\n"))
        .stdout(predicate::str::contains("ETag:         W/\"7\"\n"))
        .stdout(predicate::str::contains(
            "Failure rule: at ranks 1, 3, restoration notified\n",
        ))
        .stdout(predicate::str::contains("Login\t2\tLogin\tstale\n"))
        .stdout(predicate::str::contains(
            "1 step(s) of scenarios you do not see",
        ));

    cmd(&server, &dir)
        .args(["alerts", "get", ALERT, "--jq", ".etag"])
        .assert()
        .success()
        .stdout(predicate::eq("\"W/\\\"7\\\"\"\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn notifications_sends_canonical_filters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/notifications"))
        .and(query_param("scenario", SCENARIO))
        .and(query_param("period", "today"))
        .and(query_param("channel", "sms"))
        .and(query_param("trigger", "responseTime"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [{
                "id": "n1", "sentAt": "2026-10-12T08:00:00+02:00", "scenario": scenario(),
                "run": null, "channel": "sms", "trigger": "responseTime", "alert": "Slow login",
                "group": "On call", "sends": 2,
                "recipients": [{"name": "Ada", "sentAt": "2026-10-12T08:00:00+02:00"},
                               {"name": "Bob", "sentAt": "2026-10-12T08:00:01+02:00"}]
            }],
            "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "alerts",
            "notifications",
            "--scenario",
            SCENARIO,
            "--period",
            "TODAY",
            "--channel",
            "SMS",
            "--trigger",
            "responsetime",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "2026-10-12 08:00\tLogin\tresponseTime\tsms\tSlow login\tOn call\tAda, Bob\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_posts_the_body_with_an_idempotency_key() {
    let server = MockServer::start().await;
    let body = serde_json::json!({"name": "Checkout down", "group": GROUP,
        "failure": {"scenarios": [SCENARIO], "ranks": [1, 3]}});
    Mock::given(method("POST"))
        .and(path("/v2/alerts"))
        .and(header("content-type", "application/json"))
        .and(header_exists("idempotency-key"))
        .and(body_json(&body))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "W/\"1\"")
                .set_body_json(alert("Checkout down", true)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["alerts", "create", "-"])
        .write_stdin(body.to_string())
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Created alert Checkout down ({ALERT})"
        )))
        .stdout(predicate::str::contains("Name:         Checkout down\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_sends_a_merge_patch_with_if_match() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/alerts/{ALERT}")))
        .and(header("content-type", "application/merge-patch+json"))
        .and(header("if-match", "W/\"7\""))
        .and(body_json(serde_json::json!({"active": false})))
        .respond_with(ResponseTemplate::new(200).set_body_json(alert("Checkout down", false)))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "alerts",
            "edit",
            ALERT,
            "{\"active\": false}",
            "--if-match",
            "W/\"7\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Active:       no\n"));

    cmd(&server, &dir)
        .args(["alerts", "edit", ALERT, "{not json"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("the body is not valid JSON"));
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_and_duplicate() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/alerts/{ALERT}")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/alerts/{ALERT}:duplicate")))
        .and(header_exists("idempotency-key"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(alert("Checkout down (copy)", false)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["alerts", "rm", ALERT])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Deleted alert {ALERT}")));
    cmd(&server, &dir)
        .args(["alerts", "duplicate", ALERT, "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"name\":\"Checkout down (copy)\"",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_alert_validates_locally_then_posts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/alerts:test"))
        .and(body_json(serde_json::json!({
            "channel": "mail", "recipient": PERSON, "mails": ["ops@example.com"], "message": "drill"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "channel": "mail", "sent": 2, "recipients": ["Ada", "ops@example.com"]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "alerts",
            "test",
            "--channel",
            "MAIL",
            "--recipient",
            PERSON,
            "--mail",
            "ops@example.com",
            "--message",
            "drill",
        ])
        .assert()
        .success()
        .stdout(predicate::eq(
            "Sent 2 test alerts on mail to: Ada, ops@example.com\n",
        ));

    cmd(&server, &dir)
        .args([
            "alerts",
            "test",
            "--channel",
            "sms",
            "--mail",
            "ops@example.com",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--mail works on the mail channel only",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn groups_list_get_create_and_in_use_delete() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/alert-groups"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [group()], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/alert-groups/{GROUP}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"3\"")
                .set_body_json(group()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/alert-groups"))
        .and(body_json(
            serde_json::json!({"name": "On call", "channel": "sms", "members": [PERSON]}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(group()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/alert-groups/{GROUP}")))
        .respond_with(problem("alert-group-in-use", 409))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["alerts", "groups", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{GROUP}\tOn call\tsms\tyes\t1\t1\n"
        )));
    cmd(&server, &dir)
        .args(["alerts", "groups", "get", GROUP])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETag:        W/\"3\"\n"))
        .stdout(predicate::str::contains(format!("{PERSON}\tAda\t\n")));
    cmd(&server, &dir)
        .args([
            "alerts",
            "groups",
            "create",
            "--name",
            "On call",
            "--channel",
            "sms",
            "--member",
            PERSON,
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Created group On call"));
    cmd(&server, &dir)
        .args(["alerts", "groups", "delete", GROUP])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("alert-group-in-use"));
}

#[tokio::test(flavor = "multi_thread")]
async fn recipients_list_and_mute() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/alert-recipients"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [{"id": PERSON, "login": "ada", "name": "Ada", "hasMail": true, "hasMobile": false,
                       "muted": false, "groups": [{"id": GROUP, "name": "On call"}]}],
            "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/alert-recipients/{PERSON}:mute")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": PERSON, "name": "Ada", "muted": true
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["alerts", "recipients", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{PERSON}\tAda\tada\tyes\tno\t\tOn call\n"
        )));
    cmd(&server, &dir)
        .args(["alerts", "recipients", "mute", PERSON])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("Ada is muted"));
}

#[tokio::test(flavor = "multi_thread")]
async fn suspensions_overview_steps_and_windows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/alert-suspensions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tenantSuspended": false, "platformSuspended": false,
            "suspendedSteps": [{"scenario": scenario(), "active": true,
                "steps": [{"id": STEP, "label": "Login", "order": 2, "watchedBy": 1}]}],
            "hiddenSteps": 0,
            "windows": [window("ongoing")], "windowsAvailable": true,
            "mutedRecipients": ["Ada"]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/alert-suspensions/steps:suspend"))
        .and(body_json(
            serde_json::json!({"steps": [{"scenario": SCENARIO, "step": STEP}]}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "written": 1, "unchanged": 0, "failed": 0
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/alert-suspensions/windows"))
        .and(body_json(serde_json::json!({
            "start": "2026-11-02T01:00:00Z", "end": "2026-11-02T03:00:00Z", "all": true, "comment": "DB upgrade"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(window("upcoming")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/alert-suspensions/windows/{WINDOW}:end")))
        .respond_with(problem("window-not-ongoing", 409))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["alerts", "suspensions"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Muted people:    Ada\n"))
        .stdout(predicate::str::contains("Held steps:      1\n"))
        .stdout(predicate::str::contains(format!(
            "Login\t2\tLogin\t{STEP}\t1 alert(s)\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{WINDOW}\tongoing\t2026-11-02 02:00\t2026-11-02 04:00\tevery alert\tDB upgrade\n"
        )));
    cmd(&server, &dir)
        .args([
            "alerts",
            "suspensions",
            "suspend",
            &format!("{SCENARIO}/{STEP}"),
        ])
        .assert()
        .success()
        .stdout(predicate::eq("Held 1 step(s), 0 unchanged\n"));
    cmd(&server, &dir)
        .args([
            "alerts",
            "suspensions",
            "windows",
            "create",
            "--start",
            "2026-11-02T01:00:00Z",
            "--end",
            "2026-11-02T03:00:00Z",
            "--all",
            "--comment",
            "DB upgrade",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Created window {WINDOW} (upcoming)"
        )))
        .stdout(predicate::str::contains("Scope:   every alert\n"));
    cmd(&server, &dir)
        .args([
            "alerts",
            "suspensions",
            "windows",
            "create",
            "--start",
            "a",
            "--end",
            "b",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "give --all or at least one --step",
        ));
    cmd(&server, &dir)
        .args(["alerts", "suspensions", "windows", "end", WINDOW])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("window-not-ongoing"));
}
