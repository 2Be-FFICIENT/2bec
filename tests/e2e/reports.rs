//! End-to-end tests of `2bec reports` and `2bec metrics pdf` against a mock
//! API. No network.

use crate::common::IsolatedEnv;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{
    body_json, header, header_exists, method, path, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const REPORT: &str = "11111111-1111-4111-8111-111111111111";
const SCHEDULE: &str = "22222222-2222-4222-8222-222222222222";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const OTHER: &str = "44444444-4444-4444-8444-444444444444";

fn scenario_ref(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "name": name, "kind": "transaction"})
}

fn grouped() -> serde_json::Value {
    serde_json::json!({
        "id": REPORT, "kind": "grouped", "name": "Monthly SLA", "description": null,
        "visibility": "client", "sections": ["summary", "availabilityTrend"],
        "trendPeriods": 12, "comparisonPeriods": 3, "locale": "fr",
        "scenarios": [
            {"scenario": scenario_ref(SCENARIO, "Login"), "position": 1, "displayName": "Connexion", "active": true},
            {"scenario": scenario_ref(OTHER, "Pay"), "position": 2, "displayName": null, "active": false}
        ],
        "groups": [{"id": "g1", "name": "Core", "position": 1, "aggregation": "pooledTime", "scenarioIds": [SCENARIO, OTHER]}],
        "ownerName": null, "createdAt": "2026-01-01T00:00:00+00:00", "updatedAt": null, "apiOwned": true
    })
}

