mod http;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// 2bec - command-line client for the 2Be-FFICIENT API.
#[derive(Parser, Debug)]
#[command(name = "2bec", version, about, long_about = None)]
struct Cli {
    /// Base URL used by all requests (can also be set via TWOBEC_BASE_URL)
    #[arg(long, env = "TWOBEC_BASE_URL", default_value = "https://httpbin.org")]
    base_url: String,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 30)]
    timeout: u64,

    /// Optional bearer token sent as `Authorization: Bearer <token>`
    #[arg(long, env = "TWOBEC_TOKEN", hide_env_values = true)]
    token: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Perform a GET request and print the body
    Get {
        /// Path relative to the base URL (or a full URL)
        path: String,
        /// Pretty-print JSON responses
        #[arg(long)]
        pretty: bool,
    },
    /// Perform a POST request with a JSON body read from the argument or stdin
    Post {
        /// Path relative to the base URL (or a full URL)
        path: String,
        /// JSON body; reads from stdin when omitted
        #[arg(long)]
        body: Option<String>,
        /// Pretty-print JSON responses
        #[arg(long)]
        pretty: bool,
    },
    /// Check reachability of the base URL and print status + latency
    Ping,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = http::Client::new(&cli.base_url, cli.timeout, cli.token.as_deref())?;

    match cli.command {
        Command::Get { path, pretty } => {
            let resp = client.get(&path).await?;
            print_response(&resp, pretty)?;
        }
        Command::Post { path, body, pretty } => {
            let raw = match body {
                Some(b) => b,
                None => {
                    use std::io::Read;
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
            };
            let json: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("body is not valid JSON: {e}"))?;
            let resp = client.post_json(&path, &json).await?;
            print_response(&resp, pretty)?;
        }
        Command::Ping => {
            let r = client.ping().await?;
            println!("{} -> HTTP {} in {} ms", r.url, r.status, r.latency_ms);
        }
    }
    Ok(())
}

fn print_response(resp: &http::Response, pretty: bool) -> Result<()> {
    if resp.status >= 400 {
        eprintln!("HTTP {}", resp.status);
    }
    if pretty && let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp.body) {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!("{}", resp.body);
    Ok(())
}
