//! In-memory track cache + lookups (streams by magnet, languages, track presence).

use crab_core::conf;
use crab_core::log::{self, cat, Level};
use crab_core::models::{FfStream, FfprobeModel, TorrentDetails};
use dashmap::DashMap;
use indexmap::IndexSet;
use once_cell::sync::Lazy;
use std::path::Path;
use std::sync::Arc;

use super::logging;
use super::paths::{self, is_valid_infohash, normalize_infohash};
use super::{index, models};

/// infohash (lowercase) → probe result, filled on demand from disk and by the analyzer.
pub static DATABASE: Lazy<DashMap<String, Arc<FfprobeModel>>> = Lazy::new(DashMap::new);

/// Content types that are never analyzed. Empty → true.
pub fn the_bad(types: &[String]) -> bool {
    if types.is_empty() {
        return true;
    }
    types.iter().any(|t| t == "sport" || t == "tvshow" || t == "docuserial")
}

/// Lowercase v1 infohash from a magnet link.
pub fn infohash_from_magnet(magnet: &str) -> Option<String> {
    crab_core::util::magnet_infohash(magnet).map(|h| normalize_infohash(&h))
}

fn has_streams(m: &FfprobeModel) -> bool {
    m.streams.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
}

/// Cached streams for a magnet. `types = None` skips the content-type filter.
/// `memory_only` returns only what is already cached in memory.
pub fn get(magnet: &str, types: Option<&[String]>, memory_only: bool) -> Option<Vec<FfStream>> {
    if let Some(t) = types {
        if the_bad(t) {
            return None;
        }
    }
    let infohash = infohash_from_magnet(magnet)?;
    if !is_valid_infohash(&infohash) {
        return None;
    }
    if let Some(res) = DATABASE.get(&infohash) {
        return res.streams.clone();
    }
    if memory_only {
        return None;
    }
    let path = paths::resolve_track_path(&infohash)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let model = models::parse_ffprobe(&text).ok().flatten()?;
    if !has_streams(&model) {
        return None;
    }
    let streams = model.streams.clone();
    DATABASE.insert(infohash.clone(), Arc::new(model));
    index::register_track_hash(&infohash);
    streams
}

/// Torrent languages plus audio stream languages; `None` when empty.
pub fn languages(t: &TorrentDetails, streams: Option<&[FfStream]>) -> Option<IndexSet<String>> {
    let mut langs: IndexSet<String> = t.languages.iter().cloned().collect();
    if let Some(streams) = streams {
        for s in streams {
            if s.codec_type.as_deref() != Some("audio") {
                continue;
            }
            if let Some(l) = s.tags.as_ref().and_then(|t| t.language.as_ref()).filter(|l| !l.is_empty()) {
                langs.insert(l.clone());
            }
        }
    }
    (!langs.is_empty()).then_some(langs)
}

pub fn has_track_on_disk(infohash: &str) -> bool {
    let h = normalize_infohash(infohash);
    index::contains(&h) || paths::resolve_track_path(&h).is_some()
}

/// Track known for this torrent (row ffprobe, memory, index or a non-empty file on disk).
pub fn has_track_for_torrent(t: &TorrentDetails) -> bool {
    if t.ffprobe.as_ref().map(|f| !f.is_empty()).unwrap_or(false) {
        return true;
    }
    if t.magnet.is_empty() {
        return false;
    }
    let Some(infohash) = infohash_from_magnet(&t.magnet) else { return false };
    if DATABASE.get(&infohash).map(|m| has_streams(&m)).unwrap_or(false) {
        return true;
    }
    if index::contains(&infohash) {
        return true;
    }
    match paths::resolve_track_path(&infohash) {
        Some(p) => paths::track_file_has_streams(Path::new(&p)),
        None => false,
    }
}

/// Stats can run once the index is loaded, or when there is nothing to index.
pub fn is_track_index_ready_for_stats() -> bool {
    if index::track_index_count() > 0 {
        return true;
    }
    let dir = Path::new(paths::TRACKS_DIR);
    if !dir.exists() {
        return true;
    }
    match std::fs::read_dir(dir) {
        Ok(mut rd) => !rd.any(|e| e.map(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).unwrap_or(false)),
        Err(_) => false,
    }
}

/// Next attempt counter after a failed probe (HTTP 400 is not terminal).
pub fn next_failure_attempt(current_attempt: i32) -> i32 {
    current_attempt + 1
}

pub fn log_analysis_failure(typetask: i32, infohash: &str, api_status_code: i32, remaining: i32, error_message: Option<&str>) {
    let detail = format!(
        "Анализ треков для {infohash} без результата. Код ответа API: {api_status_code}. Осталось {remaining} попыток."
    );
    if !log::settings().tracks_console_detail {
        let body = format!(
            "[task:{typetask}] hash={infohash} code={api_status_code} remaining={remaining} msg={}",
            failure_msg_key(error_message, api_status_code)
        );
        log::write(cat::TRACKS, Level::Warning, body);
        if conf().trackslog {
            logging::log_to_file(&detail, Some(typetask));
        }
        return;
    }
    logging::tlog(detail, Some(typetask));
}

fn failure_msg_key(error_message: Option<&str>, api_status_code: i32) -> &'static str {
    if let Some(m) = error_message.filter(|m| !m.is_empty()) {
        if m.contains("Нет данных") {
            return "no_track_data";
        }
        if m.to_lowercase().contains("таймаут") {
            return "timeout";
        }
        if m.contains("JSON") {
            return "json_error";
        }
    }
    match api_status_code {
        400 => "no_track_data",
        408 => "timeout",
        _ => "error",
    }
}

/// Hook implementation registered with core.
pub struct Lookup;

impl crab_core::hooks::TracksLookup for Lookup {
    fn get(&self, magnet: &str, types: &[String], memory_only: bool) -> Option<Vec<FfStream>> {
        // An empty list means "types unknown" on the core model, so the filter is skipped.
        let types = (!types.is_empty()).then_some(types);
        get(magnet, types, memory_only)
    }

    fn languages(&self, t: &TorrentDetails, streams: Option<&[FfStream]>) -> Option<IndexSet<String>> {
        languages(t, streams)
    }

    fn the_bad(&self, types: &[String]) -> bool {
        the_bad(types)
    }

    fn has_track_for_torrent(&self, t: &TorrentDetails) -> bool {
        has_track_for_torrent(t)
    }
}
