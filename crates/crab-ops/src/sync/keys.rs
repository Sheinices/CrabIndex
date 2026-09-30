// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Per-client keys for `/sync/*`. A key names a client in the peers list and can be revoked;
//! with `syncRequireKey: true` the host answers `/sync/*` only to requests that carry a valid
//! key (`X-CrabIndex-Key` header or `?synckey=`). Managed from the admin panel
//! (`/cron/sync/keys*`), stored in `Data/temp/sync_keys.json`.

use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

pub const KEYS_PATH: &str = "Data/temp/sync_keys.json";
pub const KEY_HEADER: &str = "x-crabindex-key";
pub const KEY_QUERY: &str = "synckey";
const KEY_LEN: usize = 32;
const MAX_KEYS: usize = 200;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[allow(non_snake_case)]
pub struct SyncKey {
    /// Operator-chosen label, unique, shown in the peers list.
    pub name: String,
    pub key: String,
    pub createdAt: DateTime<Utc>,
    #[serde(default)]
    pub lastUsedAt: Option<DateTime<Utc>>,
    #[serde(default)]
    pub lastIp: String,
    #[serde(default)]
    pub disabled: bool,
}

static KEYS: Lazy<RwLock<Vec<SyncKey>>> = Lazy::new(|| RwLock::new(Vec::new()));
static DIRTY: AtomicBool = AtomicBool::new(false);
static LOADED: AtomicBool = AtomicBool::new(false);

/// Outcome of looking at a request's key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Access {
    /// Valid key: the client's name.
    Named(String),
    /// No key sent.
    Anonymous,
    /// A key was sent but it is unknown or disabled.
    Rejected,
}

fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.len() <= 40 && n.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
}

fn mask(key: &str) -> String {
    if key.len() <= 8 {
        return "…".into();
    }
    format!("{}…{}", &key[..4], &key[key.len() - 4..])
}

/// Keys with the secret masked (`abcd…wxyz`), newest first.
pub fn list() -> Vec<Value> {
    ensure_loaded();
    let g = KEYS.read();
    let mut v: Vec<&SyncKey> = g.iter().collect();
    v.sort_by(|a, b| b.createdAt.cmp(&a.createdAt));
    v.into_iter()
        .map(|k| json!({ "name": k.name, "key": mask(&k.key), "createdAt": k.createdAt, "lastUsedAt": k.lastUsedAt, "lastIp": k.lastIp, "disabled": k.disabled }))
        .collect()
}

/// New key for `name`; the full secret is returned once, here.
pub fn create(name: &str) -> Result<Value, String> {
    ensure_loaded();
    let name = name.trim();
    if !valid_name(name) {
        return Err("name: 1-40 letters, digits, - _ . or space".into());
    }
    let mut g = KEYS.write();
    if g.len() >= MAX_KEYS {
        return Err(format!("at most {MAX_KEYS} keys"));
    }
    if g.iter().any(|k| k.name.eq_ignore_ascii_case(name)) {
        return Err("a key with this name exists".into());
    }
    let key: String = rand::thread_rng().sample_iter(&Alphanumeric).take(KEY_LEN).map(char::from).collect();
    let k = SyncKey { name: name.to_string(), key: key.clone(), createdAt: Utc::now(), lastUsedAt: None, lastIp: String::new(), disabled: false };
    g.push(k);
    DIRTY.store(true, Ordering::SeqCst);
    Ok(json!({ "ok": true, "name": name, "key": key }))
}

fn set_disabled(name: &str, disabled: bool) -> bool {
    ensure_loaded();
    let mut g = KEYS.write();
    let Some(k) = g.iter_mut().find(|k| k.name.eq_ignore_ascii_case(name.trim())) else { return false };
    k.disabled = disabled;
    DIRTY.store(true, Ordering::SeqCst);
    true
}

/// Stop accepting the key (kept in the list, can be enabled again).
pub fn revoke(name: &str) -> bool {
    set_disabled(name, true)
}

