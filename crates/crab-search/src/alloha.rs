//! Alloha TV API v2 title resolver: turns `tt…` / `kp…` / `tmdb…` ids (and
//! themoviedb.org URLs) into titles usable for FileDB search. Results are cached
//! in memory under `alloha:title:{id}` for `alloha.cacheHours`.

use once_cell::sync::Lazy;
use serde_json::Value;
use std::time::Duration;

use crab_core::{conf, net, rx, util::is_blank};

use crate::cache::MemCache;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllohaResolveResult {
    pub Search: Option<String>,
    pub AltName: Option<String>,
    pub AlternativeName: Option<String>,
    pub Year: i32,
    /// FileDB type hint from the Alloha category slug (movie/serial/anime).
    pub Type: Option<String>,
    pub ImdbId: Option<String>,
    pub KpId: Option<String>,
    pub TmdbId: Option<String>,
}

impl AllohaResolveResult {
    pub fn unresolved(search: Option<&str>, altname: Option<&str>) -> Self {
        AllohaResolveResult {
            Search: search.map(str::to_string),
            AltName: altname.map(str::to_string),
            ..Default::default()
        }
    }
}

static CACHE: Lazy<MemCache<AllohaResolveResult>> = Lazy::new(MemCache::new);

const COMPACT_ID: &str = r"^(?:tt|kp|tmdb:?)\d+$";
const TMDB_URL: &str = r"themoviedb\.org/(?<kind>movie|tv)/(?<id>\d+)(?:-[^\s/?#]*)?";

pub fn is_resolvable_id(search: &str) -> bool {
    try_normalize_id(search).is_some()
}

/// Alias of [`is_resolvable_id`].
pub fn is_imdb_or_kp_id(search: &str) -> bool {
    is_resolvable_id(search)
}

/// Normalize an id / TMDB URL. Returns `(canonical_id, url_category_hint)`.
pub fn try_normalize_id(raw: &str) -> Option<(String, Option<String>)> {
    if is_blank(raw) {
        return None;
    }
    let s = raw.trim();

    if let Some(c) = rx::re_i(TMDB_URL).captures(s).ok().flatten() {
        let id = c.name("id").map(|m| m.as_str()).unwrap_or("");
        let kind = c.name("kind").map(|m| m.as_str()).unwrap_or("");
        let hint = if kind.eq_ignore_ascii_case("tv") {
            Some("serial".to_string())
        } else if kind.eq_ignore_ascii_case("movie") {
            Some("movie".to_string())
        } else {
            None
        };
        return Some((format!("tmdb{id}"), hint));
    }

    if rx::is_match_i(s, COMPACT_ID) {
        if s.len() >= 4 && s[..4].eq_ignore_ascii_case("tmdb") {
            let digits = s[4..].trim_start_matches(':');
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            return Some((format!("tmdb{digits}"), None));
        }
        return Some((s.to_lowercase(), None));
    }

    None
}

fn cache_key(id: &str) -> String {
    format!("alloha:title:{}", id.trim().to_lowercase())
}

/// Seed the resolver cache (used by tests and warm-up).
pub fn cache_set(id: &str, result: AllohaResolveResult, ttl: Duration) {
    CACHE.set(cache_key(id), result, ttl);
}

pub fn cache_clear() {
    CACHE.clear();
}

