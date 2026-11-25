//! The `/v1` surface: `POST /v1/chat/completions` gets the `provider` object injected
//! (F3); every other `/v1/*` request is passed through unchanged (F1). Responses are
//! streamed back as they arrive (F2, N3), apart from the one F7 rewrite.

use std::time::Instant;

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;

use crate::{
    routing::{self, Applied},
    state::AppState,
};

/// Headers that describe one connection, not the message (RFC 9110 §7.6.1).
const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn is_hop_by_hop(name: &HeaderName) -> bool {
    HOP_BY_HOP.contains(&name.as_str())
}

/// An error in OpenRouter's JSON shape, `{"error":{"message","code"}}`.
pub fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        routing::error_body(message, status.as_u16()).to_string(),
    )
        .into_response()
}

/// The upstream URL for `/v1/<rest>?<query>`.
fn upstream_url(base: &str, rest: &str, query: Option<&str>) -> String {
    let mut url = format!("{}/{}", base.trim_end_matches('/'), rest);
    if let Some(query) = query {
        url.push('?');
        url.push_str(query);
    }
    url
}

/// The headers to send upstream: the client's, minus hop-by-hop ones, `Host` and
/// `Content-Length` (set for the new request), `Accept-Encoding` (so the upstream
/// answers in plain text and the F7 rewrite can read it), and `Authorization`, which
/// is the client's, or the proxy's own key (F5). Credentials are marked sensitive.
fn upstream_headers(client: &HeaderMap, state: &AppState) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in client {
        if is_hop_by_hop(name)
            || name == header::HOST
            || name == header::CONTENT_LENGTH
            || name == header::ACCEPT_ENCODING
            || name == header::AUTHORIZATION
        {
            continue;
        }
        headers.append(name.clone(), value.clone());
    }
    let auth = state
        .api_key
        .clone()
        .or_else(|| client.get(header::AUTHORIZATION).cloned());
    if let Some(mut auth) = auth {
        auth.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth);
    }
    headers
}

fn response_headers(upstream: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in upstream {
        if !is_hop_by_hop(name) {
            headers.append(name.clone(), value.clone());
        }
    }
    headers
}

/// Handler for every `/v1/*` path.
pub async fn v1(State(state): State<AppState>, request: Request) -> Response {
    let start = Instant::now();
    let (parts, body) = request.into_parts();
    let config = state.config.current().await;
    let rest = parts.uri.path().strip_prefix("/v1/").unwrap_or_default();
    let url = upstream_url(&config.upstream, rest, parts.uri.query());
    let chat = parts.method == Method::POST && rest.trim_end_matches('/') == "chat/completions";

    let mut model: Option<String> = None;
    let mut applied = Applied::Untouched;
    let upstream_body = if chat {
        // N4: no limit of our own; the upstream decides what is too large.
        let raw = match to_bytes(body, usize::MAX).await {
            Ok(raw) => raw,
            Err(e) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    &format!("can't read the request body: {e}"),
                );
            }
        };
        match routing::inject_bytes(&raw, &config) {
            Ok(injected) => {
                model = injected.model;
                applied = injected.applied;
                reqwest::Body::from(injected.body)
            }
            Err(e) => {
                tracing::warn!(method = %parts.method, path = %parts.uri.path(), status = 400, "{e}");
                state.activity.record(
                    parts.method.as_str(),
                    parts.uri.path(),
                    None,
                    &Applied::Untouched,
                    400,
                    start.elapsed().as_millis() as u64,
                );
                return json_error(StatusCode::BAD_REQUEST, &e.to_string());
            }
        }
    } else {
        reqwest::Body::wrap_stream(body.into_data_stream())
    };

    let result = state
        .client
        .request(parts.method.clone(), url)
        .headers(upstream_headers(&parts.headers, &state))
        .body(upstream_body)
        .send()
        .await;
    let upstream = match result {
        Ok(upstream) => upstream,
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            tracing::error!(
                method = %parts.method, path = %parts.uri.path(), model = model.as_deref().unwrap_or("-"),
                routing = %applied, status = 502, latency_ms, "upstream unreachable: {e}"
            );
            state.activity.record(
                parts.method.as_str(),
                parts.uri.path(),
                model.as_deref(),
                &applied,
                502,
                latency_ms,
            );
            return json_error(
                StatusCode::BAD_GATEWAY,
                &format!("can't reach the upstream: {e}"),
            );
        }
    };

    let status = upstream.status();
    let headers = response_headers(upstream.headers());
    tracing::info!(
        method = %parts.method, path = %parts.uri.path(), model = model.as_deref().unwrap_or("-"),
        routing = %applied, status = status.as_u16(), latency_ms = start.elapsed().as_millis() as u64,
        "request"
    );
    state.activity.record(
        parts.method.as_str(),
        parts.uri.path(),
        model.as_deref(),
        &applied,
        status.as_u16(),
        start.elapsed().as_millis() as u64,
    );

    // F7: the one response that is read whole, and only when a pin may have caused it.
    if let (Applied::Pin(tag), StatusCode::NOT_FOUND) = (&applied, status) {
        let raw: Bytes = match upstream.bytes().await {
            Ok(raw) => raw,
            Err(e) => {
                return json_error(
                    StatusCode::BAD_GATEWAY,
                    &format!("can't read the upstream's answer: {e}"),
                );
            }
        };
        let (headers, body) = match routing::explained(
            Some(tag),
            model.as_deref().unwrap_or_default(),
            state.config.path(),
            status.as_u16(),
            &raw,
        ) {
            Some(rewritten) => {
                let mut headers = headers;
                headers.remove(header::CONTENT_LENGTH);
                headers.insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                (headers, rewritten)
            }
            None => (headers, raw),
        };
        return build(status, headers, Body::from(body));
    }

    build(status, headers, Body::from_stream(upstream.bytes_stream()))
}

