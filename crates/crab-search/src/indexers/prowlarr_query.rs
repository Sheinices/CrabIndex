//! Parses Prowlarr Search Feed brace tokens from `query` (e.g. `{TvdbId:71663} {Season:32}`)
//! and promotes Lampa-style plain queries into card fields (title / title_original / year).

use crab_core::{rx, util::is_blank};

use super::{num_query, params};

const TV_RE: &str = r"\{((?:imdbid\:)(?<imdbid>[^{]+)|(?:rid\:)(?<rid>[^{]+)|(?:tvdbid\:)(?<tvdbid>[^{]+)|(?:tmdbid\:)(?<tmdbid>[^{]+)|(?:tvmazeid\:)(?<tvmazeid>[^{]+)|(?:doubanid\:)(?<doubanid>[^{]+)|(?:season\:)(?<season>[^{]+)|(?:episode\:)(?<episode>[^{]+)|(?:year\:)(?<year>[^{]+)|(?:genre\:)(?<genre>[^{]+))\}";
const MOVIE_RE: &str = r"\{((?:imdbid\:)(?<imdbid>[^{]+)|(?:doubanid\:)(?<doubanid>[^{]+)|(?:tmdbid\:)(?<tmdbid>[^{]+)|(?:traktid\:)(?<traktid>[^{]+)|(?:year\:)(?<year>[^{]+)|(?:genre\:)(?<genre>[^{]+))\}";
const MUSIC_RE: &str = r"\{((?:artist\:)(?<artist>[^{]+)|(?:album\:)(?<album>[^{]+)|(?:track\:)(?<track>[^{]+)|(?:label\:)(?<label>[^{]+)|(?:year\:)(?<year>[^{]+)|(?:genre\:)(?<genre>[^{]+))\}";
const BOOK_RE: &str = r"\{((?:author\:)(?<author>[^{]+)|(?:publisher\:)(?<publisher>[^{]+)|(?:title\:)(?<title>[^{]+)|(?:year\:)(?<year>[^{]+)|(?:genre\:)(?<genre>[^{]+))\}";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub query: Option<String>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub season: Option<i32>,
    pub episode: Option<i32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub title: Option<String>,
    pub title_original: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub tvdb_id_only: bool,
}

fn nb(s: &Option<String>) -> bool {
    s.as_deref().map(|x| !is_blank(x)).unwrap_or(false)
}

fn positive(v: &str) -> Option<i32> {
    v.trim().parse::<i32>().ok().filter(|&n| n > 0)
}

