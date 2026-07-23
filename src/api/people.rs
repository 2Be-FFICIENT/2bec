//! `/v2/users`, `/v2/profiles` and `/v2/security-settings` types: the
//! space's users, the access profiles deciding which scenarios they see,
//! and the sign-in policy. Read only.

use serde::{Deserialize, Serialize};

/// The `status` values of a user and of the list filter.
pub const STATUSES: [&str; 3] = ["active", "locked", "disabled"];
/// The roles a user may be granted, as `roles` and the `role` filter spell them.
pub const ROLES: [&str; 5] = [
    "Alert Admin",
    "Scenario Admin",
    "API Admin",
    "User Admin",
    "Edit Admin",
];

/// An access profile named by its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRef {
    pub id: String,
    pub name: String,
}

/// One row of `GET /v2/users` and the body of `GET /v2/users/{id}`; the
/// detail-only members are absent from the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// The identity subject, the one the administration history names.
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub job_title: Option<String>,
    pub phone: Option<String>,
    pub mobile: Option<String>,
    /// Roles granted one by one; an account administrator holds every role.
    pub roles: Vec<String>,
    pub account_admin: bool,
    /// `active`, `locked` or `disabled`.
    pub status: String,
    pub profile: ProfileRef,
    /// May sign in through the company's identity provider.
    pub external_auth: bool,
    /// The password sign-in is off (detail only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_auth_only: Option<bool>,
    /// Strong authentication is not asked of them (detail only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub two_factor_exempt: Option<bool>,
    /// The language of their mails (detail only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// While locked, since when (detail only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked_at: Option<String>,
    pub last_sign_in_at: Option<String>,
    pub last_activity_at: Option<String>,
}

/// A scenario group linked to a profile: `access` when the link opens the
/// group's scenarios to the profile's users.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileGroupAccess {
    pub group: ProfileRef,
    pub access: bool,
}

/// A user holding a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileMember {
    pub id: String,
    pub display_name: String,
    pub login: String,
    pub status: String,
}

/// One row of `GET /v2/profiles` (`member_count`, no `members`) and the body
/// of `GET /v2/profiles/{id}` (`members`, no `member_count`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// The profile a first sign-in through the identity provider gets.
    pub sso_default: bool,
    /// The scenarios its open groups hold.
    pub scenario_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_count: Option<i64>,
    pub groups: Vec<ProfileGroupAccess>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<ProfileMember>>,
}

impl Profile {
    /// The holders, from either shape.
    pub fn member_count(&self) -> i64 {
        self.member_count
            .or_else(|| self.members.as_ref().map(|m| m.len() as i64))
            .unwrap_or(0)
    }
}

/// The password rules; `renew_days` is read when `renew_regularly` is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordPolicy {
    pub min_length: i64,
    pub require_upper_and_lower: bool,
    pub require_digit: bool,
    pub special_char_count: i64,
    pub forbid_reuse: bool,
    pub renew_regularly: bool,
    pub renew_days: i64,
}

/// Strong authentication: on or off, the channels, whom it is asked of, how
/// many days a verified device stays trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwoFactorPolicy {
    pub enabled: bool,
    pub sms: bool,
    pub email: bool,
    pub users: bool,
    pub admins: bool,
    pub trusted_device_days: i64,
}

/// `GET /v2/security-settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettings {
    pub password: PasswordPolicy,
    pub two_factor: TwoFactorPolicy,
    pub user_count: i64,
    pub two_factor_exempt_count: i64,
    pub without_mobile_count: i64,
    pub without_email_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_user_has_no_detail_members() {
        let json = serde_json::json!({
            "id": "u", "login": "alice", "displayName": "Alice", "firstName": "Alice",
            "lastName": null, "email": "a@x.io", "jobTitle": null, "phone": null, "mobile": null,
            "roles": ["User Admin"], "accountAdmin": false, "status": "active",
            "profile": {"id": "p", "name": "Ops"}, "externalAuth": true,
            "lastSignInAt": "2026-10-01T10:00:00+00:00", "lastActivityAt": null
        });
        let u: User = serde_json::from_value(json.clone()).unwrap();
        assert!(u.language.is_none());
        assert_eq!(serde_json::to_value(&u).unwrap(), json);
    }

    #[test]
    fn profile_members_from_either_shape() {
        let list: Profile = serde_json::from_value(serde_json::json!({
            "id": "p", "name": "Ops", "ssoDefault": false, "scenarioCount": 3,
            "memberCount": 2, "groups": []
        }))
        .unwrap();
        assert_eq!(list.member_count(), 2);
        let detail: Profile = serde_json::from_value(serde_json::json!({
            "id": "p", "name": "Ops", "ssoDefault": false, "scenarioCount": 3,
            "groups": [{"group": {"id": "g", "name": "Web"}, "access": true}],
            "members": [{"id": "u", "displayName": "Alice", "login": "alice", "status": "active"}]
        }))
        .unwrap();
        assert_eq!(detail.member_count(), 1);
        assert!(detail.groups[0].access);
    }
}
