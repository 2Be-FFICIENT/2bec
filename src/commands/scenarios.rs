//! `2bec scenarios`: the catalog of scenarios and their groups.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::runs::ensure_guid;
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
    /// Browse the groups scenarios are organized in
    Groups(GroupsArgs),
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
        ScenariosCommand::Groups(args) => groups(ctx, args.command).await,
    }
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

fn thresholds(t: &scenarios::Thresholds) -> String {
    format!(
        "target {}, client {}, unacceptable {}",
        ms(t.target),
        ms(t.client),
        ms(t.unacceptable)
    )
}

/// Milliseconds as `1.5s` or `800ms`.
fn ms(v: f64) -> String {
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
