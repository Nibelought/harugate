//! The proxy's config file: its format, defaults, validation and hot reload.
//!
//! The pure parts are [`parse`] and [`validate`] (no I/O). [`ConfigStore`] owns the
//! file on disk: it re-reads it on every [`ConfigStore::current`] call, keeps the last
//! good config when an edit is invalid, and can save changes back ([`ConfigStore::save`]).

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

use crate::routing::Routing;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8787";
pub const DEFAULT_UPSTREAM: &str = "https://openrouter.ai/api/v1";

/// The file written when the default config path doesn't exist yet.
pub const DEFAULT_CONFIG_TOML: &str = r##"# harugate config

# N1: loopback only by default. Binding elsewhere (e.g. "0.0.0.0:8787") exposes the proxy
# to your network. Changing this needs a restart.
listen = "127.0.0.1:8787"
upstream = "https://openrouter.ai/api/v1"

# Global sort for models with no pin: "price" | "throughput" | "latency".
# Omit for Balanced (OpenRouter's default).
# routing = "price"

# Only hosts that support every parameter the request sends.
require_parameters = true

# Model id (as Koharu sends it, OpenRouter's author/slug) -> endpoint tag.
# Find tags with: harugate hosts <author/slug>
[pins]
# "deepseek/deepseek-v4.1-flash-1017" = "deepinfra/fp8"
"##;

/// The config file. Missing keys take the defaults below; unknown keys are an error
/// (a typo would otherwise be silently ignored).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address to listen on. Changing it needs a restart.
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    /// OpenRouter's API base, e.g. `https://openrouter.ai/api/v1`.
    #[serde(default = "default_upstream")]
    pub upstream: String,
    /// Global sort for models with no pin. `None` is OpenRouter's balancing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<Routing>,
    #[serde(default = "default_true")]
    pub require_parameters: bool,
    /// Model id -> endpoint tag.
    #[serde(default)]
    pub pins: BTreeMap<String, String>,
}

fn default_listen() -> SocketAddr {
    DEFAULT_LISTEN
        .parse()
        .expect("valid default listen address")
}

fn default_upstream() -> String {
    DEFAULT_UPSTREAM.to_owned()
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: default_listen(),
            upstream: default_upstream(),
            routing: None,
            require_parameters: true,
            pins: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Serialize as TOML (comments of a hand-edited file are not kept).
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(|e| ConfigError::Invalid(e.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("can't read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("can't write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Not valid TOML, an unknown key, or a value of the wrong kind (e.g. `routing = "fastest"`).
    #[error("{0}")]
    Parse(String),
    /// Valid TOML that makes no usable config.
    #[error("{0}")]
    Invalid(String),
}

/// Parse and validate config text. Pure: no I/O.
pub fn parse(text: &str) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(text)
        .map_err(|e| ConfigError::Parse(e.to_string().trim_end().to_owned()))?;
    validate(&config)?;
    Ok(config)
}

/// Check what the types can't: the upstream URL and the pins. Pure.
pub fn validate(config: &Config) -> Result<(), ConfigError> {
    let url = reqwest::Url::parse(&config.upstream).map_err(|e| {
        ConfigError::Invalid(format!("upstream `{}` is not a URL: {e}", config.upstream))
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.cannot_be_a_base() || url.host().is_none() {
        return Err(ConfigError::Invalid(format!(
            "upstream `{}` must be an http(s) URL such as {DEFAULT_UPSTREAM}",
            config.upstream
        )));
    }
    for (model, tag) in &config.pins {
        if model.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "a pin has an empty model id".to_owned(),
            ));
        }
        if tag.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "the pin for `{model}` has an empty endpoint tag"
            )));
        }
    }
    Ok(())
}

/// True for an `https://openrouter.ai/...` URL, the one place the proxy is meant to send
/// the user's API key. Compares the parsed host, so `https://openrouter.ai.evil.example`
/// and `https://openrouter.ai@evil.example` are not official.
pub fn is_official_upstream(upstream: &str) -> bool {
    reqwest::Url::parse(upstream).is_ok_and(|url| {
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host.eq_ignore_ascii_case("openrouter.ai"))
            && url.port().is_none()
            && url.username().is_empty()
    })
}

