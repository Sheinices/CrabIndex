mod common;

use chrono::{TimeZone, Utc};
use common::fixture;
use crab_trackers_e::rudub::{self, parser};

const HOST: &str = "https://r4.rudub.world";

#[test]
fn constants_match_site_contract() {
    assert_eq!(parser::TRACKER_NAME, "rudub");
    assert_eq!(parser::VALIDATION_MARKER, "card__torlist__browse_2");
    assert_eq!(parser::ENDPOINT_DOWNLOAD, "/download2.php");
    assert_eq!(parser::PREFERRED_VIDEO_FORMATS, [4, 5]);
}

#[test]
fn is_preferred_quality_title_gates_ladder() {
    let cases = [
        ("Пробуждение (Ontwaak) Сезон 1 (HD1080p WEBRip)", true),
        ("Show (Original) (HD2160p WEBRip)", true),
        ("Show (Original) (BD1080p)", true),
        ("Пробуждение (Ontwaak) Сезон 1 (WEBRip XviD)", false),
        ("Пробуждение (Ontwaak) Сезон 1 (HD720p WEBRip)", false),
        ("Show (Original) (WEBRip x264)", false),
        ("Show (Original) (720p)", false),
    ];
    for (title, expected) in cases {
        assert_eq!(parser::is_preferred_quality_title(title), expected, "title={title}");
    }
}

#[test]
fn listing_fixture_keeps_1080_and_drops_sd_and_720() {
    let html = fixture("rudub/listing_sample.html");
    let items = parser::parse_torrent_list_from_html(&html, HOST);
    assert!(items.len() >= 4, "expected >=4 HD1080 cards, got {}", items.len());

    for d in &items {
        let t = &d.t;
        assert_eq!(t.trackerName, "rudub");
        assert_eq!(t.types, vec!["serial"]);
        assert!(t.url.to_lowercase().starts_with(&format!("{HOST}/details.php?id=").to_lowercase()));
        assert!(d.download_uri.to_lowercase().starts_with(&format!("{HOST}/download2.php?id=").to_lowercase()));
        assert!(!t.name.trim().is_empty());
        assert!(!t.originalname.trim().is_empty());
        assert!(!t.sizeName.trim().is_empty());
        assert!(t.quality == 1080 || t.quality == 2160, "quality={} title={}", t.quality, t.title);
        assert!(t.relased > 0, "relased={} title={}", t.relased, t.title);
        assert!(!t.title.to_lowercase().contains("xvid"));
        assert!(!t.title.to_lowercase().contains("hd720p"));
        assert!(parser::is_preferred_quality_title(&t.title));
    }

    let first = items.iter().find(|d| d.t.url.contains("id=55190")).expect("id=55190");
    assert_eq!(first.t.name, "Рулевая");
    assert_eq!(first.t.originalname, "Crew Girl");
    assert_eq!(first.t.quality, 1080);
    assert_eq!(first.t.sid, 2);
    assert_eq!(first.t.pir, 3);
    assert_eq!(first.t.sizeName, "12.55 GB");
    assert_eq!(first.t.createTime, Utc.with_ymd_and_hms(2026, 9, 10, 21, 50, 38).unwrap());
    assert_eq!(first.t.relased, 2026);
    assert_eq!(first.download_uri, format!("{HOST}/download2.php?id=55190"));
}

#[test]
fn title_fields_use_create_time_year_when_no_year_group() {
    let created = Utc.with_ymd_and_hms(2026, 9, 7, 19, 2, 58).unwrap();
    let (name, original, relased) = parser::parse_title_fields("Фонари (Lanterns) Сезон 1 Серии 01-04 (HD1080p WEBRip)", created);
    assert_eq!(name.as_deref(), Some("Фонари"));
    assert_eq!(original.as_deref(), Some("Lanterns"));
    assert_eq!(relased, 2026);

    let created = Utc.with_ymd_and_hms(2026, 9, 2, 19, 3, 28).unwrap();
    let (name, original, relased) = parser::parse_title_fields("Мистер Килл (Mr. Kill) Сезон 1 Серии 01-09 (HD1080p WEBRip)", created);
    assert_eq!(name.as_deref(), Some("Мистер Килл"));
    assert_eq!(original.as_deref(), Some("Mr. Kill"));
    assert_eq!(relased, 2026);
}

