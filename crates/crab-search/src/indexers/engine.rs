//! Combined indexer search: card (Jackett) search, query variants, native fuzzy (v1)
//! search and external-id resolution, merged by infohash.

use indexmap::{IndexMap, IndexSet};

use crab_core::config::SearchSettings;
use crab_core::models::api::{Result, TorrentInfo};
use crab_core::models::TorrentDetails;
use crab_core::util::{is_blank, search_name};
use crab_core::{conf, fdb};

use super::request::IndexerSearchRequest;
use super::{filters, merger, num_query, params, tracker_matching};
use crate::alloha;
use crate::search::jackett_service;
use crate::search::result_builder::{opt, tracker_allowed};

fn nb(s: Option<&str>) -> bool {
    s.map(|x| !is_blank(x)).unwrap_or(false)
}

pub async fn search_combined(req: &mut IndexerSearchRequest) -> Vec<Result> {
    // query-only NUM requests → card fields (also applied by the Jackett endpoint)
    num_query::apply_to_request(req);

    let c = conf();
    let settings = c.search.clone();
    let query = params::normalize_query(req.query.as_deref());

    let mut title_ru = req.title.clone();
    let mut title_en = req.title_original.clone();
    if !nb(title_ru.as_deref()) && !nb(title_en.as_deref()) {
        let (ru, en) = params::split_bilingual_query(query.as_deref());
        title_ru = ru;
        title_en = en;
    }

    let imdb_mode = !req.card_mode && params::is_imdb_or_kp_query(query.as_deref());
    let mut batches: Vec<Vec<Result>> = Vec::new();

    if imdb_mode {
        let resolved = alloha::resolve(query.as_deref(), None).await;
        batches.push(
            v1_search(resolved.Search.as_deref(), resolved.AltName.as_deref(), true, &settings.v1Sort, &req.trackers, req.season, req.rq_num)
                .await,
        );
        if nb(resolved.AlternativeName.as_deref()) {
            batches.push(
                v1_search(
                    resolved.AlternativeName.as_deref(),
                    resolved.AltName.as_deref(),
                    true,
                    &settings.v1Sort,
                    &req.trackers,
                    req.season,
                    req.rq_num,
                )
                .await,
            );
        }

        let mut merged = merger::merge_and_sort(batches);
        if req.year <= 0 && resolved.Year > 0 && c.alloha.filterByYear {
            merged = filters::filter_by_year(merged, resolved.Year);
        }
        if req.categories.is_empty() && nb(resolved.Type.as_deref()) {
            merged = filters::filter_by_type(merged, resolved.Type.as_deref());
        }
        return merged;
    }

    let category = build_category_dict(&req.categories);
    let is_serial = req.is_serial;

    if req.card_mode {
        let card = jackett_service::search_results(
            req.api_key.as_deref(),
            query.as_deref(),
            title_ru.as_deref(),
            title_en.as_deref(),
            req.year,
            category.as_ref(),
            is_serial,
            req.rq_num,
        );
        let empty = card.is_empty();
        batches.push(card);
        if empty {
            for variant in build_query_variants(query.as_deref(), title_ru.as_deref(), title_en.as_deref(), &settings) {
                batches.push(jackett_service::search_results(
                    req.api_key.as_deref(),
                    Some(&variant),
                    None,
                    None,
                    0,
                    None,
                    is_serial,
                    req.rq_num,
                ));
            }
        }
    } else {
        for variant in build_query_variants(query.as_deref(), title_ru.as_deref(), title_en.as_deref(), &settings) {
            batches.push(jackett_service::search_results(
                req.api_key.as_deref(),
                Some(&variant),
                None,
                None,
                0,
                None,
                is_serial,
                req.rq_num,
            ));
        }
    }

    for (search, altname) in v1_pairs(query.as_deref(), title_ru.as_deref(), title_en.as_deref(), &settings, req.card_mode) {
        batches.push(
            v1_search(Some(&search), altname.as_deref(), false, &settings.v1Sort, &req.trackers, req.season, req.rq_num).await,
        );
    }

    merger::merge_and_sort(batches)
}

fn build_category_dict(categories: &[i32]) -> Option<IndexMap<String, String>> {
    if categories.is_empty() {
        return None;
    }
    Some(categories.iter().enumerate().map(|(i, c)| (format!("Category[{i}]"), c.to_string())).collect())
}

