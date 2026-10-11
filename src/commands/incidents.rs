//! `2bec incidents`: streaks of consecutive failed runs.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::metrics::{self as fmt, Selection};
use super::runs::{ensure_guid, print_failure};
use crate::api::incidents::Incident;
use crate::api::metrics::IncidentListItem;
use crate::api::page::{Fetch, fetch_items};
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
pub struct IncidentsArgs {
    #[command(subcommand)]
    command: IncidentsCommand,
}

#[derive(Subcommand, Debug)]
enum IncidentsCommand {
    /// List the incidents that started in a period, newest first (last 7 days by default)
    #[command(after_help = "\
Examples:
  2bec incidents list
  2bec incidents list --ongoing
  2bec incidents list --scenario 7f3c... --period lastMonth --min-duration 600 --all --json

Transaction scenarios only, 100 at most; past the cap the API asks to name
them with --scenario. The period spans at most 366 days. Needs metrics:read.")]
    List {
        #[command(flatten)]
        selection: Selection,
        /// Only incidents still failing
        #[arg(long)]
        ongoing: bool,
        /// Only incidents at least this long, in seconds
        #[arg(long, value_name = "SECONDS")]
        min_duration: Option<u64>,
        /// Maximum number of incidents to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every incident, following pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one incident: its runs, the first failure and the alerts sent
    Get {
        /// Incident id (the id of its first failed run)
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: IncidentsArgs) -> Result<()> {
    match args.command {
        IncidentsCommand::List {
            selection,
            ongoing,
            min_duration,
            limit,
            all,
            output,
        } => list(ctx, &selection, ongoing, min_duration, limit, all, output).await,
        IncidentsCommand::Get { id, output } => get(ctx, &id, output).await,
    }
}

async fn list(
    ctx: &Context,
    selection: &Selection,
    ongoing: bool,
    min_duration: Option<u64>,
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let mut query = selection.query(100)?;
    if ongoing {
        query.push(("ongoing", "true".into()));
    }
    if let Some(m) = min_duration {
        query.push(("minDuration", m.to_string()));
    }
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page =
        fetch_items::<IncidentListItem>(&ctx.client()?, "v2/incidents", &query, fetch).await?;
    out.print(&page.items, "No incident in this period", || {
        let mut t = Table::new([
            "ID",
            "SCENARIO",
            "START",
            "END",
            "DURATION",
            "FAILED STEP",
            "RUNS",
        ]);
        for i in &page.items {
            t.row([
                i.id.clone(),
                i.scenario.name.clone(),
                short_time(&i.start),
                match &i.end {
                    Some(e) => short_time(e),
                    None if i.ongoing => "ongoing".into(),
                    None => "-".into(),
                },
                fmt::duration(i.duration_seconds),
                or_dash(i.failed_step.as_deref()),
                i.failed_runs.to_string(),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} incidents; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("incident id", id)?;
    let i: Incident = ctx
        .client()?
        .get(&format!("v2/incidents/{id}"))
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&i);
    }
    println!("Incident:     {}", i.id);
    println!("Scenario:     {} ({})", i.scenario.name, i.scenario.id);
    println!("Start:        {}", short_time(&i.start));
    println!(
        "End:          {}",
        match &i.end {
            Some(e) => short_time(e),
            None if i.ongoing => "still failing".into(),
            None => "-".into(),
        }
    );
    println!("Duration:     {}", fmt::duration(i.duration_seconds as f64));
    println!("Failed step:  {}", or_dash(i.failed_step.as_deref()));
    println!("Failed runs:  {}", i.failed_runs);
    println!(
        "Alerts sent:  {}",
        i.alerts_sent
            .map_or("unknown".to_string(), |n| n.to_string())
    );
    if let Some(f) = &i.failure {
        print_failure(f);
    }
    if !i.runs.is_empty() {
        println!();
        let mut t = Table::new(["RUN", "STARTED", "STATUS", "FAILED STEP"]);
        for r in &i.runs {
            t.row([
                r.id.clone(),
                short_time(&r.started_at),
                r.status.clone(),
                or_dash(r.failed_step.as_deref()),
            ]);
        }
        t.print();
    }
    Ok(())
}
