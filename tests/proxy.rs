mod common;

use std::time::Duration;

use common::*;
use serde_json::{Value, json};

const PIN: &str = "[pins]\n\"a/pinned\" = \"deepinfra/fp8\"\n";

#[tokio::test]
async fn chat_requests_get_the_provider_object() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(
        &config_text(&up, &format!("routing = \"price\"\n{PIN}")),
        None,
    )
    .await;

    // Pinned model: only the pin, no sort. Other fields and their order are untouched.
    let response = proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .header("authorization", "Bearer sk-client")
        .header("x-title", "Koharu")
        .body(
            r#"{"messages":[{"role":"user","content":"hi"}],"model":"a/pinned","temperature":0.7}"#,
        )
        .header("content-type", "application/json")
        .send()
        .await
        .unwrap();
    assert!(ok(response.status()));
    assert_eq!(response.json::<Value>().await.unwrap()["id"], "gen-1");
    let got = up.last();
    assert_eq!(got.uri, "/api/v1/chat/completions");
    assert_eq!(
        std::str::from_utf8(&got.body).unwrap(),
        r#"{"messages":[{"role":"user","content":"hi"}],"model":"a/pinned","temperature":0.7,"provider":{"require_parameters":true,"only":["deepinfra/fp8"],"allow_fallbacks":false}}"#
    );
    // F5: the client's key is passed through; other headers too; no Host/length confusion.
    assert_eq!(got.headers["authorization"], "Bearer sk-client");
    assert_eq!(got.headers["x-title"], "Koharu");

    // Unpinned model: global sort.
    proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "a/other", "messages": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        up.last().json()["provider"],
        json!({"require_parameters": true, "sort": "price"})
    );

    // A client's own provider keys are kept, ours win.
    proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "a/other", "provider": {"ignore": ["x"], "sort": "latency"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        up.last().json()["provider"],
        json!({"ignore": ["x"], "sort": "price", "require_parameters": true})
    );
}

#[tokio::test]
async fn balanced_and_require_parameters_false() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, "require_parameters = false"), None).await;
    proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "a/b"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        up.last().json()["provider"],
        json!({"require_parameters": false})
    );
}

#[tokio::test]
async fn a_body_without_a_model_goes_through_unchanged_and_non_json_is_400() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, "routing = \"price\""), None).await;

    let raw = "{ \"messages\" : [] }";
    proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .body(raw)
        .send()
        .await
        .unwrap();
    assert_eq!(up.last().body.as_ref(), raw.as_bytes());

    let before = up.requests().len();
    let response = proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .body("this is not json")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], 400);
    assert!(body["error"]["message"].as_str().unwrap().contains("JSON"));
    assert_eq!(up.requests().len(), before, "nothing reached the upstream");
}

#[tokio::test]
async fn upstream_errors_reach_the_client_unchanged() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, PIN), None).await;
    let response = proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "rate/limited"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 429);
    assert_eq!(response.headers()["retry-after"], "7");
    assert_eq!(response.headers()["x-ratelimit-reset"], "1234");
    assert_eq!(
        response.text().await.unwrap(),
        r#"{"error":{"message":"slow down","code":429}}"#
    );
    // The proxy does not retry on its own (KOHARU-CONTRACT §2).
    assert_eq!(up.requests().len(), 1);
}

#[tokio::test]
async fn models_and_other_v1_paths_are_passed_through() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, ""), Some("sk-env")).await;

    let response = proxy
        .http
        .get(proxy.url("/v1/models?supported_parameters=tools"))
        .header("authorization", "Bearer ignored")
        .send()
        .await
        .unwrap();
    assert!(ok(response.status()));
    assert_eq!(
        response.json::<Value>().await.unwrap()["data"][0]["id"],
        "deepseek/deepseek-v4.1-flash-1017"
    );
    let got = up.last();
    assert_eq!(got.uri, "/api/v1/models?supported_parameters=tools");
    // F5: OPENROUTER_API_KEY replaces what the client sent.
    assert_eq!(got.headers["authorization"], "Bearer sk-env");

    // Any other /v1 path, any method, body and all.
    let response = proxy
        .http
        .put(proxy.url("/v1/credits/x"))
        .body("payload")
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.json::<Value>().await.unwrap()["path"],
        "/api/v1/credits/x"
    );
    let got = up.last();
    assert_eq!(got.method, "PUT");
    assert_eq!(got.body.as_ref(), b"payload");

    // Outside /v1: 404, not forwarded.
    let before = up.requests().len();
    let response = proxy.http.get(proxy.url("/other")).send().await.unwrap();
    assert_eq!(response.status(), 404);
    assert_eq!(up.requests().len(), before);
}

#[tokio::test]
async fn without_a_key_anywhere_no_authorization_is_sent() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, ""), None).await;
    proxy
        .http
        .get(proxy.url("/v1/models"))
        .send()
        .await
        .unwrap();
    assert!(up.last().headers.get("authorization").is_none());
}

#[tokio::test]
async fn streams_are_relayed_chunk_by_chunk() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, "routing = \"throughput\""), None).await;
    let mut response = proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "a/b", "stream": true}))
        .send()
        .await
        .unwrap();
    assert!(ok(response.status()));
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(
        up.last().json()["provider"],
        json!({"require_parameters": true, "sort": "throughput"})
    );

    // The upstream holds back chunks 2 and 3 until we are told here that we have chunk 1:
    // if the proxy buffered, this would time out.
    let first = tokio::time::timeout(Duration::from_secs(5), response.chunk())
        .await
        .expect("the first chunk arrives before the upstream sends the rest")
        .unwrap()
        .unwrap();
    assert_eq!(first.as_ref(), b"data: {\"n\":1}\n\n");
    up.gate.notify_one();

    let mut rest = Vec::new();
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), response.chunk())
        .await
        .expect("the stream continues")
        .unwrap()
    {
        rest.extend_from_slice(&chunk);
    }
    assert_eq!(
        String::from_utf8(rest).unwrap(),
        "data: {\"n\":2}\n\ndata: [DONE]\n\n"
    );
}

