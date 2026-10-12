# Administration

Runtime variables, tags and badges, users and access profiles, the audit history, comments and tickets.

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

