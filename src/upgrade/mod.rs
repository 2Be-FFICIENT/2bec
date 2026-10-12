//! Self-upgrade: the release feed the installers read, the archive checks
//! and the in-place replacement of the running binary.

#![expect(dead_code, reason = "used by the upgrade command once it is wired in")]

pub mod archive;
pub mod feed;
pub mod replace;
