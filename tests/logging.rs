//! T8: a key never appears in log output. This file holds a single test because it installs
//! the process-wide log subscriber, at TRACE for every crate (hyper and reqwest included).

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use common::*;
use serde_json::json;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

#[tokio::test]
async fn a_key_never_appears_in_the_log() {
    let capture = Capture::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(capture.clone())
            .finish(),
    )
    .unwrap();

    const CLIENT_KEY: &str = "sk-or-v1-CLIENTSECRET0123456789";
    const ENV_KEY: &str = "sk-or-v1-ENVSECRET0123456789";

    let up = spawn_upstream().await;
    let config = config_text(
        &up,
        "routing = \"price\"\n[pins]\n\"no/endpoints\" = \"x/fp8\"\n",
    );

    // The client's key passes through; the proxy's own key replaces it.
    for (proxy_key, sent) in [(None, CLIENT_KEY), (Some(ENV_KEY), CLIENT_KEY)] {
        let proxy = spawn_proxy(&config, proxy_key).await;
        for body in [
            json!({"model": "a/b", "stream": true}),
            json!({"model": "no/endpoints"}),
            json!({"model": "rate/limited"}),
        ] {
            let response = proxy
                .http
                .post(proxy.url("/v1/chat/completions"))
                .bearer_auth(sent)
                .json(&body)
                .send()
                .await
                .unwrap();
            if body["stream"] == json!(true) {
                up.gate.notify_one();
            }
            let _ = response.bytes().await;
        }
        proxy
            .http
            .get(proxy.url("/v1/models"))
            .bearer_auth(sent)
            .send()
            .await
            .unwrap();
        // A bad request and a failing upstream are logged too.
        proxy
            .http
            .post(proxy.url("/v1/chat/completions"))
            .bearer_auth(sent)
            .body("not json")
            .send()
            .await
            .unwrap();
        // The state's Debug output is safe to log.
        tracing::info!("state: {:?}", proxy.state);
    }
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let proxy = spawn_proxy(
        &format!("upstream = \"http://{dead}/api/v1\"\n"),
        Some(ENV_KEY),
    )
    .await;
    proxy
        .http
        .post(proxy.url("/v1/chat/completions"))
        .bearer_auth(CLIENT_KEY)
        .json(&json!({"model": "a/b"}))
        .send()
        .await
        .unwrap();

    let log = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    // The subscriber really saw the HTTP stack's TRACE output, so the check below is not vacuous.
    assert!(
        log.contains("hyper_util") || log.contains("reqwest"),
        "{log}"
    );
    // The log is not empty: the per-request line is there, with its facts.
    assert!(log.contains("request"), "{log}");
    assert!(log.contains("a/b"), "model is logged");
    assert!(log.contains("sort:price"), "routing is logged");
    assert!(log.contains("pin:x/fp8"), "pin is logged");
    assert!(log.contains("status=429"), "status is logged");
    assert!(log.contains("latency_ms"), "latency is logged");

    for secret in ["CLIENTSECRET", "ENVSECRET", "sk-or-v1"] {
        assert!(
            !log.contains(secret),
            "{secret} leaked into the log:\n{log}"
        );
    }
    assert!(
        !log.to_lowercase().contains("bearer"),
        "an Authorization value leaked into the log:\n{log}"
    );
}
