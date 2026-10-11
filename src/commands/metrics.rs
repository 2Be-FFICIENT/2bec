//! `2bec metrics`: the dashboard figures over a selection of scenarios and a
//! period of Paris days.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::alerts::canonical;
use super::runs::ensure_guid;
use crate::api::metrics::{
    self, AvailabilityMetrics, ResponseTimesMetrics, SocialAnalysisMetrics, SocialMetrics,
    VoiceQualityFigures, VoiceQualityMetrics,
};
use crate::api::runs;
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
#[command(after_help = PERIOD_HELP)]
pub struct MetricsArgs {
    #[command(subcommand)]
    command: MetricsCommand,
}

const PERIOD_HELP: &str = "\
The period is a range of Paris days: --from/--to (RFC 3339) or --period
(last24h, last7d, last30d, today, yesterday, thisWeek, lastWeek, thisMonth,
lastMonth); --tz moves the day boundaries. Without --scenario the read covers
every scenario the credential sees; past the cap the API asks to name them.";

#[derive(Subcommand, Debug)]
enum MetricsCommand {
    /// Availability of transaction scenarios against their target (last 30 days by default)
    #[command(after_help = "\
Examples:
  2bec metrics availability
  2bec metrics availability --period lastMonth --granularity week
  2bec metrics availability --scenario 7f3c... --scenario 9a1d... --json

At most 100 scenarios and 366 days. The table shows the summary, each
scenario worst first, the weakest steps, the failures by cause and the
longest incidents; the series by bucket is in --json only.")]
    Availability {
        #[command(flatten)]
        selection: Selection,
        /// Size of each bucket of the series: day, week or month [default: day]
        #[arg(long, value_name = "UNIT")]
        granularity: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Response times of the successful runs of transaction scenarios (last 7 days by default)
    #[command(after_help = "\
Examples:
  2bec metrics response-times
  2bec metrics response-times --scenario 7f3c... --period last30d

At most 15 scenarios and 92 days; one scenario's steps are at
`2bec scenarios response-times <id>`. The daily series is in --json only.")]
    ResponseTimes {
        #[command(flatten)]
        selection: Selection,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Line quality of the calls of voice (IVR) scenarios (last 7 days by default)
    #[command(after_help = "\
Examples:
  2bec metrics voice-quality
  2bec metrics voice-quality --scenario 7f3c... --period yesterday

At most 20 scenarios and 31 days; one scenario's calls are at
`2bec scenarios voice-quality <id>`.")]
    VoiceQuality {
        #[command(flatten)]
        selection: Selection,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// What people posted about the services social monitoring scenarios watch (last 7 days by default)
    #[command(after_help = "\
Examples:
  2bec metrics social
  2bec metrics social --period today --interval 15
  2bec metrics social --events --period yesterday --json

At most 30 scenarios and 92 days. --interval sets the bucket length in
minutes (10, 15, 30, 60, 120 or 240; chosen from the period by default).
The per-bucket series is in --json only; --events adds the posts, newest first.")]
    Social {
        #[command(flatten)]
        selection: Selection,
        /// Bucket length in minutes: 10, 15, 30, 60, 120 or 240
        #[arg(long, value_name = "MINUTES")]
        interval: Option<u32>,
        /// Include the posts themselves (capped by the API)
        #[arg(long)]
        events: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Daily AI reading of the social posts: issues reported and keywords (last 7 days by default)
    #[command(
        alias = "analysis",
        after_help = "\
Examples:
  2bec metrics social-analysis
  2bec metrics social-analysis --scenario 7f3c... --period last30d

At most 30 scenarios and 31 days. A first reading of a day may take a while."
    )]
    SocialAnalysis {
        #[command(flatten)]
        selection: Selection,
        #[command(flatten)]
        output: OutputArgs,
    },
}

/// The period flags every metrics read takes.
#[derive(Args, Debug, Default)]
pub struct PeriodArgs {
    /// Start of the range, RFC 3339 (inclusive)
    #[arg(long, value_name = "TIME", conflicts_with = "period")]
    from: Option<String>,
    /// End of the range, RFC 3339 (exclusive)
    #[arg(long, value_name = "TIME", conflicts_with = "period")]
    to: Option<String>,
    /// Named period instead of --from/--to (e.g. today, last7d)
    #[arg(long, value_name = "PERIOD")]
    period: Option<String>,
    /// IANA time zone for periods and day boundaries [default: Europe/Paris]
    #[arg(long, value_name = "ZONE")]
    tz: Option<String>,
}

impl PeriodArgs {
    /// Validate locally and turn into query parameters.
    pub fn query(&self) -> Result<Vec<(&'static str, String)>> {
        let mut q = Vec::new();
        if let Some(p) = &self.period {
            q.push(("period", canonical("--period", p, &runs::PERIODS)?));
        }
        if let Some(f) = &self.from {
            q.push(("from", f.clone()));
        }
        if let Some(t) = &self.to {
            q.push(("to", t.clone()));
        }
        if let Some(tz) = &self.tz {
            q.push(("tz", tz.clone()));
        }
        Ok(q)
    }
}

/// The scenarios and the period of a fleet read.
#[derive(Args, Debug, Default)]
pub struct Selection {
    /// Only this scenario id (repeatable)
    #[arg(long = "scenario", value_name = "ID")]
    scenarios: Vec<String>,
    #[command(flatten)]
    period: PeriodArgs,
}

impl Selection {
    fn query(&self, max_scenarios: usize) -> Result<Vec<(&'static str, String)>> {
        if self.scenarios.len() > max_scenarios {
            bail!("at most {max_scenarios} --scenario values");
        }
        let mut q = Vec::new();
        for s in &self.scenarios {
            ensure_guid("--scenario", s)?;
            q.push(("scenario", s.clone()));
        }
        q.extend(self.period.query()?);
        Ok(q)
    }
}

pub async fn run(ctx: &Context, args: MetricsArgs) -> Result<()> {
    match args.command {
        MetricsCommand::Availability {
            selection,
            granularity,
            output,
        } => availability(ctx, &selection, granularity.as_deref(), output).await,
        MetricsCommand::ResponseTimes { selection, output } => {
            response_times(ctx, &selection, output).await
        }
        MetricsCommand::VoiceQuality { selection, output } => {
            voice_quality(ctx, &selection, output).await
        }
        MetricsCommand::Social {
            selection,
            interval,
            events,
            output,
        } => social(ctx, &selection, interval, events, output).await,
        MetricsCommand::SocialAnalysis { selection, output } => {
            social_analysis(ctx, &selection, output).await
        }
    }
}

async fn availability(
    ctx: &Context,
    selection: &Selection,
    granularity: Option<&str>,
    out: OutputArgs,
) -> Result<()> {
    let mut query = selection.query(100)?;
    if let Some(g) = granularity {
        query.push((
            "granularity",
            canonical("--granularity", g, &metrics::GRANULARITIES)?,
        ));
    }
    let m: AvailabilityMetrics = ctx
        .client()?
        .get_with("v2/metrics/availability", &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&m);
    }
    let s = &m.summary;
    println!(
        "Period:        {} ({} buckets); previous {}",
        period(&m.period),
        m.granularity,
        period(&m.previous_period)
    );
    println!(
        "Scenarios:     {} ({} observed)",
        m.scenario_count, m.observed_count
    );
    println!(
        "Target:        {}{}",
        pct(Some(m.target.effective)),
        if m.target.mixed {
            " (mixed: the mean of the scenarios' targets)"
        } else {
            ""
        }
    );
    println!(
        "Availability:  {} (previous {})",
        pct(s.rate),
        pct(s.previous_rate)
    );
    if let Some(b) = &s.error_budget {
        println!(
            "Error budget:  allowed {}, consumed {}, remaining {}{}",
            duration(b.allowed_down_seconds),
            duration(b.consumed_seconds),
            duration(b.remaining_seconds),
            match b.consumed_percent {
                Some(p) => format!(" ({p:.0}% used)"),
                None => String::new(),
            }
        );
    }
    println!(
        "Runs:          {} ok, {} failed, {} incident(s)",
        s.ok_runs, s.failed_runs, s.incidents
    );
    println!(
        "Up / down:     {} / {}",
        duration(s.up_seconds),
        duration(s.down_seconds)
    );
    println!(
        "MTBF / MTTR:   {} / {}; longest incident {}",
        opt_duration(s.mtbf_seconds),
        opt_duration(s.mttr_seconds),
        duration(s.longest_incident_seconds)
    );
    if s.best_day.is_some() || s.worst_day.is_some() {
        println!(
            "Best / worst:  {} / {}",
            day_rate(s.best_day.as_ref()),
            day_rate(s.worst_day.as_ref())
        );
    }

    if !m.by_scenario.is_empty() {
        println!();
        println!("By scenario (worst first):");
        let mut t = Table::new([
            "ID",
            "NAME",
            "TARGET",
            "RATE",
            "PREVIOUS",
            "OK",
            "FAILED",
            "INCIDENTS",
            "MTTR",
            "LONGEST",
        ]);
        for r in &m.by_scenario {
            t.row([
                r.scenario.id.clone(),
                r.scenario.name.clone(),
                pct(Some(r.target)),
                if r.error { "error".into() } else { pct(r.rate) },
                pct(r.previous_rate),
                r.ok_runs.to_string(),
                r.failed_runs.to_string(),
                r.incidents.to_string(),
                opt_duration(r.mttr_seconds),
                duration(r.longest_incident_seconds),
            ]);
        }
        t.print();
    }
    if !m.weakest_steps.is_empty() {
        println!();
        println!("Weakest steps:");
        let mut t = Table::new(["STEP", "FAILURES", "SHARE", "DOWNTIME", "SCENARIOS"]);
        for w in &m.weakest_steps {
            t.row([
                w.step.clone(),
                w.failures.to_string(),
                share(w.failure_share),
                duration(w.downtime_seconds),
                w.scenarios
                    .iter()
                    .map(|s| format!("{} ({})", s.scenario.name, s.failures))
                    .collect::<Vec<_>>()
                    .join(", "),
            ]);
        }
        t.print();
    }
    if let Some(f) = &m.failure_types
        && !f.types.is_empty()
    {
        println!();
        println!(
            "Failures by cause ({} failures, {} untyped, {} causes):",
            f.failures, f.untyped, f.type_count
        );
        let mut t = Table::new([
            "FAILURES",
            "SHARE",
            "STEP",
            "DESCRIPTION",
            "LAST",
            "LATEST RUN",
        ]);
        for ty in &f.types {
            t.row([
                ty.failures.to_string(),
                share(ty.failure_share),
                or_dash(ty.step.as_deref()),
                or_dash(ty.description.as_deref()),
                short_time(&ty.last),
                ty.latest_run.id.clone(),
            ]);
        }
        t.print();
    }
    if !m.longest_incidents.is_empty() {
        println!();
        println!("Longest incidents:");
        let mut t = Table::new([
            "ID",
            "SCENARIO",
            "START",
            "END",
            "DURATION",
            "FAILED STEP",
            "RUNS",
        ]);
        for i in &m.longest_incidents {
            t.row([
                i.id.clone(),
                i.scenario.name.clone(),
                short_time(&i.start),
                match &i.end {
                    Some(e) => short_time(e),
                    None if i.ongoing => "ongoing".into(),
                    None => "-".into(),
                },
                duration(i.duration_seconds),
                or_dash(i.failed_step.as_deref()),
                i.failed_runs.to_string(),
            ]);
        }
        t.print();
    }
    Ok(())
}

async fn response_times(ctx: &Context, selection: &Selection, out: OutputArgs) -> Result<()> {
    let query = selection.query(15)?;
    let m: ResponseTimesMetrics = ctx
        .client()?
        .get_with("v2/metrics/response-times", &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&m);
    }
    let s = &m.summary;
    println!(
        "Period:        {}; previous {}",
        period(&m.period),
        period(&m.previous_period)
    );
    println!(
        "Scenarios:     {} ({} measured, {} without thresholds, {} without data)",
        m.scenario_count, m.measured_count, s.without_thresholds, s.no_data
    );
    println!("Runs:          {}", s.runs);
    println!(
        "P85 / target:  {} (previous {}); 1.00 is on target",
        ratio(s.mean_p85_to_target_ratio),
        ratio(s.previous_mean_p85_to_target_ratio)
    );
    println!(
        "Bands:         {} on target, {} within client, {} insufficient, {} unacceptable",
        s.bands.target, s.bands.client, s.bands.insufficient, s.bands.unacceptable
    );
    if m.by_scenario.is_empty() {
        return Ok(());
    }
    println!();
    let mut t = Table::new([
        "ID",
        "NAME",
        "RUNS",
        "P50 MS",
        "P85 MS",
        "MEAN MS",
        "MAX MS",
        "PREV P85",
        "TARGET MS",
        "CLIENT MS",
    ]);
    for r in &m.by_scenario {
        t.row([
            r.scenario.id.clone(),
            r.scenario.name.clone(),
            if r.error {
                "error".into()
            } else {
                r.runs.to_string()
            },
            ms(r.p50_ms),
            ms(r.p85_ms),
            ms(r.mean_ms),
            ms(r.max_ms),
            ms(r.previous_p85_ms),
            ms(r.thresholds.as_ref().map(|t| t.target)),
            ms(r.thresholds.as_ref().map(|t| t.client)),
        ]);
    }
    t.print();
    Ok(())
}

