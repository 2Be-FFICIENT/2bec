//! `2bec badges`: the glyphs drawn on the runs matching a rule over their
//! tags, and the badges of the space written.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::runs::ensure_guid;
use crate::api::indicators::{self, Badge, BadgeWrite};
use crate::api::page::Page;
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
#[command(after_help = "\
Reads need scenarios:read, writes indicators:write. A badge is a symbol
(one or two characters, or an icon key) in a palette color, drawn on the
runs carrying any or all of its tags, on failed, successful or every run,
within an optional validity window. The highest priority wins when several
badges match.")]
pub struct BadgesArgs {
    #[command(subcommand)]
    command: BadgesCommand,
}

/// The definition flags shared by `create` and `edit`.
#[derive(Args, Debug, Default)]
struct DefinitionArgs {
    /// Description (500 characters at most)
    #[arg(long, value_name = "TEXT")]
    description: Option<String>,
    /// Priority; the highest wins when several badges match (default 10)
    #[arg(long, value_name = "N")]
    priority: Option<i64>,
    /// Palette key 1..7
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(i64).range(1..=indicators::MAX_COLOR))]
    color: Option<i64>,
    /// The symbol: one or two characters, or an icon key with --icon
    #[arg(long, value_name = "SYMBOL")]
    symbol: Option<String>,
    /// The symbol is an icon key rather than text
    #[arg(long)]
    icon: bool,
    /// Tag of the rule (repeatable; at least one, replaces the whole list on an edit)
    #[arg(long = "tag", value_name = "ID")]
    tags: Vec<String>,
    /// Runs must carry all the tags, not any of them
    #[arg(long)]
    all_tags: bool,
    /// Which runs are badged
    #[arg(long, value_name = "STATUS", value_parser = ["any", "failed", "success"])]
    status: Option<String>,
    /// Start of the validity window (RFC 3339)
    #[arg(long, value_name = "INSTANT")]
    valid_from: Option<String>,
    /// End of the validity window (RFC 3339)
    #[arg(long, value_name = "INSTANT")]
    valid_until: Option<String>,
}