#[tokio::test]
async fn a_pin_that_cannot_serve_the_request_is_explained() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(
        &config_text(&up, "[pins]\n\"no/endpoints\" = \"tenstorrent/fp8\"\n"),
        None,
    )
    .await;

    let response = proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "no/endpoints"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    assert_eq!(response.headers()["content-type"], "application/json");
    let length: usize = response.headers()["content-length"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    let text = response.text().await.unwrap();
    assert_eq!(
        length,
        text.len(),
        "content-length matches the rewritten body"
    );
    let body: Value = serde_json::from_str(&text).unwrap();
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("tenstorrent/fp8"));
    assert!(message.contains("no/endpoints"));
    assert!(message.contains(&proxy.config_path.display().to_string()));
    assert!(message.contains("No endpoints found"));
    assert_eq!(body["error"]["code"], 404);
    assert_eq!(body["error"].as_object().unwrap().len(), 2);

    // Not pinned: the same 404 is left exactly as OpenRouter sent it.
    proxy.write_config(&config_text(&up, ""));
    let response = proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "no/endpoints"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    let original: Value =
        serde_json::from_str(include_str!("../reference/fixtures/no-endpoints-404.json")).unwrap();
    assert_eq!(response.json::<Value>().await.unwrap(), original);
}

#[tokio::test]
async fn editing_the_config_takes_effect_on_the_next_request() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, "routing = \"price\""), None).await;
    let chat = || {
        proxy
            .http
            .post(proxy.url("/v1/chat/completions"))
            .json(&json!({"model": "a/b"}))
            .send()
    };

    chat().await.unwrap();
    assert_eq!(up.last().json()["provider"]["sort"], "price");

    // Pin it: no restart.
    proxy.write_config(&config_text(
        &up,
        "routing = \"price\"\n[pins]\n\"a/b\" = \"novita\"\n",
    ));
    chat().await.unwrap();
    assert_eq!(up.last().json()["provider"]["only"], json!(["novita"]));

    // A bad edit keeps the last good config, and /health says so.
    proxy.write_config("routing = \"fastest\"\n");
    chat().await.unwrap();
    assert_eq!(up.last().json()["provider"]["only"], json!(["novita"]));
    let health: Value = proxy
        .http
        .get(proxy.url("/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(health["config_error"].as_str().unwrap().contains("fastest"));
    assert_eq!(health["pins"], 1);

    // Fixed: back to normal.
    proxy.write_config(&config_text(&up, "routing = \"latency\""));
    chat().await.unwrap();
    assert_eq!(
        up.last().json()["provider"],
        json!({"require_parameters": true, "sort": "latency"})
    );
}

#[tokio::test]
async fn health_reports_routing_pins_and_the_config_path() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(
        &config_text(&up, &format!("routing = \"latency\"\n{PIN}")),
        None,
    )
    .await;
    let response = proxy.http.get(proxy.url("/health")).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let health: Value = response.json().await.unwrap();
    assert_eq!(health["status"], "ok");
    assert_eq!(health["routing"], "latency");
    assert_eq!(health["pins"], 1);
    assert_eq!(
        health["config_path"],
        proxy.config_path.display().to_string()
    );
    assert_eq!(health["config_error"], Value::Null);

    proxy.write_config(&config_text(&up, ""));
    let health: Value = proxy
        .http
        .get(proxy.url("/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["routing"], "balanced");
    assert_eq!(health["pins"], 0);
}

#[tokio::test]
async fn large_bodies_are_not_limited() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, ""), None).await;
    // Well past axum's 2 MB default: a page image as base64.
    let image = "A".repeat(8 * 1024 * 1024);
    let response = proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "a/b", "messages": [{"role": "user", "content": image}]}))
        .send()
        .await
        .unwrap();
    assert!(ok(response.status()));
    let got = up.last();
    assert!(got.body.len() > 8 * 1024 * 1024);
    assert_eq!(got.json()["provider"], json!({"require_parameters": true}));
}

#[tokio::test]
async fn an_unreachable_upstream_is_a_502_in_the_openrouter_shape() {
    // Bind a port and drop the listener: connections to it are refused.
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let proxy = spawn_proxy(&format!("upstream = \"http://{dead}/api/v1\"\n"), None).await;
    let response = proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "a/b"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], 502);
    assert!(body["error"]["message"].is_string());
}

#[tokio::test]
async fn the_hosts_listing_reads_the_endpoints_of_a_model() {
    let up = spawn_upstream().await;
    let hosts = harugate::hosts::hosts(&client(), &up.url(), Some("sk-test"), "z-ai/glm-5.3-flash")
        .await
        .unwrap();
    assert_eq!(
        hosts.iter().map(|h| h.tag.as_str()).collect::<Vec<_>>(),
        ["bare", "deepinfra/fp4", "novita", "tenstorrent/fp8"]
    );
    let got = up.last();
    assert_eq!(got.uri, "/api/v1/models/z-ai/glm-5.3-flash/endpoints");
    assert_eq!(got.headers["authorization"], "Bearer sk-test");
    assert!(
        harugate::hosts::hosts(&client(), &up.url(), None, "no-slash")
            .await
            .is_err()
    );
}
