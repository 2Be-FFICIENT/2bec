//! `2bec comments`: the team's comments on the runs, across scenarios. The
//! comments of one run are under `runs comments`.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::metrics::Selection;
use crate::api::governance::{self, Comment};
use crate::api::page::{Fetch, fetch_items};
use crate::output::{OutputArgs, Table, short_time};

#[derive(Args, Debug)]
#[command(after_help = "\
Needs runs:read. The feed covers the scenarios the key sees, or the ones
named with --scenario (100 at most either way), over Paris days.")]
pub struct CommentsArgs {
    #[command(subcommand)]
    command: CommentsCommand,
}

#[derive(Subcommand, Debug)]
enum CommentsCommand {
    /// List the comments newest first over a period (the last 7 days by default, 92 at most)
    #[command(after_help = "\
Examples:
  2bec comments list
  2bec comments list --period lastMonth --outcome failed
  2bec comments list --scenario <id> --scenario <id> -q \"false positive\"
  2bec comments list --from 2026-10-01T00:00:00Z --to 2026-10-08T00:00:00Z --all --json

To read or write the comments of one run: 2bec runs comments --help.")]
    List {
        #[command(flatten)]
        selection: Selection,
        /// Only comments on runs with this status
        #[arg(long, value_name = "STATUS", value_parser = governance::COMMENT_OUTCOMES)]
        outcome: Option<String>,
        /// Only comments whose text, author or scenario holds these words
        #[arg(long, short = 'q', value_name = "WORDS")]
        search: Option<String>,
        /// How many comments to show at most
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Every comment of the period, following the pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: CommentsArgs) -> Result<()> {
    match args.command {
        CommentsCommand::List {
            selection,
            outcome,
            search,
            limit,
            all,
            output,
        } => {
            list(
                ctx,
                &selection,
                outcome.as_deref(),
                search.as_deref(),
                limit,
                all,
                output,
            )
            .await
        }
    }
}

async fn list(
    ctx: &Context,
    selection: &Selection,
    outcome: Option<&str>,
    search: Option<&str>,
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let mut query = selection.query(governance::COMMENT_MAX_SCENARIOS)?;
    if let Some(o) = outcome {
        query.push(("outcome", o.to_string()));
    }
    if let Some(q) = search
        && !q.trim().is_empty()
    {
        query.push(("q", q.trim().to_string()));
    }
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page = fetch_items::<Comment>(&ctx.client()?, "v2/comments", &query, fetch).await?;
    out.print(&page.items, "No comment in this period", || {
        let mut t = Table::new([
            "ID",
            "PUBLISHED",
            "AUTHOR",
            "SCENARIO",
            "RUN",
            "STATUS",
            "TEXT",
        ]);
        for c in &page.items {
            t.row([
                c.id.clone(),
                short_time(&c.published_at),
                c.author.clone(),
                c.scenario.name.clone(),
                c.run.id.clone(),
                c.run_status.clone(),
                one_line(&c.text, 80),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} comments; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

/// The text on one line, whitespace collapsed, cut with an ellipsis past `max` characters.
pub fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_flattens_and_cuts() {
        assert_eq!(one_line("a\n  b\tc", 80), "a b c");
        assert_eq!(one_line("abcdefghij", 5), "abcd…");
        assert_eq!(one_line("abcde", 5), "abcde");
    }
}
