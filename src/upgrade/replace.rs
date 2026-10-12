//! Replacing the running binary: where it is, whether its directory can be
//! written, and the swap itself, atomic on unix and rename-based on Windows,
//! where a running executable cannot be overwritten but can be renamed away.

use anyhow::{Context as _, Result, bail};
use std::path::{Path, PathBuf};

/// The file this process runs from, symlinks resolved, so a link in
/// `/usr/local/bin` to the real binary elsewhere upgrades the real binary.
pub fn current_binary() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("cannot tell where 2bec runs from")?;
    std::fs::canonicalize(&exe).with_context(|| format!("cannot resolve {}", exe.display()))
}

/// Fail early when `dir` cannot take a new file, with the way out.
pub fn check_writable(dir: &Path) -> Result<()> {
    let probe = dir.join(format!(".2bec-upgrade-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => bail!(
            "cannot write to {}: run `sudo 2bec upgrade`, or re-run the installer",
            dir.display()
        ),
        Err(e) => Err(e).with_context(|| format!("cannot write to {}", dir.display())),
    }
}

/// The name the previous binary keeps on Windows until the next start.
#[cfg(windows)]
pub fn leftover(target: &Path) -> PathBuf {
    let mut old = target.as_os_str().to_owned();
    old.push(".old");
    PathBuf::from(old)
}

/// Delete the previous binary a Windows upgrade renamed away, if any and if
/// nothing runs it any more; silent otherwise.
#[cfg(windows)]
pub fn remove_leftover() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(leftover(&exe));
    }
}

/// Put `new` in the place of `target`, both in the same directory.
#[cfg(unix)]
pub fn install(new: &Path, target: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("cannot make {} executable", new.display()))?;
    std::fs::rename(new, target).with_context(|| format!("cannot replace {}", target.display()))?;
    Ok(())
}

/// Put `new` in the place of `target`, both in the same directory: the
/// running `target` is renamed to `<target>.old` first, and put back when
/// the new one cannot take its place.
#[cfg(windows)]
pub fn install(new: &Path, target: &Path) -> Result<()> {
    let old = leftover(target);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old)
        .with_context(|| format!("cannot move {} aside", target.display()))?;
    if let Err(e) = std::fs::rename(new, target) {
        let _ = std::fs::rename(&old, target);
        return Err(e).with_context(|| format!("cannot replace {}", target.display()));
    }
    // Still mapped by this process; `remove_leftover` gets it next time.
    let _ = std::fs::remove_file(&old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_binary_is_a_file() {
        let exe = current_binary().unwrap();
        assert!(exe.is_file(), "{}", exe.display());
        assert!(exe.is_absolute());
    }

    #[test]
    fn writable_directory_passes_and_leaves_no_probe() {
        let dir = tempfile::tempdir().unwrap();
        check_writable(dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn missing_directory_fails() {
        let dir = tempfile::tempdir().unwrap();
        let err = check_writable(&dir.path().join("nowhere")).unwrap_err();
        assert!(err.to_string().contains("cannot write to"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn read_only_directory_names_sudo() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return; // root writes anywhere
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let err = check_writable(dir.path()).unwrap_err();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(err.to_string().contains("sudo 2bec upgrade"), "{err}");
    }

    #[test]
    fn install_replaces_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("2bec");
        let new = dir.path().join("2bec.new");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        install(&new, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!new.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "{mode:o}");
        }
    }
}
