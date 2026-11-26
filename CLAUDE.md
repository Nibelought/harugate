# Working in harugate

harugate is a local proxy (Rust, axum) between Koharu and OpenRouter. It adds OpenRouter's `provider` routing object to
chat requests (a global sort rule, and a host pinned per model) and serves a small local web UI. v0.1.0 is released;
this is maintenance work.

## Where to look first
- `docs/ARCHITECTURE.md`: module map, request flow, UI/API security model, tests, "where to change X".
- `docs/REQUIREMENTS.md`: the spec. F1-F10 and N1-N12 are cited from code, tests and docs.
- `docs/ROADMAP.md`: what is open. `docs/DECISIONS.md`: why it is built this way.
- `docs/DEVELOPMENT.md`: commands, CI, live checks, releasing, docs checklist.
- `docs/KOHARU-CONTRACT.md` and `docs/OPENROUTER-NOTES.md`: verified facts about what Koharu sends and what OpenRouter does.

## Before you say it is done
Run all three and have them clean:
```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```
A change is done when its behaviour matches REQUIREMENTS and the checks pass, not when it compiles.

## Rules
- Scope: only this folder. `../koharu` is a read-only reference for checking what Koharu sends; never edit it.
  KOHARU-CONTRACT lists the relevant source paths.
- Do not add `koharu_config` or `rig_core` (or anything from Koharu's workspace) as dependencies.
- Never log or print API keys or full Authorization headers. The key comes from the client or `OPENROUTER_API_KEY` only.
- Tests are offline: fixtures and local mock upstreams (`tests/common/mod.rs`), never the real OpenRouter.
- Live OpenRouter calls cost money: run one only with the user's explicit OK each time, key via the environment, tiny
  `max_tokens` (see DEVELOPMENT.md).
- Keep REQUIREMENTS ids stable: never renumber or reuse an id.
- Don't edit `reference/` (frozen provenance, but `reference/fixtures/` is compiled into tests). Change `src/routing.rs`
  and `src/hosts.rs` instead.
- Behaviour change: update the docs named in the checklist in DEVELOPMENT.md, and add a user-visible entry under
  `[Unreleased]` in `CHANGELOG.md`.
- Don't commit, push, tag or release unless asked. The release procedure is in DEVELOPMENT.md (Releasing).
