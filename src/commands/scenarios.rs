//! `2bec scenarios`: the catalog of scenarios and their groups.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::runs::ensure_guid;
use crate::api::page::{Fetch, fetch_items};
use crate::api::scenarios::{self, Scenario, ScenarioListItem};
use crate::output::{OutputArgs, Table, or_dash};

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
    }
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
    if !out.json
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
    if out.json {
        return crate::output::print_json(&s);
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
