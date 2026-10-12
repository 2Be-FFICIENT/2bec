//! API types and errors.

#[expect(dead_code, reason = "alert types land with their commands")]
pub mod alerts;
pub mod availability;
pub mod error;
pub mod governance;
pub mod incidents;
pub mod indicators;
pub mod interruptions;
pub mod me;
pub mod metrics;
pub mod neutralizations;
pub mod page;
pub mod people;
pub mod planning;
pub mod reports;
#[expect(
    dead_code,
    reason = "diagnosis, artifact, tag and request types land with their commands"
)]
pub mod runs;
#[expect(
    dead_code,
    reason = "group, version and calendar types land with their commands"
)]
pub mod scenarios;
pub mod scopes;
pub mod settings;
pub mod variables;
pub mod webhooks;

/// Reads an explicit `null` as the type's default, for fields the API declares nullable but the
/// client keeps as a plain value (an empty list rather than `Option<Vec<_>>`). Pair it with
/// `#[serde(default)]` so a missing field reads the same way.
pub fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    Ok(<Option<T> as serde::Deserialize>::deserialize(deserializer)?.unwrap_or_default())
}
