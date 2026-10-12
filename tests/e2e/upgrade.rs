//! End-to-end tests of `2bec upgrade` against a mock release feed. The
//! command runs on a copy of the built binary, so the swap never touches
//! the one cargo built. No network.

use crate::common::IsolatedEnv;
use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// This build's platform, as the feed and the archives name it.
const PLATFORM: &str = if cfg!(target_os = "linux") {
    if cfg!(target_arch = "x86_64") {
        "linux-x64"
    } else {
        "linux-arm64"
    }
} else if cfg!(target_os = "macos") {
    if cfg!(target_arch = "x86_64") {
        "macos-x64"
    } else {
        "macos-arm64"
    }
} else if cfg!(target_arch = "x86_64") {
    "windows-x64"
} else {
    "windows-arm64"
};

const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// A copy of the built binary in `<dir>/bin`, as the installer lays it out.
fn install_copy(dir: &TempDir) -> PathBuf {
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let target = bin.join(if cfg!(windows) { "2bec.exe" } else { "2bec" });
    std::fs::copy(assert_cmd::cargo::cargo_bin("2bec"), &target).unwrap();
    // A sibling test forking while the copy was open for writing hands its
    // child the write handle until it execs, and Linux refuses to run a file
    // that is open for writing ("text file busy"): wait that moment out.
    for _ in 0..100 {
        match std::process::Command::new(&target)
            .arg("--version")
            .output()
        {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            other => {
                other.unwrap();
                break;
            }
        }
    }
    target
}

fn cmd(binary: &Path, server: &MockServer, home: &TempDir) -> Command {
    let mut c = Command::new(binary);
    c.isolate_env()
        .env("HOME", home.path())
        .env("TWOBEC_CONFIG_DIR", home.path().join("config"))
        .env("TWOBEC_INSTALL_URL", server.uri());
    c
}

fn archive_name(version: &str) -> String {
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    format!("2bec-{version}-{PLATFORM}.{ext}")
}

/// A release archive as the pipeline packs it, holding a stand-in binary
/// that prints `2bec <version>` whatever it is asked.
#[cfg(unix)]
fn pack(version: &str) -> Vec<u8> {
    use std::io::Write as _;
    let folder = format!("2bec-{version}-{PLATFORM}");
    let body = format!("#!/bin/sh\necho \"2bec {version}\"\n");
    let mut buf = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, format!("{folder}/2bec"), body.as_bytes())
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap().flush().unwrap();
    }
    buf
}

