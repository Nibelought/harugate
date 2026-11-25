# Requirements

IDs are referenced from TASKS.md.

## Functional

**F1 – OpenAI-compatible surface.** Serve, under a `/v1` prefix:
- `POST /v1/chat/completions`: forward to `{upstream}/chat/completions` after F3.
- `GET  /v1/models`: forward unchanged to `{upstream}/models`. Koharu builds its model
  list from this, so the ids are OpenRouter's `author/slug`.
- Any other path: forward unchanged (transparent passthrough), or 404. Pick one and document it.

**F2 – Streaming.** When the request has `"stream": true`, relay the upstream SSE response
chunk by chunk as it arrives, with no buffering, keeping `Content-Type: text/event-stream`.
Koharu's agent streams, and translation may too.

**F3 – Provider injection.** For `POST /chat/completions`, merge a `provider` object into
the JSON body, by the body's `model`:

| State for this model | `provider` sent |
|---|---|
| Pinned to tag `T` | `{"require_parameters": true, "only": ["T"], "allow_fallbacks": false}` |
| Not pinned, routing `price` / `throughput` / `latency` | `{"require_parameters": true, "sort": "<routing>"}` |
| Not pinned, no routing (Balanced) | `{"require_parameters": true}` |

This is exactly `provider()` in reference/routing.rs. When a pin is set, it ignores the sort.
- If the incoming body already has a `provider` object, keys the proxy sets override it
  and all other keys are kept. (Koharu sends none today. This is for other clients.)