/// Resolve `search` via Alloha when it is an external id; otherwise return it unchanged.
pub async fn resolve(search: Option<&str>, altname: Option<&str>) -> AllohaResolveResult {
    let Some((id, url_hint)) = search.and_then(try_normalize_id) else {
        return AllohaResolveResult::unresolved(search, altname);
    };

    let c = conf();
    let a = &c.alloha;
    if !a.enable || is_blank(&a.token) {
        return AllohaResolveResult::unresolved(search, altname);
    }

    let memkey = cache_key(&id);
    if let Some(cached) = CACHE.get(&memkey) {
        let from_cache = with_fallback(cached, search, altname);
        if from_cache.Type.as_deref().map(is_blank).unwrap_or(true) && url_hint.as_deref().map(|h| !is_blank(h)).unwrap_or(false) {
            let tmdb = from_cache.TmdbId.clone().or_else(|| Some(id.clone()));
            return AllohaResolveResult { Type: url_hint, TmdbId: tmdb, ..from_cache };
        }
        return from_cache;
    }

    let base_url = if a.baseUrl.is_empty() { "https://apbugall.org".to_string() } else { a.baseUrl.trim_end_matches('/').to_string() };
    let query = if id.len() >= 2 && id[..2].eq_ignore_ascii_case("kp") {
        format!("kp={}", &id[2..])
    } else if id.len() >= 4 && id[..4].eq_ignore_ascii_case("tmdb") {
        format!("tmdb={}", &id[4..])
    } else {
        format!("imdb={id}")
    };

    let timeout = if a.timeoutSeconds > 0 { a.timeoutSeconds as u64 } else { 8 };
    let req = net::Req::new().timeout(timeout).header("Authorization", format!("Bearer {}", a.token));
    let root: Option<Value> = net::get_json::<Value>(&format!("{base_url}/v2/movies/search?{query}"), &req).await;

    let data = root.as_ref().and_then(|r| r.get("data")).filter(|d| d.is_object());
    let field = |name: &str| data.and_then(|d| d.get(name)).and_then(value_to_string);
    let original_name = field("original_name");
    let name = field("name");
    let alternative_name = field("alternative_name");
    let year = data.and_then(|d| d.get("year")).and_then(value_to_i32).unwrap_or(0);
    let category_slug = data
        .and_then(|d| d.get("category"))
        .filter(|v| v.is_object())
        .and_then(|v| v.get("slug"))
        .and_then(value_to_string);
    let ids = data.and_then(|d| d.get("ids")).filter(|v| v.is_object());
    let imdb_id = normalize_imdb_id(ids.and_then(|i| i.get("imdb")).and_then(value_to_string).as_deref());
    let kp_id = normalize_numeric_id(ids.and_then(|i| i.get("kp")).and_then(value_to_string).as_deref(), "kp");
    let tmdb_id = normalize_numeric_id(ids.and_then(|i| i.get("tmdb")).and_then(value_to_string).as_deref(), "tmdb")
        .or_else(|| if id.len() >= 4 && id[..4].eq_ignore_ascii_case("tmdb") { Some(id.clone()) } else { None });

    let mapped = map_titles(
        original_name,
        name,
        alternative_name,
        year,
        category_slug.as_deref(),
        url_hint,
        imdb_id,
        kp_id,
        tmdb_id,
        search,
        altname,
    );

    let hours = if a.cacheHours > 0 { a.cacheHours as u64 } else { 24 };
    cache_result(&mapped, &memkey, Duration::from_secs(hours * 3600));
    mapped
}

fn cache_result(result: &AllohaResolveResult, query_key: &str, ttl: Duration) {
    CACHE.set(query_key.to_string(), result.clone(), ttl);
    for id in [&result.ImdbId, &result.KpId, &result.TmdbId].into_iter().flatten() {
        if !is_blank(id) {
            CACHE.set(cache_key(id), result.clone(), ttl);
        }
    }
}

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "True".into() } else { "False".into() }),
        _ => None,
    }
}

fn value_to_i32(v: &Value) -> Option<i32> {
    match v {
        Value::Number(n) => n.as_i64().map(|x| x as i32).or_else(|| n.as_f64().map(|f| f as i32)),
        Value::String(s) => s.trim().parse::<i32>().ok(),
        _ => None,
    }
}

fn normalize_imdb_id(imdb: Option<&str>) -> Option<String> {
    let imdb = imdb?.trim();
    if imdb.is_empty() {
        return None;
    }
    if imdb.len() >= 2 && imdb[..2].eq_ignore_ascii_case("tt") {
        return Some(imdb.to_lowercase());
    }
    if imdb.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("tt{imdb}"));
    }
    None
}

fn normalize_numeric_id(v: Option<&str>, prefix: &str) -> Option<String> {
    let s = v?.trim();
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("{prefix}{s}"))
}

