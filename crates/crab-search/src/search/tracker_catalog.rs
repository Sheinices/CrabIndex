//! Tracker names exposed by `/api/v1.0/trackers`, the Torznab indexer list and `/api/v2.0/indexers`.

use crab_core::conf;
use crab_core::util::is_blank;

/// Built-in tracker slugs.
pub const KNOWN_TRACKER_SLUGS: [&str; 25] = [
    "rutracker", "rutor", "kinozal", "nnmclub", "megapeer", "bitru", "toloka", "mazepa", "lostfilm", "baibako", "torrentby",
    "selezen", "animelayer", "anidub", "anistar", "anibelka", "aniliberty", "knaben", "leproduction", "viruseproject",
    "anifilm", "korsars", "ultradox", "rudub", "subsplease",
];

/// `synctrackers` when set (an empty list means none), otherwise the built-in slugs;
/// minus `disable_trackers`, trimmed, distinct and sorted (case-insensitive).
pub fn tracker_names() -> Vec<String> {
    let c = conf();
    match c.synctrackers.as_ref() {
        Some(list) => from_configured(list.iter().map(String::as_str), &c.disable_trackers),
        None => from_configured(KNOWN_TRACKER_SLUGS.iter().copied(), &c.disable_trackers),
    }
}

pub fn from_configured<'a>(source: impl Iterator<Item = &'a str>, disabled: &[String]) -> Vec<String> {
    let disabled: Vec<String> = disabled.iter().map(|d| d.to_uppercase()).collect();
    let mut out: Vec<String> = Vec::new();
    for i in source {
        if is_blank(i) {
            continue;
        }
        let t = i.trim();
        if disabled.contains(&t.to_uppercase()) {
            continue;
        }
        if out.iter().any(|x| x.to_uppercase() == t.to_uppercase()) {
            continue;
        }
        out.push(t.to_string());
    }
    out.sort_by_key(|a| a.to_uppercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_minus_disabled() {
        let names = from_configured(KNOWN_TRACKER_SLUGS.iter().copied(), &["rutor".into(), "KINOZAL".into()]);
        assert!(!names.iter().any(|n| n.eq_ignore_ascii_case("rutor")));
        assert!(!names.iter().any(|n| n.eq_ignore_ascii_case("kinozal")));
        assert!(names.contains(&"rutracker".to_string()));
        let mut sorted = names.clone();
        sorted.sort_by_key(|a| a.to_uppercase());
        assert_eq!(names, sorted);
        assert!(names.iter().all(|n| KNOWN_TRACKER_SLUGS.contains(&n.as_str())));
    }

    #[test]
    fn configured_list() {
        let names = from_configured(["rutracker", "kinozal", "rutracker"].into_iter(), &["kinozal".into()]);
        assert_eq!(names, vec!["rutracker"]);
    }

    #[test]
    fn empty_configured_list() {
        assert!(from_configured(std::iter::empty(), &[]).is_empty());
    }
}
