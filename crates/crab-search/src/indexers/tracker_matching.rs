//! Tracker name parsing/matching shared by the native, Jackett and Torznab APIs.
//! Stored `trackerName` may be comma-joined after duplicate merge ("kinozal, rutracker").

use crab_core::util::is_blank;

use super::request::IndexerSearchRequest;

/// Case-insensitive allow set (keys stored lowercase).
#[derive(Clone, Debug, Default)]
pub struct AllowSet(std::collections::HashSet<String>);

impl AllowSet {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.0.contains(&name.to_lowercase())
    }
}

/// Comma separated list → trimmed, non-empty, distinct (case-insensitive, first spelling wins).
pub fn parse_list(value: &str) -> Vec<String> {
    if is_blank(value) {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for part in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if !out.iter().any(|x| x.to_lowercase() == part.to_lowercase()) {
            out.push(part.to_string());
        }
    }
    out
}

pub fn to_allow_set<S: AsRef<str>>(trackers: &[S]) -> AllowSet {
    AllowSet(
        trackers
            .iter()
            .map(|s| s.as_ref())
            .filter(|s| !is_blank(s))
            .map(|s| s.trim().to_lowercase())
            .collect(),
    )
}

/// True when the filter list is empty or any comma-separated part of `tracker_name` is allowed.
pub fn matches_list<S: AsRef<str>>(tracker_name: Option<&str>, trackers: &[S]) -> bool {
    if trackers.is_empty() {
        return true;
    }
    matches(tracker_name, &to_allow_set(trackers))
}

pub fn matches(tracker_name: Option<&str>, allowed: &AllowSet) -> bool {
    if allowed.is_empty() {
        return true;
    }
    let Some(name) = tracker_name.filter(|s| !is_blank(s)) else {
        return false;
    };
    name.split(',').map(str::trim).any(|p| !p.is_empty() && allowed.contains(p))
}

/// When the route indexer is a specific tracker (not empty/"all"/"status:healthy"/numeric id),
/// add it to the request tracker filter.
pub fn apply_indexer_path_filter(req: &mut IndexerSearchRequest, indexer: Option<&str>) {
    let Some(indexer) = indexer else {
        return;
    };
    if is_all_indexer(indexer) {
        return;
    }
    if indexer.trim().parse::<i32>().is_ok() {
        return;
    }
    let name = indexer.trim().to_string();
    if !req.trackers.iter().any(|t| t.to_lowercase() == name.to_lowercase()) {
        req.trackers.push(name.clone());
    }
    if req.tracker.as_deref().map(is_blank).unwrap_or(true) {
        req.tracker = Some(name);
    }
}

pub fn is_all_indexer(indexer: &str) -> bool {
    is_blank(indexer) || indexer.eq_ignore_ascii_case("all") || indexer.eq_ignore_ascii_case("status:healthy")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexers::filters;
    use crab_core::models::api::Result;

    #[test]
    fn parse_list_splits_trimmed_distinct_ignore_case() {
        assert_eq!(parse_list("kinozal, RUTRACKER, kinozal,  ,toloka"), vec!["kinozal", "RUTRACKER", "toloka"]);
    }

    #[test]
    fn matches_empty_allowlist_allows_any() {
        let empty: Vec<String> = Vec::new();
        assert!(matches_list(Some("rutracker"), &empty));
    }

    #[test]
    fn matches_merged_tracker_name_matches_any_part_ignore_case() {
        let allowed = to_allow_set(&["RUTRACKER"]);
        assert!(matches(Some("kinozal, rutracker"), &allowed));
        assert!(!matches(Some("kinozal, rutor"), &allowed));
        assert!(!matches(None, &allowed));
    }

    #[test]
    fn apply_indexer_path_filter_adds_specific_indexer() {
        let mut req = IndexerSearchRequest { trackers: vec!["kinozal".into()], ..Default::default() };
        apply_indexer_path_filter(&mut req, Some("rutracker"));
        assert!(req.trackers.contains(&"rutracker".to_string()));
        assert!(req.trackers.contains(&"kinozal".to_string()));
        assert_eq!(req.tracker.as_deref(), Some("rutracker"));
    }

    #[test]
    fn apply_indexer_path_filter_ignores_numeric_all_and_healthy() {
        for ix in ["1", "all", "status:healthy"] {
            let mut req = IndexerSearchRequest::default();
            apply_indexer_path_filter(&mut req, Some(ix));
            assert!(req.trackers.is_empty(), "{ix}");
            assert!(req.tracker.is_none(), "{ix}");
        }
    }

    #[test]
    fn filter_by_trackers_uses_shared_matcher() {
        let mk = |tracker: &str, title: &str| Result {
            Tracker: Some(tracker.into()),
            Title: Some(title.into()),
            ..filters::empty_result()
        };
        let items = vec![mk("kinozal, rutracker", "a"), mk("rutor", "b")];
        let filtered = filters::filter_by_trackers(items, &["RUTRACKER".to_string()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].Title.as_deref(), Some("a"));
    }
}
