# Configuration

Precedence, the config file and profiles, the `config` command, shell completions and manual pages.

## Precedence and profiles

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

The `config` group edits that file without opening an editor (comments are
not preserved), and shows where each effective value comes from:

```sh
2bec config path                                   # where config.toml lives
2bec config list                                   # effective values and their source
2bec config get api_url
2bec --profile dev config set api_url http://localhost:5152
2bec --profile dev config unset issuer
2bec config profile list                           # configured or logged-in profiles
2bec config profile use dev                        # sets default_profile
```

Keys: `api_url`, `issuer`, `oauth_client_id`, `api_resource` (per profile),
`default_profile` and `credential_store`.

Credentials are kept per profile in `hosts.toml` in the same directory,
written with owner-only permissions. `--token` / `TWOBEC_TOKEN` override the stored
token for one invocation.

## Shell completions and manual pages

Both are generated from the command definitions, so they always match `--help`:

```sh
2bec completion bash > ~/.local/share/bash-completion/completions/2bec
2bec completion zsh > "${fpath[1]}/_2bec"
2bec completion fish > ~/.config/fish/completions/2bec.fish
2bec completion powershell >> $PROFILE

2bec man | man -l -                       # read the main page now
2bec man --dir ~/.local/share/man/man1    # install one page per command
```

