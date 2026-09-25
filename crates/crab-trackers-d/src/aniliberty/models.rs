//! aniliberty.top `/api/v1/anime/torrents` response shapes.

use crab_core::models::de;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnilibertyApiResponse {
    #[serde(deserialize_with = "de::opt_vec")]
    pub data: Option<Vec<AnilibertyTorrent>>,
    pub meta: Option<AnilibertyMeta>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnilibertyMeta {
    #[serde(deserialize_with = "de::i32")]
    pub current_page: i32,
    #[serde(deserialize_with = "de::i32")]
    pub last_page: i32,
    #[serde(deserialize_with = "de::i32")]
    pub per_page: i32,
    #[serde(deserialize_with = "de::i32")]
    pub total: i32,
}

/// `{ "value": …, "description": … }` pair used for quality/type/codec/color.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnilibertyValue {
    #[serde(deserialize_with = "de::opt_string")]
    pub value: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnilibertyTorrent {
    #[serde(deserialize_with = "de::i32")]
    pub id: i32,
    #[serde(deserialize_with = "de::opt_string")]
    pub hash: Option<String>,
    #[serde(deserialize_with = "de::i64")]
    pub size: i64,
    #[serde(deserialize_with = "de::opt_string")]
    pub magnet: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub label: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub filename: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub created_at: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub updated_at: Option<String>,
    pub quality: Option<AnilibertyValue>,
    #[serde(rename = "type")]
    pub type_: Option<AnilibertyValue>,
    pub codec: Option<AnilibertyValue>,
    #[serde(deserialize_with = "de::i32")]
    pub seeders: i32,
    #[serde(deserialize_with = "de::i32")]
    pub leechers: i32,
    #[serde(deserialize_with = "de::opt_i32")]
    pub bitrate: Option<i32>,
    #[serde(deserialize_with = "de::bool")]
    pub is_hardsub: bool,
    #[serde(deserialize_with = "de::opt_string")]
    pub description: Option<String>,
    #[serde(deserialize_with = "de::i32")]
    pub completed_times: i32,
    pub color: Option<AnilibertyValue>,
    pub release: Option<AnilibertyRelease>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnilibertyRelease {
    #[serde(deserialize_with = "de::i32")]
    pub id: i32,
    pub name: Option<AnilibertyReleaseName>,
    #[serde(deserialize_with = "de::opt_i32")]
    pub year: Option<i32>,
    #[serde(rename = "type")]
    pub type_: Option<AnilibertyValue>,
    #[serde(deserialize_with = "de::opt_string")]
    pub alias: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnilibertyReleaseName {
    #[serde(deserialize_with = "de::opt_string")]
    pub main: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub english: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub alternative: Option<String>,
}
