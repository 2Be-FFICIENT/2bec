#!/usr/bin/env bash
# Build (if needed) and run the 2bec CLI, forwarding all arguments.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin="$root/target/release/2bec"

# Rebuild when the binary is missing or any source/manifest is newer than it.
needs_build=0
if [[ ! -x "$bin" ]]; then
    needs_build=1
elif [[ -n "$(find "$root/src" "$root/Cargo.toml" "$root/Cargo.lock" -newer "$bin" -print -quit)" ]]; then
    needs_build=1
fi

if (( needs_build )); then
    cargo build --release --manifest-path "$root/Cargo.toml" --quiet
fi

exec "$bin" "$@"
