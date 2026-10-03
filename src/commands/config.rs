//! `2bec config`: read and edit `config.toml` and switch between profiles.

use anyhow::{Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use std::path::Path;

use crate::auth::store::CredentialStore;
use crate::config::{
    CONFIG_FILE, Config, CredentialStoreKind, DEFAULT_PROFILE, Profile, Settings, Source,
};
use crate::http::DEFAULT_API_URL;
use crate::output::{OutputArgs, Table};

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Print the effective settings of the active profile and where each comes from
    #[command(after_help = "\
Example:
  2bec config list
  2bec --profile dev config list --json")]
    List {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Print the effective value of one setting
    #[command(after_help = "\
Examples:
  2bec config get api_url
  2bec --profile dev config get api_url

Flags and TWOBEC_* variables take precedence over the file, so the value
printed is the one the commands use, not necessarily the one on disk.")]
    Get {
        /// Setting to read
        key: Key,
    },
    /// Write a setting into the active profile
    #[command(after_help = "\
Examples:
  2bec --profile dev config set api_url http://localhost:5152
  2bec config set default_profile dev

Comments in config.toml are not preserved.")]
    Set {
        /// Setting to write
        key: Key,
        /// New value
        value: String,
    },
    /// Remove a setting from the active profile so the default applies again
    Unset {
        /// Setting to remove
        key: Key,
    },
    /// Print the path of the config file
    Path,
    /// List the profiles or pick the default one
    Profile(ProfileArgs),
}

#[derive(Args, Debug)]
pub struct ProfileArgs {
    #[command(subcommand)]
    command: ProfileCommand,
}

#[derive(Subcommand, Debug)]
enum ProfileCommand {
    /// List the profiles (configured or logged in) and whether each has a credential
    List {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Make a profile the default (the one used without --profile)
    #[command(after_help = "\
Example:
  2bec config profile use dev

The profile is created in config.toml if it does not exist yet; set its
api_url afterwards with `2bec --profile dev config set api_url URL`.")]
    Use {
        /// Profile name
        name: String,
    },
}

/// A setting of `config.toml`, named as in the file.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Base URL of the API
    #[value(name = "api_url")]
    ApiUrl,
    /// Authorization server for the browser login (discovered when unset)
    #[value(name = "issuer")]
    Issuer,
    /// OAuth client id registered for this tool
    #[value(name = "oauth_client_id")]
    OauthClientId,
    /// `resource` (token audience) asked from the identity server
    #[value(name = "api_resource")]
    ApiResource,
    /// Profile used without --profile (not per profile)
    #[value(name = "default_profile")]
    DefaultProfile,
    /// Where the credentials' secrets are kept: file or keyring (not per profile)
    #[value(name = "credential_store")]
    CredentialStore,
}

impl Key {
    fn name(self) -> &'static str {
        match self {
            Self::ApiUrl => "api_url",
            Self::Issuer => "issuer",
            Self::OauthClientId => "oauth_client_id",
            Self::ApiResource => "api_resource",
            Self::DefaultProfile => "default_profile",
            Self::CredentialStore => "credential_store",
        }
    }

    /// Every key, in the order `config list` prints them.
    const ALL: [Key; 6] = [
        Key::DefaultProfile,
        Key::CredentialStore,
        Key::ApiUrl,
        Key::Issuer,
        Key::OauthClientId,
        Key::ApiResource,
    ];

    /// The value in the file for `profile`, if set.
    fn in_file(self, config: &Config, profile: &str) -> Option<String> {
        let p = config.profiles.get(profile);
        match self {
            Self::ApiUrl => p.and_then(|p| p.api_url.clone()),
            Self::Issuer => p.and_then(|p| p.issuer.clone()),
            Self::OauthClientId => p.and_then(|p| p.oauth_client_id.clone()),
            Self::ApiResource => p.and_then(|p| p.api_resource.clone()),
            Self::DefaultProfile => config.default_profile.clone(),
            Self::CredentialStore => config.credential_store.map(|k| k.name().to_string()),
        }
    }

    /// The effective value and its source.
    fn effective(self, settings: &Settings, config: &Config) -> (Option<String>, Source) {
        let from_file = self.in_file(config, &settings.profile);
        match self {
            Self::ApiUrl => (Some(settings.api_url.clone()), settings.api_url_source),
            Self::Issuer => (settings.issuer.clone(), source_of(from_file)),
            Self::OauthClientId => (Some(settings.oauth_client_id.clone()), source_of(from_file)),
            Self::ApiResource => (Some(settings.api_resource.clone()), source_of(from_file)),
            Self::CredentialStore => (
                Some(settings.credential_store.name().to_string()),
                source_of(from_file),
            ),
            Self::DefaultProfile => (
                Some(settings.profile.clone()),
                if from_file.as_deref() == Some(&settings.profile) {
                    Source::ConfigFile
                } else if settings.profile == DEFAULT_PROFILE && from_file.is_none() {
                    Source::Default
                } else {
                    Source::Override
                },
            ),
        }
    }

    fn write(self, config: &mut Config, profile: &str, value: Option<String>) {
        if self == Self::DefaultProfile {
            config.default_profile = value;
            return;
        }
        if self == Self::CredentialStore {
            // Checked by `check` already; an unknown name is not written.
            config.credential_store = value.and_then(|v| v.parse().ok());
            return;
        }
        let p = config.profiles.entry(profile.to_string()).or_default();
        let slot = match self {
            Self::ApiUrl => &mut p.api_url,
            Self::Issuer => &mut p.issuer,
            Self::OauthClientId => &mut p.oauth_client_id,
            Self::ApiResource => &mut p.api_resource,
            Self::DefaultProfile | Self::CredentialStore => unreachable!("handled above"),
        };
        *slot = value;
        if *p == Profile::default() {
            config.profiles.remove(profile);
        }
    }

    /// Reject values the commands could not use.
    fn check(self, value: &str) -> Result<()> {
        let value = value.trim();
        if value.is_empty() {
            bail!(
                "the value is empty; use `config unset {}` to drop it",
                self.name()
            );
        }
        match self {
            Self::ApiUrl | Self::Issuer | Self::ApiResource => {
                if !(value.starts_with("http://") || value.starts_with("https://")) {
                    bail!("{} must be an http(s) URL", self.name());
                }
            }
            Self::CredentialStore => {
                let kind: CredentialStoreKind = value.parse()?;
                if kind == CredentialStoreKind::Keyring && !cfg!(feature = "keyring") {
                    eprintln!(
                        "warning: this build of 2bec has no keyring support; the credential \
                         store will not open until you rebuild with `--features keyring`"
                    );
                }
            }
            Self::OauthClientId | Self::DefaultProfile => {}
        }
        Ok(())
    }
}

