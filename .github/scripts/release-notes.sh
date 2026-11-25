#!/usr/bin/env bash
# Print the hand-written top of the release notes (GitHub appends the generated changelog).
#
#   .github/scripts/release-notes.sh <version> <SHA256SUMS.txt>
set -euo pipefail

version=$1 sums=$2
repo="${GITHUB_REPOSITORY:-Nibelought/harugate}"
raw="https://raw.githubusercontent.com/$repo/main"

cat <<EOF
## Install

**macOS (Apple silicon or Intel), Linux, Windows**: download the file for your system below,
or use the one-line installer.

| System | Download |
|---|---|
| macOS (any Mac) | \`harugate-$version-macos-universal.tar.gz\` |
| Windows 64-bit | \`harugate-$version-windows-x64.zip\` |
| Windows on ARM | \`harugate-$version-windows-arm64.zip\` |
| Linux x86-64 | \`harugate-$version-linux-x64.tar.gz\` |
| Linux ARM64 | \`harugate-$version-linux-arm64.tar.gz\` |

(\`macos-arm64\` and \`macos-x64\` are the single-architecture builds; most people want \`macos-universal\`.)

One-line installers:

\`\`\`sh
# macOS / Linux
curl -fsSL $raw/install.sh | sh
\`\`\`

\`\`\`powershell
# Windows (PowerShell)
irm $raw/install.ps1 | iex
\`\`\`

Then run \`harugate\` (or double-click it): your browser opens the web UI.
The binaries are not code-signed: see the README for the macOS "Open Anyway" and Windows
"Run anyway" steps.

## SHA-256 checksums

\`SHA256SUMS.txt\` is attached. The same sums:

\`\`\`
$(cat "$sums")
\`\`\`

EOF
