//! `2bec auth`: log in (browser or API key), inspect or drop the credential.

use anyhow::{Context as _, Result, bail};
use clap::{Args, Subcommand};
use std::io::{IsTerminal, Read, Write};

use super::Context;
use crate::api::me::{self, Me};
use crate::auth::oauth;
use crate::auth::store::{CredentialStore, StoredCredential};
use crate::auth::token::{self, TokenKind};
use crate::auth::{Credential, TokenSource};

#[derive(Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

impl AuthArgs {
    /// `login` and `logout` replace or drop the credential, so renewing it
    /// first would be wasted work (or a confusing warning).
    pub fn skips_refresh(&self) -> bool {
        matches!(
            self.command,
            AuthCommand::Login { .. } | AuthCommand::Logout
        )
    }
}

#[derive(Subcommand, Debug)]
enum AuthCommand {
    /// Log in as yourself in the browser, or store an API key
    #[command(after_help = "\
Examples:
  2bec auth login                      # opens the browser (person login)
  2bec auth login --device             # shows a code to enter on another device
  2bec auth login --no-browser         # prints the loopback URL (port forwarding)
  2bec auth login --with-token         # prompts for an API key (hidden)
  echo \"$KEY\" | 2bec auth login       # reads an API key from stdin
  2bec --profile dev auth login --with-token < key.txt

The browser login asks the identity server for the scopes pegas-api:read
and pegas-api:write on your own account; the access token is renewed
automatically for 30 days. Over SSH, or wherever no browser can open, the
device flow is used: open the URL shown on any device and type the code. An API Admin creates API keys in the web app
under Administration > API access; the secret is shown once.")]
    Login {
        /// Store an API key instead of logging in with the browser
        #[arg(long)]
        with_token: bool,
        /// Print the login URL instead of opening a browser
        #[arg(long, conflicts_with_all = ["with_token", "device"])]
        no_browser: bool,
        /// Log in from another device with a short code (default over SSH)
        #[arg(long, conflicts_with = "with_token")]
        device: bool,
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
    /// Forget the stored credential for the active profile
    Logout,
    /// Print the active token, for use with other tools
    #[command(
        after_help = "Example:\n  curl -H \"Authorization: Bearer $(2bec auth token)\" https://api.2befficient.com/v2/me"
    )]
    Token,
}

pub async fn run(ctx: &Context, args: AuthArgs) -> Result<()> {
    match args.command {
        AuthCommand::Login {
            with_token,
            no_browser,
            device,
            json,
        } => {
            // A piped stdin carries a key; a terminal means a person login.
            // --no-browser and --device are for headless use, so they win
            // over stdin.
            let headless = no_browser || device;
            if with_token || (!headless && !std::io::stdin().is_terminal()) {
                login_with_key(ctx, json).await
            } else if device || (!no_browser && browser_unavailable()) {
                login_with_device(ctx, json).await
            } else {
                login_in_browser(ctx, no_browser, json).await
            }
        }
        AuthCommand::Status { json, show_token } => status(ctx, json, show_token).await,
        AuthCommand::Logout => logout(ctx).await,
        AuthCommand::Token => {
            let c = ctx.require_credential()?;
            println!("{}", c.token);
            Ok(())
        }
    }
}

/// No browser can open here: an SSH session without a display. The loopback
/// redirect would land on this machine, which the remote browser cannot
/// reach, so the device flow is the right default.
fn browser_unavailable() -> bool {
    let ssh = std::env::var_os("SSH_CONNECTION").is_some_and(|v| !v.is_empty())
        || std::env::var_os("SSH_TTY").is_some_and(|v| !v.is_empty());
    let display = ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()));
    ssh && !display
}

/// Device authorization grant: show a code, wait for the approval elsewhere.
async fn login_with_device(ctx: &Context, json: bool) -> Result<()> {
    let http = ctx.plain_http()?;
    let endpoints = oauth::discover(&http, &ctx.settings.api_url, ctx.settings.issuer.as_deref())
        .await
        .context("cannot find the identity server")?;
    let cfg = ctx.settings.login_config(true);
    let auth = oauth::device_authorize(&http, &endpoints, &cfg).await?;
    eprintln!("Open this URL on any device and enter the code to log in:\n");
    eprintln!("  {}", auth.verification_uri);
    eprintln!("  Code: {}\n", auth.user_code);
    if let Some(complete) = &auth.verification_uri_complete {
        eprintln!("  (or open {complete})\n");
    }
    eprintln!(
        "Waiting for the login to complete (the code is valid {} minutes)...",
        auth.expires_in.div_ceil(60)
    );
    let tokens = oauth::poll_device(&http, &endpoints, &cfg, &auth).await?;
    finish_person_login(ctx, endpoints.issuer, tokens, json).await
}

