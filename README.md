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

## Configuration

Settings are resolved in this order: flags, then `TWOBEC_*` environment
variables, then the config file, then built-in defaults.

| Option      | Environment variable | Default                       |
|-------------|----------------------|-------------------------------|
| `--profile` | `TWOBEC_PROFILE`     | `default_profile` in config, else `default` |
| `--api-url` | `TWOBEC_API_URL`     | `https://api.2befficient.com` |
| `--token`   | `TWOBEC_TOKEN`       | token stored by `auth login`  |
| `--timeout` |                      | `30` seconds                  |

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

Tokens are kept per profile in `hosts.toml` in the same directory, written
with owner-only permissions. `--token` / `TWOBEC_TOKEN` override the stored
token for one invocation.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
