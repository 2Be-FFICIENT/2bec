# 2bec

`2bec` is the command-line client for the 2Be-FFICIENT solution. It lets you call
the 2Be-FFICIENT API directly from your terminal and scripts, without going
through the web interface.

## Requirements

- Rust stable toolchain (managed via `rustup`; see `rust-toolchain.toml`)

## Build

```sh
cargo build --release
```

The binary is produced at `target/release/2bec`.

## Run

Use the wrapper script, which builds the binary if needed:

```sh
./2bec.sh ping
```

Run `./2bec.sh --help` for all commands and options.

## Authentication

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

## Runs

```sh
2bec runs list --status failed --period today       # table on a TTY, TSV in a pipe
2bec runs list --scenario <id> --period last7d --all --json
2bec runs get <run>                                  # steps, failure, tags, artifacts
2bec runs diagnosis <run> --opale                    # verdict, journey, AI analysis
2bec runs artifacts <run>
2bec runs download <run> video -o run.mp4            # also screenshot, wav; -o - for stdout
2bec runs comments list|get|add|edit|delete <run> …  # add/edit/delete need runs:write
2bec runs tag <run> <tag-id>… / 2bec runs untag <run> <tag-id>
2bec runs trigger <scenario> --var USER=bob --wait   # needs runs:request
2bec runs requests state <scenario>                  # quota, variables, recent requests
2bec runs requests list --period last30d / get <id>
```

Writes carry an `Idempotency-Key`, so a retried `comments add`, `tag` or
`trigger` never acts twice. `comments edit|delete --if-match <etag>` guards
against lost updates with the ETag shown by `comments get`. Text arguments
accept `-` for stdin and `@file`.

## Scenarios

```sh
2bec scenarios list                                  # the catalog, by name
2bec scenarios list --kind transaction --active --json
2bec scenarios list --group <id> -q login            # one group, words of the name
2bec scenarios get <scenario>                        # steps, thresholds, monitoring, target
2bec scenarios versions <scenario> --months 3        # saves of the script, newest first
2bec scenarios calendar <scenario> --from … --to …   # cadence, hours, interruption windows
2bec scenarios download-doc <scenario> -o doc.pdf    # alias doc; --technical; -o - for stdout
2bec scenarios groups list / get <group>
```

Scenario ids are what `runs list --scenario` and `runs trigger` take.
Versions, calendar and documentation exist for transaction scenarios only;
the API answers `social-scenario` (409) for a social monitoring one.

## Configuration

Settings are resolved in this order: flags, then `TWOBEC_*` environment
variables, then the config file, then built-in defaults.

| Option      | Environment variable | Default                                     |
|-------------|----------------------|---------------------------------------------|
| `--profile` | `TWOBEC_PROFILE`     | `default_profile` in config, else `default` |
| `--api-url` | `TWOBEC_API_URL`     | `https://api.2befficient.com`               |
| `--token`   | `TWOBEC_TOKEN`       | token stored by `auth login`                |
| `--timeout` |                      | `30` seconds                                |
| `--verbose` |                      | off; adds causes and the API trace id       |

The config file is `~/.config/2bec/config.toml` (`$XDG_CONFIG_HOME` is
honored; `TWOBEC_CONFIG_DIR` overrides the directory). It holds named
profiles, typically one per environment:

```toml
default_profile = "prod"

[profiles.prod]
api_url = "https://api.2befficient.com"

[profiles.dev]
api_url = "http://localhost:5152"
```

Credentials are kept per profile in `hosts.toml` in the same directory,
written with owner-only permissions. `--token` / `TWOBEC_TOKEN` override the stored
token for one invocation.

## Exit codes

| Code | Meaning                                                   |
|------|-----------------------------------------------------------|
| 0    | Success                                                   |
| 1    | Generic error                                             |
| 2    | Usage error (bad arguments)                               |
| 3    | Authentication: missing, invalid, expired or revoked key, or missing scope |
| 4    | Network: the API could not be reached                     |
| 5    | The API refused the request for another reason            |
| 6    | Rate limited and retries exhausted                        |

Errors are one line on stderr; `--verbose` adds the cause chain and the
API's trace id to quote in support requests.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
