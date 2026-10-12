//! End-to-end tests of `2bec metrics` and the per-scenario figures against a
//! mock API. No network.

mod common;

use assert_cmd::Command;
use common::IsolatedEnv;
use predicates::prelude::*;
use tempfile::TempDir;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "2bef_live_abcdefghijklmnopqrstuvwxyz012345";
const SCENARIO: &str = "33333333-3333-4333-8333-333333333333";
const OTHER: &str = "44444444-4444-4444-8444-444444444444";
const RUN: &str = "55555555-5555-4555-8555-555555555555";

fn scenario_ref(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "name": name, "kind": "transaction"})
}

fn period(from: &str, to: &str) -> serde_json::Value {
    serde_json::json!({"from": format!("{from}T00:00:00+02:00"), "to": format!("{to}T00:00:00+02:00")})
}

fn availability() -> serde_json::Value {
    serde_json::json!({
        "period": period("2026-09-12", "2026-10-12"),
        "previousPeriod": period("2026-08-13", "2026-09-12"),
        "granularity": "week", "scenarioCount": 2, "observedCount": 2,
        "target": {"effective": 99.5, "mixed": false},
        "summary": {
            "rate": 99.12, "previousRate": 99.4,
            "errorBudget": {"allowedDownSeconds": 7800.0, "consumedSeconds": 3720.0, "remainingSeconds": 4080.0, "consumedPercent": 47.7},
            "upSeconds": 2_590_000.0, "downSeconds": 3720.0, "okRuns": 10234, "failedRuns": 45, "incidents": 7,
            "mtbfSeconds": 273_600.0, "mttrSeconds": 720.0, "longestIncidentSeconds": 3720.0,
            "bestDay": {"date": "2026-10-01", "rate": 100.0}, "worstDay": {"date": "2026-09-20", "rate": 97.1}
        },
        "series": [
            {"at": "2026-09-12T00:00:00+02:00", "end": "2026-09-19T00:00:00+02:00", "rate": 99.0, "upSeconds": 1.0,
             "downSeconds": 0.0, "okRuns": 1, "failedRuns": 0, "incidents": 0, "observedScenarios": 2, "plannedInterruption": false}
        ],
        "byScenario": [
            {"scenario": scenario_ref(OTHER, "Checkout"), "target": 99.5, "rate": 98.2, "previousRate": null,
             "upSeconds": 1.0, "downSeconds": 1.0, "okRuns": 5000, "failedRuns": 40, "incidents": 6,
             "mtbfSeconds": null, "mttrSeconds": 600.0, "longestIncidentSeconds": 3720.0, "error": false},
            {"scenario": scenario_ref(SCENARIO, "Login"), "target": 99.5, "rate": null, "previousRate": 99.9,
             "upSeconds": 1.0, "downSeconds": 0.0, "okRuns": 5234, "failedRuns": 5, "incidents": 1,
             "mtbfSeconds": 1.0, "mttrSeconds": 60.0, "longestIncidentSeconds": 60.0, "error": true}
        ],
        "weakestSteps": [
            {"step": "Pay", "failures": 30, "failureShare": 0.667, "downtimeSeconds": 3000.0,
             "scenarios": [{"scenario": scenario_ref(OTHER, "Checkout"), "stepOrder": 4, "failures": 30}]}
        ],
        "failureTypes": {"failures": 45, "untyped": 5, "typeCount": 1, "types": [
            {"actionId": "a1", "failures": 40, "failureShare": 0.889, "step": "Pay", "description": "Button not found",
             "first": "2026-09-13T10:00:00+02:00", "last": "2026-10-11T10:00:00+02:00",
             "latestRun": {"id": RUN, "startedAt": "2026-10-11T10:00:00+02:00"},
             "scenarios": [{"scenario": scenario_ref(OTHER, "Checkout"), "failures": 40}]}
        ]},
        "longestIncidents": [
            {"id": "i1", "scenario": scenario_ref(OTHER, "Checkout"), "start": "2026-09-20T08:00:00+02:00",
             "end": null, "ongoing": true, "durationSeconds": 3720.0, "failedStep": "Pay", "failedRuns": 12,
             "firstRun": {"id": RUN, "startedAt": "2026-09-20T08:00:00+02:00"},
             "lastRun": {"id": RUN, "startedAt": "2026-09-20T09:00:00+02:00"}}
        ]
    })
}

