//! `/v2/tags` and `/v2/badges` types: the tags a run may carry and the
//! badges drawn on the runs matching a rule over their tags.

use serde::{Deserialize, Serialize};

/// A tag name is this long at most.
pub const MAX_TAG_NAME: usize = 256;
/// A tag comment is this long at most.
pub const MAX_TAG_COMMENT: usize = 2048;
/// A badge name is this long at most.
pub const MAX_BADGE_NAME: usize = 100;
/// A badge description is this long at most.
pub const MAX_BADGE_DESCRIPTION: usize = 500;
/// A text symbol is this many characters at most.
pub const MAX_TEXT_SYMBOL: usize = 2;
/// Palette keys run from 1 to this.
pub const MAX_COLOR: i64 = 7;
/// Prefixes of the names the platform reserves for its own tags.
pub const RESERVED_TAG_PREFIXES: [&str; 6] =
    ["obs::", "ws::", "wsconfig::", "#", "____:", "FindNeut"];

/// One row of `GET /v2/tags`, the body of `GET /v2/tags/{id}` and a tag of a
/// badge's rule. `kind` is `business` (the space's), `robot` (written by the
/// robots) or `global` (the platform's).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub comment: Option<String>,
    pub kind: String,
}

/// `POST /v2/tags` (both members) and `PATCH /v2/tags/{id}` (a merge patch:
/// an absent member stays, `comment: null` clears).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagWrite {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<Option<String>>,
}

/// One row of `GET /v2/badges` and the body of `GET /v2/badges/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Badge {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    /// The highest priority wins when several badges match.
    pub priority: i64,
    /// Palette key 1..7.
    pub color: i64,
    /// `text` or `icon`.
    pub symbol_kind: String,
    pub symbol: String,
    /// `any` or `all` of the tags.
    pub tag_match: String,
    /// `any`, `failed` or `success` runs.
    pub status_filter: String,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    pub tags: Vec<Tag>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// `POST /v2/badges` (name, symbol and tags required) and `PATCH /v2/badges/{id}`
/// (a merge patch: a present member replaces, `description`, `validFrom` and
/// `validUntil` set to null clear, an absent member stays).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BadgeWrite {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_match: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl BadgeWrite {
    /// Whether the patch changes anything at all.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The error for a tag name, `None` when acceptable. `allow_reserved` lets an
/// existing robot tag keep its name on a change.
pub fn check_tag_name(name: &str, allow_reserved: bool) -> Option<String> {
    if name.is_empty() {
        Some("a name is required".into())
    } else if name.chars().count() > MAX_TAG_NAME {
        Some(format!("a name is {MAX_TAG_NAME} characters at most"))
    } else if name.chars().any(char::is_control) {
        Some("a name cannot hold control characters".into())
    } else if !allow_reserved && RESERVED_TAG_PREFIXES.iter().any(|p| name.starts_with(p)) {
        Some(format!(
            "names starting with {} are reserved for the platform",
            RESERVED_TAG_PREFIXES.join(", ")
        ))
    } else {
        None
    }
}

/// The error for a badge name, `None` when acceptable.
pub fn check_badge_name(name: &str) -> Option<String> {
    if name.is_empty() {
        Some("a name is required".into())
    } else if name.chars().count() > MAX_BADGE_NAME {
        Some(format!("a name is {MAX_BADGE_NAME} characters at most"))
    } else if name.chars().any(char::is_control) {
        Some("a name cannot hold control characters".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_write_omits_absent_fields_and_keeps_null_comment() {
        let w = TagWrite {
            name: None,
            comment: Some(None),
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"comment": null})
        );
        assert_eq!(
            serde_json::to_value(TagWrite::default()).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn badge_write_omits_absent_fields() {
        let w = BadgeWrite {
            name: Some("Prod".into()),
            valid_until: Some(None),
            tags: Some(vec!["t".into()]),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&w).unwrap(),
            serde_json::json!({"name": "Prod", "validUntil": null, "tags": ["t"]})
        );
        assert!(BadgeWrite::default().is_empty());
        assert!(!w.is_empty());
    }

    #[test]
    fn tag_name_rules() {
        assert!(check_tag_name("release 42", false).is_none());
        assert!(check_tag_name("", false).is_some());
        assert!(check_tag_name("obs::x", false).is_some());
        assert!(check_tag_name("obs::x", true).is_none());
        assert!(check_tag_name("#x", false).is_some());
        assert!(check_tag_name("a\u{1}b", false).is_some());
        assert!(check_tag_name(&"x".repeat(257), false).is_some());
    }

    #[test]
    fn badge_name_rules() {
        assert!(check_badge_name("Prod").is_none());
        assert!(check_badge_name("").is_some());
        assert!(check_badge_name(&"x".repeat(101)).is_some());
    }

    #[test]
    fn badge_round_trips() {
        let json = serde_json::json!({
            "id": "b", "name": "Prod", "description": null, "enabled": true, "priority": 10,
            "color": 3, "symbolKind": "text", "symbol": "P", "tagMatch": "any",
            "statusFilter": "failed", "validFrom": null, "validUntil": null,
            "tags": [{"id": "t", "name": "prod", "comment": null, "kind": "business"}],
            "createdAt": "2026-01-01T00:00:00+00:00", "updatedAt": null
        });
        let b: Badge = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(b.tags[0].name, "prod");
        assert_eq!(serde_json::to_value(&b).unwrap(), json);
    }
}
