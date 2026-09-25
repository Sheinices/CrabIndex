//! Shared data models (FileDB rows, ffprobe streams, parse task slots, API DTOs, sync v2).
//!
//! JSON field names are fixed by the on-disk format, the sync protocol and clients
//! (Lampa, Sonarr…). String fields use "" for "no value" and are omitted from JSON when empty.

use chrono::{DateTime, Utc};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use crate::time;

/// Lenient deserializers: a wrongly typed field falls back to its default instead of failing the row.
pub mod de {
    use indexmap::IndexSet;
    use serde::{Deserialize, Deserializer};
    use serde_json::Value;
    use std::hash::Hash;

    pub fn string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::String(s) => s,
            Value::Null => String::new(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => String::new(),
        })
    }

    pub fn opt_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::String(s) => Some(s),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        })
    }

    fn value_i64(v: &Value) -> Option<i64> {
        match v {
            Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
            Value::String(s) => s.trim().parse::<i64>().ok().or_else(|| s.trim().parse::<f64>().ok().map(|f| f as i64)),
            Value::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }

    pub fn i32<'de, D: Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
        Ok(value_i64(&Value::deserialize(d)?).unwrap_or(0) as i32)
    }

    pub fn opt_i32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i32>, D::Error> {
        Ok(value_i64(&Value::deserialize(d)?).map(|x| x as i32))
    }

    pub fn i64<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
        Ok(value_i64(&Value::deserialize(d)?).unwrap_or(0))
    }

    pub fn f64<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            Value::String(s) => s.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        })
    }

    pub fn bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::Bool(b) => b,
            Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
            Value::String(s) => s.eq_ignore_ascii_case("true") || s == "1",
            _ => false,
        })
    }

    pub fn vec_string<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::Array(a) => a
                .into_iter()
                .filter_map(|x| match x {
                    Value::String(s) => Some(s),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect(),
            Value::String(s) => vec![s],
            _ => Vec::new(),
        })
    }

    pub fn set<'de, D, T>(d: D) -> Result<IndexSet<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: serde::de::DeserializeOwned + Eq + Hash,
    {
        Ok(match Value::deserialize(d)? {
            Value::Array(a) => a.into_iter().filter_map(|x| serde_json::from_value(x).ok()).collect(),
            _ => IndexSet::new(),
        })
    }

    pub fn opt_vec<'de, D, T>(d: D) -> Result<Option<Vec<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: serde::de::DeserializeOwned,
    {
        Ok(match Value::deserialize(d)? {
            Value::Array(a) => Some(a.into_iter().filter_map(|x| serde_json::from_value(x).ok()).collect()),
            _ => None,
        })
    }
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

