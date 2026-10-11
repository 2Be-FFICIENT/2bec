//! `2bec planning`: the agenda of the scenarios (timeline), the emergency
//! stop (pauses) and the dated notes explaining the curves (events).

use std::io::{IsTerminal, Write};

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::alerts::canonical;
use super::metrics::PeriodArgs;
use super::runs::ensure_guid;
use crate::api::page::{Fetch, Page, fetch_items};
use crate::api::planning::{self, Event, EventWrite, Pause, PauseOutcome, Timeline};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
pub struct PlanningArgs {
    #[command(subcommand)]
    command: PlanningCommand,
}

#[derive(Subcommand, Debug)]
enum PlanningCommand {
    /// Everything dated on the scenarios: events, interruptions, script versions, incidents, pauses
    #[command(after_help = "\
Examples:
  2bec planning timeline                               # 7 days back to 7 days ahead
  2bec planning timeline --from 2026-10-01T00:00:00Z --to 2026-11-01T00:00:00Z --kind plannedInterruption
  2bec planning timeline --scenario 7f3c... --kind incident --incident-min-minutes 0

The range spans at most 367 days. Incidents are read over 62 days at most and,
by default, only the ones lasting an hour or more.")]
    Timeline {
        /// Only this scenario id (repeatable, at most 500)
        #[arg(long = "scenario", value_name = "ID")]
        scenarios: Vec<String>,
        /// Start of the range, RFC 3339 [default: 7 days ago]
        #[arg(long, value_name = "TIME")]
        from: Option<String>,
        /// End of the range, RFC 3339 [default: 7 days ahead]
        #[arg(long, value_name = "TIME")]
        to: Option<String>,
        /// Only this kind (repeatable): monitoringEvent, plannedInterruption, scriptVersion, incident, monitoringPause
        #[arg(long = "kind", value_name = "KIND")]
        kinds: Vec<String>,
        /// Shortest incident shown, in minutes (0 for every incident, over at most 4 scenarios)
        #[arg(long, value_name = "MINUTES")]
        incident_min_minutes: Option<u32>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// The scenarios whose monitoring is paused, and the mates sharing their script
    Pauses {
        /// Every active scenario with its state, not only the paused ones
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Emergency stop: stop running a scenario and its script mates, with no alert, until a resume
    #[command(after_help = "\
Examples:
  2bec planning pause 7f3c...                          # asks for confirmation on a terminal
  2bec planning pause 7f3c... --yes

The scenarios sharing the script are paused too and no alert goes out. Needs
monitoring:write.")]
    Pause {
        /// Scenario id
        id: String,
        /// Confirm without asking (required when stdin is not a terminal)
        #[arg(long, short = 'y')]
        yes: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Resume a paused scenario and its script mates; the stop becomes a planned interruption
    #[command(after_help = "\
Examples:
  2bec planning resume 7f3c... --comment \"Release 4.2 deployed\"

The comment (500 characters at most) is written on the interruption covering
the pause, so the indicators ignore the stop. Needs monitoring:write.")]
    Resume {
        /// Scenario id
        id: String,
        /// Why the monitoring was stopped, for the interruption's record
        #[arg(long, value_name = "TEXT")]
        comment: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Dated notes explaining the curves: releases, configuration changes, third-party incidents
    Events(EventsArgs),
}

#[derive(Args, Debug)]
pub struct EventsArgs {
    #[command(subcommand)]
    command: EventsCommand,
}

#[derive(Subcommand, Debug)]
enum EventsCommand {
    /// List the events starting in a period, newest first (last 30 days by default)
    #[command(after_help = "\
Examples:
  2bec planning events list
  2bec planning events list --period lastMonth --category release
  2bec planning events list --scenario 7f3c... --impact availability -q outage --all")]
    List {
        /// Only this scenario id (repeatable)
        #[arg(long = "scenario", value_name = "ID")]
        scenarios: Vec<String>,
        #[command(flatten)]
        period: PeriodArgs,
        /// Only this category (e.g. infrastructure, release, configuration, thirdParty, other)
        #[arg(long, value_name = "CATEGORY")]
        category: Option<String>,
        /// Only this expected impact: none, performance, availability or both
        #[arg(long, value_name = "IMPACT")]
        impact: Option<String>,
        /// Only events whose text matches these words
        #[arg(long, short = 'q', value_name = "WORDS")]
        search: Option<String>,
        /// Maximum number of events to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every event, following pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one event and its ETag
    Get {
        /// Event id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Post a dated note on scenarios, or on the whole space without --scenario
    #[command(after_help = "\
Examples:
  2bec planning events create --title \"Release 4.2\" --start 2026-10-12T10:00:00Z --category release
  2bec planning events create --title \"CDN incident\" --start 2026-10-12T08:00:00Z --end 2026-10-12T09:30:00Z \\
      --category thirdParty --impact availability --scenario 7f3c... --label cdn --url https://status.cdn.example

Without --end the event is an instant. Categories are the space's catalog
(infrastructure, release, configuration, thirdParty, other...). Needs
monitoring:write.")]
    Create {
        /// Title of the note
        #[arg(long, value_name = "TEXT")]
        title: String,
        /// Start, RFC 3339
        #[arg(long, value_name = "TIME")]
        start: String,
        /// End, RFC 3339; none for an instant
        #[arg(long, value_name = "TIME")]
        end: Option<String>,
        /// Category from the space's catalog
        #[arg(long, value_name = "CATEGORY")]
        category: Option<String>,
        /// Expected impact: none, performance, availability or both [default: none]
        #[arg(long, value_name = "IMPACT")]
        impact: Option<String>,
        /// Longer text
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
        /// External reference (a ticket, a change number)
        #[arg(long = "ref", value_name = "TEXT")]
        reference: Option<String>,
        /// External URL
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        /// Label (repeatable)
        #[arg(long = "label", value_name = "TEXT")]
        labels: Vec<String>,
        /// Scenario id the note is about (repeatable); none for the whole space
        #[arg(long = "scenario", value_name = "ID")]
        scenarios: Vec<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change an event; only the flags given change
    #[command(after_help = "\
Examples:
  2bec planning events edit 7f3c... --end 2026-10-12T11:00:00Z
  2bec planning events edit 7f3c... --description \"\" --ref \"\"      # clears them
  2bec planning events edit 7f3c... --label a --label b --if-match <etag>

An empty --description, --end, --ref or --url clears it; --label and
--scenario replace the whole list. Imported events are read-only, and only
the author or a Scenario Admin changes a manual one (403 event-read-only).")]
    Edit {
        /// Event id
        id: String,
        #[arg(long, value_name = "TEXT")]
        title: Option<String>,
        #[arg(long, value_name = "TIME")]
        start: Option<String>,
        #[arg(long, value_name = "TIME")]
        end: Option<String>,
        #[arg(long, value_name = "CATEGORY")]
        category: Option<String>,
        #[arg(long, value_name = "IMPACT")]
        impact: Option<String>,
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
        #[arg(long = "ref", value_name = "TEXT")]
        reference: Option<String>,
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        #[arg(long = "label", value_name = "TEXT")]
        labels: Vec<String>,
        #[arg(long = "scenario", value_name = "ID")]
        scenarios: Vec<String>,
        /// Only update if the event still has this ETag (from `events get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Take an event off the agenda (the space's history keeps it)
    #[command(alias = "rm")]
    Delete {
        /// Event id
        id: String,
        /// Only delete if the event still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
}

pub async fn run(ctx: &Context, args: PlanningArgs) -> Result<()> {
    match args.command {
        PlanningCommand::Timeline {
            scenarios,
            from,
            to,
            kinds,
            incident_min_minutes,
            output,
        } => {
            timeline(
                ctx,
                &scenarios,
                from,
                to,
                &kinds,
                incident_min_minutes,
                output,
            )
            .await
        }
        PlanningCommand::Pauses { all, output } => pauses(ctx, all, output).await,
        PlanningCommand::Pause { id, yes, output } => pause(ctx, &id, yes, output).await,
        PlanningCommand::Resume {
            id,
            comment,
            output,
        } => resume(ctx, &id, comment, output).await,
        PlanningCommand::Events(args) => events(ctx, args.command).await,
    }
}

async fn timeline(
    ctx: &Context,
    scenarios: &[String],
    from: Option<String>,
    to: Option<String>,
    kinds: &[String],
    incident_min_minutes: Option<u32>,
    out: OutputArgs,
) -> Result<()> {
    if scenarios.len() > 500 {
        bail!("at most 500 --scenario values");
    }
    let mut query = Vec::new();
    for s in scenarios {
        ensure_guid("--scenario", s)?;
        query.push(("scenario", s.clone()));
    }
    if let Some(f) = from {
        query.push(("from", f));
    }
    if let Some(t) = to {
        query.push(("to", t));
    }
    for k in kinds {
        query.push(("kind", canonical("--kind", k, &planning::KINDS)?));
    }
    if let Some(m) = incident_min_minutes {
        query.push(("incidentMinMinutes", m.to_string()));
    }
    let t: Timeline = ctx
        .client()?
        .get_with("v2/timeline", &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&t);
    }
    println!(
        "Range:  {} to {}",
        short_time(&t.period.from),
        short_time(&t.period.to)
    );
    for k in &t.unavailable {
        eprintln!("warning: {k} items could not be read");
    }
    for k in &t.skipped {
        eprintln!("note: {k} items left out (narrow the range or the scenarios)");
    }
    if t.truncated {
        eprintln!("warning: some items were left out by the source's cap");
    }
    if t.items.is_empty() {
        if crate::output::stdout_is_tty() {
            eprintln!("Nothing on the agenda in this range");
        }
        return Ok(());
    }
    println!();
    let mut tbl = Table::new([
        "START",
        "END",
        "KIND",
        "SEVERITY",
        "SCOPE",
        "SCENARIOS",
        "TITLE",
        "REF",
    ]);
    for i in &t.items {
        tbl.row([
            short_time(&i.start),
            match &i.end {
                Some(e) => short_time(e),
                None if i.ongoing => "ongoing".into(),
                None => "-".into(),
            },
            i.kind.clone(),
            i.severity.clone(),
            i.scope.clone(),
            scenario_names(&i.scenarios, i.hidden_scenarios),
            i.title.clone(),
            i.event_id
                .clone()
                .or_else(|| i.run_id.clone())
                .or_else(|| i.interruption_ids.as_ref().and_then(|v| v.first().cloned()))
                .unwrap_or_else(|| "-".into()),
        ]);
    }
    tbl.print();
    Ok(())
}

fn scenario_names(scenarios: &[planning::ScenarioRef], hidden: i64) -> String {
    let mut s = match scenarios.len() {
        0 => String::new(),
        1 => scenarios[0].name.clone(),
        n => format!("{} +{}", scenarios[0].name, n - 1),
    };
    if hidden > 0 {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!("({hidden} hidden)"));
    }
    if s.is_empty() { "-".into() } else { s }
}

async fn pauses(ctx: &Context, all: bool, out: OutputArgs) -> Result<()> {
    let query: Vec<(&str, String)> = if all {
        vec![("all", "true".into())]
    } else {
        vec![]
    };
    let page: Page<Pause> = ctx.client()?.get_with("v2/pauses", &query).await?.json()?;
    out.print(&page.items, "No paused scenario", || {
        pause_table(&page.items)
    })
}

fn pause_table(items: &[Pause]) -> Table {
    let mut t = Table::new([
        "ID",
        "NAME",
        "PAUSED",
        "SINCE",
        "LAST RESUMED",
        "SHARES SCRIPT WITH",
    ]);
    for p in items {
        t.row([
            p.scenario.id.clone(),
            p.scenario.name.clone(),
            if p.paused { "yes" } else { "no" }.to_string(),
            p.paused_since
                .as_deref()
                .map_or("-".to_string(), short_time),
            p.last_resumed_at
                .as_deref()
                .map_or("-".to_string(), short_time),
            {
                let mut names: Vec<String> = p.shared_with.iter().map(|s| s.name.clone()).collect();
                if p.hidden_shared_with > 0 {
                    names.push(format!("{} hidden", p.hidden_shared_with));
                }
                if names.is_empty() {
                    "-".into()
                } else {
                    names.join(", ")
                }
            },
        ]);
    }
    t
}

async fn pause(ctx: &Context, id: &str, yes: bool, out: OutputArgs) -> Result<()> {
    ensure_guid("scenario id", id)?;
    if !yes
        && !confirm(&format!(
            "Pause the monitoring of {id} and of the scenarios sharing its script? No alert will go out until a resume. [y/N] "
        ))?
    {
        bail!("not confirmed; pass --yes to skip the question");
    }
    let o: PauseOutcome = ctx
        .client()?
        .post_json(
            &format!("v2/scenarios/{id}:pause"),
            &serde_json::json!({"acknowledge": true}),
            &WriteOptions::idempotent(),
        )
        .await?
        .json()?;
    print_outcome(&o, &out)
}

/// Ask on the terminal; `false` without one.
fn confirm(question: &str) -> Result<bool> {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Ok(false);
    }
    eprint!("{question}");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    stdin.read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

async fn resume(ctx: &Context, id: &str, comment: Option<String>, out: OutputArgs) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let comment = comment
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());
    if let Some(c) = &comment
        && c.chars().count() > planning::MAX_COMMENT
    {
        bail!("--comment is {} characters at most", planning::MAX_COMMENT);
    }
    let body = match comment {
        Some(c) => serde_json::json!({"comment": c}),
        None => serde_json::json!({}),
    };
    let o: PauseOutcome = ctx
        .client()?
        .post_json(
            &format!("v2/scenarios/{id}:resume"),
            &body,
            &WriteOptions::idempotent(),
        )
        .await?
        .json()?;
    print_outcome(&o, &out)
}

fn print_outcome(o: &PauseOutcome, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        return out.print_json(o);
    }
    println!(
        "{} at {}: {} scenario(s) changed, {} already so{}",
        if o.action == "pause" {
            "Paused"
        } else {
            "Resumed"
        },
        short_time(&o.at),
        o.changed.len(),
        o.unchanged.len(),
        if o.hidden_changed > 0 {
            format!(", {} hidden mate(s) moved too", o.hidden_changed)
        } else {
            String::new()
        }
    );
    for w in &o.warnings {
        eprintln!("warning: {w}");
    }
    if !o.changed.is_empty() {
        println!();
        pause_table(&o.changed).print();
    }
    if !o.interruptions.is_empty() {
        println!();
        println!("Interruptions written over the pause:");
        let mut t = Table::new(["START", "END", "SCENARIOS", "COMMENT"]);
        for i in &o.interruptions {
            t.row([
                short_time(&i.start),
                short_time(&i.end),
                i.scenarios
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                or_dash(i.comment.as_deref()),
            ]);
        }
        t.print();
    }
    Ok(())
}

// ---------- events ----------

async fn events(ctx: &Context, cmd: EventsCommand) -> Result<()> {
    match cmd {
        EventsCommand::List {
            scenarios,
            period,
            category,
            impact,
            search,
            limit,
            all,
            output,
        } => {
            if !all && limit == 0 {
                bail!("--limit must be at least 1");
            }
            let mut query = Vec::new();
            for s in &scenarios {
                ensure_guid("--scenario", s)?;
                query.push(("scenario", s.clone()));
            }
            query.extend(period.query()?);
            if let Some(c) = category {
                query.push(("category", c));
            }
            if let Some(i) = impact {
                query.push(("impact", canonical("--impact", &i, &planning::IMPACTS)?));
            }
            if let Some(q) = search
                && !q.trim().is_empty()
            {
                query.push(("q", q.trim().to_string()));
            }
            let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
            let page = fetch_items::<Event>(&ctx.client()?, "v2/events", &query, fetch).await?;
            output.print(&page.items, "No event in this period", || {
                let mut t = Table::new([
                    "ID",
                    "START",
                    "END",
                    "CATEGORY",
                    "IMPACT",
                    "SCOPE",
                    "SCENARIOS",
                    "TITLE",
                ]);
                for e in &page.items {
                    t.row([
                        e.id.clone(),
                        short_time(&e.start),
                        e.end.as_deref().map_or("-".to_string(), short_time),
                        e.category.clone(),
                        e.expected_impact.clone(),
                        e.scope.clone(),
                        scenario_names(&e.scenarios, e.hidden_scenarios),
                        e.title.clone(),
                    ]);
                }
                t
            })?;
            if !output.wants_json()
                && let Some(total) = page.total
                && (total as usize) > page.items.len()
            {
                eprintln!(
                    "Showing {} of {total} events; use --limit or --all for more",
                    page.items.len()
                );
            }
            Ok(())
        }
        EventsCommand::Get { id, output } => {
            ensure_guid("event id", &id)?;
            let resp = ctx.client()?.get(&format!("v2/events/{id}")).await?;
            let e: Event = resp.json()?;
            print_event(&e, resp.etag.as_deref(), &output)
        }
        EventsCommand::Create {
            title,
            start,
            end,
            category,
            impact,
            description,
            reference,
            url,
            labels,
            scenarios,
            output,
        } => {
            let title = title.trim().to_string();
            if title.is_empty() {
                bail!("--title is empty");
            }
            for s in &scenarios {
                ensure_guid("--scenario", s)?;
            }
            let body = EventWrite {
                category: category.map(|c| c.trim().to_string()),
                expected_impact: impact
                    .map(|i| canonical("--impact", &i, &planning::IMPACTS))
                    .transpose()?,
                title,
                description: non_empty(description),
                start,
                end: non_empty(end),
                external_reference: non_empty(reference),
                external_url: non_empty(url),
                labels: labels
                    .into_iter()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect(),
                scenarios,
            };
            let resp = ctx
                .client()?
                .post_json("v2/events", &body, &WriteOptions::idempotent())
                .await?;
            let e: Event = resp.json()?;
            if !output.wants_json() {
                eprintln!("Created event {}", e.id);
            }
            print_event(&e, resp.etag.as_deref(), &output)
        }
        EventsCommand::Edit {
            id,
            title,
            start,
            end,
            category,
            impact,
            description,
            reference,
            url,
            labels,
            scenarios,
            if_match,
            output,
        } => {
            ensure_guid("event id", &id)?;
            let mut patch = serde_json::Map::new();
            if let Some(t) = title {
                let t = t.trim().to_string();
                if t.is_empty() {
                    bail!("--title cannot be cleared");
                }
                patch.insert("title".into(), t.into());
            }
            if let Some(s) = start {
                patch.insert("start".into(), s.into());
            }
            if let Some(c) = category {
                patch.insert("category".into(), c.trim().to_string().into());
            }
            if let Some(i) = impact {
                patch.insert(
                    "expectedImpact".into(),
                    canonical("--impact", &i, &planning::IMPACTS)?.into(),
                );
            }
            for (field, value) in [
                ("end", end),
                ("description", description),
                ("externalReference", reference),
                ("externalUrl", url),
            ] {
                if let Some(v) = value {
                    patch.insert(field.into(), nullable(v));
                }
            }
            if !labels.is_empty() {
                let labels: Vec<String> = labels
                    .into_iter()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                patch.insert("labels".into(), labels.into());
            }
            if !scenarios.is_empty() {
                let ids: Vec<String> = scenarios
                    .into_iter()
                    .filter(|s| !s.trim().is_empty())
                    .collect();
                for s in &ids {
                    ensure_guid("--scenario", s)?;
                }
                patch.insert("scenarios".into(), ids.into());
            }
            if patch.is_empty() {
                bail!("nothing to change: give at least one flag");
            }
            let write = WriteOptions {
                idempotency_key: None,
                if_match,
            };
            let resp = ctx
                .client()?
                .patch_merge(&format!("v2/events/{id}"), &patch, &write)
                .await?;
            let e: Event = resp.json()?;
            print_event(&e, resp.etag.as_deref(), &output)
        }
        EventsCommand::Delete { id, if_match } => {
            ensure_guid("event id", &id)?;
            let write = WriteOptions {
                idempotency_key: None,
                if_match,
            };
            ctx.client()?
                .delete(&format!("v2/events/{id}"), &write)
                .await?;
            eprintln!("Deleted event {id}");
            Ok(())
        }
    }
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// An empty flag value clears the member in a merge patch.
fn nullable(v: String) -> serde_json::Value {
    match non_empty(Some(v)) {
        Some(s) => s.into(),
        None => serde_json::Value::Null,
    }
}

fn print_event(e: &Event, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(e)?;
        if let Some(t) = etag {
            v["etag"] = t.into();
        }
        return out.print_json(&v);
    }
    println!("Event:        {}", e.id);
    println!("Title:        {}", e.title);
    println!(
        "When:         {}{}",
        short_time(&e.start),
        match &e.end {
            Some(end) => format!(" to {}", short_time(end)),
            None => " (instant)".into(),
        }
    );
    println!("Category:     {}", e.category);
    println!("Impact:       {}", e.expected_impact);
    println!(
        "Scope:        {}{}",
        e.scope,
        if e.scenarios.is_empty() && e.hidden_scenarios == 0 {
            String::new()
        } else {
            format!(" ({})", scenario_names(&e.scenarios, e.hidden_scenarios))
        }
    );
    println!("Description:  {}", or_dash(e.description.as_deref()));
    println!(
        "Reference:    {}{}",
        or_dash(e.external_reference.as_deref()),
        e.external_url
            .as_deref()
            .map_or(String::new(), |u| format!(" {u}"))
    );
    println!(
        "Labels:       {}",
        if e.labels.is_empty() {
            "-".into()
        } else {
            e.labels.join(", ")
        }
    );
    println!(
        "Source:       {}, {} attachment(s)",
        e.source, e.attachments
    );
    println!(
        "Created:      {} by {}",
        short_time(&e.created_at),
        or_dash(e.created_by.as_deref())
    );
    if let Some(u) = &e.updated_at {
        println!(
            "Updated:      {} by {}",
            short_time(u),
            or_dash(e.updated_by.as_deref())
        );
    }
    if let Some(t) = etag {
        println!("ETag:         {t}");
    }
    if e.scenarios.len() > 1 {
        println!();
        let mut t = Table::new(["SCENARIO", "NAME"]);
        for s in &e.scenarios {
            t.row([s.id.clone(), s.name.clone()]);
        }
        t.print();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_names_fold_and_count_hidden() {
        let r = |n: &str| planning::ScenarioRef {
            id: n.into(),
            name: n.into(),
            kind: "transaction".into(),
        };
        assert_eq!(scenario_names(&[], 0), "-");
        assert_eq!(scenario_names(&[r("A")], 0), "A");
        assert_eq!(scenario_names(&[r("A"), r("B"), r("C")], 0), "A +2");
        assert_eq!(scenario_names(&[r("A")], 2), "A (2 hidden)");
        assert_eq!(scenario_names(&[], 3), "(3 hidden)");
    }

    #[test]
    fn empty_flag_values_clear_members() {
        assert_eq!(nullable("  ".into()), serde_json::Value::Null);
        assert_eq!(nullable(" x ".into()), serde_json::Value::from("x"));
    }
}
