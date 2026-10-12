mod api;
mod auth;
mod commands;
mod config;
mod http;
mod output;
mod upgrade;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
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
    /// The administration history: who did what on the space
    Audit(commands::audit::AuditArgs),
    /// Log in, show or drop the credential used to call the API
    Auth(commands::auth::AuthArgs),
    /// How the availability rate is computed and the targets it is read against
    Availability(commands::availability::AvailabilityArgs),
    /// Badges: symbols drawn on the runs matching a rule over their tags
    Badges(commands::badges::BadgesArgs),
    /// The team's comments on the runs, across scenarios
    Comments(commands::comments::CommentsArgs),
    /// Print a shell completion script (bash, zsh, fish, powershell, elvish)
    Completion(commands::docs::CompletionArgs),
    /// Read and edit the config file, switch profiles
    Config(commands::config::ConfigArgs),
    /// Streaks of consecutive failed runs
    Incidents(commands::incidents::IncidentsArgs),
    /// Planned non-observation windows and rules
    Interruptions(commands::interruptions::InterruptionsArgs),
    /// Print or install the manual pages
    Man(commands::docs::ManArgs),
    /// Availability, response times, voice quality and social figures over a period
    Metrics(commands::metrics::MetricsArgs),
    /// Set runs apart from the indicators, and undo it
    Neutralizations(commands::neutralizations::NeutralizationsArgs),
    /// Check that the API answers and print its version and latency
    Ping,
    /// Timeline, emergency stop and dated notes on the scenarios
    Planning(commands::planning::PlanningArgs),
    /// Access profiles: which scenario groups the users see
    Profiles(commands::profiles::ProfilesArgs),
    /// Saved reports, their data and PDFs, scheduled e-mails and deliveries
    Reports(commands::reports::ReportsArgs),
    /// List and inspect scenario runs
    Runs(commands::runs::RunsArgs),
    /// Browse the catalog of scenarios and their groups
    Scenarios(commands::scenarios::ScenariosArgs),
    /// The sign-in policy: password rules and strong authentication
    Security(commands::security::SecurityArgs),
    /// Tags a run may carry: the space's own, the robots' and the platform's
    Tags(commands::tags::TagsArgs),
    /// Support requests between the space and the platform's team
    Tickets(commands::tickets::TicketsArgs),
    /// Replace this binary with the latest release (or a pinned version)
    Upgrade(commands::upgrade::UpgradeArgs),
    /// The users of the space, their roles and access profile
    Users(commands::users::UsersArgs),
    /// Runtime variables: the named values the alert hooks receive
    Variables(commands::variables::VariablesArgs),
    /// Webhooks: subscriptions of your URLs to events, secrets, deliveries
    Webhooks(commands::webhooks::WebhooksArgs),
}

#[tokio::main]
async fn main() -> ExitCode {
    reset_sigpipe();
    #[cfg(windows)]
    upgrade::replace::remove_leftover();
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

/// Let a closed pipe (`2bec runs list | head`) end the process quietly, as
/// the shell expects, instead of panicking on the next write to stdout.
fn reset_sigpipe() {
    #[cfg(unix)]
    // SAFETY: restoring the default disposition of SIGPIPE before any other
    // thread exists; `signal` with SIG_DFL has no preconditions.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// The credential store the config asks for.
fn open_store(
    config_dir: &std::path::Path,
    kind: config::CredentialStoreKind,
) -> Result<auth::store::FileStore> {
    let store = auth::store::FileStore::in_dir(config_dir);
    match kind {
        config::CredentialStoreKind::File => Ok(store),
        #[cfg(feature = "keyring")]
        config::CredentialStoreKind::Keyring => {
            Ok(store.with_secrets(Box::new(auth::keyring::Keyring)))
        }
        #[cfg(not(feature = "keyring"))]
        config::CredentialStoreKind::Keyring => anyhow::bail!(
            "credential_store = \"keyring\" needs a build of 2bec with the `keyring` feature; \
             run `2bec config set credential_store file` to use the file store"
        ),
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
    // Local commands: no config, no credential, no network.
    match cli.command {
        Command::Completion(args) => return commands::docs::completion(Cli::command(), args),
        Command::Man(args) => return commands::docs::man(Cli::command(), args),
        Command::Upgrade(args) => return commands::upgrade::run(cli.timeout, args).await,
        _ => {}
    }
    let config_dir = config::config_dir()?;
    let config = config::Config::load(&config_dir.join(config::CONFIG_FILE))?;
    let overrides = config::Overrides {
        profile: cli.profile.clone(),
        api_url: cli.api_url.clone(),
    };
    let settings = config::Settings::resolve(&overrides, &config);
    if let Command::Config(args) = cli.command {
        // Only the index of the store is needed here, and `config set
        // credential_store file` must work even when the keyring cannot open.
        let index = auth::store::FileStore::in_dir(&config_dir);
        return commands::config::run(&config_dir, &config, &settings, &index, args);
    }
    let store = open_store(&config_dir, settings.credential_store)?;
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
        Command::Audit(args) => commands::audit::run(&ctx, args).await?,
        Command::Auth(args) => commands::auth::run(&ctx, args).await?,
        Command::Availability(args) => commands::availability::run(&ctx, args).await?,
        Command::Badges(args) => commands::badges::run(&ctx, args).await?,
        Command::Comments(args) => commands::comments::run(&ctx, args).await?,
        Command::Incidents(args) => commands::incidents::run(&ctx, args).await?,
        Command::Interruptions(args) => commands::interruptions::run(&ctx, args).await?,
        Command::Metrics(args) => commands::metrics::run(&ctx, args).await?,
        Command::Neutralizations(args) => commands::neutralizations::run(&ctx, args).await?,
        Command::Planning(args) => commands::planning::run(&ctx, args).await?,
        Command::Profiles(args) => commands::profiles::run(&ctx, args).await?,
        Command::Reports(args) => commands::reports::run(&ctx, args).await?,
        Command::Runs(args) => commands::runs::run(&ctx, args).await?,
        Command::Scenarios(args) => commands::scenarios::run(&ctx, args).await?,
        Command::Security(args) => commands::security::run(&ctx, args).await?,
        Command::Tags(args) => commands::tags::run(&ctx, args).await?,
        Command::Tickets(args) => commands::tickets::run(&ctx, args).await?,
        Command::Users(args) => commands::users::run(&ctx, args).await?,
        Command::Variables(args) => commands::variables::run(&ctx, args).await?,
        Command::Webhooks(args) => commands::webhooks::run(&ctx, args).await?,
        Command::Completion(_) | Command::Config(_) | Command::Man(_) | Command::Upgrade(_) => {
            unreachable!("handled above")
        }
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
