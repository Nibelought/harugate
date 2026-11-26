# Requirements

The living specification of what harugate does. Ids (F1-F10, N1-N12) are referenced from code comments, tests and
other docs: keep them stable (add new ids at the end of their group; never renumber or reuse). Where the code lives
is in [ARCHITECTURE.md](ARCHITECTURE.md); background on Koharu and OpenRouter is in
[KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) and [OPENROUTER-NOTES.md](OPENROUTER-NOTES.md).

## Functional

**F1 - OpenAI-compatible surface.** harugate serves, under a `/v1` prefix:
- `POST /v1/chat/completions`: forwarded to `{upstream}/chat/completions` after F3.
- `GET /v1/models`: forwarded unchanged to `{upstream}/models`. Koharu builds its model list from this, so the ids are
  OpenRouter's `author/slug`.
- Any other method and path under `/v1/`: transparent passthrough to the same path under the upstream (body streamed,
  query kept, same auth rules).
- Outside `/v1` only `/health` (F9) and the web UI routes (F10) exist. Anything else is a 404 and is never forwarded.

**F2 - Streaming.** When the request has `"stream": true`, the upstream SSE response is relayed chunk by chunk as it
arrives, with no buffering, keeping `Content-Type: text/event-stream`. Koharu's agent streams, and translation may too.

**F3 - Provider injection.** For `POST /v1/chat/completions`, a `provider` object is merged into the JSON body, chosen by
the body's `model`:

| State for this model | `provider` sent |
|---|---|
| Pinned to tag `T` | `{"require_parameters": true, "only": ["T"], "allow_fallbacks": false}` |
| Not pinned, routing `price` / `throughput` / `latency` | `{"require_parameters": true, "sort": "<routing>"}` |
| Not pinned, no routing (Balanced) | `{"require_parameters": true}` |

This is `routing::provider()`. A pin ignores the sort.
- `require_parameters` is configurable (default `true`, matching Koharu's own OpenRouter vendor); the table shows the
  default.
- If the incoming body already has a `provider` object, the keys harugate sets override it and all other keys are kept
  (Koharu sends none; this is for other clients). A `provider` that is not an object is replaced.
- Every other body field keeps its value and position (the body is re-serialized with field order preserved).
- A body with no string `model` is sent upstream byte for byte, with nothing injected.
- A body that is not valid JSON is answered 400 in OpenRouter's error shape `{"error":{"message","code"}}` and not
  forwarded.

**F4 - Config.** A TOML file (see `config.example.toml`) holding: `listen` address, `upstream` base URL, `routing`
(optional), `require_parameters`, and `[pins]` (model id → endpoint tag). Every key is optional and has a default.
- Path given by `--config` (which must exist), else `<config dir>/harugate/config.toml`, created with defaults on the
  first `serve`.
- Re-read on every request and applied when its text has changed, so a pin changes without a restart. An invalid
  edit keeps the last good config, is logged once, and is reported by `/health` (`config_error`) and the web UI.
  `listen` is read at startup only; changing it logs a warning and needs a restart.
- Invalid values are rejected with a clear message at load time: unknown keys, an unknown `routing` value (anything
  but `price`, `throughput`, `latency`), an `upstream` that is not an http(s) URL, an empty pin model id or tag. A file
  that is invalid at startup stops harugate with a plain message.

**F5 - Auth.**
- Default: Koharu's `Authorization: Bearer <key>` is passed through. In Koharu the OpenRouter key goes in the
  OpenAI-compatible provider. If the client sends none, none is sent.
- Optional: the `OPENROUTER_API_KEY` environment variable. When set (and not empty), it replaces whatever the client
  sent, so Koharu's key field can stay empty. A value that can't be sent in an HTTP header stops harugate at start.

**F6 - Errors pass through faithfully.** Upstream status code, body and headers (`Retry-After` especially) reach Koharu
unchanged, apart from hop-by-hop headers. Koharu's HTTP client retries 429 and 5xx itself, using those headers
([KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) §2), so harugate does not retry.
- Exception (F7): the 404 a pin causes.
- If the upstream can't be reached, harugate answers 502 in OpenRouter's error shape.

**F7 - Pin error explanation.** When the request was pinned and the upstream answers 404 with a body containing
`No endpoints found`, harugate returns the 404 with OpenRouter's JSON error shape and only `error.message` replaced:
``OpenRouter provider `T`, pinned for model `M` in <config path>, can't serve this request (OpenRouter: <original message>)``
(`routing::explained()`). Other errors, and 404s of unpinned requests, are untouched.

**F8 - Host listing (CLI).** `harugate hosts <author/slug>` prints the endpoints OpenRouter lists for a model
(`GET {upstream}/models/{author}/{slug}/endpoints`), one per line: tag, label, quantization (`-` if none), and
`tools=yes|no`, `structured_outputs=yes|no`. This is how you find a tag to pin.
- Hosts are listed once per tag, sorted by label (`Name (quantization, region/tier)`, see `Host::label()`); a row without
  a tag is skipped; quantization `unknown` means none.
- Hosts without `structured_outputs` are **not** dropped; the flags are shown instead. Through OpenAI-compatible, Koharu
  does not send a JSON schema ([KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) §3), so that filter does not apply.
- The id must be `author/slug` (one slash, both parts non-empty). The upstream comes from the config file if there is
  one, else the default; `OPENROUTER_API_KEY`, if set, is sent (the list needs no key). No endpoints prints a message
  to stderr.

**F9 - Health.** `GET /health` answers 200 with `status`, `version`, `routing` (`balanced` if unset), `pins` (count),
`require_parameters`, `config_path` and `config_error` (null when fine).

**F10 - Web UI.** The same binary serves a local web interface, so nobody has to use the command line or edit TOML by
hand. It is plain HTML, CSS and JS embedded in the binary (no build step, no CDN, no external fonts or scripts: it
works offline and sends nothing to third parties), with light and dark mode.
- `GET /ui` is the page; `GET /` redirects to it. `serve --open` (and a bare launch from a terminal or a double-click)
  opens it in the default browser. The URL is logged at startup.
- **Status:** config path, the last config error, `listen`, `upstream`, whether `OPENROUTER_API_KEY` is set (yes/no,
  never the key), the pin count, and the Koharu setup block of [KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) §5 with the
  computed base URL, a copy button and the context-window reminder.
- **Routing & pins:** Balanced / price / throughput / latency, and `require_parameters`. A table of model id → tag (add,
  edit, remove). The model id field completes from the upstream's model list (fetched by the server, cached 5 minutes,
  ids only). "Find hosts" lists a model's hosts as F8 does (label, tag, quantization, `tools` / `structured_outputs`,
  sorted by label), each with "Pin this". Hosts without `tools` carry the [KOHARU-CONTRACT.md](KOHARU-CONTRACT.md) §4
  caveat.
