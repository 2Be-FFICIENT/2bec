//! `/v2/reports`, `/v2/report-schedules` and `/v2/report-deliveries` types.
//!
//! A saved report is one resource with two kinds, `grouped` and
//! `comparative`, whose definitions and computed data differ; the `kind`
//! field tells which, so the read types are tagged enums.

use serde::{Deserialize, Serialize};

pub use super::metrics::{Period, ResponseTimeStats, ResponseTimeThresholds, ValuePoint};
pub use super::runs::ScenarioRef;

/// Saved report kinds.
pub const KINDS: [&str; 2] = ["grouped", "comparative"];

/// Windows of a grouped report's data.
pub const WINDOWS: [&str; 3] = ["month", "week", "day"];

/// Statistics of a comparative report.
pub const COMPARATIVE_STATS: [&str; 3] = ["p85", "p50", "mean"];

/// Statistics of a response times PDF or schedule.
pub const RESPONSE_TIME_STATS: [&str; 4] = ["p85", "p50", "mean", "max"];

/// Languages of a PDF.
pub const LANGS: [&str; 2] = ["fr", "en"];

/// Planning annotations a PDF can print.
pub const PLANNING_KINDS: [&str; 3] = ["monitoringEvent", "plannedInterruption", "scriptVersion"];

/// Report types a schedule mails.
pub const REPORT_TYPES: [&str; 6] = [
    "scenario-synthesis",
    "availability-dashboard",
    "response-times",
    "voice-quality",
    "grouped-availability",
    "comparative",
];

/// Cadences of a schedule.
pub const FREQUENCIES: [&str; 3] = ["daily", "weekly", "monthly"];

/// Outcomes of a delivery.
pub const DELIVERY_STATUSES: [&str; 4] = ["sent", "failed", "skipped", "resent"];

/// One row of `GET /v2/reports`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: Option<String>,
    /// `client` (the whole space) or `private` (the API's keys only).
    pub visibility: String,
    pub owner_name: Option<String>,
    pub updated_at: String,
    pub scenario_count: i64,
    pub groups: Vec<String>,
    /// Whether the API created it, so a key may change it.
    pub api_owned: bool,
}

/// `GET /v2/reports/{id}`, by kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Report {
    Grouped(GroupedReport),
    Comparative(ComparativeReport),
}

impl Report {
    pub fn id(&self) -> &str {
        match self {
            Report::Grouped(r) => &r.id,
            Report::Comparative(r) => &r.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Report::Grouped(r) => &r.name,
            Report::Comparative(r) => &r.name,
        }
    }
}

/// A grouped availability report: scenarios and named perimeters, computed by period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupedReport {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub sections: Vec<String>,
    pub trend_periods: i64,
    pub comparison_periods: i64,
    pub locale: String,
    pub scenarios: Vec<GroupedReportScenario>,
    pub groups: Vec<GroupedReportGroup>,
    pub owner_name: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub api_owned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupedReportScenario {
    pub scenario: ScenarioRef,
    pub position: i64,
    pub display_name: Option<String>,
    pub active: bool,
}

/// `aggregation`: `pooledTime` or `meanOfRates`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupedReportGroup {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub aggregation: String,
    pub scenario_ids: Vec<String>,
}

/// A comparative report: one or two scenarios side by side over paired steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparativeReport {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub stat: String,
    pub owner_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub scenarios: Vec<ComparedScenario>,
    pub pairs: Vec<ComparedPair>,
    pub api_owned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparedScenario {
    pub scenario: ScenarioRef,
    pub os: Option<String>,
    pub browser: Option<String>,
    pub step_ids: Vec<String>,
}

/// A pair of steps compared across two scenarios; one side `None` for a step alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparedPair {
    pub a: Option<String>,
    pub b: Option<String>,
    pub label: Option<String>,
}

