//! harugate: a local HTTP proxy that adds OpenRouter's `provider` routing object
//! to chat requests (a global sort rule, and a provider pinned per model).
//!
//! Library plus a thin binary: `main.rs` only parses the command line and calls in.
//!
//! - [`config`]: the config file, `parse` / `validate`, `ConfigStore` with hot reload and save.
//! - [`routing`]: building and injecting the `provider` object, explaining pin errors.
//! - [`proxy`]: the `/v1` handler (forwarding, streaming).
//! - [`hosts`]: listing the endpoints OpenRouter has for a model.
//! - [`state`]: `AppState`, shared by all handlers.
//! - [`activity`]: the in-memory list of recent requests shown in the web UI.
//! - [`api`]: the JSON API under `/api` that the web UI (and anything else local) uses.
//! - [`ui`]: the embedded web UI at `/ui`, and the checks that protect it and `/api`.
//! - [`server`]: `app(state) -> Router`, `serve`, `run`.
//! - [`startup`]: the plain-language report when `serve` can't start (port in use, bad config).

pub mod activity;
pub mod api;
pub mod config;
pub mod hosts;
pub mod proxy;
pub mod routing;
pub mod server;
pub mod startup;
pub mod state;
pub mod ui;

/// Install the log subscriber: level from `RUST_LOG` (default `info`), to stderr.
pub fn init_logging() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
