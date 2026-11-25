//! The web UI: the embedded page at `/ui`, the `/api` routes, and the checks that make
//! a local server that can change where API keys are sent safe to leave running.
//!
//! The page is plain HTML, CSS and JS in `ui/`, embedded with `include_str!` (one
//! self-contained binary, N6) and served with a strict Content-Security-Policy.
//!
//! [`guard`] runs on every `/ui`, `/api` and `/` request, and never on `/v1` or `/health`:
//! - **DNS rebinding** (N8): the `Host` header must be `localhost`, `127.0.0.1`, `[::1]`
//!   (any port) or the configured listen address. A page on `evil.example` that
//!   re-points its name at 127.0.0.1 still sends `Host: evil.example`.
//! - **CSRF** (N9): a request that changes something (not GET/HEAD) needs
//!   `Content-Type: application/json` (a form or a no-preflight request can't send
//!   that) and the custom header `X-Proxy-UI: 1` (which forces a CORS preflight that we
//!   never grant: no CORS headers are emitted). If the browser sent `Origin`, it must be
//!   this server's own origin, and `Sec-Fetch-Site`, if present, must say same-origin.
//! - **Headers** (N10): CSP (`default-src 'self'`, no inline script or style),
//!   `nosniff`, `frame-ancestors 'none'` / `X-Frame-Options: DENY`, `Referrer-Policy:
//!   no-referrer`, and `no-store` for the API.

use std::net::SocketAddr;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};

use crate::{api, state::AppState};

const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_CSS: &str = include_str!("../ui/app.css");
const APP_JS: &str = include_str!("../ui/app.js");
const FAVICON_SVG: &str = include_str!("../ui/favicon.svg");

/// The custom request header every changing `/api` request must carry.
pub const UI_HEADER: &str = "x-proxy-ui";

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'";

/// Largest `/api` request body (the config file is tiny).
const API_BODY_LIMIT: usize = 1024 * 1024;

/// The web UI and API routes, to merge into the proxy's router (`server::app` does).
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::temporary("/ui") }))
        .route("/ui", get(index))
        .route("/ui/", get(index))
        .route(
            "/ui/app.css",
            get(|| async { asset("text/css; charset=utf-8", APP_CSS) }),
        )
        .route(
            "/ui/app.js",
            get(|| async { asset("text/javascript; charset=utf-8", APP_JS) }),
        )
        .route(
            "/ui/favicon.svg",
            get(|| async { asset("image/svg+xml", FAVICON_SVG) }),
        )
        .route("/api/status", get(api::status))
        .route("/api/config", get(api::get_config).put(api::put_config))
        .route("/api/config/raw", get(api::get_raw).put(api::put_raw))
        .route("/api/models", get(api::models))
        .route("/api/hosts", get(api::hosts))
        .route("/api/requests", get(api::requests))
        .route("/api/{*rest}", axum::routing::any(api::not_found))
        .layer(DefaultBodyLimit::max(API_BODY_LIMIT))
        .route_layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

async fn index() -> Response {
    asset("text/html; charset=utf-8", INDEX_HTML)
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

fn reject(status: StatusCode, message: &str) -> Box<Response> {
    Box::new(api::ApiError::new(status, message).into_response())
}

/// See the module docs. Wraps every route of [`router`], so rejections get the security
/// headers too.
async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let mut response = match check(&state, &request) {
        Ok(()) => next.run(request).await,
        Err(rejection) => *rejection,
    };
    add_security_headers(response.headers_mut());
    response
}

fn check(state: &AppState, request: &Request) -> Result<(), Box<Response>> {
    let headers = request.headers();
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| request.uri().authority().map(|a| a.as_str().to_owned()));
    let listen = state.config.snapshot().listen;
    let allowed = host
        .as_deref()
        .is_some_and(|host| host_allowed(host, &[Some(listen), state.bound_addr()]));
    if !allowed {
        tracing::warn!("web UI: refused a request with Host {host:?} (DNS rebinding defense)");
        return Err(reject(
            StatusCode::FORBIDDEN,
            "this address is not allowed: open the UI as http://localhost, http://127.0.0.1 or http://[::1] (with the proxy's port)",
        ));
    }
    if !request.uri().path().starts_with("/api/") {
        return Ok(());
    }
    // Everything below is for /api. A browser tab on another site must not use it.
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok())
        && !matches!(site, "same-origin" | "none")
    {
        return Err(reject(
            StatusCode::FORBIDDEN,
            "cross-site requests are not allowed",
        ));
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let own = host.as_deref().unwrap_or_default();
        let same = origin
            .to_str()
            .ok()
            .is_some_and(|origin| origin_matches(origin, own));
        if !same {
            return Err(reject(
                StatusCode::FORBIDDEN,
                "cross-origin requests are not allowed",
            ));
        }
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        if headers.get(UI_HEADER).and_then(|v| v.to_str().ok()) != Some("1") {
            return Err(reject(
                StatusCode::FORBIDDEN,
                "missing the \"X-Proxy-UI: 1\" header",
            ));
        }
        if !is_json(headers) {
            return Err(reject(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "the Content-Type must be application/json",
            ));
        }
    }
    Ok(())
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