#[test]
fn title_fields_year_paren_is_relased_not_original() {
    let created = Utc.with_ymd_and_hms(2026, 7, 29, 0, 0, 0).unwrap();
    let (name, original, relased) = parser::parse_title_fields("Гнев (2026) (Furia (Wrath)) Сезон 1 Серии 01-06 (HD1080p WEBRip)", created);
    assert_eq!(name.as_deref(), Some("Гнев"));
    assert_eq!(original.as_deref(), Some("Furia (Wrath)"));
    assert_eq!(relased, 2026);
}

#[test]
fn title_fields_nested_original_keeps_balanced_parens() {
    let created = Utc.with_ymd_and_hms(2026, 9, 8, 0, 0, 0).unwrap();
    let (name, original, relased) = parser::parse_title_fields(
        "Мой любимый сотрудник (My Bias, My Boss (Choeaeui sawon)) Сезон 1 Серии 01-08 (HD1080p WEBRip)",
        created,
    );
    assert_eq!(name.as_deref(), Some("Мой любимый сотрудник"));
    assert_eq!(original.as_deref(), Some("My Bias, My Boss (Choeaeui sawon)"));
    assert_eq!(relased, 2026);
}

#[test]
fn listing_empty_inputs_return_empty() {
    assert!(parser::parse_torrent_list_from_html("", HOST).is_empty());
    assert!(parser::parse_torrent_list_from_html("<html></html>", HOST).is_empty());
    assert!(parser::parse_torrent_list_from_html("<div class=\"card__torlist__browse_2\"></div>", "").is_empty());
}

#[test]
fn types_equal_works() {
    let serial = vec!["serial".to_string()];
    let movie = vec!["movie".to_string()];
    assert!(parser::types_equal(None, None));
    assert!(!parser::types_equal(Some(&serial), None));
    assert!(parser::types_equal(Some(&serial), Some(&serial)));
    assert!(!parser::types_equal(Some(&serial), Some(&movie)));
}

#[test]
fn resolve_page_range_matches_contract() {
    let cases = [
        (0, 0, 0, 0, 0),
        (0, 0, 10, 0, 9),
        (0, 0, 50, 0, 49),
        (0, 0, 200, 0, 99), // clamped to MAX_LIMIT_PAGES
        (5, 12, 0, 5, 12),
        (5, 12, 50, 5, 12), // explicit range wins over limit_page
        (12, 5, 0, 5, 12),  // swapped
    ];
    for (from, to, limit, want_start, want_end) in cases {
        assert_eq!(rudub::resolve_page_range(from, to, limit), (want_start, want_end), "from={from} to={to} limit={limit}");
    }
}

#[test]
fn max_limit_pages_is_100() {
    assert_eq!(rudub::MAX_LIMIT_PAGES, 100);
}

#[test]
fn bencoded_torrent_check() {
    assert!(parser::is_valid_bencoded_torrent(b"d8:announce"));
    assert!(!parser::is_valid_bencoded_torrent(b"<html>"));
    assert!(!parser::is_valid_bencoded_torrent(b""));
}

#[test]
fn cookie_value_extraction() {
    let headers = vec![
        "PHPSESSID=abc123; path=/".to_string(),
        "uid=42; expires=Thu, 01 Jan 2030 00:00:00 GMT".to_string(),
        "pass=deadbeef".to_string(),
    ];
    assert_eq!(rudub::extract_cookie_value(&headers, "PHPSESSID").as_deref(), Some("abc123"));
    assert_eq!(rudub::extract_cookie_value(&headers, "uid").as_deref(), Some("42"));
    assert_eq!(rudub::extract_cookie_value(&headers, "pass").as_deref(), Some("deadbeef"));
    assert_eq!(rudub::extract_cookie_value(&headers, "missing"), None);
}
