//! Shared state of the running proxy.

use std::{
    net::SocketAddr,
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::http::HeaderValue;

use crate::{activity::RequestLog, api::ModelsCache, config::ConfigStore};

/// Environment variable holding the key that replaces the client's (F5).
pub const API_KEY_ENV: &str = "OPENROUTER_API_KEY";

/// What every handler needs. Cheap to clone. Its `Debug` never shows a key.
#[derive(Clone, Debug)]
pub struct AppState {
    /// The config file and its hot reload. Handlers call `config.current().await`.
    pub config: Arc<ConfigStore>,
    /// Client for the upstream: connect timeout only, no redirects, no decompression.
    pub client: reqwest::Client,
    /// `Authorization: Bearer <key>` that replaces the client's, if `OPENROUTER_API_KEY`
    /// was set. Marked sensitive, so it prints as `Sensitive`.
    pub api_key: Option<HeaderValue>,
    /// The latest requests, for the web UI (no keys, bodies or headers).
    pub activity: Arc<RequestLog>,
    /// The upstream's model ids, cached for the web UI's autocomplete.
    pub models: Arc<ModelsCache>,
    /// The address the server bound at startup (`listen` is only read then).
    bound: Arc<OnceLock<SocketAddr>>,
}

#[derive(Debug, thiserror::Error)]
#[error("the API key contains characters that can't be sent in an HTTP header")]
pub struct InvalidKey;

impl AppState {
    /// `api_key` replaces whatever Authorization the client sends; `None` passes the
    /// client's through. An empty key counts as none.
    pub fn new(
        config: Arc<ConfigStore>,
        client: reqwest::Client,
        api_key: Option<&str>,
    ) -> Result<Self, InvalidKey> {
        let api_key = match api_key.map(str::trim).filter(|key| !key.is_empty()) {
            Some(key) => {
                let mut value =
                    HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| InvalidKey)?;
                value.set_sensitive(true);
                Some(value)
            }
            None => None,
        };
        Ok(AppState {
            config,
            client,
            api_key,
            activity: Arc::default(),
            models: Arc::default(),
            bound: Arc::default(),
        })
    }

    /// Remember the address the server is listening on. The first call wins.
    pub fn set_bound_addr(&self, addr: SocketAddr) {
        let _ = self.bound.set(addr);
    }

    /// The address the server is listening on, if `serve` told us.
    pub fn bound_addr(&self) -> Option<SocketAddr> {
        self.bound.get().copied()
    }

    /// The proxy's own key (`OPENROUTER_API_KEY`) as a bare token, for the upstream
    /// requests the web UI makes (model and host lists). Never put it in a response or a log.
    pub(crate) fn upstream_key(&self) -> Option<&str> {
        self.api_key
            .as_ref()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
    }

    /// Like `new`, reading the key from `OPENROUTER_API_KEY`.
    pub fn from_env(config: Arc<ConfigStore>, client: reqwest::Client) -> Result<Self, InvalidKey> {
        Self::new(config, client, std::env::var(API_KEY_ENV).ok().as_deref())
    }
}

/// The client the proxy talks to the upstream with (N5): a connect timeout and nothing
/// else, so a reasoning model may stay silent for minutes. Redirects are not followed,
/// so the client sees the upstream's status as it is.
pub fn build_client() -> reqwest::Result<reqwest::Client> {
    install_crypto_provider();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// reqwest is built with `rustls-no-provider` and this crate picks `ring` as the TLS
/// crypto provider: unlike aws-lc it needs no CMake or NASM to build on Windows. Call
/// before building any `reqwest::Client` (`build_client` does); safe to call again.
pub fn install_crypto_provider() {
    // Err means a provider is already installed (by us earlier, or by a test).
    let _ = rustls::crypto::ring::default_provider().install_default();
}
