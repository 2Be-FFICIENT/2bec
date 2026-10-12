#!/usr/bin/env bash
# End-to-end test of install/install.sh against a fake release feed served from a temporary
# directory (no network): a release of a stand-in binary, its SHA256SUMS, the latest document.
# Checks the happy path, a pinned version, and the refusal of a corrupt archive.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)"
server_pid=""
cleanup() {
    [[ -n "$server_pid" ]] && kill "$server_pid" 2>/dev/null || true
    rm -rf "$tmp"
}
trap cleanup EXIT

os="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$os" in linux) os=linux ;; darwin) os=macos ;; esac
arch="$(uname -m)"
case "$arch" in x86_64 | amd64) arch=x64 ;; aarch64 | arm64) arch=arm64 ;; esac
platform="$os-$arch"

# A release: a shell script standing for the binary, packed as the release pipeline packs it.
release() {
    local version="$1" dir="$tmp/feed/releases/$1" stage
    mkdir -p "$dir"
    stage="$tmp/stage/2bec-$version-$platform"
    mkdir -p "$stage"
    printf '#!/bin/sh\necho "2bec %s"\n' "$version" > "$stage/2bec"
    chmod 755 "$stage/2bec"
    tar -C "$tmp/stage" -czf "$dir/2bec-$version-$platform.tar.gz" "2bec-$version-$platform"
    (cd "$dir" && sha256sum ./* | sed 's|\./||' > SHA256SUMS)
    rm -rf "$tmp/stage"
}
release 0.1.0
release 0.2.0
mkdir -p "$tmp/feed/releases/latest"
printf '{"version":"0.2.0","assets":{}}' > "$tmp/feed/releases/latest/index.html"

# The feed over a loopback port chosen by the system.
python3 -u -m http.server --bind 127.0.0.1 --directory "$tmp/feed" 0 > "$tmp/server.log" 2>&1 &
server_pid=$!
for _ in $(seq 1 50); do
    port="$(sed -n 's/.*port \([0-9]*\).*/\1/p' "$tmp/server.log" | head -n 1)"
    [[ -n "$port" ]] && break
    sleep 0.1
done
[[ -n "$port" ]] || { echo "the test server did not start"; cat "$tmp/server.log"; exit 1; }
feed="http://127.0.0.1:$port"

fail() { echo "FAIL: $*"; exit 1; }

# 1. The latest version lands in the install directory.
TWOBEC_INSTALL_URL="$feed" TWOBEC_INSTALL_DIR="$tmp/bin" sh "$root/install.sh"
[[ "$("$tmp/bin/2bec" --version)" == "2bec 0.2.0" ]] || fail "latest: expected 2bec 0.2.0, got $("$tmp/bin/2bec" --version)"

# 2. A pinned version, with or without its v, replaces it.
TWOBEC_INSTALL_URL="$feed" TWOBEC_INSTALL_DIR="$tmp/bin" TWOBEC_VERSION=v0.1.0 sh "$root/install.sh"
[[ "$("$tmp/bin/2bec" --version)" == "2bec 0.1.0" ]] || fail "pinned: expected 2bec 0.1.0"

# 3. A corrupt archive is refused and the installed binary stays.
printf 'garbage' >> "$tmp/feed/releases/0.2.0/2bec-0.2.0-$platform.tar.gz"
if TWOBEC_INSTALL_URL="$feed" TWOBEC_INSTALL_DIR="$tmp/bin" sh "$root/install.sh" 2> "$tmp/err"; then
    fail "a corrupt archive was installed"
fi
grep -q "checksum mismatch" "$tmp/err" || fail "the corrupt archive was not refused for its checksum: $(cat "$tmp/err")"
[[ "$("$tmp/bin/2bec" --version)" == "2bec 0.1.0" ]] || fail "the corrupt archive replaced the binary"

# 4. A feed that does not answer names the releases page.
if TWOBEC_INSTALL_URL="$feed/nowhere" TWOBEC_INSTALL_DIR="$tmp/bin" sh "$root/install.sh" 2> "$tmp/err"; then
    fail "a missing feed installed something"
fi
grep -q "github.com/2Be-FFICIENT/2bec/releases" "$tmp/err" || fail "the fallback message lacks the releases page: $(cat "$tmp/err")"

echo "install.sh: 4 checks passed"
