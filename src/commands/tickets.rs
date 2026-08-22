//! `2bec tickets`: the support requests between the space and the
//! platform's team. Read only, when the space's switch is on.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::comments::one_line;
use super::metrics::PeriodArgs;
use super::runs::ensure_guid;
use crate::api::governance::{self, Ticket, TicketDetail};
use crate::api::page::{Fetch, fetch_items};
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
#[command(after_help = "\
Needs scenarios:read. 403 tickets-not-enabled while the space's
administrators have not asked the platform's team for the support requests
page.")]
pub struct TicketsArgs {
    #[command(subcommand)]
    command: TicketsCommand,
}

#[derive(Subcommand, Debug)]
enum TicketsCommand {
    /// List the requests opened within a period (the last 12 months by default), newest first
    #[command(after_help = "\
Examples:
  2bec tickets list
  2bec tickets list --view open                  # open requests whatever their date
  2bec tickets list --kind Robot --scenario <id> --period lastMonth
  2bec tickets list --all --json | jq -r '.[] | [.id, .status, .title] | @tsv'")]
    List {
        #[command(flatten)]
        period: PeriodArgs,
        /// Which requests: open, waiting (for the space), solved or all
        #[arg(long, value_name = "VIEW", value_parser = governance::TICKET_VIEWS)]
        view: Option<String>,
        /// Only requests of this kind, as the platform's analysis names them
        #[arg(long, value_name = "KIND")]
        kind: Option<String>,
        /// Only requests about this scenario
        #[arg(long, value_name = "ID")]
        scenario: Option<String>,
        /// How many requests to show at most
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Every request of the period, following the pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one request with its conversation
    Get {
        /// Request id (a number)
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: TicketsArgs) -> Result<()> {
    match args.command {
        TicketsCommand::List {
            period,
            view,
            kind,
            scenario,
            limit,
            all,
            output,
        } => {
            list(
                ctx,
                &period,
                view.as_deref(),
                kind.as_deref(),
                scenario.as_deref(),
                limit,
                all,
                output,
            )
            .await
        }
        TicketsCommand::Get { id, output } => get(ctx, &id, output).await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn list(
    ctx: &Context,
    period: &PeriodArgs,
    view: Option<&str>,
    kind: Option<&str>,
    scenario: Option<&str>,
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let mut query = period.query()?;
    if let Some(v) = view {
        query.push(("view", v.to_string()));
    }
    if let Some(k) = kind
        && !k.trim().is_empty()
    {
        query.push(("kind", k.trim().to_string()));
    }
    if let Some(s) = scenario {
        ensure_guid("--scenario", s)?;
        query.push(("scenario", s.to_string()));
    }
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page = fetch_items::<Ticket>(&ctx.client()?, "v2/tickets", &query, fetch).await?;
    out.print(&page.items, "No support request in this period", || {
        let mut t = Table::new([
            "ID",
            "STATUS",
            "KIND",
            "CREATED",
            "UPDATED",
            "SCENARIOS",
            "TITLE",
        ]);
        for k in &page.items {
            t.row([
                k.id.to_string(),
                k.status.clone(),
                or_dash(k.kind.as_deref()),
                short_time(&k.created_at),
                short_time(&k.updated_at),
                scenarios(k),
                title(k),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} requests; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    let id = ticket_id(id)?;
    let d: TicketDetail = ctx
        .client()?
        .get(&format!("v2/tickets/{id}"))
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&d);
    }
    let k = &d.ticket;
    println!("Request:    {}", k.id);
    if !k.merged_ids.is_empty() {
        println!(
            "Merged:     {}",
            k.merged_ids
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!("Title:      {}", title(k));
    println!("Status:     {}", k.status);
    println!("Kind:       {}", or_dash(k.kind.as_deref()));
    println!("Created:    {}", short_time(&k.created_at));
    println!("Updated:    {}", short_time(&k.updated_at));
    if let Some(i) = &k.incident_started_at {
        println!("Incident:   since {}", short_time(i));
    }
    println!("Scenarios:  {}", scenarios(k));
    if !k.keywords.is_empty() {
        println!("Keywords:   {}", k.keywords.join(", "));
    }
    if let Some(s) = &k.summary {
        println!();
        println!("Summary:");
        println!("{s}");
    }
    if let Some(r) = &k.resolution {
        println!();
        println!("Resolution:");
        println!("{r}");
    }
    if !d.messages.is_empty() {
        println!();
        println!("Conversation:");
        for m in &d.messages {
            println!();
            println!("[{}] {}:", short_time(&m.at), m.author);
            println!("{}", m.text);
        }
    }
    Ok(())
}

/// The id as the API routes it: a positive number.
fn ticket_id(id: &str) -> Result<i64> {
    match id.trim().parse::<i64>() {
        Ok(n) if n > 0 => Ok(n),
        _ => bail!("request id must be a positive number, got `{id}`"),
    }
}

/// The title, else the subject, on one line.
fn title(k: &Ticket) -> String {
    match k.title.as_deref().or(k.subject.as_deref()) {
        Some(t) if !t.trim().is_empty() => one_line(t, 80),
        _ => "-".to_string(),
    }
}

fn scenarios(k: &Ticket) -> String {
    if k.scenarios.is_empty() {
        "-".to_string()
    } else {
        k.scenarios
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_ids_are_numbers() {
        assert_eq!(ticket_id("7").unwrap(), 7);
        assert!(ticket_id("-1").is_err());
        assert!(ticket_id("x").is_err());
    }
}
