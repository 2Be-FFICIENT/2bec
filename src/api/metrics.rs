//! `/v2/metrics/*` and `/v2/scenarios/{id}/synthesis|response-times|voice-quality` types.
//!
//! Rates are percentages (0-100) unless the name says share (0-1); durations
//! are seconds unless the name says milliseconds. Dates are `YYYY-MM-DD`
//! Paris days, instants carry an offset.

use serde::{Deserialize, Serialize};

pub use super::runs::ScenarioRef;

/// Bucket sizes `metrics availability` accepts.
pub const GRANULARITIES: [&str; 3] = ["day", "week", "month"];

/// Comparison windows `scenarios synthesis` accepts.
pub const TRENDS: [&str; 3] = ["day", "week", "month"];

/// Bucket lengths in minutes `metrics social` accepts.
pub const SOCIAL_INTERVALS: [u32; 6] = [10, 15, 30, 60, 120, 240];

/// A half-open range of instants `[from, to)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Period {
    pub from: String,
    pub to: String,
}

/// A run named from another resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRef {
    pub id: String,
    pub started_at: String,
}

/// A code and how many times it was seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Count {
    pub code: String,
    pub count: i64,
}

/// One point of a row-wise series; `None` where nothing was measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValuePoint {
    pub at: String,
    pub value: Option<f64>,
}

/// Response time bounds in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimeThresholds {
    pub target: f64,
    pub client: f64,
    pub unacceptable: f64,
}

/// Counts per response time band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandCounts {
    pub target: i64,
    pub client: i64,
    pub insufficient: i64,
    pub unacceptable: i64,
}

// ---------- availability ----------

/// `GET /v2/metrics/availability`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityMetrics {
    pub period: Period,
    pub previous_period: Period,
    /// `day`, `week` or `month`.
    pub granularity: String,
    pub scenario_count: i64,
    pub observed_count: i64,
    pub target: AvailabilityTargetSummary,
    pub summary: AvailabilitySummary,
    pub series: Vec<AvailabilityBucket>,
    pub by_scenario: Vec<AvailabilityScenarioRow>,
    pub weakest_steps: Vec<WeakestStep>,
    /// `None` when the period is longer than the failure explorer reads.
    pub failure_types: Option<FailureTypes>,
    pub longest_incidents: Vec<IncidentListItem>,
}

/// The target the selection is read against; `mixed` when the scenarios differ.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityTargetSummary {
    pub effective: f64,
    pub mixed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilitySummary {
    pub rate: Option<f64>,
    pub previous_rate: Option<f64>,
    pub error_budget: Option<ErrorBudget>,
    pub up_seconds: f64,
    pub down_seconds: f64,
    pub ok_runs: i64,
    pub failed_runs: i64,
    pub incidents: i64,
    pub mtbf_seconds: Option<f64>,
    pub mttr_seconds: Option<f64>,
    pub longest_incident_seconds: f64,
    pub best_day: Option<DayRate>,
    pub worst_day: Option<DayRate>,
}

