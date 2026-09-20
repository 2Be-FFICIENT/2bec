//! End-to-end tests of `2bec scenarios` against a mock API. No network.

mod common;

use assert_cmd::Command;
use common::IsolatedEnv;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const SOCIAL: &str = "66666666-6666-4666-8666-666666666666";
const GROUP: &str = "77777777-7777-4777-8777-777777777777";

fn item(id: &str, name: &str, kind: &str) -> serde_json::Value {
    let social = kind == "socialMonitoring";
    serde_json::json!({
        "id": id, "name": name, "kind": kind, "description": null,
        "os": if social { serde_json::Value::Null } else { "Windows".into() },
        "browser": if social { serde_json::Value::Null } else { "Chrome".into() },
        "voiceCall": false, "active": true,
        "intervalSeconds": if social { 1800 } else { 300 },
        "replayAcceleration": false,
        "group": {"id": GROUP, "name": "Web"},
        "socialSource": if social { "twitter".into() } else { serde_json::Value::Null }
    })
}

fn scenario() -> serde_json::Value {
    let mut s = item(SCENARIO, "Login", "transaction");
    let extra = serde_json::json!({
        "createdAt": "2026-01-01T00:00:00+00:00", "updatedAt": "2026-10-01T10:00:00+02:00",
        "monitoring": {"state": "paused", "since": "2026-10-10T08:00:00+02:00", "until": null, "comment": "maintenance"},
        "availabilityTarget": {"own": null, "effective": 99.5},
        "runThresholds": {"target": 3000.0, "client": 6000.0, "unacceptable": 12000.0},
        "steps": [
            {"id": "s1", "name": "Step 1 - Home", "label": "Home", "order": 1, "measured": true,
             "thresholds": {"target": 800.0, "client": 1500.0, "unacceptable": 3000.0}},
            {"id": "s2", "name": "Step 2 - Login", "label": "Login", "order": 2, "measured": false, "thresholds": null}
        ]
    });
    s.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    s
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
async fn list_sends_filters_and_prints_tsv_in_a_pipe() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/scenarios"))
        .and(query_param("kind", "transaction"))
        .and(query_param("group", GROUP))
        .and(query_param("q", "login"))
        .and(query_param("includeInactive", "false"))
        .and(query_param("limit", "50"))
        .and(query_param_is_missing("voice"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [item(SCENARIO, "Login", "transaction")], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "list",
            "--kind",
            "Transaction",
            "--group",
            GROUP,
            "-q",
            "login",
            "--active",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ID\tNAME\tKIND\tACTIVE\tINTERVAL\tGROUP\tPLATFORM\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{SCENARIO}\tLogin\ttransaction\tyes\t5m\tWeb\tWindows / Chrome\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_all_follows_cursors_and_labels_social_rows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/scenarios"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [item(SCENARIO, "Login", "transaction")], "next": "c2", "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/scenarios"))
        .and(query_param("cursor", "c2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [item(SOCIAL, "Brand on X", "socialMonitoring")], "next": null, "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "list", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("{SCENARIO}\tLogin\t")))
        .stdout(predicate::str::contains(format!(
            "{SOCIAL}\tBrand on X\tsocial\tyes\t30m\tWeb\t-\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_rejects_bad_kind_locally() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();
    cmd(&server, &dir)
        .args(["scenarios", "list", "--kind", "voice"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--kind `voice` is not one of"));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_prints_details_and_steps() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(scenario()))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "get", SCENARIO])
        .assert()
        .success()
        .stdout(predicate::str::contains("Name:         Login\n"))
        .stdout(predicate::str::contains(format!(
            "Group:        Web ({GROUP})\n"
        )))
        .stdout(predicate::str::contains(
            "Monitoring:   paused since 2026-10-10T08:00:00+02:00 (maintenance)\n",
        ))
        .stdout(predicate::str::contains(
            "Availability: 99.5% target (space default)\n",
        ))
        .stdout(predicate::str::contains(
            "Thresholds:   target 3.0s, client 6.0s, unacceptable 12.0s\n",
        ))
        .stdout(predicate::str::contains(
            "1\tHome\tyes\t800ms\t1.5s\t3.0s\n",
        ))
        .stdout(predicate::str::contains("2\tLogin\tno\t-\t-\t-\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_json_passes_the_body_through() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(scenario()))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let out = cmd(&server, &dir)
        .args(["scenarios", "get", SCENARIO, "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v, scenario());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_rejects_a_bad_id_without_calling() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();
    cmd(&server, &dir)
        .args(["scenarios", "get", "nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("scenario id must be a GUID"));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn versions_lists_saves_and_notes_older_ones() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/versions")))
        .and(query_param("months", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "scenarioId": SCENARIO, "months": 3, "since": "2026-07-11T08:00:00+00:00",
            "currentVersion": 12, "olderVersions": 4,
            "items": [
                {"id": "v12", "version": 12, "savedAt": "2026-10-01T10:00:00+02:00", "current": true,
                 "stable": true, "summary": "Renamed the login step", "description": "…", "note": null},
                {"id": "v11", "version": 11, "savedAt": "2026-09-01T10:00:00+02:00", "current": false,
                 "stable": false, "summary": null, "description": null, "note": null}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "versions", SCENARIO, "--months", "3"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "VERSION\tSAVED\tCURRENT\tSTABLE\tSUMMARY\n",
        ))
        .stdout(predicate::str::contains(
            "12\t2026-10-01 10:00\tyes\tyes\tRenamed the login step\n",
        ))
        .stdout(predicate::str::contains("11\t2026-09-01 10:00\t\t\t-\n"))
        .stderr(predicate::str::contains(
            "4 older version(s) before 2026-07-11 08:00 not shown",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn versions_of_a_social_scenario_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SOCIAL}/versions")))
        .respond_with(problem("social-scenario", 409))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "versions", SOCIAL])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("social-scenario"));
}

#[tokio::test(flavor = "multi_thread")]
async fn calendar_prints_cadence_hours_and_windows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/calendar")))
        .and(query_param("from", "2026-11-01T00:00:00Z"))
        .and(query_param_is_missing("to"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "scenarioId": SCENARIO, "from": "2026-11-01T00:00:00+00:00", "to": "2026-11-15T00:00:00+00:00",
            "cadence": {"declaredSeconds": 300, "scheduledSeconds": 600, "onDemand": true, "replayAcceleration": false},
            "observationHours": {"start": "07:00", "end": "22:00", "overnight": false},
            "pausedSince": null,
            "mates": [{"id": SOCIAL, "name": "Login (mobile)", "kind": "transaction"}],
            "windows": [
                {"interruptionId": GROUP, "kind": "weekly", "exclusion": true,
                 "start": "2026-11-02T02:00:00+01:00", "end": "2026-11-02T04:00:00+01:00", "comment": "backup"}
            ],
            "unavailable": false
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "calendar",
            SCENARIO,
            "--from",
            "2026-11-01T00:00:00Z",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Cadence:   every 5m (scheduled every 10m), on demand\n",
        ))
        .stdout(predicate::str::contains("Hours:     07:00 to 22:00\n"))
        .stdout(predicate::str::contains(format!(
            "Shares calendar with: Login (mobile) ({SOCIAL})\n"
        )))
        .stdout(predicate::str::contains(format!(
            "2026-11-02 02:00\t2026-11-02 04:00\tno observation\tweekly\t{GROUP}\tbackup\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn download_doc_writes_a_file_and_streams_to_stdout() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/documentation.pdf")))
        .and(query_param("technical", "true"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/pdf")
                .set_body_bytes(b"%PDF-1.7 fake".to_vec()),
        )
        .expect(2)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("doc.pdf");

    cmd(&server, &dir)
        .args([
            "scenarios",
            "download-doc",
            SCENARIO,
            "--technical",
            "-o",
            target.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Saved").and(predicate::str::contains("13 bytes")));
    assert_eq!(std::fs::read(&target).unwrap(), b"%PDF-1.7 fake");

    cmd(&server, &dir)
        .args(["scenarios", "doc", SCENARIO, "--technical", "-o", "-"])
        .assert()
        .success()
        .stdout(predicate::eq("%PDF-1.7 fake"));
}

#[tokio::test(flavor = "multi_thread")]
async fn download_doc_without_documentation_leaves_no_file() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/documentation.pdf")))
        .respond_with(problem("no-documentation", 404))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("doc.pdf");

    cmd(&server, &dir)
        .args([
            "scenarios",
            "download-doc",
            SCENARIO,
            "-o",
            target.to_str().unwrap(),
        ])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("no-documentation"));
    assert!(!target.exists());
    assert!(!dir.path().join("doc.pdf.part").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn groups_list_and_get() {
    let server = MockServer::start().await;
    let group = serde_json::json!({
        "id": GROUP, "name": "Web",
        "scenarios": [
            {"id": SCENARIO, "name": "Login", "kind": "transaction"},
            {"id": SOCIAL, "name": "Brand on X", "kind": "socialMonitoring"}
        ],
        "hiddenCount": 1
    });
    Mock::given(method("GET"))
        .and(path("/v2/scenario-groups"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [group], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenario-groups/{GROUP}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(group.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "groups", "list"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ID\tNAME\tSCENARIOS\tHIDDEN\n"))
        .stdout(predicate::str::contains(format!("{GROUP}\tWeb\t2\t1\n")));

    cmd(&server, &dir)
        .args(["scenarios", "groups", "get", GROUP])
        .assert()
        .success()
        .stdout(predicate::str::contains("Name:      Web\n"))
        .stdout(predicate::str::contains(
            "Scenarios: 2 visible, 1 hidden by the access profile\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{SOCIAL}\tBrand on X\tsocial\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_is_an_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/scenarios"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "list"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `scenarios:read`",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn jq_filters_the_json_output() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/scenarios"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [item(SCENARIO, "Login", "transaction"), item(SOCIAL, "Brand on X", "socialMonitoring")],
            "next": null, "total": 2
        })))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "list", "--jq", ".[].name"])
        .assert()
        .success()
        // jq ends lines with CRLF on Windows.
        .stdout(predicate::function(|s: &str| {
            s.replace("\r\n", "\n") == "\"Login\"\n\"Brand on X\"\n"
        }));

    cmd(&server, &dir)
        .args(["scenarios", "list", "--jq", ".[] | .nope("])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("jq exited with"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deprecated_route_warns_once_per_run() {
    let server = MockServer::start().await;
    let deprecated = |body: serde_json::Value| {
        ResponseTemplate::new(200)
            .insert_header("Deprecation", "@1735689600")
            .insert_header("Sunset", "Wed, 01 Jul 2026 00:00:00 GMT")
            .insert_header("Link", "</docs/changelog>; rel=\"deprecation\"")
            .set_body_json(body)
    };
    Mock::given(method("GET"))
        .and(path("/v2/scenarios"))
        .and(query_param_is_missing("cursor"))
        .respond_with(deprecated(serde_json::json!({
            "items": [item(SCENARIO, "Login", "transaction")], "next": "c2", "total": 2
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/scenarios"))
        .and(query_param("cursor", "c2"))
        .respond_with(deprecated(serde_json::json!({
            "items": [item(SOCIAL, "Brand on X", "socialMonitoring")], "next": null, "total": 2
        })))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let out = cmd(&server, &dir)
        .args(["scenarios", "list", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Brand on X"));
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let expected = "warning: /v2/scenarios is deprecated since 2025-01-01 and will be removed on \
                    Wed, 01 Jul 2026 00:00:00 GMT; see /docs/changelog\n";
    assert_eq!(stderr, expected);
}