pub fn build_query_variants(
    query: Option<&str>,
    title_ru: Option<&str>,
    title_en: Option<&str>,
    settings: &SearchSettings,
) -> Vec<String> {
    let mut variants: Vec<String> = Vec::new();
    let skip_combined = nb(query) && query.unwrap_or("").contains(" / ") && (nb(title_ru) || nb(title_en));

    if let Some(q) = query.filter(|q| !is_blank(q) && !skip_combined) {
        let stripped_season = if settings.stripSeasonEpisode { params::strip_season_episode(Some(q)) } else { None };

        if settings.stripTrailingYear {
            if let Some(y) = params::strip_trailing_year(Some(q)).filter(|s| !is_blank(s)) {
                variants.push(y);
            }
        }
        if let Some(s) = stripped_season.as_ref().filter(|s| !is_blank(s)) {
            variants.push(s.clone());
        }
        if settings.stripTrailingYear {
            if let Some(s) = stripped_season.as_deref().filter(|s| !is_blank(s)) {
                if let Some(y) = params::strip_trailing_year(Some(s)).filter(|s| !is_blank(s)) {
                    variants.push(y);
                }
            }
        }
        if !variants.iter().any(|v| v == q) {
            variants.push(q.to_string());
        }
    }

    for term in [title_ru, title_en].into_iter().flatten() {
        if !is_blank(term) && !variants.iter().any(|v| v == term) {
            variants.push(term.to_string());
        }
    }

    if variants.is_empty() {
        if let Some(q) = query.filter(|q| !is_blank(q)) {
            variants.push(q.to_string());
        }
    }
    variants
}

fn v1_pairs(
    query: Option<&str>,
    title_ru: Option<&str>,
    title_en: Option<&str>,
    settings: &SearchSettings,
    card_mode: bool,
) -> Vec<(String, Option<String>)> {
    let mode = if settings.mergeV1.is_empty() { "auto".to_string() } else { settings.mergeV1.to_lowercase() };
    if mode == "false" || mode == "0" {
        return Vec::new();
    }
    // auto: native fuzzy search only for fuzzy mode, not for card search
    if card_mode && mode == "auto" {
        return Vec::new();
    }
    if mode == "true" || mode == "1" {
        return v1_search_pairs(query, title_ru, title_en, settings, None);
    }
    v1_search_pairs(query, title_ru, title_en, settings, Some(settings.maxV1Pairs.max(1)))
}

fn v1_search_pairs(
    query: Option<&str>,
    title_ru: Option<&str>,
    title_en: Option<&str>,
    settings: &SearchSettings,
    max_pairs: Option<i32>,
) -> Vec<(String, Option<String>)> {
    let mut pairs: Vec<(String, Option<String>)> = Vec::new();
    let mut seen: IndexSet<String> = IndexSet::new();

    let mut add = |search: Option<&str>, altname: Option<&str>| {
        let Some(search) = search.filter(|s| !is_blank(s)) else {
            return;
        };
        let key = format!("{search}\0{}", altname.unwrap_or(""));
        if !seen.insert(key) {
            return;
        }
        pairs.push((search.to_string(), altname.map(str::to_string)));
    };

    if nb(title_ru) && nb(title_en) {
        add(title_en, title_ru);
        add(title_ru, title_en);
    } else if nb(title_ru) {
        add(title_ru, title_en);
    } else if nb(title_en) {
        add(title_en, title_ru);
    }

    for term in build_query_variants(query, title_ru, title_en, settings) {
        add(Some(&term), None);
        if let Some(ru) = title_ru.filter(|s| !is_blank(s)) {
            if !term.contains(ru) {
                add(Some(&term), Some(ru));
            }
        }
        if let Some(en) = title_en.filter(|s| !is_blank(s)) {
            if !term.contains(en) {
                add(Some(&term), Some(en));
            }
        }
    }

    if let Some(max) = max_pairs {
        if max > 0 && pairs.len() > max as usize {
            pairs.truncate(max as usize);
        }
    }
    pairs
}

fn row_sn(t: &TorrentDetails) -> Option<String> {
    if t._sn.is_empty() {
        search_name(&t.name)
    } else {
        Some(t._sn.clone())
    }
}

fn row_so(t: &TorrentDetails) -> Option<String> {
    if t._so.is_empty() {
        search_name(&t.originalname)
    } else {
        Some(t._so.clone())
    }
}