fn schedule() -> serde_json::Value {
    serde_json::json!({
        "id": SCHEDULE, "reportType": "availability-dashboard",
        "scenarios": [scenario_ref(SCENARIO, "Login"), scenario_ref(OTHER, "Pay")],
        "stat": null, "reportId": null, "reportName": null, "frequency": "weekly", "locale": "en",
        "enabled": true, "to": ["ops@example.com"], "cc": [], "lastSentAt": "2026-10-06T07:00:00+02:00",
        "planningKinds": ["plannedInterruption"]
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/reports", "traceId": "trace123"
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
async fn list_sends_kind_and_prints_rows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/reports"))
        .and(query_param("kind", "grouped"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [{"id": REPORT, "kind": "grouped", "name": "Monthly SLA", "description": null,
                "visibility": "client", "ownerName": "Alice", "updatedAt": "2026-10-01T10:00:00+02:00",
                "scenarioCount": 2, "groups": ["Core"], "apiOwned": false}],
            "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["reports", "list", "--kind", "Grouped"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{REPORT}\tgrouped\tMonthly SLA\tclient\t2\tCore\tAlice\t2026-10-01 10:00\t\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_prints_a_grouped_definition_with_etag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/reports/{REPORT}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"r7\"")
                .set_body_json(grouped()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["reports", "get", REPORT])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Kind:         grouped (availability and failures)",
        ))
        .stdout(predicate::str::contains(
            "Visibility:   client (created by the API)",
        ))
        .stdout(predicate::str::contains(
            "Periods:      12 in the trend, 3 compared",
        ))
        .stdout(predicate::str::contains("ETag:         \"r7\""))
        .stdout(predicate::str::contains(format!(
            "1\t{SCENARIO}\tLogin\tConnexion\tyes\n"
        )))
        .stdout(predicate::str::contains("1\tg1\tCore\tpooledTime\t2\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_prints_a_comparative_definition() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/reports/{REPORT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": REPORT, "kind": "comparative", "name": "Login A vs B", "description": "two builds",
            "visibility": "private", "stat": "p85", "ownerName": "Bob",
            "createdAt": "2026-01-01T00:00:00+00:00", "updatedAt": "2026-02-01T00:00:00+00:00",
            "scenarios": [{"scenario": scenario_ref(SCENARIO, "Login"), "os": "Windows", "browser": "Chrome", "stepIds": ["s1", "s2"]}],
            "pairs": [{"a": "s1", "b": null, "label": "Home"}], "apiOwned": false
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["reports", "get", REPORT])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Kind:         comparative (scenarios side by side)",
        ))
        .stdout(predicate::str::contains(
            "Visibility:   private (owner Bob)",
        ))
        .stdout(predicate::str::contains("Statistic:    p85"))
        .stdout(predicate::str::contains(format!(
            "{SCENARIO}\tLogin\tWindows / Chrome\t2\n"
        )))
        .stdout(predicate::str::contains("s1\t-\tHome\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn data_of_a_grouped_report_sends_window_and_prints_the_summary() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/reports/{REPORT}/data")))
        .and(query_param("window", "week"))
        .and(query_param("date", "2026-10-05"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": REPORT, "name": "Monthly SLA", "generatedAt": "2026-10-12T10:00:00+02:00",
            "asOf": "2026-10-12T09:55:00+02:00", "inProgress": true,
            "period": {"kind": "week", "start": "2026-10-05", "end": "2026-10-11"},
            "periods": [], "comparisonPeriods": [], "sections": ["summary"],
            "diagnostics": [{"code": "noData", "scenarioId": null, "message": "Pay has no run"}],
            "perimeters": [
                {"key": "r", "kind": "report", "name": "Monthly SLA", "position": 0, "groupId": null,
                 "scenarios": [], "summary": [
                    {"period": {"kind": "week", "start": "2026-10-05", "end": "2026-10-11"}, "availabilityPercent": 99.12, "deltaPointsVsCurrent": null, "failures": 4, "runs": 1200, "complete": false},
                    {"period": {"kind": "week", "start": "2026-09-28", "end": "2026-10-04"}, "availabilityPercent": 99.8, "deltaPointsVsCurrent": 0.68, "failures": 1, "runs": 1400, "complete": true}
                 ], "trend": null, "failuresByStep": null, "heatmap": null, "calendar": null, "diagnostics": []},
                {"key": "s1", "kind": "scenario", "name": "Login", "position": 1, "groupId": null,
                 "scenarios": [scenario_ref(SCENARIO, "Login")], "summary": null, "trend": null,
                 "failuresByStep": null, "heatmap": null, "calendar": null, "diagnostics": []}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "reports",
            "data",
            REPORT,
            "--window",
            "week",
            "--date",
            "2026-10-05",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Period:       week 2026-10-05 to 2026-10-11 (in progress)",
        ))
        .stdout(predicate::str::contains(
            "KIND\tPERIMETER\tFROM 2026-10-05\tFROM 2026-09-28\tFAILURES\tRUNS\n",
        ))
        .stdout(predicate::str::contains(
            "report\tMonthly SLA\t99.12%\t99.80%\t4\t1200\n",
        ))
        .stdout(predicate::str::contains("scenario\tLogin\t-\t-\t-\t-\n"))
        .stderr(predicate::str::contains("warning: noData Pay has no run"));

    cmd(&server, &dir)
        .args([
            "reports", "data", REPORT, "--window", "week", "--period", "today",
        ])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn data_of_a_comparative_report_sends_period_and_stat() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/reports/{REPORT}/data")))
        .and(query_param("period", "last30d"))
        .and(query_param("stat", "p50"))
        .and(query_param_is_missing("window"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": REPORT, "name": "Login A vs B", "stat": "p50",
            "period": {"from": "2026-09-12T00:00:00+02:00", "to": "2026-10-12T00:00:00+02:00"},
            "scenarios": [{"scenario": scenario_ref(SCENARIO, "Login"), "availability": 99.5, "runs": 800, "failures": 4,
                "selectedFailures": 2, "selectedAvailability": 99.75, "partialJourneyMeanMs": 1234.4,
                "daily": [], "steps": [{"id": "s1", "name": "Step 1", "label": "Home", "order": 1, "current": true, "measured": true,
                    "thresholds": {"target": 800.0, "client": 1500.0, "unacceptable": 3000.0}, "failures": 2, "availability": 99.75,
                    "stats": {"count": 796, "p50Ms": 600.0, "p85Ms": 700.0, "meanMs": 620.0, "maxMs": 2000.0}, "dailyMeanMs": []}],
                "error": false}],
            "pairs": []
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "reports", "data", REPORT, "--period", "LAST30D", "--stat", "P50",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Period:       2026-09-12 to 2026-10-12",
        ))
        .stdout(predicate::str::contains(
            "  Runs:          800 (4 failed), availability 99.50%",
        ))
        .stdout(predicate::str::contains(
            "  Paired steps:  2 failed, availability 99.75%, journey mean 1234 ms",
        ))
        .stdout(predicate::str::contains(
            "1\tHome\tyes\t2\t99.75%\t600\t700\t620\t800\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn pdf_downloads_with_lang_and_planning() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/reports/{REPORT}/pdf")))
        .and(query_param("window", "month"))
        .and(query_param("lang", "en"))
        .and(query_param(
            "planningKinds",
            "plannedInterruption,scriptVersion",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/pdf")
                .set_body_bytes(b"%PDF-report".to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("out.pdf");

    cmd(&server, &dir)
        .args([
            "reports",
            "pdf",
            REPORT,
            "--window",
            "month",
            "--lang",
            "EN",
            "--planning",
            "plannedInterruption",
            "--planning",
            "scriptVersion",
            "-o",
            target.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("11 bytes"));
    assert_eq!(std::fs::read(&target).unwrap(), b"%PDF-report");
}

#[tokio::test(flavor = "multi_thread")]
async fn create_posts_the_body_and_edit_patches_with_if_match() {
    let server = MockServer::start().await;
    let body = serde_json::json!({"kind": "grouped", "name": "Monthly SLA", "scenarios": [{"scenario": SCENARIO}]});
    Mock::given(method("POST"))
        .and(path("/v2/reports"))
        .and(header_exists("idempotency-key"))
        .and(body_json(body.clone()))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"r1\"")
                .set_body_json(grouped()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/reports/{REPORT}")))
        .and(header("content-type", "application/merge-patch+json"))
        .and(header("if-match", "\"r1\""))
        .and(body_json(serde_json::json!({"name": "SLA"})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"r2\"")
                .set_body_json(grouped()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["reports", "create", &body.to_string()])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Created report Monthly SLA ({REPORT})"
        )))
        .stdout(predicate::str::contains("ETag:         \"r1\""));
    cmd(&server, &dir)
        .args([
            "reports",
            "edit",
            REPORT,
            r#"{"name": "SLA"}"#,
            "--if-match",
            "\"r1\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETag:         \"r2\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_with_purge_and_not_own_report() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/reports/{REPORT}")))
        .and(query_param("purge", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/reports/{OTHER}")))
        .respond_with(problem("not-own-report", 403))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["reports", "delete", REPORT, "--purge"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Purged report {REPORT}")));
    cmd(&server, &dir)
        .args(["reports", "rm", OTHER])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("not-own-report"));
}

#[tokio::test(flavor = "multi_thread")]
async fn schedules_list_and_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/report-schedules"))
        .and(query_param("reportType", "availability-dashboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [schedule()], "next": null, "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/report-schedules/{SCHEDULE}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"s1\"")
                .set_body_json(schedule()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["reports", "schedules", "list", "--type", "availability-dashboard"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "{SCHEDULE}\tavailability-dashboard\tLogin +1\tweekly\tyes\tops@example.com\t2026-10-06 07:00\n"
        )));
    cmd(&server, &dir)
        .args(["reports", "schedules", "get", SCHEDULE])
        .assert()
        .success()
        .stdout(predicate::str::contains("Frequency:    weekly (enabled)"))
        .stdout(predicate::str::contains(format!(
            "Scenarios:    Login ({SCENARIO}), Pay ({OTHER})"
        )))
        .stdout(predicate::str::contains(
            "Planning:     plannedInterruption",
        ))
        .stdout(predicate::str::contains("ETag:         \"s1\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn schedules_create_builds_the_body_from_flags() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/report-schedules"))
        .and(header_exists("idempotency-key"))
        .and(body_json(serde_json::json!({
            "reportType": "availability-dashboard", "scenarios": [SCENARIO, OTHER],
            "frequency": "weekly", "locale": "fr", "enabled": false,
            "to": ["ops@example.com"], "cc": ["boss@example.com"], "planningKinds": ["plannedInterruption"]
        })))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("etag", "\"s1\"")
                .set_body_json(schedule()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "reports",
            "schedules",
            "create",
            "--type",
            "Availability-Dashboard",
            "--scenario",
            SCENARIO,
            "--scenario",
            OTHER,
            "--scenario",
            SCENARIO,
            "--frequency",
            "WEEKLY",
            "--locale",
            "fr",
            "--to",
            "ops@example.com",
            "--cc",
            "boss@example.com",
            "--planning",
            "plannedInterruption",
            "--disabled",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Created schedule {SCHEDULE}"
        )));

    cmd(&server, &dir)
        .args([
            "reports",
            "schedules",
            "create",
            "--type",
            "comparative",
            "--frequency",
            "daily",
            "--to",
            "nope",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--to `nope` is not an e-mail address",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn schedules_edit_sends_only_the_flags_given() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v2/report-schedules/{SCHEDULE}")))
        .and(header("if-match", "\"s1\""))
        .and(body_json(serde_json::json!({"enabled": false, "cc": [], "to": ["a@example.com", "b@example.com"]})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"s2\"")
                .set_body_json(schedule()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "reports",
            "schedules",
            "edit",
            SCHEDULE,
            "--enabled",
            "false",
            "--cc",
            "",
            "--to",
            "a@example.com",
            "--to",
            "b@example.com",
            "--if-match",
            "\"s1\"",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETag:         \"s2\""));
    cmd(&server, &dir)
        .args(["reports", "schedules", "edit", SCHEDULE])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nothing to change"));
}

