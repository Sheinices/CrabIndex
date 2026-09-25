mod support;

use chrono::Utc;
use crab_trackers_d::anifilm::{self, categories, parser};

const HOST: &str = "https://anifilm.pro";

#[test]
fn categories_cover_expected_slugs() {
    assert_eq!(categories::MAP.len(), 8);
    let serials = categories::get("serials").expect("serials");
    assert_eq!(serials.types, &["anime"]);
    let dorams = categories::get("dorams").expect("dorams");
    assert_eq!(dorams.types, &["serial"]);
    assert_eq!(categories::max_pages(serials, true), 70);
    assert_eq!(categories::max_pages(serials, false), 2);
}

#[test]
fn parse_listing_html_listing_fixture_yields_typed_items() {
    let html = support::read("Anifilm/listing_serials.html");
    let items = parser::parse_listing_html(&html, HOST, &["anime"], Utc::now());
    assert!(items.len() >= 2, "expected >=2 items, got {}", items.len());
    for t in &items {
        assert_eq!(t.t.trackerName, "anifilm");
        assert_eq!(t.t.types, vec!["anime".to_string()]);
        assert!(!t.t.name.trim().is_empty());
        assert!(!t.t.title.trim().is_empty());
        assert!(t.t.url.to_lowercase().starts_with(&format!("{HOST}/releases/")));
        assert_eq!(t.t.sid, 1);
    }
    if items[0].t.url.contains("/releases/test-show-1") {
        assert_eq!(items[0].t.name, "Тестовое аниме");
        assert_eq!(items[0].t.originalname, "Test Anime");
        assert!(items[0].t.title.contains("12"));
        assert_eq!(items[0].t.relased, 2024);
        assert_eq!(items[1].t.relased, 2023);
    }
}

#[test]
fn extract_torrent_download_path_detail_fixture_prefers_1080p() {
    let html = support::read("Anifilm/detail_sample.html");
    let (tid, is1080p) = parser::extract_torrent_download_path(&html);
    assert_eq!(tid.as_deref(), Some("releases/download-torrent/101"));
    assert!(is1080p);
}

#[test]
fn extract_torrent_download_path_fallback_without_1080() {
    let html = r#"
<li class="release__torrents-item">
  720p
  <a href="/releases/download-torrent/55">скачать</a>
</li>
"#;
    let (tid, is1080p) = parser::extract_torrent_download_path(html);
    assert_eq!(tid.as_deref(), Some("releases/download-torrent/55"));
    assert!(!is1080p);
}

#[test]
fn parse_listing_html_empty_or_invalid_returns_empty() {
    assert!(parser::parse_listing_html("", HOST, &["anime"], Utc::now()).is_empty());
    assert!(parser::parse_listing_html("<html></html>", HOST, &["anime"], Utc::now()).is_empty());
    assert!(parser::extract_torrent_download_path("").0.is_none());
    assert!(parser::extract_torrent_download_path("<html></html>").0.is_none());
}

#[test]
fn login_form_detection_and_cookie_merge() {
    assert!(anifilm::looks_like_login_form(r#"<form action="/account/login" method="post">"#));
    assert!(anifilm::looks_like_login_form("<form ACTION='/account/login'>"));
    assert!(!anifilm::looks_like_login_form(""));
    assert_eq!(anifilm::merge_cookie_strings("a=1; b=2", "B=3; c=4"), "a=1; b=3; c=4");
    assert_eq!(anifilm::merge_cookie_strings("", "_csrf=x"), "_csrf=x");
}
