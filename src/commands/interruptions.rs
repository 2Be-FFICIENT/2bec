//! `2bec interruptions`: the planned non-observation windows and rules of
//! the transaction scenarios.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::alerts::canonical;
use super::runs::ensure_guid;
use crate::api::interruptions::{
    self, Interruption, InterruptionWrite, WriteResult, describe_spec,
};
use crate::api::page::{Fetch, fetch_items};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
#[command(after_help = "\
A one-off window is a date range; a rule is daily (weekdays and hour ranges),
weekly (from a weekday and time to another) or monthly (a day of the month or
an ordinal weekday). Weekdays are ISO (1 = Monday), times Paris wall clock
HH:mm. Reads need planning:read, writes monitoring:write.")]
pub struct InterruptionsArgs {
    #[command(subcommand)]
    command: InterruptionsCommand,
}

#[derive(Subcommand, Debug)]
enum InterruptionsCommand {
    /// List the windows and rules, with the scenarios each covers
    #[command(after_help = "\
Examples:
  2bec interruptions list
  2bec interruptions list --state upcoming --scenario 7f3c...
  2bec interruptions list --occurrences --from 2026-11-01T00:00:00Z --to 2026-12-01T00:00:00Z

States: ongoing, upcoming, past (one-off windows), rule, ended (recurring).
The range (30 days back to 60 ahead by default, 366 days at most) bounds the
one-off windows listed and, with --occurrences, each rule's windows shown.")]
    List {
        /// Only interruptions covering this scenario id (repeatable)
        #[arg(long = "scenario", value_name = "ID")]
        scenarios: Vec<String>,
        /// Only this state
        #[arg(long, value_name = "STATE")]
        state: Option<String>,
        /// Start of the range, RFC 3339
        #[arg(long, value_name = "TIME")]
        from: Option<String>,
        /// End of the range, RFC 3339
        #[arg(long, value_name = "TIME")]
        to: Option<String>,
        /// Add each interruption's windows within the range
        #[arg(long)]
        occurrences: bool,
        /// Maximum number of interruptions to print
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Fetch every interruption, following pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one window or rule, its scenarios, its occurrences and its ETag
    Get {
        /// Interruption id
        id: String,
        /// Start of the occurrence range, RFC 3339
        #[arg(long, value_name = "TIME")]
        from: Option<String>,
        /// End of the occurrence range, RFC 3339
        #[arg(long, value_name = "TIME")]
        to: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Plan a window or a rule on scenarios (their script mates are covered too)
    #[command(after_help = "\
Examples:
  2bec interruptions create --scenario 7f3c... --start 2026-10-12T22:00:00Z --end 2026-10-13T02:00:00Z --comment Maintenance
  2bec interruptions create --scenario 7f3c... --scenario 9a1d... --spec @nightly.json
  2bec interruptions create --scenario 7f3c... --spec '{\"kind\": \"weekly\", \"weekly\": {\"startDay\": 6, \"startTime\": \"22:00\", \"endDay\": 7, \"endTime\": \"06:00\"}}'

--start/--end plan a one-off window; --spec takes any kind as JSON (a
literal, - for stdin, @file). --starts-at backdates a rule's start (450 days
at most). The answer lists one interruption per script and the calendars
the platform refused; 422 refused when none could be written.")]
    Create {
        /// Scenario id (repeatable, at most 200)
        #[arg(long = "scenario", value_name = "ID", required = true)]
        scenarios: Vec<String>,
        /// Start of a one-off window, RFC 3339
        #[arg(long, value_name = "TIME", requires = "end", conflicts_with = "spec")]
        start: Option<String>,
        /// End of a one-off window, RFC 3339
        #[arg(long, value_name = "TIME", requires = "start", conflicts_with = "spec")]
        end: Option<String>,
        /// Calendar spec as JSON; `-` for stdin, `@file` for a file
        #[arg(long, value_name = "JSON", required_unless_present = "start")]
        spec: Option<String>,
        /// A note on the interruption
        #[arg(long, value_name = "TEXT")]
        comment: Option<String>,
        /// Backdate a rule's start, RFC 3339
        #[arg(long, value_name = "TIME")]
        starts_at: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change the spec (same kind) or the comment of an interruption
    #[command(after_help = "\
Examples:
  2bec interruptions edit 7f3c... --comment \"Moved to Sunday\"
  2bec interruptions edit 7f3c... --spec @spec.json --if-match <etag>
  2bec interruptions edit 7f3c... --comment \"\"           # clears the comment

A rule that has already applied is versioned: the answer is the new
definition, `replaces` naming the previous one. 409 interruption-ended for
an ended rule.")]
    Edit {
        /// Interruption id
        id: String,
        /// New calendar spec as JSON, same kind; `-` for stdin, `@file` for a file
        #[arg(long, value_name = "JSON")]
        spec: Option<String>,
        /// New comment; an empty string clears it
        #[arg(long, value_name = "TEXT")]
        comment: Option<String>,
        /// Only update if the interruption still has this ETag (from `get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// End an ongoing window now, or stop a rule from now on (its past is kept)
    End {
        /// Interruption id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Put an ended rule back in force from now
    Restore {
        /// Interruption id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete a window or a rule; a rule that has applied is ended instead unless --erase-past
    #[command(alias = "rm")]
    Delete {
        /// Interruption id
        id: String,
        /// Erase a rule from the past too (the availability of those days is recomputed)
        #[arg(long)]
        erase_past: bool,
        /// Only delete if the interruption still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
}

pub async fn run(ctx: &Context, args: InterruptionsArgs) -> Result<()> {
    match args.command {
        InterruptionsCommand::List {
            scenarios,
            state,
            from,
            to,
            occurrences,
            limit,
            all,
            output,
        } => {
            if !all && limit == 0 {
                bail!("--limit must be at least 1");
            }
            let mut query = Vec::new();
            for s in &scenarios {
                ensure_guid("--scenario", s)?;
                query.push(("scenario", s.clone()));
            }
            if let Some(st) = state {
                query.push(("state", canonical("--state", &st, &interruptions::STATES)?));
            }
            query.extend(range(from, to));
            if occurrences {
                query.push(("include", "occurrences".into()));
            }
            let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
            let page =
                fetch_items::<Interruption>(&ctx.client()?, "v2/interruptions", &query, fetch)
                    .await?;
            output.print(&page.items, "No planned interruption", || {
                let mut t = Table::new([
                    "ID",
                    "KIND",
                    "STATE",
                    "WHEN",
                    "SCENARIOS",
                    "COMMENT",
                    "NEXT",
                ]);
                for i in &page.items {
                    t.row([
                        i.id.clone(),
                        i.kind.clone(),
                        i.state.clone(),
                        describe_spec(&i.spec),
                        scenario_names(i),
                        or_dash(i.comment.as_deref()),
                        i.occurrences
                            .as_ref()
                            .and_then(|o| o.first())
                            .map_or("-".to_string(), |w| short_time(&w.start)),
                    ]);
                }
                t
            })?;
            if !output.wants_json()
                && let Some(total) = page.total
                && (total as usize) > page.items.len()
            {
                eprintln!(
                    "Showing {} of {total} interruptions; use --limit or --all for more",
                    page.items.len()
                );
            }
            Ok(())
        }
        InterruptionsCommand::Get {
            id,
            from,
            to,
            output,
        } => {
            ensure_guid("interruption id", &id)?;
            let resp = ctx
                .client()?
                .get_with(&format!("v2/interruptions/{id}"), &range(from, to))
                .await?;
            let i: Interruption = resp.json()?;
            print_interruption(&i, resp.etag.as_deref(), &output)
        }
        InterruptionsCommand::Create {
            scenarios,
            start,
            end,
            spec,
            comment,
            starts_at,
            output,
        } => {
            if scenarios.len() > interruptions::MAX_SCENARIOS {
                bail!("at most {} --scenario values", interruptions::MAX_SCENARIOS);
            }
            for s in &scenarios {
                ensure_guid("--scenario", s)?;
            }
            let spec = match (start, end, spec) {
                (Some(s), Some(e), None) => {
                    serde_json::json!({"kind": "date", "date": {"start": s, "end": e}})
                }
                (None, None, Some(j)) => read_spec(&j)?,
                _ => bail!("give --start and --end, or --spec"),
            };
            let body = InterruptionWrite {
                scenarios,
                spec,
                comment: non_empty(comment),
                starts_at,
            };
            let r: WriteResult = ctx
                .client()?
                .post_json("v2/interruptions", &body, &WriteOptions::idempotent())
                .await?
                .json()?;
            if output.wants_json() {
                return output.print_json(&r);
            }
            for f in &r.failed {
                eprintln!(
                    "warning: refused for {}: {}",
                    f.scenarios
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    f.reason
                );
            }
            eprintln!(
                "Planned {} interruption(s), one per script",
                r.interruptions.len()
            );
            for i in &r.interruptions {
                println!();
                print_interruption(i, None, &output)?;
            }
            Ok(())
        }
        InterruptionsCommand::Edit {
            id,
            spec,
            comment,
            if_match,
            output,
        } => {
            ensure_guid("interruption id", &id)?;
            let mut patch = serde_json::Map::new();
            if let Some(j) = spec {
                patch.insert("spec".into(), read_spec(&j)?);
            }
            if let Some(c) = comment {
                patch.insert(
                    "comment".into(),
                    match non_empty(Some(c)) {
                        Some(c) => c.into(),
                        None => serde_json::Value::Null,
                    },
                );
            }
            if patch.is_empty() {
                bail!("nothing to change: give --spec or --comment");
            }
            let write = WriteOptions {
                idempotency_key: None,
                if_match,
            };
            let resp = ctx
                .client()?
                .patch_merge(&format!("v2/interruptions/{id}"), &patch, &write)
                .await?;
            let i: Interruption = resp.json()?;
            print_interruption(&i, resp.etag.as_deref(), &output)
        }
        InterruptionsCommand::End { id, output } => {
            ensure_guid("interruption id", &id)?;
            let resp = ctx
                .client()?
                .post_json(
                    &format!("v2/interruptions/{id}:end"),
                    &serde_json::json!({}),
                    &WriteOptions::idempotent(),
                )
                .await?;
            if resp.status == 204 || resp.body.trim().is_empty() {
                if output.wants_json() {
                    return output.print_json(&serde_json::json!({"id": id, "deleted": true}));
                }
                eprintln!("Rule {id} never applied: deleted");
                return Ok(());
            }
            let i: Interruption = resp.json()?;
            if !output.wants_json() {
                eprintln!("Ended interruption {id}");
            }
            print_interruption(&i, resp.etag.as_deref(), &output)
        }
        InterruptionsCommand::Restore { id, output } => {
            ensure_guid("interruption id", &id)?;
            let resp = ctx
                .client()?
                .post_json(
                    &format!("v2/interruptions/{id}:restore"),
                    &serde_json::json!({}),
                    &WriteOptions::idempotent(),
                )
                .await?;
            let i: Interruption = resp.json()?;
            if !output.wants_json() {
                eprintln!("Restored rule {id}");
            }
            print_interruption(&i, resp.etag.as_deref(), &output)
        }
        InterruptionsCommand::Delete {
            id,
            erase_past,
            if_match,
        } => {
            ensure_guid("interruption id", &id)?;
            let write = WriteOptions {
                idempotency_key: None,
                if_match,
            };
            let path = format!(
                "v2/interruptions/{id}{}",
                if erase_past { "?erasePast=true" } else { "" }
            );
            ctx.client()?.delete(&path, &write).await?;
            eprintln!("Deleted interruption {id}");
            Ok(())
        }
    }
}

fn range(from: Option<String>, to: Option<String>) -> Vec<(&'static str, String)> {
    let mut q = Vec::new();
    if let Some(f) = from {
        q.push(("from", f));
    }
    if let Some(t) = to {
        q.push(("to", t));
    }
    q
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// A spec from a literal, stdin or a file, checked to name a known kind.
fn read_spec(arg: &str) -> Result<serde_json::Value> {
    let spec = super::read_json_body(arg)?;
    let kind = spec.get("kind").and_then(|k| k.as_str()).ok_or_else(|| {
        anyhow::anyhow!(
            "the spec needs a `kind`: {}",
            interruptions::KINDS.join(", ")
        )
    })?;
    if !interruptions::KINDS.contains(&kind) {
        bail!(
            "spec kind `{kind}` is not one of {}",
            interruptions::KINDS.join(", ")
        );
    }
    if spec.get(kind).is_none_or(|v| !v.is_object()) {
        bail!("a `{kind}` spec needs a `{kind}` object");
    }
    Ok(spec)
}

fn scenario_names(i: &Interruption) -> String {
    let mut s = match i.scenarios.len() {
        0 => String::new(),
        1 => i.scenarios[0].name.clone(),
        n => format!("{} +{}", i.scenarios[0].name, n - 1),
    };
    if i.hidden_scenarios > 0 {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!("({} hidden)", i.hidden_scenarios));
    }
    if s.is_empty() { "-".into() } else { s }
}

fn print_interruption(i: &Interruption, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(i)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!("Interruption: {}", i.id);
    println!(
        "Kind:         {} ({}){}",
        i.kind,
        i.state,
        if i.exclusion {
            ""
        } else {
            " [legacy inclusion window, read-only]"
        }
    );
    println!("When:         {}", describe_spec(&i.spec));
    println!("Comment:      {}", or_dash(i.comment.as_deref()));
    if i.valid_from.is_some() || i.valid_until.is_some() {
        println!(
            "Valid:        from {} until {}{}",
            i.valid_from.as_deref().map_or("-".to_string(), short_time),
            i.valid_until
                .as_deref()
                .map_or("now".to_string(), short_time),
            if i.applied { " (has applied)" } else { "" }
        );
    }
    if let Some(r) = &i.replaces {
        println!("Replaces:     {r}");
    }
    println!(
        "Scenarios:    {}",
        if i.scenarios.is_empty() && i.hidden_scenarios == 0 {
            "-".into()
        } else {
            let mut names: Vec<String> = i
                .scenarios
                .iter()
                .map(|s| format!("{} ({})", s.name, s.id))
                .collect();
            if i.hidden_scenarios > 0 {
                names.push(format!("{} hidden", i.hidden_scenarios));
            }
            names.join(", ")
        }
    );
    if let Some(e) = etag {
        println!("ETag:         {e}");
    }
    if let Some(occ) = &i.occurrences
        && !occ.is_empty()
    {
        println!();
        let mut t = Table::new(["START", "END"]);
        for w in occ {
            t.row([short_time(&w.start), short_time(&w.end)]);
        }
        t.print();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_checked_for_a_kind_and_its_object() {
        assert!(read_spec(r#"{"kind": "daily", "daily": {"weekdays": [1]}}"#).is_ok());
        assert!(
            read_spec(r#"{"daily": {}}"#)
                .unwrap_err()
                .to_string()
                .contains("needs a `kind`")
        );
        assert!(
            read_spec(r#"{"kind": "yearly", "yearly": {}}"#)
                .unwrap_err()
                .to_string()
                .contains("not one of")
        );
        assert!(
            read_spec(r#"{"kind": "weekly"}"#)
                .unwrap_err()
                .to_string()
                .contains("`weekly` object")
        );
    }
}
