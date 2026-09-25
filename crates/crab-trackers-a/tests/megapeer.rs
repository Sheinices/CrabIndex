mod support;

use chrono::{TimeZone, Utc};
use crab_core::{conf, time};
use crab_trackers_a::megapeer::parser;
use support::{fixture, pages, types};

#[test]
fn fixture_yields_movies() {
    let torrents = parser::parse_torrents_from_page(&fixture("megapeer/browse_79.html"), "79");
    assert!(torrents.len() >= 40, "expected >=40 torrents, got {}", torrents.len());
    let host = conf().Megapeer.host.trim_end_matches('/').to_string() + "/";
    for t in &torrents {
        assert_eq!(t.trackerName, "megapeer");
        assert_eq!(types(t), ["movie"]);
        assert!(!t.name.trim().is_empty());
        assert!(!t.title.trim().is_empty());
        assert!(t.url.starts_with(&host));
        assert!(t.url.contains("/torrent/"));
        assert!(!t.download_id.trim().is_empty());
        assert!(!t.sizeName.trim().is_empty());
        assert!(!time::is_min(&t.createTime));
    }
}

#[test]
fn colspan_row_parses_date_and_download() {
    let torrents = parser::parse_torrents_from_page(&fixture("megapeer/browse_79.html"), "79");
    let first = &torrents[0];
    assert_eq!(first.download_id, "211657");
    assert_eq!(first.createTime, Utc.with_ymd_and_hms(2026, 8, 25, 0, 0, 0).unwrap());
    assert!(first.title.contains("Огниво против Волшебной Скважины"));
    assert_eq!(first.name, "Огниво против Волшебной Скважины");
    assert_eq!(first.relased, 2025);
    assert_eq!(first.sizeName, "1.57 GB");
    assert!(first.url.ends_with("/torrent/211657"));
}

#[test]
fn legacy_bare_td_row_still_parses() {
    let torrents = parser::parse_torrents_from_page(&fixture("megapeer/browse_79.html"), "79");
    let legacy: Vec<_> = torrents.iter().filter(|t| t.download_id == "211111").collect();
    assert_eq!(legacy.len(), 1);
    let legacy = legacy[0];
    assert_eq!(legacy.createTime, Utc.with_ymd_and_hms(2026, 8, 13, 0, 0, 0).unwrap());
    assert!(legacy.title.contains("Дед Фомич"));
    assert_eq!(legacy.name, "Дед Фомич");
    assert_eq!(legacy.relased, 2026);
}

#[test]
fn empty_or_invalid_returns_empty() {
    assert!(parser::parse_torrents_from_page("", "79").is_empty());
    assert!(parser::parse_torrents_from_page("<html></html>", "79").is_empty());
}

#[test]
fn looks_like_browse_page_logo_without_rows_is_listing() {
    let empty = "<html><div id=\"logo\"></div></html>";
    assert!(parser::looks_like_browse_page(empty));
    assert!(parser::parse_torrents_from_page(empty, "79").is_empty());
    assert!(parser::looks_like_browse_page(&fixture("megapeer/browse_79.html")));
    assert!(!parser::looks_like_browse_page(""));
    assert!(!parser::looks_like_browse_page("<html></html>"));
}

#[test]
fn last_page_from_html_caps_at_10() {
    assert_eq!(parser::last_page_from_html(&fixture("megapeer/browse_79.html")), 10);
    assert_eq!(parser::last_page_from_html(""), 0);
    assert_eq!(parser::last_page_from_html(">Всего: 199"), 3);
    assert_eq!(parser::last_page_from_html(">Всего: 8134"), 10);
}

#[test]
fn prune_pages_beyond_max_drops_ghost_tail() {
    let mut tasks = pages(20);
    assert_eq!(parser::prune_pages_beyond_max(&mut tasks, 10), 9);
    assert_eq!(tasks.len(), 11);
    assert_eq!(tasks.last().unwrap().page, 10);
    assert_eq!(parser::prune_pages_beyond_max(&mut tasks, 10), 0);
    assert_eq!(parser::prune_pages_beyond_max(&mut Vec::new(), 5), 0);
}
