# Scripting

How to use `2bec` from shell scripts, cron jobs and CI pipelines.

## Output modes

Tables on a terminal, tab-separated text in a pipe, `--json` for the full
payload (pretty on a terminal, compact in a pipe). `--jq EXPR` runs the JSON
through the `jq` program on your PATH, so scripts can pick fields in place:

```sh
2bec scenarios list --jq '.[] | select(.active) | .id'
2bec runs get <run> --jq '.steps[] | [.label, .durationMs] | @tsv'
```

Colors and spinners are never written into a pipe; `NO_COLOR` is honored.


Lists print their items as a JSON array, so `.[]` iterates them; single
objects print as is. Text arguments accept `-` for stdin and `@file`, and
`-o -` sends a download to stdout.

## Exit codes in bash

Every failure is one line on stderr and a meaningful exit code (see the
table in the [README](../README.md#exit-codes)), so a script can tell an
expired key from an outage:

```sh
#!/usr/bin/env bash
set -euo pipefail

if ! out=$(2bec runs list --status failed --period today --json); then
  case $? in
    3) echo "credential rejected, run: 2bec auth login" >&2; exit 1 ;;
    4) echo "API unreachable, retrying later" >&2; exit 0 ;;
    *) exit 1 ;;
  esac
fi
echo "$out" | jq -r '.[] | [.scenario.name, .failedStep.label // "-"] | @tsv'
```

Pass `--verbose` to keep the cause chain and the API trace id in the log of
a failing job.

## Credentials in automation

Use an API key, not a person login: a key carries exactly the scopes the
job needs, survives the 30-day refresh window of a login, and honors
`Idempotency-Key` on writes.
Give it through the environment and never store it in the script:

```sh
export TWOBEC_TOKEN="$TWOBEC_TOKEN"     # a CI secret, e.g. 2bef_live_…
2bec auth status --json | jq '.scopes'   # check what the job may do
```

`TWOBEC_PROFILE` and `TWOBEC_API_URL` select the environment the same way.

## Gate a deployment on a run

Trigger the smoke scenario after a deploy, wait for the run and fail the
pipeline when the platform reports a failure:

```sh
2bec runs trigger "$SCENARIO" --var BUILD="$CI_COMMIT_SHA" --wait
failed=$(2bec runs list --scenario "$SCENARIO" --status failed --period today --jq 'length')
[ "$failed" -eq 0 ] || { echo "$failed failed run(s) today"; exit 1; }
```

`trigger` needs `runs:request`; a retried trigger never starts two runs.

## Nightly export

A cron entry that keeps a daily TSV of the runs and the monthly SLA PDF:

```sh
# every day at 02:00, Paris time (the API's day boundary)
0 2 * * * 2bec runs list --period yesterday --all --jq '.[] | [.startedAt, .scenario.name, .status, .durationMs] | @tsv' > "/var/log/2bec/$(date +\%F).tsv"
# the first of the month, last month's availability report
0 3 1 * * 2bec metrics pdf availability --period lastMonth -o "/srv/reports/sla-$(date +\%Y-\%m).pdf"
```

## Watch for failures and page

Poll on a timer and send only when something changed since the last poll:

```sh
last=/var/tmp/2bec-last-failed
now=$(2bec runs list --status failed --period today --jq 'map(.id) | sort | join("\n")')
new=$(comm -13 <(cat "$last" 2>/dev/null) <(printf '%s\n' "$now"))
[ -n "$new" ] && printf '%s\n' "$new" | xargs -I{} 2bec runs get {} --jq '"\(.scenario.name): \(.failure.description // "failed")"' | notify-team
printf '%s\n' "$now" > "$last"
```

For a push model, a webhook on `run.failed` and `incident.opened` does the
same without polling: see [reports-and-webhooks.md](reports-and-webhooks.md).
