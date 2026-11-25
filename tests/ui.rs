//! The web UI and its JSON API: what they serve, what they refuse, and that no key ever
//! shows up in them. Offline: the upstream is the local mock from `common`.

mod common;

use common::*;
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde_json::{Value, json};

const COMMENTED: &str = "\
# my notes: keep this at the top
listen = \"127.0.0.1:8787\"   # never change this
upstream = \"UPSTREAM\"

# sort order for everything that isn't pinned
routing = \"price\"
require_parameters = true  # strict

[pins]
# the cheap one
\"a/one\" = \"deepinfra/fp8\"  # works
\"b/two\" = \"novita\"
";

fn commented(up: &Upstream) -> String {
    COMMENTED.replace("UPSTREAM", &up.url())
}

/// A mutating API request as the UI sends it.
fn ui_request(proxy: &Proxy, method: Method, path: &str, body: &Value) -> RequestBuilder {
    proxy
        .http
        .request(method, proxy.url(path))
        .header("content-type", "application/json")
        .header("x-proxy-ui", "1")
        .body(body.to_string())
}

async fn error_message(response: Response) -> String {
    let body: Value = response.json().await.unwrap();
    body["error"]["message"]
        .as_str()
        .expect("an error message")
        .to_owned()
}

fn full_config(up: &Upstream, extra: Value) -> Value {
    let mut config = json!({
        "listen": "127.0.0.1:8787",
        "upstream": up.url(),
        "routing": "price",
        "require_parameters": true,
        "pins": {"a/one": "deepinfra/fp8", "b/two": "novita"},
    });
    for (key, value) in extra.as_object().unwrap() {
        config[key] = value.clone();
    }
    config
}

#[tokio::test]
async fn the_ui_is_served_with_a_strict_policy() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, ""), None).await;

    let response = proxy.http.get(proxy.url("/ui")).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let csp = headers["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("default-src 'self'"), "{csp}");
    assert!(
        csp.contains("script-src 'self'") && !csp.contains("unsafe-inline"),
        "{csp}"
    );
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["x-frame-options"], "DENY");
    assert_eq!(headers["referrer-policy"], "no-referrer");
    assert!(headers.get("access-control-allow-origin").is_none());
    let html = response.text().await.unwrap();
    assert!(html.to_lowercase().contains("<!doctype html"));
    // No inline script or handler (the policy would block them), and no third party.
    assert!(html.contains("/ui/app.js"));
    // (The page may *mention* https://openrouter.ai as text; it loads nothing from anywhere.)
    for forbidden in [
        "onclick=",
        "onchange=",
        "onsubmit=",
        " style=",
        "src=\"http",
        "href=\"http",
        "src=\"//",
        "<style",
        "@import",
    ] {
        assert!(!html.contains(forbidden), "index.html has {forbidden}");
    }
    for inline in html.split("<script").skip(1) {
        assert!(
            inline.starts_with(" src=") || inline.starts_with(" defer src="),
            "inline script: {inline:.60}"
        );
    }

    for (path, kind) in [
        ("/ui/app.js", "javascript"),
        ("/ui/app.css", "css"),
        ("/ui/favicon.svg", "svg"),
    ] {
        let response = proxy.http.get(proxy.url(path)).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .contains(kind)
        );
        assert!(response.headers().contains_key("content-security-policy"));
    }

    // / goes to the UI.
    let no_redirect = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = no_redirect.get(proxy.url("/")).send().await.unwrap();
    assert!(response.status().is_redirection());
    assert_eq!(response.headers()["location"], "/ui");

    // The embedded script and page hold no key field.
    let js = proxy
        .http
        .get(proxy.url("/ui/app.js"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!js.to_lowercase().contains("type=\"password\""));
}