/// `GET /v2/reports/{id}/data`, by kind. The grouped shape is tried first
/// since only it carries `perimeters`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReportData {
    Grouped(GroupedReportData),
    Comparative(ComparativeReportData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupedReportData {
    pub id: String,
    pub name: String,
    pub generated_at: String,
    pub as_of: String,
    pub in_progress: bool,
    pub period: ReportPeriod,
    pub periods: Vec<ReportTrendPeriod>,
    pub comparison_periods: Vec<ReportPeriod>,
    pub sections: Vec<String>,
    pub diagnostics: Vec<ReportDiagnostic>,
    pub perimeters: Vec<ReportPerimeter>,
}

/// A period of Paris days: `kind` month, week or day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportPeriod {
    pub kind: String,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTrendPeriod {
    pub period: ReportPeriod,
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDiagnostic {
    pub code: String,
    pub scenario_id: Option<String>,
    pub message: String,
}

/// One perimeter (`kind` scenario, group or report) with its computed sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportPerimeter {
    pub key: String,
    pub kind: String,
    pub name: String,
    pub position: i64,
    pub group_id: Option<String>,
    pub scenarios: Vec<ScenarioRef>,
    pub summary: Option<Vec<ReportSummaryCell>>,
    pub trend: Option<Vec<ReportTrendPoint>>,
    pub failures_by_step: Option<ReportFailuresByStep>,
    pub heatmap: Option<ReportHeatmap>,
    pub calendar: Option<Vec<ReportCalendarDay>>,
    pub diagnostics: Vec<ReportDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummaryCell {
    pub period: ReportPeriod,
    pub availability_percent: Option<f64>,
    pub delta_points_vs_current: Option<f64>,
    pub failures: i64,
    pub runs: i64,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTrendPoint {
    pub period: ReportPeriod,
    pub availability_percent: Option<f64>,
    pub runs: i64,
    pub failures: i64,
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportFailuresByStep {
    pub total_failures: i64,
    pub rows: Vec<ReportStepFailures>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportStepFailures {
    pub step: String,
    pub label: String,
    pub failures: i64,
    pub percent: f64,
    pub scenarios: Vec<ReportStepScenario>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportStepScenario {
    pub scenario: ScenarioRef,
    pub failures: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportHeatmap {
    pub total_failures: i64,
    pub rows: Vec<ReportHeatmapRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportHeatmapRow {
    pub day_of_week: Option<i64>,
    pub date: Option<String>,
    pub hours: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportCalendarDay {
    pub date: String,
    pub in_period: bool,
    pub future: bool,
    pub in_progress: bool,
    pub failures: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparativeReportData {
    pub id: String,
    pub name: String,
    pub stat: String,
    pub period: Period,
    pub scenarios: Vec<ComparedScenarioData>,
    pub pairs: Vec<ComparedPair>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparedScenarioData {
    pub scenario: ScenarioRef,
    pub availability: Option<f64>,
    pub runs: i64,
    pub failures: i64,
    pub selected_failures: i64,
    pub selected_availability: Option<f64>,
    pub partial_journey_mean_ms: Option<f64>,
    pub daily: Vec<ComparedDay>,
    pub steps: Vec<ComparedStep>,
    pub error: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparedDay {
    pub at: String,
    pub runs: i64,
    pub failures: i64,
    pub down_seconds: f64,
    pub availability: Option<f64>,
    pub mean_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparedStep {
    pub id: String,
    pub name: Option<String>,
    pub label: Option<String>,
    pub order: i64,
    pub current: bool,
    pub measured: bool,
    pub thresholds: Option<ResponseTimeThresholds>,
    pub failures: i64,
    pub availability: Option<f64>,
    pub stats: Option<ResponseTimeStats>,
    pub daily_mean_ms: Vec<ValuePoint>,
}

// ---------- schedules ----------

/// One row of `GET /v2/report-schedules` and the body of `/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub report_type: String,
    pub scenarios: Vec<ScenarioRef>,
    pub stat: Option<String>,
    pub report_id: Option<String>,
    pub report_name: Option<String>,
    /// `daily`, `weekly` or `monthly`.
    pub frequency: String,
    pub locale: String,
    pub enabled: bool,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub last_sent_at: Option<String>,
    pub planning_kinds: Option<Vec<String>>,
}

/// `POST /v2/report-schedules` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleWrite {
    pub report_type: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scenarios: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stat: Option<String>,
    pub frequency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    pub enabled: bool,
    pub to: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub planning_kinds: Vec<String>,
}

/// `POST /v2/report-schedules/{id}:send` answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSent {
    pub sent: bool,
    pub error: Option<String>,
}

/// One row of `GET /v2/report-deliveries`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub schedule_id: String,
    pub report_type: String,
    pub label: String,
    pub scenarios: Vec<ScenarioRef>,
    pub report_id: Option<String>,
    pub frequency: String,
    pub schedule_enabled: bool,
    pub created_by: Option<String>,
    /// `sent`, `failed`, `skipped` or `resent`.
    pub status: String,
    pub period_start: String,
    pub period_end: String,
    pub subject: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub error: Option<String>,
    pub sent_at: Option<String>,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_tagged_by_kind() {
        let json = serde_json::json!({
            "id": "r1", "kind": "comparative", "name": "A vs B", "description": null,
            "visibility": "client", "stat": "p85", "ownerName": null,
            "createdAt": "2026-01-01T00:00:00+00:00", "updatedAt": "2026-01-02T00:00:00+00:00",
            "scenarios": [], "pairs": [{"a": "s1", "b": null, "label": "Home"}], "apiOwned": true
        });
        let r: Report = serde_json::from_value(json.clone()).unwrap();
        assert!(matches!(r, Report::Comparative(_)));
        assert_eq!(r.name(), "A vs B");
        assert_eq!(serde_json::to_value(&r).unwrap(), json);
    }

    #[test]
    fn data_picks_the_shape_from_the_body() {
        let comparative: ReportData = serde_json::from_value(serde_json::json!({
            "id": "r1", "name": "A vs B", "stat": "p85",
            "period": {"from": "2026-10-05T00:00:00+02:00", "to": "2026-10-12T00:00:00+02:00"},
            "scenarios": [], "pairs": []
        }))
        .unwrap();
        assert!(matches!(comparative, ReportData::Comparative(_)));
        let grouped: ReportData = serde_json::from_value(serde_json::json!({
            "id": "r2", "name": "Monthly", "generatedAt": "2026-10-12T10:00:00+02:00",
            "asOf": "2026-10-12T10:00:00+02:00", "inProgress": true,
            "period": {"kind": "month", "start": "2026-10-01", "end": "2026-10-31"},
            "periods": [], "comparisonPeriods": [], "sections": ["summary"], "diagnostics": [], "perimeters": []
        }))
        .unwrap();
        assert!(matches!(grouped, ReportData::Grouped(_)));
    }

    #[test]
    fn schedule_write_omits_empty_members() {
        let w = ScheduleWrite {
            report_type: "scenario-synthesis".into(),
            scenarios: vec!["s1".into()],
            report: None,
            stat: None,
            frequency: "weekly".into(),
            locale: None,
            enabled: true,
            to: vec!["ops@example.com".into()],
            cc: vec![],
            planning_kinds: vec![],
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"reportType": "scenario-synthesis", "scenarios": ["s1"], "frequency": "weekly",
                "enabled": true, "to": ["ops@example.com"]})
        );
    }
}
