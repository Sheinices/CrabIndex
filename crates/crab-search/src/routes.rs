//! HTTP handlers: Jackett, Torznab/Newznab, Prowlarr and native torrent search endpoints.
//!
//! Query parameter names are matched case-insensitively (the server also lowercases them).

use axum::extract::{Path, RawQuery};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use serde_json::{json, Value};

use crab_core::conf;
use crab_core::models::api::{Result, RootObject};
use crab_core::util::is_blank;

use crate::indexers::helper::{self, Bound};
use crate::indexers::{engine, params, prowlarr_format, prowlarr_query, torznab_xml, tracker_matching};
use crate::query::{parse_bool, parse_i64, parse_int, QueryCollection};
use crate::search::jackett_service::{self, JackettSearchRequest};
use crate::search::{torrent_query, tracker_catalog};

const XML: &str = "application/xml; charset=utf-8";

pub fn router() -> Router {
    Router::new()
        // Jackett
        .route("/api/v2.0/indexers/:indexer/results", any(jackett))
        .route("/api/v2.0/indexers/:indexer/results/", any(jackett))
        // Torznab / Newznab
        .route("/torznab/api", any(torznab_root))
        .route("/torznab/api/", any(torznab_root))
        .route("/api/v2.0/indexers/:indexer/results/torznab/api", any(torznab_indexer))
        .route("/api/v2.0/indexers/:indexer/results/torznab/api/", any(torznab_indexer))
        .route("/api/v1/indexer/:indexer/newznab", any(torznab_indexer))
        .route("/api/v1/indexer/:indexer/newznab/", any(torznab_indexer))
        // Jackett / Prowlarr metadata
        .route("/api/v2.0/indexers", any(indexers_list))
        .route("/api/v2.0/indexers/", any(indexers_list))
        .route("/api/v1/indexer", any(prowlarr_indexer_list))
        .route("/api/v1/indexer/", any(prowlarr_indexer_list))
        .route("/api/v1/indexer/:indexer", any(prowlarr_indexer_detail))
        .route("/api/v1/search", get(prowlarr_search))
        // native
        .route("/api/v1.0/trackers", get(trackers))
        .route("/api/v1.0/torrents", any(torrents))
        .route("/api/v1.0/qualitys", any(qualitys))
}

fn xml(body: String) -> Response {
    ([(header::CONTENT_TYPE, XML)], body).into_response()
}

fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

fn non_blank(s: String) -> Option<String> {
    if is_blank(&s) {
        None
    } else {
        Some(s)
    }
}

/// First value of a parameter, `None` when missing or empty.
fn first(q: &QueryCollection, key: &str) -> Option<String> {
    q.values(key).first().filter(|v| !v.is_empty()).cloned()
}

/// API key from `apikey=` in the raw query, `X-Api-Key`, or `Authorization: Bearer …`.
pub fn api_key_from_request(raw_query: &str, headers: &HeaderMap) -> Option<String> {
    if let Some(c) = crab_core::rx::captures(raw_query, r"(\?|&)apikey=([^&]+)") {
        let raw = c.get(2).map(|m| m.as_str()).unwrap_or("");
        return Some(urlencoding::decode(raw).map(|s| s.into_owned()).unwrap_or_else(|_| raw.to_string()));
    }
    if let Some(h) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        if !h.is_empty() {
            return Some(h.trim().to_string());
        }
    }
    if let Some(auth) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if auth.len() >= 7 && auth[..7].eq_ignore_ascii_case("bearer ") {
            return Some(auth[7..].trim().to_string());
        }
    }
    None
}

fn is_torznab_enabled() -> bool {
    conf().torznab.enable
}

fn origin(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "http".to_string());
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    format!("{scheme}://{host}")
}

// ---------------------------------------------------------------------------
// Jackett
// ---------------------------------------------------------------------------

async fn jackett(Path(indexer): Path<String>, RawQuery(raw): RawQuery, headers: HeaderMap) -> Json<RootObject> {
    let raw = raw.unwrap_or_default();
    let q = QueryCollection::parse(&raw);
    let user_agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let api_key = first(&q, "apikey").filter(|k| !is_blank(k)).or_else(|| api_key_from_request(q.raw(), &headers));

    let request = JackettSearchRequest {
        query: q.clone(),
        user_agent,
        api_key,
        query_text: first(&q, "query"),
        title: first(&q, "title"),
        title_original: first(&q, "title_original"),
        year: first(&q, "year").and_then(|v| parse_int(&v)).unwrap_or(0),
        is_serial: first(&q, "is_serial").and_then(|v| parse_int(&v)).unwrap_or(-1),
        indexer_path: Some(indexer),
    };

    let results = jackett_service::search(&request).await;
    Json(RootObject { Results: results, jacred: true })
}

