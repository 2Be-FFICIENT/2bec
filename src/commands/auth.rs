//! `2bec auth`: log in with an API key, inspect or drop the credential.

use anyhow::{Context as _, Result, bail};
use clap::{Args, Subcommand};
use std::io::{IsTerminal, Read, Write};

use super::Context;
use crate::api::me::{self, Me};
use crate::auth::store::CredentialStore;
use crate::auth::token::{self, TokenKind};
use crate::auth::{Credential, TokenSource};

#[derive(Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Subcommand, Debug)]
enum AuthCommand {
    /// Store an API key for the active profile after verifying it
    #[command(after_help = "\
Examples:
  2bec auth login                      # prompts for the key (hidden)
  echo \"$KEY\" | 2bec auth login       # reads it from stdin
  2bec --profile dev auth login --with-token < key.txt

An API Admin creates keys in the web app under Administration > API access;
the secret is shown once.")]
    Login {
        /// Read the key from stdin instead of prompting
        #[arg(long)]
        with_token: bool,
        /// Print the verified identity as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the active credential and what the API says about it
    Status {
        /// Print the identity as JSON
        #[arg(long)]
        json: bool,
        /// Include the token itself in the output
        #[arg(long)]
        show_token: bool,
    },
    /// Remove the stored credential for the active profile
    Logout,
    /// Print the active token, for use with other tools
    #[command(
        after_help = "Example:\n  curl -H \"Authorization: Bearer $(2bec auth token)\" https://api.2befficient.com/v2/me"
    )]
    Token,
}

pub async fn run(ctx: &Context, args: AuthArgs) -> Result<()> {
    match args.command {
        AuthCommand::Login { with_token, json } => login(ctx, with_token, json).await,
        AuthCommand::Status { json, show_token } => status(ctx, json, show_token).await,
        AuthCommand::Logout => logout(ctx),
        AuthCommand::Token => {
            let c = ctx.require_credential()?;
            println!("{}", c.token);
            Ok(())
        }
    }
}

async fn login(ctx: &Context, with_token: bool, json: bool) -> Result<()> {
    let raw = read_token(with_token)?;
    let kind = token::classify(&raw).map_err(|e| anyhow::anyhow!("not a valid API key: {e}"))?;
    if let TokenKind::ApiKey { env } = kind {
        warn_env_mismatch(env, &ctx.settings.api_url);
    }

    let client = ctx.client_with(Some(&raw))?;
    let me: Me = client.get(me::PATH).await?.json()?;
    ctx.store.set(&ctx.settings.profile, &raw)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&me)?);
        return Ok(());
    }
    let who = match (&me.key, &me.person) {
        (Some(k), _) => format!("API key `{}`", k.name),
        (_, Some(p)) => p.name.clone(),
        _ => me.caller.clone(),
    };
    println!(
        "Logged in to {} as {} (profile `{}`, {})",
        me.tenant.name, who, ctx.settings.profile, ctx.settings.api_url
    );
    Ok(())
}

async fn status(ctx: &Context, json: bool, show_token: bool) -> Result<()> {
    let credential = ctx.require_credential()?;
    let resp = ctx.client()?.get(me::PATH).await?;
    let me: Me = resp.json()?;

    if json {
        let mut v = serde_json::to_value(&me)?;
        v["profile"] = ctx.settings.profile.clone().into();
        v["apiUrl"] = ctx.settings.api_url.clone().into();
        v["tokenSource"] = source_name(credential.source).into();
        v["store"] = ctx.store.describe().into();
        if show_token {
            v["token"] = credential.token.clone().into();
        }
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    println!("Profile:      {}", ctx.settings.profile);
    println!("API URL:      {}", ctx.settings.api_url);
    println!("Token source: {}", credential.source);
    println!("Store:        {}", ctx.store.describe());
    let shown = if show_token {
        credential.token.clone()
    } else {
        token::display_prefix(&credential.token)
    };
    println!("Token:        {shown}");
    println!("Tenant:       {} ({})", me.tenant.name, me.tenant.id);
    if let Some(k) = &me.key {
        println!("Caller:       API key `{}` ({})", k.name, k.prefix);
        if let Some(d) = k.description.as_deref().filter(|d| !d.is_empty()) {
            println!("Description:  {d}");
        }
        println!("Profile:      {}", k.profile.name);
        println!("Scopes:       {}", join_or_none(&k.scopes));
        println!(
            "Expires:      {}",
            k.expires_at.as_deref().unwrap_or("never")
        );
        println!(
            "Last used:    {}",
            k.last_used_at.as_deref().unwrap_or("never")
        );
    }
    if let Some(p) = &me.person {
        let email = p
            .email
            .as_deref()
            .map_or(String::new(), |e| format!(" <{e}>"));
        println!("Caller:       {}{email}", p.name);
        println!("Roles:        {}", join_or_none(&p.roles));
        println!("Scopes:       {}", join_or_none(&p.scopes));
        if let Some(c) = &p.client {
            println!("Client:       {c}");
        }
    }
    let reset = match resp.rate_limit {
        Some(h) => format!("in {}s", h.reset_secs),
        None => format!("at {}", me.rate_limit.reset_at),
    };
    println!(
        "Rate limit:   {} of {} left, resets {reset}",
        me.rate_limit.remaining, me.rate_limit.limit
    );
    Ok(())
}

fn logout(ctx: &Context) -> Result<()> {
    let profile = &ctx.settings.profile;
    if ctx.store.remove(profile)? {
        println!("Removed the credential for profile `{profile}`");
    } else {
        println!("No credential stored for profile `{profile}`");
    }
    if let Some(Credential {
        source: TokenSource::Override,
        ..
    }) = ctx.credential
    {
        eprintln!("note: --token / TWOBEC_TOKEN is still set and will be used");
    }
    Ok(())
}

/// Read the key from stdin (when asked or when stdin is not a terminal) or
/// from a hidden prompt.
fn read_token(with_token: bool) -> Result<String> {
    let stdin = std::io::stdin();
    let raw = if with_token || !stdin.is_terminal() {
        let mut s = String::new();
        stdin
            .lock()
            .read_to_string(&mut s)
            .context("cannot read the token from stdin")?;
        s
    } else {
        eprint!("API key: ");
        std::io::stderr().flush().ok();
        rpassword::read_password().context("cannot read the token from the terminal")?
    };
    let token = raw.lines().next().unwrap_or_default().trim().to_string();
    if token.is_empty() {
        bail!("no token given");
    }
    Ok(token)
}

fn warn_env_mismatch(env: token::KeyEnv, api_url: &str) {
    let is_prod = api_url == crate::http::DEFAULT_API_URL;
    let mismatch = match env {
        token::KeyEnv::Live => !is_prod,
        token::KeyEnv::Next | token::KeyEnv::Dev => is_prod,
    };
    if mismatch {
        eprintln!("warning: this is a `{env}` key but the API URL is {api_url}");
    }
}

fn source_name(source: TokenSource) -> &'static str {
    match source {
        TokenSource::Override => "override",
        TokenSource::Stored => "stored",
    }
}

fn join_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}
