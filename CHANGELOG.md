# Changelog

All notable changes to harugate are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- Documentation reorganised by audience: README for users; ARCHITECTURE, DEVELOPMENT, DECISIONS, REQUIREMENTS and
  ROADMAP in `docs/`; new `reference/README.md`.

## [0.1.0] - 2026-11-26

First release.

### Added
- A local proxy between Koharu and OpenRouter. `POST /v1/chat/completions` gets OpenRouter's `provider` object: a
  global sort (price, throughput, latency, or OpenRouter's default balancing) and one pinned host per model
  (`only` plus `allow_fallbacks: false`), with `require_parameters` on by default.
- Other `/v1/*` requests (such as `/v1/models`) are forwarded unchanged; anything else is a 404. Streaming responses are
  relayed chunk by chunk; upstream errors and `Retry-After` reach Koharu unchanged.
- A pin that can't serve a request is explained in OpenRouter's error shape instead of a bare "No endpoints found".
- `OPENROUTER_API_KEY` replaces the key Koharu sends; without it Koharu's key is passed through.
- A TOML config file (created at the platform's config directory on first run, or given with `--config`), re-read on
  every request so edits apply without a restart. A bad edit keeps the last good settings and is reported.
- A web page at `/ui`, embedded in the binary: status and the "Connect Koharu" block, routing and pins with host lookup,
  advanced settings with confirmation for risky values, a raw TOML editor, and the last 100 requests. Saving keeps your
  comments and key order. The page is local-only, with Host, Origin and CSRF checks and a strict
  Content-Security-Policy.
- `harugate hosts <author/slug>` lists a model's hosts with their tags and `tools` / `structured_outputs` support.
- `GET /health`, one log line per request (`RUST_LOG`), and a JSON API under `/api`.
- A bare `harugate` (double-click or terminal) starts the proxy and opens the page; a busy port or a bad config prints a
  plain message.
- Release archives for macOS (universal, arm64, x64), Windows (x64, arm64) and Linux (x64, arm64; static musl), with
  `SHA256SUMS.txt`, and one-line installers `install.sh` and `install.ps1`. The binaries are not code-signed.

[Unreleased]: https://github.com/Nibelought/harugate/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Nibelought/harugate/releases/tag/v0.1.0