pub fn enable(name: &str) -> bool {
    set_disabled(name, false)
}

pub fn delete(name: &str) -> bool {
    ensure_loaded();
    let mut g = KEYS.write();
    let before = g.len();
    g.retain(|k| !k.name.eq_ignore_ascii_case(name.trim()));
    let removed = g.len() != before;
    if removed {
        DIRTY.store(true, Ordering::SeqCst);
    }
    removed
}

/// The raw key of a request: header first, then `?synckey=`.
pub fn key_from_request(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(v) = headers.get(KEY_HEADER).and_then(|v| v.to_str().ok()).map(str::trim).filter(|s| !s.is_empty()) {
        return Some(v.to_string());
    }
    let q = query?;
    url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == KEY_QUERY).map(|(_, v)| v.into_owned()).filter(|v| !v.trim().is_empty())
}

/// Check a request's key and, when valid, note the use (time and ip).
pub fn check(headers: &HeaderMap, query: Option<&str>, ip: &str) -> Access {
    let Some(raw) = key_from_request(headers, query) else { return Access::Anonymous };
    ensure_loaded();
    let mut g = KEYS.write();
    match g.iter_mut().find(|k| k.key == raw) {
        Some(k) if !k.disabled => {
            k.lastUsedAt = Some(Utc::now());
            if !ip.is_empty() && k.lastIp != ip {
                k.lastIp = ip.to_string();
            }
            DIRTY.store(true, Ordering::SeqCst);
            Access::Named(k.name.clone())
        }
        _ => Access::Rejected,
    }
}

fn ensure_loaded() {
    if LOADED.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Ok(s) = std::fs::read_to_string(KEYS_PATH) {
        if let Ok(list) = serde_json::from_str::<Vec<SyncKey>>(&s) {
            *KEYS.write() = list;
        }
    }
}

pub fn load() {
    ensure_loaded();
}

pub fn save_if_dirty() {
    if !DIRTY.swap(false, Ordering::SeqCst) {
        return;
    }
    let g = KEYS.read();
    if let Ok(json) = serde_json::to_string_pretty(&*g) {
        let _ = std::fs::create_dir_all("Data/temp");
        let _ = std::fs::write(KEYS_PATH, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn create_check_revoke() {
        LOADED.store(true, Ordering::SeqCst);
        KEYS.write().clear();
        assert!(create("").is_err());
        assert!(create("bad/name").is_err());
        let v = create("home-box").expect("created");
        let key = v["key"].as_str().unwrap().to_string();
        assert_eq!(key.len(), KEY_LEN);
        assert!(create("HOME-box").is_err(), "names are unique, case-insensitive");
        let listed = list();
        assert_eq!(listed[0]["name"], "home-box");
        assert_ne!(listed[0]["key"], key, "the list never shows the secret");

        let mut h = HeaderMap::new();
        assert_eq!(check(&h, None, "1.2.3.4"), Access::Anonymous);
        h.insert(KEY_HEADER, HeaderValue::from_str(&key).unwrap());
        assert_eq!(check(&h, None, "1.2.3.4"), Access::Named("home-box".into()));
        assert_eq!(list()[0]["lastIp"], "1.2.3.4");
        assert_eq!(check(&HeaderMap::new(), Some(&format!("time=1&synckey={key}")), ""), Access::Named("home-box".into()));
        h.insert(KEY_HEADER, HeaderValue::from_static("nope"));
        assert_eq!(check(&h, None, ""), Access::Rejected);

        assert!(revoke("home-box"));
        h.insert(KEY_HEADER, HeaderValue::from_str(&key).unwrap());
        assert_eq!(check(&h, None, ""), Access::Rejected);
        assert!(enable("home-box"));
        assert_eq!(check(&h, None, ""), Access::Named("home-box".into()));
        assert!(delete("home-box"));
        assert!(!delete("home-box"));
        assert_eq!(check(&h, None, ""), Access::Rejected);
    }
}
