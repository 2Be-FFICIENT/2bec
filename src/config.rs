//! Configuration file and effective settings.
//!
//! Settings are resolved in this order of precedence: flags, then
//! `TWOBEC_*` environment variables, then the config file, then built-in
//! defaults. The config file is `config.toml` under the 2bec configuration
//! directory (see [`config_dir`]) and holds named profiles, one per
//! environment:
//!
//! ```toml
//! default_profile = "prod"
//!
//! [profiles.prod]
//! api_url = "https://api.2befficient.com"
//!
//! [profiles.dev]
//! api_url = "http://localhost:5152"
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::http::DEFAULT_API_URL;

/// Name of the profile used when neither flag, env nor file names one.
pub const DEFAULT_PROFILE: &str = "default";

/// File name of the config file inside [`config_dir`].
pub const CONFIG_FILE: &str = "config.toml";

/// Directory holding `config.toml` and the credential store.
///
/// `TWOBEC_CONFIG_DIR` overrides it; otherwise it is `2bec` under the
/// platform configuration directory (`$XDG_CONFIG_HOME` or `~/.config` on
/// Linux).
pub fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("TWOBEC_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    dirs::config_dir()
        .map(|d| d.join("2bec"))
        .context("cannot determine the configuration directory; set TWOBEC_CONFIG_DIR")
}

/// Contents of `config.toml`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Profile used when `--profile` / `TWOBEC_PROFILE` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    /// Named profiles, typically one per environment.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, Profile>,
}

/// Settings of one profile. Every field is optional and falls back to the
/// built-in default.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Base URL of the API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
}

impl Config {
    /// Load the config file; a missing file is an empty config.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };
        toml::from_str(&text).with_context(|| format!("invalid config file {}", path.display()))
    }
}

/// Values given on the command line or through the environment; `None`
/// means "not given", so the config file and defaults apply.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    pub profile: Option<String>,
    pub api_url: Option<String>,
}

/// Where a setting's effective value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A flag or `TWOBEC_*` environment variable.
    Override,
    /// The profile in the config file.
    ConfigFile,
    /// The built-in default.
    Default,
}

/// Effective settings after applying the precedence rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Name of the active profile.
    pub profile: String,
    /// Base URL of the API, without a trailing slash.
    pub api_url: String,
    pub api_url_source: Source,
}

impl Settings {
    /// Resolve the effective settings from overrides and the config file.
    pub fn resolve(overrides: &Overrides, config: &Config) -> Self {
        let profile = overrides
            .profile
            .clone()
            .or_else(|| config.default_profile.clone())
            .unwrap_or_else(|| DEFAULT_PROFILE.to_string());
        let from_file = config.profiles.get(&profile);

        let (api_url, api_url_source) = if let Some(u) = &overrides.api_url {
            (u.clone(), Source::Override)
        } else if let Some(u) = from_file.and_then(|p| p.api_url.clone()) {
            (u, Source::ConfigFile)
        } else {
            (DEFAULT_API_URL.to_string(), Source::Default)
        };

        Self {
            profile,
            api_url: api_url.trim_end_matches('/').to_string(),
            api_url_source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(default: Option<&str>, name: &str, url: &str) -> Config {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            name.to_string(),
            Profile {
                api_url: Some(url.to_string()),
            },
        );
        Config {
            default_profile: default.map(str::to_string),
            profiles,
        }
    }

    #[test]
    fn defaults_apply_without_config() {
        let s = Settings::resolve(&Overrides::default(), &Config::default());
        assert_eq!(s.profile, DEFAULT_PROFILE);
        assert_eq!(s.api_url, DEFAULT_API_URL);
        assert_eq!(s.api_url_source, Source::Default);
    }

    #[test]
    fn config_default_profile_is_used() {
        let c = config_with(Some("dev"), "dev", "http://localhost:5152/");
        let s = Settings::resolve(&Overrides::default(), &c);
        assert_eq!(s.profile, "dev");
        assert_eq!(s.api_url, "http://localhost:5152");
        assert_eq!(s.api_url_source, Source::ConfigFile);
    }

    #[test]
    fn override_profile_selects_another_profile() {
        let mut c = config_with(Some("dev"), "dev", "http://localhost:5152");
        c.profiles.insert(
            "next".into(),
            Profile {
                api_url: Some("https://api.next.example".into()),
            },
        );
        let o = Overrides {
            profile: Some("next".into()),
            api_url: None,
        };
        let s = Settings::resolve(&o, &c);
        assert_eq!(s.profile, "next");
        assert_eq!(s.api_url, "https://api.next.example");
    }

    #[test]
    fn override_url_beats_config() {
        let c = config_with(Some("dev"), "dev", "http://localhost:5152");
        let o = Overrides {
            profile: None,
            api_url: Some("http://127.0.0.1:9/".into()),
        };
        let s = Settings::resolve(&o, &c);
        assert_eq!(s.api_url, "http://127.0.0.1:9");
        assert_eq!(s.api_url_source, Source::Override);
    }

    #[test]
    fn unknown_profile_falls_back_to_defaults() {
        let c = config_with(None, "dev", "http://localhost:5152");
        let o = Overrides {
            profile: Some("missing".into()),
            api_url: None,
        };
        let s = Settings::resolve(&o, &c);
        assert_eq!(s.profile, "missing");
        assert_eq!(s.api_url, DEFAULT_API_URL);
    }

    #[test]
    fn config_round_trips_through_toml() {
        let c = config_with(Some("prod"), "prod", "https://api.2befficient.com");
        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = toml::from_str::<Config>("[profiles.x]\napi_ur = \"a\"\n").unwrap_err();
        assert!(err.to_string().contains("api_ur"));
    }

    #[test]
    fn missing_file_loads_as_empty() {
        let c = Config::load(Path::new("/nonexistent/2bec/config.toml")).unwrap();
        assert_eq!(c, Config::default());
    }
}
