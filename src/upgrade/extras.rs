//! The completions and manual pages `install.sh` puts beside the binary,
//! regenerated from the new binary after an upgrade so they never lag it.
//! Only files that are already there are rewritten; nothing is created in
//! a new location, and the Windows installer puts none.

use std::path::{Path, PathBuf};

/// Where the installer may have put them: the `share` beside a `bin`
/// (`/usr/local/bin` to `/usr/local/share`) and the user's own.
pub fn share_dirs(binary: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(bin) = binary.parent()
        && bin.file_name().is_some_and(|n| n == "bin")
        && let Some(prefix) = bin.parent()
    {
        dirs.push(prefix.join("share"));
    }
    if let Some(home) = dirs::home_dir() {
        let own = home.join(".local").join("share");
        if !dirs.contains(&own) {
            dirs.push(own);
        }
    }
    dirs
}

/// Rewrite the installed completions and manual pages under `share` from
/// `binary`; the files refreshed, and the ones that could not be.
pub fn refresh(binary: &Path, share: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut done = Vec::new();
    let mut failed = Vec::new();
    let completions = [
        ("bash", share.join("bash-completion/completions/2bec")),
        ("zsh", share.join("zsh/site-functions/_2bec")),
        ("fish", share.join("fish/vendor_completions.d/2bec.fish")),
    ];
    for (shell, path) in completions {
        if !path.is_file() {
            continue;
        }
        match generate(binary, &["completion", shell])
            .and_then(|script| std::fs::write(&path, script).map_err(|e| e.to_string()))
        {
            Ok(()) => done.push(path),
            Err(e) => failed.push(format!("{}: {e}", path.display())),
        }
    }
    let man1 = share.join("man/man1");
    if man1.join("2bec.1").is_file() {
        let dir = man1.to_string_lossy().into_owned();
        match generate(binary, &["man", "--dir", &dir]) {
            Ok(_) => done.push(man1),
            Err(e) => failed.push(format!("{}: {e}", man1.display())),
        }
    }
    (done, failed)
}

/// Run the binary and keep its stdout.
fn generate(binary: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = std::process::Command::new(binary)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", binary.display()))?;
    if !output.status.success() {
        return Err(format!(
            "`2bec {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_beside_a_bin_comes_first() {
        let dirs = share_dirs(Path::new("/usr/local/bin/2bec"));
        assert_eq!(dirs[0], Path::new("/usr/local/share"));
        let dirs = share_dirs(Path::new("/opt/2bec/2bec"));
        assert!(
            dirs.iter().all(|d| d != Path::new("/opt/share")),
            "{dirs:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn refreshes_only_what_is_there() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("2bec");
        std::fs::write(&binary, "#!/bin/sh\nif [ \"$1\" = man ]; then echo page > \"$3/2bec.1\"; echo page > \"$3/2bec-runs.1\"; exit 0; fi\necho \"$1 $2\"\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let share = dir.path().join("share");
        let bash = share.join("bash-completion/completions/2bec");
        let man = share.join("man/man1");
        std::fs::create_dir_all(bash.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&man).unwrap();
        std::fs::write(&bash, "old").unwrap();
        std::fs::write(man.join("2bec.1"), "old").unwrap();

        let (done, failed) = refresh(&binary, &share);
        assert!(failed.is_empty(), "{failed:?}");
        assert_eq!(done, vec![bash.clone(), man.clone()]);
        assert_eq!(std::fs::read_to_string(&bash).unwrap(), "completion bash\n");
        assert_eq!(
            std::fs::read_to_string(man.join("2bec.1")).unwrap(),
            "page\n"
        );
        assert!(man.join("2bec-runs.1").is_file());
        assert!(!share.join("zsh").exists());
        assert!(!share.join("fish").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_generator_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("2bec");
        std::fs::write(&binary, "#!/bin/sh\necho boom >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let share = dir.path().join("share");
        let fish = share.join("fish/vendor_completions.d/2bec.fish");
        std::fs::create_dir_all(fish.parent().unwrap()).unwrap();
        std::fs::write(&fish, "old").unwrap();
        let (done, failed) = refresh(&binary, &share);
        assert!(done.is_empty());
        assert_eq!(failed.len(), 1);
        assert!(failed[0].contains("boom"), "{failed:?}");
        assert_eq!(std::fs::read_to_string(&fish).unwrap(), "old");
    }
}
