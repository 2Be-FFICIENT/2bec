//! `/v2/scenarios` and `/v2/scenario-groups` types.
//!
//! Kinds and states are camelCase strings as the API spells them; absent
//! values come back as `null`, so nullable fields are `Option`.

use serde::{Deserialize, Serialize};

pub use super::runs::ScenarioRef;

/// Scenario kinds the list filter accepts.
pub const KINDS: [&str; 2] = ["transaction", "socialMonitoring"];

/// The group a scenario belongs to, at most one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRef {
    pub id: String,
    pub name: String,
}

/// One row of `GET /v2/scenarios`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioListItem {
    pub id: String,
    pub name: String,
    /// `transaction` or `socialMonitoring`.
    pub kind: String,
    pub description: Option<String>,
    pub os: Option<String>,
    pub browser: Option<String>,
    pub voice_call: bool,
    /// Observed at all; `None` when unknown (social scenarios).
    pub active: Option<bool>,
    /// Seconds between two scheduled runs; `None` on request or unknown.
    pub interval_seconds: Option<i64>,
    pub replay_acceleration: bool,
    pub group: Option<GroupRef>,
    /// `twitter`, `totalBug` or `unknown` for a social scenario.
    pub social_source: Option<String>,
}

/// Whether the scenario is observed right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringState {
    /// `on`, `off`, `paused` or `interrupted`.
    pub state: String,
    pub since: Option<String>,
    pub until: Option<String>,
    pub comment: Option<String>,
}

/// The availability percentage the scenario is read against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityTarget {
    pub own: Option<f64>,
    pub effective: f64,
}

/// Response time bounds in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thresholds {
    pub target: f64,
    pub client: f64,
    pub unacceptable: f64,
}

/// One step of the current script version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioStep {
    pub id: String,
    pub name: String,
    pub label: String,
    pub order: i64,
    pub measured: bool,
    pub thresholds: Option<Thresholds>,
}

/// `GET /v2/scenarios/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: Option<String>,
    pub os: Option<String>,
    pub browser: Option<String>,
    pub voice_call: bool,
    pub active: Option<bool>,
    pub interval_seconds: Option<i64>,
    pub replay_acceleration: bool,
    pub group: Option<GroupRef>,
    pub social_source: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
    /// Transaction scenarios only.
    pub monitoring: Option<MonitoringState>,
    pub availability_target: Option<AvailabilityTarget>,
    /// Thresholds of the whole run (transaction scenarios only).
    pub run_thresholds: Option<Thresholds>,
    #[serde(default)]
    pub steps: Option<Vec<ScenarioStep>>,
}

/// `GET /v2/scenario-groups` row and `GET /v2/scenario-groups/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub scenarios: Vec<ScenarioRef>,
    /// Scenarios of the group the credential does not see.
    pub hidden_count: i64,
}

/// One save of the script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioVersion {
    pub id: String,
    pub version: i64,
    pub saved_at: String,
    /// The version the robots play now.
    pub current: bool,
    pub stable: bool,
    /// One-line summary, `None` while not yet written.
    pub summary: Option<String>,
    pub description: Option<String>,
    pub note: Option<String>,
}

/// `GET /v2/scenarios/{id}/versions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioVersions {
    pub scenario_id: String,
    pub months: i64,
    pub since: String,
    pub current_version: Option<i64>,
    /// Saves older than the window, not listed.
    pub older_versions: i64,
    #[serde(default)]
    pub items: Vec<ScenarioVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cadence {
    pub declared_seconds: i64,
    pub scheduled_seconds: Option<i64>,
    pub on_demand: bool,
    pub replay_acceleration: bool,
}

/// The daily window the scenario is observed in (Paris wall clock, `HH:mm`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationHours {
    pub start: String,
    pub end: String,
    pub overnight: bool,
}

/// One window of the calendar: no observation (exclusion) or observation only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarWindow {
    pub interruption_id: String,
    pub kind: String,
    pub exclusion: bool,
    pub start: String,
    pub end: String,
    pub comment: Option<String>,
}

/// `GET /v2/scenarios/{id}/calendar`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioCalendar {
    pub scenario_id: String,
    pub from: String,
    pub to: String,
    pub cadence: Cadence,
    pub observation_hours: Option<ObservationHours>,
    pub paused_since: Option<String>,
    /// Scenarios sharing the script, hence the calendar.
    #[serde(default)]
    pub mates: Vec<ScenarioRef>,
    #[serde(default)]
    pub windows: Vec<CalendarWindow>,
    /// The calendar could not be read: the windows are unknown.
    pub unavailable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_round_trips_with_nulls() {
        let json = serde_json::json!({
            "id": "a", "name": "Login", "kind": "transaction", "description": null,
            "os": "Windows", "browser": "Chrome", "voiceCall": false, "active": true,
            "intervalSeconds": 300, "replayAcceleration": true,
            "group": {"id": "g", "name": "Web"}, "socialSource": null,
            "createdAt": "2026-01-01T00:00:00+00:00", "updatedAt": null,
            "monitoring": {"state": "on", "since": null, "until": null, "comment": null},
            "availabilityTarget": {"own": null, "effective": 99.5},
            "runThresholds": {"target": 1000.0, "client": 2000.0, "unacceptable": 5000.0},
            "steps": [{"id": "s", "name": "Step 1 - Open", "label": "Open", "order": 1,
                       "measured": true, "thresholds": null}]
        });
        let s: Scenario = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(s.steps.as_ref().unwrap()[0].label, "Open");
        assert_eq!(serde_json::to_value(&s).unwrap(), json);
    }

    #[test]
    fn calendar_defaults_empty_lists() {
        let json = serde_json::json!({
            "scenarioId": "a", "from": "2026-10-11T00:00:00+00:00", "to": "2026-10-25T00:00:00+00:00",
            "cadence": {"declaredSeconds": 300, "scheduledSeconds": null, "onDemand": false, "replayAcceleration": false},
            "observationHours": null, "pausedSince": null, "unavailable": false
        });
        let c: ScenarioCalendar = serde_json::from_value(json).unwrap();
        assert!(c.mates.is_empty() && c.windows.is_empty());
    }
}
