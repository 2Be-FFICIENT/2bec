//! API types and errors.

pub mod error;
pub mod me;
pub mod page;
#[expect(
    dead_code,
    reason = "diagnosis, artifact, tag and request types land with their commands"
)]
pub mod runs;
