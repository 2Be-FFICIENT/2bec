mod auth;
mod config;
mod http;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// 2bec - command-line client for the 2Be-FFICIENT API.
#[derive(Parser, Debug)]
#[command(name = "2bec", version, about, long_about = None)]
struct Cli {
    /// Configuration profile to use (see `config.toml`)
    #[arg(long, env = "TWOBEC_PROFILE", global = true)]
    profile: Option<String>,

    /// Base URL of the API [default: https://api.2befficient.com]
    #[arg(long, env = "TWOBEC_API_URL", global = true)]
    api_url: Option<String>,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 30)]
    timeout: u64,

    /// API token; overrides the one stored by `auth login`
    #[arg(long, env = "TWOBEC_TOKEN", hide_env_values = true, global = true)]
    token: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check that the API answers and print its version and latency
    Ping,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_dir = config::config_dir()?;
    let config = config::Config::load(&config_dir.join(config::CONFIG_FILE))?;
    let overrides = config::Overrides {
        profile: cli.profile.clone(),
        api_url: cli.api_url.clone(),
    };
    let settings = config::Settings::resolve(&overrides, &config);
    let store = auth::store::FileStore::in_dir(&config_dir);
    let credential = auth::resolve(cli.token.as_deref(), &settings.profile, &store)?;
    let client = http::Client::new(
        &settings.api_url,
        cli.timeout,
        credential.as_ref().map(|c| c.token.as_str()),
    )?;

    match cli.command {
        Command::Ping => {
            let r = client.ping().await?;
            let name = serde_json::from_str::<serde_json::Value>(&r.body)
                .ok()
                .and_then(|v| {
                    let name = v.get("name")?.as_str()?.to_string();
                    let version = v.get("version")?.as_str()?.to_string();
                    Some(format!("{name} {version}"))
                })
                .unwrap_or_else(|| "unknown service".to_string());
            println!(
                "{} -> HTTP {} ({}) in {} ms",
                r.url, r.status, name, r.latency_ms
            );
            if r.status >= 400 {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
