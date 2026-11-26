# Roadmap

## History

The project was planned as tasks T0-T12 (a build plan with acceptance criteria per task). All are done except the
in-app part of T10, which is listed below.

- **T0-T9:** stack (Rust), scaffold, config and hot reload, provider injection, forwarding, streaming, pin
  error explanation, host listing CLI, health and logging, user docs.
- **T11:** the web UI and its JSON API, the comment-preserving config writer, the security checks
  (N8-N12), CI on Linux, macOS and Windows.
- **T12:** release builds for macOS, Windows and Linux, the release workflow, the install scripts,
  friendly start-up errors.
- **v0.1.0 released 2026-11-26.** See [CHANGELOG.md](../CHANGELOG.md). Why things are the way they are:
  [DECISIONS.md](DECISIONS.md).

## Open

Each item says what is left and where to start. Remove an item when it is done; add a line to
[CHANGELOG.md](../CHANGELOG.md) if users can see the result.

### 1. In-app checks with Koharu (rest of T10)

**Needs the user and costs a few cents** (see [DEVELOPMENT.md](DEVELOPMENT.md#offline-tests-and-live-checks)). Still to do,
with Koharu itself pointed at harugate (Settings → Providers → OpenAI-compatible, base URL
`http://127.0.0.1:8787/v1`):

- [ ] The model list loads in Koharu.
- [ ] Translate one page for each routing value (Balanced, price, throughput, latency); the Requests tab shows the
      expected routing.
- [ ] A pin that can't serve the request: Koharu shows the F7 message.
- [ ] The in-app agent works with Auto (no pins), and with a pin (see
      [KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) §4).

Already verified on 2026-11-25 by a scripted live run **through harugate, not through Koharu** (details in
[OPENROUTER-NOTES.md](OPENROUTER-NOTES.md#live-results-through-harugate-2026-11-25)): `/v1/models` returned 441 models;
Balanced, price, throughput and latency all answered 200, via different hosts; a pin to `deepinfra/fp8` was served by
DeepInfra; a pin to `alibaba` with a `json_schema` `response_format` gave a 404 with the F7 message; a pin edited by hand in
the file was picked up without a restart; streaming returned `text/event-stream` ending in `[DONE]`; the API key appeared
in no log, in `/api/requests` or in the config file.

### 2. Code signing and notarization

Remove the macOS Gatekeeper and Windows SmartScreen warnings with an Apple Developer ID (sign and notarize the
universal binary in `build-macos` of `.github/workflows/release.yml`) and a Windows Authenticode certificate (sign
`harugate.exe` for x64 and arm64). Both need paid certificates and secrets in the repository. Today the macOS binary is
only ad-hoc signed; the README explains the warnings and `install.sh` clears the quarantine flag.

### 3. Pins and the in-app agent

[KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) §4: a pin to a host without `tools` makes every agent request a 404. Options:
a per-pin `allow_fallbacks` setting, or pins that skip requests carrying `tools`. Either is a new requirement (F3,
F4) plus a change in `routing::inject` and the config format.

### 4. Log output has ANSI colour codes when it is not a terminal

`lib::init_logging()` lets `tracing-subscriber` colour its output even when stderr is a file or a pipe (logs go to stderr, not stdout). Disable ANSI when
stderr is not a terminal (`std::io::IsTerminal`). `tests/logging.rs` already sets `with_ansi(false)` for its own subscriber.

### 5. Node 20 deprecation in the release workflow

`mlugg/setup-zig@v2` still runs on Node 20. Watch for a new major version and bump it in `release.yml`.

### 6. A config file holding only comments

If the file has no keys at all, the first structured save from the page puts the comments below the new keys instead
of above them. Cosmetic: nothing is lost. The code is `config::save_preserving`.

### 7. Doc comment and code disagree on `Cache-Control: no-store`

The module comment of `src/ui.rs` says API responses get `no-store`, but `add_security_headers` does not set it (only the
static assets get `no-cache`). Either add `Cache-Control: no-store` to `/api` responses (and a test) or fix the comment.

### 8. Not run on real hardware or screens

- The web UI at 360 px width.
- The Windows double-click flow on a real desktop (the install script and the binary are tested on a Windows runner in
  CI).
- The Linux arm64 binary (it is cross-built and its `--version` is not run in CI).
