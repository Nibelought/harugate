# Tasks

Do them in order. Each one is done when its checks pass. "Port" means adapt the tested code in
`reference/`, keeping its tests.

## T0 – Confirm stack
Default: Rust, tokio + axum (server) + reqwest with `stream` (client) + serde/serde_json +
toml + clap + tracing. Koharu's own pins: axum 0.8, reqwest 0.13, tokio 1.x.
- [x] Choice recorded in README.md "Decisions" (change it there if you pick another language).

## T1 – Scaffold
- [x] `cargo new` binary in this folder. `cargo build`, `cargo fmt --check`, `cargo clippy` clean.
- [x] clap subcommands: `serve` (default) and `hosts <model>`; `--config <path>`.

## T2 – Config (F4)
- [x] `Config` struct matching config.example.toml. `Routing` enum serialized lowercase
      (port the enum from routing.rs, dropping `specta::Type`).
- [x] Load, validate, hot-reload (watch the file, or re-read each request); a bad edit keeps the last good config.
- Tests: example file parses; `routing = "fastest"` is rejected; missing `[pins]` → empty.

## T3 – Provider object (F3)
- [x] Port `provider()` and its 3 tests verbatim, plus `require_parameters` as a parameter.
- [x] `inject(body: &mut Value, cfg)`: picks the pin by `body.model` and merges per F3
      (proxy keys override, client keys kept).
- Tests: pinned / sorted / balanced; existing client `provider` keys kept; body without
  `model` passed through unchanged; non-JSON body → 400.

## T4 – Forward, non-streaming (F1, F5, F6, N3–N5)
- [x] `POST /v1/chat/completions` → inject → upstream. Return status, headers (minus
      hop-by-hop) and body unchanged.
- [x] `GET /v1/models` passthrough.
- [x] Auth per F5.
- Test: integration test with a mock upstream (e.g. a second axum server on a random port)
  asserting the upstream received the injected `provider`, and that 429 + `Retry-After`
  reach the client unchanged.

## T5 – Streaming (F2)
- [x] When `stream: true`, pipe the upstream body as a byte stream (`reqwest::Response::bytes_stream` → axum `Body::from_stream`).
- Test: the mock upstream sends 3 SSE chunks with delays; the client receives the first
  chunk before the last one is sent.

## T6 – Pin error explanation (F7)
- [x] Port `explained()` and its 2 tests. It now works on the upstream response (status 404 +
      body contains `No endpoints found` + request was pinned) instead of an `anyhow::Error`.
- [x] Rewrite only `error.message`; keep the JSON shape `{"error":{"message","code"}}`.
- Fixture: reference/fixtures/no-endpoints-404.json.

## T7 – Host listing CLI (F8)
- [x] Port `hosts()` (URL building, id validation) and `hosts_in()` (dedupe by tag,
      `unknown` quantization → none), **without** the `structured_outputs` filter; record
      `tools` / `structured_outputs` as flags.
- [x] Label with the logic in reference/host-label.ts; sort by label.
- Tests: adapt `the_hosts_are_read_from_an_endpoints_list` with
  reference/fixtures/endpoints-sample.json (now 4 unique tagged hosts, with flags).

## T8 – Health + logging (F9, N1, N2)
- [x] `/health`; tracing line per request; a test that a key never appears in log output.

## T9 – Docs
- [x] README "Usage": install, run, Koharu setup (KOHARU-CONTRACT.md §5), finding and
      pinning a tag with `hosts`, and the caveats (agent + pins, context window).
      (Also: build notes per OS, the web UI, its safety model and the API.)

## T10 – Manual end-to-end with Koharu (needs the user; costs a few cents)
- [ ] Koharu → OpenAI-compatible → `http://127.0.0.1:<port>/v1`; the model list loads.
- [ ] Translate one page per routing value; the proxy log shows the expected `provider`.
- [ ] Pin a working tag → translation ok. Pin a tag that can't serve the request → Koharu
      shows the F7 message.
- [ ] Edit the pin while the proxy runs → the next request uses it with no restart.
- [ ] The in-app agent works with Auto. With a pin: see KOHARU-CONTRACT.md §4.

## T11 – Web UI (F10, N6, N8–N12)
Done in this order: the config writer, the API, the guard, the page, the CLI, CI, docs.
- [x] `config::save_preserving` writes structured changes into the existing TOML in place: comments,
      blank lines and key order survive; the result is parsed and validated before the atomic write;
      a file it can't update safely is refused, not rewritten.
