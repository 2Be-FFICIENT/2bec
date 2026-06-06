//! `/v2/availability-settings` and `/v2/availability-targets` types: how the
//! availability rate is computed and against which percentage it is read.

use serde::{Deserialize, Serialize};

pub use super::runs::ScenarioRef;

/// What becomes of the neutralized runs in the availability rate.
pub const COMPUTING_TYPES: [&str; 4] = [
    "removeNeutralized",
    "ignoreNeutralized",
    "neutralizedAsSuccess",
    "neutralizedAsFailure",
];

/// `failureCountsFrom` is between these.
pub const FAILURE_COUNTS_FROM: std::ops::RangeInclusive<i64> = 1..=10;
/// Tags per tag rule, at most.
pub const MAX_RULE_TAGS: usize = 20;
/// Scenarios per targets write, at most.
pub const MAX_TARGET_SCENARIOS: usize = 500;

/// `GET /v2/availability-settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilitySettings {
    pub computing_type: String,
    /// A scenario is down from this many consecutive failures.
    pub failure_counts_from: i64,
    pub tag_rules_enabled: bool,
    pub tag_rules: TagRules,
    /// Whether the space's administrators may change these rules themselves.
    pub editable: bool,
    /// The platform's defaults, nothing saved yet.
    pub is_default: bool,
}

/// The tags marking a run as a failure, a success or neutral.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRules {
    #[serde(default)]
    pub failure: Vec<TagRef>,
    #[serde(default)]
    pub success: Vec<TagRef>,
    #[serde(default)]
    pub neutral: Vec<TagRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRef {
    pub id: String,
    pub name: Option<String>,
    pub kind: Option<String>,
}

/// `PUT /v2/availability-settings`: the whole rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilitySettingsWrite {
    pub computing_type: String,
    pub failure_counts_from: i64,
    pub tag_rules_enabled: bool,
    pub tag_rules: TagRulesWrite,
}

/// Tag rules by tag id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRulesWrite {
    pub failure: Vec<String>,
    pub success: Vec<String>,
    pub neutral: Vec<String>,
}

impl AvailabilitySettingsWrite {
    /// The current rules as a write body, to change part of them.
    pub fn from_current(s: &AvailabilitySettings) -> Self {
        let ids = |tags: &[TagRef]| tags.iter().map(|t| t.id.clone()).collect();
        Self {
            computing_type: s.computing_type.clone(),
            failure_counts_from: s.failure_counts_from,
            tag_rules_enabled: s.tag_rules_enabled,
            tag_rules: TagRulesWrite {
                failure: ids(&s.tag_rules.failure),
                success: ids(&s.tag_rules.success),
                neutral: ids(&s.tag_rules.neutral),
            },
        }
    }
}

/// What a save of the rules did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilitySettingsWritten {
    pub settings: AvailabilitySettings,
    pub rules_changed: bool,
    /// The past days are recomputed with the new rules in the background;
    /// `false` when the platform did not take it (the rules are saved anyway).
    pub recompute_scheduled: bool,
}

/// `GET /v2/availability-targets`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityTargets {
    /// The space's own target; `None` when it follows the platform's default.
    pub tenant: Option<f64>,
    pub effective: f64,
    pub default: f64,
    #[serde(default)]
    pub scenarios: Vec<ScenarioTarget>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioTarget {
    pub scenario: ScenarioRef,
    pub own: Option<f64>,
    pub effective: f64,
}

/// `PUT /v2/availability-targets`: `tenant` present sets the space's target
/// (`null` = the platform's default again); each scenario listed gets its own
/// (`null` = it follows the space's again).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityTargetsWrite {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<Option<f64>>,
    #[serde(default)]
    pub scenarios: Vec<ScenarioTargetWrite>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioTargetWrite {
    pub scenario: String,
    pub target: Option<f64>,
}

/// What a write of the targets did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityTargetsWritten {
    pub written: i64,
    pub unchanged: i64,
    pub failed: i64,
    pub targets: AvailabilityTargets,
}

/// The error for a target, `None` when acceptable: above 50, below 100, 3
/// decimals at most.
pub fn check_target(t: f64) -> Option<&'static str> {
    if t.is_nan() || t <= 50.0 || t >= 100.0 {
        Some("a target is a percentage above 50 and below 100")
    } else if ((t * 1000.0).round() / 1000.0 - t).abs() > 1e-9 {
        Some("a target has at most 3 decimals")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_write_distinguishes_absent_from_null() {
        let w = AvailabilityTargetsWrite {
            tenant: None,
            scenarios: vec![ScenarioTargetWrite {
                scenario: "s".into(),
                target: None,
            }],
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"scenarios": [{"scenario": "s", "target": null}]})
        );
        let w = AvailabilityTargetsWrite {
            tenant: Some(None),
            scenarios: vec![],
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"tenant": null, "scenarios": []})
        );
        let w = AvailabilityTargetsWrite {
            tenant: Some(Some(99.5)),
            scenarios: vec![],
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"tenant": 99.5, "scenarios": []})
        );
    }

    #[test]
    fn settings_become_a_write_body() {
        let s = AvailabilitySettings {
            computing_type: "removeNeutralized".into(),
            failure_counts_from: 2,
            tag_rules_enabled: true,
            tag_rules: TagRules {
                failure: vec![TagRef {
                    id: "t1".into(),
                    name: Some("KO".into()),
                    kind: Some("business".into()),
                }],
                success: vec![],
                neutral: vec![],
            },
            editable: true,
            is_default: false,
        };
        let w = AvailabilitySettingsWrite::from_current(&s);
        assert_eq!(w.tag_rules.failure, vec!["t1".to_string()]);
        assert_eq!(w.failure_counts_from, 2);
    }

    #[test]
    fn target_rule() {
        assert!(check_target(99.5).is_none());
        assert!(check_target(99.999).is_none());
        assert!(check_target(50.0).is_some());
        assert!(check_target(100.0).is_some());
        assert!(check_target(99.1234).is_some());
    }
}
