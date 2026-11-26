# harugate

harugate is a small local proxy between [Koharu](https://github.com/koharu-rs/koharu) and
[OpenRouter](https://openrouter.ai). It adds OpenRouter's `provider` routing object to every chat
request: a global sort rule (price, throughput, latency or OpenRouter's default balancing) and,
optionally, one pinned host per model. It ships with a web page (served by the same program) where
you choose the routing and the pins, so you need neither the command line nor a text editor.

The name: *Haru* (春, "spring") from Koharu, plus *gate*.

Koharu is not modified. You point Koharu's **OpenAI-compatible** provider at harugate, and harugate
forwards to OpenRouter. This is the route the Koharu maintainers suggested when they declined the
in-app version (koharu-rs/koharu#1532, PR #1534): *"use a proxy between Koharu and OpenRouter
instead: have it add the `provider` object to each request, and set its URL under Settings →
Providers → OpenAI-compatible."*

## Install

You don't need Rust. Download the file for your system from the
[latest release](https://github.com/Nibelought/harugate/releases/latest) (`<version>` is the release number,
e.g. `0.1.0`), unpack it, and run `harugate`:

| System | Download | Rust target |
|---|---|---|
| **macOS**, Apple silicon and Intel | `harugate-<version>-macos-universal.tar.gz` | both `aarch64-apple-darwin` and `x86_64-apple-darwin` |
| **Windows** 10/11, 64-bit | `harugate-<version>-windows-x64.zip` | `x86_64-pc-windows-msvc` |
| Windows on ARM | `harugate-<version>-windows-arm64.zip` | `aarch64-pc-windows-msvc` |
| **Linux** x86-64 | `harugate-<version>-linux-x64.tar.gz` | `x86_64-unknown-linux-musl` |
| Linux ARM64 (Raspberry Pi 4/5, ARM servers) | `harugate-<version>-linux-arm64.tar.gz` | `aarch64-unknown-linux-musl` |

Also published: `macos-arm64` and `macos-x64` (one architecture each; the universal file covers both) and
`SHA256SUMS.txt`. The Linux builds are static: they run on any distribution. Each archive holds the
program, this README, both licenses and `config.example.toml`.

**One-line install** (downloads the right file, checks its SHA-256, puts it in your user folder; no
administrator rights):

```sh
# macOS / Linux: installs to ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/Nibelought/harugate/main/install.sh | sh
```

```powershell
# Windows PowerShell: installs to %LOCALAPPDATA%\Programs\harugate and adds it to your PATH
irm https://raw.githubusercontent.com/Nibelought/harugate/main/install.ps1 | iex
```

Read them first if you like: [`install.sh`](https://github.com/Nibelought/harugate/blob/main/install.sh) and [`install.ps1`](https://github.com/Nibelought/harugate/blob/main/install.ps1) are short. A
specific version: `curl -fsSL .../install.sh | sh -s -- 0.1.0`, or in PowerShell
`$env:HARUGATE_VERSION = "0.1.0"` before the `irm` line. To uninstall, delete the `harugate` file (and, on
Windows, the folder from your PATH). Your config stays where it is (see [Configuration](#configuration-reference)).

**macOS says "can't be opened" or "Apple cannot check it":** the binaries are not signed with an Apple
developer certificate. The one-line installer avoids the warning. For a file you downloaded in the browser,
do either of these once:

- System Settings → Privacy & Security → scroll down to the message about `harugate` → **Open Anyway**; or
- in Terminal: `xattr -d com.apple.quarantine /path/to/harugate`.

**Windows SmartScreen says "Windows protected your PC":** the program is not code-signed. Click
**More info**, then **Run anyway**. (The installer script doesn't trigger it.)

**Linux:** from the file manager, open a terminal in the folder and run `./harugate` (add `chmod +x harugate`
if needed).

**Build from source** instead (needs [Rust](https://rustup.rs) 1.88 or newer; no OpenSSL, CMake or NASM):

| System | Steps |
|---|---|
| macOS | `xcode-select --install` (once, for the C compiler), then `cargo build --release` |
| Linux | a C compiler (`sudo apt install build-essential` or your distro's equivalent), then `cargo build --release` |
| Windows | install Rust with rustup (it asks for the "Desktop development with C++" Build Tools; accept), then `cargo build --release` in PowerShell |

The binary is `target/release/harugate` (`target\release\harugate.exe` on Windows). It is one file: the web
page is inside. `cargo install --git https://github.com/Nibelought/harugate` builds it and puts it on your PATH.

## Quick start

1. **Start harugate.** Double-click `harugate` (`harugate.exe`), or type `harugate` in a terminal. It starts
   and opens its page, <http://127.0.0.1:8787/ui>, in your browser. Leave the window open while you use
   Koharu; closing it stops harugate. If harugate is already running, a second start says so and opens the
   page of the first. Stop it with Ctrl-C.
2. **Connect Koharu.** In Koharu: Settings → Providers → **OpenAI-compatible**.
   - **Base URL:** `http://127.0.0.1:8787/v1` (the page shows yours, with a Copy button, under "Connect Koharu").
   - **API key:** your OpenRouter key, or leave it empty if harugate has `OPENROUTER_API_KEY` set (see below).
   - **Context window: set it.** OpenAI-compatible applies one window to all its models and reads none from
     the server; unset, a conversation grows until the server cuts it off. Use the smallest window of the
     models you use.
3. **Pick the model** under Pipeline → Translator. It shows as an OpenAI-compatible model with OpenRouter's id
   (`author/slug`).
4. **Optional:** choose a routing rule or pin a host on harugate's page (next section).

If you don't change anything, harugate asks OpenRouter to balance price and uptime (its default) and only
use hosts that support every parameter the request sends.

## Using it

### Routing and pins (web page)

1. Open the page (<http://127.0.0.1:8787/ui>). Under **Routing & pins**, pick how models *without* a pin are
   sorted: Balanced (OpenRouter's default), Price, Throughput or Latency. Leave "Only use hosts that support
   every setting the request sends" on unless you know you need it off.
2. To pin a model to one host: type its id (it completes from OpenRouter's model list) and press
   **Find hosts**. The table lists each host (sorted by label) with its quantization and whether it supports
   `tools` and `structured_outputs`. Press **Pin this** on the one you want.
3. Press **Save changes**. It applies to the next request; no restart.
4. **Requests** shows the last 100 requests (model, what harugate applied, status, time), so you can see a
   pin in use. Only those facts are kept, and only in memory.

**Advanced** holds the upstream URL and the listen address (the latter changes after a restart), and
**Raw file** edits the TOML directly; it checks your edit and shows the error without saving a bad file.

What each setting sends to OpenRouter:

| Setting for the model | `provider` object added to the request |
|---|---|
| Pinned to tag `T` | `{"require_parameters": true, "only": ["T"], "allow_fallbacks": false}` |
| No pin, routing `price`, `throughput` or `latency` | `{"require_parameters": true, "sort": "<routing>"}` |
| No pin, Balanced | `{"require_parameters": true}` |

(`require_parameters` is `false` if you turn that setting off.)

### Find a host from the command line

```sh
harugate hosts deepseek/deepseek-v4.1-flash-1017
```

prints one line per host: tag, label, quantization, `tools=…`, `structured_outputs=…`. Put the tag in the
config file and save; the running harugate picks it up on the next request:

```toml
[pins]
"deepseek/deepseek-v4.1-flash-1017" = "deepinfra/fp8"
```

### Is it working?

- The page's **Requests** tab lists each request with the routing applied (`pin:<tag>`, `sort:<rule>`,
  `balanced`).
- `http://127.0.0.1:8787/health` answers with the loaded routing, pin count and config path, and a
  `config_error` if your last edit of the file was rejected (harugate then keeps the last good settings).
- Port in use? Change `listen` (Advanced, or the file) and restart; then update Koharu's base URL.

## Caveats

- **Agent + pins.** Koharu's in-app agent goes through harugate too, and it sends `tools`. A model pinned
  to a host that doesn't support tools answers 404 to every agent request (harugate explains which pin it
  was). The page marks those hosts. If you use the agent, pin only hosts with tools=yes, or don't pin that
  model.
- **Pins don't fall back.** A pinned request fails rather than use another host; that is what a pin is.
- **Prompted structured output.** Through OpenAI-compatible, Koharu puts the JSON schema in the prompt and
  sends no `response_format`. So hosts without `structured_outputs` can serve translation (harugate lists
  them), but the result depends on the model following the prompt. Koharu asks once more if an answer doesn't
  fit.
- **Context window.** Set it in Koharu (see Quick start); harugate can't know it.
- **`~` aliases** (`~vendor/model`) list no endpoints, so they can't be pinned.
- **Unsigned binaries.** See the macOS and Windows notes under Install.

## Configuration reference

### Config file

Created with defaults on the first `serve` at:

| System | Default path |
|---|---|
| macOS | `~/Library/Application Support/harugate/config.toml` |
| Linux | `~/.config/harugate/config.toml` (or `$XDG_CONFIG_HOME/harugate/config.toml`) |
| Windows | `%APPDATA%\harugate\config.toml` |

The page shows the path in use. Use another file with `--config <path>` (it must exist). Edit it by hand or
from the page; harugate re-reads it on every request, so changes apply without a restart (except `listen`).
A bad edit keeps the last good settings and is reported in the log, in `/health` and on the page. At startup,
a bad file stops harugate with a message naming the problem. Unknown keys are errors.
[`config.example.toml`](https://github.com/Nibelought/harugate/blob/main/config.example.toml) is a commented sample.

| Key | Default | Meaning |
|---|---|---|
| `listen` | `"127.0.0.1:8787"` | Address and port harugate listens on. Read at startup only. Anything but a loopback address lets other machines use your key through harugate. |
| `upstream` | `"https://openrouter.ai/api/v1"` | Where requests are forwarded. It receives your API key. |
| `routing` | unset (Balanced) | `"price"`, `"throughput"` or `"latency"`: the sort for models without a pin. |
| `require_parameters` | `true` | Only use hosts that support every parameter the request sends. |
| `[pins]` | empty | `"author/slug" = "endpoint-tag"`, one line per pinned model. |

### Environment variables

| Variable | Effect |
|---|---|
| `OPENROUTER_API_KEY` | If set, replaces whatever key Koharu sends (Koharu's key field can stay empty). Also used by `harugate hosts` and by the page's model and host lookups. The page only shows whether it is set, never the key, and has no field for one. macOS/Linux: `export OPENROUTER_API_KEY=sk-or-...` before starting. Windows PowerShell: `setx OPENROUTER_API_KEY "sk-or-..."`, then open a new window. |
| `RUST_LOG` | Log level, e.g. `debug` (default `info`). Logs go to stderr, one line per request; keys are never logged. |

### Command line

| Command | What it does |
|---|---|
| `harugate` | Same as `serve`, and opens the page in your browser when started from a terminal or by double-click. |
| `harugate serve` | Runs harugate without opening a browser (scripts, services). The page's URL is in the log. |
| `harugate serve --open` | Runs it and always opens the page. |
| `harugate hosts <author/slug>` | Lists the hosts OpenRouter has for a model, with their tags. |
| `--config <PATH>` | Use this config file (any command). |
| `--version`, `--help` | |

### What harugate serves

| Path | |
|---|---|
| `/v1/*` | OpenAI-compatible API for Koharu. `POST /v1/chat/completions` gets the `provider` object; everything else under `/v1` is forwarded unchanged. |
| `/health` | Status as JSON. |
| `/ui`, `/api/*` | The web page and its JSON API (local only; see [ARCHITECTURE](https://github.com/Nibelought/harugate/blob/main/docs/ARCHITECTURE.md#web-ui-and-json-api)). |

Any other path is a 404 and is never forwarded.

The page can change where your API key is sent (`upstream`), so it is built for one local user: it only
answers requests addressed to localhost, refuses requests from other web pages, and asks for explicit
confirmation before you set a non-OpenRouter upstream or a non-loopback listen address. Details are in
[ARCHITECTURE](https://github.com/Nibelought/harugate/blob/main/docs/ARCHITECTURE.md#web-ui-and-json-api).

## Documentation

For people changing harugate (and for AI agents working in this repository):

| File | Read it for |
|---|---|
| [docs/ARCHITECTURE.md](https://github.com/Nibelought/harugate/blob/main/docs/ARCHITECTURE.md) | Modules, request flow, web UI and API, security model, tests, where to change what |
| [docs/DEVELOPMENT.md](https://github.com/Nibelought/harugate/blob/main/docs/DEVELOPMENT.md) | Build, test, CI, live checks, releasing, docs checklist |
| [docs/REQUIREMENTS.md](https://github.com/Nibelought/harugate/blob/main/docs/REQUIREMENTS.md) | The specification (F and N ids) |
| [docs/DECISIONS.md](https://github.com/Nibelought/harugate/blob/main/docs/DECISIONS.md) | Why it is built this way |
| [docs/ROADMAP.md](https://github.com/Nibelought/harugate/blob/main/docs/ROADMAP.md) | What is done and what remains |
| [docs/KOHARU-CONTRACT.md](https://github.com/Nibelought/harugate/blob/main/docs/KOHARU-CONTRACT.md) | What Koharu sends and expects, checked against its source |
| [docs/OPENROUTER-NOTES.md](https://github.com/Nibelought/harugate/blob/main/docs/OPENROUTER-NOTES.md) | The `provider` object, the endpoints API, live-test results |
| [CHANGELOG.md](https://github.com/Nibelought/harugate/blob/main/CHANGELOG.md) | Release history |

## License

harugate is dual-licensed under [MIT](https://github.com/Nibelought/harugate/blob/main/LICENSE-MIT) or [Apache-2.0](https://github.com/Nibelought/harugate/blob/main/LICENSE-APACHE), at your option.

Parts of `src/routing.rs`, `src/hosts.rs` and the files in `reference/` are ported from Koharu
(https://github.com/koharu-rs/koharu), Copyright (c) 2025-2026 Mayo Takanashi and Koharu contributors,
used under its MIT/Apache-2.0 license.
