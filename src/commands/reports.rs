//! `2bec reports`: the saved reports, their data and PDFs, the scheduled
//! e-mails and what was actually sent.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::alerts::canonical;
use super::metrics::{self as fmt, PeriodArgs};
use super::runs::ensure_guid;
use crate::api::page::{Fetch, Page, fetch_items};
use crate::api::reports::{
    self, ComparativeReportData, Delivery, GroupedReportData, Report, ReportData, ReportSummary,
    Schedule, ScheduleSent, ScheduleWrite,
};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
pub struct ReportsArgs {
    #[command(subcommand)]
    command: ReportsCommand,
}

#[derive(Subcommand, Debug)]
enum ReportsCommand {
    /// List the saved reports the credential sees: grouped and comparative
    List {
        /// Only this kind: grouped or comparative
        #[arg(long, value_name = "KIND")]
        kind: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show a report's definition and its ETag
    Get {
        /// Report id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Compute a report over a period
    #[command(after_help = "\
Examples:
  2bec reports data 7f3c...                           # grouped: this month; comparative: last 7 days
  2bec reports data 7f3c... --window week --date 2026-10-05
  2bec reports data 7f3c... --period lastMonth --stat p50 --json

A grouped report takes --window (month, week or day) holding --date; a
comparative one takes --from/--to or --period, --tz and --stat (p85, p50 or
mean). The full document is in --json; the table shows the summary.")]
    Data {
        /// Report id
        id: String,
        #[command(flatten)]
        query: DataQuery,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Download a report as the page prints it
    #[command(after_help = "\
Examples:
  2bec reports pdf 7f3c... -o monthly.pdf
  2bec reports pdf 7f3c... --window week --lang en --planning plannedInterruption -o - | lp

Same period flags as `reports data`; --lang fr or en; --planning (repeatable)
adds monitoringEvent, plannedInterruption or scriptVersion annotations.")]
    Pdf {
        /// Report id
        id: String,
        #[command(flatten)]
        query: DataQuery,
        #[command(flatten)]
        pdf: PdfArgs,
        /// Destination file; `-` for stdout [default: report-<id>.pdf]
        #[arg(long, short = 'o', value_name = "FILE")]
        output: Option<String>,
    },
    /// Create a saved report from a JSON definition (needs reports:write)
    #[command(after_help = "\
Examples:
  2bec reports create @grouped.json
  echo '{\"kind\": \"comparative\", \"name\": \"A vs B\", \"scenarios\": [...]}' | 2bec reports create -

The body is the definition: `kind` grouped (name, scenarios with optional
displayName, sections, trendPeriods, comparisonPeriods, locale, groups) or
comparative (name, one or two scenarios with their steps, stat, pairs);
`visibility` client (default) or private. The report belongs to the API:
any key with reports:write changes it. 409 conflict on a duplicate
comparative name, 409 limit-reached past 100 comparative reports.")]
    Create {
        /// JSON body; `-` for stdin, `@file` for a file
        body: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change a saved report with a JSON merge patch (needs reports:write)
    #[command(after_help = "\
Examples:
  2bec reports edit 7f3c... '{\"name\": \"Monthly SLA\"}'
  2bec reports edit 7f3c... @patch.json --if-match \"$(2bec reports get 7f3c... --jq .etag)\"

A present member replaces (arrays whole), null brings an optional member
back to its default, an absent one stays. The kind is fixed. Only a report
the API created can be changed (403 not-own-report).")]
    Edit {
        /// Report id
        id: String,
        /// JSON merge patch; `-` for stdin, `@file` for a file
        body: String,
        /// Only update if the report still has this ETag (from `reports get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete a report the API created; a grouped one is archived unless --purge
    #[command(alias = "rm")]
    Delete {
        /// Report id
        id: String,
        /// Erase a grouped report instead of archiving it
        #[arg(long)]
        purge: bool,
        /// Only delete if the report still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
    /// The scheduled e-mails of reports
    Schedules(SchedulesArgs),
    /// The scheduled reports actually sent by e-mail (last 30 days by default)
    #[command(after_help = "\
Examples:
  2bec reports deliveries
  2bec reports deliveries --status failed --period lastMonth
  2bec reports deliveries --schedule 7f3c... --all --json

Statuses: sent, failed, skipped (the period was already delivered), resent
(a manual send over a delivered period). PDF downloads are not deliveries.")]
    Deliveries {
        #[command(flatten)]
        period: PeriodArgs,
        /// Only this report type (see `schedules list`)
        #[arg(long = "type", value_name = "TYPE")]
        report_type: Option<String>,
        /// Only this outcome: sent, failed, skipped or resent
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
        /// Only this schedule id
        #[arg(long, value_name = "ID")]
        schedule: Option<String>,
        /// Maximum number of deliveries to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every delivery, following pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
}

/// The period of a report's data or PDF: a window for a grouped report, a
/// range of days for a comparative one.
#[derive(Args, Debug, Default)]
pub struct DataQuery {
    /// Grouped report: month, week or day [default: month]
    #[arg(long, value_name = "WINDOW", conflicts_with_all = ["from", "to", "period"])]
    window: Option<String>,
    /// Grouped report: a day of the window, YYYY-MM-DD [default: today]
    #[arg(long, value_name = "DATE", conflicts_with_all = ["from", "to", "period"])]
    date: Option<String>,
    #[command(flatten)]
    period: PeriodArgs,
    /// Comparative report: p85, p50 or mean [default: the report's]
    #[arg(long, value_name = "STAT", conflicts_with_all = ["window", "date"])]
    stat: Option<String>,
}

impl DataQuery {
    fn query(&self) -> Result<Vec<(&'static str, String)>> {
        let mut q = Vec::new();
        if let Some(w) = &self.window {
            q.push(("window", canonical("--window", w, &reports::WINDOWS)?));
        }
        if let Some(d) = &self.date {
            if d.len() != 10 || d.as_bytes()[4] != b'-' || d.as_bytes()[7] != b'-' {
                bail!("--date must be YYYY-MM-DD");
            }
            q.push(("date", d.clone()));
        }
        q.extend(self.period.query()?);
        if let Some(s) = &self.stat {
            q.push(("stat", canonical("--stat", s, &reports::COMPARATIVE_STATS)?));
        }
        Ok(q)
    }
}

/// The flags every PDF takes.
#[derive(Args, Debug, Default)]
pub struct PdfArgs {
    /// Language of the document: fr or en
    #[arg(long, value_name = "LANG")]
    lang: Option<String>,
    /// Planning annotation to print (repeatable): monitoringEvent, plannedInterruption, scriptVersion
    #[arg(long = "planning", value_name = "KIND")]
    planning: Vec<String>,
}

impl PdfArgs {
    /// Validate locally and turn into query parameters.
    pub fn query(&self) -> Result<Vec<(&'static str, String)>> {
        let mut q = Vec::new();
        if let Some(l) = &self.lang {
            q.push(("lang", canonical("--lang", l, &reports::LANGS)?));
        }
        if !self.planning.is_empty() {
            let mut kinds = Vec::new();
            for k in &self.planning {
                let k = canonical("--planning", k, &reports::PLANNING_KINDS)?;
                if !kinds.contains(&k) {
                    kinds.push(k);
                }
            }
            q.push(("planningKinds", kinds.join(",")));
        }
        Ok(q)
    }
}

#[derive(Args, Debug)]
pub struct SchedulesArgs {
    #[command(subcommand)]
    command: SchedulesCommand,
}

#[derive(Subcommand, Debug)]
enum SchedulesCommand {
    /// List the schedules: type, scenarios or report, cadence, recipients, last sending
    List {
        /// Only this report type
        #[arg(long = "type", value_name = "TYPE")]
        report_type: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one schedule and its ETag
    Get {
        /// Schedule id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Mail a report on a cadence (needs reports:write)
    #[command(after_help = "\
Examples:
  2bec reports schedules create --type scenario-synthesis --scenario 7f3c... --frequency weekly --to ops@example.com
  2bec reports schedules create --type availability-dashboard --scenario 7f3c... --scenario 9a1d... \\
      --frequency monthly --to sla@example.com --cc boss@example.com --locale fr --planning plannedInterruption
  2bec reports schedules create --type comparative --report 1a2b... --frequency daily --to ops@example.com

Types: scenario-synthesis (one scenario), availability-dashboard (1 to 100),
response-times (1 to 15, --stat p85|p50|mean|max), voice-quality (1 to 20),
grouped-availability or comparative (--report, --stat p85|p50|mean for a
comparative one). Each mail covers the last complete period. 1 to 20
recipients in all. The key is the schedule's creator: revoking it stops the
schedule. 20 schedules per space (409 limit-reached).")]
    Create {
        /// Report type
        #[arg(long = "type", value_name = "TYPE")]
        report_type: String,
        /// Scenario id (repeatable), for the scenario-based types
        #[arg(long = "scenario", value_name = "ID")]
        scenarios: Vec<String>,
        /// Saved report id, for grouped-availability and comparative
        #[arg(long, value_name = "ID")]
        report: Option<String>,
        /// Statistic: p85, p50, mean or max
        #[arg(long, value_name = "STAT")]
        stat: Option<String>,
        /// Cadence: daily, weekly or monthly
        #[arg(long, value_name = "CADENCE")]
        frequency: String,
        /// Language of the mail and PDF: en or fr [default: en]
        #[arg(long, value_name = "LANG")]
        locale: Option<String>,
        /// Recipient address (repeatable, at least one)
        #[arg(long = "to", value_name = "ADDRESS", required = true)]
        to: Vec<String>,
        /// Copied address (repeatable)
        #[arg(long = "cc", value_name = "ADDRESS")]
        cc: Vec<String>,
        /// Planning annotation printed in the PDF (repeatable)
        #[arg(long = "planning", value_name = "KIND")]
        planning: Vec<String>,
        /// Create it paused
        #[arg(long)]
        disabled: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change a schedule; only the flags given change (needs reports:write)
    #[command(after_help = "\
Examples:
  2bec reports schedules edit 7f3c... --enabled false
  2bec reports schedules edit 7f3c... --to a@example.com --to b@example.com --if-match <etag>

Lists (--scenario, --to, --cc, --planning) replace the whole list; --cc \"\"
or --planning \"\" clears one. The type and the creator are fixed.")]
    Edit {
        /// Schedule id
        id: String,
        /// New scenario list (repeatable)
        #[arg(long = "scenario", value_name = "ID")]
        scenarios: Vec<String>,
        /// New saved report id
        #[arg(long, value_name = "ID")]
        report: Option<String>,
        /// New statistic
        #[arg(long, value_name = "STAT")]
        stat: Option<String>,
        /// New cadence: daily, weekly or monthly
        #[arg(long, value_name = "CADENCE")]
        frequency: Option<String>,
        /// New language: en or fr
        #[arg(long, value_name = "LANG")]
        locale: Option<String>,
        /// Pause (false) or resume (true) the schedule
        #[arg(long, value_name = "BOOL")]
        enabled: Option<bool>,
        /// New recipient list (repeatable)
        #[arg(long = "to", value_name = "ADDRESS")]
        to: Vec<String>,
        /// New copy list (repeatable; "" clears it)
        #[arg(long = "cc", value_name = "ADDRESS")]
        cc: Vec<String>,
        /// New planning annotations (repeatable; "" clears them)
        #[arg(long = "planning", value_name = "KIND")]
        planning: Vec<String>,
        /// Only update if the schedule still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete a schedule and its deliveries
    #[command(alias = "rm")]
    Delete {
        /// Schedule id
        id: String,
        /// Only delete if the schedule still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
    /// Build the report over its last complete period and mail it now (a real e-mail)
    Send {
        /// Schedule id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: ReportsArgs) -> Result<()> {
    match args.command {
        ReportsCommand::List { kind, output } => list(ctx, kind.as_deref(), output).await,
        ReportsCommand::Get { id, output } => get(ctx, &id, output).await,
        ReportsCommand::Data { id, query, output } => data(ctx, &id, &query, output).await,
        ReportsCommand::Pdf {
            id,
            query,
            pdf,
            output,
        } => {
            ensure_guid("report id", &id)?;
            let mut q = query.query()?;
            q.extend(pdf.query()?);
            let target = output.unwrap_or_else(|| format!("report-{id}.pdf"));
            super::download_to_with(&ctx.client()?, &format!("v2/reports/{id}/pdf"), &q, &target)
                .await
        }
        ReportsCommand::Create { body, output } => create(ctx, &body, output).await,
        ReportsCommand::Edit {
            id,
            body,
            if_match,
            output,
        } => edit(ctx, &id, &body, if_match, output).await,
        ReportsCommand::Delete {
            id,
            purge,
            if_match,
        } => delete(ctx, &id, purge, if_match).await,
        ReportsCommand::Schedules(args) => schedules(ctx, args.command).await,
        ReportsCommand::Deliveries {
            period,
            report_type,
            status,
            schedule,
            limit,
            all,
            output,
        } => {
            deliveries(
                ctx,
                &period,
                report_type.as_deref(),
                status.as_deref(),
                schedule.as_deref(),
                limit,
                all,
                output,
            )
            .await
        }
    }
}

// ---------- saved reports ----------

async fn list(ctx: &Context, kind: Option<&str>, out: OutputArgs) -> Result<()> {
    let mut query = Vec::new();
    if let Some(k) = kind {
        query.push(("kind", canonical("--kind", k, &reports::KINDS)?));
    }
    let page: Page<ReportSummary> = ctx.client()?.get_with("v2/reports", &query).await?.json()?;
    out.print(&page.items, "No saved report visible", || {
        let mut t = Table::new([
            "ID",
            "KIND",
            "NAME",
            "VISIBILITY",
            "SCENARIOS",
            "GROUPS",
            "OWNER",
            "UPDATED",
            "API",
        ]);
        for r in &page.items {
            t.row([
                r.id.clone(),
                r.kind.clone(),
                r.name.clone(),
                r.visibility.clone(),
                r.scenario_count.to_string(),
                if r.groups.is_empty() {
                    "-".into()
                } else {
                    r.groups.join(", ")
                },
                or_dash(r.owner_name.as_deref()),
                short_time(&r.updated_at),
                if r.api_owned { "yes" } else { "" }.to_string(),
            ]);
        }
        t
    })
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("report id", id)?;
    let resp = ctx.client()?.get(&format!("v2/reports/{id}")).await?;
    let r: Report = resp.json()?;
    print_report(&r, resp.etag.as_deref(), &out)
}

fn print_report(r: &Report, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(r)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    match r {
        Report::Grouped(g) => {
            println!("Report:       {}", g.id);
            println!("Kind:         grouped (availability and failures)");
            println!("Name:         {}", g.name);
            println!("Description:  {}", or_dash(g.description.as_deref()));
            println!(
                "Visibility:   {}{}",
                g.visibility,
                owned(g.api_owned, g.owner_name.as_deref())
            );
            println!("Sections:     {}", g.sections.join(", "));
            println!(
                "Periods:      {} in the trend, {} compared",
                g.trend_periods, g.comparison_periods
            );
            println!("Locale:       {}", g.locale);
            println!("Created:      {}", short_time(&g.created_at));
            println!(
                "Updated:      {}",
                g.updated_at.as_deref().map_or("-".to_string(), short_time)
            );
            if let Some(e) = etag {
                println!("ETag:         {e}");
            }
            if !g.scenarios.is_empty() {
                println!();
                let mut t = Table::new(["#", "SCENARIO", "NAME", "SHOWN AS", "ACTIVE"]);
                for s in &g.scenarios {
                    t.row([
                        s.position.to_string(),
                        s.scenario.id.clone(),
                        s.scenario.name.clone(),
                        or_dash(s.display_name.as_deref()),
                        if s.active { "yes" } else { "no" }.to_string(),
                    ]);
                }
                t.print();
            }
            if !g.groups.is_empty() {
                println!();
                println!("Groups:");
                let mut t = Table::new(["#", "ID", "NAME", "AGGREGATION", "SCENARIOS"]);
                for gr in &g.groups {
                    t.row([
                        gr.position.to_string(),
                        gr.id.clone(),
                        gr.name.clone(),
                        gr.aggregation.clone(),
                        gr.scenario_ids.len().to_string(),
                    ]);
                }
                t.print();
            }
        }
        Report::Comparative(c) => {
            println!("Report:       {}", c.id);
            println!("Kind:         comparative (scenarios side by side)");
            println!("Name:         {}", c.name);
            println!("Description:  {}", or_dash(c.description.as_deref()));
            println!(
                "Visibility:   {}{}",
                c.visibility,
                owned(c.api_owned, c.owner_name.as_deref())
            );
            println!("Statistic:    {}", c.stat);
            println!("Created:      {}", short_time(&c.created_at));
            println!("Updated:      {}", short_time(&c.updated_at));
            if let Some(e) = etag {
                println!("ETag:         {e}");
            }
            if !c.scenarios.is_empty() {
                println!();
                let mut t = Table::new(["SCENARIO", "NAME", "PLATFORM", "STEPS"]);
                for s in &c.scenarios {
                    t.row([
                        s.scenario.id.clone(),
                        s.scenario.name.clone(),
                        match (&s.os, &s.browser) {
                            (Some(o), Some(b)) => format!("{o} / {b}"),
                            (Some(o), None) => o.clone(),
                            (None, Some(b)) => b.clone(),
                            (None, None) => "-".into(),
                        },
                        s.step_ids.len().to_string(),
                    ]);
                }
                t.print();
            }
            if !c.pairs.is_empty() {
                println!();
                println!("Pairs:");
                let mut t = Table::new(["A", "B", "LABEL"]);
                for p in &c.pairs {
                    t.row([
                        or_dash(p.a.as_deref()),
                        or_dash(p.b.as_deref()),
                        or_dash(p.label.as_deref()),
                    ]);
                }
                t.print();
            }
        }
    }
    Ok(())
}

fn owned(api_owned: bool, owner: Option<&str>) -> String {
    match (api_owned, owner) {
        (true, _) => " (created by the API)".into(),
        (false, Some(o)) => format!(" (owner {o})"),
        (false, None) => String::new(),
    }
}

async fn data(ctx: &Context, id: &str, query: &DataQuery, out: OutputArgs) -> Result<()> {
    ensure_guid("report id", id)?;
    let q = query.query()?;
    let d: ReportData = ctx
        .client()?
        .get_with(&format!("v2/reports/{id}/data"), &q)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&d);
    }
    match &d {
        ReportData::Grouped(g) => print_grouped_data(g),
        ReportData::Comparative(c) => print_comparative_data(c),
    }
    Ok(())
}

fn print_grouped_data(g: &GroupedReportData) {
    println!("Report:       {} ({})", g.name, g.id);
    println!(
        "Period:       {} {} to {}{}",
        g.period.kind,
        g.period.start,
        g.period.end,
        if g.in_progress { " (in progress)" } else { "" }
    );
    println!(
        "Generated:    {} (data as of {})",
        short_time(&g.generated_at),
        short_time(&g.as_of)
    );
    println!("Sections:     {}", g.sections.join(", "));
    for d in &g.diagnostics {
        eprintln!("warning: {} {}", d.code, d.message);
    }
    let compared: Vec<&str> = g
        .perimeters
        .iter()
        .find_map(|p| p.summary.as_ref())
        .map(|cells| cells.iter().map(|c| c.period.start.as_str()).collect())
        .unwrap_or_default();
    if !g.perimeters.is_empty() {
        println!();
        let mut headers = vec!["KIND".to_string(), "PERIMETER".to_string()];
        headers.extend(compared.iter().map(|p| format!("FROM {p}")));
        headers.push("FAILURES".into());
        headers.push("RUNS".into());
        let mut t = Table::new(headers);
        for p in &g.perimeters {
            let mut row = vec![p.kind.clone(), p.name.clone()];
            let cells = p.summary.as_deref().unwrap_or(&[]);
            for i in 0..compared.len() {
                row.push(
                    cells
                        .get(i)
                        .map_or("-".to_string(), |c| fmt::pct(c.availability_percent)),
                );
            }
            let current = cells.first();
            row.push(current.map_or("-".to_string(), |c| c.failures.to_string()));
            row.push(current.map_or("-".to_string(), |c| c.runs.to_string()));
            t.row(row);
        }
        t.print();
        for p in &g.perimeters {
            for d in &p.diagnostics {
                eprintln!("warning: {}: {} {}", p.name, d.code, d.message);
            }
        }
    }
}

fn print_comparative_data(c: &ComparativeReportData) {
    println!("Report:       {} ({})", c.name, c.id);
    println!("Period:       {}", fmt::period(&c.period));
    println!("Statistic:    {}", c.stat);
    for s in &c.scenarios {
        println!();
        println!(
            "{} ({}){}",
            s.scenario.name,
            s.scenario.id,
            if s.error { "  [could not be read]" } else { "" }
        );
        println!(
            "  Runs:          {} ({} failed), availability {}",
            s.runs,
            s.failures,
            fmt::pct(s.availability)
        );
        println!(
            "  Paired steps:  {} failed, availability {}, journey mean {} ms",
            s.selected_failures,
            fmt::pct(s.selected_availability),
            fmt::ms(s.partial_journey_mean_ms)
        );
        if !s.steps.is_empty() {
            let mut t = Table::new([
                "#",
                "STEP",
                "CURRENT",
                "FAILURES",
                "AVAIL",
                "P50 MS",
                "P85 MS",
                "MEAN MS",
                "TARGET MS",
            ]);
            for st in &s.steps {
                t.row([
                    st.order.to_string(),
                    or_dash(st.label.as_deref().or(st.name.as_deref())),
                    if st.current { "yes" } else { "no" }.to_string(),
                    st.failures.to_string(),
                    fmt::pct(st.availability),
                    fmt::ms(st.stats.as_ref().map(|s| s.p50_ms)),
                    fmt::ms(st.stats.as_ref().map(|s| s.p85_ms)),
                    fmt::ms(st.stats.as_ref().map(|s| s.mean_ms)),
                    fmt::ms(st.thresholds.as_ref().map(|t| t.target)),
                ]);
            }
            t.print();
        }
    }
}

async fn create(ctx: &Context, body: &str, out: OutputArgs) -> Result<()> {
    let body = super::read_json_body(body)?;
    let resp = ctx
        .client()?
        .post_json("v2/reports", &body, &WriteOptions::idempotent())
        .await?;
    let r: Report = resp.json()?;
    if !out.wants_json() {
        eprintln!("Created report {} ({})", r.name(), r.id());
    }
    print_report(&r, resp.etag.as_deref(), &out)
}

async fn edit(
    ctx: &Context,
    id: &str,
    body: &str,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("report id", id)?;
    let body = super::read_json_body(body)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .patch_merge(&format!("v2/reports/{id}"), &body, &write)
        .await?;
    let r: Report = resp.json()?;
    print_report(&r, resp.etag.as_deref(), &out)
}

async fn delete(ctx: &Context, id: &str, purge: bool, if_match: Option<String>) -> Result<()> {
    ensure_guid("report id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let path = format!("v2/reports/{id}{}", if purge { "?purge=true" } else { "" });
    ctx.client()?.delete(&path, &write).await?;
    eprintln!("{} report {id}", if purge { "Purged" } else { "Deleted" });
    Ok(())
}

// ---------- schedules ----------

async fn schedules(ctx: &Context, cmd: SchedulesCommand) -> Result<()> {
    match cmd {
        SchedulesCommand::List {
            report_type,
            output,
        } => {
            let mut query = Vec::new();
            if let Some(t) = report_type {
                query.push((
                    "reportType",
                    canonical("--type", &t, &reports::REPORT_TYPES)?,
                ));
            }
            let page: Page<Schedule> = ctx
                .client()?
                .get_with("v2/report-schedules", &query)
                .await?
                .json()?;
            output.print(&page.items, "No report schedule visible", || {
                let mut t = Table::new([
                    "ID",
                    "TYPE",
                    "SUBJECT",
                    "FREQUENCY",
                    "ENABLED",
                    "TO",
                    "LAST SENT",
                ]);
                for s in &page.items {
                    t.row([
                        s.id.clone(),
                        s.report_type.clone(),
                        subject(s),
                        s.frequency.clone(),
                        if s.enabled { "yes" } else { "no" }.to_string(),
                        s.to.join(", "),
                        s.last_sent_at
                            .as_deref()
                            .map_or("-".to_string(), short_time),
                    ]);
                }
                t
            })
        }
        SchedulesCommand::Get { id, output } => {
            ensure_guid("schedule id", &id)?;
            let resp = ctx
                .client()?
                .get(&format!("v2/report-schedules/{id}"))
                .await?;
            let s: Schedule = resp.json()?;
            print_schedule(&s, resp.etag.as_deref(), &output)
        }
        SchedulesCommand::Create {
            report_type,
            scenarios,
            report,
            stat,
            frequency,
            locale,
            to,
            cc,
            planning,
            disabled,
            output,
        } => {
            let body = ScheduleWrite {
                report_type: canonical("--type", &report_type, &reports::REPORT_TYPES)?,
                scenarios: check_guids("--scenario", &scenarios)?,
                report: report
                    .map(|r| ensure_guid("--report", &r).map(|()| r))
                    .transpose()?,
                stat: stat
                    .map(|s| canonical("--stat", &s, &reports::RESPONSE_TIME_STATS))
                    .transpose()?,
                frequency: canonical("--frequency", &frequency, &reports::FREQUENCIES)?,
                locale: locale
                    .map(|l| canonical("--locale", &l, &reports::LANGS))
                    .transpose()?,
                enabled: !disabled,
                to: check_addresses("--to", &to)?,
                cc: check_addresses("--cc", &cc)?,
                planning_kinds: check_planning(&planning)?,
            };
            if body.to.is_empty() {
                bail!("at least one --to address is required");
            }
            if body.to.len() + body.cc.len() > 20 {
                bail!("at most 20 recipients in --to and --cc together");
            }
            let resp = ctx
                .client()?
                .post_json("v2/report-schedules", &body, &WriteOptions::idempotent())
                .await?;
            let s: Schedule = resp.json()?;
            if !output.wants_json() {
                eprintln!("Created schedule {}", s.id);
            }
            print_schedule(&s, resp.etag.as_deref(), &output)
        }
        SchedulesCommand::Edit {
            id,
            scenarios,
            report,
            stat,
            frequency,
            locale,
            enabled,
            to,
            cc,
            planning,
            if_match,
            output,
        } => {
            ensure_guid("schedule id", &id)?;
            let mut patch = serde_json::Map::new();
            if !scenarios.is_empty() {
                patch.insert(
                    "scenarios".into(),
                    check_guids("--scenario", &scenarios)?.into(),
                );
            }
            if let Some(r) = report {
                ensure_guid("--report", &r)?;
                patch.insert("report".into(), r.into());
            }
            if let Some(s) = stat {
                patch.insert(
                    "stat".into(),
                    canonical("--stat", &s, &reports::RESPONSE_TIME_STATS)?.into(),
                );
            }
            if let Some(f) = frequency {
                patch.insert(
                    "frequency".into(),
                    canonical("--frequency", &f, &reports::FREQUENCIES)?.into(),
                );
            }
            if let Some(l) = locale {
                patch.insert(
                    "locale".into(),
                    canonical("--locale", &l, &reports::LANGS)?.into(),
                );
            }
            if let Some(e) = enabled {
                patch.insert("enabled".into(), e.into());
            }
            if !to.is_empty() {
                let to = check_addresses("--to", &to)?;
                if to.is_empty() {
                    bail!("--to cannot be cleared: one recipient at least");
                }
                patch.insert("to".into(), to.into());
            }
            if !cc.is_empty() {
                patch.insert("cc".into(), check_addresses("--cc", &cc)?.into());
            }
            if !planning.is_empty() {
                patch.insert("planningKinds".into(), check_planning(&planning)?.into());
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
                .patch_merge(&format!("v2/report-schedules/{id}"), &patch, &write)
                .await?;
            let s: Schedule = resp.json()?;
            print_schedule(&s, resp.etag.as_deref(), &output)
        }
        SchedulesCommand::Delete { id, if_match } => {
            ensure_guid("schedule id", &id)?;
            let write = WriteOptions {
                idempotency_key: None,
                if_match,
            };
            ctx.client()?
                .delete(&format!("v2/report-schedules/{id}"), &write)
                .await?;
            eprintln!("Deleted schedule {id}");
            Ok(())
        }
        SchedulesCommand::Send { id, output } => {
            ensure_guid("schedule id", &id)?;
            let sent: ScheduleSent = ctx
                .client()?
                .post_json(
                    &format!("v2/report-schedules/{id}:send"),
                    &serde_json::json!({}),
                    &WriteOptions::idempotent(),
                )
                .await?
                .json()?;
            if output.wants_json() {
                return output.print_json(&sent);
            }
            if sent.sent {
                println!("Sent schedule {id} to its recipients");
                Ok(())
            } else {
                bail!(
                    "the report could not go out: {}",
                    sent.error.as_deref().unwrap_or("no reason given")
                )
            }
        }
    }
}

/// What a schedule mails: its report's name or its scenarios.
fn subject(s: &Schedule) -> String {
    if let Some(name) = &s.report_name {
        return name.clone();
    }
    match s.scenarios.len() {
        0 => "-".into(),
        1 => s.scenarios[0].name.clone(),
        n => format!("{} +{}", s.scenarios[0].name, n - 1),
    }
}

fn print_schedule(s: &Schedule, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(s)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!("Schedule:     {}", s.id);
    println!(
        "Type:         {}{}",
        s.report_type,
        s.stat
            .as_deref()
            .map_or(String::new(), |st| format!(" ({st})"))
    );
    if let Some(name) = &s.report_name {
        println!(
            "Report:       {} ({})",
            name,
            or_dash(s.report_id.as_deref())
        );
    }
    if !s.scenarios.is_empty() {
        println!(
            "Scenarios:    {}",
            s.scenarios
                .iter()
                .map(|r| format!("{} ({})", r.name, r.id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!(
        "Frequency:    {} ({})",
        s.frequency,
        if s.enabled { "enabled" } else { "paused" }
    );
    println!("Locale:       {}", s.locale);
    println!("To:           {}", s.to.join(", "));
    println!(
        "Cc:           {}",
        if s.cc.is_empty() {
            "-".into()
        } else {
            s.cc.join(", ")
        }
    );
    println!(
        "Planning:     {}",
        match &s.planning_kinds {
            Some(k) if !k.is_empty() => k.join(", "),
            _ => "-".into(),
        }
    );
    println!(
        "Last sent:    {}",
        s.last_sent_at
            .as_deref()
            .map_or("-".to_string(), short_time)
    );
    if let Some(e) = etag {
        println!("ETag:         {e}");
    }
    Ok(())
}

#[expect(clippy::too_many_arguments, reason = "one flag per filter of the list")]
async fn deliveries(
    ctx: &Context,
    period: &PeriodArgs,
    report_type: Option<&str>,
    status: Option<&str>,
    schedule: Option<&str>,
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let mut query = period.query()?;
    if let Some(t) = report_type {
        query.push((
            "reportType",
            canonical("--type", t, &reports::REPORT_TYPES)?,
        ));
    }
    if let Some(s) = status {
        query.push((
            "status",
            canonical("--status", s, &reports::DELIVERY_STATUSES)?,
        ));
    }
    if let Some(s) = schedule {
        ensure_guid("--schedule", s)?;
        query.push(("schedule", s.to_string()));
    }
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page =
        fetch_items::<Delivery>(&ctx.client()?, "v2/report-deliveries", &query, fetch).await?;
    out.print(&page.items, "No delivery in this period", || {
        let mut t = Table::new([
            "SENT", "STATUS", "TYPE", "LABEL", "PERIOD", "TO", "SCHEDULE", "ERROR",
        ]);
        for d in &page.items {
            t.row([
                d.sent_at
                    .as_deref()
                    .map_or_else(|| short_time(&d.created_at), short_time),
                d.status.clone(),
                d.report_type.clone(),
                d.label.clone(),
                format!("{} to {}", d.period_start, d.period_end),
                d.to.join(", "),
                d.schedule_id.clone(),
                or_dash(d.error.as_deref()),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} deliveries; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

// ---------- validation ----------

fn check_guids(flag: &str, values: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for v in values {
        ensure_guid(flag, v)?;
        if !out.contains(v) {
            out.push(v.clone());
        }
    }
    Ok(out)
}

/// Trimmed addresses with an `@`; a single empty value clears the list.
fn check_addresses(flag: &str, values: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for v in values {
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        let (local, domain) = v
            .split_once('@')
            .ok_or_else(|| anyhow::anyhow!("{flag} `{v}` is not an e-mail address"))?;
        if local.is_empty() || !domain.contains('.') || v.contains(char::is_whitespace) {
            bail!("{flag} `{v}` is not an e-mail address");
        }
        if !out.iter().any(|o: &String| o.eq_ignore_ascii_case(v)) {
            out.push(v.to_string());
        }
    }
    Ok(out)
}

/// Known planning kinds, deduplicated; a single empty value clears the list.
fn check_planning(values: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for v in values {
        if v.trim().is_empty() {
            continue;
        }
        let k = canonical("--planning", v, &reports::PLANNING_KINDS)?;
        if !out.contains(&k) {
            out.push(k);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_query_is_canonical() {
        let q = DataQuery {
            window: Some("WEEK".into()),
            date: Some("2026-10-05".into()),
            ..Default::default()
        };
        assert_eq!(
            q.query().unwrap(),
            vec![
                ("window", "week".to_string()),
                ("date", "2026-10-05".to_string())
            ]
        );
        let bad = DataQuery {
            date: Some("yesterday".into()),
            ..Default::default()
        };
        assert!(bad.query().unwrap_err().to_string().contains("YYYY-MM-DD"));
        let stat = DataQuery {
            stat: Some("P50".into()),
            ..Default::default()
        };
        assert_eq!(stat.query().unwrap(), vec![("stat", "p50".to_string())]);
    }

    #[test]
    fn pdf_flags_join_planning_kinds() {
        let p = PdfArgs {
            lang: Some("EN".into()),
            planning: vec![
                "scriptVersion".into(),
                "monitoringevent".into(),
                "scriptVersion".into(),
            ],
        };
        assert_eq!(
            p.query().unwrap(),
            vec![
                ("lang", "en".to_string()),
                ("planningKinds", "scriptVersion,monitoringEvent".to_string())
            ]
        );
        let bad = PdfArgs {
            lang: None,
            planning: vec!["holidays".into()],
        };
        assert!(bad.query().unwrap_err().to_string().contains("--planning"));
    }

    #[test]
    fn addresses_are_checked_and_deduplicated() {
        assert_eq!(
            check_addresses(
                "--to",
                &["A@x.io".into(), " a@x.io ".into(), "b@y.org".into()]
            )
            .unwrap(),
            vec!["A@x.io".to_string(), "b@y.org".to_string()]
        );
        assert_eq!(
            check_addresses("--cc", &["".into()]).unwrap(),
            Vec::<String>::new()
        );
        assert!(check_addresses("--to", &["nope".into()]).is_err());
        assert!(check_addresses("--to", &["a@localhost".into()]).is_err());
    }

    #[test]
    fn subject_names_the_report_or_the_scenarios() {
        let mut s = Schedule {
            id: "x".into(),
            report_type: "availability-dashboard".into(),
            scenarios: vec![
                ScenarioRef {
                    id: "1".into(),
                    name: "Login".into(),
                    kind: "transaction".into(),
                },
                ScenarioRef {
                    id: "2".into(),
                    name: "Pay".into(),
                    kind: "transaction".into(),
                },
            ],
            stat: None,
            report_id: None,
            report_name: None,
            frequency: "weekly".into(),
            locale: "en".into(),
            enabled: true,
            to: vec![],
            cc: vec![],
            last_sent_at: None,
            planning_kinds: None,
        };
        assert_eq!(subject(&s), "Login +1");
        s.report_name = Some("Monthly".into());
        assert_eq!(subject(&s), "Monthly");
    }

    use crate::api::reports::ScenarioRef;
}
