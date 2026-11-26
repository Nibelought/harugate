# Development

Build, test, check, release. For how the code is organised, see [ARCHITECTURE.md](ARCHITECTURE.md).

## Setup

- Rust 1.88 or newer (`rust-version` in `Cargo.toml`; edition 2024). CI uses the current stable toolchain.
- A C compiler (Xcode command line tools on macOS, `build-essential` on Debian/Ubuntu, the MSVC Build Tools on
  Windows). No OpenSSL, CMake or NASM: TLS is rustls with the `ring` provider.

## Commands

| Command | Use |
|---|---|
| `cargo build --release` | Release binary at `target/release/harugate` |
| `cargo run -- serve` | Run from the checkout (add `--config <path>` to use a scratch config) |
| `cargo fmt --check` | Formatting (`cargo fmt` fixes it) |
| `cargo clippy --all-targets -- -D warnings` | Lints; warnings are errors |
| `cargo test --locked` | All tests (unit and integration, offline) |

Run fmt, clippy and test before declaring any change done. `Cargo.lock` is committed; `--locked` makes the build fail
rather than update it silently.

## Offline tests and live checks

Tests never touch the network beyond loopback. The upstream is a local mock (`tests/common/mod.rs`) and OpenRouter
samples are fixtures in `reference/fixtures/`. Keep it that way: a new test starts `spawn_upstream()` and
`spawn_proxy()` instead of calling OpenRouter.

A live check against the real OpenRouter costs money, so run one only when the user explicitly agrees, each time.
How to keep it cheap and safe:

1. Give the key through the environment only: `export OPENROUTER_API_KEY=...` typed in your own shell. Never put it
   on a command line, in a file in the repository, in a config file, in a log, a commit or a message. Don't paste
   output that could contain it.
2. Use a scratch config and a spare port so your normal setup is untouched (`*.local.toml` is git-ignored):
   ```sh
   cat > live.local.toml <<'EOF'
   listen = "127.0.0.1:8799"
   routing = "price"
   EOF
   cargo run -- --config live.local.toml serve
   ```
3. Send tiny requests (`"max_tokens": 16`, a one-word prompt) to `http://127.0.0.1:8799/v1/chat/completions`; harugate
   adds the key itself. Free calls first: `harugate hosts <author/slug>` and `GET /v1/models` cost nothing.
4. Check the result in `GET /api/requests` (routing applied, status) and confirm the key is absent from the
   terminal log, `/api/requests` and the config file.
5. Unset the variable and stop harugate when done.

What has been checked live, and what hasn't, is in [ROADMAP.md](ROADMAP.md) and
[OPENROUTER-NOTES.md](OPENROUTER-NOTES.md).

## CI

`.github/workflows/ci.yml` runs on pushes to `main` and on pull requests, on ubuntu, macOS and Windows (not
fail-fast), with `RUSTFLAGS=-D warnings`: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
`cargo test --locked`. Keep all three green before tagging.

## Releasing

The `version` in `Cargo.toml` is the release number, and the tag must be `v<version>`.

1. Move the `[Unreleased]` entries of [CHANGELOG.md](../CHANGELOG.md) under a new `[x.y.z] - date` heading and update
   the links at its bottom.
2. Bump `version` in `Cargo.toml` (`cargo build` updates `Cargo.lock`). Commit, push to `main`, and wait for CI.
3. Tag that commit and push the tag:
   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```
4. The **Release** workflow (`.github/workflows/release.yml`) then:
   - `prepare`: fails if the tag differs from the `Cargo.toml` version;
   - `build`: Linux x64 and arm64 (static musl, cross-built with zig 0.14.1 and `cargo-zigbuild`), Windows x64 and
     arm64 (MSVC, `+crt-static` so no redistributable is needed); runs `--version` on the x64 builds;
   - `build-macos`: arm64 and x64 builds (deployment target 11.0), a `lipo` universal binary with an ad-hoc
     signature (Apple silicon refuses unsigned code; this is not a Developer ID);
   - `package`: `SHA256SUMS.txt` over all archives;
   - `test-install`: runs `install.sh` (Linux, macOS) and `install.ps1` (Windows PowerShell 5.1) against the freshly built
     archives, and checks that `install.sh` refuses a tampered one;
   - `publish` (tags only): `gh release create` with the archives, `SHA256SUMS.txt`, install instructions from
     `.github/scripts/release-notes.sh` and GitHub's generated notes. A version with a hyphen (`0.2.0-rc1`) is marked a
     pre-release.
5. Check the release page, and try the one-line installer on at least one system.

The release workflow does not run `cargo test`; CI on `main` does, which is why step 2 waits for it.

**Dry run:** Actions → Release → *Run workflow* builds and tests everything and keeps the files as workflow artifacts;
no release is created.

**Failed release build:** fix it, delete the tag (`git push origin :v0.2.0`, `git tag -d v0.2.0`) and tag again.

### Artifacts

Archives are named `harugate-<version>-<platform>.tar.gz` (macOS, Linux) or `.zip` (Windows). Platforms:
`macos-universal`, `macos-arm64`, `macos-x64`, `linux-x64`, `linux-arm64`, `windows-x64`, `windows-arm64`. Each holds one folder
with the binary, `README.md`, both licenses and `config.example.toml`. `SHA256SUMS.txt` lists them all.
`.github/scripts/package.sh <version> <platform> <binary> <out-dir>` (run from the repository root) makes one archive
locally.

### Install scripts

`install.sh` (macOS/Linux) and `install.ps1` (Windows) pick the archive for the system, verify its SHA-256 against
`SHA256SUMS.txt`, install for the current user and change nothing else. Their options, all environment variables
(`install.sh` also takes a version as its first argument):

| Variable | Meaning | Default |
|---|---|---|
| `HARUGATE_VERSION` | Version to install | latest release |
| `HARUGATE_INSTALL_DIR` | Where to put the binary | `~/.local/bin` / `%LOCALAPPDATA%\Programs\harugate` |
| `HARUGATE_DOWNLOAD_BASE` | URL or local folder holding the release files (mirrors and tests) | the GitHub release page |

To try them against a local build, point `HARUGATE_DOWNLOAD_BASE` at a folder with the archives and
`SHA256SUMS.txt`, as the workflow does.

## Documentation checklist

When behaviour changes, update in the same change:

- [ ] [REQUIREMENTS.md](REQUIREMENTS.md): the F/N item that describes it (keep the ids; add new ids at the end of their
      group, never renumber).
- [ ] [CHANGELOG.md](../CHANGELOG.md): an entry under `[Unreleased]` for anything a user can see.
- [ ] [README.md](../README.md): install, quick start, config reference, CLI table or caveats, if they are affected.
- [ ] [ARCHITECTURE.md](ARCHITECTURE.md): module map, request flow, API table or security model, if they are affected.
- [ ] [DECISIONS.md](DECISIONS.md): a new dated entry if you made a choice a later reader would otherwise question.
- [ ] [ROADMAP.md](ROADMAP.md): remove an open item you finished; add one you found.
- [ ] `config.example.toml` and the default file in `config.rs` (`DEFAULT_CONFIG_TOML`) if the config format changed.
- [ ] [KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) or [OPENROUTER-NOTES.md](OPENROUTER-NOTES.md) if you re-verified a fact
      about Koharu or OpenRouter (say what you checked and when).
