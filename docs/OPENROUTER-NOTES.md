# OpenRouter notes

## `provider` object (fields the proxy uses)
- `sort`: `"price" | "throughput" | "latency"`. Omitting it gives OpenRouter's default
  balancing ("Balanced").
- `only`: list of endpoint tags or provider slugs that may serve the request.
- `allow_fallbacks`: `false` means fail rather than use another host.
- `require_parameters`: only hosts that support every parameter in the request.
- Also available, not used yet: `order`, `ignore`, `quantizations`, `data_collection`, `max_price`.
  Docs: https://openrouter.ai/docs/guides/routing/provider-selection

## Endpoints list
`GET https://openrouter.ai/api/v1/models/{author}/{slug}/endpoints` needs no key (it accepts
one). The fields read from `data.endpoints[]`: `tag`, `provider_name`, `quantization`
(`"unknown"` means none), `supported_parameters[]`. Several rows can share a tag. Tags look like
`deepinfra/fp4`, `sail-research/fp4/us` (provider / quantization / region or tier).
Sample: reference/fixtures/endpoints-sample.json.

## Pin failure
A pinned request no host can serve answers **404**:
`{"error":{"message":"No endpoints found that can handle the requested parameters.","code":404}}`
(reference/fixtures/no-endpoints-404.json)

## Live results from PR #1534 (2026-11-25)
The requests were built like Koharu's **native** OpenRouter requests (strict `json_schema` +
`require_parameters` + pinned `only`), one per endpoint:

| Model | Endpoints | Valid translation | Always 404 |
|---|---|---|---|
| `deepseek/deepseek-v4.1-flash-1017` | 54 | 48 | 6, none listing `structured_outputs` |
| `z-ai/glm-5.3-flash` | 42 | 41 | 1, not listing `structured_outputs` |

- 429s were transient and succeeded on retry.
- Tier endpoints (flex, priority, fast) work when pinned.
- Every routing value returned valid output on both models.
- Through the proxy (prompted mode, no `response_format`), those 404 endpoints may work.
  Re-check in T10.
- Model ids with a `~` alias returned an empty endpoints list: they can't be pinned.

## Alternatives that need no proxy
- Model suffixes `:nitro` (sort by throughput) and `:floor` (sort by price). Both may also
  pick priority or flex tier endpoints.
- Presets: `@preset/<slug>` or `<model>@preset/<slug>`, set up in the OpenRouter dashboard,
  with provider settings inside.
