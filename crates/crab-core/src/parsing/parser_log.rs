//! Per-tracker parser log files `Data/log/{tracker}.log`.

use std::io::Write;

use crate::models::TorrentDetails;

const LOG_DIR: &str = "Data/log";
const MAX_NAME_LENGTH: usize = 50;
const MAX_TITLE_LENGTH: usize = 60;

fn sanitize(tracker: &str) -> String {
    if tracker.trim().is_empty() {
        return "unknown".into();
    }
    let s: String = tracker
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    let s = s.trim_start_matches('.').to_string();
    if s.is_empty() {
        "unknown".into()
    } else {
        s
    }
}

fn append(tracker: &str, line: &str) {
    if !crate::conf().tracker_log_enabled(tracker) {
        return;
    }
    let _ = std::fs::create_dir_all(LOG_DIR);
    let path = format!("{LOG_DIR}/{}.log", sanitize(tracker));
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            let _ = writeln!(f, "[{}] {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
        }
        Err(e) => crate::log::error(crate::log::cat::PARSER, format!("I/O error while writing tracker log for '{tracker}': {e}")),
    }
}

/// Append a line to the tracker log.
pub fn write(tracker: &str, message: impl AsRef<str>) {
    append(tracker, message.as_ref());
}

/// Append "message | k=v, k2=v2".
pub fn write_kv(tracker: &str, message: &str, data: &[(String, String)]) {
    if data.is_empty() {
        append(tracker, message);
        return;
    }
    let kv: Vec<String> = data.iter().map(|(k, v)| format!("{k}={v}")).collect();
    append(tracker, &format!("{message} | {}", kv.join(", ")));
}

pub fn write_stats(tracker: &str, message: &str, parsed: i32, processed: i32, updated: i32, failed: i32) {
    let mut d = Vec::new();
    if parsed > 0 {
        d.push(("parsed".to_string(), parsed.to_string()));
    }
    if processed > 0 {
        d.push(("processed".to_string(), processed.to_string()));
    }
    if updated > 0 {
        d.push(("updated".to_string(), updated.to_string()));
    }
    if failed > 0 {
        d.push(("failed".to_string(), failed.to_string()));
    }
    write_kv(tracker, message, &d);
}

fn cut(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}...", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

fn torrent_data(action: &str, t: &TorrentDetails, reason: Option<&str>) -> Vec<(String, String)> {
    let mut d = vec![("action".to_string(), action.to_string())];
    if !t.url.trim().is_empty() {
        d.push(("url".into(), t.url.clone()));
    }
    if !t.title.trim().is_empty() {
        d.push(("title".into(), cut(&t.title, MAX_TITLE_LENGTH)));
    }
    if let Some(r) = reason.filter(|r| !r.trim().is_empty()) {
        d.push(("reason".into(), r.to_string()));
    }
    if !t.name.trim().is_empty() {
        d.push(("name".into(), cut(&t.name, MAX_NAME_LENGTH)));
    }
    if !t.originalname.trim().is_empty() {
        d.push(("originalname".into(), cut(&t.originalname, MAX_NAME_LENGTH)));
    }
    if !t._sn.trim().is_empty() {
        d.push(("_sn".into(), t._sn.clone()));
    }
    if !t._so.trim().is_empty() {
        d.push(("_so".into(), t._so.clone()));
    }
    if !t.magnet.trim().is_empty() {
        let h = crate::rx::group_i(&t.magnet, "btih:([a-fA-F0-9]{40})", 1);
        d.push(("magnet".into(), if h.is_empty() { "yes".into() } else { h }));
    }
    if !crate::time::is_min(&t.createTime) {
        d.push(("createTime".into(), t.createTime.format("%Y-%m-%d").to_string()));
    }
    if !crate::time::is_min(&t.updateTime) {
        d.push(("updateTime".into(), t.updateTime.format("%Y-%m-%d %H:%M:%S").to_string()));
    }
    if !t.sizeName.trim().is_empty() {
        d.push(("size".into(), t.sizeName.clone()));
    }
    if !t.types.is_empty() {
        d.push(("types".into(), t.types.join(",")));
    }
    d
}

pub fn write_added(tracker: &str, t: &TorrentDetails) {
    write_kv(tracker, "Torrent added", &torrent_data("added", t, None));
}

pub fn write_updated(tracker: &str, t: &TorrentDetails, reason: Option<&str>) {
    write_kv(tracker, "Torrent updated", &torrent_data("updated", t, reason));
}

pub fn write_skipped(tracker: &str, t: &TorrentDetails, reason: Option<&str>) {
    write_kv(tracker, "Torrent skipped", &torrent_data("skipped", t, reason));
}

pub fn write_failed(tracker: &str, t: &TorrentDetails, reason: Option<&str>) {
    write_kv(tracker, "Torrent failed", &torrent_data("failed", t, reason));
}
