//! Endpoints that exist only in the admin API: `overview`, `logs`, `logs/{name}`.

use chrono::{DateTime, Utc};
use crab_core::config::{self, AppOptions, TRACKER_SLUGS};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::config_api::schema::TRACKER_BLOCK_NAMES;
use crate::controllers::health::job_json;
use crate::version;

/// Log directory served by `logs`.
pub const LOG_DIR: &str = "Data/log";
pub const DEFAULT_TAIL_LINES: usize = 300;
pub const MAX_TAIL_LINES: usize = 5000;

static STARTED: Lazy<Instant> = Lazy::new(Instant::now);

/// Pin the uptime origin (call once at startup).
pub fn mark_started() {
    Lazy::force(&STARTED);
}

fn tracker_display_name(slug: &str) -> &'static str {
    TRACKER_BLOCK_NAMES.iter().find(|n| n.eq_ignore_ascii_case(slug)).copied().unwrap_or("")
}

fn read_checkpoint(path: &str) -> Option<i64> {
    std::fs::read_to_string(path).ok()?.trim_start_matches('\u{feff}').trim().parse().ok()
}

/// Sync checkpoint (file time) as an ISO-8601 UTC string; `None` when unset.
fn checkpoint_iso(file_time: Option<i64>) -> Option<String> {
    let ft = file_time.filter(|t| *t > 0)?;
    let dt = crab_core::time::from_file_time_utc(ft);
    (!crab_core::time::is_min(&dt)).then(|| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn sum_stats_torrents(stats_json: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(stats_json).ok()?;
    let rows = v.as_array()?;
    if rows.is_empty() {
        return None;
    }
    Some(rows.iter().filter_map(|r| r["alltorrents"].as_i64()).sum())
}

/// `GET {admin}/api/overview` payload (blocking: walks masterDb and reads small state files).
pub fn overview(c: &AppOptions) -> Value {
    let now = Utc::now();
    let master = &crab_core::fdb::MASTER_DB;
    let last_update = master.iter().map(|e| e.value().updateTime).max().map(|d| d.format("%d.%m.%Y %H:%M").to_string());
    let active_jobs: Vec<Value> = crab_core::trackers::get_active_jobs().iter().map(|j| job_json(j, now)).collect();
    let with_parse_all: HashSet<&'static str> =
        crab_core::trackers::parse_all_starters().iter().map(|s| s.tracker_name()).collect();
    let trackers: Vec<Value> = TRACKER_SLUGS
        .iter()
        .map(|slug| {
            let parse_all = if with_parse_all.contains(slug) {
                let s = crab_ops::maintenance::resume::describe(slug);
                json!({ "running": s.running, "pending": s.pending, "mapCount": s.mapCount })
            } else {
                Value::Null
            };
            json!({
                "slug": slug,
                "name": tracker_display_name(slug),
                "enabled": !c.is_tracker_disabled(slug),
                "parseAll": parse_all,
            })
        })
        .collect();
    let fmt_sync = |path: &str| checkpoint_iso(read_checkpoint(path));
    let syncapi = c.syncapi.clone().filter(|s| !s.trim().is_empty());
    let info = config::get_config_source_info();
    json!({
        "version": version::VERSION,
        "gitSha": version::GIT_SHA,
        "buildDate": version::BUILD_DATE,
        "uptimeSeconds": STARTED.elapsed().as_secs(),
        "listen": format!("{}:{}", c.listenip, c.listenport),
        "masterDbKeys": master.len(),
        "torrents": sum_stats_torrents(&crab_tracks::stats::read_all_json()),
        "lastUpdateDb": last_update,
        "fastDbKeys": crab_core::index::current_len(),
        "activeJobs": active_jobs,
        "trackers": trackers,
        "sync": {
            "enabled": syncapi.is_some(),
            "syncapi": syncapi,
            "lastsync": fmt_sync(crab_ops::sync::cron::LAST_SYNC_PATH),
            "starsync": fmt_sync(crab_ops::sync::cron::STAR_SYNC_PATH),
        },
        "config": { "path": info.path, "format": info.format },
    })
}

/// Log file names: `[a-z0-9._-]+\.log`, no leading dot.
pub fn is_valid_log_name(name: &str) -> bool {
    name.len() > 4
        && name.len() <= 128
        && name.ends_with(".log")
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

fn modified_utc(meta: &std::fs::Metadata) -> Option<String> {
    let t: DateTime<Utc> = meta.modified().ok()?.into();
    Some(t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// `[{name,size,modified}]` of the regular `*.log` files directly inside `dir`, by name.
/// `GET {admin}/api/logs/fdb` - FileDB change journal settings and disk usage.
pub fn fdb_log_info(c: &AppOptions) -> Value {
    let (files, bytes, oldest, newest) = crab_core::fdb::fdb_log_usage();
    json!({
        "enabled": c.logFdb,
        "retentionDays": c.logFdbRetentionDays,
        "maxSizeMb": c.logFdbMaxSizeMb,
        "maxFiles": c.logFdbMaxFiles,
        "files": files,
        "totalBytes": bytes,
        "oldest": oldest,
        "newest": newest,
    })
}

/// `POST {admin}/api/logs/fdb` - change only the journal keys and save the config the same
/// way the settings editor does (validation, atomic write, hot reload).
pub fn fdb_log_update(body: &Value) -> Result<Value, String> {
    let mut v = crate::config_api::validator::options_to_value(&crate::conf());
    let obj = v.as_object_mut().ok_or("конфиг не является объектом")?;
    if let Some(b) = body.get("enabled") {
        obj.insert("logFdb".into(), Value::Bool(b.as_bool().ok_or("enabled: ожидается true или false")?));
    }
    for (key, field) in [("retentionDays", "logFdbRetentionDays"), ("maxSizeMb", "logFdbMaxSizeMb"), ("maxFiles", "logFdbMaxFiles")] {
        if let Some(x) = body.get(key) {
            let n = x.as_i64().filter(|n| (0..=1_000_000).contains(n)).ok_or_else(|| format!("{key}: ожидается целое число от 0"))?;
            obj.insert(field.into(), json!(n));
        }
    }
    crate::config_api::save_config_object(&v, None)?;
    let c = crate::conf();
    // apply new limits right away instead of waiting for the next journal line
    crab_core::fdb::cleanup_fdb_logs(c.logFdbRetentionDays, c.logFdbMaxSizeMb, c.logFdbMaxFiles);
    Ok(fdb_log_info(&c))
}

pub fn list_logs(dir: &Path) -> Vec<Value> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut items: Vec<(String, Value)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if !is_valid_log_name(&name) {
                return None;
            }
            let meta = std::fs::symlink_metadata(e.path()).ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((name.clone(), json!({ "name": name, "size": meta.len(), "modified": modified_utc(&meta) })))
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items.into_iter().map(|(_, v)| v).collect()
}

/// Regular file `dir/name` (validated name, symlinks refused).
pub fn log_file(dir: &Path, name: &str) -> Option<PathBuf> {
    if !is_valid_log_name(name) {
        return None;
    }
    let p = dir.join(name);
    let meta = std::fs::symlink_metadata(&p).ok()?;
    meta.is_file().then_some(p)
}

/// Last `n` lines of a file, reading backwards in chunks (never loads the whole file).
pub fn tail_lines(path: &Path, n: usize) -> std::io::Result<Vec<String>> {
    const CHUNK: u64 = 64 * 1024;
    const MAX_BYTES: u64 = 16 * 1024 * 1024;
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mut pos = len;
    let mut buf: Vec<u8> = Vec::new();
    while pos > 0 && len - pos < MAX_BYTES {
        let step = CHUNK.min(pos);
        pos -= step;
        f.seek(SeekFrom::Start(pos))?;
        let mut chunk = vec![0u8; step as usize];
        f.read_exact(&mut chunk)?;
        chunk.extend_from_slice(&buf);
        buf = chunk;
        // n lines need n newlines before them (plus a possible trailing newline).
        if buf.iter().filter(|&&b| b == b'\n').count() > n {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    if pos > 0 && !lines.is_empty() {
        lines.remove(0); // partial first line
    }
    let skip = lines.len().saturating_sub(n);
    Ok(lines[skip..].iter().map(|l| l.trim_end_matches('\r').to_string()).collect())
}

/// `lines` query parameter clamped to 1..=MAX_TAIL_LINES.
pub fn tail_count(raw_query: Option<&str>) -> usize {
    raw_query
        .and_then(|q| url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == "lines").map(|(_, v)| v.into_owned()))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|n| n.clamp(1, MAX_TAIL_LINES))
        .unwrap_or(DEFAULT_TAIL_LINES)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("crab-admin-logs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn log_name_validation() {
        for ok in ["rutor.log", "fdb.2025-01-02.log", "sync_spidr.log", "a.log"] {
            assert!(is_valid_log_name(ok), "{ok}");
        }
        for bad in [".log", "..log", ".hidden.log", "../x.log", "a/b.log", "A.log", "x.txt", "x.log.gz", "x .log", "x%2f.log", "", "..%2f.log"]
        {
            assert!(!is_valid_log_name(bad), "{bad}");
        }
    }

    #[test]
    fn list_and_tail() {
        let d = temp_dir("list");
        std::fs::write(d.join("b.log"), "1\n2\n3\n").unwrap();
        std::fs::write(d.join("a.log"), "x").unwrap();
        std::fs::write(d.join("notes.txt"), "x").unwrap();
        std::fs::create_dir_all(d.join("dir.log")).unwrap();
        let names: Vec<String> = list_logs(&d).iter().map(|v| v["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["a.log", "b.log"]);
        assert_eq!(list_logs(&d)[1]["size"], 6);
        assert!(log_file(&d, "b.log").is_some());
        assert!(log_file(&d, "dir.log").is_none());
        assert!(log_file(&d, "../b.log").is_none());
        assert!(log_file(&d, "missing.log").is_none());
        let p = d.join("b.log");
        assert_eq!(tail_lines(&p, 2).unwrap(), ["2", "3"]);
        assert_eq!(tail_lines(&p, 10).unwrap(), ["1", "2", "3"]);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn tail_large_file_reads_backwards() {
        let d = temp_dir("big");
        let p = d.join("big.log");
        let body: String = (0..50_000).map(|i| format!("line {i}\r\n")).collect();
        std::fs::write(&p, body).unwrap();
        let t = tail_lines(&p, 3).unwrap();
        assert_eq!(t, ["line 49997", "line 49998", "line 49999"]);
        assert_eq!(tail_lines(&p, 5000).unwrap().len(), 5000);
        assert_eq!(tail_lines(&p, 5000).unwrap()[0], "line 45000");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn tail_count_param() {
        assert_eq!(tail_count(None), 300);
        assert_eq!(tail_count(Some("lines=20")), 20);
        assert_eq!(tail_count(Some("lines=0")), 1);
        assert_eq!(tail_count(Some("lines=999999")), 5000);
        assert_eq!(tail_count(Some("lines=x")), 300);
    }

    #[test]
    fn checkpoint_as_iso() {
        assert_eq!(checkpoint_iso(None), None);
        assert_eq!(checkpoint_iso(Some(-1)), None);
        assert_eq!(checkpoint_iso(Some(0)), None);
        // 2024-01-01T00:00:00Z as a Windows file time
        assert_eq!(checkpoint_iso(Some(133_485_408_000_000_000)).as_deref(), Some("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn stats_sum() {
        assert_eq!(sum_stats_torrents(r#"[{"alltorrents":2},{"alltorrents":3}]"#), Some(5));
        assert_eq!(sum_stats_torrents("[]"), None);
    }
}
