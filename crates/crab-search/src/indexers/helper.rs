//! Request building and post-filter pipeline shared by the indexer endpoints.

use crab_core::conf;
use crab_core::models::api::Result;
use crab_core::util::is_blank;

use super::request::IndexerSearchRequest;
use super::{filters, params};
use crate::query::{parse_int, QueryCollection};

/// Values bound from the route/controller that take precedence over the query string.
#[derive(Clone, Debug, Default)]
pub struct Bound {
    pub query: Option<String>,
    pub title: Option<String>,
    pub title_original: Option<String>,
    pub year: i32,
    /// -1 when not bound.
    pub is_serial: i32,
}

impl Bound {
    pub fn none() -> Self {
        Bound { is_serial: -1, ..Default::default() }
    }
}

pub fn build_request(query: &QueryCollection, apikey: Option<&str>, rqnum: bool, bound: Bound) -> IndexerSearchRequest {
    let mut resolved_query = bound.query.clone();
    if resolved_query.as_deref().map(is_blank).unwrap_or(true) {
        resolved_query = params::resolve_search_query(query);
    }

    let title = bound.title.clone().unwrap_or_else(|| query.get("title"));
    let title_original = bound.title_original.clone().unwrap_or_else(|| query.get("title_original"));
    let year = if bound.year > 0 { bound.year } else { params::year_from_query(query) };

    let mut is_serial = bound.is_serial;
    let has_is_serial = query.contains_key("is_serial");
    if has_is_serial {
        if let Some(n) = parse_int(&query.get("is_serial")) {
            is_serial = n;
        }
    }

    let categories = params::categories_from_query(query);
    is_serial = apply_category_is_serial_hint(is_serial, &categories);

    let genres = if query.contains_key("genres") { Some(query.get("genres")) } else { None };

    let card_mode = params::is_card_metadata_search(
        Some(&title),
        Some(&title_original),
        if has_is_serial || bound.is_serial >= 0 { Some(is_serial) } else { None },
        &categories,
        genres.as_deref(),
    );

    let trackers = params::trackers_from_query(query);
    let tracker = trackers.first().cloned();

    IndexerSearchRequest {
        query: resolved_query,
        title: Some(title),
        title_original: Some(title_original),
        year,
        is_serial,
        genres,
        categories,
        season: params::season_from_query(query),
        episode: params::episode_from_query(query),
        tracker,
        trackers,
        card_mode,
        api_key: apikey.map(str::to_string),
        rq_num: rqnum,
    }
}

/// Refine `is_serial` from categories only when the client sent `is_serial=0`.
pub fn apply_category_is_serial_hint(is_serial: i32, categories: &[i32]) -> i32 {
    if is_serial != 0 || categories.is_empty() {
        return is_serial;
    }
    let cat = categories.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",");
    if cat.contains("5020") || cat.contains("2010") {
        return 3;
    }
    if cat.contains("5080") {
        return 4;
    }
    if cat.contains("5070") {
        return 5;
    }
    if cat.starts_with("20") {
        return 1;
    }
    if cat.starts_with("50") {
        return 2;
    }
    is_serial
}

/// `cat` as sent, else the parsed category list joined by `,`.
pub fn category_param(query: &QueryCollection) -> String {
    let cat = query.get("cat");
    if !is_blank(&cat) {
        return cat;
    }
    let cats = params::categories_from_query(query);
    cats.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")
}

pub fn apply_post_filters(
    mut results: Vec<Result>,
    query: &QueryCollection,
    req: &IndexerSearchRequest,
    torznab_action: Option<&str>,
) -> Vec<Result> {
    let c = conf();
    let settings = &c.search;
    let cat_param = category_param(query);

    let mut apply_cat = !req.card_mode && !settings.skipCatFilter && !is_blank(&cat_param);
    if let Some(action) = torznab_action {
        let mut is_serial = params::is_serial_from_torznab_action(action);
        if query.contains_key("is_serial") {
            if let Some(n) = parse_int(&query.get("is_serial")) {
                is_serial = n;
            }
        }
        apply_cat = apply_cat && is_serial < 0;
    }

    if apply_cat {
        results = filters::filter_by_category(results, &cat_param);
    }
    if req.year > 0 && !req.card_mode {
        results = filters::filter_by_year(results, req.year);
    }
    if let Some(season) = req.season {
        if !settings.skipSeasonEpisodeFilter {
            results = filters::season_episode_filter(results, season, req.episode);
        }
    }
    results = filters::filter_by_trackers(results, &req.trackers);

    let (limit, offset) = params::limit_offset_from_query(query);
    filters::paginate(results, limit, Some(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_basic() {
        let q = QueryCollection::parse("q=%22Fight+Club%22&cat=2000,5000&season=2&ep=3&tracker[]=rutor&year=1999");
        let r = build_request(&q, Some("k"), false, Bound::none());
        assert_eq!(r.query.as_deref(), Some("Fight Club"));
        assert_eq!(r.categories, vec![2000, 5000]);
        assert_eq!(r.season, Some(2));
        assert_eq!(r.episode, Some(3));
        assert_eq!(r.tracker.as_deref(), Some("rutor"));
        assert_eq!(r.year, 1999);
        assert_eq!(r.is_serial, -1);
        assert!(r.card_mode, "categories imply card mode");
    }

    #[test]
    fn build_request_is_serial_zero_hint() {
        let q = QueryCollection::parse("query=x&is_serial=0&category[0]=5070");
        let r = build_request(&q, None, false, Bound::none());
        assert_eq!(r.is_serial, 5);
        assert!(r.card_mode);
    }

    #[test]
    fn category_param_prefers_cat() {
        assert_eq!(category_param(&QueryCollection::parse("cat=5000,2000")), "5000,2000");
        assert_eq!(category_param(&QueryCollection::parse("category[]=5000&category[]=2000")), "5000,2000");
    }
}
