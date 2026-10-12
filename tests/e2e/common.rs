//! Helpers shared by the end-to-end tests.

/// Starts a child process from an empty environment while keeping what the
/// operating system and the tools it shells out to need.
pub trait IsolatedEnv {
    /// Clears the environment like `env_clear`, then puts back:
    /// - `PATH`, so helpers such as `jq` resolve on every platform;
    /// - `SYSTEMROOT` on Windows: without it Winsock cannot initialize and
    ///   every socket call in the child fails with "error sending request".
    fn isolate_env(&mut self) -> &mut Self;
}

fn keep_essentials(mut set: impl FnMut(&str, std::ffi::OsString)) {
    if let Some(path) = std::env::var_os("PATH") {
        set("PATH", path);
    }
    if cfg!(windows)
        && let Some(root) = std::env::var_os("SYSTEMROOT")
    {
        set("SYSTEMROOT", root);
    }
}

impl IsolatedEnv for std::process::Command {
    fn isolate_env(&mut self) -> &mut Self {
        self.env_clear();
        keep_essentials(|k, v| {
            self.env(k, v);
        });
        self
    }
}

impl IsolatedEnv for assert_cmd::Command {
    fn isolate_env(&mut self) -> &mut Self {
        self.env_clear();
        keep_essentials(|k, v| {
            self.env(k, v);
        });
        self
    }
}