fn map_category_slug(slug: Option<&str>) -> Option<String> {
    let slug = slug?;
    if is_blank(slug) {
        return None;
    }
    match slug.trim().to_lowercase().as_str() {
        "movie" => Some("movie".into()),
        "serial" | "tv-show" => Some("serial".into()),
        "anime" | "anime-serial" => Some("anime".into()),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn map_titles(
    original_name: Option<String>,
    name: Option<String>,
    alternative_name: Option<String>,
    year: i32,
    category_slug: Option<&str>,
    url_hint: Option<String>,
    imdb_id: Option<String>,
    kp_id: Option<String>,
    tmdb_id: Option<String>,
    fallback_search: Option<&str>,
    fallback_alt: Option<&str>,
) -> AllohaResolveResult {
    let non_blank = |s: &Option<String>| s.as_deref().map(|x| !is_blank(x)).unwrap_or(false);
    let (search, alt) = if non_blank(&name) && non_blank(&original_name) {
        (original_name.clone(), name.clone())
    } else {
        let resolved = original_name.clone().or_else(|| name.clone());
        if non_blank(&resolved) {
            (resolved, fallback_alt.map(str::to_string))
        } else {
            (fallback_search.map(str::to_string), fallback_alt.map(str::to_string))
        }
    };

    let mut alt_distinct = None;
    if let Some(a) = alternative_name.as_deref().filter(|a| !is_blank(a)) {
        let a = a.trim();
        let eq = |o: &Option<String>| o.as_deref().map(|x| x.to_lowercase() == a.to_lowercase()).unwrap_or(false);
        if !eq(&search) && !eq(&alt) {
            alt_distinct = Some(a.to_string());
        }
    }

    AllohaResolveResult {
        Search: search,
        AltName: alt,
        AlternativeName: alt_distinct,
        Year: year,
        Type: map_category_slug(category_slug).or(url_hint),
        ImdbId: imdb_id,
        KpId: kp_id,
        TmdbId: tmdb_id,
    }
}

fn with_fallback(cached: AllohaResolveResult, search: Option<&str>, altname: Option<&str>) -> AllohaResolveResult {
    if cached.Search.as_deref().map(|s| !is_blank(s)).unwrap_or(false) {
        return cached;
    }
    AllohaResolveResult::unresolved(search, altname)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(raw: &str) -> Option<(String, Option<String>)> {
        try_normalize_id(raw)
    }

    #[test]
    fn try_normalize_id_compact_ids() {
        for (raw, expected) in [
            ("tt0137523", "tt0137523"),
            ("TT0137523", "tt0137523"),
            ("tt1234567", "tt1234567"),
            ("kp301", "kp301"),
            ("KP301", "kp301"),
            ("kp464963", "kp464963"),
            ("tmdb1315772", "tmdb1315772"),
            ("tmdb:1315772", "tmdb1315772"),
            ("TMDB550", "tmdb550"),
        ] {
            assert_eq!(norm(raw), Some((expected.to_string(), None)), "{raw}");
        }
    }

    #[test]
    fn try_normalize_id_tmdb_urls() {
        for (raw, id, hint) in [
            ("https://www.themoviedb.org/movie/1315772-minions-monsters", "tmdb1315772", "movie"),
            ("https://www.themoviedb.org/tv/1399-game-of-thrones", "tmdb1399", "serial"),
            ("http://themoviedb.org/movie/550", "tmdb550", "movie"),
            ("https://www.themoviedb.org/movie/1315772-minions-monsters?language=en-US", "tmdb1315772", "movie"),
        ] {
            assert_eq!(norm(raw), Some((id.to_string(), Some(hint.to_string()))), "{raw}");
        }
    }

    #[test]
    fn try_normalize_id_rejects_non_ids() {
        for raw in ["", "   ", "Fight Club", "1315772", "imdb0137523", "https://www.imdb.com/title/tt0137523/"] {
            assert!(norm(raw).is_none(), "{raw}");
            assert!(!is_resolvable_id(raw), "{raw}");
        }
    }

    #[test]
    fn is_resolvable_id_matches_alias() {
        assert!(is_resolvable_id("tmdb550"));
        assert!(is_imdb_or_kp_id("kp301"));
        assert!(!is_imdb_or_kp_id("Matrix"));
    }

    #[test]
    fn map_titles_prefers_original() {
        let r = map_titles(
            Some("Fight Club".into()),
            Some("Бойцовский клуб".into()),
            Some("fight club".into()),
            1999,
            Some("movie"),
            None,
            None,
            None,
            None,
            Some("tt0137523"),
            None,
        );
        assert_eq!(r.Search.as_deref(), Some("Fight Club"));
        assert_eq!(r.AltName.as_deref(), Some("Бойцовский клуб"));
        assert_eq!(r.AlternativeName, None);
        assert_eq!(r.Type.as_deref(), Some("movie"));
    }
}
