//! `2bec neutralizations`: set runs apart from the indicators, and undo it.

use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::alerts::canonical;
use super::planning::confirm;
use super::runs::ensure_guid;
use crate::api::neutralizations::{
    self, Neutralization, NeutralizationWrite, Preview, RunSelection,
};
use crate::api::page::{Fetch, fetch_items};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
#[command(after_help = "\
A neutralized run no longer counts in the availability and the other
indicators. Every command needs runs:write.")]
pub struct NeutralizationsArgs {
    #[command(subcommand)]
    command: NeutralizationsCommand,
}

#[derive(Subcommand, Debug)]
enum NeutralizationsCommand {
    /// List the operations of the space, newest first, whoever wrote them
    List {
        /// Only operations that touched this scenario id
        #[arg(long, value_name = "ID")]
        scenario: Option<String>,
        /// Only this kind: neutralize or restore
        #[arg(long, value_name = "KIND")]
        kind: Option<String>,
        /// Only this state: running, applied, undone or failed
        #[arg(long, value_name = "STATE")]
        state: Option<String>,
        /// Maximum number of operations to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every operation, following pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one operation and its counts; poll it while running
    Get {
        /// Operation id
        id: String,
        /// Poll until the operation is no longer running
        #[arg(long)]
        wait: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Neutralize the runs a selection matches, after a dry run and a confirmation
    #[command(after_help = "\
Examples:
  2bec neutralizations apply --from 2026-10-11T00:00:00Z --to 2026-10-12T00:00:00Z --scenario 7f3c... --dry-run
  2bec neutralizations apply --from … --to … --scenario 7f3c... --status failed --comment \"Known CDN outage\"
  2bec neutralizations apply --from … --to … --weekday 6 --weekday 7 --day-start 22:00 --day-stop 06:00 --yes

The period is required (366 days at most). Without --scenario the selection
covers every scenario the credential sees. The command runs a dry run first
and prints what would change; on a terminal it then asks, and applies pinned
to that dry run (409 selection-changed if runs changed meanwhile). --yes
skips the question (required when stdin is not a terminal); --dry-run stops
after the preview. --wait polls a background apply to its end.")]
    Apply {
        #[command(flatten)]
        selection: SelectionArgs,
        /// Why the runs are set apart (required when the preview says so)
        #[arg(long, value_name = "TEXT")]
        comment: Option<String>,
        /// Only preview what the apply would change
        #[arg(long, conflicts_with = "yes")]
        dry_run: bool,
        /// Apply without asking
        #[arg(long, short = 'y')]
        yes: bool,
        /// Poll until a background apply is done
        #[arg(long)]
        wait: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Put the runs an operation neutralized back to the status they had
    #[command(after_help = "\
Examples:
  2bec neutralizations undo 7f3c... --comment \"Outage was ours after all\"

409 neutralization-not-undoable for a restore, an operation still running
or one already undone. --wait polls a background undo to its end.")]
    Undo {
        /// Operation id
        id: String,
        /// Why
        #[arg(long, value_name = "TEXT")]
        comment: Option<String>,
        /// Poll until a background undo is done
        #[arg(long)]
        wait: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
}

/// The runs a bulk write targets.
#[derive(Args, Debug, Default)]
pub struct SelectionArgs {
    /// Start of the period, RFC 3339 (inclusive)
    #[arg(long, value_name = "TIME")]
    from: String,
    /// End of the period, RFC 3339 (exclusive)
    #[arg(long, value_name = "TIME")]
    to: String,
    /// Only runs of this scenario id (repeatable)
    #[arg(long = "scenario", value_name = "ID")]
    scenarios: Vec<String>,
    /// Only runs with this status: any, ok or failed [default: any]
    #[arg(long, value_name = "STATUS")]
    status: Option<String>,
    /// Only runs that failed at this step id (repeatable)
    #[arg(long = "failed-step", value_name = "ID")]
    failed_steps: Vec<String>,
    /// Only runs carrying this tag id (repeatable)
    #[arg(long = "tag", value_name = "ID")]
    tags: Vec<String>,
    /// Only runs on this ISO weekday, 1 = Monday (repeatable)
    #[arg(long = "weekday", value_name = "N")]
    weekdays: Vec<u8>,
    /// Only runs from this Paris time of day, HH:mm (with --day-stop)
    #[arg(long, value_name = "HH:MM", requires = "day_stop")]
    day_start: Option<String>,
    /// Only runs until this Paris time of day, HH:mm (with --day-start)
    #[arg(long, value_name = "HH:MM", requires = "day_start")]
    day_stop: Option<String>,
}

impl SelectionArgs {
    fn build(&self) -> Result<RunSelection> {
        for s in &self.scenarios {
            ensure_guid("--scenario", s)?;
        }
        for s in &self.failed_steps {
            ensure_guid("--failed-step", s)?;
        }
        for t in &self.tags {
            ensure_guid("--tag", t)?;
        }
        for d in &self.weekdays {
            if !(1..=7).contains(d) {
                bail!("--weekday must be 1 (Monday) to 7 (Sunday)");
            }
        }
        for (flag, v) in [
            ("--day-start", &self.day_start),
            ("--day-stop", &self.day_stop),
        ] {
            if let Some(t) = v
                && !is_hh_mm(t)
            {
                bail!("{flag} must be HH:mm");
            }
        }
        Ok(RunSelection {
            scenario: self.scenarios.clone(),
            from: Some(self.from.clone()),
            to: Some(self.to.clone()),
            status: self
                .status
                .as_deref()
                .map(|s| canonical("--status", s, &neutralizations::SELECTION_STATUSES))
                .transpose()?,
            failed_step: self.failed_steps.clone(),
            tag: self.tags.clone(),
            weekdays: self.weekdays.clone(),
            day_start: self.day_start.clone(),
            day_stop: self.day_stop.clone(),
        })
    }
}

fn is_hh_mm(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[3].is_ascii_digit()
        && b[4].is_ascii_digit()
        && t[..2].parse::<u8>().is_ok_and(|h| h < 24)
        && t[3..].parse::<u8>().is_ok_and(|m| m < 60)
}

pub async fn run(ctx: &Context, args: NeutralizationsArgs) -> Result<()> {
    match args.command {
        NeutralizationsCommand::List {
            scenario,
            kind,
            state,
            limit,
            all,
            output,
        } => list(ctx, scenario, kind, state, limit, all, output).await,
        NeutralizationsCommand::Get { id, wait, output } => {
            ensure_guid("operation id", &id)?;
            let resp = ctx
                .client()?
                .get(&format!("v2/neutralizations/{id}"))
                .await?;
            let mut n: Neutralization = resp.json()?;
            if wait {
                n = poll(ctx, &n).await?;
            }
            print_operation(&n, resp.etag.as_deref(), &output)
        }
        NeutralizationsCommand::Apply {
            selection,
            comment,
            dry_run,
            yes,
            wait,
            output,
        } => apply(ctx, &selection, comment, dry_run, yes, wait, output).await,
        NeutralizationsCommand::Undo {
            id,
            comment,
            wait,
            output,
        } => {
            ensure_guid("operation id", &id)?;
            let mut query = Vec::new();
            if let Some(c) = comment
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
            {
                query.push(("comment", c));
            }
            let resp = ctx
                .client()?
                .delete_with(
                    &format!("v2/neutralizations/{id}"),
                    &query,
                    &WriteOptions::idempotent(),
                )
                .await?;
            let mut n: Neutralization = resp.json()?;
            if !output.wants_json() {
                eprintln!(
                    "{} undo {} of operation {id}",
                    if resp.status == 202 {
                        "Started"
                    } else {
                        "Applied"
                    },
                    n.id
                );
            }
            if wait {
                n = poll(ctx, &n).await?;
            }
            print_operation(&n, resp.etag.as_deref(), &output)
        }
    }
}

async fn list(
    ctx: &Context,
    scenario: Option<String>,
    kind: Option<String>,
    state: Option<String>,
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let mut query = Vec::new();
    if let Some(s) = scenario {
        ensure_guid("--scenario", &s)?;
        query.push(("scenario", s));
    }
    if let Some(k) = kind {
        query.push(("kind", canonical("--kind", &k, &neutralizations::KINDS)?));
    }
    if let Some(s) = state {
        query.push(("state", canonical("--state", &s, &neutralizations::STATES)?));
    }
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page =
        fetch_items::<Neutralization>(&ctx.client()?, "v2/neutralizations", &query, fetch).await?;
    out.print(&page.items, "No neutralization operation", || {
        let mut t = Table::new([
            "ID",
            "KIND",
            "STATE",
            "CREATED",
            "BY",
            "SCENARIOS",
            "MATCHED",
            "APPLIED",
            "COMMENT",
        ]);
        for n in &page.items {
            t.row([
                n.id.clone(),
                n.kind.clone(),
                n.state.clone(),
                short_time(&n.created_at),
                n.created_by.clone(),
                scenario_names(n),
                n.matched.to_string(),
                n.applied.to_string(),
                or_dash(n.comment.as_deref()),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} operations; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

async fn apply(
    ctx: &Context,
    selection: &SelectionArgs,
    comment: Option<String>,
    dry_run: bool,
    yes: bool,
    wait: bool,
    out: OutputArgs,
) -> Result<()> {
    let selection = selection.build()?;
    let comment = comment
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());
    let client = ctx.client()?;
    let mut body = NeutralizationWrite {
        selection,
        comment,
        expected_matched: None,
        cap: None,
    };

    // The dry run is what the confirmation and the pin rest on; a scripted
    // --yes trusts the selection as it matches now and skips it.
    if !yes {
        let preview: Preview = client
            .post_json(
                "v2/neutralizations?dryRun=true",
                &body,
                &WriteOptions::idempotent(),
            )
            .await?
            .json()?;
        if dry_run && out.wants_json() {
            return out.print_json(&preview);
        }
        print_preview(&preview);
        if dry_run {
            return Ok(());
        }
        if preview.applicable == 0 {
            eprintln!("Nothing to neutralize");
            return Ok(());
        }
        if preview.comment_required && body.comment.is_none() {
            bail!("the platform requires a --comment for this selection");
        }
        if !confirm(&format!("Neutralize {} run(s)? [y/N] ", preview.applicable))? {
            bail!("not confirmed; pass --yes to skip the question");
        }
        body.expected_matched = Some(preview.matched);
        body.cap = preview.cap.clone();
    }

    let resp = client
        .post_json("v2/neutralizations", &body, &WriteOptions::idempotent())
        .await?;
    let mut n: Neutralization = resp.json()?;
    if !out.wants_json() {
        eprintln!(
            "{} operation {}",
            if resp.status == 202 {
                "Started"
            } else {
                "Applied"
            },
            n.id
        );
    }
    if wait {
        n = poll(ctx, &n).await?;
    }
    print_operation(&n, resp.etag.as_deref(), &out)
}

/// Re-read an operation every two seconds while it runs.
async fn poll(ctx: &Context, n: &Neutralization) -> Result<Neutralization> {
    let client = ctx.client()?;
    let mut current = n.clone();
    while current.state == "running" {
        tokio::time::sleep(Duration::from_secs(2)).await;
        current = client
            .get(&format!("v2/neutralizations/{}", n.id))
            .await?
            .json()?;
    }
    Ok(current)
}

fn print_preview(p: &Preview) {
    println!(
        "Would change:  {} of {} matched run(s) ({} already neutralized, {} protected)",
        p.applicable, p.matched, p.already_neutralized, p.protected
    );
    if let (Some(f), Some(l)) = (&p.first_run_at, &p.last_run_at) {
        println!("Runs from:     {} to {}", short_time(f), short_time(l));
    }
    if !p.statuses.is_empty() {
        println!(
            "By status:     {}",
            p.statuses
                .iter()
                .map(|s| format!("{} {}", s.count, s.status))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if p.comment_required {
        println!("Comment:       required by the platform");
    }
    if p.asynchronous {
        println!("Mode:          background (poll with `get`, or pass --wait)");
    }
    for w in &p.warnings {
        eprintln!("warning: {w}");
    }
    if !p.scenarios.is_empty() {
        println!();
        let mut t = Table::new(["SCENARIO", "NAME", "MATCHED", "APPLICABLE"]);
        for s in &p.scenarios {
            t.row([
                s.scenario.id.clone(),
                s.scenario.name.clone(),
                s.matched.to_string(),
                s.applicable.to_string(),
            ]);
        }
        t.print();
    }
    if !p.sample.is_empty() {
        println!();
        println!("Sample:");
        let mut t = Table::new([
            "RUN",
            "SCENARIO",
            "STARTED",
            "STATUS",
            "FAILED STEP",
            "OUTCOME",
        ]);
        for r in &p.sample {
            t.row([
                r.id.clone(),
                r.scenario.name.clone(),
                short_time(&r.started_at),
                r.status.clone(),
                or_dash(r.failed_step.as_deref()),
                r.outcome.clone(),
            ]);
        }
        t.print();
    }
}

fn scenario_names(n: &Neutralization) -> String {
    let mut s = match n.scenarios.len() {
        0 => String::new(),
        1 => n.scenarios[0].name.clone(),
        k => format!("{} +{}", n.scenarios[0].name, k - 1),
    };
    if n.hidden_scenario_count > 0 {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!("({} hidden)", n.hidden_scenario_count));
    }
    if s.is_empty() { "all".into() } else { s }
}

fn print_operation(n: &Neutralization, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(n)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!("Operation:    {}", n.id);
    println!("Kind:         {} ({}, from {})", n.kind, n.state, n.origin);
    if let Some(u) = &n.undoes {
        println!("Undoes:       {u}");
    }
    if let Some(u) = &n.undone_by {
        println!("Undone by:    {u}");
    }
    println!(
        "Period:       {}",
        n.period.as_ref().map_or("-".to_string(), |p| format!(
            "{} to {}",
            short_time(&p.from),
            short_time(&p.to)
        ))
    );
    println!("Scenarios:    {}", scenario_names(n));
    if let Some(sel) = &n.selection {
        let mut parts = Vec::new();
        if let Some(s) = &sel.status {
            parts.push(format!("status {s}"));
        }
        if !sel.failed_step.is_empty() {
            parts.push(format!("{} failed step(s)", sel.failed_step.len()));
        }
        if !sel.tag.is_empty() {
            parts.push(format!("{} tag(s)", sel.tag.len()));
        }
        if !sel.weekdays.is_empty() {
            parts.push(format!(
                "weekdays {}",
                sel.weekdays
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if let (Some(a), Some(b)) = (&sel.day_start, &sel.day_stop) {
            parts.push(format!("{a}-{b}"));
        }
        if !parts.is_empty() {
            println!("Filters:      {}", parts.join(", "));
        }
    }
    println!("Comment:      {}", or_dash(n.comment.as_deref()));
    println!(
        "Runs:         {} matched, {} applied, {} skipped",
        n.matched, n.applied, n.skipped
    );
    println!(
        "Created:      {} by {}",
        short_time(&n.created_at),
        n.created_by
    );
    println!(
        "Completed:    {}",
        n.completed_at
            .as_deref()
            .map_or("-".to_string(), short_time)
    );
    if let Some(e) = &n.error {
        println!("Error:        {e}");
    }
    println!("Undoable:     {}", if n.can_undo { "yes" } else { "no" });
    if let Some(e) = etag {
        println!("ETag:         {e}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_flags_are_checked() {
        let ok = SelectionArgs {
            from: "2026-10-01T00:00:00Z".into(),
            to: "2026-10-02T00:00:00Z".into(),
            status: Some("FAILED".into()),
            weekdays: vec![6, 7],
            day_start: Some("22:00".into()),
            day_stop: Some("06:00".into()),
            ..Default::default()
        };
        let s = ok.build().unwrap();
        assert_eq!(s.status.as_deref(), Some("failed"));
        assert_eq!(s.weekdays, vec![6, 7]);

        let bad_day = SelectionArgs {
            from: "a".into(),
            to: "b".into(),
            weekdays: vec![8],
            ..Default::default()
        };
        assert!(
            bad_day
                .build()
                .unwrap_err()
                .to_string()
                .contains("--weekday")
        );
        let bad_time = SelectionArgs {
            from: "a".into(),
            to: "b".into(),
            day_start: Some("25:00".into()),
            day_stop: Some("06:00".into()),
            ..Default::default()
        };
        assert!(bad_time.build().unwrap_err().to_string().contains("HH:mm"));
    }

    #[test]
    fn times_of_day_are_hh_mm() {
        assert!(is_hh_mm("00:00") && is_hh_mm("23:59"));
        assert!(!is_hh_mm("24:00") && !is_hh_mm("7:30") && !is_hh_mm("07-30"));
    }
}
