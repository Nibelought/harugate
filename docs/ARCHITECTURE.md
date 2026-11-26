# Architecture

How harugate is built, for people and agents changing it. Behaviour is specified in
[REQUIREMENTS.md](REQUIREMENTS.md) (ids F1-F10, N1-N12); reasons for the big choices are in
[DECISIONS.md](DECISIONS.md); build, test and release steps are in [DEVELOPMENT.md](DEVELOPMENT.md).

harugate is a library crate plus a thin binary: `src/main.rs` parses the command line and calls into
`src/lib.rs`, so tests can build the same `axum::Router` the binary serves. It is async Rust (tokio, axum 0.8,
reqwest 0.13 with rustls and the `ring` provider) and keeps no state on disk except the config file.

## Repository layout

| Path | What it is |
|---|---|
| `src/` | The crate (module map below) |
| `ui/` | The web page: `index.html`, `app.css`, `app.js`, `favicon.svg`. Plain files, embedded in the binary with `include_str!` (`src/ui.rs`), so changing them needs a rebuild. |
| `tests/` | Integration tests: `proxy.rs`, `ui.rs`, `logging.rs`, helpers in `common/mod.rs` |
| `reference/` | Frozen code and fixtures from Koharu PR #1534; see [reference/README.md](../reference/README.md). `reference/fixtures/` is read by tests at compile time. |
| `config.example.toml` | Commented sample config (parsed by a unit test; shipped in release archives) |
| `docs/` | This documentation |
| `CHANGELOG.md`, `README.md`, `CLAUDE.md` | Release history, user guide, rules for agent sessions |
| `.github/workflows/` | `ci.yml` (fmt, clippy, tests), `release.yml` (build, test installers, publish) |
| `.github/scripts/` | `package.sh` (one release archive), `release-notes.sh` (top of the release notes) |
| `install.sh`, `install.ps1` | One-line installers for macOS/Linux and Windows |

## Module map

| File | Role |
|---|---|
| `src/main.rs` | clap definitions (`serve [--open]`, `hosts <model>`, global `--config`), decides whether a bare launch opens the browser, runs `hosts` |
| `src/lib.rs` | Module list and `init_logging()` (`RUST_LOG`, default `info`, to stderr) |
| `src/server.rs` | `app(state)` builds the Router (`/health`, `/v1/{*rest}`, plus the UI routes); `run()` loads the config, binds, logs, optionally opens the browser, serves until Ctrl-C/SIGTERM; `resolve_config_path()`; `local_url()` |
| `src/proxy.rs` | The `/v1/*` handler: inject, forward, relay, F7 rewrite, hop-by-hop and auth header handling |
| `src/routing.rs` | `Routing` enum, `provider()`, `inject()` / `inject_bytes()`, `Applied` (what the log line says), `explained()` (F7), `error_body()`. Ported from Koharu; see [reference/README.md](../reference/README.md). |
| `src/hosts.rs` | The hosts OpenRouter lists for a model: URL building and id validation, `hosts_in()` parsing, `Host::label()`, sorting, `render()` for the CLI. Ported from Koharu. |
| `src/config.rs` | `Config`, defaults, `parse()` / `validate()`, risk checks (`is_official_upstream`, `new_risks`), default path, atomic writes, `save_preserving()` (comment-keeping TOML edit), `ConfigStore` (hot reload and saves) |
| `src/state.rs` | `AppState` (config store, HTTP client, optional API key, request log, models cache, bound address), `build_client()`, the ring crypto provider, `API_KEY_ENV` |
| `src/activity.rs` | `RequestLog`: the in-memory ring of recent requests |
| `src/api.rs` | The JSON handlers under `/api`, `ApiError`, the models cache |
| `src/ui.rs` | Serves the embedded page and assets, registers the `/api` routes, and holds `guard` (Host, Origin, CSRF and response-header checks) |
| `src/startup.rs` | Plain-language report when `serve` can't start (port in use, bad config); on a double-clicked Windows console it waits for Enter |
| `ui/app.js` | The page's logic: no dependencies, all API text written with `textContent`, never `innerHTML` |

The upstream HTTP client (`state::build_client()`) has a 15 s connect timeout and no total timeout (reasoning
models can be silent for minutes), follows no redirects, and is built with reqwest's `system-proxy` feature.
No `Accept-Encoding` is forwarded, so responses arrive uncompressed. Model-list and host-list lookups (UI
and `hosts`) have a 30 s timeout.

## Request flow: POST /v1/chat/completions

