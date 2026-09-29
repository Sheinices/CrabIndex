// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Sync clients seen by this host. Every `/sync/*` request is attributed to a client by IP
//! (behind a reverse proxy: `CF-Connecting-IP`, `X-Real-IP`, first `X-Forwarded-For`); CrabIndex
//! clients also send `X-CrabIndex-Version`. The registry lives in memory and is saved to
//! `Data/temp/sync_peers.json` by the sync worker so it survives restarts.

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};

pub const PEERS_PATH: &str = "Data/temp/sync_peers.json";
/// Clients not seen for this long are dropped from the list.
const KEEP_DAYS: i64 = 30;
pub const VERSION_HEADER: &str = "x-crabindex-version";
/// Compact JSON the client sends with `/sync/conf`: `{buckets, issues, errors, check, idIndex}`.
pub const STATUS_HEADER: &str = "x-crabindex-status";
const STATUS_MAX_BYTES: usize = 2048;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[allow(non_snake_case)]
pub struct Peer {
    pub ip: String,
    /// `X-CrabIndex-Version`, empty for jacred-style clients.
    pub version: String,
    pub firstSeen: DateTime<Utc>,
    pub lastSeen: DateTime<Utc>,
    pub requests: u64,
    /// `time` cursor of the last `/sync/fdb/torrents` request (fileTime), 0 when unknown.
    pub lastCursor: i64,
    /// Last full pass of slim rows (`spidr=true`).
    pub lastSpidr: Option<DateTime<Utc>>,
    /// Last `/sync/fdb/digest` request (integrity check).
    pub lastCheck: Option<DateTime<Utc>>,
    /// Last `/sync/fdb?key=` request (bucket refetch).
    pub lastRefetch: Option<DateTime<Utc>>,
    /// Self-reported state (`X-CrabIndex-Status`), CrabIndex clients only.
    #[serde(default)]
    pub status: Option<serde_json::Value>,
    #[serde(default)]
    pub statusAt: Option<DateTime<Utc>>,
}

static PEERS: Lazy<RwLock<HashMap<String, Peer>>> = Lazy::new(|| RwLock::new(HashMap::new()));
static DIRTY: AtomicBool = AtomicBool::new(false);

/// What a `/sync/*` request was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Conf { status: Option<serde_json::Value> },
    Torrents { cursor: i64, spidr: bool },
    Bucket,
    Digest,
}

/// Client address: proxy headers first (the sync host usually sits behind nginx or
/// Cloudflare), then the socket peer.
pub fn client_ip(headers: &HeaderMap, peer: Option<&ConnectInfo<SocketAddr>>) -> String {
    for name in ["cf-connecting-ip", "x-real-ip"] {
        if let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty()) {
            return v.to_string();
        }
    }
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = v.split(',').map(str::trim).find(|s| !s.is_empty()) {
            return first.to_string();
        }
    }
    peer.map(|p| p.0.ip().to_string()).unwrap_or_default()
}

