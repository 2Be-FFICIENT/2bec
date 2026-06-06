//! `2bec availability`: how the availability rate is computed and against
//! which target it is read.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::alerts::canonical;
use super::runs::ensure_guid;
use crate::api::availability::{
    self, AvailabilitySettings, AvailabilitySettingsWrite, AvailabilitySettingsWritten,
    AvailabilityTargets, AvailabilityTargetsWrite, AvailabilityTargetsWritten, ScenarioTargetWrite,
    TagRef,
};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table};

#[derive(Args, Debug)]
#[command(after_help = "\
The reads need metrics:read, the writes indicators:write. Every write is an
entry of the administration history naming the key.")]
pub struct AvailabilityArgs {
    #[command(subcommand)]
    command: AvailabilityCommand,
}

#[derive(Subcommand, Debug)]
enum AvailabilityCommand {
    /// The computing rules: neutralized runs, failures before a scenario is down, tag rules
    Settings(SettingsArgs),
    /// The percentage every availability figure is read against, the space's and each scenario's
    Targets(TargetsArgs),
}

#[derive(Args, Debug)]
pub struct SettingsArgs {
    #[command(subcommand)]
    command: SettingsCommand,
}

#[derive(Subcommand, Debug)]
enum SettingsCommand {
    /// Show the rules and their ETag
    Get {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change the rules; the past is recomputed with them in the background
    #[command(after_help = "\
Examples:
  2bec availability settings set --computing-type ignoreNeutralized
  2bec availability settings set --failure-counts-from 2
  2bec availability settings set --tag-rules true --failure-tags 1a2b...,3c4d... --neutral-tags \"\"

What is not named stays. --computing-type: removeNeutralized,
ignoreNeutralized, neutralizedAsSuccess or neutralizedAsFailure.
--failure-counts-from: 1 to 10 consecutive failures. The tag lists replace
the rule's tags (an empty value clears it); a tag is in one rule at most,
20 per rule. The write starts from the rules it read (If-Match), so a change
made meanwhile makes it fail with 412; 403 rules-locked while the platform's
team keeps the rules of the space.")]
    Set {
        /// What becomes of the neutralized runs
        #[arg(long, value_name = "TYPE")]
        computing_type: Option<String>,
        /// A scenario is down from this many consecutive failures (1 to 10)
        #[arg(long, value_name = "N")]
        failure_counts_from: Option<i64>,
        /// Whether the tag rules apply: true or false
        #[arg(long, value_name = "BOOL")]
        tag_rules: Option<bool>,
        /// Tags marking a run as a failure, comma-separated ids
        #[arg(long, value_name = "IDS", value_delimiter = ',', num_args = 0..)]
        failure_tags: Option<Vec<String>>,
        /// Tags marking a run as a success, comma-separated ids
        #[arg(long, value_name = "IDS", value_delimiter = ',', num_args = 0..)]
        success_tags: Option<Vec<String>>,
        /// Tags marking a run as neutral, comma-separated ids
        #[arg(long, value_name = "IDS", value_delimiter = ',', num_args = 0..)]
        neutral_tags: Option<Vec<String>>,
        /// Start from this ETag instead of the one just read
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
}

#[derive(Args, Debug)]
pub struct TargetsArgs {
    #[command(subcommand)]
    command: TargetsCommand,
}

#[derive(Subcommand, Debug)]
enum TargetsCommand {
    /// Show the space's target and each transaction scenario's own and effective one
    Get {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Set the space's target and scenarios' own targets
    #[command(after_help = "\
Examples:
  2bec availability targets set --tenant 99.5
  2bec availability targets set --tenant default                  # the platform's again
  2bec availability targets set --scenario 7f3c...=99.9 --scenario 1a2b...=default
  2bec availability targets set --tenant 99 --if-match \"$(2bec availability targets get --jq .etag)\"

A target is a percentage above 50 and below 100, 3 decimals at most;
`default` makes the space follow the platform's target, or a scenario the
space's. A scenario not named keeps what it has; 500 per call. No figure is
recomputed: the target only changes how they read.")]
    Set {
        /// The space's target, or `default`
        #[arg(long, value_name = "PCT|default")]
        tenant: Option<String>,
        /// A scenario's own target, SCENARIO_ID=PCT or SCENARIO_ID=default (repeatable)
        #[arg(long, value_name = "ID=PCT|default")]
        scenario: Vec<String>,
        /// Only write if the targets still have this ETag (from `targets get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: AvailabilityArgs) -> Result<()> {
    match args.command {
        AvailabilityCommand::Settings(a) => match a.command {
            SettingsCommand::Get { output } => get_settings(ctx, output).await,
            SettingsCommand::Set {
                computing_type,
                failure_counts_from,
                tag_rules,
                failure_tags,
                success_tags,
                neutral_tags,
                if_match,
                output,
            } => {
                let changes = SettingsChanges {
                    computing_type,
                    failure_counts_from,
                    tag_rules,
                    failure_tags,
                    success_tags,
                    neutral_tags,
                };
                set_settings(ctx, changes, if_match, output).await
            }
        },
        AvailabilityCommand::Targets(a) => match a.command {
            TargetsCommand::Get { output } => get_targets(ctx, output).await,
            TargetsCommand::Set {
                tenant,
                scenario,
                if_match,
                output,
            } => {
                let body = targets_write(tenant.as_deref(), &scenario)?;
                set_targets(ctx, &body, if_match, output).await
            }
        },
    }
}

// ---------- settings ----------

async fn get_settings(ctx: &Context, out: OutputArgs) -> Result<()> {
    let resp = ctx.client()?.get("v2/availability-settings").await?;
    print_settings(&resp.json()?, resp.etag.as_deref(), &out)
}

/// The flags of `settings set`, each `None` when not given.
struct SettingsChanges {
    computing_type: Option<String>,
    failure_counts_from: Option<i64>,
    tag_rules: Option<bool>,
    failure_tags: Option<Vec<String>>,
    success_tags: Option<Vec<String>>,
    neutral_tags: Option<Vec<String>>,
}

impl SettingsChanges {
    fn is_empty(&self) -> bool {
        self.computing_type.is_none()
            && self.failure_counts_from.is_none()
            && self.tag_rules.is_none()
            && self.failure_tags.is_none()
            && self.success_tags.is_none()
            && self.neutral_tags.is_none()
    }

    /// The flags checked and canonicalized, before anything is read.
    fn checked(self) -> Result<Self> {
        if self.is_empty() {
            bail!(
                "nothing to change: give --computing-type, --failure-counts-from, --tag-rules or a tag list"
            );
        }
        if let Some(n) = self.failure_counts_from
            && !availability::FAILURE_COUNTS_FROM.contains(&n)
        {
            bail!(
                "--failure-counts-from is between {} and {}",
                availability::FAILURE_COUNTS_FROM.start(),
                availability::FAILURE_COUNTS_FROM.end()
            );
        }
        Ok(Self {
            computing_type: self
                .computing_type
                .map(|t| canonical("--computing-type", &t, &availability::COMPUTING_TYPES))
                .transpose()?,
            failure_tags: self
                .failure_tags
                .map(|ids| tag_ids("--failure-tags", ids))
                .transpose()?,
            success_tags: self
                .success_tags
                .map(|ids| tag_ids("--success-tags", ids))
                .transpose()?,
            neutral_tags: self
                .neutral_tags
                .map(|ids| tag_ids("--neutral-tags", ids))
                .transpose()?,
            ..self
        })
    }

    /// The current rules with these (checked) changes applied.
    fn apply_to(self, current: &AvailabilitySettings) -> Result<AvailabilitySettingsWrite> {
        let mut w = AvailabilitySettingsWrite::from_current(current);
        if let Some(t) = self.computing_type {
            w.computing_type = t;
        }
        if let Some(n) = self.failure_counts_from {
            w.failure_counts_from = n;
        }
        if let Some(e) = self.tag_rules {
            w.tag_rules_enabled = e;
        }
        if let Some(ids) = self.failure_tags {
            w.tag_rules.failure = ids;
        }
        if let Some(ids) = self.success_tags {
            w.tag_rules.success = ids;
        }
        if let Some(ids) = self.neutral_tags {
            w.tag_rules.neutral = ids;
        }
        let all: Vec<&String> = w
            .tag_rules
            .failure
            .iter()
            .chain(&w.tag_rules.success)
            .chain(&w.tag_rules.neutral)
            .collect();
        for (i, id) in all.iter().enumerate() {
            if all[..i].iter().any(|o| o.eq_ignore_ascii_case(id)) {
                bail!("tag {id} is in more than one rule");
            }
        }
        Ok(w)
    }
}

/// A comma-separated tag list as ids, an empty value as none.
fn tag_ids(flag: &str, ids: Vec<String>) -> Result<Vec<String>> {
    let ids: Vec<String> = ids
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if ids.len() > availability::MAX_RULE_TAGS {
        bail!("{flag} takes {} tags at most", availability::MAX_RULE_TAGS);
    }
    for id in &ids {
        ensure_guid(flag, id)?;
    }
    Ok(ids)
}

async fn set_settings(
    ctx: &Context,
    changes: SettingsChanges,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    let changes = changes.checked()?;
    let client = ctx.client()?;
    let resp = client.get("v2/availability-settings").await?;
    let current: AvailabilitySettings = resp.json()?;
    if !current.editable {
        bail!("the availability rules of this space are locked by the platform's team");
    }
    let body = changes.apply_to(&current)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match: if_match.or(resp.etag),
    };
    let resp = client
        .put_json("v2/availability-settings", &body, &write)
        .await?;
    let w: AvailabilitySettingsWritten = resp.json()?;
    if out.wants_json() {
        let mut v = serde_json::to_value(&w)?;
        if let Some(e) = &resp.etag {
            v["etag"] = e.as_str().into();
        }
        return out.print_json(&v);
    }
    print_settings(&w.settings, resp.etag.as_deref(), &out)?;
    println!();
    if !w.rules_changed {
        println!("The rules were already these; nothing changed.");
    } else if w.recompute_scheduled {
        println!("The past days are being recomputed with the new rules.");
    } else {
        eprintln!(
            "warning: the rules are saved but the platform did not take the recompute of the past days"
        );
    }
    Ok(())
}

fn print_settings(s: &AvailabilitySettings, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(s)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!(
        "Neutralized runs:    {}{}",
        computing_label(&s.computing_type),
        if s.is_default {
            " (platform defaults)"
        } else {
            ""
        }
    );
    println!(
        "Down from:           {} consecutive failure(s)",
        s.failure_counts_from
    );
    println!(
        "Tag rules:           {}",
        if s.tag_rules_enabled { "on" } else { "off" }
    );
    println!("  Failure tags:      {}", tags(&s.tag_rules.failure));
    println!("  Success tags:      {}", tags(&s.tag_rules.success));
    println!("  Neutral tags:      {}", tags(&s.tag_rules.neutral));
    println!(
        "Editable:            {}",
        if s.editable {
            "yes"
        } else {
            "no (locked by the platform's team)"
        }
    );
    if let Some(e) = etag {
        println!("ETag:                {e}");
    }
    Ok(())
}

fn computing_label(t: &str) -> &str {
    match t {
        "removeNeutralized" => "removed from the figures",
        "ignoreNeutralized" => "ignored",
        "neutralizedAsSuccess" => "counted as successes",
        "neutralizedAsFailure" => "counted as failures",
        other => other,
    }
}

fn tags(tags: &[TagRef]) -> String {
    if tags.is_empty() {
        return "-".into();
    }
    tags.iter()
        .map(|t| match &t.name {
            Some(n) => format!("{n} ({})", t.id),
            None => t.id.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------- targets ----------

async fn get_targets(ctx: &Context, out: OutputArgs) -> Result<()> {
    let resp = ctx.client()?.get("v2/availability-targets").await?;
    print_targets(&resp.json()?, resp.etag.as_deref(), &out)
}

/// `PCT` or `default` as a target, `None` for the default.
fn parse_target(flag: &str, s: &str) -> Result<Option<f64>> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("default") {
        return Ok(None);
    }
    let t: f64 = s
        .trim_end_matches('%')
        .parse()
        .map_err(|_| anyhow::anyhow!("{flag} must be a percentage or `default`, got `{s}`"))?;
    if let Some(err) = availability::check_target(t) {
        bail!("{flag}: {err}");
    }
    Ok(Some(t))
}

/// The targets body from the flags, checked locally.
fn targets_write(tenant: Option<&str>, scenarios: &[String]) -> Result<AvailabilityTargetsWrite> {
    if tenant.is_none() && scenarios.is_empty() {
        bail!("give --tenant, --scenario or both");
    }
    if scenarios.len() > availability::MAX_TARGET_SCENARIOS {
        bail!(
            "at most {} scenarios per call",
            availability::MAX_TARGET_SCENARIOS
        );
    }
    let mut body = AvailabilityTargetsWrite::default();
    if let Some(t) = tenant {
        body.tenant = Some(parse_target("--tenant", t)?);
    }
    for s in scenarios {
        let Some((id, target)) = s.split_once('=') else {
            bail!("--scenario `{s}` must be SCENARIO_ID=PCT or SCENARIO_ID=default");
        };
        ensure_guid("--scenario", id)?;
        if body
            .scenarios
            .iter()
            .any(|w| w.scenario.eq_ignore_ascii_case(id))
        {
            bail!("--scenario {id} is given twice");
        }
        body.scenarios.push(ScenarioTargetWrite {
            scenario: id.to_string(),
            target: parse_target("--scenario", target)?,
        });
    }
    Ok(body)
}

async fn set_targets(
    ctx: &Context,
    body: &AvailabilityTargetsWrite,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .put_json("v2/availability-targets", body, &write)
        .await?;
    let w: AvailabilityTargetsWritten = resp.json()?;
    if out.wants_json() {
        let mut v = serde_json::to_value(&w)?;
        if let Some(e) = &resp.etag {
            v["etag"] = e.as_str().into();
        }
        return out.print_json(&v);
    }
    println!(
        "Written: {}, unchanged: {}, failed: {}",
        w.written, w.unchanged, w.failed
    );
    println!();
    print_targets(&w.targets, resp.etag.as_deref(), &out)?;
    if w.failed > 0 {
        eprintln!(
            "warning: {} target(s) were refused by the platform",
            w.failed
        );
    }
    Ok(())
}

fn print_targets(t: &AvailabilityTargets, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(t)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!(
        "Space target: {}% ({})",
        t.effective,
        match t.tenant {
            Some(_) => "own".to_string(),
            None => format!("platform default {}%", t.default),
        }
    );
    if let Some(e) = etag {
        println!("ETag:         {e}");
    }
    if t.scenarios.is_empty() {
        return Ok(());
    }
    println!();
    let mut table = Table::new(["ID", "SCENARIO", "OWN", "EFFECTIVE"]);
    for s in &t.scenarios {
        table.row([
            s.scenario.id.clone(),
            s.scenario.name.clone(),
            s.own.map_or("-".into(), |o| format!("{o}%")),
            format!("{}%", s.effective),
        ]);
    }
    table.print();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::availability::TagRules;

    const G: &str = "7f3c1a2b-0000-4000-8000-000000000000";
    const H: &str = "1a2b3c4d-0000-4000-8000-000000000000";

    fn current() -> AvailabilitySettings {
        AvailabilitySettings {
            computing_type: "removeNeutralized".into(),
            failure_counts_from: 1,
            tag_rules_enabled: false,
            tag_rules: TagRules {
                failure: vec![TagRef {
                    id: G.into(),
                    name: None,
                    kind: None,
                }],
                success: vec![],
                neutral: vec![],
            },
            editable: true,
            is_default: false,
        }
    }

    fn changes() -> SettingsChanges {
        SettingsChanges {
            computing_type: None,
            failure_counts_from: None,
            tag_rules: None,
            failure_tags: None,
            success_tags: None,
            neutral_tags: None,
        }
    }

    #[test]
    fn changes_merge_onto_the_current_rules() {
        let mut c = changes();
        c.computing_type = Some("IGNORENEUTRALIZED".into());
        c.failure_tags = Some(vec![String::new()]);
        c.neutral_tags = Some(vec![H.into()]);
        let w = c.checked().unwrap().apply_to(&current()).unwrap();
        assert_eq!(w.computing_type, "ignoreNeutralized");
        assert_eq!(w.failure_counts_from, 1);
        assert!(w.tag_rules.failure.is_empty());
        assert_eq!(w.tag_rules.neutral, vec![H.to_string()]);

        let mut c = changes();
        c.success_tags = Some(vec![G.into()]);
        assert!(c.apply_to(&current()).is_err(), "a tag in two rules");
        let mut c = changes();
        c.failure_counts_from = Some(11);
        assert!(c.checked().is_err());
        assert!(changes().checked().is_err(), "nothing to change");
    }

    #[test]
    fn targets_body_from_flags() {
        let b = targets_write(
            Some("default"),
            &[format!("{G}=99.9"), format!("{H}=default")],
        )
        .unwrap();
        assert_eq!(b.tenant, Some(None));
        assert_eq!(b.scenarios[0].target, Some(99.9));
        assert_eq!(b.scenarios[1].target, None);
        assert_eq!(
            targets_write(Some("99.5%"), &[]).unwrap().tenant,
            Some(Some(99.5))
        );
        assert!(targets_write(None, &[]).is_err());
        assert!(targets_write(Some("100"), &[]).is_err());
        assert!(targets_write(Some("99.1234"), &[]).is_err());
        assert!(targets_write(None, &[format!("{G}=99"), format!("{G}=98")]).is_err());
        assert!(targets_write(None, &["nope".into()]).is_err());
    }
}
