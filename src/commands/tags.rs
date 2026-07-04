//! `2bec tags`: the tags a run may carry, and the space's own ones written.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::runs::ensure_guid;
use crate::api::indicators::{self, Tag, TagWrite};
use crate::api::page::Page;
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash};

#[derive(Args, Debug)]
#[command(after_help = "\
Reads need scenarios:read, writes indicators:write. Only the space's own
tags (kind business) are written; the robots' (robot) and the platform's
(global) are read-only.")]
pub struct TagsArgs {
    #[command(subcommand)]
    command: TagsCommand,
}

#[derive(Subcommand, Debug)]
enum TagsCommand {
    /// List the tags by name: the space's own, the robots' and the platform's
    #[command(after_help = "\
Examples:
  2bec tags list
  2bec tags list --kind business
  2bec tags list --json | jq -r '.[].name'")]
    List {
        /// Only tags of this kind
        #[arg(long, value_name = "KIND", value_parser = ["business", "robot", "global"])]
        kind: Option<String>,
        /// Only tags whose name contains these words (case-insensitive)
        #[arg(long, short = 'q', value_name = "WORDS")]
        search: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one tag with its ETag
    Get {
        /// Tag id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Create a tag of the space
    #[command(after_help = "\
Examples:
  2bec tags create \"release 42\"
  2bec tags create maintenance --comment \"Runs during a maintenance window\"

Names are unique in the space whatever their case (409 conflict), 256
characters at most; obs::, ws::, wsconfig::, #, ____: and FindNeut prefixes
are reserved for the platform.")]
    Create {
        /// Tag name
        name: String,
        /// Comment (2048 characters at most)
        #[arg(long, value_name = "TEXT")]
        comment: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Rename a tag or change its comment
    #[command(after_help = "\
Examples:
  2bec tags edit <id> --name \"release 43\"
  2bec tags edit <id> --comment \"\"                  # clears the comment
  2bec tags edit <id> --name x --if-match '\"v3\"'

Only the flags given change (a JSON merge patch).")]
    Edit {
        /// Tag id
        id: String,
        /// New name
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// New comment; an empty string clears it
        #[arg(long, value_name = "TEXT")]
        comment: Option<String>,
        /// Only change if the tag still has this ETag (from `tags get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete a tag of the space
    #[command(
        alias = "rm",
        after_help = "\
Examples:
  2bec tags delete <id>
  2bec tags delete <id> --force              # detaches it from every run first

409 tag-in-use without --force while runs still carry the tag."
    )]
    Delete {
        /// Tag id
        id: String,
        /// Delete even when runs carry the tag, detaching it from them first
        #[arg(long)]
        force: bool,
        /// Only delete if the tag still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
}

pub async fn run(ctx: &Context, args: TagsArgs) -> Result<()> {
    match args.command {
        TagsCommand::List {
            kind,
            search,
            output,
        } => list(ctx, kind.as_deref(), search.as_deref(), output).await,
        TagsCommand::Get { id, output } => get(ctx, &id, output).await,
        TagsCommand::Create {
            name,
            comment,
            output,
        } => {
            let body = TagWrite {
                name: Some(checked_name(&name, false)?),
                comment: checked_comment(comment)?,
            };
            create(ctx, &body, output).await
        }
        TagsCommand::Edit {
            id,
            name,
            comment,
            if_match,
            output,
        } => {
            let body = TagWrite {
                name: name.map(|n| checked_name(&n, true)).transpose()?,
                comment: checked_comment(comment)?,
            };
            if body == TagWrite::default() {
                bail!("nothing to change: give --name, --comment or both");
            }
            edit(ctx, &id, &body, if_match, output).await
        }
        TagsCommand::Delete {
            id,
            force,
            if_match,
        } => delete(ctx, &id, force, if_match).await,
    }
}

fn checked_name(name: &str, allow_reserved: bool) -> Result<String> {
    let name = name.trim();
    if let Some(err) = indicators::check_tag_name(name, allow_reserved) {
        bail!("name `{name}`: {err}");
    }
    Ok(name.to_string())
}

/// The comment member: absent keeps, an empty string clears.
fn checked_comment(comment: Option<String>) -> Result<Option<Option<String>>> {
    match comment {
        Some(c) => {
            let c = c.trim().to_string();
            if c.chars().count() > indicators::MAX_TAG_COMMENT {
                bail!(
                    "--comment is {} characters at most",
                    indicators::MAX_TAG_COMMENT
                );
            }
            Ok(Some(if c.is_empty() { None } else { Some(c) }))
        }
        None => Ok(None),
    }
}

async fn list(
    ctx: &Context,
    kind: Option<&str>,
    search: Option<&str>,
    out: OutputArgs,
) -> Result<()> {
    let page: Page<Tag> = ctx.client()?.get("v2/tags").await?.json()?;
    let words: Vec<String> = search
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let items: Vec<&Tag> = page
        .items
        .iter()
        .filter(|t| kind.is_none_or(|k| t.kind == k))
        .filter(|t| {
            let name = t.name.to_lowercase();
            words.iter().all(|w| name.contains(w.as_str()))
        })
        .collect();
    out.print(&items, "No tag", || {
        let mut t = Table::new(["ID", "NAME", "KIND", "COMMENT"]);
        for tag in &items {
            t.row([
                tag.id.clone(),
                tag.name.clone(),
                tag.kind.clone(),
                or_dash(tag.comment.as_deref()),
            ]);
        }
        t
    })
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("tag id", id)?;
    let resp = ctx.client()?.get(&format!("v2/tags/{id}")).await?;
    print_tag(&resp.json()?, resp.etag.as_deref(), &out)
}

async fn create(ctx: &Context, body: &TagWrite, out: OutputArgs) -> Result<()> {
    let resp = ctx
        .client()?
        .post_json("v2/tags", body, &WriteOptions::idempotent())
        .await?;
    let tag: Tag = resp.json()?;
    eprintln!("Created tag {} ({})", tag.name, tag.id);
    print_tag(&tag, resp.etag.as_deref(), &out)
}

async fn edit(
    ctx: &Context,
    id: &str,
    body: &TagWrite,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("tag id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .patch_merge(&format!("v2/tags/{id}"), body, &write)
        .await?;
    let tag: Tag = resp.json()?;
    eprintln!("Updated tag {} ({})", tag.name, tag.id);
    print_tag(&tag, resp.etag.as_deref(), &out)
}

async fn delete(ctx: &Context, id: &str, force: bool, if_match: Option<String>) -> Result<()> {
    ensure_guid("tag id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let query: &[(&str, String)] = if force {
        &[("force", "true".to_string())]
    } else {
        &[]
    };
    ctx.client()?
        .delete_with(&format!("v2/tags/{id}"), query, &write)
        .await?;
    eprintln!("Deleted tag {id}");
    Ok(())
}

fn print_tag(tag: &Tag, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut j = serde_json::to_value(tag)?;
        if let Some(e) = etag {
            j["etag"] = e.into();
        }
        return out.print_json(&j);
    }
    println!("Tag:      {}", tag.id);
    println!("Name:     {}", tag.name);
    println!("Kind:     {}", tag.kind);
    println!("Comment:  {}", or_dash(tag.comment.as_deref()));
    if let Some(e) = etag {
        println!("ETag:     {e}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert_eq!(checked_name(" release 42 ", false).unwrap(), "release 42");
        assert!(checked_name("obs::x", false).is_err());
        assert!(checked_name("obs::x", true).is_ok());
        assert!(checked_name("", false).is_err());
    }

    #[test]
    fn comment_member() {
        assert_eq!(checked_comment(None).unwrap(), None);
        assert_eq!(checked_comment(Some("".into())).unwrap(), Some(None));
        assert_eq!(
            checked_comment(Some(" c ".into())).unwrap(),
            Some(Some("c".into()))
        );
        assert!(checked_comment(Some("c".repeat(2049))).is_err());
    }
}