/// Parsed `X-CrabIndex-Status` with only the known fields kept (untrusted input).
pub fn client_status(headers: &HeaderMap) -> Option<serde_json::Value> {
    let raw = headers.get(STATUS_HEADER)?.to_str().ok()?;
    if raw.len() > STATUS_MAX_BYTES {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let mut out = serde_json::Map::new();
    for k in ["buckets", "issues", "errors", "idIndex"] {
        if let Some(x) = v.get(k) {
            if x.is_number() || x.is_boolean() {
                out.insert(k.into(), x.clone());
            }
        }
    }
    if let Some(chk) = v.get("check").filter(|c| c.is_object()) {
        let mut c = serde_json::Map::new();
        for k in ["at", "ok", "remaining", "missing", "mismatched", "extra"] {
            if let Some(x) = chk.get(k) {
                if x.is_number() || x.is_boolean() || (x.is_string() && x.as_str().map(|s| s.len() <= 40).unwrap_or(false)) {
                    c.insert(k.into(), x.clone());
                }
            }
        }
        out.insert("check".into(), serde_json::Value::Object(c));
    }
    Some(serde_json::Value::Object(out))
}

pub fn client_version(headers: &HeaderMap) -> String {
    headers.get(VERSION_HEADER).and_then(|v| v.to_str().ok()).map(|s| s.trim().chars().take(40).collect()).unwrap_or_default()
}

pub fn record(ip: &str, version: &str, hit: Hit) {
    if ip.is_empty() {
        return;
    }
    let now = Utc::now();
    let mut g = PEERS.write();
    let p = g.entry(ip.to_string()).or_insert_with(|| Peer {
        ip: ip.to_string(),
        version: String::new(),
        firstSeen: now,
        lastSeen: now,
        requests: 0,
        lastCursor: 0,
        lastSpidr: None,
        lastCheck: None,
        lastRefetch: None,
        status: None,
        statusAt: None,
    });
    p.lastSeen = now;
    p.requests += 1;
    if !version.is_empty() {
        p.version = version.to_string();
    }
    match hit {
        Hit::Conf { status } => {
            if let Some(st) = status {
                p.status = Some(st);
                p.statusAt = Some(now);
            }
        }
        Hit::Torrents { cursor, spidr } => {
            if spidr {
                p.lastSpidr = Some(now);
            } else if cursor > 0 {
                p.lastCursor = cursor;
            }
        }
        Hit::Bucket => p.lastRefetch = Some(now),
        Hit::Digest => p.lastCheck = Some(now),
    }
    DIRTY.store(true, Ordering::Relaxed);
}

/// Clients by last activity, newest first.
pub fn list() -> Vec<Peer> {
    let mut v: Vec<Peer> = PEERS.read().values().cloned().collect();
    v.sort_by(|a, b| b.lastSeen.cmp(&a.lastSeen));
    v
}

pub fn load() {
    let Ok(s) = std::fs::read_to_string(PEERS_PATH) else { return };
    let Ok(list) = serde_json::from_str::<Vec<Peer>>(&s) else { return };
    let mut g = PEERS.write();
    for p in list {
        g.insert(p.ip.clone(), p);
    }
}

/// Save when something changed; drops clients idle for more than [`KEEP_DAYS`].
pub fn save_if_dirty() {
    if !DIRTY.swap(false, Ordering::Relaxed) {
        return;
    }
    let cutoff = Utc::now() - chrono::Duration::days(KEEP_DAYS);
    let list: Vec<Peer> = {
        let mut g = PEERS.write();
        g.retain(|_, p| p.lastSeen >= cutoff);
        let mut v: Vec<Peer> = g.values().cloned().collect();
        v.sort_by(|a, b| b.lastSeen.cmp(&a.lastSeen));
        v
    };
    let _ = std::fs::create_dir_all("Data/temp");
    if let Ok(json) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(PEERS_PATH, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn ip_prefers_proxy_headers_then_socket() {
        let mut h = HeaderMap::new();
        let peer = ConnectInfo("10.0.0.5:4444".parse::<SocketAddr>().unwrap());
        assert_eq!(client_ip(&h, Some(&peer)), "10.0.0.5");
        h.insert("x-forwarded-for", HeaderValue::from_static(" 203.0.113.9 , 10.0.0.1"));
        assert_eq!(client_ip(&h, Some(&peer)), "203.0.113.9");
        h.insert("cf-connecting-ip", HeaderValue::from_static("198.51.100.2"));
        assert_eq!(client_ip(&h, Some(&peer)), "198.51.100.2");
        assert_eq!(client_ip(&HeaderMap::new(), None), "");
    }

    #[test]
    fn record_tracks_cursor_spidr_and_version() {
        record("198.51.100.7", "", Hit::Conf { status: None });
        record("198.51.100.7", "1.2.3", Hit::Torrents { cursor: 42, spidr: false });
        record("198.51.100.7", "", Hit::Torrents { cursor: 0, spidr: true });
        record("198.51.100.7", "", Hit::Digest);
        let p = list().into_iter().find(|p| p.ip == "198.51.100.7").expect("peer");
        assert_eq!((p.requests, p.version.as_str(), p.lastCursor), (4, "1.2.3", 42));
        assert!(p.lastSpidr.is_some() && p.lastCheck.is_some() && p.lastRefetch.is_none());
        record("", "x", Hit::Conf { status: None });
        assert!(list().iter().all(|p| !p.ip.is_empty()));
    }

    #[test]
    fn status_header_keeps_known_fields_only() {
        let mut h = HeaderMap::new();
        h.insert(STATUS_HEADER, HeaderValue::from_static(r#"{"buckets":5,"issues":2,"errors":1,"idIndex":true,"check":{"at":"2026-09-29T20:40:31Z","remaining":0,"ok":true,"junk":"x"},"evil":"<script>"}"#));
        let st = client_status(&h).expect("status");
        assert_eq!(st["buckets"], 5);
        assert_eq!(st["check"]["remaining"], 0);
        assert!(st.get("evil").is_none() && st["check"].get("junk").is_none());
        h.insert(STATUS_HEADER, HeaderValue::from_static("not json"));
        assert!(client_status(&h).is_none());
        record("198.51.100.9", "1.2.3", Hit::Conf { status: Some(st.clone()) });
        let p = list().into_iter().find(|p| p.ip == "198.51.100.9").expect("peer");
        assert_eq!(p.status.as_ref().map(|s| s["issues"].as_i64()), Some(Some(2)));
        assert!(p.statusAt.is_some());
    }
}
