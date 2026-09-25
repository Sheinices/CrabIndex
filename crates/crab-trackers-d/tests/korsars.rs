mod support;

use chrono::{TimeZone, Utc};
use crab_core::models::TaskParse;
use crab_trackers_d::korsars::{categories, parser};

const HOST: &str = "https://korsars.pro";

#[test]
fn categories_match_forum_ids() {
    assert_eq!(parser::TRACKER_NAME, "korsars");
    assert_eq!(parser::TOPICS_PER_PAGE, 50);
    assert_eq!(categories::MOVIE_IDS.len(), 6);
    assert_eq!(categories::SERIAL_IDS.len(), 12);
    assert_eq!(categories::CARTOON_IDS.len(), 6);
    assert_eq!(categories::count(), 24);
    assert!(categories::MOVIE_IDS.contains(&"282"));
    assert!(categories::SERIAL_IDS.contains(&"287"));
    assert!(categories::CARTOON_IDS.contains(&"43"));
    assert_eq!(parser::category_types("282"), &["movie"]);
    assert_eq!(parser::category_types("287"), &["serial"]);
    assert_eq!(parser::category_types("43"), &["multfilm", "multserial"]);
    assert_eq!(parser::category_types("999"), &["movie"]);
}

#[test]
fn parse_listing_html_movie_fixture_has_inline_magnets() {
    let html = support::read("Korsars/listing_movie.html");
    let items = parser::parse_listing_html(&html, "282", HOST);
    assert!(!items.is_empty(), "expected >=1 movie topics");
    for t in &items {
        assert_eq!(t.trackerName, "korsars");
        assert_eq!(t.types, vec!["movie".to_string()]);
        assert!(t.url.to_lowercase().starts_with(&format!("{HOST}/viewtopic.php?t=")));
        assert!(!t.name.trim().is_empty());
        assert!(!t.title.trim().is_empty());
        assert!(t.magnet.to_lowercase().starts_with("magnet:?xt=urn:btih:"));
        assert!(!t.magnet.contains("&amp;"));
        assert!(t.sid >= 0 && t.pir >= 0);
    }
    assert!(items[0].url.contains("viewtopic.php?t="));
}

#[test]
fn parse_listing_html_serial_fixture_parses_season_titles() {
    let html = support::read("Korsars/listing_serial.html");
    let items = parser::parse_listing_html(&html, "287", HOST);
    assert!(!items.is_empty(), "expected >=1 serial topics");
    for t in &items {
        assert_eq!(t.types, vec!["serial".to_string()]);
        assert!(!t.name.trim().is_empty());
        assert!(t.magnet.to_lowercase().starts_with("magnet:?xt=urn:btih:"));
    }
}

#[test]
fn parse_listing_html_cartoon_fixture_emits_both_types() {
    let html = support::read("Korsars/listing_cartoon.html");
    let items = parser::parse_listing_html(&html, "43", HOST);
    for t in &items {
        assert_eq!(t.types, vec!["multfilm".to_string(), "multserial".to_string()]);
    }
}

#[test]
fn last_page_from_html_movie_fixture_is_14() {
    let html = support::read("Korsars/listing_movie.html");
    assert_eq!(parser::last_page_from_html(&html), 14);
    assert_eq!(parser::last_page_from_html("<html>no pagination</html>"), 0);
}

#[test]
fn prune_pages_beyond_max_drops_ghost_tail() {
    let mut tasks: Vec<TaskParse> = (0..20).map(TaskParse::new).collect();
    assert_eq!(parser::prune_pages_beyond_max(&mut tasks, 11), 8);
    assert_eq!(tasks.len(), 12);
    assert_eq!(tasks.last().map(|t| t.page), Some(11));
    assert_eq!(parser::prune_pages_beyond_max(&mut tasks, 11), 0);
    assert_eq!(parser::prune_pages_beyond_max(&mut Vec::new(), 5), 0);
}

#[test]
fn looks_like_login_form_detects_session_expiry() {
    assert!(parser::looks_like_login_form(r#"<form><input name="login_username" /><input name="login_password" /></form>"#));
    assert!(!parser::looks_like_login_form(r#"<a id="tt-1"><b>Title</b></a><input name="login_username" />"#));
}

#[test]
fn parse_title_shapes() {
    let cases: &[(&str, &str, &str, i32)] = &[
        ("Игра престолов / Game of Thrones / Game of Thrones [S01] (2011) WEB-DL", "Игра престолов", "Game of Thrones", 2011),
        ("Во все тяжкие / Breaking Bad [S01-05] (2008) BDRip", "Во все тяжкие", "Breaking Bad", 2008),
        ("Чернобыль [S01] (2019) WEBRip", "Чернобыль", "", 2019),
        ("Матрица / The Matrix / Matrix (1999) BDRemux", "Матрица", "Matrix", 1999),
        ("Начало / Inception (2010) BDRip 1080p", "Начало", "Inception", 2010),
        ("Солярис (1972) DVDRip", "Солярис", "", 1972),
    ];
    for (title, name, orig, year) in cases {
        let (n, o, y) = parser::parse_title(title);
        assert_eq!((n.as_str(), o.as_str(), y), (*name, *orig, *year), "{title}");
    }
}

#[test]
fn first_token_title_fallback() {
    assert_eq!(parser::first_token_title("Something [S01] (2020)"), "Something");
    assert_eq!(parser::first_token_title("A / B (2020)"), "A");
}

#[test]
fn parse_listing_date_moscow_to_utc() {
    assert_eq!(parser::parse_listing_date("2026-07-23 08:56"), Utc.with_ymd_and_hms(2026, 7, 23, 5, 56, 0).unwrap());
    assert!(crab_core::time::is_min(&parser::parse_listing_date("bad")));
}

#[test]
fn forum_url_pages() {
    assert_eq!(parser::forum_url("https://h/", "282", 0), "https://h/viewforum.php?f=282");
    assert_eq!(parser::forum_url("https://h", "282", 2), "https://h/viewforum.php?f=282&start=100");
}
