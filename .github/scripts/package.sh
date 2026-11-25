#!/usr/bin/env bash
# Pack one release archive: harugate-<version>-<platform>.(tar.gz|zip)
#
#   .github/scripts/package.sh <version> <platform> <path-to-binary> <out-dir>
#
# Run from the repository root. The archive holds one folder, harugate-<version>-<platform>/,
# with the binary, README.md, both licenses and config.example.toml. Used by the release
# workflow; you can run it by hand to see what a release archive looks like.
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <version> <platform> <path-to-binary> <out-dir>" >&2
  exit 2
fi
version=$1 platform=$2 binary=$3 out=$4

name="harugate-$version-$platform"
exe=harugate
case "$platform" in windows-*) exe=harugate.exe ;; esac

mkdir -p "$out"
out=$(cd "$out" && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

mkdir "$stage/$name"
cp "$binary" "$stage/$name/$exe"
chmod 755 "$stage/$name/$exe"
cp README.md LICENSE-MIT LICENSE-APACHE config.example.toml "$stage/$name/"

case "$platform" in
  windows-*)
    archive="$name.zip"
    if command -v zip >/dev/null 2>&1; then
      (cd "$stage" && zip -qr "$out/$archive" "$name")
    else
      (cd "$stage" && 7z a -tzip -bso0 "$out/$archive" "$name")
    fi
    ;;
  *)
    archive="$name.tar.gz"
    # COPYFILE_DISABLE: keep macOS tar from adding ._ metadata files.
    (cd "$stage" && COPYFILE_DISABLE=1 tar -czf "$out/$archive" "$name")
    ;;
esac

echo "$out/$archive"
