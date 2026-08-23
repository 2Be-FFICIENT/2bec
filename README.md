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

## Output

Tables on a terminal, tab-separated text in a pipe, `--json` for the full
payload (pretty on a terminal, compact in a pipe). `--jq EXPR` runs the JSON
through the `jq` program on your PATH, so scripts can pick fields in place:

```sh
2bec scenarios list --jq '.[] | select(.active) | .id'
2bec runs get <run> --jq '.steps[] | [.label, .durationMs] | @tsv'
```

Colors and spinners are never written into a pipe; `NO_COLOR` is honored.

## Scenarios

```sh
2bec scenarios list                                  # the catalog, by name
2bec scenarios list --kind transaction --active --json
2bec scenarios list --group <id> -q login            # one group, words of the name
2bec scenarios get <scenario>                        # steps, thresholds, monitoring, target
2bec scenarios versions <scenario> --months 3        # saves of the script, newest first
2bec scenarios calendar <scenario> --from … --to …   # cadence, hours, interruption windows
2bec scenarios download-doc <scenario> -o doc.pdf    # alias doc; --technical; -o - for stdout
2bec scenarios synthesis <scenario> --trend week     # quality indices out of 10, comparison windows
2bec scenarios response-times <scenario>             # milliseconds down to the step responsible
2bec scenarios voice-quality <scenario>              # MOS, error rate, jitter of every call
2bec scenarios groups list / get <group>
```

Scenario ids are what `runs list --scenario` and `runs trigger` take.
Versions, calendar and documentation exist for transaction scenarios only;
the API answers `social-scenario` (409) for a social monitoring one.

### Scenario settings

```sh
2bec scenarios settings <scenario>                   # name, catalogue id, step ids, run variables, ETag
2bec scenarios edit <scenario> --name "Login (prod)" --catalogue-id <id>
2bec scenarios edit <scenario> --clear-description --if-match '"v3"'
2bec scenarios rename-steps <scenario> <step>="Open the home page"
2bec scenarios run-variables <scenario> USER=alice ENV   # replaces the whole list; --clear for none
```

What a key may change on a transaction scenario; the cadence, the exposure
and the internal note are the platform team's (`staff-only-field`, 403).
Every write answers the settings with their ETag and takes `--if-match`.
Reads need `scenarios:read`, writes `scenarios:write`.

### Response time thresholds

```sh
2bec scenarios thresholds get <scenario>             # per version and bandwidth, run and steps
2bec scenarios thresholds compute <scenario>         # proposals from the last 7 days, nothing written
2bec scenarios thresholds compute <scenario> --from 2026-09-01 --to 2026-10-01 --percentile 0.9
2bec scenarios thresholds compute <scenario> --apply # asks before writing; --yes skips the question
2bec scenarios thresholds apply <scenario> @thresholds.json   # the `apply` of a computation, or hand-written
```

Milliseconds, target ≤ client ≤ unacceptable. A computation reads whole
days in Paris, 62 at most; an omitted run or step keeps its thresholds.
Every bandwidth is written in one transaction (`thresholds-changed`, 409,
when they moved meanwhile). Reads need `metrics:read`, compute and apply
`indicators:write`.

## Availability

```sh
2bec availability settings get                       # neutralized runs, failures before down, tag rules
2bec availability settings set --computing-type ignoreNeutralized --failure-counts-from 2
2bec availability settings set --tag-rules true --failure-tags <tag>,<tag> --neutral-tags ""
2bec availability targets get                        # the space's target and each scenario's own
2bec availability targets set --tenant 99.9 --scenario <scenario>=99.95 --scenario <other>=default
```

