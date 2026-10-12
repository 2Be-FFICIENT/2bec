//! `2bec audit`: the administration history, who did what on the space.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::metrics::PeriodArgs;
use super::runs::ensure_guid;
use crate::api::governance::{self, AuditEntry, AuditEntryDetail};
use crate::api::page::{Fetch, fetch_items};
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
#[command(after_help = "\
Needs audit:read. The key reads the areas its scopes unlock: reports,
events and notifications with audit:read alone, an area needing a role with
the write scope implying it (tags and badges with indicators:write, users
and profiles with users:write, and so on). Staff-only rows are never
listed.")]
pub struct AuditArgs {
    #[command(subcommand)]
    command: AuditCommand,
}

// Parsed once; the size gap between the list's flags and a bare id does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
enum AuditCommand {
    /// List the history newest first over a period (the last 30 days by default, 366 at most)
    #[command(after_help = "\
Examples:
  2bec audit list
  2bec audit list --period last7d --area tags --area badges
  2bec audit list --actor <user-id> --outcome failed
  2bec audit list --scenario <id> --source api -q threshold --all --json
  2bec audit list --from 2026-01-01T00:00:00Z --to 2026-04-01T00:00:00Z --action tag.delete")]
    List {
        #[command(flatten)]
        period: PeriodArgs,
        /// Only these areas (repeatable)
        #[arg(long, value_name = "AREA", value_parser = governance::AUDIT_AREAS)]
        area: Vec<String>,
        /// Only these actions (repeatable), as the entries spell them
        #[arg(long, value_name = "ACTION")]
        action: Vec<String>,
        /// Only operations by this user (the user id)
        #[arg(long, value_name = "ID")]
        actor: Option<String>,
        /// Only operations touching this scenario
        #[arg(long, value_name = "ID")]
        scenario: Option<String>,
        /// Only operations with this outcome
        #[arg(long, value_name = "OUTCOME", value_parser = governance::AUDIT_OUTCOMES)]
        outcome: Option<String>,
        /// Only operations from this source
        #[arg(long, value_name = "SOURCE", value_parser = governance::AUDIT_SOURCES)]
        source: Option<String>,
        /// Only entries whose target, comment or parameters hold these words
        #[arg(long, short = 'q', value_name = "WORDS")]
        search: Option<String>,
        /// How many entries to show at most
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Every entry of the period, following the pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one entry with the state before and after it
    Get {
        /// Entry id (a number)
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: AuditArgs) -> Result<()> {
    match args.command {
        AuditCommand::List {
            period,
            area,
            action,
            actor,
            scenario,
            outcome,
            source,
            search,
            limit,
            all,
            output,
        } => {
            let filters = Filters {
                period,
                area,
                action,
                actor,
                scenario,
                outcome,
                source,
                search,
            };
            list(ctx, filters, limit, all, output).await
        }
        AuditCommand::Get { id, output } => get(ctx, &id, output).await,
    }
}

struct Filters {
    period: PeriodArgs,
    area: Vec<String>,
    action: Vec<String>,
    actor: Option<String>,
    scenario: Option<String>,
    outcome: Option<String>,
    source: Option<String>,
    search: Option<String>,
}

impl Filters {
    fn query(&self) -> Result<Vec<(&'static str, String)>> {
        let mut q = self.period.query()?;
        for a in &self.area {
            q.push(("area", a.clone()));
        }
        for a in &self.action {
            let a = a.trim();
            if a.is_empty() {
                bail!("--action cannot be blank");
            }
            q.push(("action", a.to_string()));
        }
        if let Some(actor) = &self.actor {
            ensure_guid("--actor", actor)?;
            q.push(("actor", actor.clone()));
        }
        if let Some(s) = &self.scenario {
            ensure_guid("--scenario", s)?;
            q.push(("scenario", s.clone()));
        }
        if let Some(o) = &self.outcome {
            q.push(("outcome", o.clone()));
        }
        if let Some(s) = &self.source {
            q.push(("source", s.clone()));
        }
        if let Some(words) = &self.search
            && !words.trim().is_empty()
        {
            q.push(("q", words.trim().to_string()));
        }
        Ok(q)
    }
}

async fn list(
    ctx: &Context,
    filters: Filters,
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let query = filters.query()?;
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page = fetch_items::<AuditEntry>(&ctx.client()?, "v2/audit-entries", &query, fetch).await?;
    out.print(&page.items, "No entry in this period", || {
        let mut t = Table::new([
            "ID", "AT", "AREA", "ACTION", "ACTOR", "SOURCE", "TARGET", "OUTCOME",
        ]);
        for e in &page.items {
            t.row([
                e.id.to_string(),
                short_time(&e.occurred_at),
                e.area.clone(),
                e.action.clone(),
                actor(e),
                e.source.clone(),
                target(e),
                outcome(e),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} entries; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    let id = entry_id(id)?;
    let d: AuditEntryDetail = ctx
        .client()?
        .get(&format!("v2/audit-entries/{id}"))
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&d);
    }
    let e = &d.entry;
    println!("Entry:      {}", e.id);
    println!("At:         {}", short_time(&e.occurred_at));
    println!("Area:       {}", e.area);
    println!("Action:     {}", e.action);
    println!("Actor:      {} ({})", actor(e), e.source);
    println!("Target:     {}", target(e));
    println!("Outcome:    {}", outcome(e));
    if let Some(err) = &e.error {
        println!("Error:      {err}");
    }
    println!("Comment:    {}", or_dash(e.comment.as_deref()));
    println!(
        "Reversible: {}{}",
        if e.reversible { "yes" } else { "no" },
        e.reverted_by_entry_id
            .map_or(String::new(), |r| format!(" (reverted by entry {r})"))
    );
    if let Some(r) = e.reverts_entry_id {
        println!("Reverts:    entry {r}");
    }
    if let Some(b) = &e.batch_id {
        println!("Batch:      {b}");
    }
    if !e.scenarios.is_empty() {
        println!(
            "Scenarios:  {}",
            e.scenarios
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for (label, value) in [
        ("Parameters", &e.parameters),
        ("Before", &d.before),
        ("After", &d.after),
    ] {
        if let Some(v) = value {
            println!();
            println!("{label}:");
            println!("{}", serde_json::to_string_pretty(v)?);
        }
    }
    Ok(())
}

/// The id as the API routes it: a positive number.
fn entry_id(id: &str) -> Result<i64> {
    match id.trim().parse::<i64>() {
        Ok(n) if n > 0 => Ok(n),
        _ => bail!("entry id must be a positive number, got `{id}`"),
    }
}

fn actor(e: &AuditEntry) -> String {
    let who = or_dash(e.actor.as_deref());
    if e.actor_is_staff {
        format!("{who} (staff)")
    } else {
        who
    }
}

/// `tag release` or `tag 7`, `-` without a target.
fn target(e: &AuditEntry) -> String {
    let what = e
        .target_label
        .as_deref()
        .or(e.target_id.as_deref())
        .unwrap_or("");
    match (e.target_type.as_deref(), what) {
        (None, "") => "-".to_string(),
        (None, w) => w.to_string(),
        (Some(t), "") => t.to_string(),
        (Some(t), w) => format!("{t} {w}"),
    }
}

fn outcome(e: &AuditEntry) -> String {
    match (e.outcome.as_str(), &e.error) {
        ("failed", Some(err)) => format!("failed: {err}"),
        (o, _) => o.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_ids_are_numbers() {
        assert_eq!(entry_id(" 42 ").unwrap(), 42);
        assert!(entry_id("0").is_err());
        assert!(entry_id("abc").is_err());
    }

    #[test]
    fn filters_become_query_parameters() {
        let f = Filters {
            period: PeriodArgs::default(),
            area: vec!["tags".into()],
            action: vec!["tag.create".into()],
            actor: None,
            scenario: None,
            outcome: Some("failed".into()),
            source: None,
            search: Some(" x ".into()),
        };
        assert_eq!(
            f.query().unwrap(),
            vec![
                ("area", "tags".to_string()),
                ("action", "tag.create".to_string()),
                ("outcome", "failed".to_string()),
                ("q", "x".to_string()),
            ]
        );
        let f = Filters {
            period: PeriodArgs::default(),
            area: vec![],
            action: vec![],
            actor: Some("nope".into()),
            scenario: None,
            outcome: None,
            source: None,
            search: None,
        };
        assert!(f.query().is_err());
    }
}