// ---------------------------------------------------------------------------
// Torznab
// ---------------------------------------------------------------------------

async fn torznab_root(method: Method, uri: Uri, RawQuery(raw): RawQuery, headers: HeaderMap) -> Response {
    torznab(None, method, uri, raw, headers).await
}

async fn torznab_indexer(
    Path(indexer): Path<String>,
    method: Method,
    uri: Uri,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> Response {
    torznab(Some(indexer), method, uri, raw, headers).await
}

async fn torznab(indexer: Option<String>, method: Method, uri: Uri, raw: Option<String>, headers: HeaderMap) -> Response {
    if !is_torznab_enabled() {
        return not_found();
    }

    let q = QueryCollection::parse(&raw.unwrap_or_default());
    let origin = origin(&headers);
    let path = uri.path().trim_end_matches('/');
    let api_url = format!("{}{}", origin.trim_end_matches('/'), if path.is_empty() { "/torznab/api" } else { path });

    if method == Method::HEAD {
        return xml(String::new());
    }

    let t = first(&q, "t").unwrap_or_default();
    if t == "caps" {
        return xml(torznab_xml::caps_xml(&api_url));
    }
    if t == "indexers" {
        let configured = q.get("configured").to_lowercase();
        if configured.is_empty() || configured == "true" {
            return xml(torznab_xml::indexers_xml(Some(&tracker_catalog::tracker_names()[..])));
        }
        return xml("<?xml version=\"1.0\" encoding=\"UTF-8\"?><indexers></indexers>".to_string());
    }

    let resolved = params::resolve_search_query(&q);
    if params::tvdb_id_only(&q, resolved.as_deref()) {
        return xml_search_result(&[], &t, &q, &origin, &api_url);
    }

    if resolved.as_deref().map(is_blank).unwrap_or(true) && is_blank(&q.get("title")) && is_blank(&q.get("title_original")) {
        return xml_search_result(&[], &t, &q, &origin, &api_url);
    }

    let api_key = first(&q, "apikey");
    let mut req = helper::build_request(&q, api_key.as_deref(), false, Bound { query: resolved, ..Bound::none() });
    tracker_matching::apply_indexer_path_filter(&mut req, indexer.as_deref());
    let results = engine::search_combined(&mut req).await;
    let results = helper::apply_post_filters(results, &q, &req, Some(&t));
    xml_search_result(&results, &t, &q, &origin, &api_url)
}

fn xml_search_result(results: &[Result], t: &str, q: &QueryCollection, origin: &str, api_url: &str) -> Response {
    let cat_param = helper::category_param(q);
    let assigned = if t == "tvsearch" || t == "tv" {
        "5000".to_string()
    } else if t == "moviesearch" || t == "movie" {
        "2000".to_string()
    } else if !is_blank(&cat_param) {
        cat_param.split(',').next().unwrap_or("").trim().to_string()
    } else {
        String::new()
    };
    let enrich = conf().torznab.enrichTitles;
    let items = torznab_xml::items_xml(results, &assigned, enrich, &cat_param);
    xml(torznab_xml::wrap_rss(&items, origin, api_url))
}

// ---------------------------------------------------------------------------
// Jackett / Prowlarr metadata
// ---------------------------------------------------------------------------

fn indexer_entry(id: &str, name: &str, description: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "description": description,
        "type": "public",
        "configured": true,
        "link": "https://github.com/sheinices/crabindex"
    })
}

async fn indexers_list() -> Json<Vec<Value>> {
    let mut list = vec![indexer_entry(
        "all",
        "CrabIndex (all trackers)",
        "Aggregated CrabIndex search across all configured trackers",
    )];
    for tracker in tracker_catalog::tracker_names() {
        list.push(indexer_entry(&tracker, &tracker, &format!("CrabIndex tracker: {tracker}")));
    }
    Json(list)
}

async fn prowlarr_indexer_list() -> Response {
    if !is_torznab_enabled() {
        return not_found();
    }
    Json(json!([{
        "id": 1,
        "name": "CrabIndex (all trackers)",
        "description": "Aggregated CrabIndex search across all configured trackers",
        "implementation": "Torznab",
        "implementationName": "Torznab",
        "enable": true,
        "protocol": "torrent"
    }]))
    .into_response()
}

async fn prowlarr_indexer_detail(Path(id): Path<String>) -> Response {
    let Ok(id) = id.trim().parse::<i32>() else {
        return not_found();
    };
    if !is_torznab_enabled() || id != 1 {
        return not_found();
    }
    Json(json!({
        "id": 1,
        "name": "CrabIndex (all trackers)",
        "description": "Aggregated CrabIndex search across all configured trackers",
        "implementation": "Torznab",
        "implementationName": "Torznab",
        "enable": true,
        "fields": []
    }))
    .into_response()
}

