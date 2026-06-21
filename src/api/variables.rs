//! `/v2/variables` types: the space's named values the alert hooks receive
//! as environment variables. A secret's value is never returned.

use serde::{Deserialize, Serialize};

/// A name is this long at most; letters, digits and underscores.
pub const MAX_NAME: usize = 128;
/// A value is this long at most.
pub const MAX_VALUE: usize = 8000;
/// A comment is this long at most.
pub const MAX_COMMENT: usize = 1000;

/// One row of `GET /v2/variables` and the body of `GET /v2/variables/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub id: String,
    pub name: String,
    pub secret: bool,
    /// `None` for a secret.
    pub value: Option<String>,
    pub comment: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub updated_by: Option<String>,
    /// Stored values so far.
    pub versions: i64,
    /// The environment variable name the hooks receive.
    pub env_name: String,
    /// The value's history, newest first, on the single read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<VariableVersion>>,
    /// What is known to read the variable, on the single read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_by: Option<Vec<VariableReader>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableVersion {
    pub at: Option<String>,
    pub by: Option<String>,
    pub value: Option<String>,
}

/// `kind` is `hook`, `incidentApi`, `splunk` or `other`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableReader {
    pub kind: String,
    pub login: Option<String>,
    pub hook: Option<String>,
}

/// `PUT /v2/variables/{name}`: `secret` is fixed at the creation; `value`
/// omitted keeps the stored one; `comment` omitted keeps, `null` clears.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableWrite {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<Option<String>>,
}

/// The error for a name, `None` when acceptable.
pub fn check_name(name: &str) -> Option<String> {
    if name.is_empty() {
        Some("a name is required".into())
    } else if name.len() > MAX_NAME {
        Some(format!("a name is {MAX_NAME} characters at most"))
    } else if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some("a name holds letters, digits and underscores only".into())
    } else {
        None
    }
}

/// The error for a value, `None` when acceptable.
pub fn check_value(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        Some("a value cannot be blank".into())
    } else if value.chars().count() > MAX_VALUE {
        Some(format!("a value is {MAX_VALUE} characters at most"))
    } else if value
        .chars()
        .any(|c| (c < ' ' && !matches!(c, '\t' | '\r' | '\n')) || c == '\u{7f}')
    {
        Some("a value holds a control character".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_omits_absent_fields_and_keeps_null_comment() {
        let w = VariableWrite {
            secret: Some(true),
            value: Some("v".into()),
            comment: Some(None),
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"secret": true, "value": "v", "comment": null})
        );
        assert_eq!(
            serde_json::to_value(VariableWrite::default()).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn rules() {
        assert!(check_name("API_KEY_2").is_none());
        assert!(check_name("").is_some());
        assert!(check_name("a-b").is_some());
        assert!(check_name(&"x".repeat(129)).is_some());
        assert!(check_value("a\tb\n").is_none());
        assert!(check_value("  ").is_some());
        assert!(check_value("a\u{1}b").is_some());
    }

    #[test]
    fn list_row_has_no_history() {
        let json = serde_json::json!({
            "id": "v", "name": "TOKEN", "secret": true, "value": null, "comment": null,
            "createdAt": "2026-01-01T00:00:00+00:00", "updatedAt": null, "updatedBy": null,
            "versions": 1, "envName": "P3_TOKEN"
        });
        let v: Variable = serde_json::from_value(json.clone()).unwrap();
        assert!(v.history.is_none());
        assert_eq!(serde_json::to_value(&v).unwrap(), json);
    }
}
