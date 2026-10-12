//! `2bec profiles`: the access profiles deciding which scenarios the users
//! see. Read only.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::runs::ensure_guid;
use crate::api::page::{Fetch, fetch_items};
use crate::api::people::Profile;
use crate::output::{OutputArgs, Table};

#[derive(Args, Debug)]
#[command(after_help = "\
Needs users:read. A profile is linked to scenario groups; a link with access
opens the group's scenarios to the profile's users, a closed one is kept on
purpose. The SSO default profile is the one a first sign-in through the
company's identity provider gets.")]
pub struct ProfilesArgs {
    #[command(subcommand)]
    command: ProfilesCommand,
}

#[derive(Subcommand, Debug)]
enum ProfilesCommand {
    /// List the access profiles by name with their groups, scenarios and holders
    #[command(after_help = "\
Examples:
  2bec profiles list
  2bec profiles list --all --json | jq '.[] | select(.ssoDefault)'")]
    List {
        /// How many profiles to show at most
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Every profile, following the pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one profile with its groups and the users holding it
    Get {
        /// Profile id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: ProfilesArgs) -> Result<()> {
    match args.command {
        ProfilesCommand::List { limit, all, output } => list(ctx, limit, all, output).await,
        ProfilesCommand::Get { id, output } => get(ctx, &id, output).await,
    }
}

async fn list(ctx: &Context, limit: u64, all: bool, out: OutputArgs) -> Result<()> {
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page = fetch_items::<Profile>(&ctx.client()?, "v2/profiles", &[], fetch).await?;
    out.print(&page.items, "No access profile", || {
        let mut t = Table::new(["ID", "NAME", "SSO DEFAULT", "GROUPS", "SCENARIOS", "USERS"]);
        for p in &page.items {
            t.row([
                p.id.clone(),
                p.name.clone(),
                if p.sso_default { "yes" } else { "-" }.to_string(),
                groups(p),
                p.scenario_count.to_string(),
                p.member_count().to_string(),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} profiles; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("profile id", id)?;
    let p: Profile = ctx
        .client()?
        .get(&format!("v2/profiles/{id}"))
        .await?
        .json()?;
    if out.wants_json() {
        return out.print_json(&p);
    }
    println!("Profile:      {}", p.id);
    println!("Name:         {}", p.name);
    println!("SSO default:  {}", if p.sso_default { "yes" } else { "no" });
    println!("Scenarios:    {}", p.scenario_count);
    println!("Users:        {}", p.member_count());
    if !p.groups.is_empty() {
        println!();
        println!("Groups:");
        let mut t = Table::new(["ID", "NAME", "ACCESS"]);
        for g in &p.groups {
            t.row([
                g.group.id.clone(),
                g.group.name.clone(),
                if g.access { "open" } else { "closed" }.to_string(),
            ]);
        }
        t.print();
    }
    if let Some(members) = &p.members
        && !members.is_empty()
    {
        println!();
        println!("Members:");
        let mut t = Table::new(["ID", "LOGIN", "NAME", "STATUS"]);
        for m in members {
            t.row([
                m.id.clone(),
                m.login.clone(),
                m.display_name.clone(),
                m.status.clone(),
            ]);
        }
        t.print();
    }
    Ok(())
}

/// The open groups by name, a closed one in brackets.
fn groups(p: &Profile) -> String {
    if p.groups.is_empty() {
        return "-".to_string();
    }
    p.groups
        .iter()
        .map(|g| {
            if g.access {
                g.group.name.clone()
            } else {
                format!("[{}]", g.group.name)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
