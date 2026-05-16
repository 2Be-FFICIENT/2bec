//! `2bec runs`: list and inspect scenario runs.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use crate::api::page::{Fetch, fetch_items};
use crate::api::runs::{
    self, Artifact, Diagnosis, Run, RunComment, RunListItem, RunRequest, RunRequestState,
    RunRequestWrite, RunTags,
};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};
use std::io::Read;

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
    /// Read and write the comments of a run
    Comments(CommentsArgs),
    /// Attach tags to a run (needs runs:write)
    #[command(after_help = "Example:\n  2bec runs tag 7f3c... 1a2b... 3c4d...")]
    Tag {
        /// Run id
        id: String,
        /// Tag ids to attach
        #[arg(required = true, value_name = "TAG_ID")]
        tags: Vec<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Detach one tag from a run (needs runs:write)
    Untag {
        /// Run id
        id: String,
        /// Tag id to detach
        tag: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Ask for an immediate run of a scenario (needs runs:request)
    #[command(
        alias = "request",
        after_help = "\
Examples:
  2bec runs trigger 7f3c...
  2bec runs trigger 7f3c... --var USER=bob --var ENV=staging --comment \"smoke\"
  2bec runs trigger 7f3c... --wait --json

Check what the scenario accepts and the remaining quota first with
`2bec runs requests state <scenario>`. --wait polls the request until it is
done, failed or refused."
    )]
    Trigger {
        /// Scenario id
        scenario: String,
        /// Variable for the run, NAME=VALUE (repeatable, at most 20)
        #[arg(long = "var", value_name = "NAME=VALUE")]
        vars: Vec<String>,
        /// Free text attached to the request (at most 500 characters)
        #[arg(long, value_name = "TEXT")]
        comment: Option<String>,
        /// Poll the request until it finishes
        #[arg(long)]
        wait: bool,
        /// Give up waiting after this many seconds
        #[arg(long, default_value_t = 600, value_name = "SECS", requires = "wait")]
        wait_timeout: u64,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Inspect immediate-run requests (needs runs:request)
    Requests(RequestsArgs),
}

#[derive(Args, Debug)]
pub struct RequestsArgs {
    #[command(subcommand)]
    command: RequestsCommand,
}

