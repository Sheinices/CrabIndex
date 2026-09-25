mod support;

use chrono::{Duration, TimeZone, Timelike, Utc};
use crab_core::time;
use crab_trackers_b::mazepa::parser;

const HOST: &str = "https://mazepa.to";

#[test]
fn parse_torrents_from_category_page_fixture_yields_download_ids_without_magnets() {
    let html = support::read("Mazepa/forum_41.html");
    let torrents = parser::parse_torrents_from_category_page(&html, &["multfilm"], HOST);
    assert!(torrents.len() >= 45, "expected >=45 torrents, got {}", torrents.len());

    for d in &torrents {
        let t = &d.t;
        assert_eq!(t.trackerName, "mazepa");
        assert_eq!(t.types, ["multfilm"]);
        assert!(!t.title.trim().is_empty());
        assert!(!t.name.trim().is_empty());
        assert!(t.url.starts_with(&format!("{HOST}/viewtopic.php?t=")));
        assert!(!d.download_id.is_empty() && d.download_id.chars().all(|c| c.is_ascii_digit()));
        assert!(t.sizeName.to_lowercase().contains("gb"), "{}", t.sizeName);
        assert!(t.magnet.is_empty());
        assert!(!time::is_min(&t.createTime));
        assert!(t.sid >= 0 && t.pir >= 0);
    }

    let first: Vec<_> = torrents.iter().filter(|d| d.t.url.ends_with("t=101801")).collect();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].download_id, "101966");
    assert_eq!(first[0].t.sizeName, "3.99 GB");
    assert!(first[0].t.title.contains("Голос океану"));
    assert_eq!(first[0].t.quality, 1080);
}

#[test]
fn parse_torrents_from_category_page_empty_html_returns_empty() {
    assert!(parser::parse_torrents_from_category_page("", &["multfilm"], HOST).is_empty());
    assert!(parser::parse_torrents_from_category_page("<html></html>", &["multfilm"], HOST).is_empty());
}

#[test]
fn parse_mazepa_date_relative_ukrainian_succeeds() {
    for text in ["Сьогодні 12:21", "Вчора 18:05"] {
        assert!(parser::parse_mazepa_date(text).is_some(), "{text}");
    }
}

#[test]
fn parse_mazepa_date_today_uses_utc_today() {
    let dt = parser::parse_mazepa_date("Сьогодні 12:21").unwrap();
    assert_eq!(dt.date_naive(), Utc::now().date_naive());
    assert_eq!((dt.hour(), dt.minute()), (12, 21));
}

#[test]
fn parse_mazepa_date_yesterday_uses_utc_yesterday() {
    let dt = parser::parse_mazepa_date("Вчора 18:05").unwrap();
    assert_eq!(dt.date_naive(), Utc::now().date_naive() - Duration::days(1));
    assert_eq!((dt.hour(), dt.minute()), (18, 5));
}

#[test]
fn parse_mazepa_date_absolute_ukrainian_succeeds() {
    let cases = [
        ("4 Лис 2025, 13:00", 2025, 11, 4, 13, 0),
        ("18 Жов 2025, 11:47", 2025, 10, 18, 11, 47),
        ("30 Вер 2025, 14:23", 2025, 9, 30, 14, 23),
    ];
    for (text, y, mo, d, h, mi) in cases {
        assert_eq!(parser::parse_mazepa_date(text), Some(Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()), "{text}");
    }
}

#[test]
fn parse_mazepa_date_empty_returns_none() {
    assert_eq!(parser::parse_mazepa_date(""), None);
    assert_eq!(parser::parse_mazepa_date("not a date"), None);
}
