//! Query-string helpers for the indexer endpoints (ids, categories, trackers, paging…).

use crab_core::{rx, util::is_blank};

use crate::alloha;
use crate::query::{parse_int, QueryCollection};

pub fn strip_wrapping_quotes(value: &str) -> Option<String> {
    if is_blank(value) {
        return None;
    }
    let mut s = value.trim().to_string();
    if s.chars().count() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s = s[1..s.len() - 1].trim().to_string();
    } else if s == "\"" || s == "'" {
        // a lone quote both starts and ends the value
        s = String::new();
    }
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn normalize_imdb_id(value: &str) -> Option<String> {
    let s = strip_wrapping_quotes(value)?;
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((canonical, _)) = alloha::try_normalize_id(s) {
        return Some(canonical);
    }
    if s.len() >= 2 && (s[..2].eq_ignore_ascii_case("kp") || s[..2].eq_ignore_ascii_case("tt")) {
        return Some(s.to_lowercase());
    }
    if rx::is_match(s, r"^\d{7,10}$") {
        return Some(format!("tt{s}"));
    }
    Some(s.to_string())
}

/// True for `tt…` / `kp…` / `tmdb…` / themoviedb.org URLs.
pub fn is_imdb_or_kp_query(query: Option<&str>) -> bool {
    query.map(alloha::is_resolvable_id).unwrap_or(false)
}

pub fn normalize_query(query: Option<&str>) -> Option<String> {
    let q = strip_wrapping_quotes(query?)?;
    let q = q.trim();
    if q.is_empty() {
        return None;
    }
    if let Some((canonical, _)) = alloha::try_normalize_id(q) {
        return Some(canonical);
    }
    if rx::is_match(q, r"^\d{7,10}$") {
        return Some(format!("tt{q}"));
    }
    Some(q.to_string())
}

pub fn normalize_tmdb_id(value: &str) -> Option<String> {
    let s = strip_wrapping_quotes(value)?;
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((canonical, _)) = alloha::try_normalize_id(s) {
        if canonical.to_lowercase().starts_with("tmdb") {
            return Some(canonical);
        }
    }
    if rx::is_match(s, r"^\d+$") {
        return Some(format!("tmdb{s}"));
    }
    None
}

pub fn imdb_id_from_query(query: &QueryCollection) -> Option<String> {
    // keys are case-insensitive, so "imdbId" collapses into "imdbid"
    for key in ["imdbid", "imdb_id"] {
        if let Some(v) = query.get_non_blank(key) {
            return normalize_imdb_id(&v);
        }
    }
    None
}

/// `q` → `query` → imdb id params.
pub fn resolve_search_query(query: &QueryCollection) -> Option<String> {
    for key in ["q", "query"] {
        if let Some(v) = query.get_non_blank(key) {
            return normalize_query(Some(&v));
        }
    }
    imdb_id_from_query(query)
}

pub fn tvdb_id_only(query: &QueryCollection, resolved_query: Option<&str>) -> bool {
    if resolved_query.map(|s| !is_blank(s)).unwrap_or(false) {
        return false;
    }
    ["tvdbid", "rid"].iter().any(|k| query.contains_key(k) && !is_blank(&query.get(k)))
}

fn is_category_key(k: &str) -> bool {
    let k = k.to_lowercase();
    k.starts_with("category[") || k == "cat" || k == "category" || k == "categories"
}

/// Positive category ids from `Category[]`, `Category[n]`, `cat`, `Category`, `categories` (comma lists allowed).
pub fn categories_from_query(query: &QueryCollection) -> Vec<i32> {
    let mut cats: Vec<i32> = Vec::new();
    for (key, vals) in query.iter() {
        if !is_category_key(key) {
            continue;
        }
        for val in vals {
            for part in val.split(',') {
                if let Some(n) = parse_int(part.trim()) {
                    if n > 0 && !cats.contains(&n) {
                        cats.push(n);
                    }
                }
            }
        }
    }
    cats
}

/// Prowlarr `indexerIds`: omitted = all; -2 = torrents; -1 = usenet; 1 = this aggregate indexer.
pub fn prowlarr_indexer_ids_include_self(query: &QueryCollection) -> bool {
    if !query.contains_key("indexerids") {
        return true;
    }
    let mut ids: Vec<i32> = Vec::new();
    for val in query.values("indexerids") {
        for part in val.split(',') {
            if let Some(n) = parse_int(part.trim()) {
                ids.push(n);
            }
        }
    }
    if ids.is_empty() {
        return true;
    }
    if ids.iter().all(|&id| id == -1) {
        return false;
    }
    ids.iter().any(|&id| id == -2 || id == 1)
}

/// Tracker filter from `Tracker[]`, `Tracker[n]`, `Tracker` (comma lists allowed), distinct ignoring case.
pub fn trackers_from_query(query: &QueryCollection) -> Vec<String> {
    let mut trackers: Vec<String> = Vec::new();
    for (key, vals) in query.iter() {
        let k = key.to_lowercase();
        if !(k.starts_with("tracker[") || k == "tracker") {
            continue;
        }
        for val in vals {
            for part in val.split(',') {
                let t = part.trim();
                if !t.is_empty() && !trackers.iter().any(|x| x.to_lowercase() == t.to_lowercase()) {
                    trackers.push(t.to_string());
                }
            }
        }
    }
    trackers
}