#[derive(Subcommand, Debug)]
enum RequestsCommand {
    /// List run requests, newest first (last 7 days by default)
    List {
        /// Only requests for this scenario id
        #[arg(long, value_name = "ID")]
        scenario: Option<String>,
        /// Start of the range, RFC 3339 (inclusive)
        #[arg(long, value_name = "TIME", conflicts_with = "period")]
        from: Option<String>,
        /// End of the range, RFC 3339 (exclusive)
        #[arg(long, value_name = "TIME", conflicts_with = "period")]
        to: Option<String>,
        /// Named period instead of --from/--to (e.g. today, last30d)
        #[arg(long, value_name = "PERIOD")]
        period: Option<String>,
        /// IANA time zone for periods [default: Europe/Paris]
        #[arg(long, value_name = "ZONE")]
        tz: Option<String>,
        /// Maximum number of requests to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every matching request
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one run request and its current stage
    Get {
        /// Run request id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Whether a scenario can be triggered now: quota, variables, recent requests
    State {
        /// Scenario id
        scenario: String,
        #[command(flatten)]
        output: OutputArgs,
    },
}

#[derive(Args, Debug)]
pub struct CommentsArgs {
    #[command(subcommand)]
    command: CommentsCommand,
}

#[derive(Subcommand, Debug)]
enum CommentsCommand {
    /// List the comments of a run, oldest first
    List {
        /// Run id
        run: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one comment and its ETag
    Get {
        /// Run id
        run: String,
        /// Comment id
        comment: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Add a comment (needs runs:write)
    #[command(after_help = "\
TEXT may be a literal, `-` to read stdin, or `@file` to read a file.
The request carries an Idempotency-Key, so a retried call does not post twice.")]
    Add {
        /// Run id
        run: String,
        /// Comment text (1 to 2048 characters)
        text: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Replace the text of a comment you wrote (needs runs:write)
    Edit {
        /// Run id
        run: String,
        /// Comment id
        comment: String,
        /// New text; `-` for stdin, `@file` for a file
        text: String,
        /// Only update if the comment still has this ETag (from `comments get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete a comment you wrote (needs runs:write)
    #[command(alias = "rm")]
    Delete {
        /// Run id
        run: String,
        /// Comment id
        comment: String,
        /// Only delete if the comment still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
}

#[derive(serde::Serialize)]
struct CommentWrite<'a> {
    text: &'a str,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TagsWrite<'a> {
    tags: &'a [String],
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
        RunsCommand::Comments(args) => comments(ctx, args.command).await,
        RunsCommand::Tag { id, tags, output } => tag(ctx, &id, &tags, output).await,
        RunsCommand::Untag { id, tag, output } => untag(ctx, &id, &tag, output).await,
        RunsCommand::Trigger {
            scenario,
            vars,
            comment,
            wait,
            wait_timeout,
            output,
        } => trigger(ctx, &scenario, &vars, comment, wait, wait_timeout, output).await,
        RunsCommand::Requests(args) => requests(ctx, args.command).await,
    }
}

async fn trigger(
    ctx: &Context,
    scenario: &str,
    vars: &[String],
    comment: Option<String>,
    wait: bool,
    wait_timeout: u64,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("scenario id", scenario)?;
    let body = build_run_request(vars, comment)?;
    let client = ctx.client()?;
    let mut req: RunRequest = client
        .post_json(
            &format!("v2/scenarios/{scenario}/run-requests"),
            &body,
            &WriteOptions::idempotent(),
        )
        .await?
        .json()?;
    if !out.wants_json() {
        println!(
            "Requested run {} of {}: {}",
            req.id,
            req.scenario.name,
            outcome_line(&req)
        );
        for w in &req.warnings {
            eprintln!("warning: {w}");
        }
    }
    if wait && req.outcome == "accepted" {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait_timeout);
        let mut last_stage = req.stage.clone();
        loop {
            if is_final(&req) {
                break;
            }
            if std::time::Instant::now() >= deadline {
                bail!(
                    "still {} after {wait_timeout}s; poll with `2bec runs requests get {}`",
                    req.stage.as_deref().unwrap_or("pending"),
                    req.id
                );
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            req = client
                .get(&format!("v2/run-requests/{}", req.id))
                .await?
                .json()?;
            if !out.wants_json() && req.stage != last_stage {
                eprintln!("  {}", outcome_line(&req));
                last_stage = req.stage.clone();
            }
        }
    }
    if out.wants_json() {
        return out.print_json(&req);
    }
    if wait {
        println!("Final: {}", outcome_line(&req));
    }
    Ok(())
}

fn is_final(req: &RunRequest) -> bool {
    req.outcome != "accepted" || matches!(req.stage.as_deref(), Some("done" | "forbiddenView"))
}

fn outcome_line(req: &RunRequest) -> String {
    let mut s = req.outcome.clone();
    if let Some(stage) = &req.stage {
        s.push_str(&format!(" ({stage})"));
    }
    if let Some(e) = &req.error {
        s.push_str(&format!(": {e}"));
    }
    s
}

/// Validate `--var` and `--comment` the way the API does, before sending.
fn build_run_request(vars: &[String], comment: Option<String>) -> Result<RunRequestWrite> {
    let mut body = RunRequestWrite::default();
    if vars.len() > 20 {
        bail!("at most 20 --var values");
    }
    for v in vars {
        let Some((name, value)) = v.split_once('=') else {
            bail!("--var `{v}` must be NAME=VALUE");
        };
        let first_ok = name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        let rest_ok = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !first_ok || !rest_ok || name.len() > 64 {
            bail!(
                "--var name `{name}` must match [A-Za-z_][A-Za-z0-9_]* and be at most 64 characters"
            );
        }
        if name.starts_with("SCHEDULER_") {
            bail!("--var name `{name}` is reserved (SCHEDULER_ prefix)");
        }
        if value.chars().count() > 4000 || value.chars().any(char::is_control) {
            bail!(
                "--var `{name}` value must be at most 4000 characters without control characters"
            );
        }
        body.variables.insert(name.to_string(), value.to_string());
    }
    if let Some(c) = comment {
        let c = c.trim().to_string();
        if c.chars().count() > 500 {
            bail!("--comment must be at most 500 characters");
        }
        if !c.is_empty() {
            body.comment = Some(c);
        }
    }
    Ok(body)
}

async fn requests(ctx: &Context, cmd: RequestsCommand) -> Result<()> {
    match cmd {
        RequestsCommand::List {
            scenario,
            from,
            to,
            period,
            tz,
            limit,
            all,
            output,
        } => {
            let filter = RunFilter {
                scenarios: scenario.into_iter().collect(),
                from,
                to,
                period,
                tz,
                ..RunFilter::default()
            };
            let query = filter.query()?;
            if !all && limit == 0 {
                bail!("--limit must be at least 1");
            }
            let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
            let page =
                fetch_items::<RunRequest>(&ctx.client()?, "v2/run-requests", &query, fetch).await?;
            output.print(&page.items, "No run requests in this period", || {
                let mut t = Table::new(["ID", "REQUESTED", "SCENARIO", "OUTCOME", "STAGE", "BY"]);
                for r in &page.items {
                    t.row([
                        r.id.clone(),
                        short_time(&r.requested_at),
                        r.scenario.name.clone(),
                        r.outcome.clone(),
                        or_dash(r.stage.as_deref()),
                        or_dash(r.requested_by.as_deref()),
                    ]);
                }
                t
            })
        }
        RequestsCommand::Get { id, output } => {
            ensure_guid("run request id", &id)?;
            let r: RunRequest = ctx
                .client()?
                .get(&format!("v2/run-requests/{id}"))
                .await?
                .json()?;
            if output.wants_json() {
                return output.print_json(&r);
            }
            println!("Request:   {}", r.id);
            println!("Scenario:  {} ({})", r.scenario.name, r.scenario.id);
            println!(
                "Requested: {} by {}",
                r.requested_at,
                or_dash(r.requested_by.as_deref())
            );
            println!("Outcome:   {}", outcome_line(&r));
            println!("Comment:   {}", or_dash(r.comment.as_deref()));
            for (k, v) in &r.variables {
                println!("Variable:  {k}={v}");
            }
            for w in &r.warnings {
                println!("Warning:   {w}");
            }
            Ok(())
        }
        RequestsCommand::State { scenario, output } => {
            ensure_guid("scenario id", &scenario)?;
            let st: RunRequestState = ctx
                .client()?
                .get(&format!("v2/scenarios/{scenario}/run-requests"))
                .await?
                .json()?;
            if output.wants_json() {
                return output.print_json(&st);
            }
            println!("Scenario:    {} ({})", st.scenario.name, st.scenario.id);
            println!(
                "Can trigger: {}{}",
                yes_no(st.schedulable && st.quota.allowed),
                st.not_schedulable_reason
                    .as_deref()
                    .map_or(String::new(), |r| format!(" ({r})"))
            );
            println!(
                "Active:      {} (job {})",
                yes_no(st.active),
                yes_no(st.job_active)
            );
            if st.paused {
                println!("Paused:      since {}", or_dash(st.paused_since.as_deref()));
            }
            if st.in_off_period {
                println!(
                    "Off period:  until {}",
                    or_dash(st.off_period_end.as_deref())
                );
            }
            println!(
                "Quota:       {} of {} left in {} min (min interval {} min){}",
                st.quota.remaining,
                st.quota.limit,
                st.quota.window_minutes,
                st.quota.min_interval_minutes,
                st.quota
                    .next_allowed_at
                    .as_deref()
                    .map_or(String::new(), |t| format!(", next allowed {t}"))
            );
            if !st.variables.is_empty() {
                println!();
                let mut t = Table::new(["VARIABLE", "DEFAULT"]);
                for v in &st.variables {
                    t.row([v.name.clone(), or_dash(v.default_value.as_deref())]);
                }
                t.print();
            }
            if !st.recent.is_empty() {
                println!();
                let mut t = Table::new(["ID", "REQUESTED", "OUTCOME", "STAGE"]);
                for r in &st.recent {
                    t.row([
                        r.id.clone(),
                        short_time(&r.requested_at),
                        r.outcome.clone(),
                        or_dash(r.stage.as_deref()),
                    ]);
                }
                t.print();
            }
            Ok(())
        }
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

async fn comments(ctx: &Context, cmd: CommentsCommand) -> Result<()> {
    match cmd {
        CommentsCommand::List { run, output } => {
            ensure_guid("run id", &run)?;
            let page: crate::api::page::Page<RunComment> = ctx
                .client()?
                .get(&format!("v2/runs/{run}/comments"))
                .await?
                .json()?;
            output.print(&page.items, "This run has no comments", || {
                comment_table(&page.items)
            })
        }
        CommentsCommand::Get {
            run,
            comment,
            output,
        } => {
            ensure_guid("run id", &run)?;
            ensure_guid("comment id", &comment)?;
            let resp = ctx
                .client()?
                .get(&format!("v2/runs/{run}/comments/{comment}"))
                .await?;
            let c: RunComment = resp.json()?;
            print_comment(&c, resp.etag.as_deref(), output)
        }
        CommentsCommand::Add { run, text, output } => {
            ensure_guid("run id", &run)?;
            let text = read_text(&text)?;
            ensure_comment_text(&text)?;
            let resp = ctx
                .client()?
                .post_json(
                    &format!("v2/runs/{run}/comments"),
                    &CommentWrite { text: &text },
                    &WriteOptions::idempotent(),
                )
                .await?;
            let c: RunComment = resp.json()?;
            print_comment(&c, resp.etag.as_deref(), output)
        }
        CommentsCommand::Edit {
            run,
            comment,
            text,
            if_match,
            output,
        } => {
            ensure_guid("run id", &run)?;
            ensure_guid("comment id", &comment)?;
            let text = read_text(&text)?;
            ensure_comment_text(&text)?;
            let write = WriteOptions {
                idempotency_key: None,
                if_match,
            };
            let resp = ctx
                .client()?
                .patch_merge(
                    &format!("v2/runs/{run}/comments/{comment}"),
                    &CommentWrite { text: &text },
                    &write,
                )
                .await?;
            let c: RunComment = resp.json()?;
            print_comment(&c, resp.etag.as_deref(), output)
        }
        CommentsCommand::Delete {
            run,
            comment,
            if_match,
        } => {
            ensure_guid("run id", &run)?;
            ensure_guid("comment id", &comment)?;
            let write = WriteOptions {
                idempotency_key: None,
                if_match,
            };
            ctx.client()?
                .delete(&format!("v2/runs/{run}/comments/{comment}"), &write)
                .await?;
            eprintln!("Deleted comment {comment}");
            Ok(())
        }
    }
}

fn comment_table(items: &[RunComment]) -> Table {
    let mut t = Table::new(["ID", "AT", "AUTHOR", "TEXT"]);
    for c in items {
        t.row([
            c.id.clone(),
            short_time(&c.at),
            c.author.clone(),
            c.text.replace('\n', " "),
        ]);
    }
    t
}

fn print_comment(c: &RunComment, etag: Option<&str>, out: OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(c)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!("Comment: {}", c.id);
    println!("Author:  {}", c.author);
    println!("At:      {}", c.at);
    if let Some(e) = etag {
        println!("ETag:    {e}");
    }
    println!();
    println!("{}", c.text);
    Ok(())
}

fn ensure_comment_text(text: &str) -> Result<()> {
    let n = text.chars().count();
    if n == 0 {
        bail!("the comment text is empty");
    }
    if n > 2048 {
        bail!("the comment text has {n} characters; the limit is 2048");
    }
    Ok(())
}

/// A text argument: literal, `-` for stdin, or `@path` for a file. Trailing
/// whitespace is trimmed.
pub fn read_text(arg: &str) -> Result<String> {
    let raw = if arg == "-" {
        let mut s = String::new();
        std::io::stdin()
            .lock()
            .read_to_string(&mut s)
            .map_err(|e| anyhow::anyhow!("cannot read stdin: {e}"))?;
        s
    } else if let Some(path) = arg.strip_prefix('@') {
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("cannot read {path}: {e}"))?
    } else {
        arg.to_string()
    };
    Ok(raw.trim_end().to_string())
}

async fn tag(ctx: &Context, id: &str, tags: &[String], out: OutputArgs) -> Result<()> {
    ensure_guid("run id", id)?;
    for t in tags {
        ensure_guid("tag id", t)?;
    }
    let result: RunTags = ctx
        .client()?
        .post_json(
            &format!("v2/runs/{id}/tags"),
            &TagsWrite { tags },
            &WriteOptions::idempotent(),
        )
        .await?
        .json()?;
    print_tags(&result, out)
}

async fn untag(ctx: &Context, id: &str, tag: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("run id", id)?;
    ensure_guid("tag id", tag)?;
    let result: RunTags = ctx
        .client()?
        .delete(
            &format!("v2/runs/{id}/tags/{tag}"),
            &WriteOptions::default(),
        )
        .await?
        .json()?;
    print_tags(&result, out)
}

fn print_tags(result: &RunTags, out: OutputArgs) -> Result<()> {
    out.print(result, "The run has no tags", || {
        let mut t = Table::new(["TAG ID", "NAME"]);
        for tag in &result.tags {
            t.row([tag.id.clone(), tag.name.clone()]);
        }
        t
    })
}

async fn diagnosis(ctx: &Context, id: &str, opale: bool, out: OutputArgs) -> Result<()> {
    ensure_guid("run id", id)?;
    let query = [("opale", opale.to_string())];
    let d: Diagnosis = ctx
        .client()?
        .get_with(&format!("v2/runs/{id}/diagnosis"), &query)
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&d);
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
    let target = output.unwrap_or_else(|| format!("{id}.{}", kind.extension()));
    super::download_to(&ctx.client()?, &path, &target).await
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
    if !out.wants_json()
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
    if out.wants_json() {
        return out.print_json(&run);
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
        print_failure(f);
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

/// The failure block of a run or an incident.
pub fn print_failure(f: &runs::RunFailure) {
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
    fn run_request_body_is_validated() {
        let b = build_run_request(&["USER=bob".into()], Some(" smoke ".into())).unwrap();
        assert_eq!(b.variables["USER"], "bob");
        assert_eq!(b.comment.as_deref(), Some("smoke"));
        assert!(build_run_request(&["novalue".into()], None).is_err());
        assert!(build_run_request(&["1bad=x".into()], None).is_err());
        assert!(build_run_request(&["SCHEDULER_X=1".into()], None).is_err());
        assert!(build_run_request(&["A=b\u{7}".into()], None).is_err());
        assert!(build_run_request(&[], Some("x".repeat(501))).is_err());
        let many: Vec<String> = (0..21).map(|i| format!("V{i}=1")).collect();
        assert!(build_run_request(&many, None).is_err());
    }

    #[test]
    fn durations_are_human() {
        assert_eq!(duration(Some(950)), "950ms");
        assert_eq!(duration(Some(1234)), "1.2s");
        assert_eq!(duration(None), "-");
    }
}
