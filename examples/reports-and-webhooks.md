# Reports and webhooks

Saved reports, scheduled e-mails and webhooks subscriptions.

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

