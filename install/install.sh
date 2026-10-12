#!/bin/sh
# Installs the 2bec command line client of the 2Be-FFICIENT API.
#
#   curl -fsSL https://api.2befficient.com/cli/install.sh | sh
#
# It reads the latest version from the release feed, downloads the archive of this platform
# and its SHA256SUMS, checks the archive against them, and puts the binary in a directory of
# the PATH. Nothing else is written. Linux (x64, arm64, static) and macOS (x64, arm64); on
# Windows: irm https://api.2befficient.com/cli/install.ps1 | iex
#
# Environment:
#   TWOBEC_VERSION       a version to install (0.1.0) instead of the latest
#   TWOBEC_INSTALL_DIR   where the binary goes (default: /usr/local/bin when writable, else ~/.local/bin)
#   TWOBEC_INSTALL_URL   the release feed (default: https://api.2befficient.com/cli)
#
# This file is served by the API from the copy vendored in the pegas repository; this
# repository is its source of truth.
set -eu

feed="${TWOBEC_INSTALL_URL:-https://api.2befficient.com/cli}"
feed="${feed%/}"
releases_page="https://github.com/2Be-FFICIENT/2bec/releases"

say() { printf '%s\n' "$*" >&2; }
die() { say "install.sh: $*"; exit 1; }

# fetch URL FILE: downloads a URL into a file with curl or wget, whichever is there.
fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2"
    else
        die "neither curl nor wget is installed"
    fi
}

# sha256 FILE: the hexadecimal SHA-256 of a file, with whatever tool the system has.
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$1" | sed 's/.*= //'
    else
        die "no sha256sum, shasum or openssl to check the download"
    fi
}

# The platform, as the release archives name it.
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    *) die "unsupported system '$os' (on Windows: irm $feed/install.ps1 | iex)" ;;
esac
case "$arch" in
    x86_64 | amd64) arch=x64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *) die "unsupported architecture '$arch'" ;;
esac
platform="$os-$arch"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

# The version: pinned, else the feed's latest.
version="${TWOBEC_VERSION:-}"
if [ -z "$version" ]; then
    if ! fetch "$feed/releases/latest" "$tmp/latest.json"; then
        die "the release feed did not answer ($feed/releases/latest); the archives are at $releases_page"
    fi
    version="$(sed -n 's/.*"version":"\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -n 1)"
    [ -n "$version" ] || die "the release feed answered without a version"
fi
version="${version#v}"
case "$version" in
    *[!0-9.]* | "" | .* | *.) die "'$version' is not a version (expected 0.1.0)" ;;
esac

name="2bec-$version-$platform.tar.gz"
say "Installing 2bec $version for $platform"

if ! fetch "$feed/releases/$version/$name" "$tmp/$name"; then
    die "could not download $name from $feed/releases/$version/; the archives are at $releases_page"
fi
if ! fetch "$feed/releases/$version/SHA256SUMS" "$tmp/SHA256SUMS"; then
    die "could not download the checksums of $version"
fi

# The checksum: the line of our archive in SHA256SUMS (sha256sum's format: hash, two spaces, name).
expected="$(grep " $name\$" "$tmp/SHA256SUMS" | head -n 1 | cut -d ' ' -f 1)"
[ -n "$expected" ] || die "SHA256SUMS does not list $name"
actual="$(sha256 "$tmp/$name")"
if [ "$expected" != "$actual" ]; then
    die "checksum mismatch for $name (expected $expected, got $actual): the download is corrupt or tampered"
fi

tar -xzf "$tmp/$name" -C "$tmp"
binary="$tmp/2bec-$version-$platform/2bec"
[ -f "$binary" ] || die "the archive holds no 2bec binary"

# Where it goes: the system directory when the caller may write it, else the user's own.
dir="${TWOBEC_INSTALL_DIR:-}"
if [ -z "$dir" ]; then
    if [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
        dir=/usr/local/bin
    else
        dir="$HOME/.local/bin"
    fi
fi
mkdir -p "$dir"
cp "$binary" "$dir/2bec.new"
chmod 755 "$dir/2bec.new"
mv -f "$dir/2bec.new" "$dir/2bec"

say "Installed $dir/2bec ($("$dir/2bec" --version 2>/dev/null || echo "2bec $version"))"
case ":$PATH:" in
    *":$dir:"*) ;;
    *) say "Add $dir to your PATH, for instance: export PATH=\"$dir:\$PATH\"" ;;
esac
say "Shell completions: 2bec completion bash|zsh|fish (see 2bec completion --help). Next: 2bec auth login"
