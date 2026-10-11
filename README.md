# 2bec

A small command-line tool with an HTTP backend, written in Rust.

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
./2bec.sh get /get?x=1 --pretty
echo '{"hello":"world"}' | ./2bec.sh post /post --pretty
```

Run `./2bec.sh --help` for all commands and options.

## Configuration

| Option       | Environment variable | Default               |
|--------------|----------------------|-----------------------|
| `--base-url` | `TWOBEC_BASE_URL`    | `https://httpbin.org` |
| `--token`    | `TWOBEC_TOKEN`       | none                  |
| `--timeout`  |                      | `30` seconds          |
