//! Post-search result filters (category, year, type, tracker, season/episode) and paging.

use crab_core::models::api::Result;
use crab_core::{rx, time, util::is_blank};

use super::tracker_matching;

pub const DEFAULT_LIMIT: i32 = 100;
pub const MAX_LIMIT: i32 = 1000;

/// A blank result row (all optional fields empty).
pub fn empty_result() -> Result {
    Result {
        Tracker: None,
        Details: None,
        Title: None,
        Size: 0.0,
        PublishDate: time::min(),
        Category: None,
        CategoryDesc: None,
        Seeders: 0,
        Peers: 0,
        MagnetUri: None,
        ffprobe: None,
        languages: None,
        info: None,
    }
}

pub fn filter_by_category(items: Vec<Result>, cat_param: &str) -> Vec<Result> {
    if is_blank(cat_param) {
        return items;
    }
    let mut wanted: Vec<i32> = Vec::new();
    for part in cat_param.split(',') {
        if let Ok(n) = part.trim().parse::<i32>() {
            if !wanted.contains(&n) {
                wanted.push(n);
            }
        }
    }
    if wanted.is_empty() {
        return items;
    }
    let has_movie = wanted.iter().any(|&c| (2000..3000).contains(&c));
    let has_tv = wanted.iter().any(|&c| (5000..6000).contains(&c));
    if has_movie && has_tv {
        return items;
    }
    items
        .into_iter()
        .filter(|t| {
            let Some(cats) = t.Category.as_ref().filter(|c| !c.is_empty()) else {
                return true;
            };
            wanted.iter().any(|&w| {
                let bucket = (w / 1000) * 1000;
                cats.iter().any(|&c| c >= bucket && c < bucket + 1000)
            })
        })
        .collect()
}

pub fn filter_by_year(items: Vec<Result>, year: i32) -> Vec<Result> {
    if year <= 0 {
        return items;
    }
    items
        .into_iter()
        .filter(|t| {
            let rel = t.info.as_ref().map(|i| i.relased).unwrap_or(0);
            matches_card_year(rel, year, true)
        })
        .collect()
}

/// Card-search year gate. Unknown `relased` (≤0) passes.
/// Movies: year ± 1. Serials and other: relased ≥ year − 1.
pub fn matches_card_year(relased: i32, year: i32, movie_like: bool) -> bool {
    if year <= 0 || relased <= 0 {
        return true;
    }
    if movie_like {
        return relased == year || relased == year - 1 || relased == year + 1;
    }
    relased >= year - 1
}

/// Keep items whose `info.types` contains `kind`; items without types pass.
pub fn filter_by_type(items: Vec<Result>, kind: Option<&str>) -> Vec<Result> {
    let Some(kind) = kind.filter(|k| !is_blank(k)) else {
        return items;
    };
    items
        .into_iter()
        .filter(|t| match t.info.as_ref().and_then(|i| i.types.as_ref()) {
            None => true,
            Some(types) if types.is_empty() => true,
            Some(types) => types.iter().any(|x| x == kind),
        })
        .collect()
}

pub fn filter_by_trackers(items: Vec<Result>, trackers: &[String]) -> Vec<Result> {
    if trackers.is_empty() {
        return items;
    }
    let allowed = tracker_matching::to_allow_set(trackers);
    items.into_iter().filter(|t| tracker_matching::matches(t.Tracker.as_deref(), &allowed)).collect()
}

pub fn paginate(items: Vec<Result>, limit: Option<i32>, offset: Option<i32>) -> Vec<Result> {
    let off = offset.unwrap_or(0).max(0);
    if limit.is_none() && off == 0 {
        return items;
    }
    let lim = match limit {
        Some(l) => l.min(MAX_LIMIT).max(0),
        None => DEFAULT_LIMIT,
    };
    items.into_iter().skip(off as usize).take(lim as usize).collect()
}

// ---------------------------------------------------------------------------
// Season / episode
// ---------------------------------------------------------------------------

const SXXEXX: &str = r"(?<![0-9])s(?<season>\d{1,2})[\s._-]*e(?<episode>\d{1,3})(?![0-9])";
const NXNN: &str = r"(?<![0-9])(?<season>\d{1,2})x(?<episode>\d{1,3})(?![0-9])";
const SEASON_PACK: &str = r"(?<![0-9])s(?<season>\d{1,2})(?!\d|\s*e)(?:\s|\.|\]|/|$|[\[(])";

fn num(c: &fancy_regex::Captures<'_>, n: &str) -> Option<i32> {
    c.name(n).and_then(|m| m.as_str().parse::<i32>().ok())
}

/// (season, episode, is_season_pack) parsed from a release title.
pub fn parse_title(title: Option<&str>) -> Option<(i32, Option<i32>, bool)> {
    let title = title.filter(|t| !is_blank(t))?;
    if let Some(c) = rx::captures_i(title, SXXEXX) {
        return Some((num(&c, "season")?, num(&c, "episode"), false));
    }
    if let Some(c) = rx::captures_i(title, NXNN) {
        return Some((num(&c, "season")?, num(&c, "episode"), false));
    }
    if let Some(c) = rx::captures_i(title, SEASON_PACK) {
        return Some((num(&c, "season")?, None, true));
    }
    None
}

pub fn season_episode_filter(items: Vec<Result>, season: i32, episode: Option<i32>) -> Vec<Result> {
    if season <= 0 {
        return items;
    }
    items.into_iter().filter(|t| season_episode_matches(t, season, episode)).collect()
}

