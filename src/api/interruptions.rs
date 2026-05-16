//! `/v2/interruptions` types: planned non-observation windows and rules.

use serde::{Deserialize, Serialize};

pub use super::runs::ScenarioRef;

/// States the list filter accepts: one-off windows are `ongoing`, `upcoming`
/// or `past`; recurring rules are `rule` (in force) or `ended`.
pub const STATES: [&str; 5] = ["ongoing", "upcoming", "past", "rule", "ended"];

/// Kinds of calendar spec.
pub const KINDS: [&str; 4] = ["date", "daily", "weekly", "monthly"];

/// At most this many scenarios per planning.
pub const MAX_SCENARIOS: usize = 200;

/// A planned interruption, one-off or recurring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interruption {
    pub id: String,
    /// `date`, `daily`, `weekly` or `monthly`.
    pub kind: String,
    pub state: String,
    /// `false` marks a legacy inclusion window, read-only.
    pub exclusion: bool,
    pub comment: Option<String>,
    pub spec: serde_json::Value,
    pub scenarios: Vec<ScenarioRef>,
    pub hidden_scenarios: i64,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    pub applied: bool,
    pub replaces: Option<String>,
    pub occurrences: Option<Vec<DateWindow>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateWindow {
    pub start: String,
    pub end: String,
}

/// `POST /v2/interruptions` body. The spec is passed through as JSON since
/// it has four shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptionWrite {
    pub scenarios: Vec<String>,
    pub spec: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<String>,
}

/// What a planning did: one interruption per script, and the refusals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    pub interruptions: Vec<Interruption>,
    pub failed: Vec<Refusal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub scenarios: Vec<ScenarioRef>,
    pub reason: String,
}

/// A one-line reading of a spec for tables.
pub fn describe_spec(spec: &serde_json::Value) -> String {
    let kind = spec["kind"].as_str().unwrap_or("?");
    match kind {
        "date" => format!(
            "{} to {}",
            crate::output::short_time(spec["date"]["start"].as_str().unwrap_or("?")),
            crate::output::short_time(spec["date"]["end"].as_str().unwrap_or("?"))
        ),
        "daily" => {
            let d = &spec["daily"];
            let days = d["weekdays"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_i64())
                        .map(weekday)
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            let hours = d["hours"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|h| {
                            format!(
                                "{}-{}",
                                h["start"].as_str().unwrap_or("?"),
                                h["end"].as_str().unwrap_or("?")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            format!(
                "daily {days} {hours}{}",
                if d["bankHolidays"].as_bool() == Some(true) {
                    " +holidays"
                } else {
                    ""
                }
            )
        }
        "weekly" => {
            let w = &spec["weekly"];
            format!(
                "weekly {} {} to {} {}",
                weekday(w["startDay"].as_i64().unwrap_or(0)),
                w["startTime"].as_str().unwrap_or("?"),
                weekday(w["endDay"].as_i64().unwrap_or(0)),
                w["endTime"].as_str().unwrap_or("?")
            )
        }
        "monthly" => {
            let m = &spec["monthly"];
            let when = match (
                m["day"].as_i64(),
                m["ordinal"].as_str(),
                m["weekday"].as_i64(),
            ) {
                (Some(d), _, _) => format!("day {d}"),
                (None, Some(o), Some(w)) => format!("{o} {}", weekday(w)),
                _ => "?".into(),
            };
            let month = match m["month"].as_i64() {
                Some(0) | None => "every month".to_string(),
                Some(n) => format!("month {n}"),
            };
            format!(
                "monthly {when} {} {}-{}",
                month,
                m["startTime"].as_str().unwrap_or("?"),
                m["endTime"].as_str().unwrap_or("?")
            )
        }
        other => other.to_string(),
    }
}

fn weekday(iso: i64) -> &'static str {
    match iso {
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        6 => "Sat",
        7 => "Sun",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_read_as_one_line() {
        assert_eq!(
            describe_spec(
                &serde_json::json!({"kind": "date", "date": {"start": "2026-10-12T22:00:00+02:00", "end": "2026-10-13T02:00:00+02:00"}})
            ),
            "2026-10-12 22:00 to 2026-10-13 02:00"
        );
        assert_eq!(
            describe_spec(
                &serde_json::json!({"kind": "daily", "daily": {"weekdays": [1, 5], "bankHolidays": true, "hours": [{"start": "02:00", "end": "03:00"}]}})
            ),
            "daily Mon,Fri 02:00-03:00 +holidays"
        );
        assert_eq!(
            describe_spec(
                &serde_json::json!({"kind": "weekly", "weekly": {"startDay": 6, "startTime": "22:00", "endDay": 7, "endTime": "06:00"}})
            ),
            "weekly Sat 22:00 to Sun 06:00"
        );
        assert_eq!(
            describe_spec(
                &serde_json::json!({"kind": "monthly", "monthly": {"day": null, "ordinal": "last", "weekday": 5, "month": 0, "startTime": "20:00", "endTime": "23:00"}})
            ),
            "monthly last Fri every month 20:00-23:00"
        );
    }
}