#[derive(Subcommand, Debug)]
enum BadgesCommand {
    /// List the badges in resolution order (priority, then name), disabled ones included
    #[command(after_help = "\
Examples:
  2bec badges list
  2bec badges list --enabled
  2bec badges list --json | jq -r '.[] | [.name, .symbol] | @tsv'")]
    List {
        /// Only the enabled badges
        #[arg(long, conflicts_with = "disabled")]
        enabled: bool,
        /// Only the disabled badges
        #[arg(long)]
        disabled: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one badge with its rule and ETag
    Get {
        /// Badge id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Create a badge
    #[command(after_help = "\
Examples:
  2bec badges create --name Prod --symbol P --tag <tag-id>
  2bec badges create --name Release --symbol rocket --icon --color 3 --tag <id> --tag <id> --all-tags --status any
  2bec badges create --name Freeze --symbol F --tag <id> --valid-from 2026-12-20T00:00:00Z --valid-until 2027-01-05T00:00:00Z --disabled

Defaults: enabled, priority 10, text symbol, any of the tags, failed runs.
Names are unique in the space (409 conflict). The palette and icon keys and
the tags' ownership are checked by the API.")]
    Create {
        /// Badge name (100 characters at most)
        #[arg(long, value_name = "NAME")]
        name: String,
        /// Create the badge switched off
        #[arg(long)]
        disabled: bool,
        #[command(flatten)]
        definition: DefinitionArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change a badge's definition
    #[command(after_help = "\
Examples:
  2bec badges edit <id> --enabled false
  2bec badges edit <id> --name Production --priority 20
  2bec badges edit <id> --tag <id> --tag <id>               # replaces the rule's tags
  2bec badges edit <id> --symbol bolt --icon
  2bec badges edit <id> --clear-valid-until --if-match '\"v3\"'

Only the flags given change (a JSON merge patch); the whole definition is
then checked again. --symbol switches the kind back to text unless --icon
is given too.")]
    Edit {
        /// Badge id
        id: String,
        /// New name
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// Switch the badge on (true) or off (false)
        #[arg(long, value_name = "BOOL")]
        enabled: Option<bool>,
        #[command(flatten)]
        definition: DefinitionArgs,
        /// Drop the description
        #[arg(long, conflicts_with = "description")]
        clear_description: bool,
        /// Drop the start of the validity window
        #[arg(long, conflicts_with = "valid_from")]
        clear_valid_from: bool,
        /// Drop the end of the validity window
        #[arg(long, conflicts_with = "valid_until")]
        clear_valid_until: bool,
        /// Only change if the badge still has this ETag (from `badges get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete a badge; the runs keep their tags
    #[command(alias = "rm")]
    Delete {
        /// Badge id
        id: String,
        /// Only delete if the badge still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
}

pub async fn run(ctx: &Context, args: BadgesArgs) -> Result<()> {
    match args.command {
        BadgesCommand::List {
            enabled,
            disabled,
            output,
        } => {
            let only = if enabled {
                Some(true)
            } else if disabled {
                Some(false)
            } else {
                None
            };
            list(ctx, only, output).await
        }
        BadgesCommand::Get { id, output } => get(ctx, &id, output).await,
        BadgesCommand::Create {
            name,
            disabled,
            definition,
            output,
        } => {
            let body = create_body(&name, disabled, definition)?;
            create(ctx, &body, output).await
        }
        BadgesCommand::Edit {
            id,
            name,
            enabled,
            definition,
            clear_description,
            clear_valid_from,
            clear_valid_until,
            if_match,
            output,
        } => {
            let body = edit_body(
                name,
                enabled,
                definition,
                [clear_description, clear_valid_from, clear_valid_until],
            )?;
            edit(ctx, &id, &body, if_match, output).await
        }
        BadgesCommand::Delete { id, if_match } => delete(ctx, &id, if_match).await,
    }
}

fn checked_name(name: &str) -> Result<String> {
    let name = name.trim();
    if let Some(err) = indicators::check_badge_name(name) {
        bail!("--name `{name}`: {err}");
    }
    Ok(name.to_string())
}

/// The definition members from the shared flags: absent flags stay absent.
fn definition(d: DefinitionArgs) -> Result<BadgeWrite> {
    let mut w = BadgeWrite::default();
    if let Some(desc) = d.description {
        let desc = desc.trim().to_string();
        if desc.chars().count() > indicators::MAX_BADGE_DESCRIPTION {
            bail!(
                "--description is {} characters at most",
                indicators::MAX_BADGE_DESCRIPTION
            );
        }
        w.description = Some(if desc.is_empty() { None } else { Some(desc) });
    }
    w.priority = d.priority;
    w.color = d.color;
    if let Some(symbol) = d.symbol {
        let symbol = symbol.trim().to_string();
        if symbol.is_empty() {
            bail!("--symbol cannot be blank");
        }
        if !d.icon && symbol.chars().count() > indicators::MAX_TEXT_SYMBOL {
            bail!(
                "--symbol is {} characters at most; give --icon for an icon key",
                indicators::MAX_TEXT_SYMBOL
            );
        }
        w.symbol = Some(symbol);
        w.symbol_kind = Some(if d.icon { "icon" } else { "text" }.to_string());
    } else if d.icon {
        bail!("--icon needs --symbol");
    }
    if !d.tags.is_empty() {
        let mut tags = Vec::new();
        for t in d.tags {
            ensure_guid("--tag", &t)?;
            let t = t.to_lowercase();
            if !tags.contains(&t) {
                tags.push(t);
            }
        }
        w.tags = Some(tags);
    }
    if d.all_tags {
        w.tag_match = Some("all".to_string());
    }
    w.status_filter = d.status;
    if let (Some(from), Some(until)) = (&d.valid_from, &d.valid_until)
        && from >= until
    {
        bail!("--valid-until must come after --valid-from");
    }
    w.valid_from = d.valid_from.map(Some);
    w.valid_until = d.valid_until.map(Some);
    Ok(w)
}

fn create_body(name: &str, disabled: bool, d: DefinitionArgs) -> Result<BadgeWrite> {
    if d.symbol.is_none() {
        bail!("--symbol is required");
    }
    if d.tags.is_empty() {
        bail!("at least one --tag is required: without tags a badge would never match a run");
    }
    let mut w = definition(d)?;
    w.name = Some(checked_name(name)?);
    if disabled {
        w.enabled = Some(false);
    }
    Ok(w)
}

fn edit_body(
    name: Option<String>,
    enabled: Option<bool>,
    d: DefinitionArgs,
    [clear_description, clear_valid_from, clear_valid_until]: [bool; 3],
) -> Result<BadgeWrite> {
    let mut w = definition(d)?;
    w.name = name.as_deref().map(checked_name).transpose()?;
    w.enabled = enabled;
    if clear_description {
        w.description = Some(None);
    }
    if clear_valid_from {
        w.valid_from = Some(None);
    }
    if clear_valid_until {
        w.valid_until = Some(None);
    }
    if w.is_empty() {
        bail!("nothing to change: give at least one flag");
    }
    Ok(w)
}

async fn list(ctx: &Context, only_enabled: Option<bool>, out: OutputArgs) -> Result<()> {
    let page: Page<Badge> = ctx.client()?.get("v2/badges").await?.json()?;
    let items: Vec<&Badge> = page
        .items
        .iter()
        .filter(|b| only_enabled.is_none_or(|e| b.enabled == e))
        .collect();
    out.print(&items, "No badge", || {
        let mut t = Table::new([
            "ID", "NAME", "STATE", "PRIORITY", "SYMBOL", "COLOR", "RULE", "VALIDITY",
        ]);
        for b in &items {
            t.row([
                b.id.clone(),
                b.name.clone(),
                state(b.enabled).to_string(),
                b.priority.to_string(),
                symbol(b),
                b.color.to_string(),
                rule(b),
                validity(b),
            ]);
        }
        t
    })
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("badge id", id)?;
    let resp = ctx.client()?.get(&format!("v2/badges/{id}")).await?;
    print_badge(&resp.json()?, resp.etag.as_deref(), &out)
}

async fn create(ctx: &Context, body: &BadgeWrite, out: OutputArgs) -> Result<()> {
    let resp = ctx
        .client()?
        .post_json("v2/badges", body, &WriteOptions::idempotent())
        .await?;
    let b: Badge = resp.json()?;
    eprintln!("Created badge {} ({})", b.name, b.id);
    print_badge(&b, resp.etag.as_deref(), &out)
}

async fn edit(
    ctx: &Context,
    id: &str,
    body: &BadgeWrite,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("badge id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .patch_merge(&format!("v2/badges/{id}"), body, &write)
        .await?;
    let b: Badge = resp.json()?;
    eprintln!("Updated badge {} ({})", b.name, b.id);
    print_badge(&b, resp.etag.as_deref(), &out)
}

async fn delete(ctx: &Context, id: &str, if_match: Option<String>) -> Result<()> {
    ensure_guid("badge id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    ctx.client()?
        .delete(&format!("v2/badges/{id}"), &write)
        .await?;
    eprintln!("Deleted badge {id}");
    Ok(())
}

fn print_badge(b: &Badge, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut j = serde_json::to_value(b)?;
        if let Some(e) = etag {
            j["etag"] = e.into();
        }
        return out.print_json(&j);
    }
    println!("Badge:        {}", b.id);
    println!("Name:         {}", b.name);
    println!("Description:  {}", or_dash(b.description.as_deref()));
    println!("State:        {}", state(b.enabled));
    println!("Priority:     {}", b.priority);
    println!("Symbol:       {} (color {})", symbol(b), b.color);
    println!("Rule:         {}", rule(b));
    println!("Validity:     {}", validity(b));
    println!("Created:      {}", short_time(&b.created_at));
    println!(
        "Updated:      {}",
        b.updated_at.as_deref().map_or("-".to_string(), short_time)
    );
    if let Some(e) = etag {
        println!("ETag:         {e}");
    }
    if !b.tags.is_empty() {
        println!();
        println!("Tags:");
        let mut t = Table::new(["ID", "NAME", "KIND"]);
        for tag in &b.tags {
            t.row([tag.id.clone(), tag.name.clone(), tag.kind.clone()]);
        }
        t.print();
    }
    Ok(())
}

fn state(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

fn symbol(b: &Badge) -> String {
    if b.symbol_kind == "icon" {
        format!("icon:{}", b.symbol)
    } else {
        b.symbol.clone()
    }
}

/// `failed runs with any of: prod, eu`.
fn rule(b: &Badge) -> String {
    let runs = match b.status_filter.as_str() {
        "any" => "every run",
        "success" => "successful runs",
        "failed" => "failed runs",
        other => other,
    };
    let names: Vec<&str> = b.tags.iter().map(|t| t.name.as_str()).collect();
    let tags = if names.is_empty() {
        "-".to_string()
    } else {
        names.join(", ")
    };
    format!("{runs} with {} of: {tags}", b.tag_match)
}

fn validity(b: &Badge) -> String {
    match (b.valid_from.as_deref(), b.valid_until.as_deref()) {
        (None, None) => "always".to_string(),
        (Some(f), None) => format!("from {}", short_time(f)),
        (None, Some(u)) => format!("until {}", short_time(u)),
        (Some(f), Some(u)) => format!("{} to {}", short_time(f), short_time(u)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: &str = "11111111-1111-4111-8111-111111111111";

    fn flags(symbol: Option<&str>, icon: bool, tags: &[&str]) -> DefinitionArgs {
        DefinitionArgs {
            symbol: symbol.map(str::to_string),
            icon,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn create_body_applies_defaults_by_omission() {
        let b = create_body("Prod", false, flags(Some("P"), false, &[TAG])).unwrap();
        assert_eq!(
            serde_json::to_value(&b).unwrap(),
            serde_json::json!({"name": "Prod", "symbolKind": "text", "symbol": "P", "tags": [TAG]})
        );
        let b = create_body("Prod", true, flags(Some("rocket"), true, &[TAG, TAG])).unwrap();
        assert_eq!(b.enabled, Some(false));
        assert_eq!(b.symbol_kind.as_deref(), Some("icon"));
        assert_eq!(b.tags.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn create_body_requires_symbol_and_tags() {
        assert!(create_body("Prod", false, flags(None, false, &[TAG])).is_err());
        assert!(create_body("Prod", false, flags(Some("P"), false, &[])).is_err());
        assert!(create_body("Prod", false, flags(Some("PRO"), false, &[TAG])).is_err());
        assert!(create_body("", false, flags(Some("P"), false, &[TAG])).is_err());
        assert!(create_body("Prod", false, flags(Some("P"), false, &["nope"])).is_err());
    }

    #[test]
    fn edit_body_is_a_merge_patch() {
        let b = edit_body(
            None,
            Some(false),
            DefinitionArgs::default(),
            [false, false, true],
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&b).unwrap(),
            serde_json::json!({"enabled": false, "validUntil": null})
        );
        assert!(edit_body(None, None, DefinitionArgs::default(), [false; 3]).is_err());
        let d = DefinitionArgs {
            all_tags: true,
            status: Some("any".into()),
            valid_from: Some("2026-02-01T00:00:00Z".into()),
            valid_until: Some("2026-01-01T00:00:00Z".into()),
            ..Default::default()
        };
        assert!(edit_body(None, None, d, [false; 3]).is_err());
        let d = DefinitionArgs {
            icon: true,
            ..Default::default()
        };
        assert!(edit_body(None, None, d, [false; 3]).is_err());
    }

    fn badge() -> Badge {
        serde_json::from_value(serde_json::json!({
            "id": "b", "name": "Prod", "description": null, "enabled": true, "priority": 10,
            "color": 3, "symbolKind": "icon", "symbol": "rocket", "tagMatch": "all",
            "statusFilter": "any", "validFrom": "2026-01-01T00:00:00+00:00", "validUntil": null,
            "tags": [{"id": "t", "name": "prod", "comment": null, "kind": "business"},
                     {"id": "u", "name": "eu", "comment": null, "kind": "business"}],
            "createdAt": "2026-01-01T00:00:00+00:00", "updatedAt": null
        }))
        .unwrap()
    }

    #[test]
    fn cells() {
        let b = badge();
        assert_eq!(symbol(&b), "icon:rocket");
        assert_eq!(rule(&b), "every run with all of: prod, eu");
        assert_eq!(validity(&b), "from 2026-01-01 00:00");
    }
}
