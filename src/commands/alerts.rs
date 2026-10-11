//! `2bec alerts`: alert configurations, who receives them, what holds them,
//! and what was actually sent.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::runs::ensure_guid;
use crate::api::alerts::{
    self, Alert, Channel, Group, GroupWrite, Notification, RecipientEntry, RecipientMuted,
    TestSent, TestWrite,
};
use crate::api::page::{Fetch, Page, fetch_items};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
pub struct AlertsArgs {
    #[command(subcommand)]
    command: AlertsCommand,
}

#[derive(Subcommand, Debug)]
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
    /// Create an alert from a JSON body (needs alerts:write)
    #[command(after_help = "\
BODY is JSON: a literal, `-` for stdin, or `@file`. Shape (see the API docs
for POST /v2/alerts):
  {\"name\": \"Checkout down\", \"group\": \"<group id>\",
   \"failure\": {\"scenarios\": [\"<scenario id>\"], \"ranks\": [1, 3],
               \"notifyRestoration\": true}}
Find group ids with `2bec alerts groups list`. The request carries an
Idempotency-Key, so a retried call does not create twice.")]
    Create {
        /// JSON body; `-` for stdin, `@file` for a file
        body: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change an alert with a JSON merge patch (needs alerts:write)
    #[command(after_help = "\
Examples:
  2bec alerts edit 7f3c... '{\"active\": false}'
  2bec alerts edit 7f3c... @patch.json --if-match \"$(2bec alerts get 7f3c... --jq .etag)\"

A present member replaces, an absent one stays; `failure` and `responseTime`
merge member by member (null removes the rule); `social` and `quietPeriods`
are replaced whole.")]
    Edit {
        /// Alert id
        id: String,
        /// JSON merge patch; `-` for stdin, `@file` for a file
        body: String,
        /// Only update if the alert still has this ETag (from `alerts get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete an alert; its recipient group stays (needs alerts:write)
    #[command(alias = "rm")]
    Delete {
        /// Alert id
        id: String,
        /// Only delete if the alert still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
    /// Copy an alert, inactive, as a starting point to edit (needs alerts:write)
    Duplicate {
        /// Alert id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Really send a test alert to a recipient or to e-mail addresses (needs alerts:write)
    #[command(after_help = "\
Examples:
  2bec alerts test --channel sms --recipient 1a2b...
  2bec alerts test --channel mail --mail ops@example.com --mail oncall@example.com
  2bec alerts test --channel mail --recipient 1a2b... --message \"drill\"

Channels: mail, sms, voice, signal. Free addresses (--mail, 5 at most) work
on the mail channel only; a recipient id comes from `alerts recipients list`.")]
    Test {
        /// Channel to send through
        #[arg(long, value_name = "CHANNEL")]
        channel: String,
        /// Recipient id, a subscriber of the space
        #[arg(long, value_name = "ID")]
        recipient: Option<String>,
        /// Free e-mail address (repeatable, mail channel only)
        #[arg(long = "mail", value_name = "ADDRESS")]
        mails: Vec<String>,
        /// A line added to the test message
        #[arg(long, value_name = "TEXT")]
        message: Option<String>,
        /// Subject template to try on the mail channel
        #[arg(long, value_name = "TEMPLATE")]
        mail_subject: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// The recipient groups the alerts are sent to
    Groups(GroupsArgs),
    /// The subscribers alerts can reach, and their mute switch
    Recipients(RecipientsArgs),
}

#[derive(Args, Debug)]
pub struct GroupsArgs {
    #[command(subcommand)]
    command: GroupsCommand,
}

#[derive(Subcommand, Debug)]
enum GroupsCommand {
    /// List the recipient groups: channel, members, how many alerts use each
    List {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one group, its members and its ETag
    Get {
        /// Group id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Create a group on a channel with its members (needs alerts:write)
    #[command(after_help = "\
Examples:
  2bec alerts groups create --name \"On call\" --channel sms --member 1a2b... --member 3c4d...
  2bec alerts groups create --name Ops --channel hook --hook-type slack --member 1a2b...

Channels and hook types: `2bec alerts channels`. Member ids: `2bec alerts
recipients list`. At most 200 members.")]
    Create {
        /// Group name
        #[arg(long, value_name = "NAME")]
        name: String,
        /// Channel: mail, sms, voice, signal or hook
        #[arg(long, value_name = "CHANNEL")]
        channel: String,
        /// Kind of hook, for the hook channel
        #[arg(long, value_name = "TYPE")]
        hook_type: Option<String>,
        /// Recipient id (repeatable)
        #[arg(long = "member", value_name = "ID", required = true)]
        members: Vec<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change a group with a JSON merge patch (needs alerts:write)
    #[command(after_help = "\
Examples:
  2bec alerts groups edit 7f3c... '{\"name\": \"On call (night)\"}'
  2bec alerts groups edit 7f3c... '{\"members\": [\"1a2b...\", \"3c4d...\"]}' --if-match W/\"3\"

`members` is the whole list. The alerts sending to the group follow it.")]
    Edit {
        /// Group id
        id: String,
        /// JSON merge patch; `-` for stdin, `@file` for a file
        body: String,
        /// Only update if the group still has this ETag (from `groups get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete a group no alert sends to any more (needs alerts:write)
    #[command(alias = "rm")]
    Delete {
        /// Group id
        id: String,
        /// Only delete if the group still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct RecipientsArgs {
    #[command(subcommand)]
    command: RecipientsCommand,
}

#[derive(Subcommand, Debug)]
enum RecipientsCommand {
    /// List the subscribers of the space: contacts on file, mute state, groups
    List {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Hold every alert to a person until unmuted (needs alerts:write)
    Mute {
        /// Recipient id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Let the alerts reach a person again (needs alerts:write)
    Unmute {
        /// Recipient id
        id: String,
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
        AlertsCommand::Create { body, output } => create(ctx, &body, output).await,
        AlertsCommand::Edit {
            id,
            body,
            if_match,
            output,
        } => edit(ctx, &id, &body, if_match, output).await,
        AlertsCommand::Delete { id, if_match } => delete(ctx, &id, if_match).await,
        AlertsCommand::Duplicate { id, output } => duplicate(ctx, &id, output).await,
        AlertsCommand::Test {
            channel,
            recipient,
            mails,
            message,
            mail_subject,
            output,
        } => {
            let body = build_test(&channel, recipient, mails, message, mail_subject)?;
            test(ctx, &body, output).await
        }
        AlertsCommand::Groups(args) => groups(ctx, args.command).await,
        AlertsCommand::Recipients(args) => recipients(ctx, args.command).await,
    }
}

async fn groups(ctx: &Context, cmd: GroupsCommand) -> Result<()> {
    match cmd {
        GroupsCommand::List { output } => {
            let page: Page<Group> = ctx.client()?.get("v2/alert-groups").await?.json()?;
            output.print(&page.items, "No recipient group", || {
                let mut t =
                    Table::new(["ID", "NAME", "CHANNEL", "DELIVERABLE", "MEMBERS", "ALERTS"]);
                for g in &page.items {
                    t.row([
                        g.id.clone(),
                        g.name.clone(),
                        channel_label(&g.channel, g.hook_type.as_deref()),
                        yes_no(g.deliverable).to_string(),
                        g.members.len().to_string(),
                        g.alerts.to_string(),
                    ]);
                }
                t
            })
        }
        GroupsCommand::Get { id, output } => {
            ensure_guid("group id", &id)?;
            let resp = ctx.client()?.get(&format!("v2/alert-groups/{id}")).await?;
            let g: Group = resp.json()?;
            print_group(&g, resp.etag.as_deref(), &output)
        }
        GroupsCommand::Create {
            name,
            channel,
            hook_type,
            members,
            output,
        } => {
            let body = build_group(name, &channel, hook_type, members)?;
            let resp = ctx
                .client()?
                .post_json("v2/alert-groups", &body, &WriteOptions::idempotent())
                .await?;
            let g: Group = resp.json()?;
            if !output.wants_json() {
                eprintln!("Created group {} ({})", g.name, g.id);
            }
            print_group(&g, resp.etag.as_deref(), &output)
        }
        GroupsCommand::Edit {
            id,
            body,
            if_match,
            output,
        } => {
            ensure_guid("group id", &id)?;
            let body = super::read_json_body(&body)?;
            let write = WriteOptions {
                idempotency_key: None,
                if_match,
            };
            let resp = ctx
                .client()?
                .patch_merge(&format!("v2/alert-groups/{id}"), &body, &write)
                .await?;
            let g: Group = resp.json()?;
            print_group(&g, resp.etag.as_deref(), &output)
        }
        GroupsCommand::Delete { id, if_match } => {
            ensure_guid("group id", &id)?;
            let write = WriteOptions {
                idempotency_key: None,
                if_match,
            };
            ctx.client()?
                .delete(&format!("v2/alert-groups/{id}"), &write)
                .await?;
            eprintln!("Deleted group {id}");
            Ok(())
        }
    }
}

fn channel_label(channel: &str, hook_type: Option<&str>) -> String {
    match hook_type {
        Some(h) => format!("{channel} ({h})"),
        None => channel.to_string(),
    }
}

/// Validate the group flags the way the API does, before sending.
fn build_group(
    name: String,
    channel: &str,
    hook_type: Option<String>,
    members: Vec<String>,
) -> Result<GroupWrite> {
    let name = name.trim().to_string();
    if name.is_empty() {
        bail!("--name is empty");
    }
    let channel = canonical(
        "--channel",
        channel,
        &["mail", "sms", "voice", "signal", "hook"],
    )?;
    if channel == "hook" && hook_type.is_none() {
        bail!("--hook-type is needed on the hook channel (see `2bec alerts channels`)");
    }
    if channel != "hook" && hook_type.is_some() {
        bail!("--hook-type only applies to the hook channel");
    }
    if members.len() > 200 {
        bail!("at most 200 --member values");
    }
    for m in &members {
        ensure_guid("--member", m)?;
    }
    Ok(GroupWrite {
        name,
        channel,
        hook_type,
        members,
    })
}

fn print_group(g: &Group, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(g)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!("Group:       {}", g.id);
    println!("Name:        {}", g.name);
    println!(
        "Channel:     {}",
        channel_label(&g.channel, g.hook_type.as_deref())
    );
    println!("Deliverable: {}", yes_no(g.deliverable));
    println!("Alerts:      {} send to this group", g.alerts);
    if let Some(e) = etag {
        println!("ETag:        {e}");
    }
    if !g.members.is_empty() {
        println!();
        let mut t = Table::new(["MEMBER ID", "NAME", "MUTED"]);
        for m in &g.members {
            t.row([
                m.id.clone(),
                m.name.clone(),
                if m.muted { "yes" } else { "" }.to_string(),
            ]);
        }
        t.print();
    }
    Ok(())
}

async fn recipients(ctx: &Context, cmd: RecipientsCommand) -> Result<()> {
    match cmd {
        RecipientsCommand::List { output } => {
            let page: Page<RecipientEntry> =
                ctx.client()?.get("v2/alert-recipients").await?.json()?;
            output.print(&page.items, "No subscriber in the space", || {
                let mut t =
                    Table::new(["ID", "NAME", "LOGIN", "MAIL", "MOBILE", "MUTED", "GROUPS"]);
                for r in &page.items {
                    t.row([
                        r.id.clone(),
                        r.name.clone(),
                        r.login.clone(),
                        yes_no(r.has_mail).to_string(),
                        yes_no(r.has_mobile).to_string(),
                        if r.muted { "yes" } else { "" }.to_string(),
                        r.groups
                            .iter()
                            .map(|g| g.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ]);
                }
                t
            })
        }
        RecipientsCommand::Mute { id, output } => set_muted(ctx, &id, true, output).await,
        RecipientsCommand::Unmute { id, output } => set_muted(ctx, &id, false, output).await,
    }
}

async fn set_muted(ctx: &Context, id: &str, muted: bool, out: OutputArgs) -> Result<()> {
    ensure_guid("recipient id", id)?;
    let action = if muted { "mute" } else { "unmute" };
    let r: RecipientMuted = ctx
        .client()?
        .post_json(
            &format!("v2/alert-recipients/{id}:{action}"),
            &serde_json::json!({}),
            &WriteOptions::idempotent(),
        )
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&r);
    }
    println!(
        "{} is {}",
        r.name,
        if r.muted {
            "muted: no alert reaches them until unmuted"
        } else {
            "unmuted: alerts reach them again"
        }
    );
    Ok(())
}

async fn create(ctx: &Context, body: &str, out: OutputArgs) -> Result<()> {
    let body = super::read_json_body(body)?;
    let resp = ctx
        .client()?
        .post_json("v2/alerts", &body, &WriteOptions::idempotent())
        .await?;
    let a: Alert = resp.json()?;
    if !out.wants_json() {
        eprintln!("Created alert {} ({})", a.name, a.id);
    }
    print_alert(&a, resp.etag.as_deref(), &out)
}

async fn edit(
    ctx: &Context,
    id: &str,
    body: &str,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("alert id", id)?;
    let body = super::read_json_body(body)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .patch_merge(&format!("v2/alerts/{id}"), &body, &write)
        .await?;
    let a: Alert = resp.json()?;
    print_alert(&a, resp.etag.as_deref(), &out)
}

async fn delete(ctx: &Context, id: &str, if_match: Option<String>) -> Result<()> {
    ensure_guid("alert id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    ctx.client()?
        .delete(&format!("v2/alerts/{id}"), &write)
        .await?;
    eprintln!("Deleted alert {id}");
    Ok(())
}

async fn duplicate(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("alert id", id)?;
    let resp = ctx
        .client()?
        .post_json(
            &format!("v2/alerts/{id}:duplicate"),
            &serde_json::json!({}),
            &WriteOptions::idempotent(),
        )
        .await?;
    let a: Alert = resp.json()?;
    if !out.wants_json() {
        eprintln!("Created alert {} ({}), inactive", a.name, a.id);
    }
    print_alert(&a, resp.etag.as_deref(), &out)
}

/// Validate the test flags the way the API does, before sending.
fn build_test(
    channel: &str,
    recipient: Option<String>,
    mails: Vec<String>,
    message: Option<String>,
    mail_subject: Option<String>,
) -> Result<TestWrite> {
    let channel = canonical("--channel", channel, &alerts::TEST_CHANNELS)?;
    if let Some(r) = &recipient {
        ensure_guid("--recipient", r)?;
    }
    if recipient.is_none() && mails.is_empty() {
        bail!("give --recipient and/or --mail");
    }
    if !mails.is_empty() && channel != "mail" {
        bail!("--mail works on the mail channel only");
    }
    if mails.len() > 5 {
        bail!("at most 5 --mail addresses");
    }
    for m in &mails {
        if !m.contains('@') || m.contains(char::is_whitespace) {
            bail!("--mail `{m}` is not an e-mail address");
        }
    }
    Ok(TestWrite {
        channel,
        recipient,
        mails,
        message: message.filter(|m| !m.trim().is_empty()),
        mail_subject: mail_subject.filter(|m| !m.trim().is_empty()),
    })
}

async fn test(ctx: &Context, body: &TestWrite, out: OutputArgs) -> Result<()> {
    let sent: TestSent = ctx
        .client()?
        .post_json("v2/alerts:test", body, &WriteOptions::idempotent())
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&sent);
    }
    println!(
        "Sent {} test alert{} on {} to: {}",
        sent.sent,
        if sent.sent == 1 { "" } else { "s" },
        sent.channel,
        if sent.recipients.is_empty() {
            "-".to_string()
        } else {
            sent.recipients.join(", ")
        }
    );
    Ok(())
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

    #[test]
    fn test_body_is_validated() {
        let b = build_test("MAIL", None, vec!["a@b.c".into()], Some(" ".into()), None).unwrap();
        assert_eq!(b.channel, "mail");
        assert!(b.message.is_none());
        assert!(build_test("sms", None, vec![], None, None).is_err());
        assert!(build_test("sms", Some(G.into()), vec!["a@b.c".into()], None, None).is_err());
        assert!(build_test("mail", None, vec!["nope".into()], None, None).is_err());
        assert!(build_test("fax", Some(G.into()), vec![], None, None).is_err());
        let many: Vec<String> = (0..6).map(|i| format!("a{i}@b.c")).collect();
        assert!(build_test("mail", None, many, None, None).is_err());
    }

    #[test]
    fn group_body_is_validated() {
        let g = build_group(" Ops ".into(), "SMS", None, vec![G.into()]).unwrap();
        assert_eq!((g.name.as_str(), g.channel.as_str()), ("Ops", "sms"));
        assert!(build_group("x".into(), "hook", None, vec![G.into()]).is_err());
        assert!(build_group("x".into(), "mail", Some("slack".into()), vec![G.into()]).is_err());
        assert!(build_group("x".into(), "mail", None, vec!["nope".into()]).is_err());
        assert!(build_group(" ".into(), "mail", None, vec![G.into()]).is_err());
    }
}
