//! `2bec upgrade`: replace this binary with a release of the feed, after
//! checking the archive against `SHA256SUMS`, as the installers do.

use anyhow::{Context as _, Result, bail};
use clap::Args;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::output::OutputArgs;
use crate::upgrade::feed::{DEFAULT_FEED, Feed, PLATFORM, Release, Version};
use crate::upgrade::{archive, extras, replace};

#[derive(Args, Debug)]
#[command(after_help = "\
Examples:
  2bec upgrade                     # the latest release of the feed
  2bec upgrade --check             # say whether a newer release exists, change nothing
  2bec upgrade --check --json      # the same for scripts: {\"upToDate\": ...}
  2bec upgrade --version 0.1.0     # pin a version (also downgrades)
  sudo 2bec upgrade                # when the binary sits in a root-owned directory

The feed is the API's (https://api.2befficient.com/cli); --feed or
TWOBEC_INSTALL_URL name another one, as for the installer. The archive is
checked against the SHA256SUMS of the release before the binary is replaced
in place; the completions and manual pages the installer put under share/
are regenerated when they are there.")]
pub struct UpgradeArgs {
    /// Report whether a newer release exists; change nothing
    #[arg(long)]
    check: bool,
    /// Install this version instead of the latest (0.1.0; also downgrades)
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// The release feed [default: https://api.2befficient.com/cli]
    #[arg(long, env = "TWOBEC_INSTALL_URL", value_name = "URL")]
    feed: Option<String>,
    #[command(flatten)]
    output: OutputArgs,
}

/// What `--json` prints: the versions compared and what was done.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    /// The version running before the command.
    current: String,
    /// The version the feed offered (the latest, or the pinned one).
    target: String,
    platform: &'static str,
    binary: PathBuf,
    up_to_date: bool,
    /// `checked`, `upgraded`, `downgraded` or `unchanged`.
    action: &'static str,
    /// Completions and manual pages regenerated from the new binary.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    refreshed: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

pub async fn run(timeout: u64, args: UpgradeArgs) -> Result<()> {
    let feed_url = args.feed.as_deref().unwrap_or(DEFAULT_FEED);
    let feed = Feed::new(feed_url, timeout)?;
    let current = Version::current();
    let binary = replace::current_binary()?;

    let release = match &args.version {
        Some(v) => feed.release_of(Version::parse(v)?, PLATFORM),
        None => {
            let latest = feed.latest().await?;
            feed.release_of_latest(&latest, PLATFORM)?
        }
    };
    let target = release.version;
    let pinned = args.version.is_some();
    let up_to_date = if pinned {
        target == current
    } else {
        target <= current
    };
    let mut report = Report {
        current: current.to_string(),
        target: target.to_string(),
        platform: PLATFORM,
        binary: binary.clone(),
        up_to_date,
        action: "checked",
        refreshed: Vec::new(),
        notes: release.notes.clone(),
    };

    if args.check || up_to_date {
        if !args.check {
            report.action = "unchanged";
        }
        if args.output.wants_json() {
            return args.output.print_json(&report);
        }
        if up_to_date {
            println!("2bec {current} is up to date");
        } else if pinned {
            println!(
                "2bec {target} would replace {current}; run `2bec upgrade --version {target}`"
            );
        } else {
            println!("2bec {target} is available (you have {current}); run `2bec upgrade`");
            if let Some(notes) = &release.notes {
                println!("Release notes: {notes}");
            }
        }
        return Ok(());
    }

    if PLATFORM == "unsupported" {
        bail!("this build is not one of the release archives; install from source");
    }
    let dir = binary
        .parent()
        .context("the binary has no parent directory")?;
    replace::check_writable(dir)?;

    let stage = Staging::new(dir)?;
    eprintln!("Downloading 2bec {target} for {PLATFORM}");
    let archive_path = stage.dir.join(&release.archive_name);
    download(&feed, &release.archive_url, &archive_path).await?;
    let sums_path = stage.dir.join("SHA256SUMS");
    download(&feed, &release.checksums_url, &sums_path).await?;
    let sums = std::fs::read_to_string(&sums_path).context("cannot read the checksums")?;
    archive::verify(&archive_path, &sums, &release.archive_name)?;

    let new = stage.dir.join(archive::BINARY_NAME);
    archive::extract_binary(&archive_path, &folder_of(&release), &new)?;
    replace::install(&new, &binary)?;
    drop(stage);

    let installed = std::process::Command::new(&binary)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| format!("2bec {target}"));
    report.action = if target > current {
        "upgraded"
    } else {
        "downgraded"
    };
    let mut skipped = Vec::new();
    for share in extras::share_dirs(&binary) {
        let (done, failed) = extras::refresh(&binary, &share);
        report.refreshed.extend(done);
        skipped.extend(failed);
    }
    for note in &skipped {
        eprintln!("Could not refresh {note} (skipped)");
    }
    if args.output.wants_json() {
        return args.output.print_json(&report);
    }
    println!(
        "{} {} ({installed} at {})",
        if target > current {
            "Upgraded"
        } else {
            "Downgraded"
        },
        format_args!("2bec from {current} to {target}"),
        binary.display()
    );
    if !report.refreshed.is_empty() {
        println!(
            "Refreshed the completions and manual pages ({})",
            report
                .refreshed
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if let Some(notes) = &release.notes {
        println!("Release notes: {notes}");
    }
    Ok(())
}

/// The folder at the root of a release archive: the archive's own name.
fn folder_of(release: &Release) -> String {
    format!("2bec-{}-{PLATFORM}", release.version)
}

/// Fetch a URL of the feed into a file.
async fn download(feed: &Feed, url: &str, path: &Path) -> Result<()> {
    let mut file =
        std::fs::File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
    feed.download(url, &mut file).await?;
    Ok(())
}

/// A scratch directory beside the binary, so the new file lands on the same
/// file system and a rename can move it into place; removed on drop, which
/// a failed step reaches too.
struct Staging {
    dir: PathBuf,
}

impl Staging {
    fn new(beside: &Path) -> Result<Self> {
        let dir = beside.join(format!(".2bec-upgrade-{}", std::process::id()));
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create {}", dir.display()))?;
        Ok(Self { dir })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