/// The risky settings `new` introduces compared with `current` (N1): an upstream that is
/// not OpenRouter (it receives the API key), or a listen address that is not loopback
/// (anyone who can reach it can spend the key). A value that was already there is not
/// new, so saving other settings of a hand-edited file needs no confirmation. Empty if
/// nothing risky is introduced.
pub fn new_risks(current: &Config, new: &Config) -> Vec<String> {
    let mut risks = Vec::new();
    if new.upstream != current.upstream && !is_official_upstream(&new.upstream) {
        risks.push(format!(
            "the upstream `{}` is not https://openrouter.ai/...: it will receive your OpenRouter API key",
            new.upstream
        ));
    }
    if new.listen != current.listen && !new.listen.ip().is_loopback() {
        risks.push(format!(
            "`{}` is not a loopback address: anyone who can reach it can use your OpenRouter key through this proxy",
            new.listen
        ));
    }
    risks
}

/// `<config dir>/harugate/config.toml` (via the `dirs` crate, so the right
/// place on macOS, Linux and Windows), or `None` if the platform has no config dir.
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("harugate").join("config.toml"))
}

/// Write `text` to `path` through a temp file in the same directory and a rename, so a
/// reader (or this proxy's own reload) never sees half a file.
fn write_atomic(path: &Path, text: &str) -> Result<(), ConfigError> {
    let werr = |source| ConfigError::Write {
        path: path.to_owned(),
        source,
    };
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(werr)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).map_err(werr)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        werr(e)
    })
}

/// Create the default config file at `path` if nothing is there. Returns whether it
/// was created.
pub fn ensure_default_file(path: &Path) -> Result<bool, ConfigError> {
    if path.exists() {
        return Ok(false);
    }
    write_atomic(path, DEFAULT_CONFIG_TOML)?;
    Ok(true)
}

/// Read and parse the file at `path` (no reload machinery).
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_owned(),
        source,
    })?;
    parse(&text)
}

/// Validate and write `config` to `path` atomically.
pub fn save(path: &Path, config: &Config) -> Result<(), ConfigError> {
    validate(config)?;
    write_atomic(path, &config.to_toml()?)
}

/// `existing` (the config file's text) with `config`'s values written into it in place:
/// comments, blank lines, key order and formatting are kept; only values that changed are
/// touched. A `routing` of `None` removes the key (with the comment lines directly above
/// it); keys at their default are not added to a file that lacks them. The result is
/// parsed and validated again, and must read back as exactly `config`; otherwise an
/// error is returned and the caller writes nothing. Pure: no I/O.
pub fn save_preserving(existing: &str, config: &Config) -> Result<String, ConfigError> {
    validate(config)?;
    let mut doc = existing.parse::<DocumentMut>().map_err(|e| {
        ConfigError::Parse(format!(
            "the config file is not valid TOML, so it can't be updated in place; fix it in the raw editor first:\n{}",
            e.to_string().trim_end()
        ))
    })?;
    let root = doc.as_table_mut();

    set_scalar(
        root,
        "listen",
        Value::from(config.listen.to_string()),
        config.listen == default_listen(),
    );
    set_scalar(
        root,
        "upstream",
        Value::from(config.upstream.clone()),
        config.upstream == DEFAULT_UPSTREAM,
    );
    match config.routing {
        Some(routing) => set_scalar(root, "routing", Value::from(routing.as_str()), false),
        None => {
            root.remove("routing");
        }
    }
    set_scalar(
        root,
        "require_parameters",
        Value::from(config.require_parameters),
        config.require_parameters,
    );
    set_pins(root, config)?;

    let text = doc.to_string();
    let reread = parse(&text)?;
    if reread != *config {
        return Err(ConfigError::Invalid(
            "the config file has a layout that can't be updated in place (for example a `pins` or other key written in an unusual way); edit it in the raw editor instead"
                .to_owned(),
        ));
    }
    Ok(text)
}

