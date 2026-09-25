//! Jackett `/api/v2.0/indexers/{status}/results` search pipeline.

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use std::time::Duration;

use crab_core::conf;
use crab_core::models::api::Result;
use crab_core::util::is_blank;

use super::{card_matcher, result_builder};
use crate::cache::MemCache;
use crate::indexers::helper::{self, Bound};
use crate::indexers::{engine, num_query, params, tracker_matching};
use crate::query::QueryCollection;

/// User agent of the NUM client (query-only requests without `is_serial`).
pub const NUM_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/106.0.0.0 Safari/537.36";

static RESULTS_CACHE: Lazy<MemCache<Vec<Result>>> = Lazy::new(MemCache::new);

#[derive(Clone, Debug)]
pub struct JackettSearchRequest {
    pub query: QueryCollection,
    pub user_agent: String,
    pub api_key: Option<String>,
    pub query_text: Option<String>,
    pub title: Option<String>,
    pub title_original: Option<String>,
    pub year: i32,
    pub is_serial: i32,
    /// Route `{status}` / indexer id ("all", "rutracker", …).
    pub indexer_path: Option<String>,
}

pub async fn search(request: &JackettSearchRequest) -> Vec<Result> {
    let q = &request.query;
    let rqnum = !q.raw().contains("&is_serial=") && request.user_agent == NUM_USER_AGENT;

    let mut query = request.query_text.clone().filter(|s| !is_blank(s));
    if query.is_none() {
        query = params::resolve_search_query(q);
    }

    let nb = |s: &Option<String>| s.as_deref().map(|x| !is_blank(x)).unwrap_or(false);
    if !nb(&query) && !nb(&request.title) && !nb(&request.title_original) {
        return Vec::new();
    }

    let mut req = helper::build_request(
        q,
        request.api_key.as_deref(),
        rqnum,
        Bound {
            query: query.clone(),
            title: request.title.clone(),
            title_original: request.title_original.clone(),
            year: request.year,
            is_serial: request.is_serial,
        },
    );
    // NUM query-only requests: promote the text into card fields
    num_query::apply_to_request(&mut req);
    tracker_matching::apply_indexer_path_filter(&mut req, request.indexer_path.as_deref());
    let results = engine::search_combined(&mut req).await;
    helper::apply_post_filters(results, q, &req, None)
}

/// Card/plain FileDB search producing Jackett rows (cached for 5 min when evercache is permanent).
#[allow(clippy::too_many_arguments)]
pub fn search_results(
    apikey: Option<&str>,
    query: Option<&str>,
    title: Option<&str>,
    title_original: Option<&str>,
    year: i32,
    category: Option<&IndexMap<String, String>>,
    is_serial: i32,
    rqnum: bool,
) -> Vec<Result> {
    let cat_key = match category {
        Some(c) if !c.is_empty() => c.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(","),
        _ => "null".to_string(),
    };
    let cachekey = format!(
        "api:v2.0:indexers:{}:{}:{}:{year}:{cat_key}:{is_serial}",
        query.unwrap_or(""),
        title.unwrap_or(""),
        title_original.unwrap_or("")
    );
    if let Some(r) = RESULTS_CACHE.get(&cachekey) {
        return r;
    }

    let torrents = card_matcher::search(query, title, title_original, year, category, is_serial, rqnum);
    let results = result_builder::build(&torrents, apikey, rqnum);

    let c = conf();
    if c.evercache.enable && c.evercache.validHour == 0 {
        RESULTS_CACHE.set(cachekey, results.clone(), Duration::from_secs(300));
    }
    results
}