- `require_parameters` is configurable (default `true`, matching Koharu's own OpenRouter vendor).
- Every other body field is passed through untouched, in its original form.

**F4 – Config.** A TOML file (see config.example.toml) holding: listen address, upstream
base URL, `routing` (optional), `require_parameters`, and `[pins]` (model id → endpoint tag).
- Path given by `--config`, with a sensible default.
- Re-read when the file changes (or on every request, if that's simpler and cheap), so you
  can change a pin without restarting. An invalid edit keeps the last good config and logs
  the error.
- Unknown `routing` values are rejected at load time with a clear message.

**F5 – Auth.**
- Default: pass Koharu's `Authorization: Bearer <key>` through. In Koharu you put the
  OpenRouter key in the OpenAI-compatible provider.
- Optional: `OPENROUTER_API_KEY` env var. When set, it replaces whatever the client sent,
  so Koharu's key field can stay empty.

**F6 – Errors pass through faithfully.** Upstream status code, body and headers
(`Retry-After` especially) reach Koharu unchanged. Koharu's HTTP client retries 429 and
5xx itself, using those headers (koharu-llm/src/hosted/http.rs).
- Exception (F7): the 404 a pin causes.

**F7 – Pin error explanation.** When the request was pinned and upstream answers 404 with a
body containing `No endpoints found`, return the 404 with OpenRouter's JSON error shape and the
message made clearer, e.g.:
`OpenRouter provider \`T\`, pinned for model \`M\` in <config path>, can't serve this request (OpenRouter: <original message>)`.
- Port of `explained()` in reference/routing.rs. Other errors stay untouched.

**F8 – Host listing (CLI).** `harugate hosts <author/slug>` prints the endpoints
OpenRouter lists for a model (`GET {upstream}/models/{author}/{slug}/endpoints`), one per
line: tag, label (reference/host-label.ts logic), quantization, and whether it lists
`tools` / `structured_outputs`. This is how you find a tag to pin.
- Port of `hosts()` / `hosts_in()`, **but do not drop endpoints without
  `structured_outputs`**. Show the flags instead. Through OpenAI-compatible, Koharu does
  not send a JSON schema (see KOHARU-CONTRACT.md §3), so that filter no longer applies.
- Validates `author/slug` as `hosts()` does.

**F9 – Health.** `GET /health` → 200 with the loaded config's routing, pin count and config path.

**F10 – Web UI.** The same binary serves a local web interface, so nobody has to use the
command line or edit TOML by hand. It is plain HTML, CSS and JS embedded in the binary (no
build step, no CDN, no external fonts or scripts: it works offline and sends nothing to third
parties), with light and dark mode.
- `GET /ui` is the page; `GET /` redirects to it. `serve --open` (and a bare launch from a
  terminal or a double-click) opens it in the default browser. The URL is logged at startup.
- **Status:** config path, the last config error, `listen`, `upstream`, whether
  `OPENROUTER_API_KEY` is set (yes/no, never the key), the pin count, and the Koharu setup block
  of KOHARU-CONTRACT.md §5 with the computed base URL, a copy button and the context-window reminder.
- **Routing:** Balanced / price / throughput / latency, and `require_parameters`.
- **Pins:** a table of model id → tag (add, edit, remove). The model id field completes from the
  upstream's model list (fetched by the server, cached a few minutes, ids only). "Find hosts" lists
  a model's hosts as F8 does (label, tag, quantization, `tools` / `structured_outputs`, sorted by
  label), each with "Pin this". Hosts without `tools` carry the KOHARU-CONTRACT.md §4 caveat.
- **Advanced:** `upstream` and `listen` (`listen` takes effect after a restart). A strong warning
  and an explicit confirmation, enforced by the server too, when `upstream` is not
  `https://openrouter.ai/...` (it receives the API key) or `listen` is not loopback (N1).
- **Raw file:** a TOML editor; the server validates and shows the error inline, and writes nothing
  on error.
- **Recent requests:** the last 100 in memory (model, routing applied, upstream status, latency,
  time; no keys, headers or bodies), refreshed by polling.
- Unsaved changes are shown; saving applies at once (F4 hot reload) with success or error feedback.
- Saving from the structured form edits the file in place: the user's comments and key order stay.
- JSON API under `/api/`: `GET /api/status`, `GET|PUT /api/config`, `GET|PUT /api/config/raw`,
  `GET /api/models`, `GET /api/hosts?model=author/slug`, `GET /api/requests`. Invalid input is a
  400 `{"error":{"message"}}` and the file is untouched.

## Non-functional

**N1** Binds to `127.0.0.1` by default. Binding elsewhere needs an explicit config value.

**N2** Never logs keys. Logs one line per request: model, routing applied (pin tag / sort /
balanced), upstream status, latency. Log level comes from `RUST_LOG`.

**N3** Adds little latency: stream bodies, and don't re-serialize responses (except the F7 rewrite).

**N4** No request size limit below upstream's. Koharu sends page images when "model sees
each page" is on.

**N5** Long timeouts: reasoning models can be silent for minutes. No total request timeout
by default, only a connect timeout.

**N6** One self-contained binary (the web UI is embedded). Runs on macOS (primary), Linux and
Windows: no unix-only code outside `cfg(unix)`, paths through `std::path` / `dirs`, and TLS
that builds without OpenSSL, CMake or NASM (rustls with `ring`). CI runs fmt, clippy and the
tests on all three.

**N7** Unit tests cover F3, F4 parsing, F7 and F8 parsing, offline, with fixtures. At least one
integration test runs the proxy against a local mock upstream (non-stream and stream).

**N8** *DNS rebinding.* For `/ui` and `/api/*`, a request whose `Host` header is not
`localhost`, `127.0.0.1`, `[::1]` (any port) or the configured listen address is refused (403). The
check does not apply to `/v1/*` or `/health`, so Koharu is unaffected.

**N9** *CSRF.* A `/api` request that changes anything needs `Content-Type: application/json`
and the custom header `X-Proxy-UI: 1`; if the browser sent `Origin` it must be this server's own,
and `Sec-Fetch-Site`, if sent, must be `same-origin`. No CORS headers are ever emitted, so no other
web page can call the API. (A page that can't be trusted could otherwise change where the API key goes.)

**N10** *Browser hardening.* UI responses carry `Content-Security-Policy` (`default-src 'self'`,
no inline script or style, `frame-ancestors 'none'`), `X-Content-Type-Options: nosniff`,
`X-Frame-Options: DENY` and `Referrer-Policy: no-referrer`. The JS and CSS are separate files.

**N11** *Keys stay out of the UI.* No API response, log line or page shows an API key or an
Authorization header, and the UI has no field for one (the key comes from the client or
`OPENROUTER_API_KEY`). The status shows only whether the variable is set.

**N12** *Risky settings need consent.* Saving a new `upstream` that is not `https://openrouter.ai/...`
or a new non-loopback `listen` needs an explicit confirmation (the API answers 400
`confirmation_required` until `confirm_risky` is true).

## Out of scope

- Per-request routing chosen from the request's content.
- A desktop (native) GUI, or a UI reachable from other machines: the web UI is local-only (N8–N12).
- Anything that changes Koharu.
- Caching, rate limiting, multi-user use.