fn season_episode_matches(t: &Result, season: i32, episode: Option<i32>) -> bool {
    if let Some(seasons) = t.info.as_ref().and_then(|i| i.seasons.as_ref()).filter(|s| !s.is_empty()) {
        if !seasons.contains(&season) {
            return false;
        }
        let Some(ep) = episode else {
            return true;
        };
        return match parse_title(t.Title.as_deref()) {
            None => true,
            Some((s, e, _)) => s == season && (e.is_none() || e == Some(ep)),
        };
    }

    let Some((s, e, pack)) = parse_title(t.Title.as_deref()) else {
        return false;
    };
    if s != season {
        return false;
    }
    let Some(ep) = episode else {
        return true;
    };
    if pack {
        return true;
    }
    e == Some(ep)
}

/// Season/episode attributes for feed output.
pub fn attrs_from_result(t: &Result) -> (Option<i32>, Option<i32>) {
    match parse_title(t.Title.as_deref()) {
        None => {
            if let Some(s) = t.info.as_ref().and_then(|i| i.seasons.as_ref()).filter(|s| s.len() == 1) {
                return (s.first().copied(), None);
            }
            (None, None)
        }
        Some((s, e, _)) => (Some(s), e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crab_core::models::api::TorrentInfo;
    use indexmap::IndexSet;

    fn with_info(title: &str, info: Option<TorrentInfo>) -> Result {
        Result { Title: Some(title.into()), info, ..empty_result() }
    }

    #[test]
    fn filter_by_type_keeps_matching_and_untyped() {
        let items = vec![
            with_info("a", Some(TorrentInfo { types: Some(vec!["movie".into()]), ..Default::default() })),
            with_info("b", Some(TorrentInfo { types: Some(vec!["serial".into()]), ..Default::default() })),
            with_info("c", None),
            with_info("d", Some(TorrentInfo { types: Some(vec![]), ..Default::default() })),
        ];
        let f = filter_by_type(items, Some("movie"));
        let titles: Vec<_> = f.iter().map(|r| r.Title.clone().unwrap()).collect();
        assert_eq!(titles, vec!["a", "c", "d"]);
    }

    #[test]
    fn filter_by_year_allows_plus_minus_one() {
        let mk = |t: &str, y: i32| with_info(t, Some(TorrentInfo { relased: y, ..Default::default() }));
        let items = vec![mk("y1999", 1999), mk("y1998", 1998), mk("y2001", 2001), mk("unknown", 0)];
        let f = filter_by_year(items, 1999);
        assert_eq!(f.len(), 3);
        assert!(!f.iter().any(|r| r.Title.as_deref() == Some("y2001")));
        assert!(f.iter().any(|r| r.Title.as_deref() == Some("unknown")));
    }

    #[test]
    fn matches_card_year_unknown_relased_passes() {
        for (relased, year, movie_like, expected) in [
            (0, 2026, true, true),
            (0, 2026, false, true),
            (2026, 0, true, true),
            (2026, 2026, true, true),
            (2025, 2026, true, true),
            (2027, 2026, true, true),
            (2024, 2026, true, false),
            (2026, 2026, false, true),
            (2025, 2026, false, true),
            (2024, 2026, false, false),
            (2027, 2026, false, true),
        ] {
            assert_eq!(matches_card_year(relased, year, movie_like), expected, "{relased} {year} {movie_like}");
        }
    }

    #[test]
    fn season_episode_parse_and_filter() {
        assert_eq!(parse_title(Some("Show S02E05 1080p")), Some((2, Some(5), false)));
        assert_eq!(parse_title(Some("Show 2x05")), Some((2, Some(5), false)));
        assert_eq!(parse_title(Some("Show S02 [1080p]")), Some((2, None, true)));
        assert_eq!(parse_title(Some("Show")), None);

        let items = vec![
            with_info("Show S02E05", None),
            with_info("Show S02E06", None),
            with_info("Show S02 pack", None),
            with_info("Show S03E05", None),
        ];
        let f = season_episode_filter(items, 2, Some(5));
        let titles: Vec<_> = f.iter().map(|r| r.Title.clone().unwrap()).collect();
        assert_eq!(titles, vec!["Show S02E05", "Show S02 pack"]);

        let with_seasons = with_info(
            "Шоу (сезон 2)",
            Some(TorrentInfo { seasons: Some(IndexSet::from([2])), ..Default::default() }),
        );
        assert_eq!(attrs_from_result(&with_seasons), (Some(2), None));
        assert_eq!(season_episode_filter(vec![with_seasons], 2, Some(3)).len(), 1);
    }

    #[test]
    fn paginate_rules() {
        let items: Vec<Result> = (0..5).map(|i| with_info(&i.to_string(), None)).collect();
        assert_eq!(paginate(items.clone(), None, None).len(), 5);
        assert_eq!(paginate(items.clone(), Some(2), Some(1))[0].Title.as_deref(), Some("1"));
        assert_eq!(paginate(items, None, Some(4)).len(), 1);
    }

    #[test]
    fn category_filter() {
        let mk = |t: &str, cats: &[i32]| Result { Title: Some(t.into()), Category: Some(cats.iter().copied().collect()), ..empty_result() };
        let items = vec![mk("m", &[2000]), mk("s", &[5000]), mk("none", &[])];
        let f = filter_by_category(items.clone(), "5000");
        assert_eq!(f.len(), 2);
        assert_eq!(filter_by_category(items, "2000,5000").len(), 3);
    }
}
