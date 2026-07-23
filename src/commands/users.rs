//! `2bec users`: the users of the space. Read only.

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::Context;
use super::runs::ensure_guid;
use crate::api::page::{Fetch, fetch_items};
use crate::api::people::{self, User};
use crate::output::{OutputArgs, Table, or_dash, short_time};

#[derive(Args, Debug)]
#[command(after_help = "\
Needs users:read. Deleted accounts are never listed; the writes stay on the
web app's users page.")]
pub struct UsersArgs {
    #[command(subcommand)]
    command: UsersCommand,
}

#[derive(Subcommand, Debug)]
enum UsersCommand {
    /// List the users by name with their roles, access profile and last activity
    #[command(after_help = "\
Examples:
  2bec users list
  2bec users list --status locked --status disabled
  2bec users list --role \"User Admin\" --role \"API Admin\"    # one of the roles; account admins hold every role
  2bec users list --profile <profile-id> -q \"smith ops\"
  2bec users list --all --json | jq -r '.[] | [.login, .email] | @tsv'")]
    List {
        /// Only users in one of these statuses (repeatable)
        #[arg(long, value_name = "STATUS", value_parser = people::STATUSES)]
        status: Vec<String>,
        /// Only users holding one of these roles (repeatable)
        #[arg(long, value_name = "ROLE", value_parser = people::ROLES)]
        role: Vec<String>,
        /// Only the holders of this access profile
        #[arg(long, value_name = "ID")]
        profile: Option<String>,
        /// Only users whose name, login, e-mail or job title holds every word
        #[arg(long, short = 'q', value_name = "WORDS")]
        search: Option<String>,
        /// How many users to show at most
        #[arg(long, default_value_t = 50, conflicts_with = "all")]
        limit: u64,
        /// Every user, following the pages to the end
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show one user with their sign-in details
    Get {
        /// User id
        id: String,
        #[command(flatten)]
        output: OutputArgs,
    },
}

pub async fn run(ctx: &Context, args: UsersArgs) -> Result<()> {
    match args.command {
        UsersCommand::List {
            status,
            role,
            profile,
            search,
            limit,
            all,
            output,
        } => {
            list(
                ctx,
                &status,
                &role,
                profile.as_deref(),
                search.as_deref(),
                limit,
                all,
                output,
            )
            .await
        }
        UsersCommand::Get { id, output } => get(ctx, &id, output).await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn list(
    ctx: &Context,
    status: &[String],
    role: &[String],
    profile: Option<&str>,
    search: Option<&str>,
    limit: u64,
    all: bool,
    out: OutputArgs,
) -> Result<()> {
    if !all && limit == 0 {
        bail!("--limit must be at least 1");
    }
    let mut query: Vec<(&str, String)> = Vec::new();
    for s in status {
        query.push(("status", s.clone()));
    }
    for r in role {
        query.push(("role", r.clone()));
    }
    if let Some(p) = profile {
        ensure_guid("--profile", p)?;
        query.push(("profile", p.to_string()));
    }
    if let Some(q) = search
        && !q.trim().is_empty()
    {
        query.push(("q", q.trim().to_string()));
    }
    let fetch = if all { Fetch::All } else { Fetch::Limit(limit) };
    let page = fetch_items::<User>(&ctx.client()?, "v2/users", &query, fetch).await?;
    out.print(&page.items, "No user", || {
        let mut t = Table::new([
            "ID",
            "LOGIN",
            "NAME",
            "EMAIL",
            "STATUS",
            "ROLES",
            "PROFILE",
            "LAST ACTIVITY",
        ]);
        for u in &page.items {
            t.row([
                u.id.clone(),
                u.login.clone(),
                u.display_name.clone(),
                or_dash(u.email.as_deref()),
                u.status.clone(),
                roles(u),
                u.profile.name.clone(),
                u.last_activity_at
                    .as_deref()
                    .map_or("-".to_string(), short_time),
            ]);
        }
        t
    })?;
    if !out.wants_json()
        && let Some(total) = page.total
        && (total as usize) > page.items.len()
    {
        eprintln!(
            "Showing {} of {total} users; use --limit or --all for more",
            page.items.len()
        );
    }
    Ok(())
}

async fn get(ctx: &Context, id: &str, out: OutputArgs) -> Result<()> {
    ensure_guid("user id", id)?;
    let u: User = ctx.client()?.get(&format!("v2/users/{id}")).await?.json()?;
    if out.wants_json() {
        return out.print_json(&u);
    }
    println!("User:           {}", u.id);
    println!("Login:          {}", u.login);
    println!("Name:           {}", u.display_name);
    println!("E-mail:         {}", or_dash(u.email.as_deref()));
    println!("Job title:      {}", or_dash(u.job_title.as_deref()));
    println!(
        "Phone:          {} / mobile {}",
        or_dash(u.phone.as_deref()),
        or_dash(u.mobile.as_deref())
    );
    println!(
        "Status:         {}{}",
        u.status,
        u.locked_at
            .as_deref()
            .map_or(String::new(), |l| format!(" since {}", short_time(l)))
    );
    println!("Roles:          {}", roles(&u));
    println!("Profile:        {} ({})", u.profile.name, u.profile.id);
    println!(
        "Sign-in:        {}",
        match (u.external_auth, u.external_auth_only) {
            (true, Some(true)) => "identity provider only".to_string(),
            (true, _) => "password or identity provider".to_string(),
            (false, _) => "password".to_string(),
        }
    );
    if let Some(exempt) = u.two_factor_exempt {
        println!(
            "Strong auth:    {}",
            if exempt {
                "exempted"
            } else {
                "as the policy says"
            }
        );
    }
    if let Some(l) = &u.language {
        println!("Language:       {l}");
    }
    println!(
        "Created:        {}",
        u.created_at.as_deref().map_or("-".to_string(), short_time)
    );
    println!(
        "Last sign-in:   {}",
        u.last_sign_in_at
            .as_deref()
            .map_or("-".to_string(), short_time)
    );
    println!(
        "Last activity:  {}",
        u.last_activity_at
            .as_deref()
            .map_or("-".to_string(), short_time)
    );
    Ok(())
}

/// `account admin` or the granted roles, `-` for a plain user.
fn roles(u: &User) -> String {
    if u.account_admin {
        "account admin".to_string()
    } else if u.roles.is_empty() {
        "-".to_string()
    } else {
        u.roles.join(", ")
    }
}
