//! End-to-end tests of `2bec config` against an isolated config dir. No network.

use crate::common::IsolatedEnv;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn cmd(dir: &TempDir) -> Command {
    let mut c = Command::cargo_bin("2bec").unwrap();
    c.isolate_env().env("TWOBEC_CONFIG_DIR", dir.path());
    c
}

fn config_text(dir: &TempDir) -> String {
    std::fs::read_to_string(dir.path().join("config.toml")).unwrap_or_default()
}

#[test]
fn path_points_into_the_config_dir() {
    let dir = TempDir::new().unwrap();
    cmd(&dir)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config.toml"))
        .stdout(predicate::str::contains(dir.path().to_str().unwrap()));
}

#[test]
fn set_get_unset_round_trip_in_the_active_profile() {
    let dir = TempDir::new().unwrap();
    cmd(&dir)
        .args([
            "--profile",
            "dev",
            "config",
            "set",
            "api_url",
            "http://localhost:5152/",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Set api_url = http://localhost:5152 for profile `dev`",
        ));
    assert!(config_text(&dir).contains("[profiles.dev]\napi_url = \"http://localhost:5152\""));

    cmd(&dir)
        .args(["--profile", "dev", "config", "get", "api_url"])
        .assert()
        .success()
        .stdout("http://localhost:5152\n");
    // The default profile is untouched.
    cmd(&dir)
        .args(["config", "get", "api_url"])
        .assert()
        .success()
        .stdout("https://api.2befficient.com\n");

    cmd(&dir)
        .args(["--profile", "dev", "config", "unset", "api_url"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Removed api_url from profile `dev`",
        ));
    assert!(
        !config_text(&dir).contains("profiles.dev"),
        "{}",
        config_text(&dir)
    );
    cmd(&dir)
        .args(["--profile", "dev", "config", "unset", "api_url"])
        .assert()
        .success()
        .stdout(predicate::str::contains("api_url is not set"));
}

#[test]
fn set_rejects_a_bad_url_and_an_unknown_key() {
    let dir = TempDir::new().unwrap();
    cmd(&dir)
        .args(["config", "set", "api_url", "localhost:5152"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("api_url must be an http(s) URL"));
    cmd(&dir)
        .args(["config", "set", "colour", "blue"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value 'colour'"));
    assert!(!dir.path().join("config.toml").exists());
}

#[test]
fn get_prints_the_effective_value_and_list_names_the_source() {
    let dir = TempDir::new().unwrap();
    cmd(&dir)
        .args(["config", "set", "api_url", "http://file.example"])
        .assert()
        .success();
    cmd(&dir)
        .env("TWOBEC_API_URL", "http://env.example")
        .args(["config", "get", "api_url"])
        .assert()
        .success()
        .stdout("http://env.example\n");
    cmd(&dir)
        .env("TWOBEC_API_URL", "http://env.example")
        .args(["config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "api_url\thttp://env.example\tflag or environment",
        ))
        .stdout(predicate::str::contains("oauth_client_id\t2bec\tdefault"));
    cmd(&dir)
        .args(["config", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""api_url":"http://file.example""#,
        ))
        .stdout(predicate::str::contains(r#""api_url":"config file""#));
}

#[test]
fn profile_use_sets_the_default_and_list_marks_it() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("hosts.toml"),
        "[profiles.prod]\ntoken = \"2bef_live_x\"\n",
    )
    .unwrap();
    cmd(&dir)
        .args(["config", "profile", "use", "dev"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Default profile is now `dev`"))
        .stderr(predicate::str::contains("no credential stored for `dev`"));
    assert!(config_text(&dir).contains("default_profile = \"dev\""));

    cmd(&dir)
        .args(["--profile", "prod", "config", "profile", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "dev\thttps://api.2befficient.com\tno\tdefault\n",
        ))
        .stdout(predicate::str::contains(
            "prod\thttps://api.2befficient.com\tyes\tactive\n",
        ));

    cmd(&dir)
        .args(["config", "profile", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""name":"dev""#))
        .stdout(predicate::str::contains(r#""credential":true"#));
}

#[test]
fn a_broken_config_file_is_reported_not_overwritten() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("config.toml"), "not = [toml").unwrap();
    cmd(&dir)
        .args(["config", "set", "api_url", "http://x"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("invalid config file"));
    assert_eq!(config_text(&dir), "not = [toml");
}

#[test]
fn credential_store_is_a_top_level_setting() {
    let dir = TempDir::new().unwrap();
    cmd(&dir)
        .args(["config", "set", "credential_store", "vault"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unknown credential store `vault`"));
    cmd(&dir)
        .args([
            "--profile",
            "dev",
            "config",
            "set",
            "credential_store",
            "keyring",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Credential store is now `keyring`",
        ));
    assert!(config_text(&dir).starts_with("credential_store = \"keyring\"\n"));
    assert!(!config_text(&dir).contains("profiles"));
    cmd(&dir)
        .args(["config", "get", "credential_store"])
        .assert()
        .success()
        .stdout("keyring\n");
    // The config group keeps working even if the keyring cannot be opened.
    cmd(&dir)
        .args(["config", "unset", "credential_store"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed credential_store"));
    cmd(&dir)
        .args(["config", "get", "credential_store"])
        .assert()
        .success()
        .stdout("file\n");
}
