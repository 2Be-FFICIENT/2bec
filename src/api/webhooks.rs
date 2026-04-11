//! `/v2/webhooks`, `/v2/webhook-events` and the deliveries log types.

use serde::{Deserialize, Serialize};

/// The event kinds a webhook subscribes to, as `GET /v2/webhook-events`
/// lists them; checked locally before a write so a typo never reaches the API.
pub const EVENTS: [&str; 10] = [
    "run.failed",
    "run.recovered",
    "incident.opened",
    "incident.closed",
    "alert.sent",
    "interruption.started",
    "interruption.ended",
    "pause.started",
    "pause.ended",
    "report.delivered",
];

/// Delivery statuses the list filter accepts.
pub const DELIVERY_STATUSES: [&str; 3] = ["pending", "delivered", "failed"];

/// `url` and `description` are this long at most.
pub const MAX_TEXT: usize = 400;

/// One row of `GET /v2/webhooks` and the body of `GET /v2/webhooks/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    pub id: String,
    pub url: String,
    pub events: Vec<String>,
    pub description: Option<String>,
    pub active: bool,
    /// `active`, `inactive` (switched off by a key) or `disabled` (by the platform).
    pub state: String,
    /// `failing`, `key-revoked` or `key-expired` when disabled.
    pub disabled_reason: Option<String>,
    pub disabled_at: Option<String>,
    pub api_key: KeyRef,
    pub last_delivery_at: Option<String>,
    /// HTTP status of the last attempt; 0 when the endpoint could not be reached.
    pub last_delivery_status: Option<i64>,
    pub previous_secret_until: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// The key a webhook reads with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyRef {
    pub id: String,
    pub name: String,
}

/// `POST /v2/webhooks` body and the fields of the merge patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookWrite {
    pub url: String,
    pub events: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub active: bool,
}

/// The answer of a creation and of `:rotate-secret`: the secret, shown this once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookSecret {
    pub webhook: Webhook,
    /// `whsec_…`, the HMAC-SHA256 key of `X-Pegas-Signature`.
    pub secret: String,
}

/// One row of `GET /v2/webhook-events`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventKind {
    pub code: String,
    pub scope: String,
    pub description: String,
}

/// One row of `GET /v2/webhooks/{id}/deliveries`, and the answer of `:ping`
/// and `:redeliver`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub id: String,
    pub event: String,
    pub occurred_at: String,
    /// `pending`, `delivered` or `failed`.
    pub status: String,
    pub attempts: i64,
    pub next_attempt_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub response_status: Option<i64>,
    pub error: Option<String>,
    pub duration_ms: Option<i64>,
    pub delivered_at: Option<String>,
    pub created_at: String,
}

/// `GET /v2/webhooks/{id}/deliveries/{deliveryId}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryDetail {
    pub delivery: Delivery,
    pub body: Envelope,
}

/// The body of every delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub at: String,
    pub webhook: String,
    pub data: serde_json::Value,
    pub links: Option<Links>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    pub resource: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_omits_an_absent_description() {
        let w = WebhookWrite {
            url: "https://example.com/hook".into(),
            events: vec!["run.failed".into()],
            description: None,
            active: true,
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"url": "https://example.com/hook", "events": ["run.failed"], "active": true})
        );
    }

    #[test]
    fn envelope_keeps_the_type_field() {
        let e: Envelope = serde_json::from_value(serde_json::json!({
            "id": "e1", "type": "ping", "at": "2026-10-12T10:00:00+02:00", "webhook": "w1",
            "data": {"message": "hi"}, "links": null
        }))
        .unwrap();
        assert_eq!(e.kind, "ping");
        assert_eq!(serde_json::to_value(&e).unwrap()["type"], "ping");
    }
}