/// What the target allowed, what failures consumed, what is left (negative when exceeded).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBudget {
    pub allowed_down_seconds: f64,
    pub consumed_seconds: f64,
    pub remaining_seconds: f64,
    pub consumed_percent: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayRate {
    pub date: String,
    pub rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityBucket {
    pub at: String,
    pub end: String,
    pub rate: Option<f64>,
    pub up_seconds: f64,
    pub down_seconds: f64,
    pub ok_runs: i64,
    pub failed_runs: i64,
    pub incidents: i64,
    pub observed_scenarios: i64,
    pub planned_interruption: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityScenarioRow {
    pub scenario: ScenarioRef,
    pub target: f64,
    pub rate: Option<f64>,
    pub previous_rate: Option<f64>,
    pub up_seconds: f64,
    pub down_seconds: f64,
    pub ok_runs: i64,
    pub failed_runs: i64,
    pub incidents: i64,
    pub mtbf_seconds: Option<f64>,
    pub mttr_seconds: Option<f64>,
    pub longest_incident_seconds: f64,
    pub error: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeakestStep {
    pub step: String,
    pub failures: i64,
    pub failure_share: f64,
    pub downtime_seconds: f64,
    pub scenarios: Vec<WeakestStepScenario>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeakestStepScenario {
    pub scenario: ScenarioRef,
    pub step_order: i64,
    pub failures: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureTypes {
    pub failures: i64,
    pub untyped: i64,
    pub type_count: i64,
    pub types: Vec<FailureType>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureType {
    pub action_id: String,
    pub failures: i64,
    pub failure_share: f64,
    pub step: Option<String>,
    pub description: Option<String>,
    pub first: String,
    pub last: String,
    pub latest_run: RunRef,
    pub scenarios: Vec<FailureTypeScenario>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureTypeScenario {
    pub scenario: ScenarioRef,
    pub failures: i64,
}

/// One streak of consecutive failed runs (also a row of `GET /v2/incidents`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentListItem {
    pub id: String,
    pub scenario: ScenarioRef,
    pub start: String,
    pub end: Option<String>,
    pub ongoing: bool,
    pub duration_seconds: f64,
    pub failed_step: Option<String>,
    pub failed_runs: i64,
    pub first_run: RunRef,
    pub last_run: RunRef,
}

// ---------- response times ----------

/// `GET /v2/metrics/response-times`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimesMetrics {
    pub period: Period,
    pub previous_period: Period,
    pub scenario_count: i64,
    pub measured_count: i64,
    pub summary: ResponseTimesSummary,
    pub series: Vec<ResponseTimesBucket>,
    pub by_scenario: Vec<ResponseTimesScenarioRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimesSummary {
    pub runs: i64,
    pub mean_p85_to_target_ratio: Option<f64>,
    pub previous_mean_p85_to_target_ratio: Option<f64>,
    pub bands: BandCounts,
    pub without_thresholds: i64,
    pub no_data: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimesBucket {
    pub at: String,
    pub measured_scenarios: i64,
    pub mean_ratio_to_target: Option<f64>,
    pub bands: BandCounts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimesScenarioRow {
    pub scenario: ScenarioRef,
    pub thresholds: Option<ResponseTimeThresholds>,
    pub runs: i64,
    pub p50_ms: Option<f64>,
    pub p85_ms: Option<f64>,
    pub mean_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub previous_runs: i64,
    pub previous_p85_ms: Option<f64>,
    pub previous_mean_ms: Option<f64>,
    pub daily_mean_ms: Vec<ValuePoint>,
    pub error: bool,
}

/// `GET /v2/scenarios/{id}/response-times`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioResponseTimes {
    pub scenario: ScenarioRef,
    pub period: Period,
    pub previous_period: Period,
    pub last_day_partial: bool,
    pub thresholds: Option<ResponseTimeThresholds>,
    pub kilo_bytes_per_second: Option<i64>,
    pub successful_runs: i64,
    pub total: Option<ResponseTimeStats>,
    pub previous: Option<ResponseTimeStats>,
    pub daily: Vec<ResponseTimePoint>,
    pub previous_daily: Option<Vec<ResponseTimePoint>>,
    pub hourly: Vec<ResponseTimeHour>,
    pub bands: RunBandCounts,
    pub steps: Vec<ResponseTimeStep>,
    pub slowest_runs: Vec<SlowRun>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimeStats {
    pub count: i64,
    pub p50_ms: f64,
    pub p85_ms: f64,
    pub mean_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimePoint {
    pub at: String,
    pub count: i64,
    pub p50_ms: Option<f64>,
    pub p85_ms: Option<f64>,
    pub mean_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimeHour {
    pub hour: i64,
    pub count: i64,
    pub p50_ms: Option<f64>,
    pub p85_ms: Option<f64>,
    pub mean_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunBandCounts {
    pub target: i64,
    pub client: i64,
    pub insufficient: i64,
    pub unacceptable: i64,
    pub unrated: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimeStep {
    pub name: Option<String>,
    pub label: Option<String>,
    pub order: i64,
    pub current: bool,
    pub measured: bool,
    pub thresholds: Option<ResponseTimeThresholds>,
    pub total: Option<ResponseTimeStats>,
    pub previous: Option<ResponseTimeStats>,
    /// Share of the summed step times (0-1).
    pub share_of_total: f64,
    pub server_time_share: Option<f64>,
    pub mean_downloaded_bytes: Option<f64>,
    pub daily: Option<Vec<ResponseTimePoint>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlowRun {
    pub id: String,
    pub started_at: String,
    pub total_ms: f64,
    pub slowest_step: Option<String>,
    pub slowest_step_ms: Option<f64>,
    pub band: Option<String>,
}

// ---------- voice quality ----------

/// `GET /v2/metrics/voice-quality`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceQualityMetrics {
    pub period: Period,
    pub scenario_count: i64,
    pub totals: VoiceQualityFigures,
    pub by_scenario: Vec<VoiceQualityScenarioRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceQualityFigures {
    pub calls: i64,
    pub measured: i64,
    pub no_quality: i64,
    pub not_collected: i64,
    pub average_mos: Option<f64>,
    pub min_mos: Option<f64>,
    pub max_mos: Option<f64>,
    pub average_error_rate_percent: Option<f64>,
    pub max_error_rate_percent: Option<f64>,
    pub average_jitter_ms: Option<f64>,
    pub max_jitter_ms: Option<f64>,
    pub ratings: VoiceRatingCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRatingCounts {
    pub excellent: i64,
    pub good: i64,
    pub fair: i64,
    pub poor: i64,
    pub bad: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceQualityScenarioRow {
    pub scenario: ScenarioRef,
    pub figures: VoiceQualityFigures,
    pub error: bool,
}

/// `GET /v2/scenarios/{id}/voice-quality`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioVoiceQuality {
    pub scenario: ScenarioRef,
    pub period: Period,
    pub summary: VoiceQualityFigures,
    pub daily: Vec<VoiceQualityDay>,
    pub end_reasons: Vec<Count>,
    pub calls: Vec<VoiceCall>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceQualityDay {
    pub date: String,
    pub calls: i64,
    pub measured: i64,
    pub average_mos: Option<f64>,
    pub min_mos: Option<f64>,
    pub average_error_rate_percent: Option<f64>,
    pub average_jitter_ms: Option<f64>,
    pub ratings: VoiceRatingCounts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceCall {
    pub id: String,
    pub started_at: String,
    pub status: String,
    /// `measured`, `noQuality` or `notCollected`.
    pub measurement: String,
    pub mos: Option<f64>,
    pub rating: Option<String>,
    pub error_rate_percent: Option<f64>,
    pub loss_percent: Option<f64>,
    pub jitter_avg_ms: Option<f64>,
    pub jitter_max_ms: Option<f64>,
    pub one_way_delay_ms: Option<i64>,
    pub round_trip_ms: Option<i64>,
    pub outbound_mos: Option<f64>,
    pub outbound_error_rate_percent: Option<f64>,
    pub codec: Option<String>,
    pub end_reason: Option<String>,
    pub talk_duration_ms: Option<i64>,
    pub post_dial_delay_ms: Option<i64>,
}

// ---------- social monitoring ----------

/// `GET /v2/metrics/social`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialMetrics {
    pub period: Period,
    pub interval_minutes: i64,
    pub as_of: String,
    pub scenario_count: i64,
    pub error_count: i64,
    pub totals: SocialTotals,
    pub by_scenario: Vec<SocialScenarioRow>,
    pub series: Vec<SocialBucket>,
    pub days: Vec<SocialDay>,
    /// With `include=events` only.
    pub events: Option<Vec<SocialEvent>>,
    pub events_total: i64,
    pub events_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialTotals {
    pub events: i64,
    pub incidents: i64,
    pub incident_rate: Option<f64>,
    pub previous_events: Option<i64>,
    pub previous_incidents: Option<i64>,
    pub worst_grade: String,
    pub time_in_grade: GradeCounts,
    pub peak_bucket_start: Option<String>,
    pub peak_bucket_incidents: i64,
    pub busiest_day: Option<String>,
    pub peak_hour: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradeCounts {
    pub target: i64,
    pub client: i64,
    pub insufficient: i64,
    pub unacceptable: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialScenarioRow {
    pub scenario: ScenarioRef,
    pub source: String,
    pub state: String,
    pub enabled: bool,
    pub thresholds_per_hour: Option<SocialThresholds>,
    pub events: i64,
    pub incidents: i64,
    pub incident_rate: Option<f64>,
    pub previous_events: Option<i64>,
    pub previous_incidents: Option<i64>,
    pub worst_grade: String,
    pub last_incident: Option<String>,
    pub last_event: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialThresholds {
    pub target: i64,
    pub client: i64,
    pub unacceptable: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialBucket {
    pub at: String,
    pub end: String,
    pub events: i64,
    pub incidents: i64,
    pub grade: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialDay {
    pub date: String,
    pub events: i64,
    pub incidents: i64,
    pub grade: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialEvent {
    pub id: i64,
    pub scenario: ScenarioRef,
    pub at: String,
    pub text: Option<String>,
    pub link: Option<String>,
    pub incident: bool,
    pub explanation: Option<String>,
}

/// `GET /v2/metrics/social/analysis`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialAnalysisMetrics {
    pub period: Period,
    pub scenario_count: i64,
    pub error_count: i64,
    pub keywords: Vec<Count>,
    pub days: Vec<SocialAnalysisDay>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialAnalysisDay {
    pub date: String,
    pub scenario: ScenarioRef,
    pub events: i64,
    pub incidents: i64,
    pub incident_rate: Option<f64>,
    pub online_issues: Vec<String>,
    pub other_issues: Vec<String>,
    pub keywords: Vec<String>,
}

// ---------- synthesis ----------

/// `GET /v2/scenarios/{id}/synthesis`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioSynthesis {
    pub scenario: ScenarioRef,
    pub from: String,
    pub to: String,
    pub trend: String,
    pub indices: SynthesisAxes,
    pub summary: SynthesisSummary,
    pub trends: Option<Vec<SynthesisTrend>>,
    /// The 24 Paris hours, for a one-day period only.
    pub hourly: Option<Vec<SynthesisHour>>,
    pub daily: Option<Vec<SynthesisDay>>,
    pub percentiles: Option<SynthesisPercentiles>,
    pub reliability: Option<SynthesisReliability>,
}

/// An index out of 10 against the client and the target thresholds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesisIndex {
    pub client: f64,
    pub target: f64,
    pub valid: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesisAxes {
    pub synthesis: SynthesisIndex,
    pub duration: SynthesisIndex,
    pub interactivity: SynthesisIndex,
    pub completion: SynthesisIndex,
    pub regularity: SynthesisIndex,
    pub restoration: SynthesisIndex,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesisSummary {
    pub runs: i64,
    pub failed_runs: i64,
    pub availability_rate: f64,
    pub incidents: i64,
    pub longest_incident_seconds: f64,
    pub bands: Option<BandCounts>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesisTrend {
    pub window: String,
    pub from: String,
    pub to: String,
    pub client_index: Option<f64>,
    pub availability_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesisHour {
    pub hour: i64,
    pub client_index: Option<f64>,
    pub valid: bool,
    pub runs: i64,
    pub failures: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesisDay {
    pub date: String,
    pub client_index: Option<f64>,
    pub valid: bool,
    pub runs: i64,
    pub failures: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesisPercentiles {
    pub p50_ms: Option<f64>,
    pub p85_ms: Option<f64>,
    pub runs: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesisReliability {
    pub mttr_seconds: Option<f64>,
    pub incidents: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_round_trips() {
        let json = serde_json::json!({
            "period": {"from": "2026-09-12T00:00:00+02:00", "to": "2026-10-12T00:00:00+02:00"},
            "previousPeriod": {"from": "2026-08-13T00:00:00+02:00", "to": "2026-09-12T00:00:00+02:00"},
            "granularity": "day", "scenarioCount": 2, "observedCount": 1,
            "target": {"effective": 99.5, "mixed": false},
            "summary": {"rate": 99.1, "previousRate": null, "errorBudget": null,
                "upSeconds": 100.0, "downSeconds": 1.0, "okRuns": 10, "failedRuns": 1, "incidents": 1,
                "mtbfSeconds": null, "mttrSeconds": 60.0, "longestIncidentSeconds": 60.0,
                "bestDay": {"date": "2026-10-01", "rate": 100.0}, "worstDay": null},
            "series": [], "byScenario": [], "weakestSteps": [], "failureTypes": null, "longestIncidents": []
        });
        let m: AvailabilityMetrics = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(m.summary.best_day.as_ref().unwrap().date, "2026-10-01");
        assert_eq!(serde_json::to_value(&m).unwrap(), json);
    }

    #[test]
    fn synthesis_parses_optional_sections() {
        let json = serde_json::json!({
            "scenario": {"id": "a", "name": "Login", "kind": "transaction"},
            "from": "2026-10-05", "to": "2026-10-11", "trend": "day",
            "indices": {
                "synthesis": {"client": 8.5, "target": 7.0, "valid": true},
                "duration": {"client": 9.0, "target": 8.0, "valid": true},
                "interactivity": {"client": 9.0, "target": 8.0, "valid": true},
                "completion": {"client": 9.0, "target": 8.0, "valid": true},
                "regularity": {"client": 9.0, "target": 8.0, "valid": false},
                "restoration": {"client": 9.0, "target": 8.0, "valid": true}
            },
            "summary": {"runs": 100, "failedRuns": 2, "availabilityRate": 98.0, "incidents": 1,
                "longestIncidentSeconds": 300.0, "bands": null},
            "trends": null, "hourly": null, "daily": null, "percentiles": null, "reliability": null
        });
        let s: ScenarioSynthesis = serde_json::from_value(json).unwrap();
        assert!(s.trends.is_none());
        assert!(!s.indices.regularity.valid);
    }
}
