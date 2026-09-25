//! LostFilm HTML parsing: `/new/` feed collectors, V-page quality links, ids and naming helpers.
//! Pure functions (no network), unit-tested against captured fixtures.

use chrono::{DateTime, Datelike, Utc};
use indexmap::{IndexMap, IndexSet};
use serde::Serialize;

use crab_core::models::TorrentDetails;
use crab_core::parsing::tparse;
use crab_core::util::{html_decode, is_blank};
use crab_core::{rx, time};

pub const TRACKER: &str = "lostfilm";

const HOR_BREAKER: &str = "class=\"hor-breaker dashed\"";
const EPISODE_LINK_RE: &str =
    r#"(?i)<a\s[^>]*href="[^"]*?(/series/([^/"]+)/season_(\d+)/episode_(\d+)/)[^"]*"[^>]*>([\s\S]*?)</a>"#;
const NEW_MOVIE_RE: &str =
    r#"(?i)<a\s+class="new-movie"\s+href="(?:https?://[^"]+)?(/series/[^"]+)"[^>]*title="([^"]*)"[^>]*>([\s\S]*?)</a>"#;
const SINFO_RE: &str = r"(?i)(\d+)\s*сезон\s*(\d+)\s*серия";
const DATE_RE: &str = r"(\d{2}\.\d{2}\.\d{4})";

/// Case-insensitive map `series/...` path → (name ru, originalname) built from hor-breaker blocks.
#[derive(Clone, Debug, Default)]
pub struct HorBreakerNameMap {
    /// lowercase key → (original key, name, originalname)
    inner: IndexMap<String, (String, String, String)>,
}

impl HorBreakerNameMap {
    pub fn get(&self, key: &str) -> Option<(String, String)> {
        self.inner.get(&key.to_lowercase()).map(|(_, n, o)| (n.clone(), o.clone()))
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.contains_key(&key.to_lowercase())
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.inner.values().map(|(k, _, _)| k.as_str())
    }
    fn insert_if_absent(&mut self, key: &str, pair: &(String, String)) {
        let lk = key.to_lowercase();
        if !self.inner.contains_key(&lk) {
            self.inner.insert(lk, (key.to_string(), pair.0.clone(), pair.1.clone()));
        }
    }
}

fn hor_breaker_rows(html: &str) -> impl Iterator<Item = &str> {
    html.split(HOR_BREAKER).skip(1)
}

