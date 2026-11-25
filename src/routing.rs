// Portions ported from Koharu (https://github.com/koharu-rs/koharu), Copyright (c) 2025-2026
// Mayo Takanashi and Koharu contributors, used under its MIT or Apache-2.0 license (see README).
//! Which upstream host OpenRouter sends a request to: its `provider` request
//! object, built from the global routing rule and the host pinned for one model.
//!
//! Ported from reference/routing.rs (PR #1534): `Routing`, `provider()` and
//! `explained()`, with its tests, adapted to work on the proxy's config and on the
//! upstream response instead of rig/koharu_config types.

use std::{fmt, path::Path};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::Config;

/// What OpenRouter orders a model's hosts by, as its `provider.sort` names it.
/// Unset, it balances price against uptime.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Routing {
    Price,
    Throughput,
    Latency,
}

impl Routing {
    pub const ALL: [Routing; 3] = [Routing::Price, Routing::Throughput, Routing::Latency];

    pub fn as_str(self) -> &'static str {
        match self {
            Routing::Price => "price",
            Routing::Throughput => "throughput",
            Routing::Latency => "latency",
        }
    }
}

impl fmt::Display for Routing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the proxy did to a request, for the log line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Applied {
    /// Pinned to this endpoint tag.
    Pin(String),
    /// Sorted by this rule.
    Sort(Routing),
    /// `provider` set with no sort (OpenRouter's default balancing).
    Balanced,
    /// The body was not touched (no `model`).
    Untouched,
}

impl fmt::Display for Applied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Applied::Pin(tag) => write!(f, "pin:{tag}"),
            Applied::Sort(routing) => write!(f, "sort:{routing}"),
            Applied::Balanced => f.write_str("balanced"),
            Applied::Untouched => f.write_str("none"),
        }
    }
}

/// The `provider` object of a request: only hosts that support every parameter it
/// sends (if `require_parameters`), and then the pinned host with no fallback, or
/// else hosts in the order `routing` gives. A pinned host leaves nothing to sort.
pub fn provider(routing: Option<Routing>, pin: Option<&str>, require_parameters: bool) -> Value {
    match (pin, routing) {
        (Some(tag), _) => {
            json!({"require_parameters": require_parameters, "only": [tag], "allow_fallbacks": false})
        }
        (None, Some(routing)) => json!({"require_parameters": require_parameters, "sort": routing}),
        (None, None) => json!({"require_parameters": require_parameters}),
    }
}

/// Merge the `provider` object for `body.model` into `body`. Keys the proxy sets
/// override the client's; the client's other `provider` keys and every other body
/// field are kept. A body that isn't a JSON object with a string `model` is left
/// alone.
pub fn inject(body: &mut Value, cfg: &Config) -> Applied {
    let Some(object) = body.as_object_mut() else {
        return Applied::Untouched;
    };
    let Some(model) = object.get("model").and_then(Value::as_str) else {
        return Applied::Untouched;
    };
    let pin = cfg.pins.get(model).map(String::as_str);
    let ours = provider(cfg.routing, pin, cfg.require_parameters);
    let Value::Object(ours) = ours else {
        unreachable!("provider() builds an object")
    };
    match object.get_mut("provider") {
        Some(Value::Object(theirs)) => theirs.extend(ours),
        _ => {
            object.insert("provider".to_owned(), Value::Object(ours));
        }
    }
    match (pin, cfg.routing) {
        (Some(tag), _) => Applied::Pin(tag.to_owned()),
        (None, Some(routing)) => Applied::Sort(routing),
        (None, None) => Applied::Balanced,
    }
}

/// What `inject_bytes` found in a request body.
#[derive(Debug)]
pub struct Injected {
    /// The body to send upstream (the original bytes if nothing was injected).
    pub body: Bytes,
    pub model: Option<String>,
    pub applied: Applied,
}

