//! Test helpers: a mock OpenRouter on a random port, and the proxy in front of it.
#![allow(dead_code)]

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::Response,
};
use bytes::Bytes;
use openrouter_proxy::{config::ConfigStore, server, state::AppState};
use serde_json::{Value, json};
use tokio::sync::Notify;

#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: Method,
    pub uri: String,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Recorded {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("JSON body")
    }
}

#[derive(Clone)]
pub struct Upstream {
    pub addr: SocketAddr,
    pub recorded: Arc<Mutex<Vec<Recorded>>>,
    /// The SSE reply sends its first chunk at once and the rest after this is notified.
    pub gate: Arc<Notify>,
}

impl Upstream {
    pub fn url(&self) -> String {
        format!("http://{}/api/v1", self.addr)
    }
    pub fn requests(&self) -> Vec<Recorded> {
        self.recorded.lock().unwrap().clone()
    }
    pub fn last(&self) -> Recorded {
        self.requests().pop().expect("the upstream got a request")
    }
}

/// The mock: records every request, then answers by path and, for chat, by `model`.
pub async fn spawn_upstream() -> Upstream {
    let upstream = Upstream {
        addr: ([127, 0, 0, 1], 0).into(),
        recorded: Default::default(),
        gate: Default::default(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = Upstream {
        addr: listener.local_addr().unwrap(),
        ..upstream
    };
    let router = Router::new().fallback(mock).with_state(upstream.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    upstream
}

async fn mock(State(up): State<Upstream>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    up.recorded.lock().unwrap().push(Recorded {
        method: parts.method.clone(),
        uri: parts.uri.to_string(),
        headers: parts.headers.clone(),
        body: body.clone(),
    });
    let path = parts.uri.path();
    let json_response = |status: u16, value: Value| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap()
    };
    match (parts.method.as_str(), path) {
        ("POST", "/api/v1/chat/completions") => {
            let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            match request["model"].as_str() {
                Some("rate/limited") => Response::builder()
                    .status(429)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("retry-after", "7")
                    .header("x-ratelimit-reset", "1234")
                    .body(Body::from(
                        r#"{"error":{"message":"slow down","code":429}}"#,
                    ))
                    .unwrap(),
                Some("no/endpoints") => json_response(
                    404,
                    serde_json::from_str(include_str!(
                        "../../reference/fixtures/no-endpoints-404.json"
                    ))
                    .unwrap(),
                ),
                _ if request["stream"] == json!(true) => {
                    let gate = up.gate.clone();
                    let stream = async_stream(gate);
                    Response::builder()
                        .status(200)
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .header(header::CACHE_CONTROL, "no-cache")
                        .body(Body::from_stream(stream))
                        .unwrap()
                }
                _ => json_response(
                    200,
                    json!({"id": "gen-1", "choices": [{"message": {"role": "assistant", "content": "hi"}}]}),
                ),
            }
        }
        ("GET", "/api/v1/models") => json_response(
            200,
            json!({"data": [{"id": "deepseek/deepseek-v4.1-flash-1017"}]}),
        ),
        ("GET", p) if p.ends_with("/endpoints") => json_response(
            200,
            serde_json::from_str(include_str!(
                "../../reference/fixtures/endpoints-sample.json"
            ))
            .unwrap(),
        ),
        _ => json_response(200, json!({"path": path})),
    }
}

/// Three SSE chunks; the second and third wait for the gate.
fn async_stream(
    gate: Arc<Notify>,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> {
    futures_util::stream::unfold(0, move |step| {
        let gate = gate.clone();
        async move {
            let chunk = match step {
                0 => "data: {\"n\":1}\n\n",
                1 => {
                    gate.notified().await;
                    "data: {\"n\":2}\n\n"
                }
                2 => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    "data: [DONE]\n\n"
                }
                _ => return None,
            };
            Some((Ok(Bytes::from(chunk)), step + 1))
        }
    })
}

pub struct Proxy {
    pub addr: SocketAddr,
    pub dir: tempfile::TempDir,
    pub config_path: PathBuf,
    pub state: AppState,
    pub http: reqwest::Client,
}

impl Proxy {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
    pub fn write_config(&self, text: &str) {
        std::fs::write(&self.config_path, text).unwrap();
    }
}

/// A config pointing at `upstream`; `extra` is appended (routing, [pins]...).
pub fn config_text(upstream: &Upstream, extra: &str) -> String {
    format!("upstream = \"{}\"\n{extra}\n", upstream.url())
}

pub fn client() -> reqwest::Client {
    openrouter_proxy::state::install_crypto_provider();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

pub async fn spawn_proxy(config: &str, api_key: Option<&str>) -> Proxy {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, config).unwrap();
    let store = Arc::new(ConfigStore::open(&config_path).unwrap());
    let state = AppState::new(store, client(), api_key).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    state.set_bound_addr(addr);
    let router = server::app(state.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Proxy {
        addr,
        dir,
        config_path,
        state,
        http: client(),
    }
}

pub fn ok(status: reqwest::StatusCode) -> bool {
    status == StatusCode::OK
}
