//! The writable settings of a transaction scenario and its response time
//! thresholds: `/v2/scenarios/{id}/settings`, `…/steps`, `…/run-variables`,
//! `…/thresholds`, `…/thresholds:compute` and `…/thresholds:apply`.

use serde::{Deserialize, Serialize};

pub use super::runs::{RequestVariable, ScenarioRef};
pub use super::scenarios::Thresholds;

/// A scenario's name and a step's name are this long at most.
pub const MAX_NAME: usize = 50;
/// Run variables per scenario, at most.
pub const MAX_RUN_VARIABLES: usize = 20;
/// A run variable's name is this long at most.
pub const MAX_VARIABLE_NAME: usize = 64;
/// A run variable's default value is this long at most.
pub const MAX_VARIABLE_VALUE: usize = 4000;
/// A threshold computation reads this many days at most.
pub const MAX_COMPUTE_DAYS: i64 = 62;

// ---------- settings ----------

/// `GET /v2/scenarios/{id}/settings` and the answer of every settings write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioSettings {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// The scenario's id in the space's service catalogue.
    pub catalogue_id: Option<String>,
    #[serde(default)]
    pub steps: Vec<SettingsStep>,
    #[serde(default)]
    pub run_variables: Vec<RequestVariable>,
    pub updated_at: Option<String>,
}

/// A step of the current version: the name is the key's to change, the
/// measured flag is the platform team's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsStep {
    pub id: String,
    pub order: i64,
    pub name: String,
    pub measured: bool,
}

/// `PUT /v2/scenarios/{id}/steps`: new names for the steps listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepsWrite {
    pub steps: Vec<StepNameWrite>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepNameWrite {
    pub id: String,
    pub name: String,
}

/// `PUT /v2/scenarios/{id}/run-variables`: the whole list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunVariablesWrite {
    pub variables: Vec<RequestVariable>,
}

// ---------- thresholds ----------

/// `GET /v2/scenarios/{id}/thresholds` and the answer of `:apply`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioThresholds {
    pub scenario: ScenarioRef,
    #[serde(default)]
    pub versions: Vec<ThresholdVersion>,
    pub quality: QualityThresholds,
}

/// The thresholds of one script version, per bandwidth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdVersion {
    pub id: String,
    pub begin: String,
    pub end: Option<String>,
    pub comment: Option<String>,
    /// The version the robots play now.
    pub current: bool,
    #[serde(default)]
    pub steps: Vec<ThresholdStep>,
    #[serde(default)]
    pub bandwidths: Vec<ThresholdBandwidth>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdStep {
    pub id: String,
    pub name: String,
    pub label: String,
    pub order: i64,
}

/// The thresholds at one bandwidth: the run's and, aligned with the
/// version's steps, each step's (`None` when unset).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdBandwidth {
    pub id: String,
    pub bandwidth: String,
    pub kilo_bytes_per_second: i64,
    pub preferred: bool,
    pub run: Option<Thresholds>,
    #[serde(default)]
    pub steps: Vec<Option<Thresholds>>,
}

/// The thresholds of the synthesis' other axes, scored daily on the current
/// version: failed runs, per cent of irregular step times, minutes of the
/// longest unavailability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityThresholds {
    pub completion: QualityThreshold,
    pub regularity: QualityThreshold,
    pub restoration: QualityThreshold,
    /// The platform's defaults.
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityThreshold {
    pub target: i64,
    pub client: i64,
    pub unacceptable: i64,
}

/// The computation's settings, as ratios.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdSettings {
    /// The percentile of the successful runs read (0.85 = the 85th).
    pub percentile: f64,
    /// Seconds added to the percentile to make the client threshold.
    pub client_constant_seconds: f64,
    /// The target threshold as a ratio of the client one.
    pub percent_target: f64,
    /// The unacceptable threshold as a ratio of the client one.
    pub percent_unacceptable: f64,
    /// The run's client threshold as a ratio of the sum of its steps'.
    pub percent_duration: f64,
}

