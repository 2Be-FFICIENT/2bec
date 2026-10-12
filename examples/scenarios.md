# Scenarios and availability

The catalog, one scenario's figures and documents, its settings and thresholds, and the availability rules.

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

## Scenario settings

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

## Response time thresholds

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

