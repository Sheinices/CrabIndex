mod support;

use chrono::{Datelike, Timelike};
use crab_trackers_d::viruseproject::{categories, parser};

const HOST: &str = "https://viruseproject.tv";

fn browse_fixture_cases() -> Vec<(&'static str, String, &'static [&'static str])> {
    let mut v: Vec<_> = categories::MAP.iter().map(|c| (c.slug, format!("browse_{}.html", c.slug), c.types)).collect();
    v.sort_by(|a, b| a.0.cmp(b.0));
    v
}

#[test]
fn categories_cover_expected_slugs() {
    assert_eq!(categories::MAP.len(), 5);
    assert_eq!(parser::try_get_types("movies"), Some(&["movie"][..]));
    assert_eq!(parser::try_get_types("documentary"), Some(&["docuserial", "documovie"][..]));
    assert_eq!(parser::get_page_step("movies"), 10);
    assert_eq!(parser::get_page_step("cartoons"), 6);
    assert_eq!(parser::get_page_step("unknown"), 10);
}

#[test]
fn browse_fixture_cases_cover_entire_category_map() {
    assert_eq!(browse_fixture_cases().len(), categories::MAP.len());
}

#[test]
fn extract_post_urls_browse_fixtures_yield_posts() {
    for (cat, file, _types) in browse_fixture_cases() {
        let html = support::read(&format!("Viruseproject/{file}"));
        let urls = parser::extract_post_urls(&html, HOST);
        assert!(!urls.is_empty(), "expected >=1 post urls for cat {cat}");
        for u in &urls {
            assert!(u.to_lowercase().starts_with(HOST), "{u}");
            assert!(u.to_lowercase().contains("/releases/"), "{u}");
        }
    }
}

#[test]
fn parse_detail_html_fixture_yields_one_record_per_quality() {
    let html = support::read("Viruseproject/detail_sample.html");
    let post_url = format!("{HOST}/releases/serials/shugar-sugar-sezon-2");
    let torrents = parser::parse_detail_html(&html, &post_url, HOST, &["serial"]);
    assert!(torrents.len() >= 2, "expected >=2 quality records, got {}", torrents.len());
    for t in &torrents {
        assert_eq!(t.t.trackerName, "viruseproject");
        assert_eq!(t.t.types, vec!["serial".to_string()]);
        assert!(!t.t.name.trim().is_empty());
        assert!(!t.t.title.trim().is_empty());
        assert!(t.t.url.starts_with(&format!("{post_url}#q=")));
        assert!(t.t.url.contains("&id="));
        assert!(t.download_uri.contains("/download/"));
        assert!(!t.t.sizeName.trim().is_empty());
        assert!(t.t.sid >= 1);
        assert!(t.t.quality > 0);
    }
    let first = &torrents[0];
    assert_eq!(first.t.name, "Шугар");
    assert_eq!(first.t.originalname, "Sugar");
    assert_eq!(first.t.relased, 2026);
    assert_eq!(first.t.videotype, "WEBRip");
    assert!(first.t.title.contains("[WEBRip]"));
    let hd = torrents.iter().find(|t| t.t.quality == 1080).expect("1080p record");
    assert!(hd.t.title.contains("[1080p]"));
}

#[test]
fn detect_last_page_from_pagination_end() {
    let html = support::read("Viruseproject/browse_movies.html");
    assert!(parser::detect_last_page(&html, 10) >= 2);
    assert_eq!(parser::detect_last_page("", 10), 1);
    assert_eq!(parser::detect_last_page(r#"<li class="pagination-end"><a href="/releases/movies?start=20">end</a></li>"#, 10), 3);
}

#[test]
fn extract_post_urls_empty_returns_empty() {
    assert!(parser::extract_post_urls("", HOST).is_empty());
    assert!(parser::extract_post_urls("<html></html>", HOST).is_empty());
}

#[test]
fn parse_detail_html_empty_returns_empty() {
    assert!(parser::parse_detail_html("", &format!("{HOST}/x"), HOST, &["movie"]).is_empty());
    assert!(parser::parse_detail_html("<html></html>", &format!("{HOST}/x"), HOST, &["movie"]).is_empty());
}

#[test]
fn parse_names_cases() {
    let cases = [
        ("Ведьмак: Сирены глубин / The Witcher: Sirens of the Deep / 2025", "Ведьмак: Сирены глубин", "The Witcher: Sirens of the Deep"),
        ("Вершина / Apex / 2026", "Вершина", "Apex"),
        ("Соперники / Rivals / сезон 2 / 1-3 из 12", "Соперники", "Rivals"),
        ("Адская Кухня 11 (Hell's Kitchen 11)", "Адская Кухня 11", "Hell's Kitchen 11"),
        ("Шествие смерти (Death Parade)", "Шествие смерти", "Death Parade"),
        ("Фоллаут / Fallout / сезон 2", "Фоллаут", "Fallout"),
    ];
    for (raw, ru, en) in cases {
        let (r, e) = parser::parse_names(raw);
        assert_eq!((r.as_str(), e.as_str()), (ru, en), "{raw}");
    }
}

#[test]
fn parse_russian_date_cases() {
    let cases = [
        ("Четверг, 13 Февраль 2025 00:00", 2025, 2, 13),
        ("Вторник, 12 Май 2026 00:00", 2026, 5, 12),
        ("Понедельник, 23 Мая 2026 00:00", 2026, 5, 23),
        ("Среда, 07 Май 2014 00:00", 2014, 5, 7),
    ];
    for (raw, y, m, d) in cases {
        let got = parser::parse_russian_date(raw);
        assert_eq!((got.year(), got.month(), got.day(), got.hour(), got.minute()), (y, m, d, 0, 0), "{raw}");
    }
}
