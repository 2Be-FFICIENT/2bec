//! `/v2/neutralizations` types: setting runs apart from the indicators.

use serde::{Deserialize, Serialize};

pub use super::metrics::Period;
pub use super::runs::ScenarioRef;

/// Operation kinds.
pub const KINDS: [&str; 2] = ["neutralize", "restore"];

/// Operation states.
pub const STATES: [&str; 4] = ["running", "applied", "undone", "failed"];

/// Statuses a selection keeps.
pub const SELECTION_STATUSES: [&str; 3] = ["any", "ok", "failed"];

/// A selection of past runs for a bulk write.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSelection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenario: Vec<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_step: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tag: Vec<String>,
    /// ISO weekdays, 1 = Monday.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weekdays: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_stop: Option<String>,
}

/// `POST /v2/neutralizations` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeutralizationWrite {
    pub selection: RunSelection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_matched: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap: Option<String>,
}

/// The answer of a dry run: what the apply would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub matched: i64,
    pub applicable: i64,
    pub already_neutralized: i64,
    pub protected: i64,
    pub cap: Option<String>,
    pub first_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub comment_required: bool,
    pub asynchronous: bool,
    pub scenarios: Vec<ScenarioCount>,
    pub statuses: Vec<StatusCount>,
    pub sample: Vec<SampleRun>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioCount {
    pub scenario: ScenarioRef,
    pub matched: i64,
    pub applicable: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

/// `outcome`: `applied`, `alreadyAtTarget` or `protected`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleRun {
    pub id: String,
    pub scenario: ScenarioRef,
    pub started_at: String,
    pub status: String,
    pub failed_step: Option<String>,
    pub outcome: String,
}

/// One neutralization operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Neutralization {
    pub id: String,
    /// `neutralize` or `restore`.
    pub kind: String,
    /// `running`, `applied`, `undone` or `failed`.
    pub state: String,
    pub undoes: Option<String>,
    pub undone_by: Option<String>,
    pub selection: Option<RunSelection>,
    pub scenarios: Vec<ScenarioRef>,
    pub hidden_scenario_count: i64,
    pub period: Option<Period>,
    pub comment: Option<String>,
    pub matched: i64,
    pub applied: i64,
    pub skipped: i64,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub created_by: String,
    /// `pegas`, `platform` or `legacy`.
    pub origin: String,
    pub error: Option<String>,
    pub can_undo: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_serializes_only_what_is_set() {
        let w = NeutralizationWrite {
            selection: RunSelection {
                scenario: vec!["s1".into()],
                from: Some("2026-10-01T00:00:00Z".into()),
                to: Some("2026-10-02T00:00:00Z".into()),
                status: Some("failed".into()),
                weekdays: vec![6, 7],
                ..Default::default()
            },
            comment: None,
            expected_matched: Some(3),
            cap: Some("c1".into()),
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({
                "selection": {"scenario": ["s1"], "from": "2026-10-01T00:00:00Z", "to": "2026-10-02T00:00:00Z",
                    "status": "failed", "weekdays": [6, 7]},
                "expectedMatched": 3, "cap": "c1"
            })
        );
    }
}
