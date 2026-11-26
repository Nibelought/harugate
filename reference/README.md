# reference/

Frozen provenance from Koharu PR #1534 (branch `feat/openrouter-provider-routing-pr` @ `dc6a680a`): the code and test
fixtures harugate was ported from. **Do not edit these files.** They record what was tested in the PR.

| File | What it is |
|---|---|
| `routing.rs` | Koharu's `routing.rs` with its 6 unit tests: the `provider` object, host listing, pin-error explanation. It does not compile here (it needs `rig_core` and `koharu_config`, which harugate must not depend on). The porting notes are at the top of the file. |
| `host-label.ts` | Koharu's UI logic for labelling hosts (`DeepInfra (fp8, priority)`) |
| `fixtures/endpoints-sample.json` | A sample OpenRouter endpoints reply (has a `_note` describing what the PR's filter and harugate do with it) |
| `fixtures/no-endpoints-404.json` | The 404 body OpenRouter returns for a pin that can't serve a request |

The maintained, tested ports are:

- `src/routing.rs`: `Routing`, `provider()`, `explained()` (plus injection, which is new);
- `src/hosts.rs`: host listing and labels (`hosts()`, `hosts_in()` and `label()` from `host-label.ts`), without the
  `structured_outputs` filter.

Change those, not the files here. Both carry Koharu's copyright notice (see the README's License section).

**The fixtures are used by the build.** `reference/fixtures/*.json` are embedded with `include_str!` by `src/hosts.rs`,
`src/routing.rs` (unit tests), `tests/proxy.rs` and `tests/common/mod.rs`. Do not move or rename them; if a test needs a
different sample, add a new fixture file instead of changing these.

See [docs/KOHARU-CONTRACT.md](../docs/KOHARU-CONTRACT.md) and [docs/OPENROUTER-NOTES.md](../docs/OPENROUTER-NOTES.md)
for the research behind them.
