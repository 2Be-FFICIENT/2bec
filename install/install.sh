#!/bin/sh
# Installs the 2bec command line client of the 2Be-FFICIENT API.
#
#   curl -fsSL https://api.2befficient.com/cli/install.sh | sh
#
# It reads the latest version from the release feed, downloads the archive of this platform
# and its SHA256SUMS, checks the archive against them, and puts the binary in a directory of
# the PATH. The shell completions (bash, zsh, fish) and the manual pages of the archive go
# next to it: under /usr/local/share for a system install, under ~/.local/share otherwise;
# zsh only reads its directory once it is on fpath, which the script says. Linux (x64, arm64,
# static) and macOS (x64, arm64); on Windows: irm https://api.2befficient.com/cli/install.ps1 | iex
#
# Environment:
#   TWOBEC_VERSION              a version to install (0.1.0) instead of the latest
#   TWOBEC_INSTALL_DIR          where the binary goes (default: /usr/local/bin when writable, else ~/.local/bin)
#   TWOBEC_INSTALL_URL          the release feed (default: https://api.2befficient.com/cli)
#   TWOBEC_INSTALL_COMPLETIONS  0 to leave the completions and the manual pages out
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
unpacked="$tmp/2bec-$version-$platform"
binary="$unpacked/2bec"
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

# The completions and the manual pages, next to the binary: the share directory of a system
# install (/usr/local/bin -> /usr/local/share), the user's own otherwise.
# put SOURCE DIR NAME: copies a file of the archive when it is there.
put() {
    [ -f "$1" ] || return 0
    if ! { mkdir -p "$2" && cp "$1" "$2/$3" && chmod 644 "$2/$3"; } 2>/dev/null; then
        say "Could not write $2/$3 (skipped)"
    fi
}
if [ "${TWOBEC_INSTALL_COMPLETIONS:-1}" != 0 ]; then
    share="$HOME/.local/share"
    case "$dir" in
        */bin)
            # The share directory beside a system bin, when it (or its parent, to create it) is writable.
            candidate="${dir%/bin}/share"
            if [ -w "$candidate" ] || { [ ! -e "$candidate" ] && [ -w "${dir%/bin}" ]; }; then share="$candidate"; fi
            ;;
    esac
    put "$unpacked/completions/2bec.bash" "$share/bash-completion/completions" 2bec
    put "$unpacked/completions/2bec.zsh" "$share/zsh/site-functions" _2bec
    put "$unpacked/completions/2bec.fish" "$share/fish/vendor_completions.d" 2bec.fish
    for page in "$unpacked"/man/man1/*.1; do
        put "$page" "$share/man/man1" "$(basename "$page")"
    done
    say "Completions and manual pages under $share (bash and fish load them on the next shell)"
    case "${SHELL:-}" in
        */zsh) say "zsh: add to ~/.zshrc before compinit: fpath=(\"$share/zsh/site-functions\" \$fpath)" ;;
    esac
fi
say "Next: 2bec auth login"