/// Split a `Host` value into a lowercase host (IPv6 keeps its brackets) and a port.
fn split_host(authority: &str) -> Option<(String, Option<u16>)> {
    let authority = authority.trim();
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')?;
        let rest = &authority[end + 1..];
        let port = match rest.strip_prefix(':') {
            Some(port) => Some(port),
            None if rest.is_empty() => None,
            None => return None,
        };
        (&authority[..=end], port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    let port = match port {
        Some(port) => Some(port.parse::<u16>().ok()?),
        None => None,
    };
    (!host.is_empty()).then(|| (host.to_ascii_lowercase(), port))
}

/// Whether `authority` (a `Host` header) is loopback, any port, or one of `addrs` (host
/// and port; an unspecified address like 0.0.0.0 allows nothing extra).
fn host_allowed(authority: &str, addrs: &[Option<SocketAddr>]) -> bool {
    let Some((host, port)) = split_host(authority) else {
        return false;
    };
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]") {
        return true;
    }
    addrs.iter().flatten().any(|addr| {
        let ip = addr.ip();
        let name = match ip {
            std::net::IpAddr::V4(ip) => ip.to_string(),
            std::net::IpAddr::V6(ip) => format!("[{ip}]"),
        };
        !ip.is_unspecified() && host == name && port == Some(addr.port())
    })
}

/// `Origin` is `scheme://host[:port]`; it is ours if that authority is the `Host` header.
fn origin_matches(origin: &str, host: &str) -> bool {
    let authority = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    authority.is_some_and(|authority| !host.is_empty() && authority.eq_ignore_ascii_case(host))
}

fn add_security_headers(headers: &mut HeaderMap) {
    const FIXED: [(&str, &str); 7] = [
        ("content-security-policy", CSP),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-resource-policy", "same-origin"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
    ];
    for (name, value) in FIXED {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(text: &str) -> Option<SocketAddr> {
        Some(text.parse().unwrap())
    }

    #[test]
    fn loopback_hosts_are_allowed_on_any_port() {
        for host in [
            "localhost",
            "localhost:8787",
            "LOCALHOST:1",
            "127.0.0.1:9",
            "127.0.0.1",
            "[::1]:8787",
            "[::1]",
        ] {
            assert!(host_allowed(host, &[]), "{host}");
        }
    }

    #[test]
    fn other_hosts_are_refused() {
        for host in [
            "evil.example",
            "evil.example:8787",
            "localhost.evil.example:8787",
            "127.0.0.1.evil.example",
            "127.0.0.2:8787",
            "0.0.0.0:8787",
            "[::]:8787",
            "",
            "localhost:notaport",
            "[::1",
            "[::1]x",
        ] {
            assert!(!host_allowed(host, &[addr("127.0.0.1:8787")]), "{host:?}");
        }
    }

    #[test]
    fn the_configured_listen_address_is_allowed_with_its_port() {
        let lan = [addr("192.168.1.5:8787"), None];
        assert!(host_allowed("192.168.1.5:8787", &lan));
        assert!(!host_allowed("192.168.1.5:9999", &lan));
        assert!(!host_allowed("192.168.1.5", &lan));
        assert!(!host_allowed("192.168.1.6:8787", &lan));
        // Listening on every interface doesn't make every Host valid.
        assert!(!host_allowed("192.168.1.5:8787", &[addr("0.0.0.0:8787")]));
        assert!(host_allowed("[fd00::5]:80", &[addr("[fd00::5]:80")]));
    }

    #[test]
    fn an_origin_must_be_this_server() {
        assert!(origin_matches("http://127.0.0.1:8787", "127.0.0.1:8787"));
        assert!(origin_matches("http://LocalHost:8787", "localhost:8787"));
        assert!(!origin_matches("http://127.0.0.1:8788", "127.0.0.1:8787"));
        assert!(!origin_matches("http://evil.example", "127.0.0.1:8787"));
        assert!(!origin_matches("null", "127.0.0.1:8787"));
        assert!(!origin_matches("http://127.0.0.1:8787", ""));
    }
}