`settings set` reads the rules, applies the flags given and writes them back
with the ETag it read, since a change of the rules recomputes every past day
(`rules-locked`, 403, while the platform's team keeps them). A target is a
percentage above 50 and below 100 with 3 decimals at most; `default` makes
the space follow the platform's target, or a scenario the space's. Reads
need `metrics:read`, writes `indicators:write`.

## Variables

```sh
2bec variables list --plain -q smtp                  # the space's runtime variables, by name
2bec variables get <id>                              # value history, what reads it, ETag
2bec variables set SMTP_HOST smtp.example.com --comment "Outgoing mail"
2bec variables set API_TOKEN - --secret < token.txt  # value from stdin; @file works too
2bec variables set SMTP_HOST --comment ""            # clears the comment, keeps the value
2bec variables delete OLD_TOKEN --force              # alias rm; --force when a hook reads it
```

Runtime variables are the named values the alert hooks receive as
environment variables. A secret's value is stored ciphered and never
returned; `--secret` is fixed at the creation (`variable-kind-fixed`, 409).
Each new value is one more version of the history. Reads need
`scenarios:read`, writes `scenarios:write`.

## Tags and badges

```sh
2bec tags list --kind business -q release         # the tags a run may carry, by name
2bec tags get <id>                                # kind, comment, ETag
2bec tags create "release 42" --comment "Release runs"
2bec tags edit <id> --name "release 43" --comment ""   # a merge patch; "" clears
2bec tags delete <id> --force                     # alias rm; --force detaches it from the runs

2bec badges list --enabled                        # resolution order: priority, then name
2bec badges get <id>                              # rule, validity, tags, ETag
2bec badges create --name Prod --symbol P --tag <tag-id>
2bec badges create --name Release --symbol rocket --icon --color 3 \
  --tag <id> --tag <id> --all-tags --status any --valid-from 2026-12-20T00:00:00Z
2bec badges edit <id> --enabled false --clear-valid-until --if-match '"v3"'
2bec badges delete <id>                           # the runs keep their tags
```

Tags are `business` (the space's own, the only ones written), `robot`
(written by the robots) or `global` (the platform's); reserved prefixes such
as `obs::` are refused locally. A badge is a symbol in a palette color drawn
on the runs carrying any or all of its tags, on failed, successful or every
run, within an optional validity window; the highest priority wins. Defaults
on creation: enabled, priority 10, text symbol, any of the tags, failed runs.
Reads need `scenarios:read`, writes `indicators:write`; `tag-in-use` (409)
without `--force`, `conflict` (409) on a duplicate name.

## Users and access profiles

```sh
2bec users list --status locked --role "User Admin" -q smith   # paged; --all for everyone
2bec users get <id>                                # roles, profile, sign-in method, 2FA, lock
2bec profiles list                                 # groups (closed ones in brackets), scenarios, holders
2bec profiles get <id>                             # the groups and the users holding it
2bec security settings                             # password rules, strong authentication, counts
```

Read only, all behind `users:read`. A user's id is the identity subject the
administration history names as its actor; account administrators hold
every role whatever `roles` lists. Deleted accounts are never listed.

## Audit, comments and tickets

```sh
2bec audit list --period last7d --area tags --area badges   # who did what; last 30 days by default
2bec audit list --actor <user-id> --outcome failed --source api -q threshold
2bec audit get 42                                  # the entry with the state before and after
2bec comments list --period lastMonth --outcome failed -q "false positive"   # across scenarios
2bec tickets list --view open                      # support requests; --kind, --scenario
2bec tickets get 7                                 # the request and its conversation
```

The history needs `audit:read` and shows the areas the key's scopes unlock
(reports, events and notifications with that scope alone; an area needing a
role with the write scope implying it). Entry and request ids are numbers,
not GUIDs. The comments feed needs `runs:read` and covers the scenarios the
key sees, or `--scenario` (100 at most); the comments of one run are under
`runs comments`. Tickets need `scenarios:read` and answer
`tickets-not-enabled` (403) until the space's administrators ask for the
page.

## Metrics

```sh
2bec metrics availability                            # last 30 days against the target, error budget
2bec metrics availability --period lastMonth --granularity week --scenario <id>…
2bec metrics response-times --period last30d         # fleet P50/P85 against the thresholds
2bec metrics voice-quality                           # line quality of the voice (IVR) calls
2bec metrics social --period today --interval 15 --events   # posts, incidents, grades
2bec metrics social-analysis                         # daily AI reading: issues and keywords
2bec metrics pdf availability --period lastMonth -o sla.pdf   # also response-times, voice-quality
```

Every read takes `--from/--to`, `--period` and `--tz` like `runs list`, and
`--scenario` repeated to name the scenarios; without it the read covers every
scenario the credential sees, up to the API cap (100 for availability, 15 for
response times, 20 for voice, 30 for social), past which it asks to name them.
Series by bucket are in `--json` only. One scenario's own figures are under
`scenarios synthesis|response-times|voice-quality`. All need `metrics:read`.

## Alerts

```sh
2bec alerts list                                     # configurations, what each watches, issues
2bec alerts get <alert>                              # rules, recipients, schedule, health, ETag
2bec alerts notifications --period today --channel sms   # what was actually sent
2bec alerts create @alert.json                       # JSON body; needs alerts:write
2bec alerts edit <alert> '{"active": false}' --if-match <etag>
2bec alerts delete <alert> / duplicate <alert>
2bec alerts test --channel sms --recipient <person>  # really sends a test alert
2bec alerts channels
2bec alerts groups list / get / create --name … --channel … --member <person>… / edit / delete
2bec alerts recipients list / mute <person> / unmute <person>
2bec alerts suspensions                              # switches, held steps, windows, muted people
2bec alerts suspensions suspend|resume <scenario>/<step>…
2bec alerts suspensions windows get|create|edit|end|delete
```

Writes carry an `Idempotency-Key`; `edit`/`delete` take `--if-match` with the
ETag shown by `get`. JSON bodies accept a literal, `-` for stdin and `@file`.

## Neutralizations

```sh
2bec neutralizations list --state applied            # operations, newest first, whoever wrote them
2bec neutralizations get <operation> [--wait]
2bec neutralizations apply --from … --to … --scenario <id> --status failed --dry-run
2bec neutralizations apply --from … --to … --weekday 6 --weekday 7 --day-start 22:00 --day-stop 06:00 --comment "…"
2bec neutralizations undo <operation> --comment "…"
```

`apply` dry-runs first and prints what would change; on a terminal it then
asks and applies pinned to that dry run, so a selection that moved meanwhile
is refused (409 `selection-changed`) instead of applied blind. `--yes`
skips both, `--wait` polls a background operation. Everything needs
`runs:write`.

## Incidents

```sh
2bec incidents list                                  # streaks of failed runs, last 7 days
2bec incidents list --ongoing / --min-duration 600 / --scenario <id> --period lastMonth --all
2bec incidents get <incident>                        # runs, first failure, alerts sent
```

An incident id is the id of its first failed run. Needs `metrics:read`.

## Interruptions

```sh
2bec interruptions list --state upcoming             # ongoing, upcoming, past, rule, ended
2bec interruptions list --occurrences --from … --to …
2bec interruptions get <interruption>
2bec interruptions create --scenario <id>… --start … --end … --comment Maintenance   # a one-off window
2bec interruptions create --scenario <id>… --spec @rule.json                        # daily, weekly, monthly
2bec interruptions edit <interruption> --spec @rule.json --if-match <etag>
2bec interruptions end <interruption>                # a window ends now; a rule stops, keeps its past
2bec interruptions restore <interruption>            # an ended rule, back in force
2bec interruptions delete <interruption> [--erase-past]
```

A spec is `{"kind": "date"|"daily"|"weekly"|"monthly", "<kind>": {…}}`,
Paris wall clock, ISO weekdays. Scenarios sharing a script are covered
together: `create` answers one interruption per script and lists the
calendars the platform refused. Reads need `planning:read`, writes
`monitoring:write`.

## Planning

```sh
2bec planning timeline                               # 7 days back to 7 ahead, every kind
2bec planning timeline --scenario <id> --kind incident --incident-min-minutes 0
2bec planning pauses [--all]                         # the emergency stops, with script mates
2bec planning pause <scenario> [--yes]               # asks on a terminal; mates paused too
2bec planning resume <scenario> --comment "…"        # the stop becomes a planned interruption
2bec planning events list --period lastMonth --category release
2bec planning events get <event>
2bec planning events create --title "Release 4.2" --start 2026-10-12T10:00:00Z [--end …] [--scenario <id>…]
2bec planning events edit <event> --end … --description "" --if-match <etag>
2bec planning events delete <event>
```

Reads need `planning:read`; `pause`, `resume` and the event writes need
`monitoring:write`. Timeline kinds: monitoringEvent, plannedInterruption,
scriptVersion, incident, monitoringPause. An empty flag value on `events
edit` clears the member.

## Reports

```sh
2bec reports list --kind grouped                     # saved reports: grouped or comparative
2bec reports get <report>                            # definition, ETag, whether the API owns it
2bec reports data <report> --window week --date 2026-10-05   # grouped: month|week|day
2bec reports data <report> --period last30d --stat p50       # comparative: period + stat
2bec reports pdf <report> --lang en --planning plannedInterruption -o report.pdf
2bec reports create @definition.json                 # JSON body; needs reports:write
2bec reports edit <report> '{"name": "SLA"}' --if-match <etag>
2bec reports delete <report> [--purge]               # alias rm; grouped reports archive unless --purge
2bec reports schedules list / get <schedule>
2bec reports schedules create --type availability-dashboard --scenario <id>… --frequency weekly --to ops@example.com
2bec reports schedules edit <schedule> --enabled false --if-match <etag>
2bec reports schedules delete / send <schedule>      # send mails the report now, for real
2bec reports deliveries --period lastMonth --status failed
```

A saved report has two kinds with different definitions, so `create` and
`edit` take JSON; schedules take flags. Only reports the API created can be
changed (403 `not-own-report`). Reads need `reports:read`, writes and
`send` `reports:write`; the dashboard PDFs under `metrics pdf` need
`reports:read` too.

## Webhooks

```sh
2bec webhooks events                                 # the event kinds and the scope each needs
2bec webhooks list / get <webhook>                   # state, key, last delivery, ETag
2bec webhooks create --url https://… --event run.failed --event incident.opened
2bec webhooks edit <webhook> --active false --if-match <etag>
2bec webhooks edit <webhook> --event alert.sent --description "PagerDuty"   # events: whole list
2bec webhooks delete <webhook>                       # alias rm
2bec webhooks rotate-secret <webhook>                # alias rotate; old secret signs 24 h more
2bec webhooks ping <webhook>                         # sends a test event now
2bec webhooks deliveries list <webhook> --status failed --all
2bec webhooks deliveries get <webhook> <delivery>    # with the body as sent
2bec webhooks deliveries redeliver <webhook> <delivery>
```

`create` and `rotate-secret` print the signing secret once; it is the
HMAC-SHA256 key of `X-Pegas-Signature`. A webhook reads with the key that
created it, so it only carries the events that key's scopes allow. The URL
must be https on a public host; that, the event kinds and the description
length are checked before calling. Every command needs `webhooks:write`.

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

Third-party crates linked into the binary, their licenses and copyright notices are listed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
