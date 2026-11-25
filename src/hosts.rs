//! The hosts OpenRouter lists for a model (`openrouter-proxy hosts <model>`).
//!
//! Ported from `hosts()` / `hosts_in()` in reference/routing.rs and `label()` in
//! reference/host-label.ts. Unlike the original, endpoints that don't list
//! `structured_outputs` are kept (Koharu doesn't send a JSON schema through
//! OpenAI-compatible, KOHARU-CONTRACT §3); `tools` and `structured_outputs` are
//! recorded as flags instead.

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;

/// A host OpenRouter lists for a model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Host {
    /// What `provider.only` takes: the host and its quantization, as OpenRouter tags
    /// the endpoint.
    pub tag: String,
    pub name: String,
    pub quantization: Option<String>,
    /// Lists `tools` among its supported parameters.
    pub tools: bool,
    /// Lists `structured_outputs` among its supported parameters.
    pub structured_outputs: bool,
}

impl Host {
    /// What tells a host's endpoints apart: its quantization and the region or tier the
    /// rest of its tag names, as `Sail Research (fp4, us)` for `sail-research/fp4/us`.
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for part in self
            .quantization
            .as_deref()
            .into_iter()
            .chain(self.tag.split('/').skip(1))
        {
            if !part.is_empty() && !parts.contains(&part) {
                parts.push(part);
            }
        }
        if parts.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, parts.join(", "))
        }
    }
}

/// `hosts` sorted by label, as the PR's dropdown showed them (case-insensitive, like
/// `localeCompare` for plain text).
pub fn sorted(mut hosts: Vec<Host>) -> Vec<Host> {
    hosts.sort_by_cached_key(|host| {
        let label = host.label();
        (label.to_lowercase(), label)
    });
    hosts
}

/// The endpoints-list URL for model `id`, `author/slug`, under `upstream`.
pub fn endpoints_url(upstream: &str, id: &str) -> Result<reqwest::Url> {
    let (author, slug) = id
        .split_once('/')
        .filter(|(author, slug)| !author.is_empty() && !slug.is_empty() && !slug.contains('/'))
        .with_context(|| format!("`{id}` is no OpenRouter model: expected `author/slug`"))?;
    let mut url = reqwest::Url::parse(upstream)
        .with_context(|| format!("upstream `{upstream}` is not a URL"))?;
    url.path_segments_mut()
        .ok()
        .context("the upstream URL can't take a path")?
        .pop_if_empty()
        .extend(["models", author, slug, "endpoints"]);
    Ok(url)
}

/// The hosts OpenRouter lists for model `id`, sorted by label. The endpoints list
/// needs no key, though it takes one.
pub async fn hosts(
    client: &reqwest::Client,
    upstream: &str,
    key: Option<&str>,
    id: &str,
) -> Result<Vec<Host>> {
    let url = endpoints_url(upstream, id)?;
    let mut request = client.get(url).timeout(std::time::Duration::from_secs(30));
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.context("can't reach the host list")?;
    let status = response.status();
    ensure!(status.is_success(), "the host list answered {status}");
    let reply: Value = response
        .json()
        .await
        .context("the host list is not valid JSON")?;
    Ok(sorted(hosts_in(&reply)))
}

/// The hosts `reply`, an endpoints list, names, once each, in the list's order. A row
/// with no tag is no host a request can pin. Unknown quantization means none.
pub fn hosts_in(reply: &Value) -> Vec<Host> {
    let mut hosts: Vec<Host> = Vec::new();
    for row in reply
        .pointer("/data/endpoints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let text = |key| {
            row.get(key)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
        };
        let Some(tag) = text("tag") else { continue };
        if hosts.iter().any(|host| host.tag == tag) {
            continue;
        }
        let supports = |name: &str| {
            row.get("supported_parameters")
                .and_then(Value::as_array)
                .is_some_and(|parameters| {
                    parameters
                        .iter()
                        .any(|parameter| parameter.as_str() == Some(name))
                })
        };
        hosts.push(Host {
            tag: tag.to_owned(),
            name: text("provider_name").unwrap_or(tag).to_owned(),
            quantization: text("quantization")
                .filter(|quantization| *quantization != "unknown")
                .map(str::to_owned),
            tools: supports("tools"),
            structured_outputs: supports("structured_outputs"),
        });
    }
    hosts
}

