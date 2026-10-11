//! `2bec alerts`: alert configurations, who receives them, what holds them,
//! and what was actually sent.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::runs::ensure_guid;
use crate::api::alerts::{self, Alert, Channel, Notification};
use crate::api::page::{Fetch, Page, fetch_items};
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
pub struct AlertsArgs {
    #[command(subcommand)]
    command: AlertsCommand,
}

#[derive(Subcommand, Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "clap builds the enum once; the notifications filter is the big one"
)]
enum AlertsCommand {
    /// List the alert configurations and what is wrong with them
    List {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one alert: rules, recipients, schedule, health, ETag
    Get {
        /// Alert id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// List the alerts actually sent, newest first (last 7 days by default)
    #[command(after_help = "\
Examples:
  2bec alerts notifications --period today
  2bec alerts notifications --scenario 7f3c... --channel sms --period last30d
  2bec alerts notifications --trigger restoration --all --json

Channels: mail, sms, voice, hook, signal, push, snmp, other.
Triggers: unavailability, responseTime, restoration, other.
Periods: last24h, last7d, last30d, today, yesterday, thisWeek, lastWeek,
thisMonth, lastMonth. A range spans at most 366 days.")]
    Notifications {
        /// Only alerts about this scenario id (repeatable)
        #[arg(long = "scenario", value_name = "ID")]
        scenarios: Vec<String>,
        /// Start of the range, RFC 3339 (inclusive)
        #[arg(long, value_name = "TIME", conflicts_with = "period")]
        from: Option<String>,
        /// End of the range, RFC 3339 (exclusive)
        #[arg(long, value_name = "TIME", conflicts_with = "period")]
        to: Option<String>,
        /// Named period instead of --from/--to (e.g. today, last30d)
        #[arg(long, value_name = "PERIOD")]
        period: Option<String>,
        /// IANA time zone for periods and day boundaries [default: Europe/Paris]
        #[arg(long, value_name = "ZONE")]
        tz: Option<String>,
        /// Only this channel
        #[arg(long, value_name = "CHANNEL")]
        channel: Option<String>,
        /// Only this trigger
        #[arg(long, value_name = "TRIGGER")]
        trigger: Option<String>,
        /// Only alerts of the configuration with this name
        #[arg(long, value_name = "NAME")]
        alert: Option<String>,
        /// Only alerts sent to the group with this name
        #[arg(long, value_name = "NAME")]
        group: Option<String>,
        /// Only rows matching these words
        #[arg(long, short = 'q', value_name = "WORDS")]
        search: Option<String>,
        /// Maximum number of rows to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every matching row, following pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// List the channels a recipient group can be sent through
    Channels {
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: AlertsArgs) -> Result<()> {
    match args.command {
        AlertsCommand::List { output } => list(ctx, output).await,
        AlertsCommand::Get { id, output } => get(ctx, &id, output).await,
        AlertsCommand::Notifications {
            scenarios,
            from,
            to,
            period,
            tz,
            channel,
            trigger,
            alert,
            group,
            search,
            limit,
            all,
            output,
        } => {
            let filter = NotificationFilter {
                scenarios,
                from,
                to,
                period,
                tz,
                channel,
                trigger,
                alert,
                group,
                search,
            };
            notifications(ctx, &filter, limit, all, output).await
        }
        AlertsCommand::Channels { output } => channels(ctx, output).await,
    }
}

/// Filters of `alerts notifications`.
#[derive(Debug, Default)]
struct NotificationFilter {
    scenarios: Vec<String>,
    from: Option<String>,
    to: Option<String>,
    period: Option<String>,
    tz: Option<String>,
    channel: Option<String>,
    trigger: Option<String>,
    alert: Option<String>,
    group: Option<String>,
    search: Option<String>,
}

impl NotificationFilter {
    fn query(&self) -> Result<Vec<(&'static str, String)>> {
        let mut q = Vec::new();
        for s in &self.scenarios {
            ensure_guid("--scenario", s)?;
            q.push(("scenario", s.clone()));
        }
        if let Some(p) = &self.period {
            q.push((
                "period",
                canonical("--period", p, &crate::api::runs::PERIODS)?,
            ));
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
        if let Some(c) = &self.channel {
            q.push(("channel", canonical("--channel", c, &alerts::CHANNELS)?));
        }
        if let Some(t) = &self.trigger {
            q.push(("trigger", canonical("--trigger", t, &alerts::TRIGGERS)?));
        }
        if let Some(a) = &self.alert {
            q.push(("alert", a.clone()));
        }
        if let Some(g) = &self.group {
            q.push(("group", g.clone()));
        }
        if let Some(s) = &self.search
            && !s.trim().is_empty()
        {
            q.push(("q", s.trim().to_string()));
        }
        Ok(q)
    }
}

/// The API's spelling of `value` among `allowed`, matched case-insensitively.
pub fn canonical(flag: &str, value: &str, allowed: &[&str]) -> Result<String> {
    allowed
        .iter()
        .find(|v| v.eq_ignore_ascii_case(value))
        .map(|v| (*v).to_string())
        .ok_or_else(|| anyhow::anyhow!("{flag} `{value}` is not one of {}", allowed.join(", ")))
}

async fn list(ctx: &Context, out: OutputArgs) -> Result<()> {
    let page: Page<Alert> = ctx.client()?.get("v2/alerts").await?.json()?;
    out.print(&page.items, "No alert configured", || {
        let mut t = Table::new([
            "ID", "NAME", "ACTIVE", "GROUP", "CHANNEL", "WATCHES", "ISSUES",
        ]);
        for a in &page.items {
            t.row([
                a.id.clone(),
                a.name.clone(),
                yes_no(a.active).to_string(),
                a.group.name.clone(),
                a.group.channel.clone(),
                watches(a),
                a.health.issues().join(", "),
            ]);
        }
        t
    })
}

/// One phrase for what an alert watches.
fn watches(a: &Alert) -> String {
    let mut parts = Vec::new();
    if let Some(f) = &a.failure {
        let n = f.steps.len() as i64 + f.hidden_steps;
        parts.push(format!("{n} step{}", if n == 1 { "" } else { "s" }));
    }
    if let Some(r) = &a.response_time {
        let n = r.scenarios.len() as i64 + r.hidden_scenarios;
        parts.push(format!("response time of {n}"));
    }
    if !a.social.is_empty() {
        parts.push(format!("{} social", a.social.len()));
    }
    if parts.is_empty() {
        "-".into()
    } else {
        parts.join(", ")
    }
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("alert id", id)?;
    let resp = ctx.client()?.get(&format!("v2/alerts/{id}")).await?;
    let a: Alert = resp.json()?;
    print_alert(&a, resp.etag.as_deref(), &out)
}

pub(super) fn print_alert(a: &Alert, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(a)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!("Alert:        {}", a.id);
    println!("Name:         {}", a.name);
    println!("Active:       {}", yes_no(a.active));
    println!(
        "Group:        {} ({}, {} member{}) {}",
        a.group.name,
        a.group.channel,
        a.group.members,
        if a.group.members == 1 { "" } else { "s" },
        a.group.id
    );
    println!("Schedule:     {}", schedule(a));
    println!("Mail subject: {}", or_dash(a.mail_subject.as_deref()));
    let issues = a.health.issues();
    println!(
        "Health:       {}",
        if issues.is_empty() {
            "ok".to_string()
        } else {
            issues.join(", ")
        }
    );
    if let Some(e) = etag {
        println!("ETag:         {e}");
    }
    if let Some(f) = &a.failure {
        println!();
        println!(
            "Failure rule: {}{}{}",
            match f.trigger_mode.as_str() {
                "every" => format!("every {} failures", f.every),
                _ => format!(
                    "at rank{} {}",
                    if f.ranks.len() == 1 { "" } else { "s" },
                    f.ranks
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            if f.notify_restoration {
                ", restoration notified"
            } else {
                ""
            },
            match f.tag_mode.as_str() {
                "only" => format!(", only tags {}", f.tags.join(", ")),
                "except" => format!(", except tags {}", f.tags.join(", ")),
                _ => String::new(),
            }
        );
        if a.extra_failure_rules > 0 {
            println!(
                "  {} more failure rule(s) not shown by the API",
                a.extra_failure_rules
            );
        }
        if !f.steps.is_empty() {
            let mut t = Table::new(["SCENARIO", "#", "STEP", "STATE"]);
            for s in &f.steps {
                let mut state = Vec::new();
                if s.suspended {
                    state.push("suspended");
                }
                if s.stale {
                    state.push("stale");
                }
                t.row([
                    s.scenario.name.clone(),
                    s.order.to_string(),
                    s.label.clone(),
                    state.join(", "),
                ]);
            }
            t.print();
        }
        if f.hidden_steps > 0 {
            println!("  {} step(s) of scenarios you do not see", f.hidden_steps);
        }
    }
    if let Some(r) = &a.response_time {
        println!();
        println!(
            "Response time rule: at rank{} {}{}",
            if r.ranks.len() == 1 { "" } else { "s" },
            r.ranks
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            if r.notify_restoration {
                ", restoration notified"
            } else {
                ""
            }
        );
        for s in &r.scenarios {
            println!("  {} ({})", s.name, s.id);
        }
        if r.hidden_scenarios > 0 {
            println!("  {} scenario(s) you do not see", r.hidden_scenarios);
        }
    }
    if !a.social.is_empty() {
        println!();
        let mut t = Table::new(["SOCIAL SCENARIO", "ENABLED", "TRIGGER", "WINDOW", "REPEAT"]);
        for s in &a.social {
            t.row([
                s.scenario.as_ref().map_or("-".into(), |x| x.name.clone()),
                yes_no(s.enabled).to_string(),
                format!("{} events", s.trigger_count),
                format!("{} min", s.watch_window_minutes),
                s.repetition_minutes
                    .map_or("-".into(), |m| format!("every {m} min")),
            ]);
        }
        t.print();
    }
    Ok(())
}

fn schedule(a: &Alert) -> String {
    let n = a.schedule.quiet_periods.len();
    match n {
        0 => format!("{} (may go out at any time)", a.schedule.kind),
        1 => format!("{}, 1 quiet period", a.schedule.kind),
        _ => format!("{}, {n} quiet periods", a.schedule.kind),
    }
}

async fn notifications(
    ctx: &Context,
    filter: &NotificationFilter,
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    let query = filter.query()?;
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page =
        fetch_items::<Notification>(&ctx.client()?, "v2/notifications", &query, fetch).await?;
    out.print(&page.items, "No alert sent in this period", || {
        let mut t = Table::new([
            "SENT",
            "SCENARIO",
            "TRIGGER",
            "CHANNEL",
            "ALERT",
            "GROUP",
            "RECIPIENTS",
        ]);
        for n in &page.items {
            t.row([
                short_time(&n.sent_at),
                n.scenario.name.clone(),
                n.trigger.clone(),
                n.channel.clone(),
                n.alert.clone(),
                n.group.clone(),
                n.recipients
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} notifications; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

async fn channels(ctx: &Context, out: OutputArgs) -> Result<()> {
    let page: Page<Channel> = ctx.client()?.get("v2/alert-channels").await?.json()?;
    out.print(&page.items, "No alert channel", || {
        let mut t = Table::new(["CHANNEL", "DELIVERABLE", "CREATABLE", "HOOK TYPES"]);
        for c in &page.items {
            t.row([
                c.channel.clone(),
                yes_no(c.deliverable).to_string(),
                yes_no(c.creatable).to_string(),
                c.hook_types.join(", "),
            ]);
        }
        t
    })
}

pub(super) fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "7f3c1a2b-0000-4000-8000-000000000000";

    #[test]
    fn notification_filter_is_canonical() {
        let f = NotificationFilter {
            scenarios: vec![G.into()],
            period: Some("TODAY".into()),
            channel: Some("SMS".into()),
            trigger: Some("responsetime".into()),
            search: Some(" login ".into()),
            ..NotificationFilter::default()
        };
        assert_eq!(
            f.query().unwrap(),
            vec![
                ("scenario", G.to_string()),
                ("period", "today".into()),
                ("channel", "sms".into()),
                ("trigger", "responseTime".into()),
                ("q", "login".into()),
            ]
        );
        let f = NotificationFilter {
            channel: Some("fax".into()),
            ..NotificationFilter::default()
        };
        assert!(f.query().unwrap_err().to_string().contains("--channel"));
    }
}