#[cfg(unix)]
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Serve `version` as the latest release, with `archive` as its archive of
/// this platform and `sums` as its checksums.
async fn serve(server: &MockServer, version: &str, archive: Vec<u8>, sums: String) {
    let name = archive_name(version);
    Mock::given(method("GET"))
        .and(path("/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "version": version,
            "publishedAt": "2026-10-12T10:00:00+00:00",
            "notes": format!("https://github.com/2Be-FFICIENT/2bec/releases/tag/v{version}"),
            "assets": { PLATFORM: format!("{}/releases/{version}/{name}", server.uri()) },
            "checksums": format!("{}/releases/{version}/SHA256SUMS", server.uri()),
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/releases/{version}/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/releases/{version}/SHA256SUMS")))
        .respond_with(ResponseTemplate::new(200).set_body_string(sums))
        .mount(server)
        .await;
}

#[tokio::test]
async fn check_reports_a_newer_release_and_changes_nothing() {
    let server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let binary = install_copy(&home);
    let before = std::fs::read(&binary).unwrap();
    serve(&server, "9.9.9", b"not needed".to_vec(), String::new()).await;

    cmd(&binary, &server, &home)
        .args(["upgrade", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "2bec 9.9.9 is available (you have {CURRENT}); run `2bec upgrade`"
        )))
        .stdout(predicate::str::contains("releases/tag/v9.9.9"));
    cmd(&binary, &server, &home)
        .args(["upgrade", "--check", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""upToDate":false"#))
        .stdout(predicate::str::contains(r#""target":"9.9.9""#))
        .stdout(predicate::str::contains(format!(
            r#""platform":"{PLATFORM}""#
        )));
    assert_eq!(std::fs::read(&binary).unwrap(), before);
}

#[tokio::test]
async fn an_up_to_date_binary_downloads_nothing() {
    let server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let binary = install_copy(&home);
    serve(&server, CURRENT, b"not needed".to_vec(), String::new()).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/releases/{CURRENT}/{}",
            archive_name(CURRENT)
        )))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    cmd(&binary, &server, &home)
        .arg("upgrade")
        .assert()
        .success()
        .stdout(format!("2bec {CURRENT} is up to date\n"));
    cmd(&binary, &server, &home)
        .args(["upgrade", "--check", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""upToDate":true"#));
    // Pinned to the version already installed: nothing to do either.
    cmd(&binary, &server, &home)
        .args(["upgrade", "--version", CURRENT])
        .assert()
        .success()
        .stdout(format!("2bec {CURRENT} is up to date\n"));
}

#[tokio::test]
async fn a_feed_that_does_not_answer_is_an_api_error() {
    let server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let binary = install_copy(&home);
    Mock::given(method("GET"))
        .and(path("/releases/latest"))
        .respond_with(
            ResponseTemplate::new(503)
                .insert_header("content-type", "application/problem+json")
                .set_body_string(r#"{"type":"urn:pegas:problem:cli-unavailable","title":"The CLI feed is not configured","status":503}"#),
        )
        .mount(&server)
        .await;

    cmd(&binary, &server, &home)
        .args(["upgrade", "--check"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("the release feed did not answer"))
        .stderr(predicate::str::contains(server.uri()));
}

#[test]
fn a_bad_version_is_a_usage_error() {
    let home = TempDir::new().unwrap();
    let binary = install_copy(&home);
    Command::new(&binary)
        .isolate_env()
        .env("TWOBEC_INSTALL_URL", "http://127.0.0.1:9")
        .args(["upgrade", "--version", "latest"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "'latest' is not a version (expected 0.1.0)",
        ));
}

#[cfg(unix)]
#[tokio::test]
async fn upgrades_the_binary_in_place_and_refreshes_the_extras() {
    let server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let binary = install_copy(&home);
    let archive = pack("9.9.9");
    let sums = format!("{}  {}\n", sha256(&archive), archive_name("9.9.9"));
    serve(&server, "9.9.9", archive, sums).await;
    // The share beside the bin, as install.sh lays it out: bash and the
    // manual pages are there, fish is not.
    let share = home.path().join("share");
    let bash = share.join("bash-completion/completions/2bec");
    let man1 = share.join("man/man1");
    std::fs::create_dir_all(bash.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&man1).unwrap();
    std::fs::write(&bash, "old").unwrap();
    std::fs::write(man1.join("2bec.1"), "old").unwrap();

    cmd(&binary, &server, &home)
        .arg("upgrade")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Upgraded 2bec from {CURRENT} to 9.9.9 (2bec 9.9.9 at {}",
            // The command names the canonical path (macOS: /private/var/...).
            binary.canonicalize().unwrap().display()
        )))
        .stdout(predicate::str::contains(
            "Refreshed the completions and manual pages",
        ))
        .stdout(predicate::str::contains("releases/tag/v9.9.9"))
        .stderr(predicate::str::contains(format!(
            "Downloading 2bec 9.9.9 for {PLATFORM}"
        )));

    let out = std::process::Command::new(&binary)
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2bec 9.9.9");
    assert_eq!(std::fs::read_to_string(&bash).unwrap(), "2bec 9.9.9\n");
    assert!(!share.join("fish").exists());
    let leftovers: Vec<_> = std::fs::read_dir(binary.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, vec![std::ffi::OsString::from("2bec")]);
}

#[cfg(unix)]
#[tokio::test]
async fn a_corrupt_archive_leaves_the_binary_alone() {
    let server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let binary = install_copy(&home);
    let before = std::fs::read(&binary).unwrap();
    let mut archive = pack("9.9.9");
    let sums = format!("{}  {}\n", sha256(&archive), archive_name("9.9.9"));
    archive.extend_from_slice(b"garbage");
    serve(&server, "9.9.9", archive, sums).await;

    cmd(&binary, &server, &home)
        .arg("upgrade")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("checksum mismatch"))
        .stderr(predicate::str::contains("corrupt or tampered"));
    assert_eq!(std::fs::read(&binary).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(binary.parent().unwrap()).unwrap().count(),
        1,
        "the staging directory was not removed"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_pinned_version_comes_from_the_fixed_layout() {
    let server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let binary = install_copy(&home);
    // The latest is 9.9.9, but 0.0.1 is asked for: a downgrade, from its own directory.
    serve(&server, "9.9.9", b"not needed".to_vec(), String::new()).await;
    let archive = pack("0.0.1");
    let name = archive_name("0.0.1");
    let sums = format!("{}  {name}\n", sha256(&archive));
    Mock::given(method("GET"))
        .and(path(format!("/releases/0.0.1/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/releases/0.0.1/SHA256SUMS"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sums))
        .expect(1)
        .mount(&server)
        .await;

    cmd(&binary, &server, &home)
        .args(["upgrade", "--version", "v0.0.1", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""action":"downgraded""#))
        .stdout(predicate::str::contains(r#""target":"0.0.1""#));
    let out = std::process::Command::new(&binary)
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2bec 0.0.1");
}
