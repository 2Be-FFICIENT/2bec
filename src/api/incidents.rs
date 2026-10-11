//! `/v2/incidents` types: streaks of consecutive failed runs.

use serde::{Deserialize, Serialize};

pub use super::runs::{RunFailure, ScenarioRef};

/// `GET /v2/incidents/{id}`: the incident with its runs, its first failure
/// and the alerts it caused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    pub id: String,
    pub scenario: ScenarioRef,
    pub start: String,
    pub end: Option<String>,
    pub ongoing: bool,
    pub duration_seconds: i64,
    pub failed_step: Option<String>,
    pub failed_runs: i64,
    pub runs: Vec<IncidentRun>,
    pub failure: Option<RunFailure>,
    /// `None` when the alert log could not be read.
    pub alerts_sent: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentRun {
    pub id: String,
    pub started_at: String,
    pub status: String,
    pub failed_step: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incident_parses_without_failure_or_alerts() {
        let i: Incident = serde_json::from_value(serde_json::json!({
            "id": "i1", "scenario": {"id": "s", "name": "Login", "kind": "transaction"},
            "start": "2026-10-11T08:00:00+02:00", "end": null, "ongoing": true, "durationSeconds": 3720,
            "failedStep": "Pay", "failedRuns": 12,
            "runs": [{"id": "r1", "startedAt": "2026-10-11T08:00:00+02:00", "status": "failed", "failedStep": "Pay"}],
            "failure": null, "alertsSent": null
        }))
        .unwrap();
        assert!(i.ongoing);
        assert_eq!(i.runs.len(), 1);
    }
}
