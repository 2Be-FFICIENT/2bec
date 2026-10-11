//! `/v2/timeline`, `/v2/pauses`, `/v2/events` and the pause/resume actions.

use serde::{Deserialize, Serialize};

pub use super::metrics::Period;
pub use super::runs::ScenarioRef;

/// Kinds of timeline item.
pub const KINDS: [&str; 5] = [
    "monitoringEvent",
    "plannedInterruption",
    "scriptVersion",
    "incident",
    "monitoringPause",
];

/// Expected impacts of a monitoring event.
pub const IMPACTS: [&str; 4] = ["none", "performance", "availability", "both"];

/// A resume comment is this long at most.
pub const MAX_COMMENT: usize = 500;

/// `GET /v2/timeline`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    pub period: Period,
    pub items: Vec<TimelineItem>,
    /// Kinds whose source did not answer.
    pub unavailable: Vec<String>,
    /// Kinds left out by rule.
    pub skipped: Vec<String>,
    pub truncated: bool,
}

/// One dated thing: `kind` among [`KINDS`], `scope` scenario, client or
/// platform, `severity` info, expected, degraded or down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItem {
    pub key: String,
    pub kind: String,
    pub scope: String,
    pub scenarios: Vec<ScenarioRef>,
    pub hidden_scenarios: i64,
    pub start: String,
    /// Exclusive; `None` for an instant and for an ongoing pause.
    pub end: Option<String>,
    pub instant: bool,
    pub all_day: bool,
    pub ongoing: bool,
    pub title: String,
    pub summary: Option<String>,
    pub severity: String,
    pub category: Option<String>,
    pub expected_impact: Option<String>,
    pub labels: Vec<String>,
    pub actor: Option<String>,
    pub event_id: Option<String>,
    pub interruption_ids: Option<Vec<String>>,
    pub run_id: Option<String>,
    pub failed_runs: Option<i64>,
    pub comments: Option<i64>,
}

/// One row of `GET /v2/pauses`: a scenario and the mates sharing its script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pause {
    pub scenario: ScenarioRef,
    pub paused: bool,
    pub paused_since: Option<String>,
    pub last_resumed_at: Option<String>,
    pub shared_with: Vec<ScenarioRef>,
    pub hidden_shared_with: i64,
}

/// What `:pause` or `:resume` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseOutcome {
    /// `pause` or `resume`.
    pub action: String,
    pub at: String,
    pub changed: Vec<Pause>,
    pub unchanged: Vec<Pause>,
    pub hidden_changed: i64,
    pub interruptions: Vec<PauseInterruption>,
    pub warnings: Vec<String>,
}

/// A planned interruption written over a pause when the monitoring resumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseInterruption {
    pub scenarios: Vec<ScenarioRef>,
    pub start: String,
    pub end: String,
    pub comment: Option<String>,
}

/// `GET /v2/events` row and `/{id}` body: a dated note explaining the curves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    /// `scenario`, `client` or `platform`.
    pub scope: String,
    pub category: String,
    /// `none`, `performance`, `availability` or `both`.
    pub expected_impact: String,
    pub title: String,
    pub description: Option<String>,
    pub start: String,
    pub end: Option<String>,
    pub instant: bool,
    pub external_reference: Option<String>,
    pub external_url: Option<String>,
    pub labels: Vec<String>,
    /// `manual`, `api` or `import`.
    pub source: String,
    pub scenarios: Vec<ScenarioRef>,
    pub hidden_scenarios: i64,
    pub attachments: i64,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_by: Option<String>,
    pub updated_at: Option<String>,
}

/// `POST /v2/events` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventWrite {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_impact: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub start: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    pub scenarios: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_write_keeps_scenarios_even_when_empty() {
        let w = EventWrite {
            category: None,
            expected_impact: None,
            title: "Release 4.2".into(),
            description: None,
            start: "2026-10-12T10:00:00Z".into(),
            end: None,
            external_reference: None,
            external_url: None,
            labels: vec![],
            scenarios: vec![],
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"title": "Release 4.2", "start": "2026-10-12T10:00:00Z", "scenarios": []})
        );
    }
}
