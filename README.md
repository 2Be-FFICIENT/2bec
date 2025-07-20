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

| Option      | Environment variable | Default                       |
|-------------|----------------------|-------------------------------|
| `--api-url` | `TWOBEC_API_URL`     | `https://api.2befficient.com` |
| `--token`   | `TWOBEC_TOKEN`       | none                          |
| `--timeout` |                      | `30` seconds                  |

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
