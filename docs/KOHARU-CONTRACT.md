# What Koharu sends and expects

Verified against `../koharu`, branch `feat/openrouter-provider-routing-pr` (dc6a680a), which is upstream 0.95.2 plus
PR #1534. Paths are relative to `../koharu` (a read-only reference: never edit it). If Koharu changes, re-check these
facts against its source before relying on them.

## 1. Transport
- Vendor `openai-compatible` uses rig 0.47's OpenAI client in **Chat Completions** mode (not Responses):
  `POST {base_url}/chat/completions`.
  (`crates/koharu-llm/src/hosted/openai_compatible.rs`)
- The model list comes from `GET {base_url}/models`, read as OpenAI's shape (`data[].id`, optional `name`).
- `base_url` "ends where OpenAI's `/v1` does", so set it to `http://127.0.0.1:<port>/v1`.
- Auth: `Authorization: Bearer <key stored for openai-compatible>`; an empty key if none is stored.

## 2. Retries
Koharu's HTTP client (`crates/koharu-llm/src/hosted/http.rs`) retries what rig calls retryable (429, 5xx) up to 3 times,
waiting as the reply's headers say (`Retry-After`, rate-limit reset), else 1 s / 2 s / 4 s, capped at 120 s. It doesn't
retry when it can't connect. **So harugate must not retry on its own, and must pass statuses and headers through.**

## 3. Structured output
`crates/koharu-llm/src/model.rs`, `output_mode()`: for `openai-compatible` Koharu uses `OutputMode::Prompted`. The JSON
schema is in the prompt, **not** in `response_format`. Consequences:
- `require_parameters: true` does not filter hosts by `structured_outputs`, since that parameter isn't sent. The PR's host
  list dropped those endpoints. Through harugate, hosts without it can serve translation too, so harugate lists them
  and shows the flag.
- Koharu's own OpenRouter vendor (`Native` mode + `require_parameters`) is stricter. Translation through harugate relies
  on the model following the prompt. Koharu asks again once if an answer doesn't fit the schema
  (`default_max_turns(2)`).

## 4. Which requests go through
Everything for a model on the OpenAI-compatible vendor:
- Translation (`crates/koharu-pipeline/src/stages/translate/turn.rs` → `koharu_llm::answers`).
- Context summaries (`crates/koharu-llm/src/context.rs`, `summarize`).
- **The in-app agent** (`crates/koharu-agent/src/agent.rs`, `attempt()`, streams with tools). The PR did not route the
  agent; harugate does. Risk: a pinned host that can't call tools answers 404 to every agent request (with
  `require_parameters`, OpenRouter requires `tools` support). The user sees the F7 message, and the web page marks hosts
  without tools. Possible follow-ups, not built: a per-pin `allow_fallbacks`, or pins that skip requests carrying
  `tools` (see [ROADMAP.md](ROADMAP.md)).

## 5. Koharu setup
Settings → Providers → **OpenAI-compatible**:
- Base URL: `http://127.0.0.1:8787/v1` (or your port).
- API key: the OpenRouter key (or leave empty and set `OPENROUTER_API_KEY` for harugate).
- **Context window: set it.** OpenAI-compatible applies one context window to all of its models and reads none from
  `/models`; unset, a conversation grows until the server cuts it off. Use the smallest window of the models you use.

Then pick the model under Pipeline → Translator. It shows as an OpenAI-compatible model with OpenRouter's id.

## 6. Differences from Koharu's own OpenRouter vendor
- No catalog facts (pricing, limits, reasoning support): models are "unlisted" with unknown limits, tools allowed, and
  image input allowed.
- Prompted, not native, structured output (§3).