fn source_of(from_file: Option<String>) -> Source {
    if from_file.is_some() {
        Source::ConfigFile
    } else {
        Source::Default
    }
}

fn source_name(source: Source) -> &'static str {
    match source {
        Source::Override => "flag or environment",
        Source::ConfigFile => "config file",
        Source::Default => "default",
    }
}

pub fn run(
    config_dir: &Path,
    config: &Config,
    settings: &Settings,
    store: &dyn CredentialStore,
    args: ConfigArgs,
) -> Result<()> {
    let path = config_dir.join(CONFIG_FILE);
    match args.command {
        ConfigCommand::List { output } => list(settings, config, &output),
        ConfigCommand::Get { key } => {
            let (value, _) = key.effective(settings, config);
            match value {
                Some(v) => println!("{v}"),
                None => {
                    if crate::output::stdout_is_tty() {
                        eprintln!("{} is not set", key.name());
                    }
                }
            }
            Ok(())
        }
        ConfigCommand::Set { key, value } => {
            key.check(&value)?;
            let value = value.trim().trim_end_matches('/').to_string();
            let mut edited = config.clone();
            key.write(&mut edited, &settings.profile, Some(value.clone()));
            edited.save(&path)?;
            if key == Key::DefaultProfile {
                println!("Default profile is now `{value}`");
            } else if key == Key::CredentialStore {
                println!("Credential store is now `{value}`; existing logins stay readable");
            } else {
                println!(
                    "Set {} = {value} for profile `{}`",
                    key.name(),
                    settings.profile
                );
            }
            Ok(())
        }
        ConfigCommand::Unset { key } => {
            let mut edited = config.clone();
            if key.in_file(config, &settings.profile).is_none() {
                println!("{} is not set in {}", key.name(), path.display());
                return Ok(());
            }
            key.write(&mut edited, &settings.profile, None);
            edited.save(&path)?;
            if matches!(key, Key::DefaultProfile | Key::CredentialStore) {
                println!("Removed {}", key.name());
            } else {
                println!("Removed {} from profile `{}`", key.name(), settings.profile);
            }
            Ok(())
        }
        ConfigCommand::Path => {
            println!("{}", path.display());
            Ok(())
        }
        ConfigCommand::Profile(p) => match p.command {
            ProfileCommand::List { output } => profiles(config, settings, store, &output),
            ProfileCommand::Use { name } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    bail!("the profile name is empty");
                }
                let mut edited = config.clone();
                edited.profiles.entry(name.clone()).or_default();
                edited.default_profile = Some(name.clone());
                edited.save(&path)?;
                println!("Default profile is now `{name}`");
                if !store.profiles()?.contains(&name) {
                    eprintln!("note: no credential stored for `{name}`; run `2bec auth login`");
                }
                Ok(())
            }
        },
    }
}

