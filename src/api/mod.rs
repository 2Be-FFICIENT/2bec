//! API types and errors.

#[expect(dead_code, reason = "alert types land with their commands")]
pub mod alerts;
pub mod error;
pub mod me;
#[expect(dead_code, reason = "metrics types land with their commands")]
pub mod metrics;
pub mod page;
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
