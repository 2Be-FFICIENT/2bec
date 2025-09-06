//! `GET /v2/me`: who the current credential is.
//!
//! No scope is needed; any active API key or person token can call it, which
//! makes it the verification step of `auth login` and the body of
//! `auth status`.

use serde::{Deserialize, Serialize};

/// Path of the endpoint, relative to the API URL.
pub const PATH: &str = "v2/me";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    /// `"key"` or `"person"`.
    pub caller: String,
    pub tenant: Tenant,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Key>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<Person>,
    pub rate_limit: RateLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tenant {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Key {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub prefix: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub profile: Profile,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    pub reset_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_caller_parses() {
        let json = r#"{"caller":"key","tenant":{"id":"t1","name":"Acme"},
            "key":{"id":"k1","name":"ci","description":null,"prefix":"2bef_live_ab",
                   "scopes":["runs:read"],"profile":{"id":"p1","name":"All"},
                   "createdAt":"2026-01-01T00:00:00+00:00","expiresAt":null,"lastUsedAt":null},
            "person":null,
            "rateLimit":{"limit":600,"remaining":599,"resetAt":"2026-01-01T00:01:00+00:00"}}"#;
        let me: Me = serde_json::from_str(json).unwrap();
        assert_eq!(me.caller, "key");
        assert_eq!(me.tenant.name, "Acme");
        let key = me.key.unwrap();
        assert_eq!(key.scopes, ["runs:read"]);
        assert_eq!(key.description, None);
        assert!(me.person.is_none());
        assert_eq!(me.rate_limit.remaining, 599);
    }

    #[test]
    fn person_caller_parses() {
        let json = r#"{"caller":"person","tenant":{"id":"t1","name":"Acme"},
            "person":{"id":"u1","name":"Ada","email":"ada@example.com",
                      "roles":["Admin"],"scopes":["runs:read","runs:write"],"client":"claude"},
            "rateLimit":{"limit":600,"remaining":1,"resetAt":"x"}}"#;
        let me: Me = serde_json::from_str(json).unwrap();
        let p = me.person.unwrap();
        assert_eq!(p.client.as_deref(), Some("claude"));
        assert_eq!(p.scopes.len(), 2);
    }
}
