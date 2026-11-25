#!/bin/sh
# harugate installer for macOS and Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/Nibelought/harugate/main/install.sh | sh
#
# What it does, in order:
#   1. works out your system (macOS, or Linux x86-64 / ARM64);
#   2. downloads the matching archive from the GitHub release (the latest, or the version
#      you ask for) and its SHA256SUMS.txt;
#   3. checks the archive against the checksum and stops if it does not match;
#   4. copies the `harugate` binary into ~/.local/bin (or HARUGATE_INSTALL_DIR);
#   5. on macOS, removes the "downloaded from the internet" flag from that binary.
# It changes nothing else: no sudo, no shell profile edits, no background services.
#
# Options (environment variables; a version can also be the first argument):
#   HARUGATE_VERSION        e.g. 0.1.0 (default: the latest release)
#   HARUGATE_INSTALL_DIR    where to put the binary (default: ~/.local/bin)
#   HARUGATE_DOWNLOAD_BASE  URL or local folder holding the release files (for mirrors
#                           and tests; default: the GitHub release page)
#
# Example: curl -fsSL .../install.sh | sh -s -- 0.1.0

set -eu

REPO="Nibelought/harugate"

say() { printf '%s\n' "$*"; }
die() { printf 'harugate install: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required but was not found"; }

need curl
need tar
need uname
need mktemp

version="${1:-${HARUGATE_VERSION:-}}"
version="${version#v}"
install_dir="${HARUGATE_INSTALL_DIR:-$HOME/.local/bin}"

# 1. Which archive do we need?
os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Darwin) platform="macos-universal" ;; # one file for Apple silicon and Intel
  Linux)
    case "$arch" in
      x86_64 | amd64) platform="linux-x64" ;;
      aarch64 | arm64) platform="linux-arm64" ;;
      *) die "unsupported CPU '$arch' (harugate is built for x86-64 and ARM64)" ;;
    esac
    ;;
  MINGW* | MSYS* | CYGWIN*) die "this is the macOS/Linux installer; on Windows use PowerShell: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" ;;
  *) die "unsupported system '$os' (harugate is built for macOS, Linux and Windows)" ;;
esac

# 2. Which version, and where from?
if [ -z "$version" ]; then
  say "Looking up the latest release..."
  # /releases/latest redirects to /releases/tag/vX.Y.Z; read the address it ends up at.
  latest_url=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") ||
    die "could not reach github.com to find the latest release"
  case "$latest_url" in
    */releases/tag/*) version="${latest_url##*/}" ;;
    *) die "no release has been published yet: see https://github.com/$REPO/releases" ;;
  esac
  version="${version#v}"
fi

base="${HARUGATE_DOWNLOAD_BASE:-https://github.com/$REPO/releases/download/v$version}"
if [ -d "$base" ]; then base="file://$base"; fi

name="harugate-$version-$platform"
archive="$name.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading harugate $version for $platform..."
curl -fsSL -o "$tmp/$archive" "$base/$archive" || die "download failed: $base/$archive"
curl -fsSL -o "$tmp/SHA256SUMS.txt" "$base/SHA256SUMS.txt" || die "download failed: $base/SHA256SUMS.txt"

# 3. Verify.
expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS.txt")
[ -n "$expected" ] || die "$archive is not listed in SHA256SUMS.txt"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$archive" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$tmp/$archive" | awk '{ print $1 }')
else
  die "need sha256sum or shasum to verify the download"
fi
[ "$expected" = "$actual" ] || die "checksum mismatch for $archive (expected $expected, got $actual); nothing was installed"
say "Checksum OK."

# 4. Install. Copy to a temporary name first, then move it into place, so a running
# harugate is replaced cleanly.
tar -xzf "$tmp/$archive" -C "$tmp" || die "could not unpack $archive"
[ -f "$tmp/$name/harugate" ] || die "the archive does not contain harugate"
mkdir -p "$install_dir" || die "could not create $install_dir"
cp "$tmp/$name/harugate" "$install_dir/.harugate.new"
chmod 755 "$install_dir/.harugate.new"
mv -f "$install_dir/.harugate.new" "$install_dir/harugate"

# 5. macOS: drop the quarantine flag, so Gatekeeper does not block the first start.
if [ "$os" = Darwin ]; then
  xattr -d com.apple.quarantine "$install_dir/harugate" 2>/dev/null || true
fi

say ""
say "Installed: $install_dir/harugate"
"$install_dir/harugate" --version 2>/dev/null || true
say ""
case ":$PATH:" in
  *":$install_dir:"*)
    say "Next: run  harugate"
    ;;
  *)
    say "$install_dir is not on your PATH yet. Either run it by its full path:"
    say "  $install_dir/harugate"
    say "or add it to your PATH (then open a new terminal):"
    case "${SHELL:-}" in
      */zsh) profile="~/.zshrc" ;;
      */bash) profile="~/.bashrc" ;;
      *) profile="your shell's startup file" ;;
    esac
    say "  echo 'export PATH=\"$install_dir:\$PATH\"' >> $profile"
    say "Then run  harugate"
    ;;
esac
say "It opens the web UI in your browser; leave the terminal open while you use Koharu."
