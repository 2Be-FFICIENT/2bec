# Authentication

How to log in, with a browser, a device code or an API key, and where the credential is kept.

Two ways in. The browser login signs you in as yourself; an API key
authenticates as a key an API Admin created in the web app under
Administration > API access (the secret is shown once).

```sh
2bec auth login                 # opens the browser, logs you in as yourself
2bec auth login --device        # shows a code to enter on any device (SSH, containers)
2bec auth login --no-browser    # prints the loopback URL, for `ssh -L` port forwarding
2bec auth login --with-token    # prompts for an API key (hidden input)
echo "$KEY" | 2bec auth login   # reads an API key from stdin, for scripts
2bec auth status                # who you are, scopes, expiry, rate limit
2bec auth status --json         # same, for scripts
2bec auth token                 # print the token, e.g. for curl
2bec auth logout                # forget the credential (and revoke the login)
```

The browser login uses the authorization code flow with PKCE against the
2Be-FFICIENT identity server (client id `2bec`, loopback redirect). Over
SSH without a display, `auth login` switches to the device flow by itself:
open the URL it prints on any device, enter the code, and the CLI picks the
login up within a few seconds. Access tokens last an hour and are renewed automatically from the refresh token
for 30 days, after which you log in again. Person logins ignore
`Idempotency-Key` and never get `webhooks:write`; use an API key for those.

Profiles can pin the identity server and client for other environments:

```toml
[profiles.dev]
api_url = "http://localhost:5152"
issuer = "https://identity.2befficient.com"
oauth_client_id = "2bec"
api_resource = "https://api.2befficient.com"
```

## OS keyring

A build with the `keyring` feature (`cargo build --release --features keyring`,
and the release binaries) can keep the secrets in the operating system keyring
instead: Secret Service on Linux, Keychain on macOS, Credential Manager on
Windows.

```sh
2bec config set credential_store keyring
2bec auth login            # from now on the token goes to the keyring
```

`hosts.toml` then only lists the logged-in profiles (`keyring = true`) with
the non-secret fields. Logins made before the switch stay readable; log in
again to move them. Set `credential_store` back to `file` to return to the
file, which is the default.