async fn voice_quality(ctx: &Context, selection: &Selection, out: OutputArgs) -> Result<()> {
    let query = selection.query(20)?;
    let m: VoiceQualityMetrics = ctx
        .client()?
        .get_with("v2/metrics/voice-quality", &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&m);
    }
    println!("Period:        {}", period(&m.period));
    println!("Scenarios:     {}", m.scenario_count);
    print_figures(&m.totals);
    if m.by_scenario.is_empty() {
        return Ok(());
    }
    println!();
    let mut t = Table::new([
        "ID",
        "NAME",
        "CALLS",
        "MEASURED",
        "AVG MOS",
        "MIN MOS",
        "ERR %",
        "JITTER MS",
        "POOR+BAD",
    ]);
    for r in &m.by_scenario {
        let f = &r.figures;
        t.row([
            r.scenario.id.clone(),
            r.scenario.name.clone(),
            if r.error {
                "error".into()
            } else {
                f.calls.to_string()
            },
            f.measured.to_string(),
            f1(f.average_mos),
            f1(f.min_mos),
            f1(f.average_error_rate_percent),
            f1(f.average_jitter_ms),
            (f.ratings.poor + f.ratings.bad).to_string(),
        ]);
    }
    t.print();
    Ok(())
}

async fn social(
    ctx: &Context,
    selection: &Selection,
    interval: Option<u32>,
    events: bool,
    out: OutputArgs,
) -> Result<()> {
    let mut query = selection.query(30)?;
    if let Some(i) = interval {
        if !metrics::SOCIAL_INTERVALS.contains(&i) {
            bail!(
                "--interval must be one of {}",
                metrics::SOCIAL_INTERVALS
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        query.push(("interval", i.to_string()));
    }
    if events {
        query.push(("include", "events".into()));
    }
    let m: SocialMetrics = ctx
        .client()?
        .get_with("v2/metrics/social", &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&m);
    }
    let t = &m.totals;
    println!(
        "Period:        {} ({} min buckets, as of {})",
        period(&m.period),
        m.interval_minutes,
        short_time(&m.as_of)
    );
    println!(
        "Scenarios:     {}{}",
        m.scenario_count,
        if m.error_count > 0 {
            format!(" ({} not answered)", m.error_count)
        } else {
            String::new()
        }
    );
    println!(
        "Posts:         {} ({} incidents, {}); previous {} / {}",
        t.events,
        t.incidents,
        pct(t.incident_rate),
        opt_count(t.previous_events),
        opt_count(t.previous_incidents)
    );
    println!("Worst grade:   {}", t.worst_grade);
    let g = &t.time_in_grade;
    println!(
        "Time in grade: {} target, {} client, {} insufficient, {} unacceptable (buckets)",
        g.target, g.client, g.insufficient, g.unacceptable
    );
    if let Some(peak) = &t.peak_bucket_start {
        println!(
            "Peak bucket:   {} ({} incidents)",
            short_time(peak),
            t.peak_bucket_incidents
        );
    }
    if t.busiest_day.is_some() || t.peak_hour.is_some() {
        println!(
            "Busiest:       day {}, hour {}",
            or_dash(t.busiest_day.as_deref()),
            t.peak_hour.map_or("-".to_string(), |h| format!("{h:02}h"))
        );
    }

    if !m.by_scenario.is_empty() {
        println!();
        let mut tbl = Table::new([
            "ID",
            "NAME",
            "SOURCE",
            "STATE",
            "POSTS",
            "INCIDENTS",
            "RATE",
            "WORST",
            "LAST INCIDENT",
        ]);
        for r in &m.by_scenario {
            tbl.row([
                r.scenario.id.clone(),
                r.scenario.name.clone(),
                r.source.clone(),
                match &r.error {
                    Some(e) => format!("error: {e}"),
                    None => r.state.clone(),
                },
                r.events.to_string(),
                r.incidents.to_string(),
                pct(r.incident_rate),
                r.worst_grade.clone(),
                r.last_incident
                    .as_deref()
                    .map_or("-".to_string(), short_time),
            ]);
        }
        tbl.print();
    }
    if !m.days.is_empty() {
        println!();
        println!("By day:");
        let mut tbl = Table::new(["DATE", "POSTS", "INCIDENTS", "GRADE"]);
        for d in &m.days {
            tbl.row([
                d.date.clone(),
                d.events.to_string(),
                d.incidents.to_string(),
                d.grade.clone(),
            ]);
        }
        tbl.print();
    }
    if let Some(ev) = &m.events {
        println!();
        println!(
            "Posts, newest first ({} of {}{}):",
            ev.len(),
            m.events_total,
            if m.events_truncated {
                ", truncated"
            } else {
                ""
            }
        );
        let mut tbl = Table::new(["AT", "SCENARIO", "INCIDENT", "TEXT", "LINK"]);
        for e in ev {
            tbl.row([
                short_time(&e.at),
                e.scenario.name.clone(),
                if e.incident { "yes" } else { "" }.to_string(),
                one_line(e.text.as_deref()),
                or_dash(e.link.as_deref()),
            ]);
        }
        tbl.print();
    }
    Ok(())
}

async fn social_analysis(ctx: &Context, selection: &Selection, out: OutputArgs) -> Result<()> {
    let query = selection.query(30)?;
    let m: SocialAnalysisMetrics = ctx
        .client()?
        .get_with("v2/metrics/social/analysis", &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&m);
    }
    println!("Period:        {}", period(&m.period));
    println!(
        "Scenarios:     {}{}",
        m.scenario_count,
        if m.error_count > 0 {
            format!(" ({} not answered)", m.error_count)
        } else {
            String::new()
        }
    );
    println!(
        "Keywords:      {}",
        if m.keywords.is_empty() {
            "-".to_string()
        } else {
            m.keywords
                .iter()
                .map(|k| format!("{} ({})", k.code, k.count))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    if m.days.is_empty() {
        if crate::output::stdout_is_tty() {
            eprintln!("No day with a reading in this period");
        }
        return Ok(());
    }
    for d in &m.days {
        println!();
        println!(
            "{}  {}  {} posts, {} incidents ({})",
            d.date,
            d.scenario.name,
            d.events,
            d.incidents,
            pct(d.incident_rate)
        );
        for issue in &d.online_issues {
            println!("  online:   {issue}");
        }
        for issue in &d.other_issues {
            println!("  other:    {issue}");
        }
        if !d.keywords.is_empty() {
            println!("  keywords: {}", d.keywords.join(", "));
        }
    }
    Ok(())
}

/// The lines every voice quality view shares.
pub fn print_figures(f: &VoiceQualityFigures) {
    println!(
        "Calls:         {} ({} measured, {} without quality, {} not collected)",
        f.calls, f.measured, f.no_quality, f.not_collected
    );
    println!(
        "MOS:           avg {} (min {}, max {})",
        f1(f.average_mos),
        f1(f.min_mos),
        f1(f.max_mos)
    );
    println!(
        "Error rate:    avg {}% (max {}%)",
        f1(f.average_error_rate_percent),
        f1(f.max_error_rate_percent)
    );
    println!(
        "Jitter:        avg {} ms (max {} ms)",
        f1(f.average_jitter_ms),
        f1(f.max_jitter_ms)
    );
    let r = &f.ratings;
    println!(
        "Ratings:       {} excellent, {} good, {} fair, {} poor, {} bad",
        r.excellent, r.good, r.fair, r.poor, r.bad
    );
}

// ---------- formatting ----------

/// `YYYY-MM-DD to YYYY-MM-DD`: the days of a half-open range of instants.
pub fn period(p: &metrics::Period) -> String {
    format!("{} to {}", day(&p.from), day(&p.to))
}

fn day(iso: &str) -> &str {
    if iso.len() >= 10 { &iso[..10] } else { iso }
}

/// A percentage with two decimals, or a dash.
pub fn pct(v: Option<f64>) -> String {
    match v {
        Some(v) => format!("{v:.2}%"),
        None => "-".into(),
    }
}

/// A 0-1 share as a percentage.
pub fn share(v: f64) -> String {
    format!("{:.0}%", v * 100.0)
}

/// A ratio with two decimals, or a dash.
pub fn ratio(v: Option<f64>) -> String {
    match v {
        Some(v) => format!("{v:.2}"),
        None => "-".into(),
    }
}

/// One decimal, or a dash.
pub fn f1(v: Option<f64>) -> String {
    match v {
        Some(v) => format!("{v:.1}"),
        None => "-".into(),
    }
}

/// Whole milliseconds, or a dash.
pub fn ms(v: Option<f64>) -> String {
    match v {
        Some(v) => format!("{v:.0}"),
        None => "-".into(),
    }
}

/// Seconds as `1d2h`, `3h04m`, `5m10s` or `42s`; negative values keep their sign.
pub fn duration(seconds: f64) -> String {
    let negative = seconds < 0.0;
    let total = seconds.abs().round() as i64;
    let (d, h, m, s) = (
        total / 86_400,
        (total % 86_400) / 3600,
        (total % 3600) / 60,
        total % 60,
    );
    let body = if d > 0 {
        format!("{d}d{h}h")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    };
    if negative { format!("-{body}") } else { body }
}

pub fn opt_duration(seconds: Option<f64>) -> String {
    match seconds {
        Some(s) => duration(s),
        None => "-".into(),
    }
}

fn opt_count(v: Option<i64>) -> String {
    v.map_or("-".to_string(), |v| v.to_string())
}

/// Whitespace collapsed to single spaces so a post fits one table row.
fn one_line(text: Option<&str>) -> String {
    match text {
        Some(t) if !t.trim().is_empty() => t.split_whitespace().collect::<Vec<_>>().join(" "),
        _ => "-".into(),
    }
}

fn day_rate(d: Option<&metrics::DayRate>) -> String {
    match d {
        Some(d) => format!("{} {}", d.date, pct(Some(d.rate))),
        None => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_builds_the_query() {
        let s = Selection {
            scenarios: vec!["33333333-3333-4333-8333-333333333333".into()],
            period: PeriodArgs {
                period: Some("LAST7D".into()),
                tz: Some("UTC".into()),
                ..Default::default()
            },
        };
        assert_eq!(
            s.query(10).unwrap(),
            vec![
                (
                    "scenario",
                    "33333333-3333-4333-8333-333333333333".to_string()
                ),
                ("period", "last7d".to_string()),
                ("tz", "UTC".to_string()),
            ]
        );
    }

    #[test]
    fn selection_rejects_too_many_scenarios_and_bad_periods() {
        let s = Selection {
            scenarios: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        assert!(s.query(1).unwrap_err().to_string().contains("at most 1"));
        let s = Selection {
            period: PeriodArgs {
                period: Some("lastYear".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(s.query(1).unwrap_err().to_string().contains("--period"));
    }

    #[test]
    fn durations_pick_the_two_largest_units() {
        assert_eq!(duration(42.0), "42s");
        assert_eq!(duration(310.0), "5m10s");
        assert_eq!(duration(3_840.0), "1h04m");
        assert_eq!(duration(93_600.0), "1d2h");
        assert_eq!(duration(-90.0), "-1m30s");
    }

    #[test]
    fn periods_show_days() {
        let p = metrics::Period {
            from: "2026-09-12T00:00:00+02:00".into(),
            to: "2026-10-12T00:00:00+02:00".into(),
        };
        assert_eq!(period(&p), "2026-09-12 to 2026-10-12");
        assert_eq!(pct(Some(99.123)), "99.12%");
        assert_eq!(share(0.256), "26%");
        assert_eq!(ms(Some(1234.6)), "1235");
    }
}