async fn prowlarr_search(RawQuery(raw): RawQuery) -> Response {
    if !is_torznab_enabled() {
        return not_found();
    }
    let q = QueryCollection::parse(&raw.unwrap_or_default());
    let empty = || Json(Vec::<Value>::new()).into_response();

    if !params::prowlarr_indexer_ids_include_self(&q) {
        return empty();
    }

    let mut kind = q.get("type").trim().to_string();
    if is_blank(&kind) {
        kind = "search".to_string();
    }

    let mut raw_query = q.get("query");
    if is_blank(&raw_query) {
        raw_query = q.get("q");
    }

    let parsed = prowlarr_query::parse(Some(&raw_query), Some(&kind));
    if parsed.tvdb_id_only {
        return empty();
    }

    let nb = |s: &Option<String>| s.as_deref().map(|x| !is_blank(x)).unwrap_or(false);
    if !nb(&parsed.query)
        && !nb(&parsed.title)
        && !nb(&parsed.title_original)
        && is_blank(&q.get("title"))
        && is_blank(&q.get("title_original"))
    {
        return empty();
    }

    let lower = kind.to_lowercase();
    let torznab_action = match lower.as_str() {
        "tv" => "tvsearch".to_string(),
        "moviesearch" => "movie".to_string(),
        _ => lower.clone(),
    };

    // is_serial=1 (movie) / 2 (serial) from type, else from categories
    let mut is_serial = params::is_serial_from_torznab_action(&torznab_action);
    if is_serial < 0 {
        is_serial = params::is_serial_from_categories(&params::categories_from_query(&q));
    }

    let bound_title = non_blank(q.get("title")).or_else(|| parsed.title.clone());
    let bound_title_original = non_blank(q.get("title_original")).or_else(|| parsed.title_original.clone());

    let api_key = first(&q, "apikey");
    let mut req = helper::build_request(
        &q,
        api_key.as_deref(),
        false,
        Bound {
            query: parsed.query.clone(),
            title: bound_title,
            title_original: bound_title_original,
            year: parsed.year.unwrap_or(0),
            is_serial,
        },
    );

    if parsed.season.is_some() {
        req.season = parsed.season;
    }
    if parsed.episode.is_some() {
        req.episode = parsed.episode;
    }
    if nb(&parsed.genre) && !nb(&req.genres) {
        req.genres = parsed.genre.clone();
    }

    let results = engine::search_combined(&mut req).await;
    let results = helper::apply_post_filters(results, &q, &req, Some(&torznab_action));
    let enrich = conf().torznab.enrichTitles;
    Json(prowlarr_format::map_releases(&results, enrich)).into_response()
}

// ---------------------------------------------------------------------------
// Native API
// ---------------------------------------------------------------------------

async fn trackers() -> Json<Vec<String>> {
    Json(tracker_catalog::tracker_names())
}

async fn torrents(RawQuery(raw): RawQuery) -> Json<Vec<torrent_query::TorrentRow>> {
    let q = QueryCollection::parse(&raw.unwrap_or_default());
    let long = |k: &str| first(&q, k).and_then(|v| parse_i64(&v)).unwrap_or(0);
    let p = torrent_query::TorrentsQuery {
        search: first(&q, "search"),
        altname: first(&q, "altname"),
        exact: first(&q, "exact").and_then(|v| parse_bool(&v)).unwrap_or(false),
        kind: first(&q, "type"),
        sort: first(&q, "sort"),
        tracker: first(&q, "tracker"),
        voice: first(&q, "voice"),
        videotype: first(&q, "videotype"),
        relased: long("relased"),
        quality: long("quality"),
        season: long("season"),
    };
    Json(torrent_query::query_torrents(p).await)
}

async fn qualitys(RawQuery(raw): RawQuery) -> Json<Value> {
    let q = QueryCollection::parse(&raw.unwrap_or_default());
    let page = first(&q, "page").and_then(|v| parse_int(&v)).unwrap_or(1);
    let take = first(&q, "take").and_then(|v| parse_int(&v)).unwrap_or(1000);
    Json(torrent_query::query_qualitys(
        first(&q, "name").as_deref(),
        first(&q, "originalname").as_deref(),
        first(&q, "type").as_deref(),
        page,
        take,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_sources() {
        let h = HeaderMap::new();
        assert_eq!(api_key_from_request("?x=1&apikey=ab%20c", &h).as_deref(), Some("ab c"));
        let mut h = HeaderMap::new();
        h.insert("x-api-key", " k1 ".parse().unwrap());
        assert_eq!(api_key_from_request("", &h).as_deref(), Some("k1"));
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Bearer k2".parse().unwrap());
        assert_eq!(api_key_from_request("", &h).as_deref(), Some("k2"));
    }

    #[test]
    fn router_builds() {
        let _ = router();
    }
}
