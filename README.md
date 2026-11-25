# harugate

A local OpenRouter provider-routing proxy for Koharu. The name: *Haru* (春, "spring") from Koharu, plus *gate*.

A small local HTTP proxy that sits between Koharu and OpenRouter. It adds OpenRouter's
`provider` routing object to each chat request: a global sort rule, and a provider pinned
per model.

This exists because the maintainers rejected the in-app version
(issue koharu-rs/koharu#1532, PR #1534): *"use a proxy between Koharu and OpenRouter
instead: have it add the `provider` object to each request, and set its URL under
Settings → Providers → OpenAI-compatible."*

Koharu itself is **not** modified. The proxy is a standalone project. It ships with a small web
interface (`/ui`, served by the same binary) for choosing the routing and pins, so you don't need
the command line or a text editor.

## Install

You don't need Rust. Download the file for your system from the
[latest release](https://github.com/Nibelought/harugate/releases/latest) (`<version>` is the release number,
e.g. `0.1.0`), unpack it, and run `harugate`:

| System | Download | Rust target |
|---|---|---|
| **macOS**, Apple silicon and Intel | `harugate-<version>-macos-universal.tar.gz` | both `aarch64-apple-darwin` and `x86_64-apple-darwin` |
| **Windows** 10/11, 64-bit | `harugate-<version>-windows-x64.zip` | `x86_64-pc-windows-msvc` |
| Windows on ARM | `harugate-<version>-windows-arm64.zip` | `aarch64-pc-windows-msvc` |
| **Linux** x86-64 | `harugate-<version>-linux-x64.tar.gz` | `x86_64-unknown-linux-musl` |
| Linux ARM64 (Raspberry Pi 4/5, ARM servers) | `harugate-<version>-linux-arm64.tar.gz` | `aarch64-unknown-linux-musl` |

Also published: `macos-arm64` and `macos-x64` (one architecture each; the universal file covers both) and
`SHA256SUMS.txt`. The Linux builds are static: they run on any distribution. Each archive holds the
program, this README, both licenses and `config.example.toml`.

**One-line install** (downloads the right file, checks its SHA-256, puts it in your user folder; no
administrator rights):

```sh
# macOS / Linux: installs to ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/Nibelought/harugate/main/install.sh | sh
```

```powershell
# Windows PowerShell: installs to %LOCALAPPDATA%\Programs\harugate and adds it to your PATH
irm https://raw.githubusercontent.com/Nibelought/harugate/main/install.ps1 | iex
```

Read them first if you like: [`install.sh`](install.sh) and [`install.ps1`](install.ps1) are short. A
specific version: `curl -fsSL .../install.sh | sh -s -- 0.1.0`, or in PowerShell
`$env:HARUGATE_VERSION = "0.1.0"` before the `irm` line. To uninstall, delete the `harugate` file (and, on
Windows, the folder from your PATH). Your config stays where it is (see "Run").

**First run:** double-click `harugate` (`harugate.exe`), or type `harugate` in a terminal. The proxy starts and
your browser opens its page at <http://127.0.0.1:8787/ui>. Leave the window open while you use Koharu;
closing it stops the proxy. Then follow "Set up Koharu" below. If the proxy is already running, a second
start says so and opens the page of the first.

**macOS says "can't be opened" or "Apple cannot check it":** the binaries are not signed with an Apple
developer certificate. The one-line installer avoids the warning. For a file you downloaded in the browser,
do either of these once:

- System Settings → Privacy & Security → scroll down to the message about `harugate` → **Open Anyway**; or
- in Terminal: `xattr -d com.apple.quarantine /path/to/harugate`.

**Windows SmartScreen says "Windows protected your PC":** the program is not code-signed. Click
**More info**, then **Run anyway**. (The installer script doesn't trigger it.)

**Linux:** from the file manager, open a terminal in the folder and run `./harugate` (add `chmod +x harugate`
if needed).

**Build from source** instead (needs [Rust](https://rustup.rs) 1.88+):

```sh
cargo install --git https://github.com/Nibelought/harugate
```

More options are under "Build from source" in Usage.

## Folder map

| Path | What it is |
|---|---|
| `CLAUDE.md` | Scoped instructions for an agent session working in this folder |
| `docs/REQUIREMENTS.md` | What the proxy must do (functional and non-functional) |
| `docs/TASKS.md` | Ordered tasks, each with acceptance criteria |
| `docs/KOHARU-CONTRACT.md` | What Koharu sends and expects, verified against its source |
| `docs/OPENROUTER-NOTES.md` | The `provider` object, the endpoints API, results of the live tests run for the PR |
| `config.example.toml` | The proposed config format |
| `ui/` | The web UI: one HTML, CSS and JS file, embedded in the binary |
| `.github/workflows/ci.yml` | fmt, clippy and tests on Linux, macOS and Windows |
| `.github/workflows/release.yml` | Builds the release archives for every platform and publishes a GitHub Release when a `v*` tag is pushed |
| `.github/scripts/` | `package.sh` (makes one archive), `release-notes.sh` (the top of the release notes) |
| `install.sh`, `install.ps1` | The one-line installers for macOS/Linux and Windows |
| `reference/routing.rs` | **Tested** Rust from PR #1534: building the `provider` object, parsing the host list, explaining pin errors (6 unit tests) |
| `reference/host-label.ts` | **Tested** labelling of hosts from the PR's UI (e.g. `DeepInfra (fp8, priority)`) |
| `reference/fixtures/` | Sample endpoints reply and the 404 body OpenRouter returns for a pin that can't serve a request |

## Decisions already made

- **Language: Rust** (recommended; change it in TASKS.md T0 if you prefer something else).
  `reference/routing.rs` then ports almost verbatim, tests included, and the stack
  (tokio, axum, reqwest) is the one Koharu already uses.
- **The TOML file stays the source of truth**, edited by hand or by the local web UI. Routing rule
  and pins live in a TOML file that is re-read on change; the UI writes into that same file, in place,
  so your comments and key order survive.
- **Web UI: plain HTML/CSS/JS, embedded** with `include_str!` (no npm, no build step, no CDN or
  external fonts: it works offline and talks to nobody but your proxy). It is local-only and guarded
  against malicious web pages (DNS rebinding, CSRF, framing; see "The web UI and its safety" below
  and N8–N12).
- **A bare launch opens the UI.** `harugate` with no subcommand is `serve`, and also opens
  the browser when started from a terminal or by double-click (stdin and stdout are terminals), so a
  double-click on Windows or macOS is enough. `serve` alone never opens a browser (scripts, services);
  `serve --open` always does.
- **TLS: rustls with `ring`**, not reqwest's default aws-lc. aws-lc-sys needs CMake and, on Windows,
  NASM (or its prebuilt-objects switch) to build; `ring` ships its assembly pre-generated and needs
  only a C compiler (MSVC on Windows), so `cargo build` works on a fresh Windows machine. The provider
  is installed in `state::build_client()`.
- **Koharu connects through "OpenAI-compatible"**, with base URL `http://127.0.0.1:<port>/v1`.
- **T0 stack, confirmed: Rust.** tokio + axum 0.8 (server), reqwest 0.13 with `stream` (client,
  rustls, so no OpenSSL on any OS), serde/serde_json (`preserve_order`, so the request body keeps
  its field order), toml, clap, tracing + tracing-subscriber (`RUST_LOG`), `dirs` for the default
  config path. Built as a library plus a thin binary (`src/lib.rs`, `src/main.rs`), so a UI can be
  added on top of the same `axum::Router`. Targets macOS, Linux and Windows: no unix-only APIs outside `cfg(unix)`; CI runs on all three.
- **F1, other paths: transparent passthrough under `/v1/*`** (any method, body streamed, query
  kept, same auth rules); anything outside `/v1` other than `/health` is a 404 and is never forwarded.
- **Default config path:** `<config dir>/harugate/config.toml` (macOS
  `~/Library/Application Support`, Linux `~/.config`, Windows `%APPDATA%`), created with defaults
  on first `serve`. An explicit `--config` path must exist.
- **Hot reload:** the config file is re-read on every request (it is tiny) and compared with the last
  text, rather than watched. A bad edit keeps the last good config, is logged once, and is shown by
  `/health` as `config_error`. `listen` is only read at startup.
- **No `Accept-Encoding` is forwarded**, so the upstream answers uncompressed and the F7 rewrite can
  read its 404. Redirects are not followed; the client sees the upstream's status as is.

## Usage

### Build from source

(Most people want the downloads under "Install" at the top.) You need [Rust](https://rustup.rs) 1.88 or
newer. No OpenSSL, CMake or NASM.

| System | Steps |
|---|---|
| macOS | `xcode-select --install` (once, for the C compiler), then `cargo build --release` |
| Linux | a C compiler (`sudo apt install build-essential` or your distro's equivalent), then `cargo build --release` |
| Windows | install Rust with rustup (it asks for the "Desktop development with C++" Build Tools; accept), then `cargo build --release` in PowerShell |

The binary is `target/release/harugate` (`harugate.exe` in `target\release` on Windows).
Copy it anywhere, or `cargo install --path .` (from a checkout) or `cargo install --git
https://github.com/Nibelought/harugate` to put it on your PATH. It is one file: the UI is inside.

### Run

- **Double-click** the binary, or run `harugate` in a terminal. The proxy starts and your browser
  opens the UI at <http://127.0.0.1:8787/ui>. Leave the window open: closing it stops the proxy.
- `harugate serve --open` does the same explicitly; `harugate serve` starts it without
  opening a browser (the UI URL is in the log: `web UI: http://127.0.0.1:8787/ui`).
- Stop with Ctrl-C.

The config file is created on first run at `<your config dir>/harugate/config.toml`
(macOS `~/Library/Application Support`, Linux `~/.config`, Windows `%APPDATA%`). Use another file
with `--config <path>` (it must exist). The file's path is shown in the UI.

Your OpenRouter key goes in Koharu (below). To keep it in the proxy instead, set `OPENROUTER_API_KEY`
before starting it (macOS/Linux: `export OPENROUTER_API_KEY=sk-or-...`; Windows PowerShell:
`setx OPENROUTER_API_KEY "sk-or-..."`, then open a new window). It replaces whatever Koharu sends.
The UI only shows whether it is set, never the key, and has no field for one.

### Set up Koharu

In Koharu: Settings → Providers → **OpenAI-compatible**.

1. **Base URL:** `http://127.0.0.1:8787/v1` (the UI shows yours, with a Copy button, in "Connect Koharu").
2. **API key:** your OpenRouter key, or leave it empty if the proxy has `OPENROUTER_API_KEY`.
3. **Context window: set it.** OpenAI-compatible applies one window to all its models and reads none from
   the server; unset, a conversation grows until the server cuts it off. Use the smallest window of the
   models you use.
4. Pick the model under Pipeline → Translator. It shows as an OpenAI-compatible model with OpenRouter's id.

### Choose routing and pin a host (web UI)

1. Open the UI. Under **Routing & pins**, pick how models *without* a pin are sorted: Balanced
   (OpenRouter's default), Price, Throughput or Latency. Leave "Only use hosts that support every setting
   the request sends" on unless you know you need it off.
2. To pin a model to one host: type its id (it completes from OpenRouter's model list) and press
   **Find hosts**. The table lists each host (sorted by name) with its quantization and whether it supports
   `tools` and `structured_outputs`. Press **Pin this** on the one you want.
3. Press **Save changes**. It applies to the next request; no restart.
4. **Requests** shows the last 100 requests (model, what the proxy applied, status, time), so you can
   see a pin in use. Nothing but those facts is kept, and only in memory.

**Advanced** holds the upstream URL and the listen address (the latter changes after a restart), and
**Raw file** edits the TOML directly; it checks your edit and shows the error without saving a bad file.

### Find and pin a tag from the command line

```
harugate hosts deepseek/deepseek-v4.1-flash-1017
```

prints one line per host: tag, label, quantization, `tools=…`, `structured_outputs=…`. Put the tag in
the config file and save; the running proxy picks it up on the next request:

```toml
[pins]
"deepseek/deepseek-v4.1-flash-1017" = "deepinfra/fp8"
```

### Caveats

- **Agent + pins.** Koharu's in-app agent goes through the proxy too, and it sends `tools`. A model
  pinned to a host that doesn't support tools answers 404 to every agent request (the proxy explains
  which pin it was). The UI marks those hosts. If you use the agent, pin only hosts with tools=yes, or
  don't pin that model.
- **Pins don't fall back.** A pinned request fails rather than use another host; that is what a pin is.
- **Context window.** Set it in Koharu (see above); the proxy can't know it.
- **`~` aliases** (`~vendor/model`) list no endpoints, so they can't be pinned.
- **Port in use?** Change `listen` (Advanced, or the file) and restart; then update Koharu's base URL.

### The web UI and its safety

The UI can change where your API key is sent (the `upstream`), so it is built for a local, single user:

- It only answers requests addressed to `localhost`, `127.0.0.1` or `[::1]` (or your configured
  `listen` address). A web page that points its own name at your machine (DNS rebinding) is refused.
- Changes need `Content-Type: application/json` and an `X-Proxy-UI: 1` header, and any `Origin` must be
  the proxy's own. No CORS headers are sent, so other web pages can't call the API.
- Pages carry a strict Content-Security-Policy (no inline scripts, no outside resources), `nosniff`,
  no framing and no referrer.
- Setting an upstream that isn't `https://openrouter.ai/...` or a listen address that isn't loopback
  shows a strong warning and needs your explicit confirmation (the API enforces this too).
- Keys never appear in API answers, the request list, the page or the logs.

`/v1/*` and `/health` are not part of this and behave as before, so Koharu is unaffected.

### API (for scripts)

All under `/api`, JSON; errors are `{"error":{"message"}}`. Changing requests need the two headers above.

| Route | |
|---|---|
| `GET /api/status` | config path, config error, listen, upstream, whether the key is set, pin count, Koharu base URL |
| `GET /api/config`, `PUT /api/config` | the settings as JSON; the PUT body is the whole config (`routing` null = Balanced) and writes into the file in place |
| `GET /api/config/raw`, `PUT /api/config/raw` | the file's text; the PUT body is `{"text": "..."}` |
| `GET /api/models` | model ids from the upstream (cached 5 minutes) |
| `GET /api/hosts?model=author/slug` | the model's hosts, sorted by label |
| `GET /api/requests` | the last 100 requests, newest first |

### Development

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must be clean (CI runs
them on Linux, macOS and Windows). Tests are offline: the upstream is a local mock. Live OpenRouter
checks are task T10, run by hand.

### Releasing

For maintainers. The version in `Cargo.toml` is the release number, and the tag must match it.

1. Bump `version` in `Cargo.toml` (`cargo build` updates `Cargo.lock`), commit and push to `main`; wait for CI.
2. Tag that commit and push the tag:
   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```
3. The **Release** workflow checks that the tag equals the `Cargo.toml` version, builds all platforms, tests
   the install scripts against the fresh archives, and only then publishes the GitHub Release (archives,
   `SHA256SUMS.txt`, install instructions, generated notes). A tag with a hyphen (`v0.2.0-rc1`) is marked a
   pre-release.
4. To rehearse without publishing: Actions → Release → *Run workflow*. It builds and tests everything and
   keeps the files as workflow artifacts; no release is created.

If a release build fails, fix it, delete the tag (`git push origin :v0.1.0`, `git tag -d v0.1.0`) and tag again.
`.github/scripts/package.sh <version> <platform> <binary> <out-dir>` makes one archive locally.

## License

harugate is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

Parts of `src/routing.rs`, `src/hosts.rs` and the files in `reference/` are ported from Koharu
(https://github.com/koharu-rs/koharu), Copyright (c) 2025-2026 Mayo Takanashi and Koharu contributors,
used under its MIT/Apache-2.0 license.
