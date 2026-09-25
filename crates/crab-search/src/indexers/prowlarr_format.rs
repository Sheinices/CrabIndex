//! Result rows → Prowlarr Search Feed (ReleaseResource) JSON.

use chrono::{Datelike, Utc};
use serde_json::{json, Map, Value};

use crab_core::models::api::Result;
use crab_core::{time, util::is_blank};

use super::torznab_xml;

const AGGREGATE_INDEXER_ID: i32 = 1;
const AGGREGATE_INDEXER_NAME: &str = "CrabIndex (all trackers)";

fn category_name(id: i32) -> Option<&'static str> {
    match id {
        2000 => Some("Movies"),
        2010 => Some("Movies/Foreign"),
        5000 => Some("TV"),
        5020 => Some("TV/Foreign"),
        5070 => Some("TV/Anime"),
        5080 => Some("TV/Documentary"),
        _ => None,
    }
}

pub fn map_releases(results: &[Result], enrich_titles: bool) -> Vec<Value> {
    results.iter().map(|r| map_release(r, enrich_titles)).collect()
}

fn put(map: &mut Map<String, Value>, key: &str, v: Value) {
    if !v.is_null() {
        map.insert(key.to_string(), v);
    }
}

pub fn map_release(torrent: &Result, enrich_titles: bool) -> Value {
    let display = torznab_xml::display_title(torrent, enrich_titles);
    let magnet = torrent.MagnetUri.clone().unwrap_or_default();
    let details = torrent.Details.clone();
    let info_hash = torznab_xml::extract_info_hash(&magnet);
    let guid = info_hash.clone().unwrap_or_else(|| torznab_xml::stable_guid(&display));
    let size_bytes = torznab_xml::resolve_size_bytes(torrent);

    let now = Utc::now();
    let publish = if time::is_min(&torrent.PublishDate) || torrent.PublishDate.year() < 2000 { now } else { torrent.PublishDate };
    let age = now - publish;
    let age_ms = age.num_milliseconds() as f64;

    let magnet_url = if magnet.len() >= 7 && magnet[..7].eq_ignore_ascii_case("magnet:") { Some(magnet.clone()) } else { None };
    let download_url = if !is_blank(&magnet) { Some(magnet.clone()) } else { details.clone() };
    let info_url = details.clone().filter(|d| !is_blank(d) && d.len() >= 4 && d[..4].eq_ignore_ascii_case("http"));

    let mut m = Map::new();
    put(&mut m, "guid", json!(guid));
    put(&mut m, "age", json!(((age_ms / 86_400_000.0) as i64).max(0)));
    put(&mut m, "ageHours", json!((age_ms / 3_600_000.0).max(0.0)));
    put(&mut m, "ageMinutes", json!((age_ms / 60_000.0).max(0.0)));
    put(&mut m, "size", json!(size_bytes));
    put(&mut m, "indexerId", json!(AGGREGATE_INDEXER_ID));
    put(
        &mut m,
        "indexer",
        json!(torrent.Tracker.clone().filter(|t| !is_blank(t)).unwrap_or_else(|| AGGREGATE_INDEXER_NAME.to_string())),
    );
    put(&mut m, "title", json!(display));
    put(&mut m, "sortTitle", json!(display));
    put(&mut m, "publishDate", json!(time::format_net(&publish)));
    put(&mut m, "downloadUrl", json!(download_url));
    put(&mut m, "magnetUrl", json!(magnet_url));
    put(&mut m, "infoUrl", json!(info_url));
    put(&mut m, "commentUrl", json!(info_url));
    put(&mut m, "categories", Value::Array(build_categories(torrent)));
    put(&mut m, "protocol", json!("torrent"));
    put(&mut m, "infoHash", json!(info_hash));
    put(&mut m, "seeders", json!(torrent.Seeders));
    put(&mut m, "leechers", json!(torrent.Peers));
    put(&mut m, "ffprobe", serde_json::to_value(&torrent.ffprobe).unwrap_or(Value::Null));
    put(&mut m, "languages", serde_json::to_value(&torrent.languages).unwrap_or(Value::Null));
    put(&mut m, "info", serde_json::to_value(&torrent.info).unwrap_or(Value::Null));
    Value::Object(m)
}

fn build_categories(torrent: &Result) -> Vec<Value> {
    let mut cats = Vec::new();
    let desc = torrent.CategoryDesc.clone().filter(|d| !is_blank(d));
    if let Some(ids) = torrent.Category.as_ref() {
        for &id in ids {
            let name = match (&desc, ids.len() == 1) {
                (Some(d), true) => d.clone(),
                _ => category_name(id).map(str::to_string).unwrap_or_else(|| id.to_string()),
            };
            cats.push(json!({ "id": id, "name": name, "subCategories": [] }));
        }
    }
    if cats.is_empty() {
        let name = desc.unwrap_or_else(|| "Movies".to_string());
        cats.push(json!({ "id": 2000, "name": name, "subCategories": [] }));
    }
    cats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexers::filters::empty_result;
    use indexmap::IndexSet;

    #[test]
    fn release_shape() {
        let r = Result {
            Tracker: Some("rutor".into()),
            Details: Some("http://rutor.info/torrent/1".into()),
            Title: Some("T".into()),
            Size: 10.0,
            Category: Some(IndexSet::from([5020, 2010])),
            CategoryDesc: Some("TV/Foreign".into()),
            MagnetUri: Some("magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01".into()),
            Seeders: 2,
            Peers: 1,
            ..empty_result()
        };
        let v = map_release(&r, true);
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            vec![
                "guid", "age", "ageHours", "ageMinutes", "size", "indexerId", "indexer", "title", "sortTitle", "publishDate",
                "downloadUrl", "magnetUrl", "infoUrl", "commentUrl", "categories", "protocol", "infoHash", "seeders",
                "leechers"
            ]
        );
        assert_eq!(v["guid"], "abcdef0123456789abcdef0123456789abcdef01");
        assert_eq!(v["categories"][0]["name"], "TV/Foreign");
        assert_eq!(v["categories"][1]["name"], "Movies/Foreign");
        assert_eq!(v["size"], 10);
    }
}
