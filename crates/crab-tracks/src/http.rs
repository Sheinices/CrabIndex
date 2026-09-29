// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! HTTP endpoints: `/stats/{torrents,tracks,meta}` and the tracks admin actions under `/dev/`.

use axum::extract::Query;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use crab_core::conf;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::stats;
use crate::tracks::export::{self, DEFAULT_EXPORT_DIR};
use crate::tracks::models::{format_local, format_utc};
use crate::tracks::{paths, stats_cache};

pub fn router() -> Router {
    Router::new()
        .route("/stats/torrents", any(stats_torrents))
        .route("/stats/tracks", any(stats_tracks))
        .route("/stats/meta", any(stats_meta))
        .route("/dev/tracksstats", any(dev_tracks_stats))
        .route("/dev/exporttracks", any(dev_export_tracks))
        .route("/dev/exporttracksstatus", any(dev_export_tracks_status))
        .route("/dev/backfilltracks", any(dev_backfill_tracks))
}

/// Case-insensitive `true`/`false`; anything else falls back to `default`.
fn qbool(v: &Option<String>, default: bool) -> bool {
    match v.as_deref().map(str::trim) {
        Some(s) if s.eq_ignore_ascii_case("true") => true,
        Some(s) if s.eq_ignore_ascii_case("false") => false,
        _ => default,
    }
}

fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        "{\"error\":\"internal server error\"}",
    )
        .into_response()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(f).await.ok()
}

/// Object with `None` values dropped.
fn obj(pairs: Vec<(&str, Option<Value>)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        if let Some(v) = v {
            m.insert(k.to_string(), v);
        }
    }
    Value::Object(m)
}

fn date_value(dt: Option<chrono::DateTime<chrono::Utc>>) -> Option<Value> {
    dt.map(|d| Value::String(format_utc(&d)))
}

// ---------------------------------------------------------------------------
// /stats
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct StatsQuery {
    format: Option<String>,
}

/// `/stats/torrents` rows as CSV (`format=csv`): one line per tracker, UTF-8 with BOM so
/// spreadsheets open it directly.
pub fn stats_csv(json: &str) -> String {
    let rows: Vec<Value> = serde_json::from_str(json).unwrap_or_default();
    let mut out = String::from("\u{feff}tracker,alltorrents,newtor,update,check,lastnewtor,tracks_wait,tracks_confirm,tracks_skip\n");
    for r in rows {
        let cell = |v: &Value| match v {
            Value::String(s) => {
                let s = s.replace('"', "\"\"");
                if s.contains([',', '"', '\n']) { format!("\"{s}\"") } else { s }
            }
            Value::Null => String::new(),
            other => other.to_string(),
        };
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            cell(&r["trackerName"]),
            cell(&r["alltorrents"]),
            cell(&r["newtor"]),
            cell(&r["update"]),
            cell(&r["check"]),
            cell(&r["lastnewtor"]),
            cell(&r["tracks"]["wait"]),
            cell(&r["tracks"]["confirm"]),
            cell(&r["tracks"]["skip"]),
        ));
    }
    out
}

async fn stats_torrents(Query(q): Query<StatsQuery>) -> Response {
    let body = if conf().openstats { blocking(stats::read_all_json).await.unwrap_or_else(|| "[]".into()) } else { "[]".into() };
    if q.format.as_deref().map(|f| f.eq_ignore_ascii_case("csv")).unwrap_or(false) {
        let csv = stats_csv(&body);
        return (
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
                (header::CONTENT_DISPOSITION, "attachment; filename=\"crabindex-trackers.csv\"".to_string()),
            ],
            csv,
        )
            .into_response();
    }
    ([(header::CONTENT_TYPE, "application/json; charset=utf-8")], body).into_response()
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TracksStatsQuery {
    includetorrentdb: Option<String>,
    refresh: Option<String>,
}

/// `{ ok, updatedAt, fromCache, stats }` for the tracks export stats.
pub fn tracks_stats_payload(include_torrent_db: bool, refresh: bool) -> Value {
    let stats = stats_cache::get_export_stats(include_torrent_db, refresh);
    obj(vec![
        ("ok", Some(json!(true))),
        ("updatedAt", date_value(stats_cache::stats_cache_updated_at())),
        ("fromCache", Some(json!(stats_cache::last_export_stats_from_cache()))),
        ("stats", Some(json!(stats))),
    ])
}

async fn stats_tracks(Query(q): Query<TracksStatsQuery>) -> Response {
    if !conf().openstats {
        return Json(json!({ "ok": false })).into_response();
    }
    let include = qbool(&q.includetorrentdb, true);
    match blocking(move || tracks_stats_payload(include, false)).await {
        Some(v) => Json(v).into_response(),
        None => internal_error(),
    }
}