#[derive(Debug, thiserror::Error)]
#[error("request body is not valid JSON: {0}")]
pub struct NotJson(#[from] serde_json::Error);

/// `inject` on a raw request body. Non-JSON is an error (the proxy answers 400); a
/// body with no `model` is passed through byte for byte.
pub fn inject_bytes(raw: &Bytes, cfg: &Config) -> Result<Injected, NotJson> {
    let mut value: Value = serde_json::from_slice(raw)?;
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let applied = inject(&mut value, cfg);
    let body = if applied == Applied::Untouched {
        raw.clone()
    } else {
        Bytes::from(serde_json::to_vec(&value)?)
    };
    Ok(Injected {
        body,
        model,
        applied,
    })
}

/// OpenRouter's error shape, `{"error":{"message","code"}}`.
pub fn error_body(message: &str, code: u16) -> Value {
    json!({"error": {"message": message, "code": code}})
}

/// If `status`/`body` is the 404 OpenRouter answers when the one host a request is
/// pinned to can't serve it, the body to return instead: same JSON shape, with only
/// `error.message` made clearer. `None` leaves the response as it is.
pub fn explained(
    pin: Option<&str>,
    model: &str,
    config_path: &Path,
    status: u16,
    body: &[u8],
) -> Option<Bytes> {
    let tag = pin?;
    if status != 404 {
        return None;
    }
    let text = std::str::from_utf8(body).ok()?;
    if !text.contains("No endpoints found") {
        return None;
    }
    let mut value: Value = serde_json::from_str(text).ok()?;
    let original = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or(text)
        .to_owned();
    let message = format!(
        "OpenRouter provider `{tag}`, pinned for model `{model}` in {}, can't serve this request (OpenRouter: {original})",
        config_path.display()
    );
    match value.pointer_mut("/error").and_then(Value::as_object_mut) {
        Some(error) => {
            error.insert("message".to_owned(), Value::String(message));
        }
        None => {
            let mut error = Map::new();
            error.insert("message".to_owned(), Value::String(message));
            error.insert("code".to_owned(), Value::from(404));
            value = json!({ "error": error });
        }
    }
    serde_json::to_vec(&value).ok().map(Bytes::from)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use super::*;

    fn cfg(routing: Option<Routing>, pins: &[(&str, &str)]) -> Config {
        Config {
            routing,
            pins: pins
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            ..Config::default()
        }
    }

    #[test]
    fn balanced_routing_sorts_nothing() {
        assert_eq!(
            provider(None, None, true),
            json!({"require_parameters": true})
        );
    }

    #[test]
    fn a_routing_rule_sorts_by_its_name() {
        for (routing, sort) in [
            (Routing::Price, "price"),
            (Routing::Throughput, "throughput"),
            (Routing::Latency, "latency"),
        ] {
            assert_eq!(
                provider(Some(routing), None, true),
                json!({"require_parameters": true, "sort": sort})
            );
        }
    }

    /// A pinned host serves the request or it fails; a sort has nothing left
    /// to order.
    #[test]
    fn a_pinned_host_is_the_only_one_and_is_not_sorted() {
        assert_eq!(
            provider(Some(Routing::Price), Some("deepinfra/fp4"), true),
            json!({
                "require_parameters": true,
                "only": ["deepinfra/fp4"],
                "allow_fallbacks": false,
            })
        );
    }

    #[test]
    fn require_parameters_is_a_parameter() {
        assert_eq!(
            provider(Some(Routing::Latency), None, false),
            json!({"require_parameters": false, "sort": "latency"})
        );
        assert_eq!(
            provider(None, Some("novita"), false),
            json!({"require_parameters": false, "only": ["novita"], "allow_fallbacks": false})
        );
    }

    #[test]
    fn inject_picks_the_pin_by_model() {
        let cfg = cfg(Some(Routing::Price), &[("a/pinned", "deepinfra/fp8")]);

        let mut body = json!({"model": "a/pinned", "messages": [], "temperature": 0.7});
        assert_eq!(
            inject(&mut body, &cfg),
            Applied::Pin("deepinfra/fp8".into())
        );
        assert_eq!(
            body,
            json!({"model": "a/pinned", "messages": [], "temperature": 0.7,
                "provider": {"require_parameters": true, "only": ["deepinfra/fp8"], "allow_fallbacks": false}})
        );

        let mut body = json!({"model": "a/other"});
        assert_eq!(inject(&mut body, &cfg), Applied::Sort(Routing::Price));
        assert_eq!(
            body["provider"],
            json!({"require_parameters": true, "sort": "price"})
        );

        let mut body = json!({"model": "a/other"});
        assert_eq!(inject(&mut body, &Config::default()), Applied::Balanced);
        assert_eq!(body["provider"], json!({"require_parameters": true}));
    }

    #[test]
    fn existing_client_provider_keys_are_kept_and_ours_win() {
        let cfg = cfg(Some(Routing::Latency), &[]);
        let mut body = json!({
            "model": "a/b",
            "provider": {"sort": "price", "ignore": ["x"], "require_parameters": false}
        });
        inject(&mut body, &cfg);
        assert_eq!(
            body["provider"],
            json!({"sort": "latency", "ignore": ["x"], "require_parameters": true})
        );

        // A `provider` that isn't an object is replaced.
        let mut body = json!({"model": "a/b", "provider": "nonsense"});
        inject(&mut body, &cfg);
        assert_eq!(
            body["provider"],
            json!({"require_parameters": true, "sort": "latency"})
        );
    }

    #[test]
    fn a_body_without_a_model_is_unchanged() {
        let cfg = cfg(Some(Routing::Price), &[]);
        for original in [
            json!({"messages": []}),
            json!({"model": 3}),
            json!([1, 2]),
            json!("text"),
        ] {
            let mut body = original.clone();
            assert_eq!(inject(&mut body, &cfg), Applied::Untouched);
            assert_eq!(body, original);
        }
        // Byte for byte, whitespace and all.
        let raw = Bytes::from_static(b"{ \"messages\" :  [ ] }");
        let injected = inject_bytes(&raw, &cfg).unwrap();
        assert_eq!(injected.body, raw);
        assert_eq!(injected.model, None);
    }

    #[test]
    fn non_json_is_an_error() {
        let cfg = Config::default();
        assert!(inject_bytes(&Bytes::from_static(b"not json"), &cfg).is_err());
        assert!(inject_bytes(&Bytes::new(), &cfg).is_err());
    }

    #[test]
    fn inject_bytes_keeps_field_order_and_values() {
        let raw = Bytes::from_static(
            br#"{"zeta":1,"model":"a/b","alpha":[1.5,{"k":null}],"stream":true,"max_tokens":9007199254740993}"#,
        );
        let injected = inject_bytes(&raw, &cfg(None, &[])).unwrap();
        let text = std::str::from_utf8(&injected.body).unwrap();
        assert_eq!(
            text,
            r#"{"zeta":1,"model":"a/b","alpha":[1.5,{"k":null}],"stream":true,"max_tokens":9007199254740993,"provider":{"require_parameters":true}}"#
        );
        assert_eq!(injected.model.as_deref(), Some("a/b"));
    }

    const NO_ENDPOINTS: &str = include_str!("../reference/fixtures/no-endpoints-404.json");
    const MODEL: &str = "z-ai/glm-5.3-flash";

    fn path() -> PathBuf {
        PathBuf::from("cfg.toml")
    }

    #[test]
    fn a_pinned_host_that_cannot_serve_the_request_is_named() {
        let body = explained(
            Some("tenstorrent/fp8"),
            MODEL,
            &path(),
            404,
            NO_ENDPOINTS.as_bytes(),
        )
        .expect("rewritten");
        let value: Value = serde_json::from_slice(&body).unwrap();
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("tenstorrent/fp8"));
        assert!(message.contains(MODEL));
        assert!(message.contains("cfg.toml"));
        // The original message is kept inside the new one.
        assert!(message.contains("No endpoints found that can handle the requested parameters."));
        // Same shape: {"error": {"message", "code"}}.
        assert_eq!(value["error"]["code"], json!(404));
        assert_eq!(value["error"].as_object().unwrap().len(), 2);
        assert_eq!(value.as_object().unwrap().len(), 1);
        assert_ne!(body.as_ref(), NO_ENDPOINTS.as_bytes());
    }

    #[test]
    fn an_error_is_left_as_it_is_unless_a_pin_caused_it() {
        let unchanged = |pin, status, body: &str| {
            assert!(explained(pin, MODEL, &path(), status, body.as_bytes()).is_none());
        };
        // No pin.
        unchanged(None, 404, NO_ENDPOINTS);
        // Pinned, but a different failure.
        unchanged(
            Some("novita"),
            429,
            r#"{"error":{"message":"Too Many Requests","code":429}}"#,
        );
        unchanged(Some("novita"), 429, NO_ENDPOINTS);
        unchanged(
            Some("novita"),
            404,
            r#"{"error":{"message":"Model not found","code":404}}"#,
        );
        // Not JSON.
        unchanged(Some("novita"), 404, "No endpoints found, plain text");
    }
}