- [x] `GET /ui` serves the page (and `GET /` redirects to it): HTML, CSS and JS embedded in the
      binary, no CDN or external fonts, light and dark mode, no API-key field.
- [x] Status (path, config error, listen, upstream, key set yes/no, pins, Koharu setup block with
      base URL and copy button, context-window reminder), routing, pins (add / edit / remove),
      Find hosts with "Pin this" and the §4 caveat, Advanced with warnings and confirmation,
      Raw TOML editor with inline errors, recent requests (last 100, polled), unsaved-changes
      indicator, save feedback.
- [x] API: `GET /api/status`, `GET|PUT /api/config`, `GET|PUT /api/config/raw`, `GET /api/models`,
      `GET /api/hosts`, `GET /api/requests`. Invalid input is 400 `{"error":{"message"}}` and the
      file is untouched.
- [x] N8: a `Host` other than localhost / 127.0.0.1 / [::1] / the listen address is refused for
      `/ui` and `/api/*`; `/v1/*` and `/health` are not affected.
- [x] N9: changing requests need `Content-Type: application/json` and `X-Proxy-UI: 1`; a foreign
      `Origin` is refused; no CORS headers.
- [x] N10: CSP (`default-src 'self'`, no inline script or style), nosniff, frame-ancestors none /
      X-Frame-Options DENY, Referrer-Policy no-referrer on UI responses.
- [x] N11/N12: no key in any answer or log; a non-OpenRouter upstream or non-loopback listen needs
      `confirm_risky`.
- [x] CLI: `serve --open` opens the UI; the UI URL is logged at startup; a bare launch from a
      terminal or double-click opens it too.
- [x] CI: `.github/workflows/ci.yml` runs fmt, clippy -D warnings and the tests on ubuntu, macos and
      windows. TLS uses rustls + ring, so no CMake or NASM is needed on Windows.
- [x] Tests (offline, `tests/ui.rs` and unit tests): comment-preserving config round trip; invalid
      routing rejected with the file unchanged; foreign Host refused; cross-origin writes, a missing
      header and a wrong content type refused; `/api/hosts` against the mock upstream with
      `reference/fixtures/endpoints-sample.json`; `/ui` with its CSP; the request ring records a
      proxied request without any key; no API answer contains the key.
- [ ] Not tested here: Windows (CI will), a real browser session by a person (the page was driven in
      headless Chrome against a mock upstream), and live OpenRouter (T10).

## T12 – Releases (N6)
Goal: a person with no Rust installed can get a working `harugate` on macOS, Windows or Linux.
- [x] `[profile.release]`: LTO, one codegen unit, stripped (about 5.4 MB on macOS arm64).
- [x] `.github/workflows/release.yml`: a `v*` tag builds and publishes a GitHub Release; a manual run
      is a dry run (workflow artifacts only, no release). The tag must equal the `Cargo.toml` version.
- [x] Targets: Linux x64 and arm64 (static musl, cross-built with zig), macOS arm64, x64 and a `lipo`
      universal binary (ad-hoc signed), Windows x64 and arm64 (MSVC, C runtime linked in).
- [x] Archives `harugate-<version>-<platform>.tar.gz` / `.zip` (platform: `macos-universal`, `macos-arm64`,
      `macos-x64`, `linux-x64`, `linux-arm64`, `windows-x64`, `windows-arm64`), each with the binary,
      README, both licenses and `config.example.toml`; plus `SHA256SUMS.txt`.
- [x] `install.sh` (macOS/Linux) and `install.ps1` (Windows, PowerShell 5.1+): pick the archive, verify
      SHA-256, install per user, clear the macOS quarantine flag, print next steps. The release workflow runs
      both against the freshly built archives (and checks that `install.sh` refuses a tampered one).
- [x] `harugate --version`. A failed start (port in use, bad config) prints a plain message; on Windows a
      double-clicked window waits for Enter; a busy port says the UI is probably already running and, when the
      UI would have opened anyway, opens it (`src/startup.rs`, 3 tests).
- [x] README: Install section (downloads, one-liners, first run, Gatekeeper, SmartScreen, from source) and
      "Releasing" for maintainers.
- [ ] Not tested here: a first double-click on a real Windows desktop, and a published release (the first
      `v*` tag does that).
