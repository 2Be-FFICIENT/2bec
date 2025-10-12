//! `2bec runs`: list and inspect scenario runs.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use crate::api::page::{Fetch, fetch_items};
use crate::api::runs::{self, Artifact, Diagnosis, Run, RunListItem};
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
pub struct RunsArgs {
    #[command(subcommand)]
    command: RunsCommand,
}

#[derive(Subcommand, Debug)]
enum RunsCommand {
    /// List runs, newest first (last 24 hours by default)
    #[command(after_help = "\
Examples:
  2bec runs list --status failed --period today
  2bec runs list --scenario 7f3c... --period last7d --all --json
  2bec runs list --from 2026-10-01T00:00:00Z --to 2026-10-02T00:00:00Z

Periods: last24h, last7d, last30d, today, yesterday, thisWeek, lastWeek,
thisMonth, lastMonth. A range spans at most 31 days.")]
    List {
        #[command(flatten)]
        filter: RunFilter,
        /// Maximum number of runs to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every matching run, following pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one run: steps, failure, tags, artifacts
    Get {
        /// Run id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Explain why a run failed: verdict, journey, optional AI diagnosis
    #[command(after_help = "\
Only failed runs have a diagnosis. --opale asks for the AI analysis, which
can take up to two minutes the first time; raise --timeout accordingly.")]
    Diagnosis {
        /// Run id
        id: String,
        /// Include the Opale AI analysis
        #[arg(long)]
        opale: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// List the files a run produced (video, screenshot, wav)
    Artifacts {
        /// Run id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Download one artifact of a run
    #[command(after_help = "\
Examples:
  2bec runs download 7f3c... video              # writes 7f3c....mp4
  2bec runs download 7f3c... screenshot -o s.png
  2bec runs download 7f3c... wav -o - | ffplay -")]
    Download {
        /// Run id
        id: String,
        /// Artifact kind: video, screenshot or wav
        kind: ArtifactKind,
        /// Destination file; `-` for stdout [default: <id>.<ext>]
        #[arg(long, short = 'o', value_name = "FILE")]
        output: Option<String>,
    },
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum ArtifactKind {
    Video,
    Screenshot,
    Wav,
}

impl ArtifactKind {
    fn path_segment(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Screenshot => "screenshot",
            Self::Wav => "wav",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Video => "mp4",
            Self::Screenshot => "png",
            Self::Wav => "wav",
        }
    }
}

/// Filters shared by the run list routes.
#[derive(Args, Debug, Default)]
pub struct RunFilter {
    /// Only runs of this scenario id (repeatable, at most 100)
    #[arg(long = "scenario", value_name = "ID")]
    scenarios: Vec<String>,
    /// Only runs with this status (repeatable): ok, failed, outOfScope, neutralized, notObserved
    #[arg(long = "status", value_name = "STATUS")]
    statuses: Vec<String>,
    /// Only runs that failed at this step id
    #[arg(long, value_name = "ID")]
    failed_step: Option<String>,
    /// Only replays (true) or only regular runs (false)
    #[arg(long, value_name = "BOOL")]
    replay: Option<bool>,
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

impl RunFilter {
    /// Validate locally and turn into query parameters.
    fn query(&self) -> Result<Vec<(&'static str, String)>> {
        let mut q = Vec::new();
        if self.scenarios.len() > 100 {
            bail!("at most 100 --scenario values");
        }
        for s in &self.scenarios {
            ensure_guid("--scenario", s)?;
            q.push(("scenario", s.clone()));
        }
        for s in &self.statuses {
            let canonical = runs::STATUSES
                .iter()
                .find(|v| v.eq_ignore_ascii_case(s))
                .ok_or_else(|| {
                    anyhow::anyhow!("--status `{s}` is not one of {}", runs::STATUSES.join(", "))
                })?;
            q.push(("status", (*canonical).to_string()));
        }
        if let Some(step) = &self.failed_step {
            ensure_guid("--failed-step", step)?;
            q.push(("failedStep", step.clone()));
        }
        if let Some(r) = self.replay {
            q.push(("replay", r.to_string()));
        }
        if let Some(p) = &self.period {
            let canonical = runs::PERIODS
                .iter()
                .find(|v| v.eq_ignore_ascii_case(p))
                .ok_or_else(|| {
                    anyhow::anyhow!("--period `{p}` is not one of {}", runs::PERIODS.join(", "))
                })?;
            q.push(("period", (*canonical).to_string()));
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

pub async fn run(ctx: &Context, args: RunsArgs) -> Result<()> {
    match args.command {
        RunsCommand::List {
            filter,
            limit,
            all,
            output,
        } => list(ctx, &filter, limit, all, output).await,
        RunsCommand::Get { id, output } => get(ctx, &id, output).await,
        RunsCommand::Diagnosis { id, opale, output } => diagnosis(ctx, &id, opale, output).await,
        RunsCommand::Artifacts { id, output } => artifacts(ctx, &id, output).await,
        RunsCommand::Download { id, kind, output } => download(ctx, &id, kind, output).await,
    }
}

async fn diagnosis(ctx: &Context, id: &str, opale: bool, out: OutputArgs) -> Result<()> {
    ensure_guid("run id", id)?;
    let query = [("opale", opale.to_string())];
    let d: Diagnosis = ctx
        .client()?
        .get_with(&format!("v2/runs/{id}/diagnosis"), &query)
        .await?
        .json()?;
    if out.json {
        return crate::output::print_json(&d);
    }
    println!("Run:          {}", d.id);
    println!("Scenario:     {} ({})", d.scenario.name, d.scenario.id);
    println!("Started:      {}", d.started_at);
    println!("Status:       {}", d.status);
    println!(
        "Failed step:  {}",
        step_name(d.failure.failed_step.as_ref())
    );
    println!(
        "Action:       {}",
        or_dash(d.failure.failed_action.as_deref())
    );
    println!(
        "Description:  {}",
        or_dash(d.failure.description.as_deref())
    );
    if let Some(c) = d.failure.confidence {
        println!("Confidence:   {c}/5");
    }
    if let Some(e) = &d.verdict.error_message {
        println!("Error:        {e}");
    }
    if !d.verdict.text_not_found.is_empty() {
        println!("Text not found: {}", d.verdict.text_not_found.join(" | "));
    }
    if !d.verdict.web_service_errors.is_empty() {
        println!(
            "Web service errors: {}",
            d.verdict.web_service_errors.join(" | ")
        );
    }
    if !d.journey.is_empty() {
        println!();
        let mut t = Table::new(["#", "STEP", "STATE", "OFFSET", "DURATION", "ACTIONS"]);
        for j in &d.journey {
            t.row([
                j.order.to_string(),
                j.label.clone(),
                j.state.clone(),
                duration(j.offset_ms),
                duration(j.duration_ms),
                j.action_count.to_string(),
            ]);
        }
        t.print();
    }
    match &d.opale {
        Some(o) => {
            println!();
            println!(
                "Opale diagnosis (confidence {}/5{})",
                o.confidence,
                flag(o.recycled, ", cached")
            );
            println!("  {}", o.description);
            if let Some(r) = &o.confidence_reason {
                println!("  Reason: {r}");
            }
            for c in &o.possible_causes {
                println!("  - cause: {c}");
            }
            for tip in &o.tips {
                println!("  - tip: {tip}");
            }
        }
        None if d.opale_available && !opale => {
            eprintln!("An AI diagnosis is available: add --opale");
        }
        None => {}
    }
    if !d.comments.is_empty() {
        println!();
        println!("Comments");
        for c in &d.comments {
            println!("  [{}] {}: {}", short_time(&c.at), c.author, c.text);
        }
    }
    Ok(())
}

async fn artifacts(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("run id", id)?;
    let page: crate::api::page::Page<Artifact> = ctx
        .client()?
        .get(&format!("v2/runs/{id}/artifacts"))
        .await?
        .json()?;
    out.print(&page.items, "This run has no artifacts", || {
        let mut t = Table::new(["KIND", "CONTENT TYPE", "URL"]);
        for a in &page.items {
            t.row([a.kind.clone(), a.content_type.clone(), a.url.clone()]);
        }
        t
    })
}

async fn download(
    ctx: &Context,
    id: &str,
    kind: ArtifactKind,
    output: Option<String>,
) -> Result<()> {
    ensure_guid("run id", id)?;
    let path = format!("v2/runs/{id}/artifacts/{}", kind.path_segment());
    let client = ctx.client()?;
    let target = output.unwrap_or_else(|| format!("{id}.{}", kind.extension()));
    if target == "-" {
        let mut stdout = std::io::stdout().lock();
        client.download(&path, &mut stdout).await?;
        return Ok(());
    }
    let tmp = format!("{target}.part");
    let mut file =
        std::fs::File::create(&tmp).map_err(|e| anyhow::anyhow!("cannot create {tmp}: {e}"))?;
    let result = client.download(&path, &mut file).await;
    drop(file);
    match result {
        Ok((_, bytes)) => {
            std::fs::rename(&tmp, &target)
                .map_err(|e| anyhow::anyhow!("cannot write {target}: {e}"))?;
            eprintln!("Saved {target} ({bytes} bytes)");
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.into())
        }
    }
}

async fn list(
    ctx: &Context,
    filter: &RunFilter,
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    let query = filter.query()?;
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let page = fetch_items::<RunListItem>(&ctx.client()?, "v2/runs", &query, fetch).await?;
    out.print(&page.items, "No runs match these filters", || {
        let mut t = Table::new([
            "ID",
            "STARTED",
            "SCENARIO",
            "STATUS",
            "DURATION",
            "FAILED STEP",
            "TAGS",
        ]);
        for r in &page.items {
            t.row([
                r.id.clone(),
                short_time(&r.started_at),
                r.scenario.name.clone(),
                r.status.clone(),
                duration(r.duration_ms),
                step_name(r.failed_step.as_ref()),
                r.tags.join(","),
            ]);
        }
        t
    })?;
    if !out.json
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} runs; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("run id", id)?;
    let run: Run = ctx.client()?.get(&format!("v2/runs/{id}")).await?.json()?;
    if out.json {
        return crate::output::print_json(&run);
    }
    println!("Run:         {}", run.id);
    println!("Scenario:    {} ({})", run.scenario.name, run.scenario.id);
    println!("Started:     {}", run.started_at);
    println!("Ended:       {}", or_dash(run.ended_at.as_deref()));
    println!(
        "Status:      {}{}",
        run.status,
        flag(run.replay, " (replay)")
    );
    println!("Performance: {}", or_dash(run.performance.as_deref()));
    println!("Duration:    {}", duration(run.duration_ms));
    println!("Tags:        {}", or_dash(Some(&run.tags.join(", "))));
    println!("Artifacts:   {}", or_dash(Some(&run.artifacts.join(", "))));
    println!("Comments:    {}", run.comment_count);
    if let Some(reason) = &run.not_observed_reason {
        println!("Not observed: {reason}");
    }
    if let Some(f) = &run.failure {
        println!();
        println!("Failure");
        println!("  Step:        {}", step_name(f.failed_step.as_ref()));
        println!("  Action:      {}", or_dash(f.failed_action.as_deref()));
        println!("  Description: {}", or_dash(f.description.as_deref()));
        if let Some(c) = f.confidence {
            println!("  Confidence:  {c}/5");
        }
        if f.in_planned_interruption {
            println!("  During a planned interruption");
        }
    }
    if !run.steps.is_empty() {
        println!();
        let mut t = Table::new(["#", "STEP", "DURATION", "SERVER", "BYTES", "PERF"]);
        for s in &run.steps {
            t.row([
                s.order.to_string(),
                s.label.clone(),
                duration(s.duration_ms),
                duration(s.server_ms),
                s.bytes.map_or("-".into(), |b| b.to_string()),
                or_dash(s.performance.as_deref()),
            ]);
        }
        t.print();
    }
    Ok(())
}

/// Milliseconds as `1.234s`, or a dash.
fn duration(ms: Option<i64>) -> String {
    match ms {
        Some(ms) if ms >= 1000 => format!("{:.1}s", ms as f64 / 1000.0),
        Some(ms) => format!("{ms}ms"),
        None => "-".into(),
    }
}

fn step_name(step: Option<&runs::StepRef>) -> String {
    let label = step.and_then(|s| s.label.as_deref().or(s.name.as_deref()));
    or_dash(label)
}

fn flag(on: bool, text: &str) -> &str {
    if on { text } else { "" }
}

/// Reject ids the API would answer with a generic 400 for.
pub fn ensure_guid(what: &str, value: &str) -> Result<()> {
    let parts: Vec<&str> = value.split('-').collect();
    let shape = [8, 4, 4, 4, 12];
    let ok = parts.len() == 5
        && parts
            .iter()
            .zip(shape)
            .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()));
    if !ok {
        bail!("{what} must be a GUID like 7f3c1a2b-0000-4000-8000-000000000000, got `{value}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "7f3c1a2b-0000-4000-8000-000000000000";

    #[test]
    fn guid_check() {
        assert!(ensure_guid("x", G).is_ok());
        assert!(ensure_guid("x", "7f3c").is_err());
        assert!(ensure_guid("x", "7f3c1a2b-0000-4000-8000-00000000000g").is_err());
    }

    #[test]
    fn filter_builds_canonical_query() {
        let f = RunFilter {
            scenarios: vec![G.into()],
            statuses: vec!["FAILED".into(), "outofscope".into()],
            failed_step: None,
            replay: Some(false),
            from: None,
            to: None,
            period: Some("LAST7D".into()),
            tz: Some("UTC".into()),
        };
        let q = f.query().unwrap();
        assert_eq!(
            q,
            vec![
                ("scenario", G.to_string()),
                ("status", "failed".into()),
                ("status", "outOfScope".into()),
                ("replay", "false".into()),
                ("period", "last7d".into()),
                ("tz", "UTC".into()),
            ]
        );
    }

    #[test]
    fn filter_rejects_bad_values() {
        let f = RunFilter {
            statuses: vec!["broken".into()],
            ..RunFilter::default()
        };
        assert!(f.query().unwrap_err().to_string().contains("--status"));
        let f = RunFilter {
            period: Some("lastYear".into()),
            ..RunFilter::default()
        };
        assert!(f.query().unwrap_err().to_string().contains("--period"));
        let f = RunFilter {
            scenarios: vec!["nope".into()],
            ..RunFilter::default()
        };
        assert!(f.query().unwrap_err().to_string().contains("--scenario"));
    }

    #[test]
    fn durations_are_human() {
        assert_eq!(duration(Some(950)), "950ms");
        assert_eq!(duration(Some(1234)), "1.2s");
        assert_eq!(duration(None), "-");
    }
}
