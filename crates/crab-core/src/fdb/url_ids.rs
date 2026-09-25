//! Stable numeric torrent id from tracker URL: lets FileDB
//! update a row whose slug changed instead of creating a duplicate.
//!
//! Tracker crates that need custom logic (kinozal, subsplease, lostfilm) register
//! an extractor via [`register_id_extractor`].

use dashmap::DashMap;
use once_cell::sync::Lazy;

use crate::rx;

pub type IdExtractor = fn(&str) -> i32;

static EXTRACTORS: Lazy<DashMap<String, IdExtractor>> = Lazy::new(DashMap::new);

/// Register a tracker-specific url → id function (tracker slug lowercase).
pub fn register_id_extractor(tracker: &str, f: IdExtractor) {
    EXTRACTORS.insert(tracker.to_ascii_lowercase(), f);
}

fn num(url: &str, pattern: &str, ic: bool) -> i32 {
    let g = if ic { rx::group_i(url, pattern, 1) } else { rx::group(url, pattern, 1) };
    g.parse().unwrap_or(0)
}

pub fn torrent_id_from_url(tracker: &str, url: &str) -> i32 {
    if url.is_empty() {
        return 0;
    }
    let t = tracker.to_ascii_lowercase();
    if let Some(f) = EXTRACTORS.get(&t) {
        return f(url);
    }
    match t.as_str() {
        "rutor" | "megapeer" => num(url, r"/torrent/(\d+)", false),
        "torrentby" => {
            let id = num(url, r"https?://[^/]+/(\d+)/", false);
            if id > 0 {
                id
            } else {
                num(url, r"/(\d+)/", false)
            }
        }
        "selezen" => num(url, r"/relizy-ot-selezen/(\d+)-", false),
        "baibako" | "rudub" => num(url, r"details\.php\?id=(\d+)", true),
        "kinozal" => {
            // details.php?id= but not userdetails.php?id= (fallback when parser crate did not register)
            num(url, r"(?<![a-z])details\.php\?id=(\d+)", true)
        }
        "nnmclub" | "anibelka" | "korsars" => num(url, r"viewtopic\.php\?t=(\d+)", true),
        "anistar" | "leproduction" => num(url, r"[?&]id=(\d+)", true),
        "viruseproject" => num(url, r"[#?&]id=(\d+)", true),
        _ => 0,
    }
}

