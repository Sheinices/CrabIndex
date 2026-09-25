//! Tracks data types: TorrServer status DTOs, export/backfill results, stats cache file,
//! plus JSON helpers that keep the on-disk format stable.

use chrono::{DateTime, Local, Utc};
use crab_core::models::{de, FfStream, FfprobeModel};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

// ---------------------------------------------------------------------------
// TorrServer API
// ---------------------------------------------------------------------------

/// Torrent status returned by TorrServer `POST /torrents` (`action=get` / `action=list`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TorrentInfo {
    #[serde(deserialize_with = "de::opt_string")]
    pub title: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub category: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub poster: Option<String>,
    #[serde(deserialize_with = "de::i64")]
    pub timestamp: i64,
    #[serde(deserialize_with = "de::opt_string")]
    pub name: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub hash: Option<String>,
    #[serde(deserialize_with = "de::i32")]
    pub stat: i32,
    #[serde(deserialize_with = "de::opt_string")]
    pub stat_string: Option<String>,
    #[serde(deserialize_with = "de::opt_vec")]
    pub file_stats: Option<Vec<TorrentFileStat>>,
    #[serde(deserialize_with = "de::i32")]
    pub connected_seeders: i32,
    #[serde(deserialize_with = "de::i32")]
    pub active_peers: i32,
    #[serde(deserialize_with = "de::i64")]
    pub download_speed: i64,
    #[serde(deserialize_with = "de::i64")]
    pub bytes_read: i64,
    #[serde(deserialize_with = "de::i64")]
    pub loaded_size: i64,
    #[serde(deserialize_with = "de::i64")]
    pub preloaded_bytes: i64,
    #[serde(deserialize_with = "de::i64")]
    pub preload_size: i64,
}

/// File entry of a TorrServer torrent (1-based ids).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TorrentFileStat {
    #[serde(deserialize_with = "de::i32")]
    pub id: i32,
    #[serde(deserialize_with = "de::opt_string")]
    pub path: Option<String>,
    #[serde(deserialize_with = "de::i64")]
    pub length: i64,
}

impl TorrentFileStat {
    pub fn new(id: i32, path: &str, length: i64) -> Self {
        TorrentFileStat { id, path: Some(path.to_string()), length }
    }
}