/// `POST /v2/scenarios/{id}/thresholds:compute`: whole days in Paris, `from`
/// included, `to` excluded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdComputeWrite {
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<ThresholdSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bandwidths: Option<Vec<String>>,
}

/// The proposals computed from the successful runs of a period, nothing
/// written; `apply` is the body that writes them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdComputation {
    pub from: String,
    pub to: String,
    pub settings: ThresholdSettings,
    #[serde(default)]
    pub versions: Vec<ThresholdVersionProposal>,
    pub apply: ThresholdApplyWrite,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdVersionProposal {
    pub id: String,
    /// Successful runs of the version over the period.
    pub runs: i64,
    #[serde(default)]
    pub bandwidths: Vec<ThresholdBandwidthProposal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdBandwidthProposal {
    pub id: String,
    pub bandwidth: String,
    pub run: ThresholdProposal,
    #[serde(default)]
    pub steps: Vec<ThresholdStepProposal>,
}

/// A proposal: the thresholds (`None` when there was no sample and nothing
/// to keep), whether the current ones were kept for lack of samples, how the
/// period's runs fall in the bands now and with the proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdProposal {
    pub proposed: Option<Thresholds>,
    pub kept_current: bool,
    pub samples: i64,
    pub current_bands: Option<ThresholdBands>,
    pub proposed_bands: Option<ThresholdBands>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdStepProposal {
    pub id: String,
    pub name: String,
    /// The percentile read, in milliseconds.
    pub percentile: Option<f64>,
    pub proposal: ThresholdProposal,
}

/// Runs per band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdBands {
    pub target: i64,
    pub client: i64,
    pub insufficient: i64,
    pub unacceptable: i64,
}

/// `POST /v2/scenarios/{id}/thresholds:apply`: the thresholds of the
/// bandwidths named; an omitted `run` or step keeps its current ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdApplyWrite {
    #[serde(default)]
    pub bandwidths: Vec<ThresholdBandwidthWrite>,
    /// Where a computed draft came from, recorded with the write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computation: Option<ThresholdComputeWrite>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdBandwidthWrite {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Thresholds>,
    #[serde(default)]
    pub steps: Vec<ThresholdStepWrite>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdStepWrite {
    pub id: String,
    pub thresholds: Thresholds,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let json = serde_json::json!({
            "id": "a", "name": "Login", "description": null, "catalogueId": "c",
            "steps": [{"id": "s1", "order": 1, "name": "Home", "measured": true}],
            "runVariables": [{"name": "USER", "defaultValue": null}],
            "updatedAt": "2026-10-01T10:00:00+02:00"
        });
        let s: ScenarioSettings = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(s.run_variables[0].name, "USER");
        assert_eq!(serde_json::to_value(&s).unwrap(), json);
    }

    #[test]
    fn apply_write_omits_what_is_not_given() {
        let w = ThresholdApplyWrite {
            bandwidths: vec![ThresholdBandwidthWrite {
                id: "b".into(),
                run: None,
                steps: vec![],
            }],
            computation: None,
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"bandwidths": [{"id": "b", "steps": []}]})
        );
        let back: ThresholdApplyWrite =
            serde_json::from_value(serde_json::json!({"bandwidths": [{"id": "b"}]})).unwrap();
        assert_eq!(back, w);
    }

    #[test]
    fn computation_reads_null_proposals() {
        let json = serde_json::json!({
            "from": "2026-10-01", "to": "2026-10-08",
            "settings": {"percentile": 0.85, "clientConstantSeconds": 1.0, "percentTarget": 0.8,
                         "percentUnacceptable": 1.25, "percentDuration": 1.0},
            "versions": [{"id": "v", "runs": 10, "bandwidths": [{
                "id": "b", "bandwidth": "ADSL",
                "run": {"proposed": null, "keptCurrent": true, "samples": 0, "currentBands": null, "proposedBands": null},
                "steps": []
            }]}],
            "apply": {"bandwidths": [], "computation": null}
        });
        let c: ThresholdComputation = serde_json::from_value(json).unwrap();
        assert!(c.versions[0].bandwidths[0].run.kept_current);
        assert!(c.apply.computation.is_none());
    }
}