/// One line per host: tag, label, quantization, flags.
pub fn render(hosts: &[Host]) -> String {
    let rows: Vec<[String; 5]> = hosts
        .iter()
        .map(|host| {
            let yes = |flag| if flag { "yes" } else { "no" };
            [
                host.tag.clone(),
                host.label(),
                host.quantization.clone().unwrap_or_else(|| "-".to_owned()),
                format!("tools={}", yes(host.tools)),
                format!("structured_outputs={}", yes(host.structured_outputs)),
            ]
        })
        .collect();
    let widths: Vec<usize> = (0..3)
        .map(|i| {
            rows.iter()
                .map(|row| row[i].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i < 3 {
                out.push_str(&format!("{cell:<w$}  ", w = widths[i]));
            } else if i == 3 {
                out.push_str(&format!("{cell:<9}  "));
            } else {
                out.push_str(cell);
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SAMPLE: &str = include_str!("../reference/fixtures/endpoints-sample.json");

    fn host(tag: &str, name: &str, quantization: Option<&str>) -> Host {
        Host {
            tag: tag.into(),
            name: name.into(),
            quantization: quantization.map(str::to_owned),
            tools: false,
            structured_outputs: false,
        }
    }

    #[test]
    fn the_hosts_are_read_from_an_endpoints_list() {
        let reply: Value = serde_json::from_str(SAMPLE).unwrap();
        let hosts = hosts_in(&reply);
        assert_eq!(
            hosts
                .iter()
                .map(|host| (
                    host.tag.as_str(),
                    host.name.as_str(),
                    host.quantization.as_deref(),
                    host.tools,
                    host.structured_outputs,
                ))
                .collect::<Vec<_>>(),
            [
                ("deepinfra/fp4", "DeepInfra", Some("fp4"), false, true),
                ("novita", "Novita", None, false, true),
                ("tenstorrent/fp8", "Tenstorrent", Some("fp8"), false, false),
                ("bare", "Bare", Some("fp8"), false, false),
            ]
        );
        assert!(hosts_in(&json!({})).is_empty());
    }

    #[test]
    fn the_tools_flag_is_read() {
        let reply = json!({"data": {"endpoints": [
            {"provider_name": "A", "tag": "a", "supported_parameters": ["tools", "max_tokens"]},
            {"tag": "b/fp8", "supported_parameters": "tools"},
        ]}});
        let hosts = hosts_in(&reply);
        assert!(hosts[0].tools && !hosts[0].structured_outputs);
        assert!(!hosts[1].tools);
        // No provider_name: the tag stands in.
        assert_eq!(hosts[1].name, "b/fp8");
    }

    #[test]
    fn a_host_is_labelled_by_what_tells_its_endpoints_apart() {
        assert_eq!(host("novita", "Novita", None).label(), "Novita");
        assert_eq!(
            host("deepinfra/fp4", "DeepInfra", Some("fp4")).label(),
            "DeepInfra (fp4)"
        );
        assert_eq!(
            host("sail-research/fp4/us", "Sail Research", Some("fp4")).label(),
            "Sail Research (fp4, us)"
        );
        assert_eq!(
            host("deepinfra/fp8/priority", "DeepInfra", None).label(),
            "DeepInfra (fp8, priority)"
        );
        assert_eq!(host("a//b", "A", None).label(), "A (b)");
    }

    #[test]
    fn hosts_are_sorted_by_label() {
        let reply: Value = serde_json::from_str(SAMPLE).unwrap();
        let labels: Vec<String> = sorted(hosts_in(&reply)).iter().map(Host::label).collect();
        assert_eq!(
            labels,
            [
                "Bare (fp8)",
                "DeepInfra (fp4)",
                "Novita",
                "Tenstorrent (fp8)"
            ]
        );
        let mixed = sorted(vec![host("b", "banana", None), host("a", "Apple", None)]);
        assert_eq!(mixed[0].name, "Apple");
    }

    #[test]
    fn the_model_id_is_validated_and_the_url_built() {
        let url = endpoints_url("https://openrouter.ai/api/v1", "z-ai/glm-5.3-flash").unwrap();
        assert_eq!(
            url.as_str(),
            "https://openrouter.ai/api/v1/models/z-ai/glm-5.3-flash/endpoints"
        );
        let url = endpoints_url("http://127.0.0.1:9/api/v1/", "a/b").unwrap();
        assert_eq!(url.path(), "/api/v1/models/a/b/endpoints");
        for bad in ["", "noslash", "/slug", "author/", "a/b/c", "~a/b/../x/y"] {
            assert!(
                endpoints_url("https://openrouter.ai/api/v1", bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn render_prints_one_line_per_host() {
        let reply: Value = serde_json::from_str(SAMPLE).unwrap();
        let out = render(&sorted(hosts_in(&reply)));
        assert_eq!(out.lines().count(), 4);
        let novita = out.lines().find(|l| l.starts_with("novita")).unwrap();
        assert!(novita.contains("Novita"));
        assert!(novita.contains("structured_outputs=yes"));
        assert!(novita.contains("tools=no"));
    }
}