// ---------------------------------------------------------------------------
// ffprobe
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FfTags {
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub BPS: Option<String>,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub DURATION: Option<String>,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FfStream {
    #[serde(deserialize_with = "de::i32")]
    pub index: i32,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub codec_name: Option<String>,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub codec_long_name: Option<String>,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub codec_type: Option<String>,
    #[serde(deserialize_with = "de::opt_i32", skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(deserialize_with = "de::opt_i32", skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    #[serde(deserialize_with = "de::opt_i32", skip_serializing_if = "Option::is_none")]
    pub coded_width: Option<i32>,
    #[serde(deserialize_with = "de::opt_i32", skip_serializing_if = "Option::is_none")]
    pub coded_height: Option<i32>,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub sample_fmt: Option<String>,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<String>,
    #[serde(deserialize_with = "de::opt_i32", skip_serializing_if = "Option::is_none")]
    pub channels: Option<i32>,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub channel_layout: Option<String>,
    #[serde(deserialize_with = "de::opt_string", skip_serializing_if = "Option::is_none")]
    pub bit_rate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<FfTags>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FfprobeModel {
    #[serde(deserialize_with = "de::opt_vec", skip_serializing_if = "Option::is_none")]
    pub streams: Option<Vec<FfStream>>,
}

// ---------------------------------------------------------------------------
// Torrent row (TorrentBaseDetails + TorrentDetails)
// ---------------------------------------------------------------------------

/// One FileDB row. Parsers fill the "base" part (tracker, url, title, sid/pir, sizeName,
/// magnet, name/originalname, relased, createTime); FileDB derives size/quality/videotype/
/// voices/languages/seasons in [`crate::fdb::update_full_details`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TorrentDetails {
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub trackerName: String,
    #[serde(deserialize_with = "de::vec_string", skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(deserialize_with = "de::i32")]
    pub sid: i32,
    #[serde(deserialize_with = "de::i32")]
    pub pir: i32,
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub sizeName: String,
    #[serde(with = "time::net")]
    pub createTime: DateTime<Utc>,
    #[serde(with = "time::net")]
    pub updateTime: DateTime<Utc>,
    #[serde(with = "time::net")]
    pub checkTime: DateTime<Utc>,
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub magnet: String,
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub originalname: String,
    #[serde(deserialize_with = "de::i32")]
    pub relased: i32,
    #[serde(deserialize_with = "de::set", skip_serializing_if = "IndexSet::is_empty")]
    pub languages: IndexSet<String>,
    #[serde(deserialize_with = "de::opt_vec", skip_serializing_if = "Option::is_none")]
    pub ffprobe: Option<Vec<FfStream>>,
    #[serde(deserialize_with = "de::i32", skip_serializing_if = "is_zero_i32")]
    pub ffprobe_tryingdata: i32,
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub _sn: String,
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub _so: String,

    // --- TorrentDetails (derived) ---
    #[serde(deserialize_with = "de::f64")]
    pub size: f64,
    #[serde(deserialize_with = "de::i32")]
    pub quality: i32,
    #[serde(deserialize_with = "de::string", skip_serializing_if = "String::is_empty")]
    pub videotype: String,
    #[serde(deserialize_with = "de::set")]
    pub voices: IndexSet<String>,
    #[serde(deserialize_with = "de::set")]
    pub seasons: IndexSet<i32>,
}

impl Default for TorrentDetails {
    fn default() -> Self {
        let now = time::now();
        TorrentDetails {
            trackerName: String::new(),
            types: Vec::new(),
            url: String::new(),
            title: String::new(),
            sid: 0,
            pir: 0,
            sizeName: String::new(),
            createTime: now,
            updateTime: now,
            checkTime: now,
            magnet: String::new(),
            name: String::new(),
            originalname: String::new(),
            relased: 0,
            languages: IndexSet::new(),
            ffprobe: None,
            ffprobe_tryingdata: 0,
            _sn: String::new(),
            _so: String::new(),
            size: 0.0,
            quality: 0,
            videotype: String::new(),
            voices: IndexSet::new(),
            seasons: IndexSet::new(),
        }
    }
}

impl TorrentDetails {
    /// Convenience constructor for parsers.
    pub fn new(tracker: &str, types: &[&str], url: impl Into<String>, title: impl Into<String>) -> Self {
        TorrentDetails {
            trackerName: tracker.to_string(),
            types: types.iter().map(|s| s.to_string()).collect(),
            url: url.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn has_type(&self, t: &str) -> bool {
        self.types.iter().any(|x| x == t)
    }
}

impl AsRef<TorrentDetails> for TorrentDetails {
    fn as_ref(&self) -> &TorrentDetails {
        self
    }
}

impl AsMut<TorrentDetails> for TorrentDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        self
    }
}

// ---------------------------------------------------------------------------
// masterDb / parse tasks
// ---------------------------------------------------------------------------

/// Per-shard metadata in masterDb (update time + file ordering key).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterDbShard {
    #[serde(with = "time::net")]
    pub updateTime: DateTime<Utc>,
    #[serde(deserialize_with = "de::i64")]
    pub fileTime: i64,
}

impl Default for MasterDbShard {
    fn default() -> Self {
        MasterDbShard { updateTime: time::min(), fileTime: 0 }
    }
}

