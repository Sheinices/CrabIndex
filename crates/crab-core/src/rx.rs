//! Regex helpers used by the parsers.
//!
//! * Patterns are compiled once and cached (keyed by pattern + flags).
//! * Backed by `fancy_regex`, so lookarounds / backreferences work.
//! * Group accessors return `""` for a missing match/group.
//! * Replacement strings use `$1` / `${name}`; write `${1}x` when a digit/letter follows.
//! * [`split`] drops captured groups; [`split_with_captures`] keeps them in the output.

use dashmap::DashMap;
use fancy_regex::{Captures, Regex, RegexBuilder};
use once_cell::sync::Lazy;
use std::sync::Arc;

static CACHE: Lazy<DashMap<(String, bool), Arc<Regex>>> = Lazy::new(DashMap::new);

fn build(pattern: &str, ignore_case: bool) -> Arc<Regex> {
    let key = (pattern.to_string(), ignore_case);
    if let Some(r) = CACHE.get(&key) {
        return r.clone();
    }
    // The flag is embedded in the pattern: fancy_regex ignores builder case-insensitivity
    // for patterns that need backtracking (\b, lookarounds, backreferences).
    let full = if ignore_case { format!("(?i){pattern}") } else { pattern.to_string() };
    let r = RegexBuilder::new(&full)
        .backtrack_limit(10_000_000)
        .build()
        .unwrap_or_else(|e| panic!("invalid regex {pattern:?}: {e}"));
    let r = Arc::new(r);
    CACHE.insert(key, r.clone());
    r
}

/// Cached case-sensitive regex.
pub fn re(pattern: &str) -> Arc<Regex> {
    build(pattern, false)
}

/// Cached case-insensitive regex.
pub fn re_i(pattern: &str) -> Arc<Regex> {
    build(pattern, true)
}

fn get(pattern: &str, ic: bool) -> Arc<Regex> {
    build(pattern, ic)
}

pub fn is_match(text: &str, pattern: &str) -> bool {
    get(pattern, false).is_match(text).unwrap_or(false)
}

pub fn is_match_i(text: &str, pattern: &str) -> bool {
    get(pattern, true).is_match(text).unwrap_or(false)
}

/// First match captures.
pub fn captures<'t>(text: &'t str, pattern: &str) -> Option<Captures<'t>> {
    get(pattern, false).captures(text).ok().flatten()
}

pub fn captures_i<'t>(text: &'t str, pattern: &str) -> Option<Captures<'t>> {
    get(pattern, true).captures(text).ok().flatten()
}

/// Group `idx` of the first match ("" if no match).
pub fn group(text: &str, pattern: &str, idx: usize) -> String {
    captures(text, pattern).and_then(|c| c.get(idx).map(|m| m.as_str().to_string())).unwrap_or_default()
}

pub fn group_i(text: &str, pattern: &str, idx: usize) -> String {
    captures_i(text, pattern).and_then(|c| c.get(idx).map(|m| m.as_str().to_string())).unwrap_or_default()
}

/// All groups of the first match as owned strings (index 0 = whole match); missing groups are "".
pub fn groups(text: &str, pattern: &str) -> Vec<String> {
    groups_impl(text, pattern, false)
}

pub fn groups_i(text: &str, pattern: &str) -> Vec<String> {
    groups_impl(text, pattern, true)
}

fn groups_impl(text: &str, pattern: &str, ic: bool) -> Vec<String> {
    let r = get(pattern, ic);
    let n = r.captures_len();
    match r.captures(text).ok().flatten() {
        Some(c) => (0..n).map(|i| c.get(i).map(|m| m.as_str().to_string()).unwrap_or_default()).collect(),
        None => vec![String::new(); n],
    }
}

/// Groups of every match.
pub fn all_groups(text: &str, pattern: &str) -> Vec<Vec<String>> {
    all_groups_impl(text, pattern, false)
}

pub fn all_groups_i(text: &str, pattern: &str) -> Vec<Vec<String>> {
    all_groups_impl(text, pattern, true)
}

fn all_groups_impl(text: &str, pattern: &str, ic: bool) -> Vec<Vec<String>> {
    let r = get(pattern, ic);
    let n = r.captures_len();
    r.captures_iter(text)
        .filter_map(|c| c.ok())
        .map(|c| (0..n).map(|i| c.get(i).map(|m| m.as_str().to_string()).unwrap_or_default()).collect())
        .collect()
}

/// Replace all matches.
pub fn replace(text: &str, pattern: &str, replacement: &str) -> String {
    get(pattern, false).replace_all(text, replacement).into_owned()
}

pub fn replace_i(text: &str, pattern: &str, replacement: &str) -> String {
    get(pattern, true).replace_all(text, replacement).into_owned()
}

fn split_impl(text: &str, pattern: &str, ic: bool, with_caps: bool) -> Vec<String> {
    let r = get(pattern, ic);
    let mut out = Vec::new();
    let mut last = 0;
    for c in r.captures_iter(text).filter_map(|c| c.ok()) {
        let m = c.get(0).unwrap();
        if m.start() == m.end() && m.start() == 0 {
            continue;
        }
        out.push(text[last..m.start()].to_string());
        if with_caps {
            for i in 1..c.len() {
                if let Some(g) = c.get(i) {
                    out.push(g.as_str().to_string());
                }
            }
        }
        last = m.end();
    }
    out.push(text[last..].to_string());
    out
}

/// Split without captured groups in the output.
pub fn split(text: &str, pattern: &str) -> Vec<String> {
    split_impl(text, pattern, false, false)
}

pub fn split_i(text: &str, pattern: &str) -> Vec<String> {
    split_impl(text, pattern, true, false)
}

/// Split that also returns captured groups between the pieces.
pub fn split_with_captures(text: &str, pattern: &str) -> Vec<String> {
    split_impl(text, pattern, false, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert_eq!(group("abc 2020)", r"([0-9]{4})\)", 1), "2020");
        assert_eq!(group("abc", r"([0-9]{4})", 1), "");
        assert_eq!(split("a<tr class=\"gai\">b<tr class=\"tum\">c", "<tr class=\"(gai|tum)\">"), vec!["a", "b", "c"]);
        assert_eq!(
            split_with_captures("a<tr class=\"gai\">b", "<tr class=\"(gai|tum)\">"),
            vec!["a", "gai", "b"]
        );
        assert_eq!(replace("a  b", "[ ]+", " "), "a b");
        assert!(is_match_i("HELLO", "hello"));
        assert!(is_match("foo123", r"(?<=foo)\d+"));
    }
}

#[cfg(test)]
mod ic_tests {
    #[test]
    fn ignore_case_with_backtracking_features() {
        assert_eq!(super::replace_i("2160P", r"\b2160p\b", "x"), "x");
        assert!(super::is_match_i("FOO123", r"(?<=foo)\d+"));
    }
}
