# Monitoring

Metrics, alerts, neutralizations, incidents, interruptions and planning.

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

