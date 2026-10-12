//! The CLI release feed served by the API under `/cli` (anonymous): the
//! latest release as JSON and the archives of a version beside their
//! `SHA256SUMS`, the same files `install/install.sh` reads.

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

use crate::http::Client;

/// The production feed; `--feed` or `TWOBEC_INSTALL_URL` point elsewhere.
pub const DEFAULT_FEED: &str = "https://api.2befficient.com/cli";

/// This build's platform, as the release pipeline names the archives.
pub const PLATFORM: &str = if cfg!(target_os = "linux") {
    if cfg!(target_arch = "x86_64") {
        "linux-x64"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "unsupported"
    }
} else if cfg!(target_os = "macos") {
    if cfg!(target_arch = "x86_64") {
        "macos-x64"
    } else if cfg!(target_arch = "aarch64") {
        "macos-arm64"
    } else {
        "unsupported"
    }
} else if cfg!(target_os = "windows") {
    if cfg!(target_arch = "x86_64") {
        "windows-x64"
    } else if cfg!(target_arch = "aarch64") {
        "windows-arm64"
    } else {
        "unsupported"
    }
} else {
    "unsupported"
};

/// A release version, `MAJOR.MINOR.PATCH`, ordered numerically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// Parse `0.1.0` or `v0.1.0`; anything else (a range, a word) is refused.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        let core = text.strip_prefix('v').unwrap_or(text);
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() == 3
            && let Ok(nums) = parts
                .iter()
                .map(|p| {
                    (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                        .then(|| p.parse::<u64>().ok())
                        .flatten()
                        .ok_or(())
                })
                .collect::<Result<Vec<u64>, ()>>()
        {
            return Ok(Self(nums[0], nums[1], nums[2]));
        }
        bail!("'{text}' is not a version (expected 0.1.0)")
    }

    /// The version this binary was built as.
    pub fn current() -> Self {
        // The manifest version is validated by cargo; a build with a
        // non-semver version cannot exist.
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Self(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// What `GET /cli/releases/latest` answers.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Latest {
    pub version: String,
    #[serde(default)]
    pub published_at: Option<String>,
    /// The release notes page.
    #[serde(default)]
    pub notes: Option<String>,
    /// Archive URL by platform, only for the platforms the release holds.
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
    /// URL of the `SHA256SUMS` of the release.
    pub checksums: String,
}

/// The files of one release for this platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// Archive name, as listed in `SHA256SUMS`.
    pub archive_name: String,
    pub archive_url: String,
    pub checksums_url: String,
    pub notes: Option<String>,
}

