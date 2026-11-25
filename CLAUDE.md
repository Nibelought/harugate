# Working in harugate

Scope: only this folder. Do not edit `../koharu`; it is a read-only reference for checking
what Koharu sends (see docs/KOHARU-CONTRACT.md for the relevant file paths).

- Start from docs/TASKS.md, in order. Each task lists its acceptance criteria. A task is
  done when those pass, not when the code compiles.
- `reference/` holds code that was tested in PR #1534. Port it; don't rewrite it from
  scratch. Keep its tests and adapt them. It used `koharu_config` and `rig_core`, which
  this project must **not** depend on (see the porting notes at the top of each file).
- Never log or print API keys or full Authorization headers.
- Live OpenRouter calls cost money. Unit tests use fixtures. Live checks are a manual
  task (T10) run only when the user agrees.
