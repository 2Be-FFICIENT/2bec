//! End-to-end tests of `2bec completion` and `2bec man`. No network.

use crate::common::IsolatedEnv;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn cmd() -> Command {
    let mut c = Command::cargo_bin("2bec").unwrap();
    c.isolate_env();
    c
}

#[test]
fn completion_scripts_cover_the_commands() {
    for (shell, needle) in [
        ("bash", "_2bec()"),
        ("zsh", "#compdef 2bec"),
        ("fish", "complete -c 2bec"),
        ("powershell", "Register-ArgumentCompleter"),
    ] {
        cmd()
            .args(["completion", shell])
            .assert()
            .success()
            .stdout(predicate::str::contains(needle))
            .stdout(predicate::str::contains("runs"))
            .stdout(predicate::str::contains("webhooks"));
    }
}

#[test]
fn completion_needs_a_known_shell() {
    cmd()
        .args(["completion", "tcsh"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value 'tcsh'"));
}

#[test]
fn man_prints_the_main_page_without_touching_the_config() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("config.toml"), "not = [toml").unwrap();
    cmd()
        .env("TWOBEC_CONFIG_DIR", dir.path())
        .arg("man")
        .assert()
        .success()
        .stdout(predicate::str::starts_with(".ie \\n(.g"))
        .stdout(predicate::str::contains(".TH 2bec 1"))
        .stdout(predicate::str::contains("2Be\\-FFICIENT"));
}

#[test]
fn man_dir_writes_one_page_per_command() {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("man1");
    cmd()
        .args(["man", "--dir"])
        .arg(&out)
        .assert()
        .success()
        .stderr(predicate::str::contains("Wrote the manual pages to"));
    for page in [
        "2bec.1",
        "2bec-runs.1",
        "2bec-runs-list.1",
        "2bec-auth-login.1",
    ] {
        assert!(out.join(page).is_file(), "{page} missing");
    }
    let main = std::fs::read_to_string(out.join("2bec.1")).unwrap();
    assert!(!main.contains("2bec\\-help"), "help subcommand listed");
}
