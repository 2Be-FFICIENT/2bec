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
