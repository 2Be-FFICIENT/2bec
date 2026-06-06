//! `2bec scenarios`: the catalog of scenarios and their groups.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::alerts::canonical;
use super::metrics::{self as fmt, PeriodArgs};
use super::runs::ensure_guid;
use crate::api::metrics::{self, ScenarioResponseTimes, ScenarioSynthesis, ScenarioVoiceQuality};
use crate::api::page::{Fetch, fetch_items};
use crate::api::scenarios::{
    self, Scenario, ScenarioCalendar, ScenarioGroup, ScenarioListItem, ScenarioVersions,
};
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
pub struct ScenariosArgs {
    #[command(subcommand)]
    command: ScenariosCommand,
}

#[derive(Subcommand, Debug)]
enum ScenariosCommand {
    /// List the scenarios the credential sees, by name
    #[command(after_help = "\
Examples:
  2bec scenarios list
  2bec scenarios list --kind transaction --active --json
  2bec scenarios list --group 1a2b... --search login
  2bec scenarios list --voice --all")]
    List {
        /// Only this kind: transaction or socialMonitoring
        #[arg(long, value_name = "KIND")]
        kind: Option<String>,
        /// Only voice call (IVR) scenarios
        #[arg(long)]
        voice: bool,
        /// Only scenarios of this group id
        #[arg(long, value_name = "ID")]
        group: Option<String>,
        /// Only scenarios whose name or description matches these words
        #[arg(long, short = 'q', value_name = "WORDS")]
        search: Option<String>,
        /// Only scenarios the scheduler runs (drops inactive transaction scenarios)
        #[arg(long)]
        active: bool,
        /// Maximum number of scenarios to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every matching scenario, following pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one scenario: steps, thresholds, monitoring state, availability target
    Get {
        /// Scenario id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// List the saved versions of a scenario's script, newest first
    #[command(after_help = "\
Transaction scenarios only. The summary of a version is written in the
background; a recent save may show a dash for a while.")]
    Versions {
        /// Scenario id
        id: String,
        /// How many months back to look (1 to 13) [default: 13]
        #[arg(long, value_name = "N")]
        months: Option<u8>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show what decides when a scenario runs: cadence, hours, pauses, interruption windows
    #[command(after_help = "\
Examples:
  2bec scenarios calendar 7f3c...
  2bec scenarios calendar 7f3c... --from 2026-11-01T00:00:00Z --to 2026-12-01T00:00:00Z

The range defaults to the next 14 days and spans at most 92 days.
Transaction scenarios only, and only active ones.")]
    Calendar {
        /// Scenario id
        id: String,
        /// Start of the range, RFC 3339 [default: now]
        #[arg(long, value_name = "TIME")]
        from: Option<String>,
        /// End of the range, RFC 3339 [default: from + 14 days]
        #[arg(long, value_name = "TIME")]
        to: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Download the documentation of a scenario as a PDF
    #[command(
        alias = "doc",
        after_help = "\
Examples:
  2bec scenarios download-doc 7f3c...                 # writes livedoc-7f3c....pdf
  2bec scenarios download-doc 7f3c... --technical -o doc.pdf
  2bec scenarios download-doc 7f3c... -o - | lp

Transaction scenarios only; 404 no-documentation until a successful run of
the current script version has been observed."
    )]
    DownloadDoc {
        /// Scenario id
        id: String,
        /// Add the technical details of each step
        #[arg(long)]
        technical: bool,
        /// Destination file; `-` for stdout [default: livedoc-<id>.pdf]
        #[arg(long, short = 'o', value_name = "FILE")]
        output: Option<String>,
    },
    /// The quality indices of a transaction scenario over a period (last 7 days by default)
    #[command(after_help = "\
Examples:
  2bec scenarios synthesis 7f3c...
  2bec scenarios synthesis 7f3c... --period lastMonth --trend week
  2bec scenarios synthesis 7f3c... --period today --json     # adds the 24 hours

The synthesis and its five axes (duration, interactivity, completion,
regularity, restoration) are out of 10, read against the client thresholds
and the target ones. --trend picks the comparison windows: day, week or
month. At most 366 days; 409 social-scenario for a social monitoring one,
404 no-synthesis when nothing was observed.")]
    Synthesis {
        /// Scenario id
        id: String,
        #[command(flatten)]
        period: PeriodArgs,
        /// Comparison windows: day, week or month [default: day]
        #[arg(long, value_name = "UNIT")]
        trend: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Response times of a transaction scenario down to its steps (last 7 days by default)
    #[command(after_help = "\
Examples:
  2bec scenarios response-times 7f3c...
  2bec scenarios response-times 7f3c... --period last30d --json   # daily and hourly series

Milliseconds over the successful runs. At most 92 days; 409 social-scenario
for a social monitoring one. The fleet view is `2bec metrics response-times`.")]
    ResponseTimes {
        /// Scenario id
        id: String,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Line quality of every call of a voice (IVR) scenario (last 7 days by default)
    #[command(after_help = "\
Examples:
  2bec scenarios voice-quality 7f3c...
  2bec scenarios voice-quality 7f3c... --period yesterday --json

At most 31 days; 409 not-a-voice-call for any other scenario. The fleet
view is `2bec metrics voice-quality`.")]
    VoiceQuality {
        /// Scenario id
        id: String,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Browse the groups scenarios are organized in
    Groups(GroupsArgs),
    #[command(flatten)]
    Settings(super::settings::SettingsCommand),
}

#[derive(Args, Debug)]
pub struct GroupsArgs {
    #[command(subcommand)]
    command: GroupsCommand,
}

#[derive(Subcommand, Debug)]
enum GroupsCommand {
    /// List the groups, by name, with how many scenarios each holds
    List {
        /// Maximum number of groups to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every group
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one group and the scenarios of it the credential sees
    Get {
        /// Group id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
}

/// Query parameters of `scenarios list`, validated locally.
fn list_query(
    kind: Option<&str>,
    voice: bool,
    group: Option<&str>,
    q: Option<&str>,
    active: bool,
) -> Result<Vec<(&'static str, String)>> {
    let mut query = Vec::new();
    if let Some(k) = kind {
        let canonical = scenarios::KINDS
            .iter()
            .find(|v| v.eq_ignore_ascii_case(k))
            .ok_or_else(|| {
                anyhow::anyhow!("--kind `{k}` is not one of {}", scenarios::KINDS.join(", "))
            })?;
        query.push(("kind", (*canonical).to_string()));
    }
    if voice {
        query.push(("voice", "true".into()));
    }
    if let Some(g) = group {
        ensure_guid("--group", g)?;
        query.push(("group", g.to_string()));
    }
    if let Some(q) = q
        && !q.trim().is_empty()
    {
        query.push(("q", q.trim().to_string()));
    }
    if active {
        query.push(("includeInactive", "false".into()));
    }
    Ok(query)
}

pub async fn run(ctx: &Context, args: ScenariosArgs) -> Result<()> {
    match args.command {
        ScenariosCommand::List {
            kind,
            voice,
            group,
            search,
            active,
            limit,
            all,
            output,
        } => {
            let query = list_query(
                kind.as_deref(),
                voice,
                group.as_deref(),
                search.as_deref(),
                active,
            )?;
            list(ctx, &query, limit, all, output).await
        }
        ScenariosCommand::Get { id, output } => get(ctx, &id, output).await,
        ScenariosCommand::Versions { id, months, output } => {
            versions(ctx, &id, months, output).await
        }
        ScenariosCommand::Calendar {
            id,
            from,
            to,
            output,
        } => calendar(ctx, &id, from, to, output).await,
        ScenariosCommand::DownloadDoc {
            id,
            technical,
            output,
        } => download_doc(ctx, &id, technical, output).await,
        ScenariosCommand::Synthesis {
            id,
            period,
            trend,
            output,
        } => synthesis(ctx, &id, &period, trend.as_deref(), output).await,
        ScenariosCommand::ResponseTimes { id, period, output } => {
            response_times(ctx, &id, &period, output).await
        }
        ScenariosCommand::VoiceQuality { id, period, output } => {
            voice_quality(ctx, &id, &period, output).await
        }
        ScenariosCommand::Groups(args) => groups(ctx, args.command).await,
        ScenariosCommand::Settings(cmd) => super::settings::run(ctx, cmd).await,
    }
}

async fn synthesis(
    ctx: &Context,
    id: &str,
    period: &PeriodArgs,
    trend: Option<&str>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let mut query = period.query()?;
    if let Some(t) = trend {
        query.push(("trend", canonical("--trend", t, &metrics::TRENDS)?));
    }
    let s: ScenarioSynthesis = ctx
        .client()?
        .get_with(&format!("v2/scenarios/{id}/synthesis"), &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&s);
    }
    println!("Scenario:      {} ({})", s.scenario.name, s.scenario.id);
    println!(
        "Period:        {} to {} (trend by {})",
        s.from, s.to, s.trend
    );
    let i = &s.indices;
    println!("Synthesis:     {}", index(&i.synthesis));
    println!("  Duration:      {}", index(&i.duration));
    println!("  Interactivity: {}", index(&i.interactivity));
    println!("  Completion:    {}", index(&i.completion));
    println!("  Regularity:    {}", index(&i.regularity));
    println!("  Restoration:   {}", index(&i.restoration));
    let sm = &s.summary;
    println!(
        "Runs:          {} ({} failed), availability {}",
        sm.runs,
        sm.failed_runs,
        fmt::pct(Some(sm.availability_rate))
    );
    println!(
        "Incidents:     {}, longest {}",
        sm.incidents,
        fmt::duration(sm.longest_incident_seconds)
    );
    if let Some(b) = &sm.bands {
        println!(
            "Bands:         {} on target, {} within client, {} insufficient, {} unacceptable",
            b.target, b.client, b.insufficient, b.unacceptable
        );
    }
    if let Some(p) = &s.percentiles {
        println!(
            "Response:      P50 {} ms, P85 {} ms over {} runs",
            fmt::ms(p.p50_ms),
            fmt::ms(p.p85_ms),
            p.runs
        );
    }
    if let Some(r) = &s.reliability {
        println!(
            "Restore:       MTTR {} over {} incident(s)",
            fmt::opt_duration(r.mttr_seconds),
            r.incidents
        );
    }
    if let Some(trends) = &s.trends
        && !trends.is_empty()
    {
        println!();
        println!("Comparison windows:");
        let mut t = Table::new(["WINDOW", "FROM", "TO", "INDEX", "AVAILABILITY"]);
        for w in trends {
            t.row([
                w.window.clone(),
                w.from.clone(),
                w.to.clone(),
                fmt::ratio(w.client_index),
                fmt::pct(w.availability_rate),
            ]);
        }
        t.print();
    }
    if let Some(days) = &s.daily
        && !days.is_empty()
    {
        println!();
        let mut t = Table::new(["DATE", "INDEX", "VALID", "RUNS", "FAILURES"]);
        for d in days {
            t.row([
                d.date.clone(),
                fmt::ratio(d.client_index),
                if d.valid { "yes" } else { "no" }.to_string(),
                d.runs.to_string(),
                d.failures.to_string(),
            ]);
        }
        t.print();
    }
    if let Some(hours) = &s.hourly
        && !hours.is_empty()
    {
        println!();
        let mut t = Table::new(["HOUR", "INDEX", "VALID", "RUNS", "FAILURES"]);
        for h in hours {
            t.row([
                format!("{:02}h", h.hour),
                fmt::ratio(h.client_index),
                if h.valid { "yes" } else { "no" }.to_string(),
                h.runs.to_string(),
                h.failures.to_string(),
            ]);
        }
        t.print();
    }
    Ok(())
}

fn index(i: &metrics::SynthesisIndex) -> String {
    format!(
        "{:.1} / 10 (target reading {:.1}){}",
        i.client,
        i.target,
        if i.valid { "" } else { " [not enough runs]" }
    )
}

async fn response_times(
    ctx: &Context,
    id: &str,
    period: &PeriodArgs,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let query = period.query()?;
    let r: ScenarioResponseTimes = ctx
        .client()?
        .get_with(&format!("v2/scenarios/{id}/response-times"), &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&r);
    }
    println!("Scenario:      {} ({})", r.scenario.name, r.scenario.id);
    println!(
        "Period:        {}{}; previous {}",
        fmt::period(&r.period),
        if r.last_day_partial {
            " (today still moving)"
        } else {
            ""
        },
        fmt::period(&r.previous_period)
    );
    println!(
        "Thresholds:    {}{}",
        match &r.thresholds {
            Some(t) => thresholds(&scenarios::Thresholds {
                target: t.target,
                client: t.client,
                unacceptable: t.unacceptable,
            }),
            None => "-".into(),
        },
        r.kilo_bytes_per_second
            .map_or(String::new(), |k| format!(" at {k} kB/s"))
    );
    println!("Runs:          {} successful", r.successful_runs);
    println!("Total:         {}", stats(r.total.as_ref()));
    println!("Previous:      {}", stats(r.previous.as_ref()));
    let b = &r.bands;
    println!(
        "Bands:         {} on target, {} within client, {} insufficient, {} unacceptable, {} unrated",
        b.target, b.client, b.insufficient, b.unacceptable, b.unrated
    );
    if !r.steps.is_empty() {
        println!();
        let mut t = Table::new([
            "#",
            "STEP",
            "CURRENT",
            "P50 MS",
            "P85 MS",
            "MEAN MS",
            "PREV MEAN",
            "SHARE",
            "SERVER",
            "TARGET MS",
        ]);
        for st in &r.steps {
            t.row([
                st.order.to_string(),
                or_dash(st.label.as_deref().or(st.name.as_deref())),
                if st.current { "yes" } else { "no" }.to_string(),
                fmt::ms(st.total.as_ref().map(|s| s.p50_ms)),
                fmt::ms(st.total.as_ref().map(|s| s.p85_ms)),
                fmt::ms(st.total.as_ref().map(|s| s.mean_ms)),
                fmt::ms(st.previous.as_ref().map(|s| s.mean_ms)),
                fmt::share(st.share_of_total),
                st.server_time_share.map_or("-".into(), fmt::share),
                fmt::ms(st.thresholds.as_ref().map(|t| t.target)),
            ]);
        }
        t.print();
    }
    if !r.slowest_runs.is_empty() {
        println!();
        println!("Slowest runs:");
        let mut t = Table::new([
            "RUN",
            "STARTED",
            "TOTAL MS",
            "SLOWEST STEP",
            "STEP MS",
            "BAND",
        ]);
        for run in &r.slowest_runs {
            t.row([
                run.id.clone(),
                short_time(&run.started_at),
                fmt::ms(Some(run.total_ms)),
                or_dash(run.slowest_step.as_deref()),
                fmt::ms(run.slowest_step_ms),
                or_dash(run.band.as_deref()),
            ]);
        }
        t.print();
    }
    Ok(())
}

fn stats(s: Option<&metrics::ResponseTimeStats>) -> String {
    match s {
        Some(s) => format!(
            "P50 {} ms, P85 {} ms, mean {} ms, max {} ms over {} runs",
            fmt::ms(Some(s.p50_ms)),
            fmt::ms(Some(s.p85_ms)),
            fmt::ms(Some(s.mean_ms)),
            fmt::ms(Some(s.max_ms)),
            s.count
        ),
        None => "-".into(),
    }
}

async fn voice_quality(
    ctx: &Context,
    id: &str,
    period: &PeriodArgs,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let query = period.query()?;
    let v: ScenarioVoiceQuality = ctx
        .client()?
        .get_with(&format!("v2/scenarios/{id}/voice-quality"), &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&v);
    }
    println!("Scenario:      {} ({})", v.scenario.name, v.scenario.id);
    println!("Period:        {}", fmt::period(&v.period));
    fmt::print_figures(&v.summary);
    if !v.end_reasons.is_empty() {
        println!(
            "End reasons:   {}",
            v.end_reasons
                .iter()
                .map(|r| format!("{} ({})", r.code, r.count))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !v.daily.is_empty() {
        println!();
        let mut t = Table::new([
            "DATE",
            "CALLS",
            "MEASURED",
            "AVG MOS",
            "MIN MOS",
            "ERR %",
            "JITTER MS",
            "POOR+BAD",
        ]);
        for d in &v.daily {
            t.row([
                d.date.clone(),
                d.calls.to_string(),
                d.measured.to_string(),
                fmt::f1(d.average_mos),
                fmt::f1(d.min_mos),
                fmt::f1(d.average_error_rate_percent),
                fmt::f1(d.average_jitter_ms),
                (d.ratings.poor + d.ratings.bad).to_string(),
            ]);
        }
        t.print();
    }
    if !v.calls.is_empty() {
        println!();
        println!("Calls:");
        let mut t = Table::new([
            "RUN",
            "STARTED",
            "STATUS",
            "MOS",
            "RATING",
            "ERR %",
            "JITTER MS",
            "CODEC",
            "END",
        ]);
        for c in &v.calls {
            t.row([
                c.id.clone(),
                short_time(&c.started_at),
                c.status.clone(),
                if c.measurement == "measured" {
                    fmt::f1(c.mos)
                } else {
                    c.measurement.clone()
                },
                or_dash(c.rating.as_deref()),
                fmt::f1(c.error_rate_percent),
                fmt::f1(c.jitter_avg_ms),
                or_dash(c.codec.as_deref()),
                or_dash(c.end_reason.as_deref()),
            ]);
        }
        t.print();
    }
    Ok(())
}

async fn groups(ctx: &Context, cmd: GroupsCommand) -> Result<()> {
    match cmd {
        GroupsCommand::List { limit, all, output } => {
            if !all && limit == 0 {
                bail!("--limit must be at least 1");
            }
            let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
            let page =
                fetch_items::<ScenarioGroup>(&ctx.client()?, "v2/scenario-groups", &[], fetch)
                    .await?;
            output.print(&page.items, "No scenario group visible", || {
                let mut t = Table::new(["ID", "NAME", "SCENARIOS", "HIDDEN"]);
                for g in &page.items {
                    t.row([
                        g.id.clone(),
                        g.name.clone(),
                        g.scenarios.len().to_string(),
                        g.hidden_count.to_string(),
                    ]);
                }
                t
            })
        }
        GroupsCommand::Get { id, output } => {
            ensure_guid("group id", &id)?;
            let g: ScenarioGroup = ctx
                .client()?
                .get(&format!("v2/scenario-groups/{id}"))
                .await?
                .json()?;
            if output.wants_json() {
                return output.print_json(&g);
            }
            println!("Group:     {}", g.id);
            println!("Name:      {}", g.name);
            println!(
                "Scenarios: {} visible, {} hidden by the access profile",
                g.scenarios.len(),
                g.hidden_count
            );
            if !g.scenarios.is_empty() {
                println!();
                let mut t = Table::new(["ID", "NAME", "KIND"]);
                for s in &g.scenarios {
                    t.row([s.id.clone(), s.name.clone(), kind_label(&s.kind, false)]);
                }
                t.print();
            }
            Ok(())
        }
    }
}

async fn versions(ctx: &Context, id: &str, months: Option<u8>, out: OutputArgs) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let mut query = Vec::new();
    if let Some(m) = months {
        if !(1..=13).contains(&m) {
            bail!("--months must be between 1 and 13");
        }
        query.push(("months", m.to_string()));
    }
    let v: ScenarioVersions = ctx
        .client()?
        .get_with(&format!("v2/scenarios/{id}/versions"), &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&v);
    }
    out.print(&v, "No version saved in this window", || {
        let mut t = Table::new(["VERSION", "SAVED", "CURRENT", "STABLE", "SUMMARY"]);
        for ver in &v.items {
            t.row([
                ver.version.to_string(),
                short_time(&ver.saved_at),
                if ver.current { "yes" } else { "" }.to_string(),
                if ver.stable { "yes" } else { "" }.to_string(),
                or_dash(ver.summary.as_deref()),
            ]);
        }
        t
    })?;
    if v.older_versions > 0 {
        eprintln!(
            "{} older version(s) before {} not shown",
            v.older_versions,
            short_time(&v.since)
        );
    }
    Ok(())
}

async fn calendar(
    ctx: &Context,
    id: &str,
    from: Option<String>,
    to: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let mut query = Vec::new();
    if let Some(f) = from {
        query.push(("from", f));
    }
    if let Some(t) = to {
        query.push(("to", t));
    }
    let c: ScenarioCalendar = ctx
        .client()?
        .get_with(&format!("v2/scenarios/{id}/calendar"), &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&c);
    }
    println!("Scenario:  {}", c.scenario_id);
    println!("Range:     {} to {}", c.from, c.to);
    let mut cadence = format!("every {}", interval(Some(c.cadence.declared_seconds)));
    if let Some(s) = c.cadence.scheduled_seconds
        && s != c.cadence.declared_seconds
    {
        cadence.push_str(&format!(" (scheduled every {})", interval(Some(s))));
    }
    if c.cadence.on_demand {
        cadence.push_str(", on demand");
    }
    if c.cadence.replay_acceleration {
        cadence.push_str(", replay acceleration");
    }
    println!("Cadence:   {cadence}");
    println!(
        "Hours:     {}",
        match &c.observation_hours {
            Some(h) => format!(
                "{} to {}{}",
                h.start,
                h.end,
                if h.overnight { " (overnight)" } else { "" }
            ),
            None => "around the clock".into(),
        }
    );
    if let Some(p) = &c.paused_since {
        println!("Paused:    since {p}");
    }
    if !c.mates.is_empty() {
        println!(
            "Shares calendar with: {}",
            c.mates
                .iter()
                .map(|m| format!("{} ({})", m.name, m.id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if c.unavailable {
        eprintln!("warning: the calendar could not be read; the windows are unknown");
    }
    if c.windows.is_empty() {
        if crate::output::stdout_is_tty() {
            eprintln!("No interruption window in this range");
        }
        return Ok(());
    }
    println!();
    let mut t = Table::new(["START", "END", "EFFECT", "KIND", "INTERRUPTION", "COMMENT"]);
    for w in &c.windows {
        t.row([
            short_time(&w.start),
            short_time(&w.end),
            if w.exclusion {
                "no observation"
            } else {
                "observation only"
            }
            .to_string(),
            w.kind.clone(),
            w.interruption_id.clone(),
            or_dash(w.comment.as_deref()),
        ]);
    }
    t.print();
    Ok(())
}

async fn download_doc(
    ctx: &Context,
    id: &str,
    technical: bool,
    output: Option<String>,
) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let path = format!(
        "v2/scenarios/{id}/documentation.pdf{}",
        if technical { "?technical=true" } else { "" }
    );
    let target = output.unwrap_or_else(|| format!("livedoc-{id}.pdf"));
    super::download_to(&ctx.client()?, &path, &target).await
}

async fn list(
    ctx: &Context,
    query: &[(&str, String)],
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page =
        fetch_items::<ScenarioListItem>(&ctx.client()?, "v2/scenarios", query, fetch).await?;
    out.print(&page.items, "No scenarios match these filters", || {
        let mut t = Table::new([
            "ID", "NAME", "KIND", "ACTIVE", "INTERVAL", "GROUP", "PLATFORM",
        ]);
        for s in &page.items {
            t.row([
                s.id.clone(),
                s.name.clone(),
                kind_label(&s.kind, s.voice_call),
                active_label(s.active),
                interval(s.interval_seconds),
                or_dash(s.group.as_ref().map(|g| g.name.as_str())),
                platform(s.os.as_deref(), s.browser.as_deref()),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} scenarios; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let s: Scenario = ctx
        .client()?
        .get(&format!("v2/scenarios/{id}"))
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&s);
    }
    println!("Scenario:     {}", s.id);
    println!("Name:         {}", s.name);
    println!("Kind:         {}", kind_label(&s.kind, s.voice_call));
    println!("Description:  {}", or_dash(s.description.as_deref()));
    println!(
        "Platform:     {}",
        platform(s.os.as_deref(), s.browser.as_deref())
    );
    println!("Group:        {}", group_label(s.group.as_ref()));
    println!(
        "Active:       {}{}",
        active_label(s.active),
        if s.replay_acceleration {
            " (replay acceleration)"
        } else {
            ""
        }
    );
    println!("Interval:     {}", interval(s.interval_seconds));
    if let Some(src) = &s.social_source {
        println!("Source:       {src}");
    }
    println!("Created:      {}", s.created_at);
    println!("Updated:      {}", or_dash(s.updated_at.as_deref()));
    if let Some(m) = &s.monitoring {
        let mut line = m.state.clone();
        if let Some(since) = &m.since {
            line.push_str(&format!(" since {since}"));
        }
        if let Some(until) = &m.until {
            line.push_str(&format!(" until {until}"));
        }
        if let Some(c) = &m.comment {
            line.push_str(&format!(" ({c})"));
        }
        println!("Monitoring:   {line}");
    }
    if let Some(t) = &s.availability_target {
        println!(
            "Availability: {}% target{}",
            t.effective,
            match t.own {
                Some(_) => " (own)",
                None => " (space default)",
            }
        );
    }
    if let Some(t) = &s.run_thresholds {
        println!("Thresholds:   {}", thresholds(t));
    }
    if let Some(steps) = &s.steps
        && !steps.is_empty()
    {
        println!();
        let mut t = Table::new(["#", "STEP", "MEASURED", "TARGET", "CLIENT", "UNACCEPTABLE"]);
        for st in steps {
            let th = st.thresholds.as_ref();
            t.row([
                st.order.to_string(),
                st.label.clone(),
                if st.measured { "yes" } else { "no" }.to_string(),
                th.map_or("-".into(), |t| ms(t.target)),
                th.map_or("-".into(), |t| ms(t.client)),
                th.map_or("-".into(), |t| ms(t.unacceptable)),
            ]);
        }
        t.print();
    }
    Ok(())
}

fn kind_label(kind: &str, voice: bool) -> String {
    match (kind, voice) {
        ("transaction", true) => "voice".into(),
        ("socialMonitoring", _) => "social".into(),
        (k, _) => k.to_string(),
    }
}

fn active_label(active: Option<bool>) -> String {
    match active {
        Some(true) => "yes".into(),
        Some(false) => "no".into(),
        None => "-".into(),
    }
}

fn group_label(group: Option<&scenarios::GroupRef>) -> String {
    group.map_or("-".into(), |g| format!("{} ({})", g.name, g.id))
}

/// Seconds as `5m`, `1h30m` or `45s`; a dash when on request.
fn interval(seconds: Option<i64>) -> String {
    let Some(s) = seconds else {
        return "-".into();
    };
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    let mut out = String::new();
    if h > 0 {
        out.push_str(&format!("{h}h"));
    }
    if m > 0 {
        out.push_str(&format!("{m}m"));
    }
    if sec > 0 || out.is_empty() {
        out.push_str(&format!("{sec}s"));
    }
    out
}

fn platform(os: Option<&str>, browser: Option<&str>) -> String {
    match (os, browser) {
        (Some(o), Some(b)) => format!("{o} / {b}"),
        (Some(o), None) => o.to_string(),
        (None, Some(b)) => b.to_string(),
        (None, None) => "-".into(),
    }
}

pub(super) fn thresholds(t: &scenarios::Thresholds) -> String {
    format!(
        "target {}, client {}, unacceptable {}",
        ms(t.target),
        ms(t.client),
        ms(t.unacceptable)
    )
}

/// Milliseconds as `1.5s` or `800ms`.
pub(super) fn ms(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}s", v / 1000.0)
    } else {
        format!("{}ms", v.round() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "7f3c1a2b-0000-4000-8000-000000000000";

    #[test]
    fn list_query_is_canonical() {
        let q = list_query(
            Some("SOCIALMONITORING"),
            true,
            Some(G),
            Some(" login "),
            true,
        )
        .unwrap();
        assert_eq!(
            q,
            vec![
                ("kind", "socialMonitoring".into()),
                ("voice", "true".into()),
                ("group", G.into()),
                ("q", "login".into()),
                ("includeInactive", "false".into()),
            ]
        );
        assert!(
            list_query(None, false, None, None, false)
                .unwrap()
                .is_empty()
        );
        assert!(list_query(Some("voice"), false, None, None, false).is_err());
        assert!(list_query(None, false, Some("nope"), None, false).is_err());
    }

    #[test]
    fn formats() {
        assert_eq!(interval(Some(300)), "5m");
        assert_eq!(interval(Some(5400)), "1h30m");
        assert_eq!(interval(Some(45)), "45s");
        assert_eq!(interval(Some(0)), "0s");
        assert_eq!(interval(None), "-");
        assert_eq!(ms(1500.0), "1.5s");
        assert_eq!(ms(800.0), "800ms");
        assert_eq!(kind_label("transaction", true), "voice");
        assert_eq!(kind_label("socialMonitoring", false), "social");
        assert_eq!(platform(Some("Windows"), None), "Windows");
    }
}