pub fn season_from_query(query: &QueryCollection) -> Option<i32> {
    query.get_i32("season").filter(|&n| n > 0)
}

pub fn episode_from_query(query: &QueryCollection) -> Option<i32> {
    if let Some(n) = query.get_i32("ep").filter(|&n| n > 0) {
        return Some(n);
    }
    query.get_i32("episode").filter(|&n| n > 0)
}

pub fn limit_offset_from_query(query: &QueryCollection) -> (Option<i32>, i32) {
    let offset = query.get_i32("offset").filter(|&o| o > 0).unwrap_or(0);
    let limit = query.get_i32("limit").filter(|&l| l > 0);
    (limit, offset)
}

pub fn year_from_query(query: &QueryCollection) -> i32 {
    query.get_i32("year").filter(|&y| y > 0).unwrap_or(0)
}

/// Torznab/Prowlarr search type → `is_serial` (1 movie, 2 serial, -1 unknown).
pub fn is_serial_from_torznab_action(t: &str) -> i32 {
    match t {
        "moviesearch" | "movie" => 1,
        "tvsearch" | "tv" => 2,
        _ => -1,
    }
}

/// Infer `is_serial` from Newznab categories when the search type is ambiguous.
pub fn is_serial_from_categories(categories: &[i32]) -> i32 {
    if categories.is_empty() {
        return -1;
    }
    let has_tv = categories.iter().any(|&c| (5000..6000).contains(&c));
    let has_movie = categories.iter().any(|&c| (2000..3000).contains(&c));
    // anime cards send 2000/5000 + 5070 together - keep the broad filter
    if has_tv && has_movie {
        return -1;
    }
    if has_tv {
        if categories.contains(&5020) {
            return 3;
        }
        if categories.contains(&5080) {
            return 4;
        }
        if categories.iter().all(|&c| c == 5070) {
            return 5;
        }
        return 2;
    }
    if has_movie {
        if categories.contains(&2010) {
            return 3;
        }
        return 1;
    }
    -1
}

pub fn is_card_metadata_search(
    title: Option<&str>,
    title_original: Option<&str>,
    is_serial: Option<i32>,
    categories: &[i32],
    genres: Option<&str>,
) -> bool {
    let nb = |s: Option<&str>| s.map(|x| !is_blank(x)).unwrap_or(false);
    if nb(title) || nb(title_original) {
        return true;
    }
    if is_serial.map(|v| v >= 0).unwrap_or(false) {
        return true;
    }
    if !categories.is_empty() {
        return true;
    }
    nb(genres)
}

/// `"Русское / English"` → (ru, en). Returns `(None, None)` when there is no ` / ` split.
pub fn split_bilingual_query(query: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(query) = query else {
        return (None, None);
    };
    if is_blank(query) || !query.contains(" / ") {
        return (None, None);
    }
    let mut parts = query.splitn(2, " / ");
    let left = parts.next().unwrap_or("").trim().to_string();
    let right = parts.next().unwrap_or("").trim().to_string();
    if left.is_empty() || right.is_empty() {
        return (None, None);
    }
    let cyr = r"[а-яА-ЯёЁ]";
    let lat = r"[a-zA-Z]";
    if rx::is_match(&left, cyr) && rx::is_match(&right, lat) {
        return (Some(left), Some(right));
    }
    if rx::is_match(&left, lat) && rx::is_match(&right, cyr) {
        return (Some(right), Some(left));
    }
    (Some(left), Some(right))
}

pub fn strip_trailing_year(query: Option<&str>) -> Option<String> {
    let q = query?;
    if is_blank(q) {
        return None;
    }
    let c = rx::captures(q.trim(), r"^(.+?)\s+(19|20)\d{2}$")?;
    Some(c.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default())
}

const SEASON_EPISODE_TOKEN: &str = r"\b(S\d{1,2}E\d{1,2}|S\d{1,2}E?\d{0,2}|E\d{1,2}|\d{1,2}x\d{1,2})\b";
const RUSSIAN_SEASON_SUFFIX: &str = r"\s*\d{1,2}(-\d{1,2})?\s*сезон\s*.*$";
const SEASON_WORD_SUFFIX: &str = r"\b(Сезон|Season)\s*\d{1,2}(?!\d).*$";