/// The archive name of a version for a platform.
pub fn archive_name(version: Version, platform: &str) -> String {
    let ext = if platform.starts_with("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("2bec-{version}-{platform}.{ext}")
}

/// The feed, read with the plain (anonymous) HTTP client: the feed sits on
/// the API host, but never sees a credential.
pub struct Feed {
    client: Client,
    base: String,
}

impl Feed {
    pub fn new(base: &str, timeout_secs: u64) -> Result<Self> {
        let base = base.trim_end_matches('/').to_string();
        Ok(Self {
            client: Client::new(&base, timeout_secs, None)?,
            base,
        })
    }

    /// The latest release document.
    pub async fn latest(&self) -> Result<Latest> {
        let r = self.client.get("releases/latest").await.with_context(|| {
            format!(
                "the release feed did not answer ({}/releases/latest)",
                self.base
            )
        })?;
        r.json()
            .context("the release feed answered something that is not a release")
    }

    /// The files of the latest release for `platform`.
    pub fn release_of_latest(&self, latest: &Latest, platform: &str) -> Result<Release> {
        let version = Version::parse(&latest.version)
            .context("the release feed answered with an invalid version")?;
        let Some(archive_url) = latest.assets.get(platform) else {
            bail!(
                "release {version} has no archive for {platform} (available: {})",
                if latest.assets.is_empty() {
                    "none".to_string()
                } else {
                    latest.assets.keys().cloned().collect::<Vec<_>>().join(", ")
                }
            );
        };
        Ok(Release {
            version,
            archive_name: archive_name(version, platform),
            archive_url: archive_url.clone(),
            checksums_url: latest.checksums.clone(),
            notes: latest.notes.clone(),
        })
    }

    /// The files of a pinned version, at the feed's fixed layout
    /// (`releases/<version>/<archive>` beside `releases/<version>/SHA256SUMS`).
    pub fn release_of(&self, version: Version, platform: &str) -> Release {
        let archive_name = archive_name(version, platform);
        Release {
            version,
            archive_url: format!("{}/releases/{version}/{archive_name}", self.base),
            checksums_url: format!("{}/releases/{version}/SHA256SUMS", self.base),
            archive_name,
            notes: None,
        }
    }

    /// Download a URL of the feed into `sink`; returns the byte count.
    pub async fn download(&self, url: &str, sink: &mut dyn std::io::Write) -> Result<u64> {
        let (_, bytes) = self
            .client
            .download(url, sink)
            .await
            .with_context(|| format!("could not download {url}"))?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions_with_or_without_v() {
        assert_eq!(Version::parse("0.1.0").unwrap(), Version(0, 1, 0));
        assert_eq!(Version::parse("v1.20.3").unwrap(), Version(1, 20, 3));
        assert_eq!(Version::parse(" 0.1.0\n").unwrap(), Version(0, 1, 0));
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "latest",
            "1.x.0",
            "1..0",
            "-1.0.0",
            "0.1.0-rc1",
        ] {
            assert!(Version::parse(bad).is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn versions_order_numerically() {
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert!(Version(1, 0, 0) > Version(0, 99, 99));
        assert_eq!(Version(0, 1, 0).to_string(), "0.1.0");
    }

    #[test]
    fn current_is_the_manifest_version() {
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn archive_names_follow_the_pipeline() {
        assert_eq!(
            archive_name(Version(0, 2, 0), "linux-x64"),
            "2bec-0.2.0-linux-x64.tar.gz"
        );
        assert_eq!(
            archive_name(Version(0, 2, 0), "windows-arm64"),
            "2bec-0.2.0-windows-arm64.zip"
        );
    }

    #[test]
    fn this_platform_is_one_the_pipeline_builds() {
        assert!(
            [
                "linux-x64",
                "linux-arm64",
                "macos-x64",
                "macos-arm64",
                "windows-x64",
                "windows-arm64"
            ]
            .contains(&PLATFORM),
            "{PLATFORM}"
        );
    }

    #[test]
    fn latest_release_picks_this_platforms_archive() {
        let latest: Latest = serde_json::from_str(
            r#"{"version":"0.2.0","publishedAt":"2026-10-12T10:00:00+00:00","notes":"https://github.com/2Be-FFICIENT/2bec/releases/tag/v0.2.0",
                "assets":{"linux-x64":"https://api.2befficient.com/cli/releases/0.2.0/2bec-0.2.0-linux-x64.tar.gz"},
                "checksums":"https://api.2befficient.com/cli/releases/0.2.0/SHA256SUMS"}"#,
        )
        .unwrap();
        let feed = Feed::new(DEFAULT_FEED, 5).unwrap();
        let r = feed.release_of_latest(&latest, "linux-x64").unwrap();
        assert_eq!(r.version, Version(0, 2, 0));
        assert_eq!(r.archive_name, "2bec-0.2.0-linux-x64.tar.gz");
        assert!(
            r.archive_url
                .ends_with("/0.2.0/2bec-0.2.0-linux-x64.tar.gz")
        );
        assert!(r.checksums_url.ends_with("/0.2.0/SHA256SUMS"));
        let err = feed.release_of_latest(&latest, "macos-arm64").unwrap_err();
        assert!(
            err.to_string().contains("no archive for macos-arm64"),
            "{err}"
        );
        assert!(err.to_string().contains("linux-x64"), "{err}");
    }

    #[test]
    fn pinned_release_uses_the_feed_layout() {
        let feed = Feed::new("https://feed.example/cli/", 5).unwrap();
        let r = feed.release_of(Version(0, 1, 0), "macos-arm64");
        assert_eq!(
            r.archive_url,
            "https://feed.example/cli/releases/0.1.0/2bec-0.1.0-macos-arm64.tar.gz"
        );
        assert_eq!(
            r.checksums_url,
            "https://feed.example/cli/releases/0.1.0/SHA256SUMS"
        );
    }
}