async fn stats_meta() -> Response {
    if !conf().openstats {
        return Json(json!({ "ok": false })).into_response();
    }
    let updated_at = match stats::last_collected_at() {
        Some(d) => Some(d),
        None => blocking(stats::try_read_stats_meta_updated_at).await.flatten(),
    };
    Json(obj(vec![
        ("ok", Some(json!(true))),
        ("updatedAt", date_value(updated_at)),
        ("updatedAtLocal", updated_at.map(|d| Value::String(format_local(&d)))),
        ("tracksStatsUpdatedAt", date_value(stats_cache::stats_cache_updated_at())),
    ]))
    .into_response()
}

// ---------------------------------------------------------------------------
// /dev tracks admin
// ---------------------------------------------------------------------------

/// Tracks admin operations behind the `/dev/*tracks*` endpoints (all blocking).
pub mod admin {
    use super::*;

    /// Stats for `Data/tracks` files + FileDB `ffprobe` fields.
    pub fn tracks_stats(include_torrent_db: bool, refresh: bool) -> Value {
        tracks_stats_payload(include_torrent_db, refresh)
    }

    /// Export all tracks to `dir` (`{aa}/{b}/{hash}.json`). `dry_run` = stats only;
    /// otherwise runs in the background unless `background` is false.
    pub fn export_tracks(dir: &str, dry_run: bool, include_torrent_db: bool, background: bool) -> Result<Value, export::InvalidOutputDir> {
        if dry_run || !background {
            let result = export::export_all(dir, dry_run, include_torrent_db, None)?;
            return Ok(json!({ "ok": true, "result": result }));
        }
        if !export::try_start_export(dir, include_torrent_db)? {
            return Ok(json!({ "ok": false, "alreadyRunning": true, "status": export::get_export_job_status() }));
        }
        Ok(json!({ "ok": true, "started": true, "status": export::get_export_job_status() }))
    }

    pub fn export_tracks_status() -> Value {
        json!({ "ok": true, "status": export::get_export_job_status() })
    }

    /// Backfill `Data/tracks` (legacy migration + missing tracks from FileDB).
    pub fn backfill_tracks(dry_run: bool, migrate_legacy: bool, include_torrent_db: bool) -> Value {
        let result = export::backfill_tracks(paths::TRACKS_DIR, dry_run, include_torrent_db, migrate_legacy);
        json!({ "ok": true, "result": result })
    }
}

async fn dev_tracks_stats(Query(q): Query<TracksStatsQuery>) -> Response {
    let include = qbool(&q.includetorrentdb, true);
    let refresh = qbool(&q.refresh, false);
    match blocking(move || admin::tracks_stats(include, refresh)).await {
        Some(v) => Json(v).into_response(),
        None => internal_error(),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ExportQuery {
    dir: Option<String>,
    dryrun: Option<String>,
    includetorrentdb: Option<String>,
    background: Option<String>,
}

async fn dev_export_tracks(Query(q): Query<ExportQuery>) -> Response {
    let dir = q.dir.clone().unwrap_or_else(|| DEFAULT_EXPORT_DIR.to_string());
    let dry_run = qbool(&q.dryrun, false);
    let include = qbool(&q.includetorrentdb, true);
    let background = qbool(&q.background, true);
    match blocking(move || admin::export_tracks(&dir, dry_run, include, background)).await {
        Some(Ok(v)) => Json(v).into_response(),
        _ => internal_error(),
    }
}

async fn dev_export_tracks_status() -> Response {
    Json(admin::export_tracks_status()).into_response()
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BackfillQuery {
    dryrun: Option<String>,
    migratelegacy: Option<String>,
    includetorrentdb: Option<String>,
}

async fn dev_backfill_tracks(Query(q): Query<BackfillQuery>) -> Response {
    let dry_run = qbool(&q.dryrun, false);
    let migrate = qbool(&q.migratelegacy, true);
    let include = qbool(&q.includetorrentdb, true);
    match blocking(move || admin::backfill_tracks(dry_run, migrate, include)).await {
        Some(v) => Json(v).into_response(),
        None => internal_error(),
    }
}

#[cfg(test)]
mod csv_tests {
    use super::*;

    #[test]
    fn stats_csv_escapes_and_orders_columns() {
        let json = r#"[{"trackerName":"ru,tor","lastnewtor":"27.09.2026","newtor":7,"update":42,"check":43,"alltorrents":1490788,"tracks":{"wait":1,"confirm":2,"skip":0}}]"#;
        let csv = stats_csv(json);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "\u{feff}tracker,alltorrents,newtor,update,check,lastnewtor,tracks_wait,tracks_confirm,tracks_skip");
        assert_eq!(lines[1], "\"ru,tor\",1490788,7,42,43,27.09.2026,1,2,0");
        assert_eq!(stats_csv("[]").lines().count(), 1);
    }
}
