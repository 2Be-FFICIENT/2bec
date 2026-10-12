# Security policy

## Reporting a vulnerability

If you believe you have found a security vulnerability in `2bec`, please report it
privately by email to **security@2befficient.fr**. Do not open a public GitHub issue
for security problems.

Please include as much of the following as you can:

- A description of the issue and its impact.
- Steps to reproduce, or a proof of concept.
- The `2bec` version (`2bec --version`), operating system and how the binary was installed.
- Any relevant logs or output, with credentials redacted.

You will receive an acknowledgement within 3 business days. We will keep you informed
of progress, and we ask that you give us a reasonable time to fix the issue before
disclosing it publicly.

## Scope

This policy covers the `2bec` command-line client and this repository. Issues in the
2Be-FFICIENT API or web application should be reported to the same address.

## Supported versions

Only the latest released version of `2bec` receives security fixes.

## Handling credentials

`2bec` stores API keys and OAuth tokens in `hosts.toml` under the user's config
directory with `0600` permissions and never prints them. If you find a code path that
exposes a token in output, logs or error messages, please report it as described above.