- **Advanced:** `upstream` and `listen` (`listen` takes effect after a restart). A strong warning and an explicit
  confirmation, enforced by the server too (N12), when `upstream` is not `https://openrouter.ai/...` (it receives the
  API key) or `listen` is not loopback (N1).
- **Raw file:** a TOML editor; the server validates and shows the error inline, and writes nothing on error.
- **Requests:** the last 100 `/v1` requests in memory (method, path, model, routing applied, upstream status, latency,
  time; no keys, headers or bodies), refreshed by polling.
- Unsaved changes are shown; saving applies at once (F4) with success or error feedback.
- Saving from the structured form edits the file in place: the user's comments and key order stay.
- JSON API under `/api/`: `GET /api/status`, `GET|PUT /api/config`, `GET|PUT /api/config/raw`, `GET /api/models`,
  `GET /api/hosts?model=author/slug`, `GET /api/requests`. Invalid input is a 400 `{"error":{"message"}}` and the file is
  untouched. Unknown `/api` paths are a JSON 404.

## Non-functional

**N1** harugate binds to `127.0.0.1` by default. Binding elsewhere needs an explicit config value, and logs a warning
at startup.

**N2** Never logs keys. Logs one line per proxied request: method, path, model, routing applied (`pin:<tag>` /
`sort:<rule>` / `balanced` / `none`), upstream status, latency. Log level comes from `RUST_LOG` (default `info`); logs go to
stderr.

**N3** Adds little latency: bodies are streamed, and responses are not re-serialized (except the F7 rewrite).

**N4** No request size limit of harugate's own on `/v1`. Koharu sends page images when "model sees each page" is on.

**N5** Long timeouts: reasoning models can be silent for minutes. No total request timeout, only a 15 s connect timeout.

**N6** One self-contained binary (the web UI is embedded). Runs on macOS, Linux and Windows: no unix-only code outside
`cfg(unix)`, paths through `std::path` / `dirs`, and TLS that builds without OpenSSL, CMake or NASM (rustls with `ring`).
CI runs fmt, clippy and the tests on all three. Releases publish macOS (universal, arm64, x64), Windows x64 and arm64,
and Linux x64 and arm64; the Linux binaries are static (musl), so they run on any distribution.

**N7** Offline tests cover F3, F4 parsing, F7 and F8 parsing with fixtures, and integration tests run harugate against a
local mock upstream (non-stream and stream). See [ARCHITECTURE.md](ARCHITECTURE.md#tests).

**N8** *DNS rebinding.* For `/`, `/ui*` and `/api/*`, a request whose `Host` header is not `localhost`, `127.0.0.1`, `[::1]` (any
port) or the configured or bound listen address with its port is refused (403). The check does not apply to `/v1/*` or
`/health`, so Koharu is unaffected.

**N9** *CSRF.* A `/api` request that changes anything (not GET/HEAD) needs `Content-Type: application/json` (else 415) and
the custom header `X-Proxy-UI: 1` (else 403). If the browser sent `Origin`, it must be this server's own, and
`Sec-Fetch-Site`, if sent, must be `same-origin` or `none` (else 403); these two apply to every `/api` request. No CORS headers
are ever emitted, so no other web page can call the API. (A page that can't be trusted could otherwise change where the
API key goes.)

**N10** *Browser hardening.* UI responses carry `Content-Security-Policy` (`default-src 'self'`, no inline script or style,
`frame-ancestors 'none'`), `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY` and `Referrer-Policy: no-referrer` (and
cross-origin opener/resource policies and a restrictive `Permissions-Policy`). The JS and CSS are separate files.

**N11** *Keys stay out of the UI.* No API response, log line or page shows an API key or an Authorization header, and the
UI has no field for one (the key comes from the client or `OPENROUTER_API_KEY`). The status shows only whether the
variable is set.

**N12** *Risky settings need consent.* Saving a new `upstream` that is not `https://openrouter.ai/...` or a new non-loopback
`listen` needs an explicit confirmation: the API answers 400 with `code: confirmation_required` until `confirm_risky` is
true. A value already in the file is not new, so saving other settings of a hand-edited file needs none.

## Out of scope

- Per-request routing chosen from the request's content.
- A desktop (native) GUI, or a UI reachable from other machines: the web UI is local-only (N8-N12).
- Anything that changes Koharu.
- Caching, rate limiting, multi-user use.
