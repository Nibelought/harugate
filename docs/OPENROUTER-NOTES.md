# OpenRouter notes

What harugate relies on in OpenRouter's API, and what was measured. Behaviour is specified in
[REQUIREMENTS.md](REQUIREMENTS.md).

## `provider` object (fields harugate uses)
- `sort`: `"price" | "throughput" | "latency"`. Omitting it gives OpenRouter's default balancing ("Balanced").
- `only`: list of endpoint tags or provider slugs that may serve the request.
- `allow_fallbacks`: `false` means fail rather than use another host.
- `require_parameters`: only hosts that support every parameter in the request.
- Also available, not used by harugate: `order`, `ignore`, `quantizations`, `data_collection`, `max_price`.
  Docs: https://openrouter.ai/docs/guides/routing/provider-selection

## Endpoints list
`GET https://openrouter.ai/api/v1/models/{author}/{slug}/endpoints` needs no key (it accepts one). The fields read from
`data.endpoints[]`: `tag`, `provider_name`, `quantization` (`"unknown"` means none), `supported_parameters[]`. Several rows can
share a tag. Tags look like `deepinfra/fp4`, `sail-research/fp4/us` (provider / quantization / region or tier).
Sample: `reference/fixtures/endpoints-sample.json`.

## Pin failure
A pinned request no host can serve answers **404**:
`{"error":{"message":"No endpoints found that can handle the requested parameters.","code":404}}`
(`reference/fixtures/no-endpoints-404.json`). harugate rewrites only the message (F7).

## Live results from PR #1534 (2026-11-25)
The requests were built like Koharu's **native** OpenRouter requests (strict `json_schema` + `require_parameters` + pinned
`only`), one per endpoint:

| Model | Endpoints | Valid translation | Always 404 |
|---|---|---|---|
| `deepseek/deepseek-v4.1-flash-1017` | 54 | 48 | 6, none listing `structured_outputs` |
| `z-ai/glm-5.3-flash` | 42 | 41 | 1, not listing `structured_outputs` |

- 429s were transient and succeeded on retry.
- Tier endpoints (flex, priority, fast) work when pinned.
- Every routing value returned valid output on both models.
- Through harugate (prompted mode, no `response_format`), those 404 endpoints may work. Not yet re-checked with Koharu
  ([ROADMAP.md](ROADMAP.md)).
- Model ids with a `~` alias returned an empty endpoints list: they can't be pinned.

## Live results through harugate (2026-11-25)
A scripted run against the real OpenRouter, through harugate (not through Koharu):

| Check | Result |
|---|---|
| `GET /v1/models` | 441 models |
| Balanced, `price`, `throughput`, `latency` | All answered 200, via different hosts (Cohere, Wafer, Nebius, InferenceNet) |
| Pin `deepinfra/fp8` | Served by DeepInfra |
| Pin `alibaba` plus a `json_schema` `response_format` | 404 with the F7 explanation |
| Pin edited by hand in the config file | Picked up on the next request, no restart (served by Together) |
| `stream: true` | `text/event-stream` ending with `[DONE]` |
| API key | Absent from the log, from `/api/requests` and from the config file |

## Alternatives that need no proxy
- Model suffixes `:nitro` (sort by throughput) and `:floor` (sort by price). Both may also pick priority or flex tier
  endpoints.
- Presets: `@preset/<slug>` or `<model>@preset/<slug>`, set up in the OpenRouter dashboard, with provider settings inside.