fn list(settings: &Settings, config: &Config, output: &OutputArgs) -> Result<()> {
    let rows: Vec<serde_json::Value> = Key::ALL
        .iter()
        .map(|k| {
            let (value, source) = k.effective(settings, config);
            serde_json::json!({
                "key": k.name(),
                "value": value,
                "source": source_name(source),
            })
        })
        .collect();
    if output.wants_json() {
        let mut values = serde_json::Map::new();
        let mut sources = serde_json::Map::new();
        for r in &rows {
            let key = r["key"].as_str().unwrap_or_default().to_string();
            values.insert(key.clone(), r["value"].clone());
            sources.insert(key, r["source"].clone());
        }
        return output.print_json(&serde_json::json!({
            "profile": settings.profile,
            "values": values,
            "sources": sources,
        }));
    }
    let mut t = Table::new(["KEY", "VALUE", "SOURCE"]);
    for r in &rows {
        t.row([
            r["key"].as_str().unwrap_or_default(),
            r["value"].as_str().unwrap_or("(unset)"),
            r["source"].as_str().unwrap_or_default(),
        ]);
    }
    t.print();
    Ok(())
}

fn profiles(
    config: &Config,
    settings: &Settings,
    store: &dyn CredentialStore,
    output: &OutputArgs,
) -> Result<()> {
    // A profile exists when the config file or the credential store knows it.
    let logged_in = store.profiles()?;
    let mut names: Vec<String> = config.profiles.keys().cloned().collect();
    names.extend(logged_in.iter().cloned());
    names.extend(config.default_profile.clone());
    names.push(settings.profile.clone());
    names.sort();
    names.dedup();
    let default = config
        .default_profile
        .clone()
        .unwrap_or_else(|| DEFAULT_PROFILE.to_string());
    let mut rows = Vec::new();
    for name in &names {
        let p = config.profiles.get(name);
        let api_url = p
            .and_then(|p| p.api_url.clone())
            .unwrap_or_else(|| DEFAULT_API_URL.to_string());
        let credential = logged_in.contains(name);
        rows.push(serde_json::json!({
            "name": name,
            "apiUrl": api_url.trim_end_matches('/'),
            "default": *name == default,
            "active": *name == settings.profile,
            "credential": credential,
        }));
    }
    output.print(&rows, "No profiles configured.", || {
        let mut t = Table::new(["NAME", "API URL", "CREDENTIAL", ""]);
        for r in &rows {
            let mut marks = Vec::new();
            if r["default"].as_bool() == Some(true) {
                marks.push("default");
            }
            if r["active"].as_bool() == Some(true) {
                marks.push("active");
            }
            t.row([
                r["name"].as_str().unwrap_or_default(),
                r["apiUrl"].as_str().unwrap_or_default(),
                if r["credential"].as_bool() == Some(true) {
                    "yes"
                } else {
                    "no"
                },
                &marks.join(", "),
            ]);
        }
        t
    })
}
