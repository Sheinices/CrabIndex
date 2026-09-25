mod support;

use crab_trackers_d::leproduction::{categories, parser};

const HOST: &str = "https://www.le-production.online";

fn browse_fixture_cases() -> Vec<(&'static str, String, &'static [&'static str])> {
    let mut v: Vec<_> = categories::MAP.iter().map(|c| (c.slug, format!("browse_{}.html", c.slug), c.types)).collect();
    v.sort_by(|a, b| a.0.cmp(b.0));
    v
}

#[test]
fn categories_cover_expected_slugs() {
    assert_eq!(categories::MAP.len(), 6);
    assert_eq!(parser::try_get_types("film"), Some(&["movie"][..]));
    assert_eq!(parser::try_get_types("FILM"), Some(&["movie"][..]));
    assert_eq!(parser::try_get_types("nope"), None);
}

#[test]
fn browse_fixture_cases_cover_entire_category_map() {
    assert_eq!(browse_fixture_cases().len(), categories::MAP.len());
}

#[test]
fn extract_post_urls_browse_fixtures_yield_posts() {
    for (cat, file, _types) in browse_fixture_cases() {
        let html = support::read(&format!("Leproduction/{file}"));
        let urls = parser::extract_post_urls(&html, HOST);
        assert!(!urls.is_empty(), "expected >=1 post urls for cat {cat}");
        for u in &urls {
            assert!(u.to_lowercase().starts_with(&HOST.to_lowercase()), "{u}");
            assert!(u.to_lowercase().ends_with(".html"), "{u}");
        }
    }
}

#[test]
fn parse_detail_html_fixture_yields_typed_torrents() {
    let html = support::read("Leproduction/detail_sample.html");
    let post_url = format!("{HOST}/anime/1579-van-pis.html");
    let torrents = parser::parse_detail_html(&html, &post_url, &["anime"]);
    assert!(!torrents.is_empty(), "expected >=1 torrents");
    for t in &torrents {
        assert_eq!(t.trackerName, "leproduction");
        assert_eq!(t.types, vec!["anime".to_string()]);
        assert!(!t.name.trim().is_empty());
        assert!(!t.title.trim().is_empty());
        assert!(t.url.starts_with(&format!("{post_url}?")));
        assert!(t.url.contains("&id="));
        assert!(t.magnet.to_lowercase().starts_with("magnet:"));
        assert!(!t.sizeName.trim().is_empty());
        assert!(t.sid >= 0 && t.pir >= 0);
    }
    let first = &torrents[0];
    assert!(first.name.contains("Ван Пис"));
    assert_eq!(first.originalname, "One Piece");
    assert_eq!(first.relased, 2026);
    assert_eq!(parser::extract_torrent_id(&first.url).as_deref(), Some("8371"));
    assert!(first.sizeName.to_lowercase().contains("71.04"));
}

#[test]
fn detect_last_page_finds_max_page() {
    let html = r#"<a href="/film/page/2/">2</a><a href="/film/page/12/">12</a><a href="/film/page/5/">5</a>"#;
    assert_eq!(parser::detect_last_page(html, None), 12);
    assert_eq!(parser::detect_last_page(html, Some("film")), 12);
    assert_eq!(parser::detect_last_page("", None), 1);
}

#[test]
fn detect_last_page_serial_fixture_is_3() {
    let html = support::read("Leproduction/browse_serial.html");
    assert_eq!(parser::detect_last_page(&html, None), 3);
    assert_eq!(parser::detect_last_page(&html, Some("serial")), 3);
    assert_eq!(parser::detect_last_page(&html, Some("film")), 1);
}

#[test]
fn detect_last_page_ignores_script_page_numbers() {
    let html = r#"<script>var junk="/page/2613/";</script>
<span class="navigation"><a href="https://www.le-production.online/serial/page/23/">23</a></span>
<span class="pnext"><a href="/serial/page/2/">Дальше</a></span>
"#;
    assert_eq!(parser::detect_last_page(html, Some("serial")), 23);
    assert_eq!(parser::detect_last_page(html, None), 23);
}

#[test]
fn extract_post_urls_empty_returns_empty() {
    assert!(parser::extract_post_urls("", HOST).is_empty());
    assert!(parser::extract_post_urls("<html></html>", HOST).is_empty());
}

#[test]
fn parse_detail_html_empty_returns_empty() {
    assert!(parser::parse_detail_html("", &format!("{HOST}/x.html"), &["movie"]).is_empty());
    assert!(parser::parse_detail_html("<html></html>", &format!("{HOST}/x.html"), &["movie"]).is_empty());
}

#[test]
fn extract_magnet_from_href() {
    let html = r#"<a href="magnet:?xt=urn:btih:ABC&amp;dn=x">m</a>"#;
    let magnet = parser::extract_magnet(html).expect("magnet");
    assert!(magnet.to_lowercase().starts_with("magnet:?xt=urn:btih:abc"));
    assert!(magnet.contains("&dn=x"));
}
