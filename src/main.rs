mod http;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// 2bec - command-line client for the 2Be-FFICIENT API.
#[derive(Parser, Debug)]
#[command(name = "2bec", version, about, long_about = None)]
struct Cli {
    /// Base URL of the API (can also be set via TWOBEC_API_URL)
    #[arg(long, env = "TWOBEC_API_URL", default_value = http::DEFAULT_API_URL)]
    api_url: String,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 30)]
    timeout: u64,

    /// Bearer token sent as `Authorization: Bearer <token>`
    #[arg(long, env = "TWOBEC_TOKEN", hide_env_values = true)]
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
    let client = http::Client::new(&cli.api_url, cli.timeout, cli.token.as_deref())?;

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