/// Strip inline TV season/episode tokens from text queries. `None` when nothing changed.
pub fn strip_season_episode(query: Option<&str>) -> Option<String> {
    let q = query?;
    if is_blank(q) || is_imdb_or_kp_query(Some(q)) {
        return None;
    }
    let original = q.trim();
    let t = rx::replace_i(original, SEASON_EPISODE_TOKEN, "");
    let t = rx::replace_i(&t, RUSSIAN_SEASON_SUFFIX, "");
    let t = rx::replace_i(&t, SEASON_WORD_SUFFIX, "");
    let t = rx::replace(&t, r"\s{2,}", " ");
    let t = t.trim();
    if t.is_empty() || t.to_lowercase() == original.to_lowercase() {
        return None;
    }
    Some(t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_query_external_ids() {
        for (raw, expected) in [
            ("tt0137523", "tt0137523"),
            ("TT0137523", "tt0137523"),
            ("kp301", "kp301"),
            ("KP464963", "kp464963"),
            ("tmdb1315772", "tmdb1315772"),
            ("tmdb:550", "tmdb550"),
            ("https://www.themoviedb.org/movie/1315772-minions-monsters", "tmdb1315772"),
            ("0137523", "tt0137523"),
        ] {
            let n = normalize_query(Some(raw));
            assert_eq!(n.as_deref(), Some(expected), "{raw}");
            assert!(is_imdb_or_kp_query(n.as_deref()), "{raw}");
        }
    }

    #[test]
    fn normalize_query_plain_title_unchanged() {
        assert_eq!(normalize_query(Some("Fight Club")).as_deref(), Some("Fight Club"));
        assert!(!is_imdb_or_kp_query(Some("Fight Club")));
    }

    #[test]
    fn normalize_tmdb_id_cases() {
        assert_eq!(normalize_tmdb_id("1315772").as_deref(), Some("tmdb1315772"));
        assert_eq!(normalize_tmdb_id("tmdb550").as_deref(), Some("tmdb550"));
        assert_eq!(normalize_tmdb_id("https://www.themoviedb.org/tv/1399-got").as_deref(), Some("tmdb1399"));
        assert_eq!(normalize_tmdb_id("tt0137523"), None);
    }

    #[test]
    fn kp_plain_query_normalizes() {
        assert_eq!(normalize_query(Some("kp361")).as_deref(), Some("kp361"));
        assert!(is_imdb_or_kp_query(Some("kp361")));
    }

    #[test]
    fn strip_season_episode_silo_queries() {
        for (raw, expected) in [
            ("silo S01", "silo"),
            ("укрытие S01", "укрытие"),
            ("silo us S01", "silo us"),
            ("укрытие 2023 S01E01", "укрытие 2023"),
            ("silo 2023 S01", "silo 2023"),
            ("укрытие S01E01", "укрытие"),
            ("укрытие 2023 S01", "укрытие 2023"),
            ("silo S01E01", "silo"),
            ("silo 2023 S01E01", "silo 2023"),
            ("silo us S01E01", "silo us"),
        ] {
            assert_eq!(strip_season_episode(Some(raw)).as_deref(), Some(expected), "{raw}");
        }
    }

    #[test]
    fn strip_season_episode_unchanged_returns_none() {
        for raw in ["Fight Club 1999", "tt14688458", "Breaking Bad"] {
            assert_eq!(strip_season_episode(Some(raw)), None, "{raw}");
        }
    }

    #[test]
    fn strip_season_episode_chained_with_trailing_year() {
        let s = strip_season_episode(Some("укрытие 2023 S01"));
        assert_eq!(s.as_deref(), Some("укрытие 2023"));
        assert_eq!(strip_trailing_year(s.as_deref()).as_deref(), Some("укрытие"));
    }

    #[test]
    fn categories_and_trackers_from_query() {
        let q = QueryCollection::parse("category[]=2000&Category[1]=5000,5070&cat=2000&tracker[]=Rutor&tracker=rutor,kinozal");
        assert_eq!(categories_from_query(&q), vec![2000, 5000, 5070]);
        assert_eq!(trackers_from_query(&q), vec!["Rutor", "kinozal"]);
    }

    #[test]
    fn prowlarr_indexer_ids() {
        assert!(prowlarr_indexer_ids_include_self(&QueryCollection::parse("")));
        assert!(prowlarr_indexer_ids_include_self(&QueryCollection::parse("indexerIds=-2")));
        assert!(!prowlarr_indexer_ids_include_self(&QueryCollection::parse("indexerIds=-1")));
        assert!(!prowlarr_indexer_ids_include_self(&QueryCollection::parse("indexerIds=5")));
        assert!(prowlarr_indexer_ids_include_self(&QueryCollection::parse("indexerIds=5,1")));
    }

    #[test]
    fn is_serial_from_categories_rules() {
        assert_eq!(is_serial_from_categories(&[]), -1);
        assert_eq!(is_serial_from_categories(&[2000, 5000]), -1);
        assert_eq!(is_serial_from_categories(&[5000]), 2);
        assert_eq!(is_serial_from_categories(&[5070]), 5);
        assert_eq!(is_serial_from_categories(&[5000, 5020]), 3);
        assert_eq!(is_serial_from_categories(&[2000]), 1);
        assert_eq!(is_serial_from_categories(&[2010]), 3);
    }

    #[test]
    fn bilingual_split() {
        assert_eq!(
            split_bilingual_query(Some("Fight Club / Бойцовский клуб")),
            (Some("Бойцовский клуб".into()), Some("Fight Club".into()))
        );
        assert_eq!(split_bilingual_query(Some("Fight Club")), (None, None));
    }
}
