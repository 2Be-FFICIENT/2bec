# 2bec

**A command-line client for the 2Be-FFICIENT public API.**

[![Release](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fapi.2befficient.com%2Fcli%2Freleases%2Flatest&query=%24.version&label=release&prefix=v&color=blue)](https://github.com/2Be-FFICIENT/2bec/releases)
[![CI](https://github.com/2Be-FFICIENT/2bec/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/2Be-FFICIENT/2bec/actions/workflows/ci.yml)
![Platforms](https://img.shields.io/badge/platforms-linux%20%7C%20macOS%20%7C%20windows-lightgrey)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)

[Install](#install) · [Quick start](#quick-start) · [Commands](#commands) · [Examples](examples/README.md) · [API docs](https://api.2befficient.com/docs)

`2bec` gives you the monitoring platform's API (`/v2`) from a shell. Each
resource is a command group: runs, scenarios, metrics, incidents, alerts,
planning, reports, webhooks and so on. Commands read as
`2bec <group> <action>`, so if you know `gh` or `kubectl` you already know
the shape.

```console
$ 2bec auth login
$ 2bec runs list --status failed --period today
$ 2bec runs get <run> --jq '.steps[] | [.label, .durationMs] | @tsv'
```

### When it helps

- **Checking on things** without opening the browser, such as last night's failures or an open incident.
- **Continuous integration (CI) gates** that start a smoke run and fail the pipeline on the exit code.
- **Scheduled exports** of SLA figures or report PDFs from cron.
- **Your own tooling** fed with `--json`, or shaped on the spot with `--jq`.

> [!NOTE]
> `2bec` is a client, not an administration console. API keys are created
> by an API Admin in the web app (`/admin/api-keys`), and users and access
> profiles are read-only here.

### How it behaves

| Area | Behavior |
|------|----------|
| Output | Tables on a terminal, tab-separated text in a pipe, `--json` and `--jq` for scripts. No colors when piped or when `NO_COLOR` is set. |
| Login | Browser, a short device code (the default over SSH), or an API key. |
| Credentials | A `0600` file by default, or the OS keyring. |
| Settings | Profiles per environment. Flags win over `TWOBEC_*` variables, which win over `config.toml`. |
| Writes | Idempotency keys on retries, `--if-match` against lost updates, dry run and confirmation before bulk changes. |
| Failures | One line on stderr, with the cause chain and trace id under `--verbose`. Retries with backoff on `429` and `5xx`. |
| Help | Every command has examples in `--help`. Shell completions and man pages come from the same definitions. |

> [!TIP]
> In CI, skip `auth login` and set `TWOBEC_TOKEN` to an API key. Locally,
> `echo "$KEY" | 2bec auth login` stores a key read from stdin.

> [!IMPORTANT]
> Exit codes are part of the interface. Auth, network, API and rate-limit
> failures each have their own code (3 to 6), so a script can tell them
> apart. See [Exit codes](#exit-codes).

> [!WARNING]
> `--jq` hands the JSON to the `jq` on your `PATH`. Install
> [jq](https://jqlang.org) to use it, or pipe `--json` to any tool you like.

## Install

One line, from the API's release feed (`install/`: the scripts check the archive
against its `SHA256SUMS`; `TWOBEC_VERSION` pins a version, `TWOBEC_INSTALL_DIR`
chooses the directory). On Linux and macOS the completions for bash, zsh and
fish and the manual pages go under the matching `share` directory
(`/usr/local/share` or `~/.local/share`; `TWOBEC_INSTALL_COMPLETIONS=0` skips
them, and zsh needs its `site-functions` directory on `fpath`):

```sh
curl -fsSL https://api.2befficient.com/cli/install.sh | sh
```

```powershell
irm https://api.2befficient.com/cli/install.ps1 | iex
```

Each [release](https://github.com/2Be-FFICIENT/2bec/releases) ships a static
binary per platform (Linux x64/arm64, macOS x64/arm64, Windows x64/arm64) with
the completions and manual pages in the archive and a `SHA256SUMS` next to it;
the feed serves the same archives at
`https://api.2befficient.com/cli/releases/<version>/<archive>`. By hand:

```sh
tar xzf 2bec-<version>-linux-x64.tar.gz
install -m 755 2bec-<version>-linux-x64/2bec ~/.local/bin/
```

Already installed? `2bec upgrade` fetches the latest release from the same
feed, checks it against `SHA256SUMS` and replaces the binary in place
(`--check` only reports, `--version 0.1.0` pins or downgrades, `--feed` or
`TWOBEC_INSTALL_URL` name another feed); the completions and manual pages the
installer put under `share/` are regenerated.

From source: `cargo build --release` (add `--features keyring` for OS keyring
support, which the release binaries include).

## Quick start

```sh
2bec auth login                                   # browser login; --with-token for an API key
2bec auth status                                  # who you are, scopes, rate limit
2bec runs list --status failed --period today     # a table here, TSV in a pipe
2bec runs get <run> --jq '.steps[] | [.label, .durationMs] | @tsv'
```

More recipes, group by group, live in [`examples/`](examples/README.md).

## Commands

Commands follow `2bec <group> <action> [args] [flags]`; `2bec <group> --help`
lists the actions.

| Group | What it covers |
|-------|----------------|
| `auth` | Log in, show or drop the credential used to call the API |
| `config` | Read and edit the config file, switch profiles |
| `ping` | Check that the API answers and print its version and latency |
| `runs` | List and inspect scenario runs |
| `scenarios` | Browse the catalog of scenarios and their groups |
| `metrics` | Availability, response times, voice quality and social figures over a period |
| `availability` | How the availability rate is computed and the targets it is read against |
| `incidents` | Streaks of consecutive failed runs |
| `alerts` | Alert configurations, recipients, suspensions and the alerts sent |
| `neutralizations` | Set runs apart from the indicators, and undo it |
| `interruptions` | Planned non-observation windows and rules |
| `planning` | Timeline, emergency stop and dated notes on the scenarios |
| `comments` | The team's comments on the runs, across scenarios |
| `tags` | Tags a run may carry: the space's own, the robots' and the platform's |
| `badges` | Badges: symbols drawn on the runs matching a rule over their tags |
| `variables` | Runtime variables: the named values the alert hooks receive |
| `reports` | Saved reports, their data and PDFs, scheduled e-mails and deliveries |
| `webhooks` | Webhooks: subscriptions of your URLs to events, secrets, deliveries |
| `users` | The users of the space, their roles and access profile |
| `profiles` | Access profiles: which scenario groups the users see |
| `security` | The sign-in policy: password rules and strong authentication |
| `audit` | The administration history: who did what on the space |
| `tickets` | Support requests between the space and the platform's team |
| `completion` | Print a shell completion script (bash, zsh, fish, powershell, elvish) |
| `man` | Print or install the manual pages |
| `upgrade` | Replace this binary with the latest release (or a pinned version) |

## Configuration

Settings are resolved in this order: flags, then `TWOBEC_*` environment
variables, then `~/.config/2bec/config.toml`, then built-in defaults.

| Option      | Environment variable | Default                                     |
|-------------|----------------------|---------------------------------------------|
| `--profile` | `TWOBEC_PROFILE`     | `default_profile` in config, else `default` |
| `--api-url` | `TWOBEC_API_URL`     | `https://api.2befficient.com`               |
| `--token`   | `TWOBEC_TOKEN`       | token stored by `auth login`                |
| `--timeout` |                      | `30` seconds                                |
| `--verbose` |                      | off; adds causes and the API trace id       |

Profiles, the `config` command and the keyring are described in
[`examples/configuration.md`](examples/configuration.md) and
[`examples/auth.md`](examples/auth.md).

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
API's trace id to quote in support requests. A route answering with
`Deprecation` / `Sunset` headers gets a one-line warning once per invocation.

## Contributing

```sh
cargo test                                                   # unit and end-to-end tests, no network
cargo clippy --all-targets --all-features -- -D warnings
./2bec.sh ping                                               # rebuilds on change and forwards args
```

To cut a release, bump `version` in `Cargo.toml`, commit, and push a tag
`v<version>`; the tag must match the manifest or the pipeline stops.

## License

Licensed under the [Apache License, Version 2.0](LICENSE). Third-party crates
linked into the binary and their licenses are listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
