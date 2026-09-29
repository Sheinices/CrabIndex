// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Sync API served to other instances (`/sync/*`) and the sync workers that pull from `syncapi`.
//!
//! Wire format (v2): `/sync/fdb/torrents?time=<fileTime>&start=<fileTime>&spidr=<bool>` returns
//! `{nextread, countread, take, collections:[{Key, Value:{time, fileTime, torrents}}]}` with
//! buckets ordered by masterDb `fileTime`.

pub mod check;
pub mod cron;
pub mod peers;

use axum::extract::{ConnectInfo, Query};
use axum::http::HeaderMap;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use crab_core::fdb;
use crab_core::log::{self, cat};
use crab_core::models::sync::{Collection, Value as SyncValue};
use crab_core::models::{MasterDbShard, TorrentDetails};
use crab_core::{conf, hooks, time};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::query::Params;

/// Max torrents per `/sync/fdb/torrents` page (the page ends once `countread` exceeds it).
pub const TAKE: i32 = 2_000;

type SortedMaster = Arc<Vec<(String, MasterDbShard)>>;

static MASTER_CACHE: Lazy<RwLock<Option<SortedMaster>>> = Lazy::new(|| RwLock::new(None));

fn build_sorted_master() -> SortedMaster {
    let mut v = fdb::master_db_snapshot();
    v.sort_by_key(|(_, s)| s.fileTime);
    Arc::new(v)
}

/// masterDb ordered by `fileTime`, snapshotted at first use and refreshed every 10 minutes.
pub fn sorted_master() -> SortedMaster {
    if let Some(c) = MASTER_CACHE.read().clone() {
        return c;
    }
    let mut g = MASTER_CACHE.write();
    if let Some(c) = g.clone() {
        return c;
    }
    let c = build_sorted_master();
    *g = Some(c.clone());
    log::debug(cat::SYNC, "sync cache initialized");
    c
}

/// Rebuild the ordered masterDb snapshot now.
pub fn refresh_sorted_master() {
    let c = build_sorted_master();
    *MASTER_CACHE.write() = Some(c);
}

pub fn router() -> Router {
    Router::new()
        .route("/sync/conf", any(sync_conf))
        .route("/sync/fdb", any(fdb_key))
        .route("/sync/fdb/torrents", any(fdb_torrents))
        .route("/sync/fdb/digest", any(fdb_digest))
        .route("/sync/torrents", any(torrents_legacy))
        // DevAdmin policy (everything under /cron/), used by the admin panel.
        .route("/cron/sync/peers", any(cron_peers))
        .route("/cron/sync/check", any(cron_check))
        .route("/cron/sync/checkstatus", any(cron_check_status))
}

pub fn spawn_workers(shutdown: CancellationToken) {
    peers::load();
    let s = shutdown.clone();
    tokio::spawn(async move {
        loop {
            if !crate::sleep_ct(std::time::Duration::from_secs(600), &s).await {
                peers::save_if_dirty();
                return;
            }
            let _ = tokio::task::spawn_blocking(|| {
                refresh_sorted_master();
                peers::save_if_dirty();
            })
            .await;
        }
    });
    tokio::spawn(cron::run_worker(shutdown.clone()));
    tokio::spawn(check::run_worker(shutdown));
}

/// Clients that pulled from this host (admin panel, section "Clients").
async fn cron_peers() -> axum::Json<serde_json::Value> {
    axum::Json(json!({ "ok": true, "opensync": conf().opensync, "peers": peers::list() }))
}

/// Start an integrity check against `syncapi` now: `ok` / `work` / `disabled`.
async fn cron_check() -> String {
    check::start_now().to_string()
}

async fn cron_check_status() -> axum::Json<serde_json::Value> {
    axum::Json(json!({ "ok": true, "running": check::is_running(), "progress": check::progress(), "last": check::last_report() }))
}