`proxy::v1` handles every `/v1/*` path (any method). The route has no body size limit (N4).

1. **Config.** `state.config.current().await` re-reads the config file (see below) and returns the config for this
   request. The upstream URL is `{upstream}/{rest}?{query}`.
2. **Is it chat?** It is when the method is POST and the path after `/v1/` is `chat/completions` (a trailing slash is
   ignored). Any other request skips to step 4 with its body streamed through unchanged.
3. **Inject.** The body is read whole and passed to `routing::inject_bytes`:
   - Not valid JSON: 400 in OpenRouter's error shape (`{"error":{"message","code"}}`); logged and recorded.
   - JSON without a string `model` (or not an object): sent upstream byte for byte, routing applied is `none`.
   - Otherwise a `provider` object is built by `routing::provider()` from the model's pin, the global `routing` and
     `require_parameters`, and merged into any `provider` object the client sent (harugate's keys win, the client's
     other keys stay; a `provider` that isn't an object is replaced). The body is re-serialized with
     `serde_json` `preserve_order`, so field order and values are kept. `Applied` is `Pin(tag)`, `Sort(rule)` or
     `Balanced`.
4. **Headers.** `upstream_headers()` copies the client's headers minus hop-by-hop ones, `Host`, `Content-Length`,
   `Accept-Encoding` and `Authorization`. `Authorization` is `Bearer $OPENROUTER_API_KEY` if that is set, else the
   client's own (none if neither). Credentials are marked sensitive.
5. **Forward.** One request with the shared client. If the upstream can't be reached: 502 in OpenRouter's error
   shape (logged and recorded). No retries: Koharu retries 429 and 5xx itself
   ([KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) §2).
6. **Log and record** at the moment the response headers arrive: one log line and one entry in the request ring.
7. **F7 rewrite.** Only if the request was pinned and the status is 404: the body is read whole and, if it contains
   `No endpoints found`, only `error.message` is replaced by the explanation (pin, model, config path, original
   text). The `Content-Length` header is dropped and `Content-Type` set to JSON. Any other 404 body passes unchanged.
8. **Relay.** Every other response goes back with the upstream's status and headers (minus hop-by-hop) and a
   streamed body, chunk by chunk, so SSE is not buffered. (A pinned 404 that step 7 did not rewrite is returned from
   the buffered bytes.)

`GET /health` answers from the loaded config. Any path outside `/v1`, `/health` and the UI routes is a 404 and is never
forwarded.

## Config store and hot reload

`config::ConfigStore` owns the file.

- **Format.** `Config` has `listen`, `upstream`, optional `routing`, `require_parameters` and `pins`; unknown keys are
  rejected (`deny_unknown_fields`). `validate()` checks that `upstream` is an http(s) URL and that no pin has an
  empty model id or tag.
- **Reload.** There is no file watcher and no mtime check. `current()` reads the (tiny) file on every request and
  compares its text with the text it last saw; only if it differs is it parsed. A valid file replaces the config. An
  invalid or unreadable file keeps the last good config, logs the error once per distinct content, and exposes it
  through `last_error()` (shown by `/health`, `/api/status` and the page). A changed `listen` is only warned about:
  the listener is bound once at startup.
- **Saves.** Writes go to `<file>.tmp` and are renamed over the file, so a reader never sees half a file; the store
  then updates its own copy at once.
  - `save_preserving()` (structured form, `PUT /api/config`): edits the existing text with `toml_edit`. Comments,
    blank lines, key order and formatting stay; only changed values are touched; a `routing` of none removes the key;
    default-valued keys are not added to a file that lacks them. The result is parsed again and must equal the
    wanted config, otherwise nothing is written.
  - `save_text()` (raw editor, `PUT /api/config/raw`): validates and writes the text as given.
  - `save()` writes `toml::to_string_pretty` output (comments are lost); production code does not use it.
- **Default file.** `ensure_default_file()` creates `DEFAULT_CONFIG_TOML` only at the default path
  (`dirs::config_dir()/harugate/config.toml`). An explicit `--config` path must exist.

## Web UI and JSON API

The page is served by the same process on the same port. `ui::router` registers it; `guard` wraps every route in it.

| Route | Answer |
|---|---|
| `GET /` | Redirect (307) to `/ui` |
| `GET /ui`, `/ui/`, `/ui/app.css`, `/ui/app.js`, `/ui/favicon.svg` | The embedded files (`Cache-Control: no-cache`) |
| `GET /api/status` | Version, config path and error, `listen` (configured and running), `restart_required`, `upstream`, `upstream_is_official`, `listen_is_loopback`, `api_key_set` (a boolean, never the key), routing, `require_parameters`, pin count, `base_url` and `ui_url` |
| `GET /api/config` | `{config, config_error}` |
| `PUT /api/config` | Body: the whole config (`listen`, `upstream`, `routing` null for Balanced, `require_parameters`, `pins`) plus optional `confirm_risky`. Saves with `save_preserving`. Answers `{saved, config, restart_required}`. |
| `GET /api/config/raw` | `{path, text, config_error}` |
| `PUT /api/config/raw` | Body `{"text": "...", "confirm_risky"?: bool}`. Validates and saves the text as written. |
| `GET /api/models` | `{models, fetched_ms}`: ids from `{upstream}/models`, sorted, cached 5 minutes per upstream. 502 if the upstream fails. |
| `GET /api/hosts?model=author/slug` | `{model, pinned, hosts}`; each host has `tag`, `name`, `label`, `quantization`, `tools`, `structured_outputs`; sorted by label. 400 for a bad id, 502 if the upstream fails. |
| `GET /api/requests` | `{capacity, requests}`, newest first |
| `/api/*` (anything else) | 404 `{"error":{"message":"no such API route"}}` |

Errors are `{"error":{"message"}}`, with a string `code` added when a client can act on it (`confirmation_required`).
Invalid input is 400 and the file is untouched. The page polls `/api/status` every 10 s, and `/api/requests` every
2 s while the Requests tab is open.

### Security model

The page can change `upstream` (where the API key is sent) and `listen`, so `ui::guard` treats every request to
`/`, `/ui*` and `/api/*` as hostile until it passes the checks below. `/v1/*` and `/health` are never subject to them, so Koharu (which is not a
browser) is unaffected.

| Check | Rule | Refusal |
|---|---|---|
| Host (N8, DNS rebinding) | `Host` must be `localhost`, `127.0.0.1` or `[::1]` (any port), or the configured/bound listen address with its port (an unspecified address like `0.0.0.0` adds nothing) | 403 |
| `Sec-Fetch-Site` (N9) | For `/api`, if sent, must be `same-origin` or `none` | 403 |
| `Origin` (N9) | For `/api`, if sent, must be this server's own origin (compared with `Host`) | 403 |
| `X-Proxy-UI: 1` (N9) | For `/api` requests other than GET/HEAD | 403 |
| `Content-Type: application/json` (N9) | For `/api` requests other than GET/HEAD | 415 |
| Body size | `/api` bodies are limited to 1 MiB | 413 |
| Consent (N12) | `PUT /api/config` and `PUT /api/config/raw` that introduce a new non-OpenRouter `upstream` or non-loopback `listen` need `confirm_risky: true` (a value already in the file is not new) | 400, `code: confirmation_required` |

Responses from every guarded route carry (N10): a `Content-Security-Policy` of `default-src 'self'` with `script-src` and
`style-src 'self'` (no inline script or style), `frame-ancestors 'none'`, `base-uri 'none'`, `object-src 'none'`; `X-Content-Type-Options:
nosniff`; `X-Frame-Options: DENY`; `Referrer-Policy: no-referrer`; `Cross-Origin-Opener-Policy` and
`Cross-Origin-Resource-Policy: same-origin`; and a `Permissions-Policy` denying camera, microphone and geolocation. No CORS headers are
ever sent. Keys never appear in API answers (N11): `api_key_set` is a boolean, and the key is only used by
`AppState::upstream_key()` for the model and host lookups.

## Recent requests

`activity::RequestLog` is a mutex-guarded ring of the last 100 requests, filled by `proxy::v1` for every `/v1/*`
request, including a 400 for a bad body and a 502 for an unreachable upstream. A record has an increasing id,
the time, method, path (no query), model, routing applied (`pin:<tag>`, `sort:<rule>`, `balanced`, `none`), status and
latency (time until the response headers arrive, so it is not the length of a stream). Text fields are cut at 200
characters. No keys, headers, bodies or queries are stored, and the list is lost when harugate stops.

## Logging

- `lib::init_logging()`: `tracing-subscriber` with `RUST_LOG` (default `info`), to stderr.
- One line per proxied request with fields `method`, `path`, `model`, `routing`, `status`, `latency_ms`; plus lines for
  config reloads and errors, startup (config path, listen address, UI URL) and refused UI requests.
- Never log a key, a header value or a request body. `AppState`'s `Debug` output hides the key, header values are
  marked sensitive, and `tests/logging.rs` captures a TRACE log of every crate and checks that no key and no
  `bearer` appears. If you add a log line or a `Debug` derive, run that test.
- The config path and pin tags may be logged; they are not secrets.

## Tests

All tests are offline (see [DEVELOPMENT.md](DEVELOPMENT.md#offline-tests-and-live-checks)).

| Where | What |
|---|---|
| `#[cfg(test)]` in `src/*.rs` | Unit tests next to the code: config parsing, validation and comment-preserving saves, hot reload, `provider()` and `inject()`, `explained()`, host parsing, labels and rendering, request ring, header filtering, Host/Origin helpers, startup messages |
| `tests/proxy.rs` | The `/v1` surface against a mock upstream: injection, errors and `Retry-After` passthrough, other paths, auth rules, streaming chunk by chunk, F7, live config edits, `/health`, large bodies, 502 |
| `tests/ui.rs` | The page and `/api`: CSP headers, Host/Origin/header refusals, comment-keeping saves, invalid saves leaving the file alone, risky settings, raw editor, models and hosts lookups, the ring, no key in any answer |
| `tests/logging.rs` | One test, in its own file because it installs the global log subscriber: no key in the log |
| `tests/common/mod.rs` | Helpers (below) |

Helpers in `tests/common/mod.rs`: `spawn_upstream()` starts a mock OpenRouter on a random port. It records every request and
answers `POST /api/v1/chat/completions` by `model`: `rate/limited` gives 429 with `Retry-After: 7`, `no/endpoints` gives the
404 fixture, a `stream: true` request gets three SSE chunks of which the 2nd waits for the `gate` (`Notify`) so tests can prove
the first chunk arrives early, and anything else gets a small chat reply. `GET /api/v1/models` and
`.../endpoints` return canned JSON; other paths echo the path. `spawn_proxy(config, api_key)` binds harugate on a random
loopback port in front of it with a temp config file; `config_text()` builds a config pointing at the mock.

Fixtures: `reference/fixtures/endpoints-sample.json` and `no-endpoints-404.json` are embedded with `include_str!` by
`src/hosts.rs`, `src/routing.rs`, `tests/proxy.rs` and `tests/common/mod.rs`. Moving them breaks the build.
`src/config.rs`'s tests embed `config.example.toml`.

## Where to change X

| To change | Look at | Also update |
|---|---|---|
| The `provider` object, merge rules | `routing::provider`, `routing::inject` | tests in `routing.rs` and `tests/proxy.rs`; F3; README table |
| Which requests are injected | `proxy::v1` (the `chat` test) | F1, F3 |
| Headers sent upstream or back, auth | `proxy::upstream_headers`, `response_headers` | F5, F6 |
| The pin-error message | `routing::explained` | F7; the test in `routing.rs` and `tests/proxy.rs` |
| Host listing, labels, CLI output | `hosts.rs` (`hosts_in`, `label`, `render`) | F8; `api::hosts` if the JSON changes |
| A config key | `config::Config`, `DEFAULT_CONFIG_TOML`, `save_preserving` (`set_scalar` / `set_pins`), `api::config_json` and `ConfigUpdate`, `ui/app.js`, `ui/index.html` | `config.example.toml`, F4, README configuration reference, CHANGELOG |
| Config file location or reload behaviour | `config::default_path`, `ConfigStore::current` | F4; README |
| A new `/api` route | handler in `api.rs`, route in `ui::router` (the guard applies automatically), a test in `tests/ui.rs` | table above, F10 |
| CSP or response headers, Host/Origin rules | `ui::CSP`, `add_security_headers`, `check` | N8-N10; `tests/ui.rs` |
| Risky-setting rules | `config::new_risks`, `is_official_upstream`, `api::require_confirmation` | N12; `tests/ui.rs` |
| The page's look or behaviour | `ui/index.html`, `ui/app.css`, `ui/app.js` (rebuild to embed) | F10 |
| What the request list holds | `activity.rs`, `api::requests`, `ui/app.js` | F10; keep keys, headers and bodies out |
| Log lines | `proxy::v1`, `init_logging` | N2; run `tests/logging.rs` |
| CLI flags, bare-launch behaviour | `main.rs` | README command table, F8, F10 |
| Start-up error messages | `startup.rs` | |
| Timeouts, TLS, the HTTP client | `state::build_client` | N3-N5; DECISIONS if TLS changes |
| Release archives or installers | `.github/workflows/release.yml`, `.github/scripts/`, `install.sh`, `install.ps1` | [DEVELOPMENT.md](DEVELOPMENT.md#releasing) |
