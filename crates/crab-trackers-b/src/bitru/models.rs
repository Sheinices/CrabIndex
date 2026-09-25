//! BitRu `api.php?get=torrents` response shapes. The documented error shape is
//! `{"error":"message"}`; a legacy boolean `error` is also accepted.

use crab_core::models::de;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BitruApiResponse {
    /// Bool (legacy) or string message.
    pub error: Value,
    #[serde(deserialize_with = "de::opt_string")]
    pub message: Option<String>,
    pub result: Option<BitruApiResult>,
}

impl BitruApiResponse {
    pub fn has_error(&self) -> bool {
        match &self.error {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::String(s) => !s.trim().is_empty(),
            _ => true,
        }
    }

    pub fn error_message(&self) -> Option<String> {
        match &self.error {
            Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
            _ => self.message.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BitruApiResult {
    /// max(added) on this page (unix; string or number). A request `after_date=X` returns
    /// items with added < X (older-than); the official docs label is inverted.
    pub after_date: Value,
    /// min(added) on this page (unix; string or number). Send this value as the next
    /// request's `after_date` to get the next older page.
    pub before_date: Value,
    pub items: Option<Vec<BitruApiItemWrapper>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BitruApiItemWrapper {
    pub item: Option<BitruApiItemInner>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BitruApiItemInner {
    pub torrent: Option<BitruApiTorrent>,
    pub info: Option<BitruApiInfo>,
    pub template: Option<BitruApiTemplate>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BitruApiTorrent {
    #[serde(deserialize_with = "de::i64")]
    pub id: i64,
    #[serde(deserialize_with = "de::i64")]
    pub added: i64,
    #[serde(deserialize_with = "de::i64")]
    pub size: i64,
    #[serde(deserialize_with = "de::i32")]
    pub leechers: i32,
    #[serde(deserialize_with = "de::i32")]
    pub seeders: i32,
    #[serde(deserialize_with = "de::opt_string")]
    pub file: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BitruApiInfo {
    #[serde(deserialize_with = "de::opt_string")]
    pub name: Option<String>,
    /// Release year: number (2020) or range string ("2011-2015").
    pub year: Value,
    #[serde(deserialize_with = "de::opt_vec")]
    pub country: Option<Vec<String>>,
    #[serde(deserialize_with = "de::opt_string")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BitruApiTemplate {
    #[serde(deserialize_with = "de::opt_string")]
    pub category: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub section: Option<String>,
    #[serde(deserialize_with = "de::opt_vec")]
    pub subsection: Option<Vec<String>>,
    #[serde(deserialize_with = "de::opt_string")]
    pub orig_name: Option<String>,
    pub video: Option<BitruApiVideo>,
    #[serde(deserialize_with = "de::opt_string")]
    pub other: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BitruApiVideo {
    #[serde(deserialize_with = "de::opt_string")]
    pub quality: Option<String>,
}