/// Page slot for UpdateTasksParse / ParseAllTask (`Data/temp/{tracker}_taskParse.json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskParse {
    #[serde(with = "time::net")]
    pub updateTime: DateTime<Utc>,
    /// ParseAllTask cycle id when this page was last completed in a full crawl.
    #[serde(deserialize_with = "de::opt_string")]
    pub parseAllCycleId: Option<String>,
    /// Consecutive ParseAllTask failures for this slot.
    #[serde(deserialize_with = "de::i32")]
    pub parseAllFailCount: i32,
    #[serde(deserialize_with = "de::i32")]
    pub page: i32,
}

impl Default for TaskParse {
    fn default() -> Self {
        TaskParse { updateTime: time::min(), parseAllCycleId: None, parseAllFailCount: 0, page: 0 }
    }
}

impl TaskParse {
    pub fn new(page: i32) -> Self {
        TaskParse { page, ..Default::default() }
    }
}

/// Flat slot map: category → pages.
pub type TaskMap = IndexMap<String, Vec<TaskParse>>;
/// Nested slot map: category → arg → pages.
pub type NestedTaskMap = IndexMap<String, IndexMap<String, Vec<TaskParse>>>;

// ---------------------------------------------------------------------------
// API DTOs (native /api/v1.0/torrents & Jackett JSON)
// ---------------------------------------------------------------------------

pub mod api {
    use super::*;

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    pub struct TorrentInfo {
        pub quality: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub videotype: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub voices: Option<IndexSet<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub seasons: Option<IndexSet<i32>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub types: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub sizeName: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub originalname: Option<String>,
        pub relased: i32,
    }

    /// Jackett-compatible result row.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Result {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub Tracker: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub Details: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub Title: Option<String>,
        pub Size: f64,
        #[serde(with = "time::net")]
        pub PublishDate: DateTime<Utc>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub Category: Option<IndexSet<i32>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub CategoryDesc: Option<String>,
        pub Seeders: i32,
        pub Peers: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub MagnetUri: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub ffprobe: Option<Vec<FfStream>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub languages: Option<IndexSet<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub info: Option<TorrentInfo>,
    }

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    pub struct RootObject {
        pub Results: Vec<Result>,
        pub jacred: bool,
    }
}

// ---------------------------------------------------------------------------
// Sync v2 (/sync/fdb/torrents)
// ---------------------------------------------------------------------------

pub mod sync {
    use super::*;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(default)]
    pub struct Value {
        #[serde(with = "time::net")]
        pub time: DateTime<Utc>,
        #[serde(deserialize_with = "de::i64")]
        pub fileTime: i64,
        pub torrents: IndexMap<String, TorrentDetails>,
    }

    impl Default for Value {
        fn default() -> Self {
            Value { time: time::min(), fileTime: 0, torrents: IndexMap::new() }
        }
    }

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    #[serde(default)]
    pub struct Collection {
        pub Key: String,
        pub Value: Value,
    }

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    #[serde(default)]
    pub struct RootObject {
        pub nextread: bool,
        pub take: i32,
        pub countread: i32,
        pub collections: Vec<Collection>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torrent_roundtrip_lenient() {
        let j = r#"{"trackerName":"rutor","types":["movie"],"url":"http://rutor.info/torrent/1","title":"T","sid":"5","pir":null,
            "sizeName":"1.5 GB","createTime":"2023-01-02T03:04:05.1234567+03:00","updateTime":"2023-01-02T03:04:05Z",
            "checkTime":"0001-01-01T00:00:00","magnet":"magnet:?xt=urn:btih:abc","name":"n","originalname":null,"relased":2020,
            "languages":["rus"],"ffprobe":null,"voices":["LostFilm"],"seasons":[1,2],"size":1610612736.0,"quality":1080,"videotype":"sdr","_sn":"n","_so":"n"}"#;
        let t: TorrentDetails = serde_json::from_str(j).unwrap();
        assert_eq!(t.sid, 5);
        assert_eq!(t.pir, 0);
        assert_eq!(t.originalname, "");
        assert!(time::is_min(&t.checkTime));
        let back = serde_json::to_string(&t).unwrap();
        let t2: TorrentDetails = serde_json::from_str(&back).unwrap();
        assert_eq!(t2.createTime, t.createTime);
        assert_eq!(t2.seasons.len(), 2);
    }
}