/// Set `key` to `new` unless it already holds that value. A missing key whose wanted
/// value is the default is left missing. The old value's surrounding whitespace and
/// trailing comment are kept.
fn set_scalar(table: &mut Table, key: &str, mut new: Value, is_default: bool) {
    match table.get_mut(key) {
        Some(item) => {
            let same = match (&*item, &new) {
                (Item::Value(Value::String(old)), Value::String(new)) => old.value() == new.value(),
                (Item::Value(Value::Boolean(old)), Value::Boolean(new)) => {
                    old.value() == new.value()
                }
                _ => false,
            };
            if same {
                return;
            }
            if let Item::Value(old) = item {
                *new.decor_mut() = old.decor().clone();
            }
            *item = Item::Value(new);
        }
        None if is_default => {}
        None => {
            table.insert(key, Item::Value(new));
        }
    }
}

/// Make the `[pins]` table hold exactly `config.pins`: existing entries keep their place,
/// comments and formatting; removed ones are dropped; new ones are appended.
fn set_pins(root: &mut Table, config: &Config) -> Result<(), ConfigError> {
    if root.get("pins").is_none() {
        if config.pins.is_empty() {
            return Ok(());
        }
        root.insert("pins", Item::Table(Table::new()));
    }
    let pins: &mut dyn TableLike = root
        .get_mut("pins")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| {
            ConfigError::Invalid("`pins` in the config file is not a table".to_owned())
        })?;
    let stale: Vec<String> = pins
        .iter()
        .map(|(model, _)| model.to_owned())
        .filter(|model| !config.pins.contains_key(model))
        .collect();
    for model in stale {
        pins.remove(&model);
    }
    for (model, tag) in &config.pins {
        match pins.get_mut(model) {
            Some(item) => {
                if item.as_str() != Some(tag.as_str()) {
                    let mut new = Value::from(tag.clone());
                    if let Item::Value(old) = item {
                        *new.decor_mut() = old.decor().clone();
                    }
                    *item = Item::Value(new);
                }
            }
            None => {
                pins.insert(model, Item::Value(Value::from(tag.clone())));
            }
        }
    }
    Ok(())
}

struct Inner {
    /// The text last read from the file (good or bad), to skip re-parsing unchanged text.
    raw: Option<String>,
    config: Arc<Config>,
    /// Why the text in `raw` was rejected; the config above is then the last good one.
    parse_error: Option<String>,
    /// Why the file couldn't be read.
    read_error: Option<String>,
}

