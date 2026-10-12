//! `2bec variables`: the space's runtime variables, the named values the
//! alert hooks receive as environment variables.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::runs::{ensure_guid, read_text};
use crate::api::page::Page;
use crate::api::variables::{self, Variable, VariableWrite};
use crate::http::WriteOptions;
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
#[command(after_help = "\
Reads need scenarios:read, writes scenarios:write. A secret's value is
stored ciphered and never returned.")]
pub struct VariablesArgs {
    #[command(subcommand)]
    command: VariablesCommand,
}

#[derive(Subcommand, Debug)]
enum VariablesCommand {
    /// List the variables by name, with their value (plain ones) and when they last changed
    #[command(after_help = "\
Examples:
  2bec variables list
  2bec variables list --secret -q token
  2bec variables list --plain --json")]
    List {
        /// Only the secret variables
        #[arg(long, conflicts_with = "plain")]
        secret: bool,
        /// Only the plain variables
        #[arg(long)]
        plain: bool,
        /// Only variables whose name matches these words
        #[arg(long, short = 'q', value_name = "WORDS")]
        search: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one variable with its value history and what reads it
    Get {
        /// Variable id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Create a variable or change its value or comment
    #[command(after_help = "\
Examples:
  2bec variables set SMTP_HOST smtp.example.com --comment \"Outgoing mail\"
  2bec variables set API_TOKEN - --secret < token.txt       # the value from stdin, never on the command line
  2bec variables set API_TOKEN @token.txt --secret
  2bec variables set SMTP_HOST --comment \"\"                 # clears the comment, keeps the value
  2bec variables set SMTP_HOST mail.example.com --if-match '\"v3\"'

The name holds letters, digits and underscores (128 at most) and is unique
in the space whatever its case. A value is required to create; omitted on a
change, the stored one stays. --secret is fixed at the creation (409
variable-kind-fixed later). Every new value is one more version of the
history. 422 refused when secrets cannot be written at the moment.")]
    Set {
        /// Variable name
        name: String,
        /// The value: a literal, `-` for stdin or `@file`; omitted, the stored value stays
        value: Option<String>,
        /// Store the value ciphered, never returned (creation only)
        #[arg(long)]
        secret: bool,
        /// Comment; an empty value clears it
        #[arg(long, value_name = "TEXT")]
        comment: Option<String>,
        /// Only change if the variable still has this ETag (from `variables get`)
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Delete a variable and every stored value
    #[command(
        alias = "rm",
        after_help = "\
Examples:
  2bec variables delete SMTP_HOST
  2bec variables delete OLD_TOKEN --force      # even if an alert hook or a forward reads it

409 variable-in-use without --force when something is known to read it."
    )]
    Delete {
        /// Variable name
        name: String,
        /// Delete even when an alert hook or a forward reads it
        #[arg(long)]
        force: bool,
        /// Only delete if the variable still has this ETag
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
}

pub async fn run(ctx: &Context, args: VariablesArgs) -> Result<()> {
    match args.command {
        VariablesCommand::List {
            secret,
            plain,
            search,
            output,
        } => list(ctx, secret, plain, search.as_deref(), output).await,
        VariablesCommand::Get { id, output } => get(ctx, &id, output).await,
        VariablesCommand::Set {
            name,
            value,
            secret,
            comment,
            if_match,
            output,
        } => {
            let name = checked_name(&name)?;
            let body = write_body(value.as_deref(), secret, comment)?;
            set(ctx, &name, &body, if_match, output).await
        }
        VariablesCommand::Delete {
            name,
            force,
            if_match,
        } => {
            let name = checked_name(&name)?;
            delete(ctx, &name, force, if_match).await
        }
    }
}

fn checked_name(name: &str) -> Result<String> {
    let name = name.trim();
    if let Some(err) = variables::check_name(name) {
        bail!("name `{name}`: {err}");
    }
    Ok(name.to_string())
}

/// The write body from the arguments, the value read from stdin or a file
/// when asked, checked like the API does.
fn write_body(value: Option<&str>, secret: bool, comment: Option<String>) -> Result<VariableWrite> {
    let value = match value {
        Some(v) => {
            let v = read_text(v)?;
            if let Some(err) = variables::check_value(&v) {
                bail!("value: {err}");
            }
            Some(v)
        }
        None => None,
    };
    let comment = match comment {
        Some(c) => {
            let c = c.trim().to_string();
            if c.chars().count() > variables::MAX_COMMENT {
                bail!("--comment is {} characters at most", variables::MAX_COMMENT);
            }
            Some(if c.is_empty() { None } else { Some(c) })
        }
        None => None,
    };
    if value.is_none() && comment.is_none() {
        bail!("nothing to write: give a value, --comment or both");
    }
    Ok(VariableWrite {
        secret: if secret { Some(true) } else { None },
        value,
        comment,
    })
}

async fn list(
    ctx: &Context,
    secret: bool,
    plain: bool,
    search: Option<&str>,
    out: OutputArgs,
) -> Result<()> {
    let mut query = Vec::new();
    if secret {
        query.push(("secret", "true".to_string()));
    } else if plain {
        query.push(("secret", "false".to_string()));
    }
    if let Some(q) = search
        && !q.trim().is_empty()
    {
        query.push(("q", q.trim().to_string()));
    }
    let page: Page<Variable> = ctx
        .client()?
        .get_with("v2/variables", &query)
        .await?
        .json()?;
    out.print(&page.items, "No variable", || {
        let mut t = Table::new([
            "ID", "NAME", "KIND", "VALUE", "VERSIONS", "UPDATED", "COMMENT",
        ]);
        for v in &page.items {
            t.row([
                v.id.clone(),
                v.name.clone(),
                kind(v.secret).to_string(),
                value_cell(v),
                v.versions.to_string(),
                v.updated_at.as_deref().map_or("-".to_string(), short_time),
                or_dash(v.comment.as_deref()),
            ]);
        }
        t
    })
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("variable id", id)?;
    let resp = ctx.client()?.get(&format!("v2/variables/{id}")).await?;
    print_variable(&resp.json()?, resp.etag.as_deref(), &out)
}

async fn set(
    ctx: &Context,
    name: &str,
    body: &VariableWrite,
    if_match: Option<String>,
    out: OutputArgs,
) -> Result<()> {
    let write = WriteOptions {
        idempotency_key: None,
        if_match,
    };
    let resp = ctx
        .client()?
        .put_json(&format!("v2/variables/{name}"), body, &write)
        .await?;
    let v: Variable = resp.json()?;
    if resp.status == 201 {
        eprintln!("Created variable {} ({})", v.name, v.id);
    } else {
        eprintln!("Updated variable {} ({})", v.name, v.id);
    }
    print_variable(&v, resp.etag.as_deref(), &out)
}

async fn delete(ctx: &Context, name: &str, force: bool, if_match: Option<String>) -> Result<()> {
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
        .delete_with(&format!("v2/variables/{name}"), query, &write)
        .await?;
    eprintln!("Deleted variable {name}");
    Ok(())
}

fn print_variable(v: &Variable, etag: Option<&str>, out: &OutputArgs) -> Result<()> {
    if out.wants_json() {
        let mut j = serde_json::to_value(v)?;
        if let Some(e) = etag {
            j["etag"] = e.into();
        }
        return out.print_json(&j);
    }
    println!("Variable:  {}", v.id);
    println!("Name:      {} (env {})", v.name, v.env_name);
    println!("Kind:      {}", kind(v.secret));
    println!("Value:     {}", value_cell(v));
    println!("Comment:   {}", or_dash(v.comment.as_deref()));
    println!("Created:   {}", short_time(&v.created_at));
    println!(
        "Updated:   {}{}",
        v.updated_at.as_deref().map_or("-".to_string(), short_time),
        v.updated_by
            .as_deref()
            .map_or(String::new(), |b| format!(" by {b}"))
    );
    println!("Versions:  {}", v.versions);
    if let Some(e) = etag {
        println!("ETag:      {e}");
    }
    if let Some(readers) = &v.read_by {
        println!(
            "Read by:   {}",
            if readers.is_empty() {
                "nothing known".to_string()
            } else {
                readers
                    .iter()
                    .map(|r| {
                        let mut s = r.kind.clone();
                        if let Some(h) = &r.hook {
                            s.push_str(&format!(" {h}"));
                        }
                        if let Some(l) = &r.login {
                            s.push_str(&format!(" ({l})"));
                        }
                        s
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
    }
    if let Some(history) = &v.history
        && !history.is_empty()
    {
        println!();
        println!("History:");
        let mut t = Table::new(["AT", "BY", "VALUE"]);
        for h in history {
            t.row([
                h.at.as_deref().map_or("-".to_string(), short_time),
                or_dash(h.by.as_deref()),
                if v.secret {
                    "(secret)".to_string()
                } else {
                    or_dash(h.value.as_deref())
                },
            ]);
        }
        t.print();
    }
    Ok(())
}

fn kind(secret: bool) -> &'static str {
    if secret { "secret" } else { "plain" }
}

fn value_cell(v: &Variable) -> String {
    if v.secret {
        "(secret)".to_string()
    } else {
        or_dash(v.value.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert_eq!(checked_name(" SMTP_HOST ").unwrap(), "SMTP_HOST");
        assert!(checked_name("smtp-host").is_err());
        assert!(checked_name("").is_err());
    }

    #[test]
    fn body_from_arguments() {
        let b = write_body(Some("v"), true, Some(" note ".into())).unwrap();
        assert_eq!(
            serde_json::to_value(&b).unwrap(),
            serde_json::json!({"secret": true, "value": "v", "comment": "note"})
        );
        let b = write_body(None, false, Some(String::new())).unwrap();
        assert_eq!(
            serde_json::to_value(&b).unwrap(),
            serde_json::json!({"comment": null})
        );
        let b = write_body(Some("v"), false, None).unwrap();
        assert_eq!(
            serde_json::to_value(&b).unwrap(),
            serde_json::json!({"value": "v"})
        );
        assert!(write_body(None, false, None).is_err());
        assert!(write_body(Some("  "), false, None).is_err());
        assert!(write_body(None, false, Some("c".repeat(1001))).is_err());
    }
}
