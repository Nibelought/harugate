# Decisions

Why harugate is built the way it is. One short entry per decision: context, decision, why. Add new entries at the
end, with the date; if a decision is reversed, add a new entry and mark the old one "Superseded by" instead of
deleting it. Behaviour itself is specified in [REQUIREMENTS.md](REQUIREMENTS.md).

## 2026-11-25: A proxy, not an in-app feature

- **Context.** The first attempt was an in-app OpenRouter routing setting for Koharu (issue koharu-rs/koharu#1532,
  PR #1534). The maintainers declined it and suggested a proxy that adds the `provider` object, configured under
  Settings → Providers → OpenAI-compatible.
- **Decision.** A standalone local proxy. Koharu is not modified; `../koharu` is only a read-only reference.
- **Why.** It is what the maintainers asked for, and it needs nothing from Koharu beyond its existing
  OpenAI-compatible provider. The tested code from the PR is ported (`src/routing.rs`, `src/hosts.rs`), not rewritten.

## 2026-11-25: Rust, as a library plus a thin binary

- **Decision.** Rust with tokio, axum 0.8, reqwest 0.13 (`stream`), serde/serde_json (`preserve_order`), toml, clap,
  tracing and `dirs`. `src/lib.rs` holds everything; `src/main.rs` only parses arguments.
- **Why.** The PR's code ports almost verbatim, tests included, and the stack is the one Koharu uses. A library
  lets tests build the same `Router` the binary serves. No unix-only APIs outside `cfg(unix)`, so macOS, Linux and
  Windows all work.

## 2026-11-25: Koharu connects through "OpenAI-compatible"

- **Decision.** Base URL `http://127.0.0.1:<port>/v1`; harugate serves the Chat Completions surface Koharu's
  OpenAI-compatible vendor uses. Everything on that vendor passes through, including the in-app agent.
- **Why.** It is the only provider type that takes a custom URL. The cost is that the agent's requests carry `tools`,
  so a pin to a host without tools breaks the agent; this is documented, and the page warns
  ([KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) §4). Koharu uses prompted structured output on this vendor, so hosts
  without `structured_outputs` are listed, unlike in the PR (§3).

## 2026-11-25: Pins are exclusive; the pin wins over the sort

- **Decision.** A pinned model gets `only: [tag]` and `allow_fallbacks: false`, with no `sort`. `require_parameters`
  defaults to `true` (matching Koharu's own OpenRouter vendor) and is configurable.
- **Why.** A pin means "this host or fail". Falling back would hide that the pinned host can't serve the request.
  A pin that fails is explained in plain words (F7).

## 2026-11-25: The TOML file is the source of truth, and the UI edits it in place

- **Decision.** Routing and pins live in one TOML file, edited by hand or by the web page. The page's structured
  form writes with `toml_edit` into the existing text, so comments, blank lines and key order survive; the result is
  re-parsed and must equal the requested config, or nothing is written. Writes are atomic (temp file and rename).
- **Why.** People edit the file by hand too; rewriting it from a data structure would erase their comments.

## 2026-11-25: Hot reload by re-reading the file on every request

- **Decision.** `ConfigStore::current()` reads the file on every request and re-parses it only if its text changed.
  No file watcher, no mtime check. A bad edit keeps the last good config, is logged once and shown in `/health` and
  the page. `listen` is only read at startup.
- **Why.** The file is tiny, a read per request is cheap, and it behaves the same on every OS (no watcher
  backends, no coarse-mtime races). Comparing text means an edit is never missed.

## 2026-11-25: /v1 is passed through; everything else is a 404

- **Decision.** Under `/v1/*`, any method and path is forwarded to the upstream with the body streamed, the query
  kept and the same auth rules; only `POST /v1/chat/completions` is modified. Outside `/v1`, only `/health` and the UI
  routes exist; everything else is a 404 and is never forwarded.
- **Why.** Koharu reads `/v1/models` and may call other endpoints; transparent passthrough needs no per-path code.
  Paths outside `/v1` have no reason to reach the upstream.

## 2026-11-25: Transparent transport: no retries, no redirects, no compression, no total timeout

- **Decision.** Upstream statuses, bodies and headers (`Retry-After` included) reach Koharu unchanged, except the F7
  rewrite. harugate does not retry, does not follow redirects, forwards no `Accept-Encoding` (so the upstream answers
  uncompressed and the F7 rewrite can read its 404), and has a connect timeout only.
- **Why.** Koharu's HTTP client already retries 429 and 5xx using those headers, and reasoning models can be silent
  for minutes.

## 2026-11-26: rustls with ring, not aws-lc

- **Decision.** reqwest is built with `rustls-no-provider` and the `ring` provider is installed in
  `state::build_client()`.
- **Why.** reqwest's default (aws-lc) needs CMake and, on Windows, NASM to build. `ring` ships its assembly
  pre-generated and needs only a C compiler, so `cargo build` works on a fresh Windows machine, and no OpenSSL is
  needed anywhere.

## 2026-11-26: The web UI is embedded, with no build step and no CDN

- **Decision.** `ui/` is one HTML, one CSS and one JS file (no framework, no npm), embedded with `include_str!` and
  served by the same binary. No external fonts or scripts.
- **Why.** One self-contained file to download, it works offline, nothing is sent to third parties, and the strict
  Content-Security-Policy (no inline script or style) stays simple.

## 2026-11-26: The UI is local-only and guarded

- **Context.** The page can change `upstream`, which receives the user's API key, and `listen`.
- **Decision.** Host-header check (DNS rebinding), a JSON content type plus a custom `X-Proxy-UI` header, `Origin`
  and `Sec-Fetch-Site` checks, no CORS headers, a strict CSP and related headers, and an explicit confirmation
  (`confirm_risky`, enforced by the API) for a non-OpenRouter upstream or non-loopback listen address. No field for an
  API key; its presence is shown as yes/no only. Details in [ARCHITECTURE.md](ARCHITECTURE.md#security-model);
  requirements N8-N12.
- **Why.** A malicious web page open in the same browser could otherwise redirect the key to itself.

## 2026-11-26: A bare launch opens the page

- **Decision.** `harugate` with no subcommand is `serve` and also opens the browser when stdin and stdout are both
  terminals (so a double-click works). `serve` alone never opens one; `serve --open` always does. A busy port says the
  page is probably already running and opens it. On a double-clicked Windows console, errors wait for Enter.
- **Why.** The target user may not use a command line at all; scripts and services must not open browsers.

## 2026-11-26: Default config path from the platform's config directory

- **Decision.** `<config dir>/harugate/config.toml` via the `dirs` crate, created with defaults on the first `serve`.
  An explicit `--config` path must exist.
- **Why.** The right place on each OS without code of our own; an explicit path that doesn't exist is more likely a typo
  than a request to create a file.

## 2026-11-26: Release targets and build choices

- **Decision.** macOS: arm64 and x64 plus a `lipo` universal binary, ad-hoc signed. Windows: x64 and arm64 (MSVC),
  C runtime linked in (`+crt-static`). Linux: x64 and arm64, static musl, cross-built with zig. Release profile: LTO,
  one codegen unit, stripped. The binaries are not signed with a Developer ID or Authenticode certificate.
- **Why.** Static musl runs on any distribution; `crt-static` avoids the Visual C++ redistributable; Apple silicon
  refuses completely unsigned code. Paid certificates were out of scope for the first release
  (see [ROADMAP.md](ROADMAP.md)); the README explains the warnings.

## 2026-11-26: Install scripts are per-user and verify checksums

- **Decision.** `install.sh` and `install.ps1` download the matching archive, check its SHA-256 against
  `SHA256SUMS.txt`, install for the current user (`~/.local/bin`, `%LOCALAPPDATA%\Programs\harugate`) and change
  nothing else: no sudo, no services. The release workflow tests both against the freshly built archives and
  publishes only if they pass.
- **Why.** An install must be safe to run from a pipe, and a broken script should never reach a release.

## 2026-11-26: Dual MIT / Apache-2.0 license

- **Decision.** `MIT OR Apache-2.0`, at the user's option.
- **Why.** It matches Koharu's license, under which the ported code (`src/routing.rs`, `src/hosts.rs`, `reference/`) is
  used; the copyright notice for that code is in the README and the file headers.

## 2026-11-26: The name "harugate"

- **Decision.** *Haru* (春, "spring") from Koharu, plus *gate*. Renamed from the working title `openrouter-proxy`
  before the first release.
- **Why.** A short, distinctive name that says it belongs with Koharu and is a gate to OpenRouter.