async fn login_in_browser(ctx: &Context, no_browser: bool, json: bool) -> Result<()> {
    let http = ctx.plain_http()?;
    let endpoints = oauth::discover(&http, &ctx.settings.api_url, ctx.settings.issuer.as_deref())
        .await
        .context("cannot find the identity server")?;
    let cfg = ctx.settings.login_config(no_browser);
    let tokens = oauth::login(&http, &endpoints, &cfg).await?;
    finish_person_login(ctx, endpoints.issuer, tokens, json).await
}

/// Verify the new tokens against `/v2/me`, store them, report.
async fn finish_person_login(
    ctx: &Context,
    issuer: String,
    tokens: oauth::Tokens,
    json: bool,
) -> Result<()> {
    let client = ctx.client_with(Some(&tokens.access_token))?;
    let me: Me = client.get(me::PATH).await?.json()?;
    ctx.store.set(
        &ctx.settings.profile,
        &StoredCredential::OAuth {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            issuer,
        },
    )?;
    report_login(ctx, &me, json)
}

async fn login_with_key(ctx: &Context, json: bool) -> Result<()> {
    let raw = read_token()?;
    let kind = token::classify(&raw).map_err(|e| anyhow::anyhow!("not a valid API key: {e}"))?;
    if let TokenKind::ApiKey { env } = kind {
        warn_env_mismatch(env, &ctx.settings.api_url);
    }

    let client = ctx.client_with(Some(&raw))?;
    let me: Me = client.get(me::PATH).await?.json()?;
    ctx.store.set(
        &ctx.settings.profile,
        &StoredCredential::ApiKey { token: raw },
    )?;
    report_login(ctx, &me, json)
}

fn report_login(ctx: &Context, me: &Me, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(me)?);
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
        v["tokenKind"] = kind_name(credential).into();
        v["store"] = ctx.store.describe().into();
        if let Some(o) = &credential.oauth {
            v["expiresAt"] = o.expires_at.into();
            v["issuer"] = o.issuer.clone().into();
            v["canRefresh"] = o.refresh_token.is_some().into();
        }
        if show_token {
            v["token"] = credential.token.clone().into();
        }
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    println!("Profile:      {}", ctx.settings.profile);
    println!("API URL:      {}", ctx.settings.api_url);
    println!("Token source: {}", credential.source);
    println!("Token kind:   {}", kind_name(credential));
    println!("Store:        {}", ctx.store.describe());
    let shown = if show_token {
        credential.token.clone()
    } else {
        token::display_prefix(&credential.token)
    };
    println!("Token:        {shown}");
    if let Some(o) = &credential.oauth {
        let left = o.expires_at.saturating_sub(oauth::now_unix());
        let renew = if o.refresh_token.is_some() {
            "renewed automatically"
        } else {
            "no refresh token"
        };
        println!(
            "Access token: expires in {}m{}s, {renew}",
            left / 60,
            left % 60
        );
        println!("Issuer:       {}", o.issuer);
    }
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
        println!("Note:         person tokens ignore Idempotency-Key and never get webhooks:write");
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

async fn logout(ctx: &Context) -> Result<()> {
    let profile = &ctx.settings.profile;
    // Revoke the refresh token first so the login cannot be renewed any more.
    if let Some(Credential {
        source: TokenSource::Stored,
        oauth: Some(state),
        ..
    }) = &ctx.credential
        && let Some(refresh) = &state.refresh_token
    {
        let http = ctx.plain_http()?;
        let revoked = async {
            let endpoints =
                oauth::discover(&http, &ctx.settings.api_url, Some(&state.issuer)).await?;
            oauth::revoke(&http, &endpoints, &ctx.settings.oauth_client_id, refresh).await
        }
        .await;
        match revoked {
            Ok(()) => eprintln!("Revoked the login at {}", state.issuer),
            Err(e) => eprintln!("warning: could not revoke the login ({e}); it expires on its own"),
        }
    }
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

/// Read the key from stdin when it is not a terminal, else from a hidden
/// prompt.
fn read_token() -> Result<String> {
    let stdin = std::io::stdin();
    let raw = if !stdin.is_terminal() {
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

fn kind_name(c: &Credential) -> &'static str {
    if c.oauth.is_some() {
        "person login (OAuth)"
    } else if c.token.starts_with(token::KEY_PREFIX) {
        "API key"
    } else {
        "bearer token"
    }
}

fn join_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}
