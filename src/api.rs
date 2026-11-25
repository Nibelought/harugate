//! The JSON API under `/api`, used by the web UI.
//!
//! Routes (see [`crate::ui::router`]): `GET /api/status`, `GET|PUT /api/config`,
//! `GET|PUT /api/config/raw`, `GET /api/models`, `GET /api/hosts?model=author/slug`,
//! `GET /api/requests`. Errors are `{"error":{"message"}}` (plus a string `code` when a
//! client can act on it). Nothing here ever returns or logs an API key.

use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    config::{self, Config, ConfigError},
    hosts,
    routing::Routing,
    server::local_url,
    state::AppState,
};

/// How long the upstream's model list is reused.
const MODELS_TTL: Duration = Duration::from_secs(5 * 60);

/// An error answer: `{"error":{"message"[, "code"]}}`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
    code: Option<&'static str>,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        ApiError {
            status,
            message: message.into(),
            code: None,
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut error = serde_json::Map::new();
        error.insert("message".to_owned(), Value::String(self.message));
        if let Some(code) = self.code {
            error.insert("code".to_owned(), Value::String(code.to_owned()));
        }
        (self.status, Json(json!({ "error": error }))).into_response()
    }
}

impl From<ConfigError> for ApiError {
    fn from(e: ConfigError) -> Self {
        let status = match e {
            ConfigError::Parse(_) | ConfigError::Invalid(_) => StatusCode::BAD_REQUEST,
            ConfigError::Read { .. } | ConfigError::Write { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        ApiError::new(status, e.to_string())
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

fn parse_body<T: DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|e| ApiError::bad_request(format!("invalid request body: {e}")))
}

/// The config as the UI sees it. `routing` is `null` for Balanced.
fn config_json(config: &Config) -> Value {
    json!({
        "listen": config.listen.to_string(),
        "upstream": config.upstream,
        "routing": config.routing.map(Routing::as_str),
        "require_parameters": config.require_parameters,
        "pins": config.pins,
    })
}

fn restart_required(state: &AppState, config: &Config) -> bool {
    state
        .bound_addr()
        .is_some_and(|bound| bound != config.listen)
}

/// `GET /api/status`
pub async fn status(State(state): State<AppState>) -> ApiResult {
    let config = state.config.current().await;
    let shown = state.bound_addr().unwrap_or(config.listen);
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "config_path": state.config.path().display().to_string(),
        "config_error": state.config.last_error(),
        "listen": config.listen.to_string(),
        "listen_running": state.bound_addr().map(|addr| addr.to_string()),
        "restart_required": restart_required(&state, &config),
        "upstream": config.upstream,
        "upstream_is_official": config::is_official_upstream(&config.upstream),
        "listen_is_loopback": config.listen.ip().is_loopback(),
        // Whether the key is set, never the key.
        "api_key_set": state.api_key.is_some(),
        "routing": config.routing.map(Routing::as_str),
        "require_parameters": config.require_parameters,
        "pins": config.pins.len(),
        "base_url": local_url(shown, "/v1"),
        "ui_url": local_url(shown, "/ui"),
    })))
}

/// `GET /api/config`
pub async fn get_config(State(state): State<AppState>) -> ApiResult {
    let config = state.config.current().await;
    Ok(Json(json!({
        "config": config_json(&config),
        "config_error": state.config.last_error(),
    })))
}

/// The body of `PUT /api/config`: the whole config (`routing` null or missing is Balanced).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigUpdate {
    listen: String,
    upstream: String,
    #[serde(default)]
    routing: Option<Routing>,
    require_parameters: bool,
    pins: BTreeMap<String, String>,
    /// The user confirmed the warnings in [`config::new_risks`].
    #[serde(default)]
    confirm_risky: bool,
}

fn require_confirmation(risks: &[String], confirmed: bool) -> Result<(), ApiError> {
    if risks.is_empty() || confirmed {
        return Ok(());
    }
    Err(ApiError::bad_request(format!(
        "this change needs your confirmation: {}",
        risks.join("; ")
    ))
    .with_code("confirmation_required"))
}

/// `PUT /api/config`: validate, write into the file in place (comments and key order
/// kept) and apply at once. Nothing is written on any error.
pub async fn put_config(State(state): State<AppState>, body: Bytes) -> ApiResult {
    let update: ConfigUpdate = parse_body(&body)?;
    let listen = update.listen.trim().parse::<SocketAddr>().map_err(|_| {
        ApiError::bad_request(format!(
            "listen `{}` is not an address like 127.0.0.1:8787",
            update.listen.trim()
        ))
    })?;
    let config = Config {
        listen,
        upstream: update.upstream.trim().to_owned(),
        routing: update.routing,
        require_parameters: update.require_parameters,
        pins: update
            .pins
            .into_iter()
            .map(|(model, tag)| (model.trim().to_owned(), tag.trim().to_owned()))
            .collect(),
    };
    config::validate(&config)?;
    let current = state.config.current().await;
    require_confirmation(&config::new_risks(&current, &config), update.confirm_risky)?;
    state.config.save_preserving(config.clone())?;
    tracing::info!(
        "config: saved from the web UI (routing: {}, {} pin(s))",
        config.routing.map_or("balanced", Routing::as_str),
        config.pins.len()
    );
    Ok(Json(json!({
        "saved": true,
        "config": config_json(&config),
        "restart_required": restart_required(&state, &config),
    })))
}

