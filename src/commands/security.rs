//! `2bec security-settings`: the sign-in policy of the space. Read only.

use anyhow::Result;
use clap::Args;

use super::Context;
use crate::api::people::SecuritySettings;
use crate::output::OutputArgs;

#[derive(Args, Debug)]
#[command(after_help = "\
Needs users:read. The password rules and strong authentication of the
space, with how many users they apply to, how many are exempted from strong
authentication, and how many an SMS or e-mail code cannot reach.

Examples:
  2bec security-settings
  2bec security-settings --jq .twoFactor.enabled")]
pub struct SecuritySettingsArgs {
    #[command(flatten)]
    output: OutputArgs,
}

pub async fn run(ctx: &Context, args: SecuritySettingsArgs) -> Result<()> {
    let s: SecuritySettings = ctx.client()?.get("v2/security-settings").await?.json()?;
    if args.output.wants_json() {
        return args.output.print_json(&s);
    }
    let p = &s.password;
    println!("Password rules:");
    println!("  Minimum length:        {}", p.min_length);
    println!(
        "  Upper and lower case:  {}",
        yes_no(p.require_upper_and_lower)
    );
    println!("  Digit:                 {}", yes_no(p.require_digit));
    println!("  Special characters:    {}", p.special_char_count);
    println!("  Reuse forbidden:       {}", yes_no(p.forbid_reuse));
    println!(
        "  Renewal:               {}",
        if p.renew_regularly {
            format!("every {} days", p.renew_days)
        } else {
            "never".to_string()
        }
    );
    let t = &s.two_factor;
    println!();
    println!("Strong authentication:");
    println!("  Enabled:               {}", yes_no(t.enabled));
    println!("  Channels:              {}", channels(t.sms, t.email));
    println!("  Asked of:              {}", asked_of(t.users, t.admins));
    println!("  Trusted device:        {} days", t.trusted_device_days);
    println!();
    println!("Users:");
    println!("  Total:                 {}", s.user_count);
    println!("  Exempted from 2FA:     {}", s.two_factor_exempt_count);
    println!("  Without mobile:        {}", s.without_mobile_count);
    println!("  Without e-mail:        {}", s.without_email_count);
    Ok(())
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn channels(sms: bool, email: bool) -> &'static str {
    match (sms, email) {
        (true, true) => "SMS, e-mail",
        (true, false) => "SMS",
        (false, true) => "e-mail",
        (false, false) => "none",
    }
}

fn asked_of(users: bool, admins: bool) -> &'static str {
    match (users, admins) {
        (true, true) => "users and administrators",
        (true, false) => "users",
        (false, true) => "administrators",
        (false, false) => "nobody",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(channels(true, true), "SMS, e-mail");
        assert_eq!(channels(false, false), "none");
        assert_eq!(asked_of(false, true), "administrators");
    }
}
