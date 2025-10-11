//! API types and errors.

pub mod error;
pub mod me;
#[expect(dead_code, reason = "first user is `runs list`")]
pub mod page;
#[expect(dead_code, reason = "used by the `runs` commands")]
pub mod runs;