/// Native (v1-style) masterDb search mapped to Jackett rows.
pub async fn v1_search(
    search: Option<&str>,
    altname: Option<&str>,
    exact: bool,
    sort: &str,
    trackers: &[String],
    season: Option<i32>,
    rqnum: bool,
) -> Vec<Result> {
    if !nb(search) {
        return Vec::new();
    }

    let resolved = alloha::resolve(search, altname).await;
    let search = resolved.Search;
    let altname = resolved.AltName;

    let mut torrents: IndexMap<String, TorrentDetails> = IndexMap::new();
    let mut add = |t: TorrentDetails| {
        if !tracker_allowed(&t.trackerName) {
            return;
        }
        if !tracker_matching::matches_list(Some(&t.trackerName), trackers) {
            return;
        }
        let replace = match torrents.get(&t.url) {
            None => true,
            Some(v) => t.updateTime > v.updateTime,
        };
        if replace {
            torrents.insert(t.url.clone(), t);
        }
    };

    let sn = search.as_deref().and_then(search_name);
    let alt_sn = altname.as_deref().and_then(search_name);
    if sn.is_none() && alt_sn.is_none() {
        return Vec::new();
    }

    if exact {
        let keys: Vec<String> = fdb::MASTER_DB
            .iter()
            .filter(|e| {
                let k = e.key();
                sn.as_deref().map(|s| k.starts_with(&format!("{s}:")) || k.ends_with(&format!(":{s}"))).unwrap_or(false)
                    || alt_sn.as_deref().map(|a| k.contains(a)).unwrap_or(false)
            })
            .map(|e| e.key().clone())
            .collect();
        for key in keys {
            for t in fdb::open_read(&key, true, true).into_values() {
                if t.types.is_empty() {
                    continue;
                }
                let n = row_sn(&t);
                let o = row_so(&t);
                if (sn.is_some() && (n == sn || o == sn)) || (alt_sn.is_some() && (n == alt_sn || o == alt_sn)) {
                    add(t);
                }
            }
        }
    } else {
        let c = conf();
        let limit = !c.evercache.enable || c.evercache.validHour > 0;
        let mut keys: Vec<String> = fdb::MASTER_DB
            .iter()
            .filter(|e| {
                let k = e.key();
                sn.as_deref().map(|s| k.contains(s)).unwrap_or(false) || alt_sn.as_deref().map(|a| k.contains(a)).unwrap_or(false)
            })
            .map(|e| e.key().clone())
            .collect();
        if limit {
            keys.truncate(c.maxreadfile.max(0) as usize);
        }
        for key in keys {
            for t in fdb::open_read(&key, true, true).into_values() {
                if !t.types.is_empty() {
                    add(t);
                }
            }
        }
    }

    let mut rows: Vec<TorrentDetails> = torrents.into_values().collect();
    match sort {
        "pir" => rows.sort_by(|a, b| b.pir.cmp(&a.pir)),
        "size" => rows.sort_by(|a, b| b.size.partial_cmp(&a.size).unwrap_or(std::cmp::Ordering::Equal)),
        _ => rows.sort_by(|a, b| b.sid.cmp(&a.sid)),
    }
    if let Some(s) = season.filter(|&s| s > 0) {
        rows.retain(|i| i.seasons.contains(&s));
    }

    rows.iter().take(2000).map(|i| map_v1(i, rqnum)).collect()
}

fn map_v1(i: &TorrentDetails, rqnum: bool) -> Result {
    let mut cats = IndexSet::new();
    let mut desc: Option<&str> = None;
    for kind in &i.types {
        match kind.as_str() {
            "movie" => {
                cats.insert(2000);
                desc = Some("Movies");
            }
            "serial" => {
                cats.insert(5000);
                desc = Some("TV");
            }
            "anime" => {
                cats.insert(5070);
                desc = Some("TV/Anime");
            }
            _ => {}
        }
    }

    Result {
        Tracker: opt(&i.trackerName),
        Details: if i.url.starts_with("http") { Some(i.url.clone()) } else { None },
        Title: opt(&i.title),
        Size: i.size,
        PublishDate: i.createTime,
        Category: Some(cats),
        CategoryDesc: desc.map(str::to_string),
        Seeders: i.sid,
        Peers: i.pir,
        MagnetUri: opt(&i.magnet),
        ffprobe: if rqnum || !conf().tracks { None } else { i.ffprobe.clone() },
        languages: if i.languages.is_empty() { None } else { Some(i.languages.clone()) },
        info: if rqnum {
            None
        } else {
            Some(TorrentInfo {
                name: opt(&i.name),
                originalname: opt(&i.originalname),
                relased: i.relased,
                sizeName: opt(&i.sizeName),
                voices: Some(i.voices.clone()),
                seasons: Some(i.seasons.clone()),
                types: Some(i.types.clone()),
                ..Default::default()
            })
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_variants() {
        let s = SearchSettings::default();
        assert_eq!(build_query_variants(Some("укрытие 2023 S01"), None, None, &s), vec!["укрытие 2023", "укрытие", "укрытие 2023 S01"]);
        assert_eq!(build_query_variants(Some("Fight Club 1999"), None, None, &s), vec!["Fight Club", "Fight Club 1999"]);
        assert_eq!(
            build_query_variants(Some("Бойцовский клуб / Fight Club"), Some("Бойцовский клуб"), Some("Fight Club"), &s),
            vec!["Бойцовский клуб", "Fight Club"]
        );
    }

    #[test]
    fn v1_pairs_modes() {
        let mut s = SearchSettings::default();
        assert!(v1_pairs(Some("x"), None, None, &s, true).is_empty());
        let p = v1_pairs(Some("Fight Club"), Some("Бойцовский клуб"), Some("Fight Club"), &s, false);
        assert_eq!(p.len(), 4);
        assert_eq!(p[0], ("Fight Club".to_string(), Some("Бойцовский клуб".to_string())));
        s.mergeV1 = "false".into();
        assert!(v1_pairs(Some("x"), None, None, &s, false).is_empty());
    }
}
