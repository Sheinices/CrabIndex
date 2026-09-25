//! Knaben API v1 request/response models and the archive backfill checkpoint.

use chrono::{DateTime, Utc};
use crab_core::models::de;
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::common::null_default;

/// POST body for `{host}/v1` (null fields are omitted).
#[derive(Clone, Debug, Default, Serialize)]
pub struct KnabenApiRequest {
    #[serde(rename = "query", skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(rename = "search_field", skip_serializing_if = "Option::is_none")]
    pub search_field: Option<String>,
    #[serde(rename = "search_type", skip_serializing_if = "Option::is_none")]
    pub search_type: Option<String>,
    #[serde(rename = "categories", skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<i32>>,
    #[serde(rename = "order_by", skip_serializing_if = "Option::is_none")]
    pub order_by: Option<String>,
    #[serde(rename = "order_direction", skip_serializing_if = "Option::is_none")]
    pub order_direction: Option<String>,
    #[serde(rename = "from")]
    pub from: i32,
    #[serde(rename = "size")]
    pub size: i32,
    #[serde(rename = "hide_unsafe")]
    pub hide_unsafe: bool,
    #[serde(rename = "hide_xxx")]
    pub hide_xxx: bool,
    #[serde(rename = "seconds_since_last_seen", skip_serializing_if = "Option::is_none")]
    pub seconds_since_last_seen: Option<i32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct KnabenApiResponse {
    #[serde(rename = "total")]
    pub total: Option<KnabenTotal>,
    #[serde(rename = "hits", deserialize_with = "opt_hits")]
    pub hits: Option<Vec<KnabenHit>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct KnabenTotal {
    #[serde(rename = "relation", deserialize_with = "de::opt_string")]
    pub relation: Option<String>,
    #[serde(rename = "value", deserialize_with = "de::i32")]
    pub value: i32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct KnabenHit {
    #[serde(rename = "title", deserialize_with = "de::string")]
    pub title: String,
    #[serde(rename = "bytes", deserialize_with = "de::i64")]
    pub bytes: i64,
    #[serde(rename = "seeders", deserialize_with = "de::i32")]
    pub seeders: i32,
    #[serde(rename = "peers", deserialize_with = "de::i32")]
    pub peers: i32,
    #[serde(rename = "magnetUrl", deserialize_with = "de::string")]
    pub magnet_url: String,
    #[serde(rename = "link", deserialize_with = "de::string")]
    pub link: String,
    #[serde(rename = "details", deserialize_with = "de::string")]
    pub details: String,
    #[serde(rename = "category", deserialize_with = "de::string")]
    pub category: String,
    #[serde(rename = "categoryId", deserialize_with = "opt_ids")]
    pub category_id: Option<Vec<i32>>,
    #[serde(rename = "date", deserialize_with = "de::string")]
    pub date: String,
    #[serde(rename = "lastSeen", deserialize_with = "de::string")]
    pub last_seen: String,
    #[serde(rename = "tracker", deserialize_with = "de::string")]
    pub tracker: String,
    #[serde(rename = "trackerId", deserialize_with = "de::string")]
    pub tracker_id: String,
    #[serde(rename = "id", deserialize_with = "de::string")]
    pub id: String,
    #[serde(rename = "hash", deserialize_with = "de::string")]
    pub hash: String,
}

fn opt_hits<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<KnabenHit>>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(a) => Some(a.into_iter().filter_map(|x| serde_json::from_value(x).ok()).collect()),
        _ => None,
    })
}

fn opt_ids<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<i32>>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(a) => Some(
            a.into_iter()
                .filter_map(|x| match x {
                    Value::Number(n) => n.as_i64().map(|v| v as i32),
                    Value::String(s) => s.trim().parse().ok(),
                    _ => None,
                })
                .collect(),
        ),
        Value::Number(n) => n.as_i64().map(|v| vec![v as i32]),
        _ => None,
    })
}

/// Checkpoint for the archive backfill (`Data/temp/knaben_backfill.json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct KnabenBackfillState {
    #[serde(deserialize_with = "de::i32")]
    pub CategoryIndex: i32,
    #[serde(deserialize_with = "de::i32")]
    pub CategoryId: i32,
    /// asc | desc
    #[serde(deserialize_with = "de::string")]
    pub Direction: String,
    #[serde(deserialize_with = "de::i32")]
    pub From: i32,
    /// categoryId → pending | complete | partial
    #[serde(deserialize_with = "null_default")]
    pub CategoryStatus: IndexMap<String, String>,
    /// Knaben hit IDs from the last asc page (inner edge of the old window).
    #[serde(deserialize_with = "null_default")]
    pub AscEdgeIds: Vec<String>,
    /// True if any desc-page ID intersected AscEdgeIds during the current category.
    #[serde(deserialize_with = "de::bool")]
    pub DescSawOverlap: bool,
    #[serde(deserialize_with = "de::bool")]
    pub Finished: bool,
    #[serde(deserialize_with = "de::i32")]
    pub TotalFetched: i32,
    #[serde(deserialize_with = "de::i32")]
    pub TotalAdded: i32,
    #[serde(deserialize_with = "de::i32")]
    pub TotalUpdated: i32,
    #[serde(serialize_with = "ser_updated_at", deserialize_with = "crab_core::time::net::deserialize")]
    pub UpdatedAt: DateTime<Utc>,
}

impl Default for KnabenBackfillState {
    fn default() -> Self {
        KnabenBackfillState {
            CategoryIndex: 0,
            CategoryId: 0,
            Direction: "asc".to_string(),
            From: 0,
            CategoryStatus: IndexMap::new(),
            AscEdgeIds: Vec::new(),
            DescSawOverlap: false,
            Finished: false,
            TotalFetched: 0,
            TotalAdded: 0,
            TotalUpdated: 0,
            UpdatedAt: crab_core::time::min(),
        }
    }
}

fn ser_updated_at<S: Serializer>(dt: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    if crab_core::time::is_min(dt) {
        s.serialize_str("0001-01-01T00:00:00")
    } else {
        s.serialize_str(&crate::common::iso_trimmed(dt))
    }
}