/// The config file and its last good contents. Cheap to share (`Arc<ConfigStore>`).
pub struct ConfigStore {
    path: PathBuf,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for ConfigStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl ConfigStore {
    /// Load `path`, which must hold a valid config.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let path = path.into();
        let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let config = parse(&text)?;
        Ok(ConfigStore {
            path,
            inner: Mutex::new(Inner {
                raw: Some(text),
                config: Arc::new(config),
                parse_error: None,
                read_error: None,
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The last good config, without looking at the file.
    pub fn snapshot(&self) -> Arc<Config> {
        self.lock().config.clone()
    }

    /// The error from the last reload attempt, if the file on disk is currently not
    /// usable (the proxy then runs on the last good config).
    pub fn last_error(&self) -> Option<String> {
        let inner = self.lock();
        inner
            .read_error
            .clone()
            .or_else(|| inner.parse_error.clone())
    }

    /// Hot reload: re-read the file and return the config to use for this request.
    /// An unreadable or invalid file keeps the last good config and logs the error
    /// once per distinct file content. Small file, one read per request: portable and
    /// immune to coarse mtimes (no watcher needed).
    pub async fn current(&self) -> Arc<Config> {
        let read = tokio::fs::read_to_string(&self.path).await;
        let mut inner = self.lock();
        match read {
            Err(e) => {
                let msg = format!("can't read {}: {e}", self.path.display());
                if inner.read_error.as_deref() != Some(msg.as_str()) {
                    tracing::error!("config: {msg}; keeping the last good config");
                    inner.read_error = Some(msg);
                }
            }
            Ok(text) => {
                inner.read_error = None;
                if inner.raw.as_deref() != Some(text.as_str()) {
                    match parse(&text) {
                        Ok(config) => {
                            if config.listen != inner.config.listen {
                                tracing::warn!(
                                    "config: `listen` changed to {}; restart the proxy to apply it",
                                    config.listen
                                );
                            }
                            tracing::info!(
                                "config: reloaded {} (routing: {}, {} pin(s))",
                                self.path.display(),
                                config.routing.map_or("balanced", Routing::as_str),
                                config.pins.len()
                            );
                            inner.config = Arc::new(config);
                            inner.parse_error = None;
                        }
                        Err(e) => {
                            tracing::error!(
                                "config: {} is invalid, keeping the last good config: {e}",
                                self.path.display()
                            );
                            inner.parse_error = Some(e.to_string());
                        }
                    }
                    inner.raw = Some(text);
                }
            }
        }
        inner.config.clone()
    }

    /// Validate `config`, write it to the file and make it current right away.
    pub fn save(&self, config: Config) -> Result<(), ConfigError> {
        validate(&config)?;
        let text = config.to_toml()?;
        write_atomic(&self.path, &text)?;
        let mut inner = self.lock();
        inner.raw = Some(text);
        inner.config = Arc::new(config);
        inner.parse_error = None;
        inner.read_error = None;
        Ok(())
    }

    /// Like [`ConfigStore::save`], but written into the existing file in place
    /// ([`save_preserving`]), so the user's comments and key order survive. Nothing is
    /// written if the update or its validation fails.
    pub fn save_preserving(&self, config: Config) -> Result<(), ConfigError> {
        let existing = self.read_text()?;
        let text = save_preserving(&existing, &config)?;
        write_atomic(&self.path, &text)?;
        let mut inner = self.lock();
        inner.raw = Some(text);
        inner.config = Arc::new(config);
        inner.parse_error = None;
        inner.read_error = None;
        Ok(())
    }

    /// Validate raw TOML (e.g. from a text editor in a UI), write it as is, comments
    /// included, and make it current.
    pub fn save_text(&self, text: &str) -> Result<(), ConfigError> {
        let config = parse(text)?;
        write_atomic(&self.path, text)?;
        let mut inner = self.lock();
        inner.raw = Some(text.to_owned());
        inner.config = Arc::new(config);
        inner.parse_error = None;
        inner.read_error = None;
        Ok(())
    }

    /// The file's current text, for a raw editor.
    pub fn read_text(&self) -> Result<String, ConfigError> {
        std::fs::read_to_string(&self.path).map_err(|source| ConfigError::Read {
            path: self.path.clone(),
            source,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../config.example.toml");

    #[test]
    fn the_example_file_parses() {
        let config = parse(EXAMPLE).unwrap();
        assert_eq!(config.listen, "127.0.0.1:8787".parse().unwrap());
        assert_eq!(config.upstream, "https://openrouter.ai/api/v1");
        assert_eq!(config.routing, Some(Routing::Price));
        assert!(config.require_parameters);
        assert_eq!(
            config
                .pins
                .get("deepseek/deepseek-v4.1-flash-1017")
                .map(String::as_str),
            Some("deepinfra/fp8")
        );
    }

    #[test]
    fn the_default_file_matches_the_default_config() {
        assert_eq!(parse(DEFAULT_CONFIG_TOML).unwrap(), Config::default());
    }

    #[test]
    fn an_unknown_routing_value_is_rejected_with_a_clear_message() {
        let err = parse("routing = \"fastest\"").unwrap_err().to_string();
        assert!(err.contains("fastest"), "{err}");
        assert!(err.contains("price"), "lists the valid values: {err}");
        for ok in ["price", "throughput", "latency"] {
            assert!(parse(&format!("routing = \"{ok}\"")).is_ok());
        }
    }

    #[test]
    fn missing_pins_is_empty_and_everything_has_a_default() {
        let config = parse("routing = \"latency\"\n").unwrap();
        assert!(config.pins.is_empty());
        assert_eq!(config.listen, default_listen());
        assert_eq!(config.upstream, DEFAULT_UPSTREAM);
        assert!(config.require_parameters);
        assert_eq!(parse("").unwrap(), Config::default());
    }

    #[test]
    fn bad_values_are_rejected() {
        assert!(parse("listen = \"nope\"").is_err());
        assert!(parse("upstream = \"ftp://x\"").is_err());
        assert!(parse("upstream = \"not a url\"").is_err());
        assert!(parse("typo_key = 1").is_err());
        assert!(parse("[pins]\n\"a/b\" = \"\"").is_err());
        assert!(parse("[pins]\n\"a/b\" = 3").is_err());
        assert!(parse("require_parameters = \"yes\"").is_err());
    }

    #[test]
    fn serialization_round_trips() {
        let mut config = Config {
            routing: Some(Routing::Throughput),
            require_parameters: false,
            ..Config::default()
        };
        config.pins.insert("a/b".into(), "x/fp8".into());
        let text = config.to_toml().unwrap();
        assert!(text.contains("routing = \"throughput\""), "{text}");
        assert_eq!(parse(&text).unwrap(), config);
        let none = Config::default().to_toml().unwrap();
        assert!(!none.contains("routing"));
        assert_eq!(parse(&none).unwrap(), Config::default());
    }

    #[test]
    fn default_path_is_under_the_config_dir() {
        if let Some(path) = default_path() {
            assert!(path.ends_with(Path::new("harugate").join("config.toml")));
        }
    }

    #[test]
    fn the_default_file_is_created_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        assert!(ensure_default_file(&path).unwrap());
        assert_eq!(load(&path).unwrap(), Config::default());
        std::fs::write(&path, "routing = \"price\"").unwrap();
        assert!(!ensure_default_file(&path).unwrap());
        assert_eq!(load(&path).unwrap().routing, Some(Routing::Price));
    }

    #[tokio::test]
    async fn a_bad_edit_keeps_the_last_good_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "routing = \"price\"\n[pins]\n\"a/b\" = \"x\"\n").unwrap();
        let store = ConfigStore::open(&path).unwrap();
        assert_eq!(store.current().await.routing, Some(Routing::Price));
        assert!(store.last_error().is_none());

        // Valid edit: picked up.
        std::fs::write(&path, "routing = \"latency\"\n").unwrap();
        let config = store.current().await;
        assert_eq!(config.routing, Some(Routing::Latency));
        assert!(config.pins.is_empty());

        // Invalid edit: last good kept, error reported.
        std::fs::write(&path, "routing = \"fastest\"\n").unwrap();
        assert_eq!(store.current().await.routing, Some(Routing::Latency));
        assert!(store.last_error().unwrap().contains("fastest"));
        assert_eq!(store.current().await.routing, Some(Routing::Latency));
        assert!(store.last_error().is_some());

        // Deleted file: last good kept.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(store.current().await.routing, Some(Routing::Latency));
        assert!(store.last_error().is_some());

        // Fixed: back to normal.
        std::fs::write(&path, "routing = \"throughput\"\n").unwrap();
        assert_eq!(store.current().await.routing, Some(Routing::Throughput));
        assert!(store.last_error().is_none());
    }

    #[tokio::test]
    async fn save_writes_the_file_and_takes_effect_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let store = ConfigStore::open(&path).unwrap();
        let mut config = Config::default();
        config.pins.insert("a/b".into(), "novita".into());
        store.save(config.clone()).unwrap();
        assert_eq!(*store.snapshot(), config);
        assert_eq!(load(&path).unwrap(), config);
        assert_eq!(*store.current().await, config);

        let bad = Config {
            upstream: "nope".into(),
            ..Config::default()
        };
        assert!(store.save(bad).is_err());
        assert_eq!(*store.snapshot(), config);

        store.save_text("# mine\nrouting = \"price\"\n").unwrap();
        assert_eq!(store.snapshot().routing, Some(Routing::Price));
        assert!(store.read_text().unwrap().contains("# mine"));
        assert!(store.save_text("routing = 5").is_err());
    }

    const COMMENTED: &str = "\
# top comment
listen = \"127.0.0.1:8787\" # loopback
upstream = \"https://openrouter.ai/api/v1\"

# the sort
routing = \"price\"

[pins]
# first
\"a/b\" = \"x/fp8\" # note
\"c/d\" = \"y\"
";

    #[test]
    fn save_preserving_keeps_comments_order_and_untouched_lines() {
        let mut config = parse(COMMENTED).unwrap();
        // Nothing changed: byte for byte the same.
        assert_eq!(save_preserving(COMMENTED, &config).unwrap(), COMMENTED);

        config.routing = Some(Routing::Latency);
        config.pins.remove("c/d");
        config.pins.insert("a/b".into(), "z".into());
        config.pins.insert("e/f".into(), "w/fp4".into());
        config.require_parameters = false;
        let text = save_preserving(COMMENTED, &config).unwrap();
        assert_eq!(parse(&text).unwrap(), config);
        for kept in [
            "# top comment",
            "# loopback",
            "# the sort",
            "# first",
            "# note",
        ] {
            assert!(text.contains(kept), "{kept}: {text}");
        }
        assert!(text.contains("routing = \"latency\""), "{text}");
        assert!(text.contains("\"a/b\" = \"z\" # note"), "{text}");
        assert!(text.contains("require_parameters = false"), "{text}");
        assert!(!text.contains("c/d"), "{text}");
        assert!(text.find("\"a/b\"").unwrap() < text.find("\"e/f\"").unwrap());

        config.routing = None;
        let text = save_preserving(&text, &config).unwrap();
        assert!(!text.contains("routing"), "{text}");
        assert_eq!(parse(&text).unwrap(), config);
    }

    #[test]
    fn save_preserving_starts_from_an_empty_or_minimal_file() {
        let mut config = Config::default();
        assert_eq!(save_preserving("", &config).unwrap(), "");
        config.pins.insert("a/b".into(), "x".into());
        config.routing = Some(Routing::Price);
        let text = save_preserving("# mine\n", &config).unwrap();
        assert!(text.contains("# mine"), "{text}");
        assert_eq!(parse(&text).unwrap(), config);
        // Inline-table pins work too.
        let text = save_preserving("pins = { \"a/b\" = \"x\" }\n", &Config::default()).unwrap();
        assert_eq!(parse(&text).unwrap(), Config::default());
    }

    #[test]
    fn save_preserving_refuses_what_it_cannot_do_and_invalid_configs() {
        let err = save_preserving("routing = ", &Config::default()).unwrap_err();
        assert!(err.to_string().contains("raw editor"), "{err}");
        let bad = Config {
            upstream: "nope".into(),
            ..Config::default()
        };
        assert!(save_preserving(COMMENTED, &bad).is_err());
        let err = save_preserving(
            "pins = 3\n",
            &Config {
                pins: [("a/b".to_owned(), "x".to_owned())].into(),
                ..Config::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("pins"), "{err}");
    }

    #[test]
    fn only_openrouter_over_https_is_official() {
        for ok in [
            "https://openrouter.ai/api/v1",
            "https://OpenRouter.ai/api/v1/",
            "https://openrouter.ai",
        ] {
            assert!(is_official_upstream(ok), "{ok}");
        }
        for bad in [
            "http://openrouter.ai/api/v1",
            "https://openrouter.ai.evil.example/api/v1",
            "https://openrouter.ai@evil.example/api/v1",
            "https://evil.example/openrouter.ai",
            "https://openrouter.ai:8443/api/v1",
            "https://sub.openrouter.ai/api/v1",
            "http://127.0.0.1:9/api/v1",
            "not a url",
        ] {
            assert!(!is_official_upstream(bad), "{bad}");
        }
    }

    #[test]
    fn new_risks_only_count_what_changes() {
        let safe = Config::default();
        assert!(new_risks(&safe, &safe).is_empty());
        let proxy = Config {
            upstream: "https://example.com/v1".into(),
            listen: "0.0.0.0:8787".parse().unwrap(),
            ..Config::default()
        };
        assert_eq!(new_risks(&safe, &proxy).len(), 2);
        // Already that way: no new risk.
        assert!(new_risks(&proxy, &proxy).is_empty());
        // Going back to safe is never risky; loopback variants are fine.
        assert!(new_risks(&proxy, &safe).is_empty());
        let v6 = Config {
            listen: "[::1]:9000".parse().unwrap(),
            ..Config::default()
        };
        assert!(new_risks(&safe, &v6).is_empty());
    }
}