/// `GET /api/config/raw`
pub async fn get_raw(State(state): State<AppState>) -> ApiResult {
    // Reload first, so `config_error` describes the file as it is now.
    state.config.current().await;
    Ok(Json(json!({
        "path": state.config.path().display().to_string(),
        "text": state.config.read_text()?,
        "config_error": state.config.last_error(),
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUpdate {
    text: String,
    #[serde(default)]
    confirm_risky: bool,
}

/// `PUT /api/config/raw`: the TOML text, as the user wrote it. Validation errors come
/// back as a 400 with the parser's message; the file is untouched then.
pub async fn put_raw(State(state): State<AppState>, body: Bytes) -> ApiResult {
    let update: RawUpdate = parse_body(&body)?;
    let config = config::parse(&update.text)?;
    let current = state.config.current().await;
    require_confirmation(&config::new_risks(&current, &config), update.confirm_risky)?;
    state.config.save_text(&update.text)?;
    tracing::info!("config: saved from the web UI's raw editor");
    Ok(Json(json!({
        "saved": true,
        "text": update.text,
        "config": config_json(&config),
        "restart_required": restart_required(&state, &config),
    })))
}

/// The upstream's model ids, reused for [`MODELS_TTL`].
#[derive(Debug, Default)]
pub struct ModelsCache {
    entry: Mutex<Option<CachedModels>>,
}

#[derive(Debug)]
struct CachedModels {
    upstream: String,
    fetched: Instant,
    fetched_ms: u64,
    ids: Vec<String>,
}

impl ModelsCache {
    fn get(&self, upstream: &str) -> Option<(u64, Vec<String>)> {
        let entry = self.entry.lock().unwrap_or_else(|e| e.into_inner());
        entry
            .as_ref()
            .filter(|c| c.upstream == upstream && c.fetched.elapsed() < MODELS_TTL)
            .map(|c| (c.fetched_ms, c.ids.clone()))
    }

    fn put(&self, upstream: &str, ids: Vec<String>) -> u64 {
        let fetched_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        *self.entry.lock().unwrap_or_else(|e| e.into_inner()) = Some(CachedModels {
            upstream: upstream.to_owned(),
            fetched: Instant::now(),
            fetched_ms,
            ids,
        });
        fetched_ms
    }
}

/// The model ids in an OpenAI-style `/models` reply (`data[].id`), sorted, once each.
pub fn model_ids(reply: &Value) -> Vec<String> {
    let mut ids: Vec<String> = reply
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| model.get("id").and_then(Value::as_str))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

async fn fetch_models(state: &AppState, upstream: &str) -> Result<Vec<String>> {
    let url = format!("{}/models", upstream.trim_end_matches('/'));
    let mut request = state.client.get(url).timeout(Duration::from_secs(30));
    if let Some(key) = state.upstream_key() {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.context("can't reach the model list")?;
    let status = response.status();
    ensure!(status.is_success(), "the model list answered {status}");
    let reply: Value = response
        .json()
        .await
        .context("the model list is not valid JSON")?;
    Ok(model_ids(&reply))
}

/// `GET /api/models`: ids only, fetched from `{upstream}/models` and cached for a few minutes.
pub async fn models(State(state): State<AppState>) -> ApiResult {
    let config = state.config.current().await;
    if let Some((fetched_ms, ids)) = state.models.get(&config.upstream) {
        return Ok(Json(json!({ "models": ids, "fetched_ms": fetched_ms })));
    }
    let ids = fetch_models(&state, &config.upstream)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    let fetched_ms = state.models.put(&config.upstream, ids.clone());
    Ok(Json(json!({ "models": ids, "fetched_ms": fetched_ms })))
}

/// `GET /api/hosts?model=author/slug`: the hosts OpenRouter lists, sorted by label.
pub async fn hosts(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let id = query
        .get("model")
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            ApiError::bad_request("missing the `model` query parameter (author/slug)")
        })?;
    let config = state.config.current().await;
    hosts::endpoints_url(&config.upstream, id)
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    let list = hosts::hosts(&state.client, &config.upstream, state.upstream_key(), id)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    let hosts: Vec<Value> = list
        .iter()
        .map(|host| {
            json!({
                "tag": host.tag,
                "name": host.name,
                "label": host.label(),
                "quantization": host.quantization,
                "tools": host.tools,
                "structured_outputs": host.structured_outputs,
            })
        })
        .collect();
    Ok(Json(json!({
        "model": id,
        "pinned": config.pins.get(id),
        "hosts": hosts,
    })))
}

/// `GET /api/requests`: the recent requests, newest first.
pub async fn requests(State(state): State<AppState>) -> ApiResult {
    Ok(Json(json!({
        "capacity": state.activity.capacity(),
        "requests": state.activity.recent(),
    })))
}

/// A JSON 404 for an `/api` path that doesn't exist.
pub async fn not_found() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        ApiError::new(StatusCode::NOT_FOUND, "no such API route"),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_sorted_unique_and_ids_only() {
        let reply = json!({"data": [
            {"id": "b/two", "name": "Two", "pricing": {"prompt": "1"}},
            {"id": "a/one"}, {"id": "b/two"}, {"id": ""}, {"name": "no id"}
        ]});
        assert_eq!(model_ids(&reply), ["a/one", "b/two"]);
        assert!(model_ids(&json!({})).is_empty());
    }

    #[test]
    fn risks_need_confirmation_once() {
        let risks = vec!["x".to_owned()];
        let err = require_confirmation(&risks, false).unwrap_err();
        assert_eq!(err.code, Some("confirmation_required"));
        assert!(require_confirmation(&risks, true).is_ok());
        assert!(require_confirmation(&[], false).is_ok());
    }
}