/// `[[key, fileTime], ...]` for every bucket: the integrity check on a client compares it with
/// its own masterDb and refetches or drops buckets that differ.
async fn fdb_digest(headers: HeaderMap, peer: Option<ConnectInfo<SocketAddr>>) -> Response {
    if !conf().opensync {
        return axum::Json(json!({ "count": 0, "buckets": [] })).into_response();
    }
    peers::record(&peers::client_ip(&headers, peer.as_ref()), &peers::client_version(&headers), peers::Hit::Digest);
    let res = tokio::task::spawn_blocking(|| {
        let snap = fdb::master_db_snapshot();
        let buckets: Vec<(String, i64)> = snap.into_iter().map(|(k, s)| (k, s.fileTime)).collect();
        serde_json::to_vec(&json!({ "count": buckets.len(), "buckets": buckets }))
    })
    .await;
    match res {
        Ok(Ok(body)) => ([(header::CONTENT_TYPE, "application/json; charset=utf-8")], body).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Trackers whose rows this host serves: `synctrackers` when set, else every built-in slug,
/// minus `disable_trackers`. Clients treat a served tracker's rows in a bucket as complete and
/// drop local rows of that tracker the bucket no longer contains.
pub fn served_trackers() -> Vec<String> {
    let c = conf();
    let disabled: Vec<String> = c.disable_trackers.iter().map(|d| d.to_lowercase()).collect();
    let source: Vec<String> = match c.synctrackers.as_ref() {
        Some(list) => list.iter().map(|s| s.trim().to_lowercase()).collect(),
        None => crab_core::config::TRACKER_SLUGS.iter().map(|s| s.to_string()).collect(),
    };
    let mut out: Vec<String> = source.into_iter().filter(|s| !s.is_empty() && !disabled.contains(s)).collect();
    out.sort();
    out.dedup();
    out
}

async fn sync_conf(headers: HeaderMap, peer: Option<ConnectInfo<SocketAddr>>) -> axum::Json<serde_json::Value> {
    peers::record(&peers::client_ip(&headers, peer.as_ref()), &peers::client_version(&headers), peers::Hit::Conf { status: peers::client_status(&headers) });
    // `count` lets a client show how full its copy is versus this host (admin progress only).
    let count = crab_core::index::current_len().unwrap_or(0);
    axum::Json(json!({ "fbd": true, "spidr": true, "version": 2, "count": count, "trackers": served_trackers() }))
}

#[derive(Serialize)]
struct FdbKeyItem {
    Key: String,
    #[serde(with = "crab_core::time::net")]
    updateTime: chrono::DateTime<chrono::Utc>,
    fileTime: i64,
    path: String,
    value: fdb::ShardMap,
}

/// Bucket lookup by substring (debug aid for sync peers).
pub fn fdb_key_items(key: &str) -> Vec<serde_json::Value> {
    let matches: Vec<(String, MasterDbShard)> = fdb::MASTER_DB
        .iter()
        .filter(|e| e.key().contains(key))
        .take(20)
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect();
    matches
        .into_iter()
        .map(|(k, s)| {
            let md5 = crab_core::util::md5(&k);
            let item = FdbKeyItem {
                path: format!("Data/fdb/{}/{}", &md5[..2], &md5[2..]),
                value: fdb::open_read(&k, false, false),
                Key: k,
                updateTime: s.updateTime,
                fileTime: s.fileTime,
            };
            serde_json::to_value(item).unwrap_or(serde_json::Value::Null)
        })
        .collect()
}

async fn fdb_key(headers: HeaderMap, peer: Option<ConnectInfo<SocketAddr>>, q: Query<HashMap<String, String>>) -> Response {
    let p = Params::from_query(q);
    peers::record(&peers::client_ip(&headers, peer.as_ref()), &peers::client_version(&headers), peers::Hit::Bucket);
    if !conf().opensync {
        return ([(header::CONTENT_TYPE, "application/json; charset=utf-8")], "[]").into_response();
    }
    let Some(key) = p.str("key").map(|s| s.to_string()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match tokio::task::spawn_blocking(move || fdb_key_items(&key)).await {
        Ok(items) => axum::Json(items).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Response page of `/sync/fdb/torrents`.
#[derive(Serialize, Debug)]
pub struct TorrentsPage {
    pub nextread: bool,
    pub countread: i32,
    pub take: i32,
    pub collections: Vec<Collection>,
}

/// Minimal row sent in spidr mode (or for rows older than `start`): sid/pir/url only.
fn slim_row(t: &TorrentDetails) -> TorrentDetails {
    TorrentDetails { sid: t.sid, pir: t.pir, url: t.url.clone(), ..Default::default() }
}

/// Build one `/sync/fdb/torrents` page from an ordered masterDb snapshot.
pub fn build_torrents_page(master: &[(String, MasterDbShard)], time: i64, start: i64, spidr: bool) -> TorrentsPage {
    let c = conf();
    let mut nextread = false;
    let mut countread = 0i32;
    let mut collections = Vec::new();

    for (key, shard) in master.iter().filter(|(_, s)| s.fileTime > time) {
        let mut torrents: IndexMap<String, TorrentDetails> = IndexMap::new();
        for (url, t) in fdb::open_read(key, false, false) {
            if c.disable_trackers.iter().any(|d| d == &t.trackerName) {
                continue;
            }
            if torrents.contains_key(&url) {
                continue;
            }
            if spidr || (start != -1 && start > time::to_file_time_utc(&t.updateTime)) {
                torrents.insert(url, slim_row(&t));
                continue;
            }
            if t.ffprobe.is_none() || t.languages.is_empty() {
                if let Some(streams) = hooks::tracks_get(&t.magnet, &t.types) {
                    let mut t2 = t.clone();
                    t2.languages = hooks::tracks_languages(&t2, Some(&streams)).unwrap_or_default();
                    t2.ffprobe = Some(streams);
                    torrents.insert(url, t2);
                } else {
                    torrents.insert(url, t);
                }
            } else {
                torrents.insert(url, t);
            }
        }

        if !torrents.is_empty() {
            countread += torrents.len() as i32;
            collections.push(Collection {
                Key: key.clone(),
                Value: SyncValue { time: shard.updateTime, fileTime: shard.fileTime, torrents },
            });
        }

        if countread > TAKE {
            nextread = true;
            break;
        }
    }

    TorrentsPage { nextread, countread, take: TAKE, collections }
}

async fn fdb_torrents(headers: HeaderMap, peer: Option<ConnectInfo<SocketAddr>>, q: Query<HashMap<String, String>>) -> Response {
    let p = Params::from_query(q);
    let time = p.i64("time", 0);
    let start = p.i64("start", -1);
    let spidr = p.bool("spidr", false);
    peers::record(&peers::client_ip(&headers, peer.as_ref()), &peers::client_version(&headers), peers::Hit::Torrents { cursor: time, spidr });

    if !conf().opensync || time == 0 {
        return axum::Json(json!({ "nextread": false, "collections": [] })).into_response();
    }

    let res = tokio::task::spawn_blocking(move || {
        let master = sorted_master();
        build_torrents_page(&master, time, start, spidr)
    })
    .await;
    match res {
        Ok(page) => axum::Json(page).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn torrents_legacy() -> axum::Json<serde_json::Value> {
    axum::Json(json!({ "error": "use GET /sync/fdb/torrents" }))
}
