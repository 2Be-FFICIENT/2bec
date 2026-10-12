# Runs

Listing, inspecting, triggering and annotating scenario runs.

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

