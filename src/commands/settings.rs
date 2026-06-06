//! `2bec scenarios settings|edit|rename-steps|run-variables|thresholds`: the
//! writable settings of a transaction scenario and its response time
//! thresholds.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::planning::confirm;
use super::runs::ensure_guid;
use super::scenarios::{ms, thresholds as fmt_thresholds};
use crate::api::settings::{
    self, RequestVariable, RunVariablesWrite, ScenarioSettings, ScenarioThresholds, StepNameWrite,
    StepsWrite, ThresholdApplyWrite, ThresholdComputation, ThresholdComputeWrite,
    ThresholdProposal, ThresholdSettings, Thresholds,
};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};

/// The settings and thresholds commands, flattened into `2bec scenarios`.
#[derive(Subcommand, Debug)]
pub enum SettingsCommand {
    /// Show what a key may change on a transaction scenario, with the ETag its writes take
    #[command(after_help = "\
Examples:
  2bec scenarios settings 7f3c...
  2bec scenarios settings 7f3c... --jq .etag

The name, description and catalogue id, the names of the current version's
steps and the variables an immediate run offers. Needs scenarios:read; 409
social-scenario for a social monitoring scenario.")]
    Settings {
        /// Scenario id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Change the name, description or catalogue id of a scenario
    #[command(after_help = "\
Examples:
  2bec scenarios edit 7f3c... --name \"Login (prod)\"
  2bec scenarios edit 7f3c... --description \"Checked every 5 minutes\" --catalogue-id 1a2b...
  2bec scenarios edit 7f3c... --clear-description --if-match \"$(2bec scenarios settings 7f3c... --jq .etag)\"

What is not named stays. The cadence, the exposure and the internal note are
the platform team's (403 staff-only-field). Needs scenarios:write; the answer
is the settings.")]
    Edit {
        /// Scenario id
        id: String,
        /// New name (50 characters at most)
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// New description
        #[arg(long, value_name = "TEXT", conflicts_with = "clear_description")]
        description: Option<String>,
        /// Remove the description
        #[arg(long)]
        clear_description: bool,
        /// The scenario's id in the space's service catalogue
        #[arg(long, value_name = "ID", conflicts_with = "clear_catalogue_id")]
        catalogue_id: Option<String>,
        /// Remove the catalogue id
        #[arg(long)]
        clear_catalogue_id: bool,
        /// Only write if the settings still have this ETag (from `scenarios settings`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Rename steps of the current script version
    #[command(after_help = "\
Examples:
  2bec scenarios rename-steps 7f3c... 1a2b...=\"Open the home page\"
  2bec scenarios rename-steps 7f3c... 1a2b...=Home 3c4d...=Login --if-match '\"v3\"'

Step ids are in `scenarios settings`; the steps not named keep their name.
Needs scenarios:write; the answer is the settings.")]
    RenameSteps {
        /// Scenario id
        id: String,
        /// STEP_ID=NAME, repeated (names 50 characters at most)
        #[arg(value_name = "STEP=NAME", required = true)]
        steps: Vec<String>,
        /// Only write if the settings still have this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Replace the variables an immediate run of the scenario offers
    #[command(after_help = "\
Examples:
  2bec scenarios run-variables 7f3c... USER=alice ENV            # ENV without a default
  2bec scenarios run-variables 7f3c... --clear                   # none

The list is replaced as a whole: name every variable to keep. Names are
letters, digits and underscores, not starting with a digit, 64 characters
at most, SCHEDULER_ reserved; 20 variables at most; a default is 4000
characters at most. Needs scenarios:write; the answer is the settings.")]
    RunVariables {
        /// Scenario id
        id: String,
        /// NAME or NAME=DEFAULT, repeated
        #[arg(value_name = "NAME[=DEFAULT]", conflicts_with = "clear")]
        variables: Vec<String>,
        /// Remove every run variable
        #[arg(long)]
        clear: bool,
        /// Only write if the settings still have this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Response time thresholds: read them, compute them from past runs, apply them
    Thresholds(ThresholdsArgs),
}

#[derive(Args, Debug)]
pub struct ThresholdsArgs {
    #[command(subcommand)]
    command: ThresholdsCommand,
}

#[derive(Subcommand, Debug)]
enum ThresholdsCommand {
    /// The thresholds of each script version and bandwidth, for the run and each measured step
    #[command(after_help = "\
Examples:
  2bec scenarios thresholds get 7f3c...
  2bec scenarios thresholds get 7f3c... --jq '.versions[] | select(.current)'

Milliseconds; target ≤ client ≤ unacceptable. Needs metrics:read. 409
social-scenario for a social monitoring scenario, 404 when the scenario has
no version yet.")]
    Get {
        /// Scenario id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Propose thresholds from the successful runs of a period (nothing written without --apply)
    #[command(after_help = "\
Examples:
  2bec scenarios thresholds compute 7f3c...                              # last 7 days
  2bec scenarios thresholds compute 7f3c... --from 2026-09-01 --to 2026-10-01
  2bec scenarios thresholds compute 7f3c... --percentile 0.9 --apply     # asks before writing
  2bec scenarios thresholds compute 7f3c... --json | jq .apply > thresholds.json

Whole days in Paris, --from included, --to excluded, 62 days at most, --to
tomorrow at the latest. The current version's bandwidths unless --bandwidth
names them. The settings default to the platform's: the percentile of each
step, the seconds added to make the client threshold, the target and
unacceptable thresholds as ratios of the client one, the run's as a ratio of
the sum of its steps'. A step without runs keeps its thresholds. Needs
indicators:write. --apply writes the proposals after a confirmation on a
terminal (--yes skips it).")]
    Compute {
        /// Scenario id
        id: String,
        /// First day, YYYY-MM-DD [default: 7 days ago]
        #[arg(long, value_name = "DATE")]
        from: Option<String>,
        /// Day after the last one, YYYY-MM-DD [default: today]
        #[arg(long, value_name = "DATE")]
        to: Option<String>,
        /// Bandwidth id to compute for (repeatable) [default: the current version's]
        #[arg(long, value_name = "ID")]
        bandwidth: Vec<String>,
        /// Percentile of the successful runs read, 0 to 1 (0.85 = the 85th)
        #[arg(long, value_name = "RATIO")]
        percentile: Option<f64>,
        /// Seconds added to the percentile to make the client threshold
        #[arg(long, value_name = "SECONDS")]
        client_constant: Option<f64>,
        /// The target threshold as a ratio of the client one (0.8)
        #[arg(long, value_name = "RATIO")]
        percent_target: Option<f64>,
        /// The unacceptable threshold as a ratio of the client one (1.25)
        #[arg(long, value_name = "RATIO")]
        percent_unacceptable: Option<f64>,
        /// The run's client threshold as a ratio of the sum of its steps' (1)
        #[arg(long, value_name = "RATIO")]
        percent_duration: Option<f64>,
        /// Write the proposals once computed
        #[arg(long)]
        apply: bool,
        /// Do not ask before writing
        #[arg(long, short = 'y', requires = "apply")]
        yes: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Write thresholds from a JSON body (the `apply` of a computation, or hand-written)
    #[command(after_help = "\
Examples:
  2bec scenarios thresholds apply 7f3c... @thresholds.json
  2bec scenarios thresholds compute 7f3c... --json | jq .apply | 2bec scenarios thresholds apply 7f3c... -
  2bec scenarios thresholds apply 7f3c... '{\"bandwidths\":[{\"id\":\"1a2b...\",\"run\":{\"target\":3000,\"client\":6000,\"unacceptable\":12000}}]}'

Body: {bandwidths: [{id, run?: {target, client, unacceptable}, steps?: [{id,
thresholds}]}], computation?}. Milliseconds, target ≤ client ≤ unacceptable.
An omitted run or step keeps its thresholds; a step that has none must be
given. Every bandwidth is written in one transaction: 409 thresholds-changed
when they changed meanwhile, nothing written. Needs indicators:write.")]
    Apply {
        /// Scenario id
        id: String,
        /// JSON body: a literal, `-` for stdin or `@file`
        body: String,
        /// Only write if the thresholds still have this ETag (from `thresholds get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, cmd: SettingsCommand) -> Result<()> {
    match cmd {
        SettingsCommand::Settings { id, output } => get_settings(ctx, &id, output).await,
        SettingsCommand::Edit {
            id,
            name,
            description,
            clear_description,
            catalogue_id,
            clear_catalogue_id,
            if_match,
            output,
        } => {
            let patch = edit_patch(
                name,
                description,
                clear_description,
                catalogue_id,
                clear_catalogue_id,
            )?;
            edit(ctx, &id, &patch, if_match, output).await
        }
        SettingsCommand::RenameSteps {
            id,
            steps,
            if_match,
            output,
        } => {
            let body = steps_write(&steps)?;
            rename_steps(ctx, &id, &body, if_match, output).await
        }
        SettingsCommand::RunVariables {
            id,
            variables,
            clear,
            if_match,
            output,
        } => {
            let body = run_variables_write(&variables, clear)?;
            set_run_variables(ctx, &id, &body, if_match, output).await
        }
        SettingsCommand::Thresholds(args) => match args.command {
            ThresholdsCommand::Get { id, output } => get_thresholds(ctx, &id, output).await,
            ThresholdsCommand::Compute {
                id,
                from,
                to,
                bandwidth,
                percentile,
                client_constant,
                percent_target,
                percent_unacceptable,
                percent_duration,
                apply,
                yes,
                output,
            } => {
                let settings = compute_settings(
                    percentile,
                    client_constant,
                    percent_target,
                    percent_unacceptable,
                    percent_duration,
                )?;
                let body = compute_write(from, to, &bandwidth, settings)?;
                compute(ctx, &id, &body, apply, yes, output).await
            }
            ThresholdsCommand::Apply {
                id,
                body,
                if_match,
                output,
            } => {
                ensure_guid("scenario id", &id)?;
                let body: ThresholdApplyWrite =
                    serde_json::from_value(super::read_json_body(&body)?)
                        .map_err(|e| anyhow::anyhow!("the body is not an apply body: {e}"))?;
                apply_thresholds(ctx, &id, &body, if_match, output).await
            }
        },
    }
}

// ---------- settings ----------

async fn get_settings(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let resp = ctx
        .client()?
        .get(&format!("v2/scenarios/{id}/settings"))
        .await?;
    print_settings(&resp.json()?, resp.etag.as_deref(), &out)
}

/// The merge patch of `scenarios edit`, validated locally.
fn edit_patch(
    name: Option<String>,
    description: Option<String>,
    clear_description: bool,
    catalogue_id: Option<String>,
    clear_catalogue_id: bool,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let mut patch = serde_json::Map::new();
    if let Some(n) = name {
        let n = n.trim().to_string();
        if n.is_empty() {
            bail!("--name cannot be empty");
        }
        if n.chars().count() > settings::MAX_NAME {
            bail!("--name is {} characters at most", settings::MAX_NAME);
        }
        patch.insert("name".into(), n.into());
    }
    if let Some(d) = description {
        patch.insert("description".into(), d.into());
    } else if clear_description {
        patch.insert("description".into(), serde_json::Value::Null);
    }
    if let Some(c) = catalogue_id {
        ensure_guid("--catalogue-id", &c)?;
        patch.insert("catalogueId".into(), c.into());
    } else if clear_catalogue_id {
        patch.insert("catalogueId".into(), serde_json::Value::Null);
    }
    if patch.is_empty() {
        bail!(
            "nothing to change: give --name, --description, --clear-description, --catalogue-id or --clear-catalogue-id"
        );
    }
    Ok(patch)
}

async fn edit(
    ctx: &Context,
    id: &str,
    patch: &serde_json::Map<String, serde_json::Value>,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .patch_merge(&format!("v2/scenarios/{id}"), patch, &write)
        .await?;
    print_settings(&resp.json()?, resp.etag.as_deref(), &out)
}

/// `STEP=NAME` arguments as the steps body.
fn steps_write(args: &[String]) -> Result<StepsWrite> {
    let mut steps: Vec<StepNameWrite> = Vec::new();
    for a in args {
        let Some((id, name)) = a.split_once('=') else {
            bail!("`{a}` must be STEP_ID=NAME");
        };
        ensure_guid("step id", id)?;
        let name = name.trim();
        if name.is_empty() {
            bail!("the name of step {id} cannot be empty");
        }
        if name.chars().count() > settings::MAX_NAME {
            bail!("a step name is {} characters at most", settings::MAX_NAME);
        }
        if steps.iter().any(|s| s.id.eq_ignore_ascii_case(id)) {
            bail!("step {id} is given twice");
        }
        steps.push(StepNameWrite {
            id: id.to_string(),
            name: name.to_string(),
        });
    }
    Ok(StepsWrite { steps })
}

async fn rename_steps(
    ctx: &Context,
    id: &str,
    body: &StepsWrite,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .put_json(&format!("v2/scenarios/{id}/steps"), body, &write)
        .await?;
    print_settings(&resp.json()?, resp.etag.as_deref(), &out)
}

/// `NAME[=DEFAULT]` arguments as the run variables body, checked like the
/// API does.
fn run_variables_write(args: &[String], clear: bool) -> Result<RunVariablesWrite> {
    if args.is_empty() && !clear {
        bail!("name the variables to keep, or pass --clear to remove them all");
    }
    if args.len() > settings::MAX_RUN_VARIABLES {
        bail!("at most {} run variables", settings::MAX_RUN_VARIABLES);
    }
    let mut variables: Vec<RequestVariable> = Vec::new();
    for a in args {
        let (name, default) = match a.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        let first_ok = name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        let rest_ok = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !first_ok || !rest_ok || name.len() > settings::MAX_VARIABLE_NAME {
            bail!(
                "variable name `{name}` must match [A-Za-z_][A-Za-z0-9_]* and be at most {} characters",
                settings::MAX_VARIABLE_NAME
            );
        }
        if name.starts_with("SCHEDULER_") {
            bail!("variable name `{name}` is reserved (SCHEDULER_ prefix)");
        }
        if let Some(v) = &default
            && (v.chars().count() > settings::MAX_VARIABLE_VALUE || v.chars().any(char::is_control))
        {
            bail!(
                "the default of `{name}` must be at most {} characters without control characters",
                settings::MAX_VARIABLE_VALUE
            );
        }
        if variables.iter().any(|v| v.name == name) {
            bail!("variable `{name}` is given twice");
        }
        variables.push(RequestVariable {
            name: name.to_string(),
            default_value: default,
        });
    }
    Ok(RunVariablesWrite { variables })
}

async fn set_run_variables(
    ctx: &Context,
    id: &str,
    body: &RunVariablesWrite,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .put_json(&format!("v2/scenarios/{id}/run-variables"), body, &write)
        .await?;
    print_settings(&resp.json()?, resp.etag.as_deref(), &out)
}

fn print_settings(s: &ScenarioSettings, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(s)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!("Scenario:     {}", s.id);
    println!("Name:         {}", s.name);
    println!("Description:  {}", or_dash(s.description.as_deref()));
    println!("Catalogue id: {}", or_dash(s.catalogue_id.as_deref()));
    println!(
        "Updated:      {}",
        s.updated_at.as_deref().map_or("-".to_string(), short_time)
    );
    if let Some(e) = etag {
        println!("ETag:         {e}");
    }
    if !s.steps.is_empty() {
        println!();
        let mut t = Table::new(["#", "STEP ID", "NAME", "MEASURED"]);
        for st in &s.steps {
            t.row([
                st.order.to_string(),
                st.id.clone(),
                st.name.clone(),
                if st.measured { "yes" } else { "no" }.to_string(),
            ]);
        }
        t.print();
    }
    println!();
    if s.run_variables.is_empty() {
        println!("Run variables: none");
    } else {
        println!("Run variables:");
        let mut t = Table::new(["NAME", "DEFAULT"]);
        for v in &s.run_variables {
            t.row([v.name.clone(), or_dash(v.default_value.as_deref())]);
        }
        t.print();
    }
    Ok(())
}

// ---------- thresholds ----------

async fn get_thresholds(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let resp = ctx
        .client()?
        .get(&format!("v2/scenarios/{id}/thresholds"))
        .await?;
    print_thresholds(&resp.json()?, resp.etag.as_deref(), &out)
}

/// The computation settings when any flag names one; the platform's otherwise.
fn compute_settings(
    percentile: Option<f64>,
    client_constant: Option<f64>,
    percent_target: Option<f64>,
    percent_unacceptable: Option<f64>,
    percent_duration: Option<f64>,
) -> Result<Option<ThresholdSettings>> {
    let flags = [
        ("--percentile", percentile),
        ("--client-constant", client_constant),
        ("--percent-target", percent_target),
        ("--percent-unacceptable", percent_unacceptable),
        ("--percent-duration", percent_duration),
    ];
    if flags.iter().all(|(_, v)| v.is_none()) {
        return Ok(None);
    }
    for (flag, v) in &flags {
        match v {
            Some(v) if !v.is_finite() || *v < 0.0 => bail!("{flag} must be a positive number"),
            _ => {}
        }
    }
    if let Some(p) = percentile
        && !(0.0..=1.0).contains(&p)
    {
        bail!("--percentile is a ratio between 0 and 1");
    }
    Ok(Some(ThresholdSettings {
        percentile: percentile.unwrap_or(DEFAULT_SETTINGS.percentile),
        client_constant_seconds: client_constant
            .unwrap_or(DEFAULT_SETTINGS.client_constant_seconds),
        percent_target: percent_target.unwrap_or(DEFAULT_SETTINGS.percent_target),
        percent_unacceptable: percent_unacceptable.unwrap_or(DEFAULT_SETTINGS.percent_unacceptable),
        percent_duration: percent_duration.unwrap_or(DEFAULT_SETTINGS.percent_duration),
    }))
}

/// The platform's computation settings, filled in for the flags not given.
const DEFAULT_SETTINGS: ThresholdSettings = ThresholdSettings {
    percentile: 0.85,
    client_constant_seconds: 1.0,
    percent_target: 0.8,
    percent_unacceptable: 1.25,
    percent_duration: 1.0,
};

/// Days of the default computation period.
const DEFAULT_DAYS: i64 = 7;

/// The compute body: the period checked for shape and length locally.
fn compute_write(
    from: Option<String>,
    to: Option<String>,
    bandwidths: &[String],
    settings: Option<ThresholdSettings>,
) -> Result<ThresholdComputeWrite> {
    let today = days_since_epoch_today();
    let to = match to {
        Some(t) => t,
        None => civil(today),
    };
    let from = match from {
        Some(f) => f,
        None => civil(parse_day("--to", &to)? - DEFAULT_DAYS),
    };
    let (f, t) = (parse_day("--from", &from)?, parse_day("--to", &to)?);
    if t <= f {
        bail!("--to must be after --from (--to is excluded)");
    }
    if t - f > settings::MAX_COMPUTE_DAYS {
        bail!("the period is {} days at most", settings::MAX_COMPUTE_DAYS);
    }
    for b in bandwidths {
        ensure_guid("--bandwidth", b)?;
    }
    Ok(ThresholdComputeWrite {
        from,
        to,
        settings,
        bandwidths: if bandwidths.is_empty() {
            None
        } else {
            Some(bandwidths.to_vec())
        },
    })
}

/// `YYYY-MM-DD` as days since 1970-01-01.
fn parse_day(flag: &str, s: &str) -> Result<i64> {
    let parts: Vec<&str> = s.split('-').collect();
    let bad = || anyhow::anyhow!("{flag} must be a date like 2026-10-01, got `{s}`");
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return Err(bad());
    }
    let y: i64 = parts[0].parse().map_err(|_| bad())?;
    let m: i64 = parts[1].parse().map_err(|_| bad())?;
    let d: i64 = parts[2].parse().map_err(|_| bad())?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(bad());
    }
    Ok(days_from_civil(y, m, d))
}

/// Days since the epoch of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DD` of a day count since the epoch.
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today in UTC as days since the epoch.
fn days_since_epoch_today() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    (secs / 86_400) as i64
}

async fn compute(
    ctx: &Context,
    id: &str,
    body: &ThresholdComputeWrite,
    apply: bool,
    yes: bool,
    out: OutputArgs,
) -> Result<()> {
    ensure_guid("scenario id", id)?;
    let client = ctx.client()?;
    let c: ThresholdComputation = client
        .post_json(
            &format!("v2/scenarios/{id}/thresholds:compute"),
            body,
            &WriteOptions::idempotent(),
        )
        .await?
        .json()?;
    if !apply {
        if out.wants_json() {
            return out.print_json(&c);
        }
        print_computation(&c);
        return Ok(());
    }
    if c.apply.bandwidths.is_empty() {
        bail!("nothing to apply: no bandwidth had a proposal");
    }
    if !out.wants_json() {
        print_computation(&c);
        println!();
    }
    if !yes && !confirm("Apply these thresholds? [y/N] ")? {
        bail!("not confirmed; pass --yes to skip the question");
    }
    apply_thresholds(ctx, id, &c.apply, None, out).await
}

async fn apply_thresholds(
    ctx: &Context,
    id: &str,
    body: &ThresholdApplyWrite,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    if body.bandwidths.is_empty() {
        bail!("the body names no bandwidth");
    }
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .post_json(&format!("v2/scenarios/{id}/thresholds:apply"), body, &write)
        .await?;
    print_thresholds(&resp.json()?, resp.etag.as_deref(), &out)
}

fn print_thresholds(t: &ScenarioThresholds, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut v = serde_json::to_value(t)?;
        if let Some(e) = etag {
            v["etag"] = e.into();
        }
        return out.print_json(&v);
    }
    println!("Scenario:    {} ({})", t.scenario.name, t.scenario.id);
    let q = &t.quality;
    println!(
        "Quality:     completion {}/{}/{} failed runs, regularity {}/{}/{} %, restoration {}/{}/{} min{}",
        q.completion.target,
        q.completion.client,
        q.completion.unacceptable,
        q.regularity.target,
        q.regularity.client,
        q.regularity.unacceptable,
        q.restoration.target,
        q.restoration.client,
        q.restoration.unacceptable,
        if q.is_default {
            " (platform defaults)"
        } else {
            ""
        }
    );
    if let Some(e) = etag {
        println!("ETag:        {e}");
    }
    for v in &t.versions {
        println!();
        println!(
            "Version {}{} from {}{}{}",
            v.id,
            if v.current { " (current)" } else { "" },
            short_time(&v.begin),
            v.end
                .as_deref()
                .map_or(String::new(), |e| format!(" to {}", short_time(e))),
            v.comment
                .as_deref()
                .map_or(String::new(), |c| format!(": {c}"))
        );
        let mut table = Table::new([
            "ID",
            "BANDWIDTH",
            "STEP",
            "TARGET",
            "CLIENT",
            "UNACCEPTABLE",
        ]);
        for b in &v.bandwidths {
            let label = format!(
                "{} ({} kB/s){}",
                b.bandwidth,
                b.kilo_bytes_per_second,
                if b.preferred { " *" } else { "" }
            );
            table.row(threshold_row(&b.id, &label, "run", b.run.as_ref()));
            for (i, st) in v.steps.iter().enumerate() {
                let th = b.steps.get(i).and_then(|t| t.as_ref());
                table.row(threshold_row(&st.id, "", &st.label, th));
            }
        }
        table.print();
    }
    Ok(())
}

fn threshold_row(id: &str, bandwidth: &str, step: &str, t: Option<&Thresholds>) -> [String; 6] {
    [
        id.to_string(),
        bandwidth.to_string(),
        step.to_string(),
        t.map_or("-".into(), |t| ms(t.target)),
        t.map_or("-".into(), |t| ms(t.client)),
        t.map_or("-".into(), |t| ms(t.unacceptable)),
    ]
}

fn print_computation(c: &ThresholdComputation) {
    println!(
        "Period:      {} to {} (excluded), whole days in Paris",
        c.from, c.to
    );
    let s = &c.settings;
    println!(
        "Settings:    P{:.0} + {}s = client; target x{}, unacceptable x{}; run x{} of the steps' sum",
        s.percentile * 100.0,
        s.client_constant_seconds,
        s.percent_target,
        s.percent_unacceptable,
        s.percent_duration
    );
    for v in &c.versions {
        for b in &v.bandwidths {
            println!();
            println!(
                "Version {} ({} successful runs), bandwidth {} ({})",
                v.id, v.runs, b.bandwidth, b.id
            );
            let mut t = Table::new([
                "STEP",
                "SAMPLES",
                "PERCENTILE",
                "CURRENT",
                "PROPOSED",
                "NOTE",
            ]);
            t.row(proposal_row("run", None, &b.run));
            for st in &b.steps {
                t.row(proposal_row(&st.name, st.percentile, &st.proposal));
            }
            t.print();
            if let (Some(now), Some(then)) = (&b.run.current_bands, &b.run.proposed_bands) {
                println!(
                    "Run bands:   now {} target / {} client / {} insufficient / {} unacceptable; proposed {} / {} / {} / {}",
                    now.target,
                    now.client,
                    now.insufficient,
                    now.unacceptable,
                    then.target,
                    then.client,
                    then.insufficient,
                    then.unacceptable
                );
            }
        }
    }
    if c.apply.bandwidths.is_empty() {
        println!();
        println!("Nothing to apply.");
    }
}

fn proposal_row(step: &str, percentile: Option<f64>, p: &ThresholdProposal) -> [String; 5] {
    [
        step.to_string(),
        p.samples.to_string(),
        percentile.map_or("-".into(), ms),
        p.proposed.as_ref().map_or("-".into(), fmt_thresholds),
        if p.kept_current {
            "kept current (no sample)"
        } else if p.proposed.is_none() {
            "nothing to set"
        } else {
            ""
        }
        .to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "7f3c1a2b-0000-4000-8000-000000000000";
    const H: &str = "1a2b3c4d-0000-4000-8000-000000000000";

    #[test]
    fn edit_patch_clears_with_null() {
        let p = edit_patch(Some(" Login ".into()), None, true, None, true).unwrap();
        assert_eq!(
            serde_json::Value::Object(p),
            serde_json::json!({"name": "Login", "description": null, "catalogueId": null})
        );
        assert!(edit_patch(None, None, false, None, false).is_err());
        assert!(edit_patch(Some("  ".into()), None, false, None, false).is_err());
        assert!(edit_patch(Some("x".repeat(51)), None, false, None, false).is_err());
        assert!(edit_patch(None, None, false, Some("nope".into()), false).is_err());
    }

    #[test]
    fn steps_write_parses_pairs() {
        let w = steps_write(&[format!("{G}=Home"), format!("{H}= Login ")]).unwrap();
        assert_eq!(w.steps[1].name, "Login");
        assert!(steps_write(&["nope".into()]).is_err());
        assert!(steps_write(&[format!("{G}=")]).is_err());
        assert!(steps_write(&[format!("{G}=a"), format!("{G}=b")]).is_err());
    }

    #[test]
    fn run_variables_follow_the_api_rules() {
        let w = run_variables_write(&["USER=alice".into(), "ENV".into()], false).unwrap();
        assert_eq!(w.variables[0].default_value.as_deref(), Some("alice"));
        assert_eq!(w.variables[1].default_value, None);
        assert!(run_variables_write(&[], true).unwrap().variables.is_empty());
        assert!(run_variables_write(&[], false).is_err());
        assert!(run_variables_write(&["1X=1".into()], false).is_err());
        assert!(run_variables_write(&["SCHEDULER_X".into()], false).is_err());
        assert!(run_variables_write(&["A".into(), "A=1".into()], false).is_err());
        assert!(run_variables_write(&[format!("A={}", "v".repeat(4001))], false).is_err());
    }

    #[test]
    fn compute_period_is_checked() {
        let w = compute_write(
            Some("2026-09-01".into()),
            Some("2026-10-01".into()),
            &[G.into()],
            None,
        )
        .unwrap();
        assert_eq!(w.bandwidths.as_deref(), Some(&[G.to_string()][..]));
        assert!(
            compute_write(
                Some("2026-10-01".into()),
                Some("2026-10-01".into()),
                &[],
                None
            )
            .is_err()
        );
        assert!(
            compute_write(
                Some("2026-07-01".into()),
                Some("2026-10-01".into()),
                &[],
                None
            )
            .is_err()
        );
        assert!(compute_write(Some("2026/07/01".into()), None, &[], None).is_err());
        let w = compute_write(None, Some("2026-10-12".into()), &[], None).unwrap();
        assert_eq!(w.from, "2026-10-05");
    }

    #[test]
    fn civil_dates_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil(0), "1970-01-01");
        for (y, m, d) in [(2026, 10, 12), (2024, 2, 29), (2000, 3, 1), (1999, 12, 31)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil(days), format!("{y:04}-{m:02}-{d:02}"));
        }
        assert_eq!(civil(days_from_civil(2026, 3, 1) - 1), "2026-02-28");
    }

    #[test]
    fn settings_fill_the_defaults() {
        assert!(
            compute_settings(None, None, None, None, None)
                .unwrap()
                .is_none()
        );
        let s = compute_settings(Some(0.9), None, None, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(s.percentile, 0.9);
        assert_eq!(s.percent_target, 0.8);
        assert!(compute_settings(Some(1.5), None, None, None, None).is_err());
        assert!(compute_settings(None, Some(-1.0), None, None, None).is_err());
    }
}
