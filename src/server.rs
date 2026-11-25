//! The axum router and the `serve` entry point.

use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{any, get},
};
use serde_json::{Value, json};

use crate::{
    config::{self, ConfigStore},
    proxy,
    routing::Routing,
    state::{AppState, build_client},
    ui,
};

/// The proxy's routes: `GET /health` and `/v1/*`, plus the web UI (`/`, `/ui`, `/api/*`,
/// see [`ui::router`]); anything else is a 404 (F1). Returns a `Router<()>` with the state
/// applied. The UI's checks (Host, Origin, headers) apply to its routes only; `/v1/*` and
/// `/health` behave as before.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/{*rest}", any(proxy::v1))
        .layer(DefaultBodyLimit::disable())
        .with_state(state.clone())
        .merge(ui::router(state))
}

/// `http://<addr><path>` for a browser or Koharu on this machine: an unspecified bind
/// address (0.0.0.0, ::) is reached through loopback.
pub fn local_url(addr: SocketAddr, path: &str) -> String {
    let ip = match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!("http://{}{path}", SocketAddr::new(ip, addr.port()))
}

/// F9: the loaded config's routing, pin count and config path.
async fn health(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let config = state.config.current().await;
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "routing": config.routing.map_or("balanced", Routing::as_str),
            "pins": config.pins.len(),
            "require_parameters": config.require_parameters,
            "config_path": state.config.path().display().to_string(),
            "config_error": state.config.last_error(),
        })),
    )
}

/// Resolve where the config lives: `--config` if given, else the default path.
/// Returns the path and whether it is the default (which `serve` creates if missing).
pub fn resolve_config_path(explicit: Option<PathBuf>) -> Result<(PathBuf, bool)> {
    match explicit {
        Some(path) => Ok((path, false)),
        None => config::default_path()
            .map(|path| (path, true))
            .context("can't find this platform's config directory; pass --config <path>"),
    }
}

/// Run `serve`: load (and, at the default path, create) the config, bind its `listen`
/// address and serve until Ctrl-C / SIGTERM. With `open_browser`, the web UI is opened in
/// the default browser once the port is bound (a failure to do so is only logged).
pub async fn run(config_path: Option<PathBuf>, open_browser: bool) -> Result<()> {
    let (path, is_default) = resolve_config_path(config_path)?;
    if is_default && config::ensure_default_file(&path)? {
        tracing::info!("created a default config at {}", path.display());
    }
    if !path.exists() {
        anyhow::bail!(
            "config file {} does not exist (create it, see config.example.toml)",
            path.display()
        );
    }
    let store = Arc::new(ConfigStore::open(&path)?);
    let config = store.snapshot();
    let state = AppState::from_env(store, build_client()?)?;
    if state.api_key.is_some() {
        tracing::info!("using the key from {}", crate::state::API_KEY_ENV);
    }
    if !config.listen.ip().is_loopback() {
        tracing::warn!(
            "listening on {}, which is not loopback: anyone who can reach it can use your OpenRouter key",
            config.listen
        );
    }
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("can't listen on {}", config.listen))?;
    let addr = listener.local_addr()?;
    state.set_bound_addr(addr);
    tracing::info!(
        "config {} | listening on http://{}/v1 | routing: {} | {} pin(s)",
        path.display(),
        addr,
        config.routing.map_or("balanced", Routing::as_str),
        config.pins.len()
    );
    let ui_url = local_url(addr, "/ui");
    tracing::info!("web UI: {ui_url}");
    if open_browser {
        // `webbrowser::open` can block (console browsers), so keep it off the runtime.
        tokio::task::spawn_blocking(move || match webbrowser::open(&ui_url) {
            Ok(()) => tracing::info!("opened the web UI in your browser"),
            Err(e) => tracing::warn!("couldn't open a browser ({e}); open {ui_url} yourself"),
        });
    }
    serve(listener, app(state), shutdown_signal()).await?;
    tracing::info!("stopped");
    Ok(())
}

/// Serve `router` on `listener` until `shutdown` completes.
pub async fn serve(
    listener: tokio::net::TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

/// Completes on Ctrl-C (any platform) or SIGTERM (unix).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
