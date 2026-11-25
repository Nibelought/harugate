//! The recent-requests list the web UI shows: a fixed-size ring buffer in memory.
//!
//! A record holds the facts of the per-request log line (model, routing applied, status,
//! latency) and nothing else: no keys, headers, bodies or query strings. It is lost when
//! the proxy stops.

use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::routing::Applied;

/// How many requests the UI list keeps.
pub const CAPACITY: usize = 100;

/// Longest model id / path kept, so one odd request can't make the list large.
const MAX_TEXT: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RequestRecord {
    /// Counts up from 1 for the life of the process.
    pub id: u64,
    /// When the upstream answered (unix milliseconds).
    pub time_ms: u64,
    pub method: String,
    /// The request path, with no query.
    pub path: String,
    pub model: Option<String>,
    /// What the proxy applied, as in the log line: `pin:<tag>`, `sort:<rule>`, `balanced`
    /// or `none`.
    pub routing: String,
    /// The upstream's status (502 when it could not be reached, 400 for a bad body).
    pub status: u16,
    /// Time until the upstream's response headers arrived.
    pub latency_ms: u64,
}

#[derive(Debug)]
struct Inner {
    next_id: u64,
    items: VecDeque<RequestRecord>,
}

/// A bounded, thread-safe list of the latest requests.
#[derive(Debug)]
pub struct RequestLog {
    capacity: usize,
    inner: Mutex<Inner>,
}

impl Default for RequestLog {
    fn default() -> Self {
        Self::new(CAPACITY)
    }
}

fn clip(text: &str) -> String {
    text.chars().take(MAX_TEXT).collect()
}

impl RequestLog {
    pub fn new(capacity: usize) -> Self {
        RequestLog {
            capacity: capacity.max(1),
            inner: Mutex::new(Inner {
                next_id: 1,
                items: VecDeque::new(),
            }),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Add one request, dropping the oldest when full.
    pub fn record(
        &self,
        method: &str,
        path: &str,
        model: Option<&str>,
        applied: &Applied,
        status: u16,
        latency_ms: u64,
    ) {
        let time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let id = inner.next_id;
        inner.next_id += 1;
        if inner.items.len() >= self.capacity {
            inner.items.pop_front();
        }
        inner.items.push_back(RequestRecord {
            id,
            time_ms,
            method: clip(method),
            path: clip(path),
            model: model.map(clip),
            routing: applied.to_string(),
            status,
            latency_ms,
        });
    }

    /// The kept requests, newest first.
    pub fn recent(&self) -> Vec<RequestRecord> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.items.iter().rev().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::Routing;

    #[test]
    fn the_oldest_request_is_dropped_when_full() {
        let log = RequestLog::new(3);
        for n in 0..5u16 {
            log.record(
                "POST",
                "/v1/chat/completions",
                Some("a/b"),
                &Applied::Balanced,
                200 + n,
                1,
            );
        }
        let recent = log.recent();
        assert_eq!(recent.len(), 3);
        assert_eq!(
            recent.iter().map(|r| r.id).collect::<Vec<_>>(),
            [5, 4, 3],
            "newest first"
        );
        assert_eq!(recent[0].status, 204);
    }

    #[test]
    fn the_routing_is_recorded_as_in_the_log_line() {
        let log = RequestLog::default();
        log.record("GET", "/v1/models", None, &Applied::Untouched, 200, 3);
        log.record(
            "POST",
            "/v1/chat/completions",
            Some("a/b"),
            &Applied::Pin("x/fp8".into()),
            404,
            9,
        );
        log.record(
            "POST",
            "/v1/chat/completions",
            Some("a/b"),
            &Applied::Sort(Routing::Price),
            200,
            9,
        );
        let routing: Vec<_> = log.recent().into_iter().map(|r| r.routing).collect();
        assert_eq!(routing, ["sort:price", "pin:x/fp8", "none"]);
    }

    #[test]
    fn long_text_is_clipped() {
        let log = RequestLog::default();
        log.record(
            "GET",
            &"p".repeat(5000),
            Some(&"m".repeat(5000)),
            &Applied::Balanced,
            200,
            1,
        );
        let record = &log.recent()[0];
        assert_eq!(record.path.len(), MAX_TEXT);
        assert_eq!(record.model.as_deref().map(str::len), Some(MAX_TEXT));
    }
}
