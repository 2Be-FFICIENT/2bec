//! `/v2/runs` and `/v2/run-requests` types.
//!
//! Status and other codes are camelCase strings, never enums, and the API
//! writes `null` for every absent value, so nullable fields are `Option`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Run statuses the list filter accepts, as the API spells them.
pub const STATUSES: [&str; 5] = ["ok", "failed", "outOfScope", "neutralized", "notObserved"];

/// Named periods the list filter accepts.
pub const PERIODS: [&str; 9] = [
    "last24h",
    "last7d",
    "last30d",
    "today",
    "yesterday",
    "thisWeek",
    "lastWeek",
    "thisMonth",
    "lastMonth",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioRef {
    pub id: String,
    pub name: String,
    /// `transaction` or `socialMonitoring`.
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRef {
    pub id: Option<String>,
    pub name: Option<String>,
    pub label: Option<String>,
    pub order: Option<i64>,
}

/// One row of `GET /v2/runs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunListItem {
    pub id: String,
    pub scenario: ScenarioRef,
    pub started_at: String,
    /// `ok`, `failed`, `outOfScope`, `neutralized`, `notObserved`.
    pub status: String,
    pub status_code: i64,
    pub performance: Option<String>,
    pub duration_ms: Option<i64>,
    pub failed_step: Option<StepRef>,
    pub replay: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    pub not_observed_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStep {
    pub id: String,
    pub name: String,
    pub label: String,
    pub order: i64,
    pub measured: bool,
    pub duration_ms: Option<i64>,
    pub server_ms: Option<i64>,
    pub bytes: Option<i64>,
    pub performance: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFailure {
    pub failed_step: Option<StepRef>,
    pub failed_action: Option<String>,
    pub failed_action_id: Option<String>,
    pub failed_step_started_at: Option<String>,
    pub failed_step_duration_ms: Option<i64>,
    #[serde(default)]
    pub in_planned_interruption: bool,
    pub description: Option<String>,
    /// 0..=5.
    pub confidence: Option<i64>,
}

/// `GET /v2/runs/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub scenario: ScenarioRef,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub status_code: i64,
    pub performance: Option<String>,
    pub duration_ms: Option<i64>,
    pub replay: bool,
    #[serde(default)]
    pub voice_call: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    pub not_observed_reason: Option<String>,
    #[serde(default)]
    pub steps: Vec<RunStep>,
    pub failure: Option<RunFailure>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub comment_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunComment {
    pub id: String,
    pub author: String,
    pub at: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunVerdict {
    pub failed_step: Option<String>,
    pub failed_action: Option<String>,
    pub error_message: Option<String>,
    pub pattern_search: Option<String>,
    #[serde(default)]
    pub expected_pattern_count: i64,
    #[serde(default)]
    pub text_not_found: Vec<String>,
    #[serde(default)]
    pub web_service_errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyStep {
    pub name: String,
    pub label: String,
    pub order: i64,
    pub state: String,
    pub offset_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub measured: bool,
    pub action_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Opale {
    pub description: String,
    pub confidence: i64,
    pub confidence_reason: Option<String>,
    #[serde(default)]
    pub possible_causes: Vec<String>,
    #[serde(default)]
    pub tips: Vec<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub recycled: bool,
}

/// `GET /v2/runs/{id}/diagnosis`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub id: String,
    pub scenario: ScenarioRef,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    #[serde(default)]
    pub failure: RunFailure,
    #[serde(default)]
    pub verdict: RunVerdict,
    #[serde(default)]
    pub journey: Vec<JourneyStep>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub comments: Vec<RunComment>,
    pub opale: Option<Opale>,
    #[serde(default)]
    pub opale_available: bool,
}

/// One entry of `GET /v2/runs/{id}/artifacts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    /// `video`, `screenshot`, `fluxzy`, `har`, `pcap` or `wav`.
    pub kind: String,
    pub content_type: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTag {
    pub id: String,
    pub name: String,
}

/// Answer of the tag and untag routes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTags {
    pub run: String,
    pub scenario: ScenarioRef,
    #[serde(default)]
    pub tags: Vec<RunTag>,
}

/// A run request (immediate run) as listed, created or polled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    pub id: String,
    pub scenario: ScenarioRef,
    pub requested_at: String,
    pub requested_by: Option<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    pub comment: Option<String>,
    /// `accepted`, `refused` or `failed`.
    pub outcome: String,
    pub error: Option<String>,
    /// `sleeping`, `alreadyRunning`, `waitingForTarget`, `running`,
    /// `forbiddenView`, `done`, `unknown` or null.
    pub stage: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestVariable {
    pub name: String,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestQuota {
    pub limit: i64,
    pub window_minutes: i64,
    pub min_interval_minutes: i64,
    pub used_in_window: i64,
    pub remaining: i64,
    pub allowed: bool,
    pub next_allowed_at: Option<String>,
}

/// `GET /v2/scenarios/{id}/run-requests`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequestState {
    pub scenario: ScenarioRef,
    pub active: bool,
    pub job_active: bool,
    pub schedulable: bool,
    pub not_schedulable_reason: Option<String>,
    pub paused: bool,
    pub paused_since: Option<String>,
    pub in_off_period: bool,
    pub off_period_end: Option<String>,
    #[serde(default)]
    pub variables: Vec<RequestVariable>,
    pub quota: RequestQuota,
    #[serde(default)]
    pub recent: Vec<RunRequest>,
}

/// Body of `POST /v2/scenarios/{id}/run-requests`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequestWrite {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_item_with_nulls_parses() {
        let json = r#"{"id":"r1","scenario":{"id":"s1","name":"Login","kind":"transaction"},
            "startedAt":"2026-10-11T08:00:00+02:00","status":"failed","statusCode":100,
            "performance":null,"durationMs":1234,
            "failedStep":{"id":"st1","name":"login","label":"Login","order":2},
            "replay":false,"tags":["prod"],"notObservedReason":null}"#;
        let r: RunListItem = serde_json::from_str(json).unwrap();
        assert_eq!(r.status, "failed");
        assert_eq!(r.failed_step.unwrap().label.as_deref(), Some("Login"));
        assert_eq!(r.performance, None);
    }

    #[test]
    fn run_detail_parses() {
        let json = r#"{"id":"r1","scenario":{"id":"s1","name":"Login","kind":"transaction"},
            "startedAt":"2026-10-11T08:00:00+02:00","endedAt":null,"status":"ok","statusCode":0,
            "performance":"target","durationMs":10,"replay":false,"voiceCall":false,"tags":[],
            "notObservedReason":null,
            "steps":[{"id":"st1","name":"home","label":"Home","order":1,"measured":true,
                      "durationMs":5,"serverMs":2,"bytes":100,"performance":"target"}],
            "failure":null,"artifacts":["video"],"commentCount":0}"#;
        let r: Run = serde_json::from_str(json).unwrap();
        assert_eq!(r.steps.len(), 1);
        assert!(r.failure.is_none());
        assert_eq!(r.artifacts, ["video"]);
    }

    #[test]
    fn run_request_write_omits_empty_fields() {
        let w = RunRequestWrite::default();
        assert_eq!(serde_json::to_string(&w).unwrap(), "{}");
        let mut w = RunRequestWrite::default();
        w.variables.insert("USER".into(), "bob".into());
        w.comment = Some("smoke".into());
        assert_eq!(
            serde_json::to_string(&w).unwrap(),
            r#"{"variables":{"USER":"bob"},"comment":"smoke"}"#
        );
    }
}
