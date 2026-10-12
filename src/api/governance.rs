//! `/v2/audit-entries`, `/v2/comments` and `/v2/tickets` types: the
//! administration history, the team's comments on the runs, and the support
//! requests. Read only.

use serde::{Deserialize, Serialize};

use super::metrics::RunRef;
use super::runs::ScenarioRef;

/// The areas of the administration history a key may read (the staff-only
/// ones are never listed).
pub const AUDIT_AREAS: [&str; 17] = [
    "tags",
    "badges",
    "neutralizations",
    "availability",
    "monitoring",
    "interruptions",
    "alerts",
    "users",
    "profiles",
    "scripts",
    "variables",
    "scenarios",
    "reports",
    "events",
    "notifications",
    "access",
    "ai",
];
/// The `outcome` values of an entry.
pub const AUDIT_OUTCOMES: [&str; 3] = ["succeeded", "failed", "partial"];
/// The `source` values of an entry.
pub const AUDIT_SOURCES: [&str; 4] = ["web", "worker", "api", "agent"];
/// The audit list covers this many days at most.
pub const AUDIT_MAX_DAYS: u32 = 366;
/// The `outcome` values of the comments filter: the run's status.
pub const COMMENT_OUTCOMES: [&str; 3] = ["ok", "failed", "neutralized"];
/// The comments list takes this many scenarios at most.
pub const COMMENT_MAX_SCENARIOS: usize = 100;
/// The `view` values of the tickets list.
pub const TICKET_VIEWS: [&str; 4] = ["open", "waiting", "solved", "all"];

/// One operation of the administration history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub occurred_at: String,
    pub area: String,
    pub action: String,
    pub actor: Option<String>,
    /// Done by the platform's staff on the space's behalf.
    pub actor_is_staff: bool,
    /// `web`, `worker`, `api` or `agent`.
    pub source: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub target_label: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub comment: Option<String>,
    /// `succeeded`, `failed` or `partial`.
    pub outcome: String,
    pub error: Option<String>,
    pub reversible: bool,
    pub reverts_entry_id: Option<i64>,
    pub reverted_by_entry_id: Option<i64>,
    pub batch_id: Option<String>,
    pub scenarios: Vec<ScenarioRef>,
}

/// `GET /v2/audit-entries/{id}`: the entry with the state before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntryDetail {
    pub entry: AuditEntry,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

/// A comment left on a run, by publication time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub text: String,
    pub author: String,
    pub published_at: String,
    pub scenario: ScenarioRef,
    pub run: RunRef,
    /// The run's status: `ok`, `failed` or `neutralized`.
    pub run_status: String,
}

/// A support request between the space and the platform's team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticket {
    pub id: i64,
    pub merged_ids: Vec<i64>,
    pub title: Option<String>,
    pub subject: Option<String>,
    /// `new`, `open`, `pending`, `hold`, `solved` or `closed`.
    pub status: String,
    pub kind: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub summary: Option<String>,
    pub resolution: Option<String>,
    pub scenarios: Vec<ScenarioRef>,
    pub keywords: Vec<String>,
    pub incident_started_at: Option<String>,
}

/// `GET /v2/tickets/{id}`: the request with its conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketDetail {
    pub ticket: Ticket,
    pub messages: Vec<TicketMessage>,
}

/// `author` is `customer`, `support` or `automated`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketMessage {
    pub at: String,
    pub author: String,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_entry_round_trips_with_free_form_parameters() {
        let json = serde_json::json!({
            "id": 42, "occurredAt": "2026-10-01T10:00:00+00:00", "area": "tags",
            "action": "tag.create", "actor": "alice", "actorIsStaff": false, "source": "api",
            "targetType": "tag", "targetId": "7", "targetLabel": "release",
            "parameters": {"name": "release", "nested": [1, 2]}, "comment": null,
            "outcome": "succeeded", "error": null, "reversible": true, "revertsEntryId": null,
            "revertedByEntryId": 43, "batchId": null,
            "scenarios": [{"id": "s", "name": "Login", "kind": "transaction"}]
        });
        let e: AuditEntry = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(e.reverted_by_entry_id, Some(43));
        assert_eq!(serde_json::to_value(&e).unwrap(), json);
    }

    #[test]
    fn ticket_detail_reads_messages() {
        let d: TicketDetail = serde_json::from_value(serde_json::json!({
            "ticket": {
                "id": 7, "mergedIds": [8], "title": "Login fails", "subject": null,
                "status": "open", "kind": "Robot", "createdAt": "2026-10-01T10:00:00+00:00",
                "updatedAt": "2026-10-02T10:00:00+00:00", "summary": null, "resolution": null,
                "scenarios": [], "keywords": ["login"], "incidentStartedAt": null
            },
            "messages": [{"at": "2026-10-01T10:00:00+00:00", "author": "customer", "text": "Hi"}]
        }))
        .unwrap();
        assert_eq!(d.messages[0].author, "customer");
        assert_eq!(d.ticket.merged_ids, vec![8]);
    }
}