// ---------------------------------------------------------------------------
// Export / stats
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TracksExportStats {
    pub total: i32,
    pub filesScanned: i32,
    pub fromTracksFiles: i32,
    pub fromMemory: i32,
    pub fromTorrentDb: i32,
    pub torrentsScanned: i32,
    pub invalidPath: i32,
    pub emptyStreams: i32,
    pub readErrors: i32,
    pub magnetErrors: i32,
    pub torrentDbErrors: i32,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorSample {
    pub hash: String,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TracksExportResult {
    pub outputDir: String,
    pub dryRun: bool,
    pub includeTorrentDb: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<TracksExportStats>,
    pub written: i32,
    pub writeErrors: i32,
    pub errorSamples: Vec<ErrorSample>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TracksExportJobStatus {
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputDir: Option<String>,
    pub includeTorrentDb: bool,
    #[serde(with = "opt_date", skip_serializing_if = "Option::is_none")]
    pub startedAt: Option<DateTime<Utc>>,
    #[serde(with = "opt_date", skip_serializing_if = "Option::is_none")]
    pub completedAt: Option<DateTime<Utc>>,
    pub total: i32,
    pub written: i32,
    pub writeErrors: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<TracksExportStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TracksExportResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TracksBackfillResult {
    pub tracksDir: String,
    pub dryRun: bool,
    pub includeTorrentDb: bool,
    pub migrateLegacy: bool,
    pub stats: TracksExportStats,
    pub written: i32,
    pub migratedLegacy: i32,
    pub skippedExisting: i32,
    pub writeErrors: i32,
    pub errorSamples: Vec<ErrorSample>,
}

/// `Data/temp/tracks-stats.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TracksStatsCacheFile {
    #[serde(with = "date")]
    pub updatedAt: DateTime<Utc>,
    #[serde(deserialize_with = "de::opt_vec")]
    pub entries: Option<Vec<TracksStatsCacheEntry>>,
}

impl Default for TracksStatsCacheFile {
    fn default() -> Self {
        TracksStatsCacheFile { updatedAt: crab_core::time::min(), entries: None }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TracksStatsCacheEntry {
    #[serde(deserialize_with = "de::bool")]
    pub includeTorrentDb: bool,
    pub stats: Option<TracksExportStats>,
}

/// `Data/temp/tracks-index.bz` (gzip JSON).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TracksIndexFile {
    #[serde(with = "date")]
    pub builtAt: DateTime<Utc>,
    #[serde(deserialize_with = "de::opt_vec")]
    pub hashes: Option<Vec<String>>,
}

impl Default for TracksIndexFile {
    fn default() -> Self {
        TracksIndexFile { builtAt: crab_core::time::min(), hashes: None }
    }
}

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

/// UTC timestamp as `yyyy-MM-ddTHH:mm:ss[.fffffff]Z` (up to 7 fractional digits, trailing zeros trimmed).
pub fn format_utc(dt: &DateTime<Utc>) -> String {
    if crab_core::time::is_min(dt) {
        return "0001-01-01T00:00:00".to_string();
    }
    let base = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    let ticks = dt.timestamp_subsec_nanos() / 100;
    if ticks == 0 {
        return format!("{base}Z");
    }
    let frac = format!("{ticks:07}");
    format!("{base}.{}Z", frac.trim_end_matches('0'))
}

/// Local wall-clock `yyyy-MM-dd HH:mm:ss`.
pub fn format_local(dt: &DateTime<Utc>) -> String {
    dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string()
}

pub mod date {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(dt: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format_utc(dt))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        crab_core::time::net::deserialize(d)
    }
}

pub mod opt_date {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(dt: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match dt {
            Some(d) => s.serialize_str(&format_utc(d)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        crab_core::time::net_opt::deserialize(d)
    }
}

// ---------------------------------------------------------------------------
// ffprobe JSON (track files)
// ---------------------------------------------------------------------------

/// Parse a track file / `/ffp` body. Tolerates a UTF-8 BOM and unknown fields.
/// `Ok(None)` for a JSON `null` body.
pub fn parse_ffprobe(text: &str) -> Result<Option<FfprobeModel>, serde_json::Error> {
    serde_json::from_str::<Option<FfprobeModel>>(text.trim_start_matches('\u{feff}'))
}

fn opt_str(v: &Option<String>) -> Value {
    v.as_ref().map(|s| Value::String(s.clone())).unwrap_or(Value::Null)
}

fn opt_i32(v: &Option<i32>) -> Value {
    v.map(|x| json!(x)).unwrap_or(Value::Null)
}

fn stream_value(s: &FfStream) -> Value {
    let mut m = Map::new();
    m.insert("index".into(), json!(s.index));
    m.insert("codec_name".into(), opt_str(&s.codec_name));
    m.insert("codec_long_name".into(), opt_str(&s.codec_long_name));
    m.insert("codec_type".into(), opt_str(&s.codec_type));
    m.insert("width".into(), opt_i32(&s.width));
    m.insert("height".into(), opt_i32(&s.height));
    m.insert("coded_width".into(), opt_i32(&s.coded_width));
    m.insert("coded_height".into(), opt_i32(&s.coded_height));
    m.insert("sample_fmt".into(), opt_str(&s.sample_fmt));
    m.insert("sample_rate".into(), opt_str(&s.sample_rate));
    m.insert("channels".into(), opt_i32(&s.channels));
    m.insert("channel_layout".into(), opt_str(&s.channel_layout));
    m.insert("bit_rate".into(), opt_str(&s.bit_rate));
    let tags = match &s.tags {
        Some(t) => {
            let mut tm = Map::new();
            tm.insert("language".into(), opt_str(&t.language));
            tm.insert("BPS".into(), opt_str(&t.BPS));
            tm.insert("DURATION".into(), opt_str(&t.DURATION));
            tm.insert("title".into(), opt_str(&t.title));
            Value::Object(tm)
        }
        None => Value::Null,
    };
    m.insert("tags".into(), tags);
    Value::Object(m)
}

/// Indented track-file JSON with every property present (nulls included).
pub fn ffprobe_to_json(model: &FfprobeModel) -> String {
    let streams = match &model.streams {
        Some(list) => Value::Array(list.iter().map(stream_value).collect()),
        None => Value::Null,
    };
    let mut root = Map::new();
    root.insert("streams".into(), streams);
    serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_else(|_| "{}".into())
}

/// Write a track file: UTF-8 with BOM, indented JSON.
pub fn write_track_file(path: &std::path::Path, model: &FfprobeModel) -> std::io::Result<()> {
    let mut bytes = Vec::with_capacity(4096);
    bytes.extend_from_slice(b"\xEF\xBB\xBF");
    bytes.extend_from_slice(ffprobe_to_json(model).as_bytes());
    std::fs::write(path, bytes)
}

/// Write text as UTF-8 with BOM (manifests).
pub fn write_text_bom(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let mut bytes = Vec::with_capacity(text.len() + 3);
    bytes.extend_from_slice(b"\xEF\xBB\xBF");
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(path, bytes)
}
