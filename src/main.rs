mod api;
mod auth;
mod commands;
mod config;
mod http;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::process::ExitCode;

/// 2bec - command-line client for the 2Be-FFICIENT API.
#[derive(Parser, Debug)]
#[command(name = "2bec", version, about, long_about = None)]
struct Cli {
    /// Configuration profile to use (see `config.toml`)
    #[arg(
        long,
        env = "TWOBEC_PROFILE",
        global = true,
        help_heading = "Global options"
    )]
    profile: Option<String>,

    /// Base URL of the API [default: https://api.2befficient.com]
    #[arg(
        long,
        env = "TWOBEC_API_URL",
        global = true,
        help_heading = "Global options"
    )]
    api_url: Option<String>,

    /// Request timeout in seconds
    #[arg(
        long,
        default_value_t = 30,
        global = true,
        help_heading = "Global options"
    )]
    timeout: u64,

    /// Show error details (cause chain, trace id)
    #[arg(long, short = 'v', global = true, help_heading = "Global options")]
    verbose: bool,

    /// API token; overrides the one stored by `auth login`
    #[arg(
        long,
        env = "TWOBEC_TOKEN",
        hide_env_values = true,
        global = true,
        help_heading = "Global options"
    )]
    token: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Alert configurations, recipients, suspensions and the alerts sent
    Alerts(commands::alerts::AlertsArgs),
    /// Log in, show or drop the credential used to call the API
    Auth(commands::auth::AuthArgs),
    /// Availability, response times, voice quality and social figures over a period
    Metrics(commands::metrics::MetricsArgs),
    /// Check that the API answers and print its version and latency
    Ping,
    /// Saved reports, their data and PDFs, scheduled e-mails and deliveries
    Reports(commands::reports::ReportsArgs),
    /// List and inspect scenario runs
    Runs(commands::runs::RunsArgs),
    /// Browse the catalog of scenarios and their groups
    Scenarios(commands::scenarios::ScenariosArgs),
    /// Webhooks: subscriptions of your URLs to events, secrets, deliveries
    Webhooks(commands::webhooks::WebhooksArgs),
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let verbose = cli.verbose;
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            report_error(&err, verbose);
            api::error::exit_code_for(&err)
        }
    }
}

/// Print a one-line error to stderr; `--verbose` adds causes and trace id.
fn report_error(err: &anyhow::Error, verbose: bool) {
    eprintln!("error: {err}");
    if !verbose {
        return;
    }
    for cause in err.chain().skip(1) {
        eprintln!("  caused by: {cause}");
    }
    if let Some(trace) = err
        .downcast_ref::<api::error::ApiError>()
        .and_then(|e| e.trace_id())
    {
        eprintln!("  trace id: {trace}");
    }
}

async fn run(cli: Cli) -> Result<()> {
    let config_dir = config::config_dir()?;
    let config = config::Config::load(&config_dir.join(config::CONFIG_FILE))?;
    let overrides = config::Overrides {
        profile: cli.profile.clone(),
        api_url: cli.api_url.clone(),
    };
    let settings = config::Settings::resolve(&overrides, &config);
    let store = auth::store::FileStore::in_dir(&config_dir);
    let mut credential = auth::resolve(cli.token.as_deref(), &settings.profile, &store)?;
    let skip_refresh = matches!(&cli.command, Command::Auth(a) if a.skips_refresh());
    if !skip_refresh {
        let http = commands::plain_http(cli.timeout)?;
        auth::refresh_if_needed(&mut credential, &settings, &store, &http).await;
    }
    let ctx = commands::Context {
        settings,
        store,
        credential,
        timeout: cli.timeout,
    };

    match cli.command {
        Command::Alerts(args) => commands::alerts::run(&ctx, args).await?,
        Command::Auth(args) => commands::auth::run(&ctx, args).await?,
        Command::Metrics(args) => commands::metrics::run(&ctx, args).await?,
        Command::Reports(args) => commands::reports::run(&ctx, args).await?,
        Command::Runs(args) => commands::runs::run(&ctx, args).await?,
        Command::Scenarios(args) => commands::scenarios::run(&ctx, args).await?,
        Command::Webhooks(args) => commands::webhooks::run(&ctx, args).await?,
        Command::Ping => {
            let r = ctx.client()?.ping().await?;
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
                return Err(api::error::ApiError::from_response(r.status, None, &r.body).into());
            }
        }
    }
    Ok(())
}