#[tokio::test(flavor = "multi_thread")]
async fn schedules_send_reports_the_outcome() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/report-schedules/{SCHEDULE}:send")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"sent": false, "error": "no run in the period"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/report-schedules/{SCHEDULE}")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["reports", "schedules", "send", SCHEDULE])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "the report could not go out: no run in the period",
        ));
    cmd(&server, &dir)
        .args(["reports", "schedules", "rm", SCHEDULE])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Deleted schedule {SCHEDULE}"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn deliveries_filter_and_print() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/report-deliveries"))
        .and(query_param("period", "lastMonth"))
        .and(query_param("status", "failed"))
        .and(query_param("schedule", SCHEDULE))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [{"scheduleId": SCHEDULE, "reportType": "availability-dashboard", "label": "Login +1",
                "scenarios": [], "reportId": null, "frequency": "weekly", "scheduleEnabled": true, "createdBy": "ops",
                "status": "failed", "periodStart": "2026-09-28", "periodEnd": "2026-10-04", "subject": null,
                "to": ["ops@example.com"], "cc": [], "error": "SMTP timeout", "sentAt": null,
                "createdAt": "2026-10-06T07:00:00+02:00"}],
            "next": null, "total": 3
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "reports", "deliveries", "--period", "lastmonth", "--status", "FAILED", "--schedule", SCHEDULE,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "2026-10-06 07:00\tfailed\tavailability-dashboard\tLogin +1\t2026-09-28 to 2026-10-04\tops@example.com\t{SCHEDULE}\tSMTP timeout\n"
        )))
        .stderr(predicate::str::contains("Showing 1 of 3 deliveries"));
}

#[tokio::test(flavor = "multi_thread")]
async fn metrics_pdf_sends_the_selection_and_stat() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/metrics/response-times/pdf"))
        .and(query_param("scenario", SCENARIO))
        .and(query_param("period", "last7d"))
        .and(query_param("stat", "p50"))
        .and(query_param("lang", "fr"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/pdf")
                .set_body_bytes(b"%PDF-rt".to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let out = cmd(&server, &dir)
        .args([
            "metrics",
            "pdf",
            "Response-Times",
            "--scenario",
            SCENARIO,
            "--period",
            "last7d",
            "--stat",
            "p50",
            "--lang",
            "fr",
            "-o",
            "-",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(out, b"%PDF-rt");

    cmd(&server, &dir)
        .args(["metrics", "pdf", "availability", "--stat", "p50"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--stat only applies to the response-times PDF",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_names_reports_read() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/report-deliveries"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["reports", "deliveries"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `reports:read`",
        ));
}
