//! `2bec webhooks`: URLs of the space's own tooling subscribed to events,
//! their signing secret and their deliveries log.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::alerts::canonical;
use super::runs::ensure_guid;
use crate::api::page::{Fetch, Page, fetch_items};
use crate::api::webhooks::{
    self, Delivery, DeliveryDetail, EventKind, Webhook, WebhookSecret, WebhookWrite,
};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
#[command(after_help = "\
Every webhook command needs the scope webhooks:write. A webhook reads with
the key that created it: that key's access profile decides the scenarios the
events cover and its scopes the events it may carry (run.* and incident.*
need runs:read, alert.sent alerts:read, interruption.* and pause.*
planning:read, report.delivered reports:read).")]
pub struct WebhooksArgs {
    #[command(subcommand)]
    command: WebhooksCommand,
}

#[derive(Subcommand, Debug)]
enum WebhooksCommand {
    /// List the webhooks of the space, newest first, with their state and last delivery
    List {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one webhook and its ETag (the secret is never shown again)
    Get {
        /// Webhook id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// List the kinds of event a webhook subscribes to and the scope each needs
    Events {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Subscribe an https URL to events; prints the signing secret, shown this once
    #[command(after_help = "\
Examples:
  2bec webhooks create --url https://ops.example.com/2be --event run.failed --event run.recovered
  2bec webhooks create --url https://… --event alert.sent --description \"PagerDuty bridge\" --json

The URL must be https on a public host; the endpoint answers any 2xx within
10 seconds. Ten webhooks per space (409 limit-reached). Keep the secret: it
is the HMAC-SHA256 key of the X-Pegas-Signature header.")]
    Create {
        /// The https URL to deliver to
        #[arg(long, value_name = "URL")]
        url: String,
        /// Event kind to subscribe (repeatable; see `webhooks events`)
        #[arg(long = "event", value_name = "KIND", required = true)]
        events: Vec<String>,
        /// A note on what the webhook feeds (400 characters at most)
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
        /// Create it switched off; `edit --active true` starts the deliveries
        #[arg(long)]
        inactive: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change the URL, the events (the whole list), the description or the switch
    #[command(after_help = "\
Examples:
  2bec webhooks edit 7f3c... --active false
  2bec webhooks edit 7f3c... --event run.failed --event incident.opened --if-match \"$(2bec webhooks get 7f3c... --jq .etag)\"
  2bec webhooks edit 7f3c... --description \"\"        # clears the description

Only the flags given change (a JSON merge patch). Switching a disabled
webhook back on clears the platform's disablement and resumes from now.")]
    Edit {
        /// Webhook id
        id: String,
        /// New https URL
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        /// New list of event kinds (repeatable; replaces the whole list)
        #[arg(long = "event", value_name = "KIND")]
        events: Vec<String>,
        /// New description; an empty string clears it
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
        /// Switch the deliveries on (true) or off (false)
        #[arg(long, value_name = "BOOL")]
        active: Option<bool>,
        /// Only update if the webhook still has this ETag (from `webhooks get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete a webhook and its deliveries log
    #[command(alias = "rm")]
    Delete {
        /// Webhook id
        id: String,
        /// Only delete if the webhook still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
    /// Issue a new signing secret, shown this once; the old one signs too for 24 hours
    #[command(alias = "rotate")]
    RotateSecret {
        /// Webhook id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Send a `ping` event to the URL now and show what the endpoint answered
    Ping {
        /// Webhook id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// The events sent to a webhook over the last 30 days
    Deliveries(DeliveriesArgs),
}

#[derive(Args, Debug)]
pub struct DeliveriesArgs {
    #[command(subcommand)]
    command: DeliveriesCommand,
}

#[derive(Subcommand, Debug)]
enum DeliveriesCommand {
    /// List the deliveries, newest first
    #[command(after_help = "\
Examples:
  2bec webhooks deliveries list 7f3c...
  2bec webhooks deliveries list 7f3c... --status failed --all --json")]
    List {
        /// Webhook id
        id: String,
        /// Only deliveries in this state: pending, delivered or failed
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
        /// Maximum number of deliveries to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every delivery, following pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one delivery with the body as it was sent
    Get {
        /// Webhook id
        id: String,
        /// Delivery id
        delivery: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Queue a delivery again, the same body with a fresh signature
    Redeliver {
        /// Webhook id
        id: String,
        /// Delivery id
        delivery: String,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: WebhooksArgs) -> Result<()> {
    match args.command {
        WebhooksCommand::List { output } => list(ctx, output).await,
        WebhooksCommand::Get { id, output } => get(ctx, &id, output).await,
        WebhooksCommand::Events { output } => events(ctx, output).await,
        WebhooksCommand::Create {
            url,
            events,
            description,
            inactive,
            output,
        } => create(ctx, url, events, description, !inactive, output).await,
        WebhooksCommand::Edit {
            id,
            url,
            events,
            description,
            active,
            if_match,
            output,
        } => edit(ctx, &id, url, events, description, active, if_match, output).await,
        WebhooksCommand::Delete { id, if_match } => delete(ctx, &id, if_match).await,
        WebhooksCommand::RotateSecret { id, output } => rotate(ctx, &id, output).await,
        WebhooksCommand::Ping { id, output } => ping(ctx, &id, output).await,
        WebhooksCommand::Deliveries(args) => deliveries(ctx, args.command).await,
    }
}

async fn list(ctx: &Context, out: OutputArgs) -> Result<()> {
    let page: Page<Webhook> = ctx.client()?.get("v2/webhooks").await?.json()?;
    out.print(&page.items, "No webhook in the space", || {
        let mut t = Table::new([
            "ID",
            "URL",
            "STATE",
            "EVENTS",
            "KEY",
            "LAST DELIVERY",
            "STATUS",
        ]);
        for w in &page.items {
            t.row([
                w.id.clone(),
                w.url.clone(),
                state_label(w),
                w.events.join(","),
                w.api_key.name.clone(),
                w.last_delivery_at
                    .as_deref()
                    .map_or("-".to_string(), short_time),
                w.last_delivery_status.map_or("-".to_string(), http_status),
            ]);
        }
        t
    })
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("webhook id", id)?;
    let resp = ctx.client()?.get(&format!("v2/webhooks/{id}")).await?;
    let w: Webhook = resp.json()?;
    print_webhook(&w, resp.etag.as_deref(), None, &out)
}

async fn events(ctx: &Context, out: OutputArgs) -> Result<()> {
    let page: Page<EventKind> = ctx.client()?.get("v2/webhook-events").await?.json()?;
    out.print(&page.items, "No event kind", || {
        let mut t = Table::new(["EVENT", "SCOPE", "DESCRIPTION"]);
        for e in &page.items {
            t.row([e.code.clone(), e.scope.clone(), e.description.clone()]);
        }
        t
    })
}

async fn create(
    ctx: &Context,
    url: String,
    events: Vec<String>,
    description: Option<String>,
    active: bool,
    out: OutputArgs,
) -> Result<()> {
    let body = WebhookWrite {
        url: check_url(&url)?,
        events: check_events(&events)?,
        description: check_description(description)?,
        active,
    };
    let resp = ctx
        .client()?
        .post_json("v2/webhooks", &body, &WriteOptions::idempotent())
        .await?;
    let created: WebhookSecret = resp.json()?;
    print_webhook(
        &created.webhook,
        resp.etag.as_deref(),
        Some(&created.secret),
        &out,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "one flag per field of the merge patch"
)]
async fn edit(
    ctx: &Context,
    id: &str,
    url: Option<String>,
    events: Vec<String>,
    description: Option<String>,
    active: Option<bool>,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("webhook id", id)?;
    let mut patch = serde_json::Map::new();
    if let Some(u) = url {
        patch.insert("url".into(), check_url(&u)?.into());
    }
    if !events.is_empty() {
        patch.insert("events".into(), check_events(&events)?.into());
    }
    if let Some(d) = description {
        patch.insert(
            "description".into(),
            match check_description(Some(d))? {
                Some(d) => d.into(),
                None => serde_json::Value::Null,
            },
        );
    }
    if let Some(a) = active {
        patch.insert("active".into(), a.into());
    }
    if patch.is_empty() {
        bail!("nothing to change: give --url, --event, --description or --active");
    }
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .patch_merge(&format!("v2/webhooks/{id}"), &patch, &write)
        .await?;
    let w: Webhook = resp.json()?;
    print_webhook(&w, resp.etag.as_deref(), None, &out)
}

async fn delete(ctx: &Context, id: &str, if_match: Option<String>) -> Result<()> {
    ensure_guid("webhook id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    ctx.client()?
        .delete(&format!("v2/webhooks/{id}"), &write)
        .await?;
    eprintln!("Deleted webhook {id}");
    Ok(())
}

async fn rotate(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("webhook id", id)?;
    let resp = ctx
        .client()?
        .post_json(
            &format!("v2/webhooks/{id}:rotate-secret"),
            &serde_json::json!({}),
            &WriteOptions::idempotent(),
        )
        .await?;
    let rotated: WebhookSecret = resp.json()?;
    print_webhook(
        &rotated.webhook,
        resp.etag.as_deref(),
        Some(&rotated.secret),
        &out,
    )
}

async fn ping(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("webhook id", id)?;
    let d: Delivery = ctx
        .client()?
        .post_json(
            &format!("v2/webhooks/{id}:ping"),
            &serde_json::json!({}),
            &WriteOptions::idempotent(),
        )
        .await?
        .json()?;
    print_delivery(&d, &out)
}

async fn deliveries(ctx: &Context, cmd: DeliveriesCommand) -> Result<()> {
    match cmd {
        DeliveriesCommand::List {
            id,
            status,
            limit,
            all,
            output,
        } => {
            ensure_guid("webhook id", &id)?;
            if !all && limit == 0 {
                bail!("--limit must be at least 1");
            }
            let mut query = Vec::new();
            if let Some(s) = status {
                query.push((
                    "status",
                    canonical("--status", &s, &webhooks::DELIVERY_STATUSES)?,
                ));
            }
            let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
            let page = fetch_items::<Delivery>(
                &ctx.client()?,
                &format!("v2/webhooks/{id}/deliveries"),
                &query,
                fetch,
            )
            .await?;
            output.print(&page.items, "No delivery in the last 30 days", || {
                delivery_table(&page.items)
            })?;
            if !output.wants_json()
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
        DeliveriesCommand::Get {
            id,
            delivery,
            output,
        } => {
            ensure_guid("webhook id", &id)?;
            ensure_guid("delivery id", &delivery)?;
            let d: DeliveryDetail = ctx
                .client()?
                .get(&format!("v2/webhooks/{id}/deliveries/{delivery}"))
                .await?
                .json()?;
            if output.wants_json() {
                return output.print_json(&d);
            }
            print_delivery(&d.delivery, &output)?;
            println!();
            println!("Body:");
            println!("{}", serde_json::to_string_pretty(&d.body)?);
            Ok(())
        }
        DeliveriesCommand::Redeliver {
            id,
            delivery,
            output,
        } => {
            ensure_guid("webhook id", &id)?;
            ensure_guid("delivery id", &delivery)?;
            let d: Delivery = ctx
                .client()?
                .post_json(
                    &format!("v2/webhooks/{id}/deliveries/{delivery}:redeliver"),
                    &serde_json::json!({}),
                    &WriteOptions::idempotent(),
                )
                .await?
                .json()?;
            if !output.wants_json() {
                eprintln!("Queued delivery {delivery} again");
            }
            print_delivery(&d, &output)
        }
    }
}

// ---------- validation, as the API does it ----------

/// An https URL on a public host, 400 characters at most.
fn check_url(url: &str) -> Result<String> {
    let url = url.trim();
    if url.is_empty() {
        bail!("--url is empty");
    }
    if url.len() > webhooks::MAX_TEXT {
        bail!("--url is {} characters at most", webhooks::MAX_TEXT);
    }
    let Some(rest) = url.strip_prefix("https://") else {
        bail!("--url must use https");
    };
    let authority = rest.split(['/', '?']).next().unwrap_or("");
    if authority.contains('@') {
        bail!("--url must not carry credentials");
    }
    if url.contains('#') {
        bail!("--url must not carry a fragment");
    }
    let host = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
        .map_or(authority, |(h, _)| h);
    if host.is_empty() {
        bail!("--url has no host");
    }
    if host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".local")
        || !host.contains('.')
    {
        bail!("--url must reach a public host");
    }
    Ok(url.to_string())
}

/// Known event kinds, deduplicated, in the catalog's order.
fn check_events(events: &[String]) -> Result<Vec<String>> {
    if events.is_empty() {
        bail!(
            "at least one --event is required: {}",
            webhooks::EVENTS.join(", ")
        );
    }
    let mut known = Vec::new();
    for e in events {
        let e = e.trim();
        if !webhooks::EVENTS.contains(&e) {
            bail!(
                "--event `{e}` is not one of {}",
                webhooks::EVENTS.join(", ")
            );
        }
        if !known.contains(&e) {
            known.push(e);
        }
    }
    Ok(webhooks::EVENTS
        .iter()
        .filter(|e| known.contains(e))
        .map(|e| (*e).to_string())
        .collect())
}

/// Trimmed, 400 characters at most; empty means none.
fn check_description(description: Option<String>) -> Result<Option<String>> {
    match description {
        Some(d) => {
            let d = d.trim().to_string();
            if d.chars().count() > webhooks::MAX_TEXT {
                bail!("--description is {} characters at most", webhooks::MAX_TEXT);
            }
            Ok(if d.is_empty() { None } else { Some(d) })
        }
        None => Ok(None),
    }
}

// ---------- output ----------

fn print_webhook(
    w: &Webhook,
    etag: Option<&str>,
    secret: Option<&str>,
    out: &OutputArgs,
) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(w)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        if let Some(s) = secret {
            v["secret"] = s.into();
        }
        return out.print_json(&v);
    }
    println!("Webhook:       {}", w.id);
    println!("URL:           {}", w.url);
    println!("State:         {}", state_label(w));
    println!("Events:        {}", w.events.join(", "));
    println!("Description:   {}", or_dash(w.description.as_deref()));
    println!("Reads with:    {} ({})", w.api_key.name, w.api_key.id);
    println!(
        "Last delivery: {}",
        match (&w.last_delivery_at, w.last_delivery_status) {
            (Some(at), Some(st)) => format!("{} ({})", short_time(at), http_status(st)),
            (Some(at), None) => short_time(at),
            _ => "-".into(),
        }
    );
    if let Some(until) = &w.previous_secret_until {
        println!("Old secret:    signs too until {}", short_time(until));
    }
    println!("Created:       {}", short_time(&w.created_at));
    println!(
        "Updated:       {}",
        w.updated_at.as_deref().map_or("-".to_string(), short_time)
    );
    if let Some(e) = etag {
        println!("ETag:          {e}");
    }
    if let Some(s) = secret {
        println!();
        println!("Secret:        {s}");
        eprintln!(
            "Keep the secret: it is shown this once. It signs X-Pegas-Signature (HMAC-SHA256)."
        );
    }
    Ok(())
}

fn print_delivery(d: &Delivery, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        return out.print_json(d);
    }
    println!("Delivery:      {}", d.id);
    println!("Event:         {}", d.event);
    println!("Occurred:      {}", short_time(&d.occurred_at));
    println!("Status:        {}", d.status);
    println!(
        "Attempts:      {}{}",
        d.attempts,
        d.next_attempt_at
            .as_deref()
            .map_or(String::new(), |n| format!(", next at {}", short_time(n)))
    );
    println!(
        "Last attempt:  {}",
        match (&d.last_attempt_at, d.response_status) {
            (Some(at), Some(st)) => format!(
                "{} answered {}{}",
                short_time(at),
                http_status(st),
                d.duration_ms
                    .map_or(String::new(), |ms| format!(" in {ms} ms"))
            ),
            (Some(at), None) => short_time(at),
            _ => "-".into(),
        }
    );
    if let Some(e) = &d.error {
        println!("Error:         {e}");
    }
    if let Some(at) = &d.delivered_at {
        println!("Delivered:     {}", short_time(at));
    }
    Ok(())
}

fn delivery_table(items: &[Delivery]) -> Table {
    let mut t = Table::new([
        "ID", "EVENT", "OCCURRED", "STATUS", "ATTEMPTS", "LAST", "ANSWER", "NEXT",
    ]);
    for d in items {
        t.row([
            d.id.clone(),
            d.event.clone(),
            short_time(&d.occurred_at),
            d.status.clone(),
            d.attempts.to_string(),
            d.last_attempt_at
                .as_deref()
                .map_or("-".to_string(), short_time),
            match (d.response_status, &d.error) {
                (Some(st), _) if st != 0 => http_status(st),
                (_, Some(e)) => e.clone(),
                _ => "-".into(),
            },
            d.next_attempt_at
                .as_deref()
                .map_or("-".to_string(), short_time),
        ]);
    }
    t
}

fn state_label(w: &Webhook) -> String {
    match (w.state.as_str(), &w.disabled_reason) {
        ("disabled", Some(r)) => format!("disabled ({r})"),
        (s, _) => s.to_string(),
    }
}

/// `0` means the endpoint could not be reached.
fn http_status(status: i64) -> String {
    if status == 0 {
        "unreachable".into()
    } else {
        status.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_follow_the_api_rules() {
        assert_eq!(
            check_url(" https://ops.example.com/hook?x=1 ").unwrap(),
            "https://ops.example.com/hook?x=1"
        );
        assert!(check_url("https://ops.example.com:8443/h").is_ok());
        assert!(
            check_url("http://ops.example.com")
                .unwrap_err()
                .to_string()
                .contains("https")
        );
        assert!(
            check_url("https://localhost/h")
                .unwrap_err()
                .to_string()
                .contains("public host")
        );
        assert!(
            check_url("https://intranet/h")
                .unwrap_err()
                .to_string()
                .contains("public host")
        );
        assert!(
            check_url("https://u:p@ops.example.com/h")
                .unwrap_err()
                .to_string()
                .contains("credentials")
        );
        assert!(
            check_url("https://ops.example.com/h#frag")
                .unwrap_err()
                .to_string()
                .contains("fragment")
        );
        assert!(
            check_url(&format!("https://e.com/{}", "a".repeat(400)))
                .unwrap_err()
                .to_string()
                .contains("400")
        );
    }

    #[test]
    fn events_are_checked_and_canonical() {
        assert_eq!(
            check_events(&[
                "run.recovered".into(),
                "run.failed".into(),
                "run.failed".into()
            ])
            .unwrap(),
            vec!["run.failed".to_string(), "run.recovered".to_string()]
        );
        assert!(
            check_events(&[])
                .unwrap_err()
                .to_string()
                .contains("at least one")
        );
        assert!(
            check_events(&["run.exploded".into()])
                .unwrap_err()
                .to_string()
                .contains("not one of")
        );
    }

    #[test]
    fn descriptions_are_trimmed_and_capped() {
        assert_eq!(check_description(Some("  ".into())).unwrap(), None);
        assert_eq!(
            check_description(Some(" x ".into())).unwrap(),
            Some("x".into())
        );
        assert!(check_description(Some("é".repeat(401))).is_err());
        assert!(check_description(Some("é".repeat(400))).is_ok());
    }
}
