//! `/v2/alerts`, `/v2/alert-groups`, `/v2/alert-recipients`,
//! `/v2/alert-suspensions`, `/v2/alert-channels` and `/v2/notifications` types.
//!
//! Recipients are named, never their addresses; delivery hooks never appear.
//! Codes are camelCase strings; absent values are `null`, hence `Option`.

use serde::{Deserialize, Serialize};

pub use super::runs::ScenarioRef;

/// Channels the notifications filter accepts.
pub const CHANNELS: [&str; 8] = [
    "mail", "sms", "voice", "hook", "signal", "push", "snmp", "other",
];

/// Triggers the notifications filter accepts.
pub const TRIGGERS: [&str; 4] = ["unavailability", "responseTime", "restoration", "other"];

/// Channels a test alert can go through.
pub const TEST_CHANNELS: [&str; 4] = ["mail", "sms", "voice", "signal"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRef {
    pub id: String,
    pub name: String,
    pub channel: String,
    pub members: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRef {
    pub id: String,
    pub scenario: ScenarioRef,
    pub label: String,
    pub order: i64,
    pub suspended: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureRule {
    pub notify_restoration: bool,
    /// `ranks` or `every`.
    pub trigger_mode: String,
    #[serde(default)]
    pub ranks: Vec<i64>,
    pub every: i64,
    /// `none`, `only` or `except`.
    pub tag_mode: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub steps: Vec<StepRef>,
    pub hidden_steps: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseTimeRule {
    pub notify_restoration: bool,
    #[serde(default)]
    pub ranks: Vec<i64>,
    #[serde(default)]
    pub scenarios: Vec<ScenarioRef>,
    pub hidden_scenarios: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialRule {
    pub scenario: Option<ScenarioRef>,
    pub enabled: bool,
    pub trigger_count: i64,
    pub watch_window_minutes: i64,
    pub repetition_minutes: Option<i64>,
}

/// The schedule; quiet periods keep the API's calendar shape as raw JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub kind: String,
    #[serde(default)]
    pub quiet_periods: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub no_recipient: bool,
    pub watches_nothing: bool,
    pub stale_steps: bool,
    pub duplicate: bool,
    pub unreachable: bool,
}

impl Health {
    /// Short labels of what is wrong, empty when healthy.
    pub fn issues(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.no_recipient {
            v.push("no recipient");
        }
        if self.watches_nothing {
            v.push("watches nothing");
        }
        if self.stale_steps {
            v.push("stale steps");
        }
        if self.duplicate {
            v.push("duplicate");
        }
        if self.unreachable {
            v.push("unreachable");
        }
        v
    }
}

/// `GET /v2/alerts` row and `GET /v2/alerts/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub group: GroupRef,
    pub failure: Option<FailureRule>,
    pub extra_failure_rules: i64,
    pub response_time: Option<ResponseTimeRule>,
    pub extra_response_time_rules: i64,
    #[serde(default)]
    pub social: Vec<SocialRule>,
    pub schedule: Schedule,
    pub health: Health,
    pub mail_subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipient {
    pub id: String,
    pub name: String,
    pub muted: bool,
}

/// `GET /v2/alert-groups` row and `GET /v2/alert-groups/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    pub channel: String,
    pub hook_type: Option<String>,
    pub deliverable: bool,
    /// How many alerts send to the group.
    pub alerts: i64,
    #[serde(default)]
    pub members: Vec<Recipient>,
}

/// Body of `POST /v2/alert-groups`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupWrite {
    pub name: String,
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_type: Option<String>,
    pub members: Vec<String>,
}

/// `GET /v2/alert-channels` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub channel: String,
    pub deliverable: bool,
    pub creatable: bool,
    #[serde(default)]
    pub hook_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupName {
    pub id: String,
    pub name: String,
}

/// `GET /v2/alert-recipients` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipientEntry {
    pub id: String,
    pub login: String,
    pub name: String,
    pub has_mail: bool,
    pub has_mobile: bool,
    pub muted: bool,
    #[serde(default)]
    pub groups: Vec<GroupName>,
}

/// Answer of `POST /v2/alert-recipients/{id}:mute|unmute`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipientMuted {
    pub id: String,
    pub name: String,
    pub muted: bool,
}

/// Body of `POST /v2/alerts:test`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestWrite {
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mails: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mail_subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSent {
    pub channel: String,
    pub sent: i64,
    #[serde(default)]
    pub recipients: Vec<String>,
}

/// A step named for a write: the scenario and the step of its current version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepWrite {
    pub scenario: String,
    pub step: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepsWrite {
    pub steps: Vec<StepWrite>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepsWritten {
    pub written: i64,
    pub unchanged: i64,
    pub failed: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspendedStep {
    pub id: String,
    pub label: String,
    pub order: i64,
    pub watched_by: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspendedScenario {
    pub scenario: ScenarioRef,
    pub active: bool,
    #[serde(default)]
    pub steps: Vec<SuspendedStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowStep {
    pub id: String,
    pub label: String,
    pub order: i64,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowScenario {
    pub scenario: ScenarioRef,
    #[serde(default)]
    pub steps: Vec<WindowStep>,
}

/// A dated window holding the alerts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub id: String,
    pub start: String,
    pub end: String,
    /// `upcoming`, `ongoing` or `past`.
    pub state: String,
    /// `all`, `steps` or `pattern`.
    pub scope: String,
    #[serde(default)]
    pub scenarios: Vec<WindowScenario>,
    pub hidden_targets: i64,
    pub scenario_pattern: Option<String>,
    pub step_pattern: Option<String>,
    pub comment: Option<String>,
    pub created_by: Option<String>,
    pub created_at: Option<String>,
}

/// Body of `POST /v2/alert-suspensions/windows`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowWrite {
    pub start: String,
    pub end: String,
    pub all: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<StepWrite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// `GET /v2/alert-suspensions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suspensions {
    pub tenant_suspended: bool,
    pub platform_suspended: bool,
    #[serde(default)]
    pub suspended_steps: Vec<SuspendedScenario>,
    pub hidden_steps: i64,
    #[serde(default)]
    pub windows: Vec<Window>,
    pub windows_available: bool,
    #[serde(default)]
    pub muted_recipients: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRef {
    pub id: String,
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRecipient {
    pub name: String,
    pub sent_at: String,
}

/// `GET /v2/notifications` row: one alert event actually sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub sent_at: String,
    pub scenario: ScenarioRef,
    pub run: Option<RunRef>,
    pub channel: String,
    pub trigger: String,
    /// The alert configuration's name.
    pub alert: String,
    pub group: String,
    pub sends: i64,
    #[serde(default)]
    pub recipients: Vec<NotificationRecipient>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_issues() {
        let h = Health {
            no_recipient: true,
            watches_nothing: false,
            stale_steps: true,
            duplicate: false,
            unreachable: false,
        };
        assert_eq!(h.issues(), vec!["no recipient", "stale steps"]);
    }

    #[test]
    fn writes_skip_absent_fields() {
        let w = TestWrite {
            channel: "mail".into(),
            ..TestWrite::default()
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"channel": "mail"})
        );
        let w = WindowWrite {
            start: "a".into(),
            end: "b".into(),
            all: true,
            ..WindowWrite::default()
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"start": "a", "end": "b", "all": true})
        );
    }
}