pub fn parse(query: Option<&str>, kind: Option<&str>) -> Parsed {
    let mut result = Parsed { query: query.map(str::to_string), ..Default::default() };
    let Some(query) = query.filter(|q| !is_blank(q)) else {
        return result;
    };

    let t = kind.unwrap_or("search").trim().to_lowercase();
    let mut q = query.to_string();
    let mut had_tvdb = false;

    let pattern = match t.as_str() {
        "tvsearch" | "tv" => Some(TV_RE),
        "movie" | "moviesearch" => Some(MOVIE_RE),
        "music" => Some(MUSIC_RE),
        "book" => Some(BOOK_RE),
        _ => None,
    };

    if let Some(pattern) = pattern {
        let re = rx::re_i(pattern);
        let matches: Vec<(String, Vec<(&'static str, String)>)> = re
            .captures_iter(query)
            .filter_map(|c| c.ok())
            .map(|c| {
                let whole = c.get(0).map(|m| m.as_str().to_string()).unwrap_or_default();
                let names: [&'static str; 14] = [
                    "imdbid", "tvdbid", "tmdbid", "season", "episode", "year", "genre", "artist", "album", "title", "author",
                    "rid", "doubanid", "publisher",
                ];
                let found = names
                    .iter()
                    .filter_map(|n| c.name(n).map(|m| (*n, m.as_str().to_string())))
                    .collect();
                (whole, found)
            })
            .collect();

        for (whole, groups) in matches {
            let g = |name: &str| groups.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone());
            match t.as_str() {
                "tvsearch" | "tv" => {
                    if g("tvdbid").is_some() {
                        had_tvdb = true;
                    }
                    if let Some(v) = g("imdbid") {
                        result.imdb_id = params::normalize_imdb_id(&v);
                    }
                    if let Some(v) = g("tmdbid") {
                        result.tmdb_id = params::normalize_tmdb_id(&v);
                    }
                    if let Some(n) = g("season").as_deref().and_then(positive) {
                        result.season = Some(n);
                    }
                    if let Some(n) = g("episode").as_deref().and_then(positive) {
                        result.episode = Some(n);
                    }
                    if let Some(n) = g("year").as_deref().and_then(positive) {
                        result.year = Some(n);
                    }
                    if let Some(v) = g("genre") {
                        result.genre = Some(v.trim().to_string());
                    }
                }
                "movie" | "moviesearch" => {
                    if let Some(v) = g("imdbid") {
                        result.imdb_id = params::normalize_imdb_id(&v);
                    }
                    if let Some(v) = g("tmdbid") {
                        result.tmdb_id = params::normalize_tmdb_id(&v);
                    }
                    if let Some(n) = g("year").as_deref().and_then(positive) {
                        result.year = Some(n);
                    }
                    if let Some(v) = g("genre") {
                        result.genre = Some(v.trim().to_string());
                    }
                }
                "music" => {
                    if let Some(v) = g("artist") {
                        result.artist = Some(v.trim().to_string());
                    }
                    if let Some(v) = g("album") {
                        result.album = Some(v.trim().to_string());
                    }
                    if let Some(n) = g("year").as_deref().and_then(positive) {
                        result.year = Some(n);
                    }
                    if let Some(v) = g("genre") {
                        result.genre = Some(v.trim().to_string());
                    }
                }
                _ => {
                    if let Some(v) = g("title") {
                        result.title = Some(v.trim().to_string());
                    }
                    if let Some(v) = g("author") {
                        if !nb(&result.title) {
                            result.title = Some(v.trim().to_string());
                        }
                    }
                    if let Some(n) = g("year").as_deref().and_then(positive) {
                        result.year = Some(n);
                    }
                    if let Some(v) = g("genre") {
                        result.genre = Some(v.trim().to_string());
                    }
                }
            }
            if !whole.is_empty() {
                q = q.replace(&whole, "");
            }
        }
    }

    let mut q = params::normalize_query(Some(q.trim()));
    if !nb(&q) && nb(&result.imdb_id) {
        q = result.imdb_id.clone();
    }
    if !nb(&q) && nb(&result.tmdb_id) {
        q = result.tmdb_id.clone();
    }
    if !nb(&q) && nb(&result.title) {
        q = result.title.clone();
    }
    if !nb(&q) && nb(&result.artist) {
        q = if nb(&result.album) {
            Some(format!("{} {}", result.artist.as_deref().unwrap_or(""), result.album.as_deref().unwrap_or("")))
        } else {
            result.artist.clone()
        };
    }

    // plain query only - promote into card fields
    if nb(&q) && !nb(&result.imdb_id) && !nb(&result.tmdb_id) {
        enrich_plain_query(&mut result, q.as_deref());
    }

    result.query = q;
    result.tvdb_id_only = had_tvdb && !nb(&result.query) && !nb(&result.imdb_id) && !nb(&result.tmdb_id);
    result
}

fn enrich_plain_query(result: &mut Parsed, q: Option<&str>) {
    let parsed = num_query::parse(q);
    if !parsed.matched {
        return;
    }
    if result.title.is_none() {
        result.title = parsed.title;
    }
    if result.title_original.is_none() {
        result.title_original = parsed.title_original;
    }
    if result.year.is_none() && parsed.year > 0 {
        result.year = Some(parsed.year);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_brace_tmdb_id_becomes_query() {
        let p = parse(Some("{TmdbId:1315772}"), Some("movie"));
        assert_eq!(p.tmdb_id.as_deref(), Some("tmdb1315772"));
        assert_eq!(p.query.as_deref(), Some("tmdb1315772"));
        assert_eq!(p.title, None);
    }

    #[test]
    fn tv_brace_tmdb_id_becomes_query() {
        let p = parse(Some("{tmdbid:1399} {Season:1}"), Some("tvsearch"));
        assert_eq!(p.tmdb_id.as_deref(), Some("tmdb1399"));
        assert_eq!(p.query.as_deref(), Some("tmdb1399"));
        assert_eq!(p.season, Some(1));
    }

    #[test]
    fn imdb_brace_token() {
        let p = parse(Some("{ImdbId:tt0137523}"), Some("moviesearch"));
        assert_eq!(p.imdb_id.as_deref(), Some("tt0137523"));
        assert_eq!(p.query.as_deref(), Some("tt0137523"));
    }

    #[test]
    fn tvdb_only() {
        let p = parse(Some("{TvdbId:71663} {Season:32}"), Some("tvsearch"));
        assert!(p.tvdb_id_only);
        assert_eq!(p.season, Some(32));
        let p = parse(Some("Simpsons {TvdbId:71663}"), Some("tvsearch"));
        assert!(!p.tvdb_id_only);
        assert_eq!(p.query.as_deref(), Some("Simpsons"));
    }
}