fn build(status: StatusCode, headers: HeaderMap, body: Body) -> Response {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_urls_are_joined_with_one_slash_and_the_query_kept() {
        assert_eq!(
            upstream_url("https://openrouter.ai/api/v1", "models", None),
            "https://openrouter.ai/api/v1/models"
        );
        assert_eq!(
            upstream_url("http://h:1/api/v1/", "models?x", Some("a=b&c=d")),
            "http://h:1/api/v1/models?x?a=b&c=d"
        );
    }

    #[test]
    fn hop_by_hop_headers_are_dropped_both_ways() {
        let mut client = HeaderMap::new();
        client.insert("connection", "close".parse().unwrap());
        client.insert("keep-alive", "1".parse().unwrap());
        client.insert("host", "127.0.0.1".parse().unwrap());
        client.insert("accept-encoding", "gzip".parse().unwrap());
        client.insert("x-title", "Koharu".parse().unwrap());
        client.insert("authorization", "Bearer client".parse().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        std::fs::write(&path, "").unwrap();
        let config = crate::config::ConfigStore::open(path).unwrap();
        let client_http = crate::state::build_client().unwrap();
        let state = AppState::new(config.into(), client_http, None).unwrap();
        let out = upstream_headers(&client, &state);
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out["x-title"], "Koharu");
        assert_eq!(out["authorization"], "Bearer client");
        assert!(out["authorization"].is_sensitive());

        let keyed =
            AppState::new(state.config.clone(), state.client.clone(), Some("sk-env")).unwrap();
        assert_eq!(
            upstream_headers(&client, &keyed)["authorization"],
            "Bearer sk-env"
        );
        // The proxy's key is added even when the client sends none.
        assert_eq!(
            upstream_headers(&HeaderMap::new(), &keyed)["authorization"],
            "Bearer sk-env"
        );
        assert!(!format!("{keyed:?}").contains("sk-env"));

        let mut upstream = HeaderMap::new();
        upstream.insert("transfer-encoding", "chunked".parse().unwrap());
        upstream.insert("retry-after", "7".parse().unwrap());
        let out = response_headers(&upstream);
        assert_eq!(out.len(), 1);
        assert_eq!(out["retry-after"], "7");
    }
}