fn row_href(row: &str) -> String {
    rx::group_i(row, r#"href="/([^"]+)""#, 1).trim().to_string()
}

fn row_div(row: &str, class: &str) -> String {
    rx::group_i(row, &format!(r#"<div class="{class}">([^<]+)</div>"#), 1).trim().to_string()
}

fn row_date(row: &str) -> String {
    rx::group_i(row, r#"<div class="right-part">(\d{2}\.\d{2}\.\d{4})</div>"#, 1).trim().to_string()
}

fn serie_slug(url_path: &str) -> String {
    rx::group(url_path, r"series/([^/]+)(?:/|$)", 1)
}

fn collapse_ws(s: &str) -> String {
    rx::replace(s, r"[\s]+", " ").trim().to_string()
}

/// Builds `series/.../season_N/episode_M` → (name ru, originalname) from hor-breaker blocks,
/// plus a per-series key (`series/Slug`) so every episode of a show gets the same Russian name.
pub fn build_hor_breaker_name_map(html: &str) -> HorBreakerNameMap {
    let mut map = HorBreakerNameMap::default();
    for row in hor_breaker_rows(html) {
        if is_blank(row) {
            continue;
        }
        let url = row_href(row);
        let name = row_div(row, "name-ru");
        let originalname = row_div(row, "name-en");
        if url.is_empty() || !url.starts_with("series/") || name.is_empty() || originalname.is_empty() {
            continue;
        }
        let key = url.trim_end_matches('/');
        let pair = (html_decode(&name), html_decode(&originalname));
        map.insert_if_absent(key, &pair);
        let caps = rx::groups_i(&url, r"^series/([^/]+)(?:/|$)");
        if !caps[0].is_empty() {
            let series_key = format!("series/{}", caps[1].trim_end_matches('/'));
            map.insert_if_absent(&series_key, &pair);
        }
    }
    map
}

/// Keeps one row per url (case-insensitive); on duplicates prefers the row with a Russian name.
pub fn dedupe_list_by_url(list: &mut Vec<TorrentDetails>) {
    let mut by_url: IndexMap<String, TorrentDetails> = IndexMap::new();
    for t in list.drain(..) {
        if t.url.is_empty() {
            continue;
        }
        let k = t.url.to_lowercase();
        match by_url.get_mut(&k) {
            None => {
                by_url.insert(k, t);
            }
            Some(existing) => {
                if has_ru_name_t(&t) && !has_ru_name_t(existing) {
                    *existing = t;
                }
            }
        }
    }
    list.extend(by_url.into_values());
}

/// Release year and Russian series name from a series or `/seasons/` page.
pub fn parse_relased_and_name_from_html(html: &str) -> (i32, Option<String>) {
    if html.is_empty() {
        return (0, None);
    }
    let caps = rx::groups(html, r#"itemprop="dateCreated"\s+content="(\d{4})-\d{2}-\d{2}""#);
    if caps[0].is_empty() {
        return (0, None);
    }
    let year: i32 = match caps[1].parse() {
        Ok(y) if y > 0 => y,
        _ => return (0, None),
    };
    let mut russian_name: Option<String> = None;
    let og = rx::groups_i(html, r#"<meta\s+property="og:title"\s+content="([^"]+)""#);
    if !og[0].is_empty() {
        russian_name = Some(html_decode(og[1].trim()));
    }
    if russian_name.as_deref().map(is_blank).unwrap_or(true) {
        let tit = rx::groups_i(html, r"<title>([^<]+?)\.?\s*[–-]\s*LostFilm");
        if !tit[0].is_empty() {
            russian_name = Some(shorten_series_name(&html_decode(tit[1].trim())));
        }
    } else if let Some(n) = russian_name.take() {
        russian_name = Some(shorten_series_name(&n));
    }
    (year, russian_name)
}

/// Magnets are only served to logged-in users, so a cookie is mandatory.
pub fn has_auth_cookie(cookie: Option<&str>) -> bool {
    cookie.map(|c| !is_blank(c)).unwrap_or(false)
}

/// Qualities stored from V-pages (episodes, movies, season packs).
pub const PREFERRED_QUALITIES: [&str; 2] = ["1080p", "2160p"];

pub fn is_preferred_quality(quality: &str) -> bool {
    let q = normalize_quality(quality);
    if q.is_empty() {
        return false;
    }
    PREFERRED_QUALITIES.iter().any(|p| p.eq_ignore_ascii_case(&q))
}

/// 1080/720 → 1080p/720p, sd → SD, mp4 → 720p; anything else unchanged.
pub fn normalize_quality(quality: &str) -> String {
    if is_blank(quality) {
        return quality.to_string();
    }
    let q = quality.trim();
    if rx::is_match_i(q, r"^\d{3,4}p$") {
        return q.to_lowercase();
    }
    let l = q.to_ascii_lowercase();
    match l.as_str() {
        "1080" => "1080p".into(),
        "2160" => "2160p".into(),
        "720" => "720p".into(),
        "sd" => "SD".into(),
        "mp4" => "720p".into(),
        _ => q.to_string(),
    }
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack.to_lowercase().find(&needle.to_lowercase()).and_then(|i| {
        // map back to a char boundary in the original string (lowercasing may change byte lengths)
        let prefix_chars = haystack.to_lowercase()[..i].chars().count();
        haystack.char_indices().nth(prefix_chars).map(|(b, _)| b)
    })
}

/// Short Russian series name for name/title fields (og:title is often a long description).
pub fn shorten_series_name(title: &str) -> String {
    if is_blank(title) {
        return title.trim().to_string();
    }
    const MAX: usize = 200;
    let mut s = title.trim().to_string();

    if let Some(idx) = find_ci(&s, ". Сериал") {
        s = s[..idx].trim().to_string();
        if let Some(p) = s.find(" (") {
            s = s[..p].trim().to_string();
        }
        let n = s.chars().count();
        if n > 0 && n <= MAX {
            return s;
        }
    }

    let caps = rx::groups(&s, r"^(.+?)\s*/\s*[^/]+?\s*/\s*\d+\s*сезон\s*\d+\s*серия\s*\[\d{4}(?:,[^\]]*)?\]\s*$");
    if !caps[0].is_empty() {
        s = caps[1].trim().to_string();
        let n = s.chars().count();
        if n > 0 && n <= MAX {
            return s;
        }
    }

    if let Some(p) = s.find(" (") {
        s = s[..p].trim().to_string();
    }
    if s.chars().count() > MAX {
        s = s.chars().take(MAX).collect::<String>().trim().to_string();
    }
    if s.is_empty() {
        title.trim().to_string()
    } else {
        s
    }
}

// ---------------------------------------------------------------------------
// ids / names
// ---------------------------------------------------------------------------

/// True when name and originalname differ (Russian title present) - a good FileDB bucket.
pub fn has_ru_name(name: &str, originalname: &str) -> bool {
    !is_blank(name) && !is_blank(originalname) && name.trim().to_lowercase() != originalname.trim().to_lowercase()
}

pub fn has_ru_name_t(t: &TorrentDetails) -> bool {
    has_ru_name(&t.name, &t.originalname)
}

/// Slug-only bucket (Ponies:Ponies).
pub fn is_xx_name_bucket(name: &str, originalname: &str) -> bool {
    !has_ru_name(name, originalname)
}

fn play_id(html: &str, fname: &str) -> Option<String> {
    if html.is_empty() {
        return None;
    }
    let long = rx::groups_i(html, &format!(r#"{fname}\s*\(\s*['"](\d{{6,}})['"]"#));
    if !long[0].is_empty() {
        return Some(long[1].clone());
    }
    let three = rx::groups_i(html, &format!(r#"{fname}\s*\(\s*['"](\d+)['"]\s*,\s*['"](\d+)['"]\s*,\s*['"](\d+)['"]"#));
    if !three[0].is_empty() {
        if let (Ok(season), Ok(episode)) = (three[2].parse::<i32>(), three[3].parse::<i32>()) {
            return Some(format!("{}{:03}{:03}", three[1], season, episode));
        }
    }
    let short = rx::groups_i(html, &format!(r#"{fname}\s*\(\s*['"]?(\d+)['"]?\s*\)"#));
    if !short[0].is_empty() {
        Some(short[1].clone())
    } else {
        None
    }
}

/// Combined PlayEpisode id (6+ digits), else `id + sss + eee` from PlayEpisode('id','s','e').
pub fn try_extract_play_episode_id(html: &str) -> Option<String> {
    play_id(html, "PlayEpisode")
}

/// PlayMovie / PlayEpisode id from a movie page.
pub fn try_extract_play_movie_or_episode_id(html: &str) -> Option<String> {
    play_id(html, "Play(?:Movie|Episode)")
}

pub fn strip_url_fragment(url: &str) -> &str {
    match url.find('#') {
        Some(i) => &url[..i],
        None => url,
    }
}

pub fn is_episode_path_url(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    rx::is_match_i(strip_url_fragment(url), r"/series/[^/]+/season_\d+/episode_\d+/?")
}

pub fn append_quality_to_title(title: &str, quality: &str) -> String {
    let q = normalize_quality(quality);
    if q.is_empty() {
        return title.to_string();
    }
    let t = title.trim_end();
    if let Some(stripped) = t.strip_suffix(']') {
        format!("{stripped}, {q}]")
    } else {
        format!("{t} [{q}]")
    }
}

/// Apply cached magnet/size; restore names only when the cache has a RU pair and incoming is X:X.
pub fn apply_magnet_cache(incoming: &mut TorrentDetails, cached: &TorrentDetails) {
    if !cached.magnet.is_empty() {
        incoming.magnet = cached.magnet.clone();
    }
    if !cached.sizeName.is_empty() {
        incoming.sizeName = cached.sizeName.clone();
    }
    if is_xx_name_bucket(&incoming.name, &incoming.originalname) && has_ru_name_t(cached) {
        incoming.name = cached.name.clone();
        incoming.originalname = cached.originalname.clone();
    }
}

/// Clone an episode row for one quality with a `#quality` url suffix.
pub fn clone_with_quality(source: &TorrentDetails, magnet: &str, quality: &str, size_name: &str) -> TorrentDetails {
    let q = normalize_quality(quality);
    let bare = strip_url_fragment(&source.url);
    TorrentDetails {
        trackerName: source.trackerName.clone(),
        types: source.types.clone(),
        url: format!("{bare}#{q}"),
        title: append_quality_to_title(&source.title, &q),
        sid: source.sid,
        pir: source.pir,
        createTime: source.createTime,
        name: source.name.clone(),
        originalname: source.originalname.clone(),
        relased: source.relased,
        magnet: magnet.to_string(),
        sizeName: size_name.to_string(),
        ..Default::default()
    }
}

/// Host-independent lowercase path + fragment.
pub fn canonical_lostfilm_path(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    let u = url.trim();
    let caps = rx::groups_i(u, r"https?://[^/]+(/.*)$");
    let path = if caps[0].is_empty() { u.to_string() } else { caps[1].clone() };
    path.replace('\\', "/").to_lowercase()
}

/// Stable positive FNV-1a hash of the canonical path (keeps `#quality` rows distinct).
pub fn stable_url_id(url: &str) -> i32 {
    let key = canonical_lostfilm_path(url);
    if key.is_empty() {
        return 0;
    }
    let mut hash: u32 = 2166136261;
    for unit in key.encode_utf16() {
        hash ^= unit as u32;
        hash = hash.wrapping_mul(16777619);
    }
    let id = (hash & 0x7FFF_FFFF) as i32;
    if id == 0 {
        1
    } else {
        id
    }
}

// ---------------------------------------------------------------------------
// V-page
// ---------------------------------------------------------------------------

/// 1080p / 2160p torrent links from a V-page (`inner-box--link main`).
pub fn parse_v_page_quality_link_urls(search_html: &str) -> Vec<(String, String)> {
    if search_html.is_empty() || !search_html.contains("inner-box--link") {
        return Vec::new();
    }
    let flat = rx::replace(search_html, r"[\n\r\t]+", " ");
    let mut results = Vec::new();
    for m in rx::all_groups_i(&flat, r#"<div\s+class="inner-box--link\s+main"[^>]*><a\s+href="([^"]+)"[^>]*>([^<]+)</a></div>"#) {
        let link_text = &m[2];
        let mut quality = rx::group_i(link_text, r"(2160p|1080p)", 1);
        if quality.is_empty() {
            quality = rx::group_i(link_text, r"\b(2160|1080)\b", 1);
        }
        if quality.is_empty() {
            continue;
        }
        let quality = normalize_quality(&quality);
        if !is_preferred_quality(&quality) {
            continue;
        }
        let torrent_url = m[1].clone();
        if torrent_url.is_empty() {
            continue;
        }
        results.push((torrent_url, quality));
    }
    results
}

// ---------------------------------------------------------------------------
// /new/ feed
// ---------------------------------------------------------------------------

pub fn extract_total_pages_from_new_page_html(html: &str) -> i32 {
    let mut total = 1;
    if !html.is_empty() && html.contains("LostFilm.TV") {
        for m in rx::all_groups(html, r"/new/page_(\d+)") {
            if let Ok(n) = m[1].parse::<i32>() {
                if n > total {
                    total = n;
                }
            }
        }
        if total > 100 {
            total = 100;
        }
    }
    total
}

fn parse_date_or_now(date_str: &str) -> (Option<DateTime<Utc>>, DateTime<Utc>) {
    let parsed = tparse::parse_create_time(date_str, "dd.MM.yyyy");
    let ct = parsed.unwrap_or_else(time::now);
    (parsed, ct)
}

fn serial_row(url: String, title: String, create_time: DateTime<Utc>, name: String, originalname: String, relased: i32) -> TorrentDetails {
    TorrentDetails {
        trackerName: TRACKER.into(),
        types: vec!["serial".into()],
        url,
        title,
        sid: 1,
        createTime: create_time,
        name,
        originalname,
        relased,
        ..Default::default()
    }
}

/// Episode links `<a href=".../series/Slug/season_N/episode_M/">… N сезон M серия … dd.MM.yyyy</a>`.
pub fn collect_from_episode_links(html: &str, host: &str, list: &mut Vec<TorrentDetails>, _page: i32, map: Option<&HorBreakerNameMap>) {
    let mut seen: IndexSet<String> = IndexSet::new();
    for m in rx::all_groups(html, EPISODE_LINK_RE) {
        let url_path = m[1].trim_start_matches('/').to_string();
        let serie_name = &m[2];
        let block = &m[5];
        if serie_name.is_empty() || seen.contains(&url_path.to_lowercase()) {
            continue;
        }
        let sm = rx::group(block, SINFO_RE, 0);
        let dates = rx::all_groups(block, DATE_RE);
        if sm.is_empty() || dates.is_empty() {
            continue;
        }
        let date_str = &dates[dates.len() - 1][1];
        let (_, create_time) = parse_date_or_now(date_str);
        let relased = create_time.year();
        if relased <= 0 {
            continue;
        }
        seen.insert(url_path.to_lowercase());
        let sinfo = html_decode(&collapse_ws(&sm));
        let mut originalname = serie_name.replace('_', " ");
        let mut name = originalname.clone();
        if let Some(map) = map {
            if let Some((n, o)) = map.get(url_path.trim_end_matches('/')).or_else(|| map.get(&format!("series/{serie_name}"))) {
                name = n;
                originalname = o;
            }
        }
        list.push(serial_row(
            format!("{host}/{url_path}"),
            format!("{name} / {originalname} / {sinfo} [{relased}]"),
            create_time,
            name,
            originalname,
            relased,
        ));
    }
}

fn new_movie_sinfo_and_date(block: &str) -> (String, String) {
    let sinfo = rx::group_i(block, r#"<div\s+class="title"[^>]*>\s*([^<]+)\s*</div>"#, 1);
    let sinfo = html_decode(&collapse_ws(&sinfo));
    let dates = rx::all_groups_i(block, r#"<div\s+class="date"[^>]*>(\d{2}\.\d{2}\.\d{4})</div>"#);
    let date_str = dates.last().map(|d| d[1].clone()).unwrap_or_default();
    (sinfo, date_str)
}

/// `<a class="new-movie" href="/series/…" title="…">` cards.
pub fn collect_from_new_movie(html: &str, host: &str, list: &mut Vec<TorrentDetails>, page: i32, map: Option<&HorBreakerNameMap>) {
    for m in rx::all_groups(html, NEW_MOVIE_RE) {
        let url_path = m[1].trim_start_matches('/').to_string();
        let name_from_attr = shorten_series_name(&html_decode(m[2].trim()));
        let block = &m[3];
        if url_path.is_empty() || !url_path.starts_with("series/") || name_from_attr.is_empty() {
            continue;
        }
        let (sinfo, date_str) = new_movie_sinfo_and_date(block);
        let (parsed, create_time) = parse_date_or_now(&date_str);
        if parsed.is_none() && page != 1 {
            continue;
        }
        let relased = create_time.year();
        if relased <= 0 {
            continue;
        }
        let serie_name = serie_slug(&url_path);
        if serie_name.is_empty() {
            continue;
        }
        let mut originalname = serie_name.replace('_', " ");
        let mut series_name = if !is_blank(&name_from_attr) { name_from_attr.clone() } else { originalname.clone() };
        if let Some(map) = map {
            if let Some((n, o)) = map.get(url_path.trim_end_matches('/')).or_else(|| map.get(&format!("series/{serie_name}"))) {
                series_name = n;
                originalname = o;
            }
        }
        list.push(serial_row(
            format!("{host}/{url_path}"),
            format!("{series_name} / {originalname} / {sinfo} [{relased}]"),
            create_time,
            series_name,
            originalname,
            relased,
        ));
    }
}

/// `class="hor-breaker dashed"` rows (series only).
pub fn collect_from_hor_breaker(html: &str, host: &str, list: &mut Vec<TorrentDetails>, page: i32) {
    for row in hor_breaker_rows(html).filter(|r| !is_blank(r)) {
        let url = row_href(row);
        let sinfo = row_div(row, "left-part");
        let name = row_div(row, "name-ru");
        let originalname = row_div(row, "name-en");
        let date_str = row_date(row);
        if url.is_empty() || !url.starts_with("series/") || name.is_empty() || originalname.is_empty() || sinfo.is_empty() {
            continue;
        }
        let (parsed, create_time) = parse_date_or_now(&date_str);
        if parsed.is_none() && page != 1 {
            continue;
        }
        let relased = create_time.year();
        if relased <= 0 {
            continue;
        }
        if serie_slug(&url).is_empty() {
            continue;
        }
        list.push(serial_row(
            format!("{host}/{url}"),
            format!("{name} / {originalname} / {sinfo} [{relased}]"),
            create_time,
            html_decode(&name),
            html_decode(&originalname),
            relased,
        ));
    }
}

/// Movie rows from the `/new/` feed (hor-breaker blocks with `movies/` href and "Фильм").
#[derive(Clone, Debug)]
pub struct MovieRow {
    pub name: String,
    pub originalname: String,
    pub date_str: String,
    /// `movies/Slug...` path (no leading slash).
    pub url: String,
}

pub fn movie_rows(html: &str) -> Vec<MovieRow> {
    let mut out = Vec::new();
    for row in hor_breaker_rows(html) {
        if is_blank(row) {
            continue;
        }
        let url = row_href(row);
        if url.is_empty() || !url.starts_with("movies/") {
            continue;
        }
        let left = row_div(row, "left-part");
        if find_ci(&left, "Фильм").is_none() {
            continue;
        }
        let name = row_div(row, "name-ru");
        let originalname = row_div(row, "name-en");
        let date_str = row_date(row);
        if name.is_empty() || originalname.is_empty() || date_str.is_empty() {
            continue;
        }
        out.push(MovieRow { name, originalname, date_str, url });
    }
    out
}

/// One `/new/` item as reported by the verify endpoint.
#[derive(Clone, Debug, Serialize)]
pub struct NewPageItem {
    pub title: String,
    pub dateStr: String,
    pub relased: i32,
    pub url: String,
    pub source: String,
}

/// Parses `/new/` and returns (title, dateStr, relased, url, source) for every collector.
pub fn parse_new_page_dates(html: &str, host: &str) -> Vec<NewPageItem> {
    let host = host.trim_end_matches('/');
    let mut result: Vec<NewPageItem> = Vec::new();

    let mut seen: IndexSet<String> = IndexSet::new();
    for m in rx::all_groups(html, EPISODE_LINK_RE) {
        let url_path = m[1].trim_start_matches('/').to_string();
        let serie_name = &m[2];
        let block = &m[5];
        if serie_name.is_empty() || seen.contains(&url_path.to_lowercase()) {
            continue;
        }
        let sm = rx::group(block, SINFO_RE, 0);
        let dates = rx::all_groups(block, DATE_RE);
        if sm.is_empty() || dates.is_empty() {
            continue;
        }
        let sinfo = html_decode(&collapse_ws(&sm));
        let date_str = dates[dates.len() - 1][1].clone();
        let (_, ct) = parse_date_or_now(&date_str);
        let relased = ct.year();
        if relased <= 0 {
            continue;
        }
        seen.insert(url_path.to_lowercase());
        let originalname = serie_name.replace('_', " ");
        let name = originalname.clone();
        result.push(NewPageItem {
            title: format!("{name} / {originalname} / {sinfo} [{relased}]"),
            dateStr: date_str,
            relased,
            url: format!("{host}/{url_path}"),
            source: "episode_links".into(),
        });
    }

    for m in rx::all_groups(html, NEW_MOVIE_RE) {
        let url_path = m[1].trim_start_matches('/').to_string();
        let name_from_attr = shorten_series_name(&html_decode(m[2].trim()));
        let block = &m[3];
        if url_path.is_empty() || !url_path.starts_with("series/") {
            continue;
        }
        let (sinfo, date_str) = new_movie_sinfo_and_date(block);
        let (_, ct) = parse_date_or_now(&date_str);
        let relased = ct.year();
        if relased <= 0 {
            continue;
        }
        let originalname = serie_slug(&url_path).replace('_', " ");
        let series_name = if !is_blank(&name_from_attr) { name_from_attr } else { originalname.clone() };
        let full_url = format!("{host}/{url_path}");
        if !result.iter().any(|i| i.url == full_url) {
            result.push(NewPageItem {
                title: format!("{series_name} / {originalname} / {sinfo} [{relased}]"),
                dateStr: date_str,
                relased,
                url: full_url,
                source: "new-movie".into(),
            });
        }
    }

    for row in hor_breaker_rows(html).filter(|r| !is_blank(r)) {
        let url = row_href(row);
        let sinfo = row_div(row, "left-part");
        let name = row_div(row, "name-ru");
        let originalname = row_div(row, "name-en");
        let date_str = row_date(row);
        if url.is_empty() || !url.starts_with("series/") || name.is_empty() || originalname.is_empty() || sinfo.is_empty() || date_str.is_empty() {
            continue;
        }
        let (_, ct) = parse_date_or_now(&date_str);
        let relased = ct.year();
        if relased <= 0 {
            continue;
        }
        let full_url = format!("{host}/{url}");
        if !result.iter().any(|i| i.url == full_url) {
            result.push(NewPageItem {
                title: format!("{} / {} / {sinfo} [{relased}]", html_decode(&name), html_decode(&originalname)),
                dateStr: date_str,
                relased,
                url: full_url,
                source: "hor-breaker".into(),
            });
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_patterns() {
        assert_eq!(shorten_series_name("Пони / Ponies / 1 сезон 2 серия [2026, 1080p]"), "Пони");
        assert_eq!(shorten_series_name("Пони (Ponies)"), "Пони");
        assert_eq!(shorten_series_name("  "), "");
    }

    #[test]
    fn append_quality() {
        assert_eq!(append_quality_to_title("A [2026]", "1080"), "A [2026, 1080p]");
        assert_eq!(append_quality_to_title("A", "2160p"), "A [2160p]");
        assert_eq!(append_quality_to_title("A", ""), "A");
    }

    #[test]
    fn relased_and_name() {
        let html = r#"<meta property="og:title" content="Капли Бога (Drops of God). Сериал Капли Бога"><span itemprop="dateCreated" content="2023-09-14">"#;
        assert_eq!(parse_relased_and_name_from_html(html), (2023, Some("Капли Бога".to_string())));
        assert_eq!(parse_relased_and_name_from_html("<html>"), (0, None));
    }
}