#[tokio::test]
async fn a_foreign_host_header_is_refused_for_the_ui_and_api_only() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, ""), None).await;
    let port = proxy.addr.port();

    // DNS rebinding: the browser still thinks it talks to evil.example.
    for path in [
        "/ui",
        "/ui/app.js",
        "/api/status",
        "/api/config",
        "/api/requests",
        "/",
    ] {
        for host in [
            "evil.example".to_owned(),
            format!("evil.example:{port}"),
            format!("localhost.evil.example:{port}"),
            "0.0.0.0:8787".to_owned(),
        ] {
            let response = proxy
                .http
                .get(proxy.url(path))
                .header("host", host.as_str())
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "{path} with Host {host}"
            );
            assert!(response.headers().contains_key("content-security-policy"));
            assert!(error_message(response).await.contains("not allowed"));
        }
    }

    // The ways to say "this machine" all work, on any port.
    for host in [
        format!("localhost:{port}"),
        format!("127.0.0.1:{port}"),
        format!("[::1]:{port}"),
        "localhost".to_owned(),
    ] {
        let response = proxy
            .http
            .get(proxy.url("/api/status"))
            .header("host", host.as_str())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "Host {host}");
    }

    // The proxy proper behaves as before, whatever the Host header says.
    for path in ["/health", "/v1/models"] {
        let response = proxy
            .http
            .get(proxy.url(path))
            .header("host", "koharu.internal")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn cross_origin_and_header_less_writes_are_refused() {
    let up = spawn_upstream().await;
    let text = commented(&up);
    let proxy = spawn_proxy(&text, None).await;
    let body = full_config(&up, json!({"routing": "latency"}));
    let port = proxy.addr.port();

    let attempts: Vec<(&str, RequestBuilder, StatusCode)> = vec![
        (
            "a page on another site",
            ui_request(&proxy, Method::PUT, "/api/config", &body)
                .header("origin", "http://evil.example"),
            StatusCode::FORBIDDEN,
        ),
        (
            "another port on localhost",
            ui_request(&proxy, Method::PUT, "/api/config", &body)
                .header("origin", format!("http://localhost:{}", port + 1)),
            StatusCode::FORBIDDEN,
        ),
        (
            "a sandboxed frame (Origin: null)",
            ui_request(&proxy, Method::PUT, "/api/config", &body).header("origin", "null"),
            StatusCode::FORBIDDEN,
        ),
        (
            "a cross-site fetch",
            ui_request(&proxy, Method::PUT, "/api/config", &body)
                .header("sec-fetch-site", "cross-site"),
            StatusCode::FORBIDDEN,
        ),
        (
            "no X-Proxy-UI header",
            proxy
                .http
                .put(proxy.url("/api/config"))
                .header("content-type", "application/json")
                .body(body.to_string()),
            StatusCode::FORBIDDEN,
        ),
        (
            "a wrong X-Proxy-UI value",
            proxy
                .http
                .put(proxy.url("/api/config"))
                .header("content-type", "application/json")
                .header("x-proxy-ui", "0")
                .body(body.to_string()),
            StatusCode::FORBIDDEN,
        ),
        (
            "a form post (text/plain)",
            proxy
                .http
                .put(proxy.url("/api/config"))
                .header("x-proxy-ui", "1")
                .header("content-type", "text/plain")
                .body(body.to_string()),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            "no content type",
            proxy
                .http
                .put(proxy.url("/api/config/raw"))
                .header("x-proxy-ui", "1")
                .body(json!({"text": "routing = \"latency\""}).to_string()),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            "a POST",
            ui_request(&proxy, Method::POST, "/api/config", &body)
                .header("origin", "http://evil.example"),
            StatusCode::FORBIDDEN,
        ),
    ];
    for (what, request, expected) in attempts {
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), expected, "{what}");
        assert!(
            !response
                .headers()
                .keys()
                .any(|name| name.as_str().starts_with("access-control-")),
            "{what}: no CORS headers"
        );
    }
    assert_eq!(
        std::fs::read_to_string(&proxy.config_path).unwrap(),
        text,
        "nothing was written"
    );

    // A cross-origin read is refused too (the page could not read it, but don't answer).
    let response = proxy
        .http
        .get(proxy.url("/api/config"))
        .header("origin", "http://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // A CORS preflight gets no permission.
    let response = proxy
        .http
        .request(Method::OPTIONS, proxy.url("/api/config"))
        .header("origin", "http://evil.example")
        .header("access-control-request-method", "PUT")
        .header("access-control-request-headers", "x-proxy-ui,content-type")
        .send()
        .await
        .unwrap();
    assert!(!response.status().is_success());
    assert!(
        !response
            .headers()
            .contains_key("access-control-allow-origin")
    );

    // The same request from the UI itself (same origin, with the header) works.
    let response = ui_request(&proxy, Method::PUT, "/api/config", &body)
        .header("origin", format!("http://127.0.0.1:{port}"))
        .header("sec-fetch-site", "same-origin")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn saving_the_config_keeps_comments_and_key_order() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&commented(&up), None).await;

    let body: Value = proxy
        .http
        .get(proxy.url("/api/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["config"]["routing"], "price");
    assert_eq!(body["config"]["pins"]["a/one"], "deepinfra/fp8");
    assert_eq!(body["config_error"], Value::Null);

    // Change the sort, change one pin, drop one, add one, flip the toggle.
    let update = full_config(
        &up,
        json!({
            "routing": "throughput",
            "require_parameters": false,
            "pins": {"a/one": "novita", "c/three": "x/fp4"},
        }),
    );
    let response = ui_request(&proxy, Method::PUT, "/api/config", &update)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let saved: Value = response.json().await.unwrap();
    assert_eq!(saved["saved"], true);
    assert_eq!(saved["config"]["routing"], "throughput");
    // (The test server is bound to a random port, not the file's 8787.)
    assert_eq!(saved["restart_required"], true);

    let file = std::fs::read_to_string(&proxy.config_path).unwrap();
    for kept in [
        "# my notes: keep this at the top",
        "# never change this",
        "# sort order for everything that isn't pinned",
        "# strict",
        "# the cheap one",
        "# works",
    ] {
        assert!(file.contains(kept), "lost {kept:?}:\n{file}");
    }
    assert!(file.contains("routing = \"throughput\""), "{file}");
    assert!(file.contains("require_parameters = false"), "{file}");
    assert!(file.contains("\"a/one\" = \"novita\"  # works"), "{file}");
    assert!(!file.contains("b/two"), "{file}");
    assert!(file.contains("\"c/three\" = \"x/fp4\""), "{file}");
    let at = |needle: &str| {
        file.find(needle)
            .unwrap_or_else(|| panic!("{needle} missing:\n{file}"))
    };
    assert!(at("listen") < at("upstream") && at("upstream") < at("routing"));
    assert!(at("routing") < at("require_parameters") && at("require_parameters") < at("[pins]"));
    assert!(at("a/one") < at("c/three"));

    // The proxy uses it now (hot reload), including for the next request.
    let reloaded: Value = proxy
        .http
        .get(proxy.url("/api/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        reloaded["config"]["pins"],
        json!({"a/one": "novita", "c/three": "x/fp4"})
    );
    proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .json(&json!({"model": "c/three"}))
        .send()
        .await
        .unwrap();
    assert_eq!(up.last().json()["provider"]["only"], json!(["x/fp4"]));

    // Back to Balanced: the routing key goes, everything else stays.
    let update = full_config(&up, json!({"routing": null, "pins": {}}));
    let response = ui_request(&proxy, Method::PUT, "/api/config", &update)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let file = std::fs::read_to_string(&proxy.config_path).unwrap();
    assert!(!file.contains("routing ="), "{file}");
    assert!(
        file.contains("# my notes: keep this at the top") && file.contains("# never change this")
    );
    assert!(file.contains("[pins]"), "{file}");
    assert!(
        !file.contains("c/three") && !file.contains("a/one"),
        "{file}"
    );
    assert_eq!(proxy.state.config.snapshot().routing, None);
}

#[tokio::test]
async fn invalid_saves_are_refused_and_leave_the_file_alone() {
    let up = spawn_upstream().await;
    let text = commented(&up);
    let proxy = spawn_proxy(&text, None).await;

    let bad: Vec<(&str, Value, &str)> = vec![
        (
            "an unknown routing",
            full_config(&up, json!({"routing": "fastest"})),
            "fastest",
        ),
        (
            "a bad upstream",
            full_config(&up, json!({"upstream": "ftp://x"})),
            "upstream",
        ),
        (
            "a bad listen",
            full_config(&up, json!({"listen": "everywhere"})),
            "listen",
        ),
        (
            "an empty tag",
            full_config(&up, json!({"pins": {"a/b": " "}})),
            "empty endpoint tag",
        ),
        (
            "an unknown field",
            full_config(&up, json!({"typo": 1})),
            "typo",
        ),
        (
            "a wrong type",
            full_config(&up, json!({"require_parameters": "yes"})),
            "invalid",
        ),
    ];
    for (what, body, expect) in bad {
        let response = ui_request(&proxy, Method::PUT, "/api/config", &body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{what}");
        let message = error_message(response).await;
        assert!(message.contains(expect), "{what}: {message}");
        assert_eq!(
            std::fs::read_to_string(&proxy.config_path).unwrap(),
            text,
            "{what}: file changed"
        );
    }
    // Not JSON at all.
    let response = proxy
        .http
        .put(proxy.url("/api/config"))
        .header("content-type", "application/json")
        .header("x-proxy-ui", "1")
        .body("routing = price")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.json::<Value>().await.unwrap()["error"]["message"].is_string());
    assert_eq!(std::fs::read_to_string(&proxy.config_path).unwrap(), text);
}

#[tokio::test]
async fn risky_settings_need_an_explicit_confirmation() {
    let up = spawn_upstream().await;
    let text = commented(&up);
    let proxy = spawn_proxy(&text, None).await;

    for (what, change) in [
        (
            "an upstream that isn't OpenRouter",
            json!({"upstream": "https://openrouter.ai.evil.example/api/v1"}),
        ),
        (
            "a plain http upstream",
            json!({"upstream": "http://openrouter.ai/api/v1"}),
        ),
        (
            "a listen address on the network",
            json!({"listen": "0.0.0.0:8787"}),
        ),
    ] {
        let body = full_config(&up, change.clone());
        let response = ui_request(&proxy, Method::PUT, "/api/config", &body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{what}");
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["error"]["code"], "confirmation_required", "{what}");
        assert_eq!(
            std::fs::read_to_string(&proxy.config_path).unwrap(),
            text,
            "{what}"
        );
    }

    // The same through the raw editor.
    let raw = text.replace("127.0.0.1:8787", "192.168.1.5:8787");
    let response = ui_request(
        &proxy,
        Method::PUT,
        "/api/config/raw",
        &json!({"text": raw}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "confirmation_required"
    );
    assert_eq!(std::fs::read_to_string(&proxy.config_path).unwrap(), text);

    // Confirmed: it is saved, and a listen change says it needs a restart.
    let body = full_config(
        &up,
        json!({"listen": "0.0.0.0:8787", "confirm_risky": true}),
    );
    let response = ui_request(&proxy, Method::PUT, "/api/config", &body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<Value>().await.unwrap()["restart_required"],
        true
    );
    assert!(
        std::fs::read_to_string(&proxy.config_path)
            .unwrap()
            .contains("0.0.0.0:8787")
    );

    // Once it is the saved value, saving other things asks nothing.
    let body = full_config(&up, json!({"listen": "0.0.0.0:8787", "routing": "latency"}));
    let response = ui_request(&proxy, Method::PUT, "/api/config", &body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let status: Value = proxy
        .http
        .get(proxy.url("/api/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["listen_is_loopback"], false);
    assert_eq!(status["upstream_is_official"], false);
    assert_eq!(status["restart_required"], true);
}

#[tokio::test]
async fn the_raw_editor_saves_text_as_written_and_shows_errors() {
    let up = spawn_upstream().await;
    let text = commented(&up);
    let proxy = spawn_proxy(&text, None).await;

    let raw: Value = proxy
        .http
        .get(proxy.url("/api/config/raw"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(raw["text"], text.as_str());
    assert!(raw["path"].as_str().unwrap().ends_with("config.toml"));

    let broken = text.replace("\"price\"", "\"fastest\"");
    let response = ui_request(
        &proxy,
        Method::PUT,
        "/api/config/raw",
        &json!({"text": broken}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let message = error_message(response).await;
    assert!(
        message.contains("fastest") && message.contains("price"),
        "{message}"
    );
    assert!(message.contains("line"), "the parser says where: {message}");
    assert_eq!(std::fs::read_to_string(&proxy.config_path).unwrap(), text);

    let edited = format!("# edited in the UI\n{text}");
    let response = ui_request(
        &proxy,
        Method::PUT,
        "/api/config/raw",
        &json!({"text": edited}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(std::fs::read_to_string(&proxy.config_path).unwrap(), edited);

    // A file that is broken on disk is reported, and the raw editor can fix it.
    proxy.write_config("routing = ");
    let raw: Value = proxy
        .http
        .get(proxy.url("/api/config/raw"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(raw["text"], "routing = ");
    assert!(raw["config_error"].is_string());
    let status: Value = proxy
        .http
        .get(proxy.url("/api/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(status["config_error"].is_string());
    // A structured save refuses to rewrite a file it can't read, and says why.
    let response = ui_request(
        &proxy,
        Method::PUT,
        "/api/config",
        &full_config(&up, json!({})),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(error_message(response).await.contains("raw editor"));
    assert_eq!(
        std::fs::read_to_string(&proxy.config_path).unwrap(),
        "routing = "
    );
    let fixed = format!("upstream = \"{}\"\n", up.url());
    let response = ui_request(
        &proxy,
        Method::PUT,
        "/api/config/raw",
        &json!({"text": fixed}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: Value = proxy
        .http
        .get(proxy.url("/api/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["config_error"], Value::Null);
}

#[tokio::test]
async fn hosts_and_models_come_from_the_upstream() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(
        &config_text(
            &up,
            "[pins]\n\"deepseek/deepseek-v4.1-flash-1017\" = \"novita\"\n",
        ),
        None,
    )
    .await;

    let response = proxy
        .http
        .get(proxy.url("/api/hosts?model=deepseek%2Fdeepseek-v4.1-flash-1017"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["model"], "deepseek/deepseek-v4.1-flash-1017");
    assert_eq!(body["pinned"], "novita");
    let hosts = body["hosts"].as_array().unwrap();
    let labels: Vec<&str> = hosts.iter().map(|h| h["label"].as_str().unwrap()).collect();
    assert_eq!(
        labels,
        [
            "Bare (fp8)",
            "DeepInfra (fp4)",
            "Novita",
            "Tenstorrent (fp8)"
        ],
        "sorted by label"
    );
    let deepinfra = &hosts[1];
    assert_eq!(deepinfra["tag"], "deepinfra/fp4");
    assert_eq!(deepinfra["quantization"], "fp4");
    assert_eq!(deepinfra["structured_outputs"], true);
    assert_eq!(deepinfra["tools"], false);
    assert_eq!(
        hosts[2]["quantization"],
        Value::Null,
        "unknown quantization is none"
    );
    assert_eq!(
        up.last().uri,
        "/api/v1/models/deepseek/deepseek-v4.1-flash-1017/endpoints"
    );

    for bad in [
        "/api/hosts",
        "/api/hosts?model=",
        "/api/hosts?model=nope",
        "/api/hosts?model=a%2Fb%2Fc",
    ] {
        let response = proxy.http.get(proxy.url(bad)).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{bad}");
        assert!(error_message(response).await.len() > 5);
    }

    // The model list: ids only, fetched once and then cached.
    let count = |up: &Upstream| {
        up.requests()
            .iter()
            .filter(|r| r.uri == "/api/v1/models")
            .count()
    };
    for _ in 0..2 {
        let body: Value = proxy
            .http
            .get(proxy.url("/api/models"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(body["models"], json!(["deepseek/deepseek-v4.1-flash-1017"]));
    }
    assert_eq!(count(&up), 1, "the second call came from the cache");
}

#[tokio::test]
async fn an_unreachable_upstream_is_a_502_not_a_hang() {
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let proxy = spawn_proxy(&format!("upstream = \"http://{dead}/api/v1\"\n"), None).await;
    for path in ["/api/models", "/api/hosts?model=a%2Fb"] {
        let response = proxy.http.get(proxy.url(path)).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{path}");
        assert!(error_message(response).await.contains("can't reach"));
    }
    let response = proxy.http.get(proxy.url("/api/nope")).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn recent_requests_are_recorded_without_keys() {
    const CLIENT_KEY: &str = "sk-or-v1-CLIENTSECRET0123456789";
    const ENV_KEY: &str = "sk-or-v1-ENVSECRET0123456789";
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(
        &config_text(
            &up,
            "routing = \"latency\"\n[pins]\n\"a/pinned\" = \"x/fp8\"\n\"no/endpoints\" = \"y\"\n",
        ),
        Some(ENV_KEY),
    )
    .await;

    let empty: Value = proxy
        .http
        .get(proxy.url("/api/requests"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(empty["requests"], json!([]));
    assert_eq!(empty["capacity"], 100);

    for model in ["a/pinned", "a/other", "rate/limited", "no/endpoints"] {
        proxy
            .http
            .post(proxy.url("/v1/chat/completions"))
            .bearer_auth(CLIENT_KEY)
            .header("x-secret-header", "SECRETHEADER")
            .json(
                &json!({"model": model, "messages": [{"role": "user", "content": "SECRETPROMPT"}]}),
            )
            .send()
            .await
            .unwrap();
    }
    proxy
        .http
        .get(proxy.url("/v1/models?token=SECRETQUERY"))
        .bearer_auth(CLIENT_KEY)
        .send()
        .await
        .unwrap();
    proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .body("not json")
        .send()
        .await
        .unwrap();

    let text = proxy
        .http
        .get(proxy.url("/api/requests"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let body: Value = serde_json::from_str(&text).unwrap();
    let list = body["requests"].as_array().unwrap();
    assert_eq!(list.len(), 6);
    let view: Vec<(String, String, u64)> = list
        .iter()
        .rev()
        .map(|r| {
            (
                r["model"].as_str().unwrap_or("-").to_owned(),
                r["routing"].as_str().unwrap().to_owned(),
                r["status"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        view,
        [
            ("a/pinned".into(), "pin:x/fp8".into(), 200),
            ("a/other".into(), "sort:latency".into(), 200),
            ("rate/limited".into(), "sort:latency".into(), 429),
            ("no/endpoints".into(), "pin:y".into(), 404),
            ("-".into(), "none".into(), 200),
            ("-".into(), "none".into(), 400),
        ]
    );
    let newest = &list[0];
    assert_eq!(newest["method"], "POST");
    assert_eq!(newest["path"], "/v1/chat/completions");
    assert!(
        newest["latency_ms"].is_u64() && newest["time_ms"].as_u64().unwrap() > 1_600_000_000_000
    );
    assert_eq!(list[1]["path"], "/v1/models", "no query string is kept");
    for secret in [
        "CLIENTSECRET",
        "ENVSECRET",
        "sk-or",
        "SECRETPROMPT",
        "SECRETHEADER",
        "SECRETQUERY",
        "earer",
    ] {
        assert!(!text.contains(secret), "{secret} leaked:\n{text}");
    }
}

#[tokio::test]
async fn the_ring_holds_the_last_hundred() {
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(&config_text(&up, ""), None).await;
    for _ in 0..105 {
        proxy.state.activity.record(
            "GET",
            "/v1/models",
            None,
            &harugate::routing::Applied::Untouched,
            200,
            1,
        );
    }
    let body: Value = proxy
        .http
        .get(proxy.url("/api/requests"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let list = body["requests"].as_array().unwrap();
    assert_eq!(list.len(), 100);
    assert_eq!(list[0]["id"], 105);
    assert_eq!(list[99]["id"], 6);
}

#[tokio::test]
async fn no_api_answer_contains_a_key() {
    const ENV_KEY: &str = "sk-or-v1-ENVSECRET0123456789";
    let up = spawn_upstream().await;
    let proxy = spawn_proxy(
        &config_text(&up, "[pins]\n\"a/b\" = \"x\"\n"),
        Some(ENV_KEY),
    )
    .await;
    proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .bearer_auth("sk-or-v1-CLIENTSECRET0123456789")
        .json(&json!({"model": "a/b"}))
        .send()
        .await
        .unwrap();

    let status: Value = proxy
        .http
        .get(proxy.url("/api/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["api_key_set"], true, "a yes/no, never the key");
    assert_eq!(status["pins"], 1);
    assert_eq!(status["routing"], Value::Null);
    assert_eq!(status["upstream"], up.url().as_str());
    assert_eq!(
        status["base_url"],
        format!("http://{}/v1", proxy.addr).as_str()
    );
    assert!(
        status["config_path"]
            .as_str()
            .unwrap()
            .ends_with("config.toml")
    );

    for path in [
        "/api/status",
        "/api/config",
        "/api/config/raw",
        "/api/requests",
        "/api/models",
        "/api/hosts?model=a%2Fb",
        "/ui",
        "/ui/app.js",
        "/ui/app.css",
    ] {
        let response = proxy.http.get(proxy.url(path)).send().await.unwrap();
        let text = response.text().await.unwrap();
        for secret in ["ENVSECRET", "CLIENTSECRET", "sk-or-v1"] {
            assert!(!text.contains(secret), "{path} contains {secret}");
        }
    }
    // The upstream got the proxy's own key for the lists the UI asked for, but nobody got it back.
    assert!(up.requests().iter().any(|r| {
        r.uri.ends_with("/models")
            && r.headers["authorization"]
                .to_str()
                .unwrap()
                .contains("ENVSECRET")
    }));

    let without = spawn_proxy(&config_text(&up, ""), None).await;
    let status: Value = without
        .http
        .get(without.url("/api/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["api_key_set"], false);
}