fn figures(calls: i64) -> serde_json::Value {
    serde_json::json!({
        "calls": calls, "measured": calls - 2, "noQuality": 1, "notCollected": 1,
        "averageMos": 4.12, "minMos": 2.3, "maxMos": 4.4,
        "averageErrorRatePercent": 0.25, "maxErrorRatePercent": 3.0,
        "averageJitterMs": 12.4, "maxJitterMs": 40.0,
        "ratings": {"excellent": 1, "good": 5, "fair": 1, "poor": 1, "bad": 0}
    })
}

fn problem(code: &str, status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/problem+json")
        .set_body_json(serde_json::json!({
            "type": format!("urn:pegas:problem:{code}"), "title": "T", "status": status,
            "detail": "D", "instance": "/v2/metrics", "traceId": "trace123"
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
async fn availability_sends_the_selection_and_prints_every_block() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/metrics/availability"))
        .and(query_param("scenario", SCENARIO))
        .and(query_param("period", "last30d"))
        .and(query_param("granularity", "week"))
        .and(query_param_is_missing("from"))
        .respond_with(ResponseTemplate::new(200).set_body_json(availability()))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "metrics",
            "availability",
            "--scenario",
            SCENARIO,
            "--period",
            "LAST30D",
            "--granularity",
            "Week",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Period:        2026-09-12 to 2026-10-12 (week buckets); previous 2026-08-13 to 2026-09-12",
        ))
        .stdout(predicate::str::contains("Availability:  99.12% (previous 99.40%)"))
        .stdout(predicate::str::contains(
            "Error budget:  allowed 2h10m, consumed 1h02m, remaining 1h08m (48% used)",
        ))
        .stdout(predicate::str::contains(
            "MTBF / MTTR:   3d4h / 12m00s; longest incident 1h02m",
        ))
        .stdout(predicate::str::contains("Best / worst:  2026-10-01 100.00% / 2026-09-20 97.10%"))
        .stdout(predicate::str::contains(format!(
            "{OTHER}\tCheckout\t99.50%\t98.20%\t-\t5000\t40\t6\t10m00s\t1h02m\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{SCENARIO}\tLogin\t99.50%\terror\t99.90%\t"
        )))
        .stdout(predicate::str::contains("Pay\t30\t67%\t50m00s\tCheckout (30)\n"))
        .stdout(predicate::str::contains(
            "Failures by cause (45 failures, 5 untyped, 1 causes):",
        ))
        .stdout(predicate::str::contains(format!(
            "40\t89%\tPay\tButton not found\t2026-10-11 10:00\t{RUN}\n"
        )))
        .stdout(predicate::str::contains(
            "i1\tCheckout\t2026-09-20 08:00\tongoing\t1h02m\tPay\t12\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn availability_rejects_bad_flags_locally() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["metrics", "availability", "--granularity", "hour"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--granularity `hour` is not one of",
        ));
    cmd(&server, &dir)
        .args(["metrics", "availability", "--scenario", "nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--scenario must be a GUID"));
    cmd(&server, &dir)
        .args([
            "metrics",
            "availability",
            "--from",
            "x",
            "--period",
            "today",
        ])
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn response_times_prints_the_fleet_table() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/metrics/response-times"))
        .and(query_param("tz", "UTC"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "period": period("2026-10-05", "2026-10-12"),
            "previousPeriod": period("2026-09-28", "2026-10-05"),
            "scenarioCount": 2, "measuredCount": 1,
            "summary": {"runs": 1234, "meanP85ToTargetRatio": 0.85, "previousMeanP85ToTargetRatio": null,
                "bands": {"target": 1, "client": 0, "insufficient": 0, "unacceptable": 0},
                "withoutThresholds": 0, "noData": 1},
            "series": [],
            "byScenario": [
                {"scenario": scenario_ref(SCENARIO, "Login"),
                 "thresholds": {"target": 3000.0, "client": 6000.0, "unacceptable": 12000.0},
                 "runs": 1234, "p50Ms": 2100.4, "p85Ms": 2550.0, "meanMs": 2200.0, "maxMs": 9000.0,
                 "previousRuns": 1200, "previousP85Ms": 2600.0, "previousMeanMs": 2250.0,
                 "dailyMeanMs": [{"at": "2026-10-05T00:00:00+02:00", "value": 2200.0}], "error": false},
                {"scenario": scenario_ref(OTHER, "Checkout"), "thresholds": null,
                 "runs": 0, "p50Ms": null, "p85Ms": null, "meanMs": null, "maxMs": null,
                 "previousRuns": 0, "previousP85Ms": null, "previousMeanMs": null, "dailyMeanMs": [], "error": true}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["metrics", "response-times", "--tz", "UTC"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "P85 / target:  0.85 (previous -); 1.00 is on target",
        ))
        .stdout(predicate::str::contains(
            "Bands:         1 on target, 0 within client, 0 insufficient, 0 unacceptable",
        ))
        .stdout(predicate::str::contains(format!(
            "{SCENARIO}\tLogin\t1234\t2100\t2550\t2200\t9000\t2600\t3000\t6000\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{OTHER}\tCheckout\terror\t-\t-\t-\t-\t-\t-\t-\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn response_times_caps_the_selection_locally() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();
    let mut args = vec!["metrics".to_string(), "response-times".to_string()];
    for i in 0..16 {
        args.push("--scenario".into());
        args.push(format!("{i:08x}-0000-4000-8000-000000000000"));
    }

    cmd(&server, &dir)
        .args(&args)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("at most 15 --scenario values"));
}

#[tokio::test(flavor = "multi_thread")]
async fn voice_quality_prints_totals_and_rows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/metrics/voice-quality"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "period": period("2026-10-05", "2026-10-12"), "scenarioCount": 1,
            "totals": figures(10),
            "byScenario": [{"scenario": scenario_ref(SCENARIO, "IVR"), "figures": figures(10), "error": false}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["metrics", "voice-quality"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Calls:         10 (8 measured, 1 without quality, 1 not collected)",
        ))
        .stdout(predicate::str::contains(
            "MOS:           avg 4.1 (min 2.3, max 4.4)",
        ))
        .stdout(predicate::str::contains(
            "Ratings:       1 excellent, 5 good, 1 fair, 1 poor, 0 bad",
        ))
        .stdout(predicate::str::contains(format!(
            "{SCENARIO}\tIVR\t10\t8\t4.1\t2.3\t0.2\t12.4\t1\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn social_sends_interval_and_events_and_prints_posts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/metrics/social"))
        .and(query_param("interval", "15"))
        .and(query_param("include", "events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "period": period("2026-10-11", "2026-10-12"), "intervalMinutes": 15,
            "asOf": "2026-10-12T09:30:00+02:00", "scenarioCount": 1, "errorCount": 0,
            "totals": {"events": 40, "incidents": 4, "incidentRate": 10.0, "previousEvents": 30, "previousIncidents": 1,
                "worstGrade": "client", "timeInGrade": {"target": 90, "client": 6, "insufficient": 0, "unacceptable": 0},
                "peakBucketStart": "2026-10-11T18:00:00+02:00", "peakBucketIncidents": 3,
                "busiestDay": "2026-10-11", "peakHour": 18},
            "byScenario": [{"scenario": {"id": SCENARIO, "name": "Bank on X", "kind": "socialMonitoring"},
                "source": "twitter", "state": "target", "enabled": true,
                "thresholdsPerHour": {"target": 2, "client": 5, "unacceptable": 10},
                "events": 40, "incidents": 4, "incidentRate": 10.0, "previousEvents": 30, "previousIncidents": 1,
                "worstGrade": "client", "lastIncident": "2026-10-11T18:10:00+02:00",
                "lastEvent": "2026-10-11T23:00:00+02:00", "error": null}],
            "series": [],
            "days": [{"date": "2026-10-11", "events": 40, "incidents": 4, "grade": "client"}],
            "events": [{"id": 1, "scenario": {"id": SCENARIO, "name": "Bank on X", "kind": "socialMonitoring"},
                "at": "2026-10-11T18:10:00+02:00", "text": "App down\nagain", "link": "https://x.com/1",
                "incident": true, "explanation": "outage"}],
            "eventsTotal": 40, "eventsTruncated": true
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["metrics", "social", "--interval", "15", "--events"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Posts:         40 (4 incidents, 10.00%); previous 30 / 1",
        ))
        .stdout(predicate::str::contains(
            "Peak bucket:   2026-10-11 18:00 (3 incidents)",
        ))
        .stdout(predicate::str::contains(
            "Busiest:       day 2026-10-11, hour 18h",
        ))
        .stdout(predicate::str::contains(format!(
            "{SCENARIO}\tBank on X\ttwitter\ttarget\t40\t4\t10.00%\tclient\t2026-10-11 18:10\n"
        )))
        .stdout(predicate::str::contains(
            "Posts, newest first (1 of 40, truncated):",
        ))
        .stdout(predicate::str::contains(
            "2026-10-11 18:10\tBank on X\tyes\tApp down again\thttps://x.com/1\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn social_rejects_a_bad_interval_locally() {
    let server = MockServer::start().await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["metrics", "social", "--interval", "20"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--interval must be one of 10, 15, 30, 60, 120, 240",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn social_analysis_prints_days_and_keywords() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/metrics/social/analysis"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "period": period("2026-10-05", "2026-10-12"), "scenarioCount": 1, "errorCount": 1,
            "keywords": [{"code": "login", "count": 3}, {"code": "slow", "count": 1}],
            "days": [{"date": "2026-10-11", "scenario": {"id": SCENARIO, "name": "Bank on X", "kind": "socialMonitoring"},
                "events": 40, "incidents": 4, "incidentRate": 10.0,
                "onlineIssues": ["Login page times out"], "otherIssues": [], "keywords": ["login", "timeout"]}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["metrics", "analysis"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Scenarios:     1 (1 not answered)"))
        .stdout(predicate::str::contains("Keywords:      login (3), slow (1)"))
        .stdout(predicate::str::contains(
            "2026-10-11  Bank on X  40 posts, 4 incidents (10.00%)\n  online:   Login page times out\n  keywords: login, timeout\n",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn scenario_synthesis_prints_indices_and_trends() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/synthesis")))
        .and(query_param("trend", "week"))
        .and(query_param("period", "lastMonth"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "scenario": scenario_ref(SCENARIO, "Login"), "from": "2026-09-01", "to": "2026-09-30", "trend": "week",
            "indices": {
                "synthesis": {"client": 8.53, "target": 7.0, "valid": true},
                "duration": {"client": 9.0, "target": 8.0, "valid": true},
                "interactivity": {"client": 9.0, "target": 8.0, "valid": true},
                "completion": {"client": 9.0, "target": 8.0, "valid": true},
                "regularity": {"client": 6.0, "target": 5.0, "valid": false},
                "restoration": {"client": 9.0, "target": 8.0, "valid": true}
            },
            "summary": {"runs": 8000, "failedRuns": 20, "availabilityRate": 99.75, "incidents": 3,
                "longestIncidentSeconds": 900.0, "bands": {"target": 7000, "client": 900, "insufficient": 80, "unacceptable": 0}},
            "trends": [{"window": "previousWeek", "from": "2026-08-25", "to": "2026-08-31", "clientIndex": 8.1, "availabilityRate": 99.5}],
            "hourly": null,
            "daily": [{"date": "2026-09-01", "clientIndex": 8.4, "valid": true, "runs": 280, "failures": 1}],
            "percentiles": {"p50Ms": 2100.0, "p85Ms": 2600.0, "runs": 7980},
            "reliability": {"mttrSeconds": 420.0, "incidents": 3}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args([
            "scenarios",
            "synthesis",
            SCENARIO,
            "--period",
            "lastmonth",
            "--trend",
            "WEEK",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Synthesis:     8.5 / 10 (target reading 7.0)",
        ))
        .stdout(predicate::str::contains(
            "  Regularity:    6.0 / 10 (target reading 5.0) [not enough runs]",
        ))
        .stdout(predicate::str::contains(
            "Runs:          8000 (20 failed), availability 99.75%",
        ))
        .stdout(predicate::str::contains(
            "Response:      P50 2100 ms, P85 2600 ms over 7980 runs",
        ))
        .stdout(predicate::str::contains(
            "Restore:       MTTR 7m00s over 3 incident(s)",
        ))
        .stdout(predicate::str::contains(
            "previousWeek\t2026-08-25\t2026-08-31\t8.10\t99.50%\n",
        ))
        .stdout(predicate::str::contains("2026-09-01\t8.40\tyes\t280\t1\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scenario_synthesis_of_a_social_scenario_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/synthesis")))
        .respond_with(problem("social-scenario", 409))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "synthesis", SCENARIO])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("social-scenario"));
}

#[tokio::test(flavor = "multi_thread")]
async fn scenario_response_times_prints_steps_and_slowest_runs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/response-times")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "scenario": scenario_ref(SCENARIO, "Login"),
            "period": period("2026-10-05", "2026-10-12"), "previousPeriod": period("2026-09-28", "2026-10-05"),
            "lastDayPartial": true,
            "thresholds": {"target": 3000.0, "client": 6000.0, "unacceptable": 12000.0}, "kiloBytesPerSecond": 512,
            "successfulRuns": 1234,
            "total": {"count": 1234, "p50Ms": 2100.0, "p85Ms": 2550.0, "meanMs": 2200.0, "maxMs": 9000.0},
            "previous": null,
            "daily": [], "previousDaily": null, "hourly": [],
            "bands": {"target": 1000, "client": 200, "insufficient": 30, "unacceptable": 4, "unrated": 0},
            "steps": [
                {"name": "Step 1 - Home", "label": "Home", "order": 1, "current": true, "measured": true,
                 "thresholds": {"target": 800.0, "client": 1500.0, "unacceptable": 3000.0},
                 "total": {"count": 1234, "p50Ms": 600.0, "p85Ms": 700.0, "meanMs": 620.0, "maxMs": 2000.0},
                 "previous": {"count": 1200, "p50Ms": 610.0, "p85Ms": 710.0, "meanMs": 630.0, "maxMs": 2100.0},
                 "shareOfTotal": 0.28, "serverTimeShare": 0.6, "meanDownloadedBytes": 120000.0, "daily": null},
                {"name": "Old step", "label": null, "order": 9, "current": false, "measured": true,
                 "thresholds": null, "total": null, "previous": null, "shareOfTotal": 0.0,
                 "serverTimeShare": null, "meanDownloadedBytes": null, "daily": null}
            ],
            "slowestRuns": [{"id": RUN, "startedAt": "2026-10-11T10:00:00+02:00", "totalMs": 9000.0,
                "slowestStep": "Home", "slowestStepMs": 2000.0, "band": "insufficient"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "response-times", SCENARIO])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Period:        2026-10-05 to 2026-10-12 (today still moving); previous 2026-09-28 to 2026-10-05",
        ))
        .stdout(predicate::str::contains(
            "Thresholds:    target 3.0s, client 6.0s, unacceptable 12.0s at 512 kB/s",
        ))
        .stdout(predicate::str::contains(
            "Total:         P50 2100 ms, P85 2550 ms, mean 2200 ms, max 9000 ms over 1234 runs",
        ))
        .stdout(predicate::str::contains("Previous:      -"))
        .stdout(predicate::str::contains(
            "1\tHome\tyes\t600\t700\t620\t630\t28%\t60%\t800\n",
        ))
        .stdout(predicate::str::contains("9\tOld step\tno\t-\t-\t-\t-\t0%\t-\t-\n"))
        .stdout(predicate::str::contains(format!(
            "{RUN}\t2026-10-11 10:00\t9000\tHome\t2000\tinsufficient\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn scenario_voice_quality_prints_days_and_calls() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/scenarios/{SCENARIO}/voice-quality")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "scenario": scenario_ref(SCENARIO, "IVR"), "period": period("2026-10-05", "2026-10-12"),
            "summary": figures(10),
            "daily": [{"date": "2026-10-11", "calls": 10, "measured": 8, "averageMos": 4.1, "minMos": 2.3,
                "averageErrorRatePercent": 0.25, "averageJitterMs": 12.4,
                "ratings": {"excellent": 1, "good": 5, "fair": 1, "poor": 1, "bad": 0}}],
            "endReasons": [{"code": "hangup", "count": 9}, {"code": "timeout", "count": 1}],
            "calls": [
                {"id": RUN, "startedAt": "2026-10-11T10:00:00+02:00", "status": "ok", "measurement": "measured",
                 "mos": 4.3, "rating": "good", "errorRatePercent": 0.1, "lossPercent": 0.0, "jitterAvgMs": 10.0,
                 "jitterMaxMs": 20.0, "oneWayDelayMs": 80, "roundTripMs": 160, "outboundMos": 4.2,
                 "outboundErrorRatePercent": 0.1, "codec": "PCMA", "endReason": "hangup", "talkDurationMs": 30000,
                 "postDialDelayMs": 1200},
                {"id": OTHER, "startedAt": "2026-10-11T11:00:00+02:00", "status": "failed", "measurement": "notCollected",
                 "mos": null, "rating": null, "errorRatePercent": null, "lossPercent": null, "jitterAvgMs": null,
                 "jitterMaxMs": null, "oneWayDelayMs": null, "roundTripMs": null, "outboundMos": null,
                 "outboundErrorRatePercent": null, "codec": null, "endReason": "timeout", "talkDurationMs": null,
                 "postDialDelayMs": null}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["scenarios", "voice-quality", SCENARIO])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "End reasons:   hangup (9), timeout (1)",
        ))
        .stdout(predicate::str::contains(
            "2026-10-11\t10\t8\t4.1\t2.3\t0.2\t12.4\t1\n",
        ))
        .stdout(predicate::str::contains(format!(
            "{RUN}\t2026-10-11 10:00\tok\t4.3\tgood\t0.1\t10.0\tPCMA\thangup\n"
        )))
        .stdout(predicate::str::contains(format!(
            "{OTHER}\t2026-10-11 11:00\tfailed\tnotCollected\t-\t-\t-\t-\ttimeout\n"
        )));
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_missing_names_metrics_read() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/metrics/social/analysis"))
        .respond_with(problem("scope-missing", 403))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    cmd(&server, &dir)
        .args(["metrics", "social-analysis"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "credential lacks the scope `metrics:read`",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn json_passes_the_body_through() {
    let server = MockServer::start().await;
    let body = availability();
    Mock::given(method("GET"))
        .and(path("/v2/metrics/availability"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();

    let out = cmd(&server, &dir)
        .args(["metrics", "availability", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let printed: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(printed, body);
}
