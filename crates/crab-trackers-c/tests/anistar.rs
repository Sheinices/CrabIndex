mod common;

use crab_trackers_c::anistar::parser::*;

const CANON_HOST: &str = "https://anistar.org";

#[test]
fn extract_post_urls_listing_fixture_yields_absolute_post_links() {
    let html = common::read("anistar/listing_anime.html");
    let urls = extract_post_urls(&html, CANON_HOST);
    assert!(!urls.is_empty(), "expected >=1 post urls, got {}", urls.len());
    for u in &urls {
        assert!(u.to_lowercase().starts_with(CANON_HOST));
        assert!(u.to_lowercase().contains(".html"));
        assert!(crab_core::rx::is_match(u, r"/\d{2,}-"));
    }
}

#[test]
fn extract_post_urls_absolute_mirror_links_normalize_to_canon_host() {
    let html = r#"
        <a href="https://v30.astar.bz/11012-gundam.html">Gundam</a>
        <a href="/11011-nano.html">Nano</a>
    "#;
    let urls = extract_post_urls(html, CANON_HOST);
    assert_eq!(urls.len(), 2);
    assert!(urls.contains(&"https://anistar.org/11012-gundam.html".to_string()));
    assert!(urls.contains(&"https://anistar.org/11011-nano.html".to_string()));
}

#[test]
fn detect_last_page_listing_fixture_is_682() {
    let html = common::read("anistar/listing_anime.html");
    assert_eq!(detect_last_page(&html, None), 682);
    assert_eq!(detect_last_page(&html, Some("anime")), 682);
    assert_eq!(detect_last_page(&html, Some("dorama")), 1);
}

#[test]
fn detect_last_page_ignores_script_page_numbers() {
    let html = r#"
        <script>var junk="/page/2613/";</script>
        <div class="pages"><a href="https://v30.astar.bz/anime/page/680/">680</a></div>
    "#;
    assert_eq!(detect_last_page(html, Some("anime")), 680);
    assert_eq!(detect_last_page(html, None), 680);
    assert_eq!(detect_last_page("", None), 1);
}

#[test]
fn parse_detail_torrents_detail_fixture_yields_typed_torrents() {
    let html = common::read("anistar/detail_sample.html");
    let post_url = "https://anistar.org/12-test-show.html";
    let torrents = parse_detail_torrents(&html, post_url, &["anime"]);
    assert!(!torrents.is_empty(), "expected >=1 torrents, got {}", torrents.len());

    for t in &torrents {
        assert_eq!(t.t.trackerName, "anistar");
        assert_eq!(t.t.types, vec!["anime"]);
        assert!(!t.t.name.trim().is_empty());
        assert!(!t.t.title.trim().is_empty());
        assert!(t.t.url.starts_with(&format!("{post_url}?")));
        assert!(t.t.url.contains("&id="));
        assert!(!t.download_id.is_empty());
        assert!(t.download_id.chars().all(|c| c.is_ascii_digit()));
        assert!(!crab_core::time::is_min(&t.t.createTime));
        assert!(t.t.relased >= 1900);
    }

    if torrents.len() == 2 && torrents[0].download_id == "1001" {
        assert_eq!(torrents[0].t.name, "Тестовое аниме");
        assert_eq!(torrents[0].t.originalname, "Test Anime");
        assert!(torrents[0].t.title.contains("Серия 3"));
        assert_eq!(torrents[0].t.relased, 2024);
        assert_eq!(torrents[0].t.sid, 10);
        assert!(torrents[1].t.title.contains("Серии 1-12"));
        assert_eq!(torrents[1].t.relased, 2023);
    }
}

#[test]
fn parse_title_names_splits_russian_and_original() {
    let (name, original) = parse_title_names("Тестовое аниме / Test Anime").expect("names");
    assert_eq!(name, "Тестовое аниме");
    assert_eq!(original, "Test Anime");
}

#[test]
fn parse_episode_label_film_size_is_not_episode_number() {
    assert_eq!(parse_episode_label("Фильм (3.05 Gb)"), ("Фильм".to_string(), "film".to_string()));
    assert_eq!(parse_episode_label("Серия 7 (466.54 Mb)"), ("Серия 7".to_string(), "7".to_string()));
    assert_eq!(parse_episode_label("Серии 1-12"), ("Серии 1-12".to_string(), "1".to_string()));
}

#[test]
fn parse_detail_torrents_film_info_d1_does_not_use_size_as_episode() {
    let html = r#"
        <html><body>
        <h1>Гандам / Gundam</h1>
        <div id="torrent_46365_info" class="torrent">
          <div class="info_d1">Фильм (3.05 Gb)</div>
          <div>18-08-2026</div>
          <div class="li_distribute">0</div>
          <div class="li_swing">23</div>
        </div>
        </body></html>
    "#;
    let post_url = "https://anistar.org/11012-gundam.html";
    let torrents = parse_detail_torrents(html, post_url, &["anime"]);
    assert_eq!(torrents.len(), 1);
    assert_eq!(torrents[0].download_id, "46365");
    assert!(torrents[0].t.title.contains("Фильм"));
    assert!(!torrents[0].t.title.contains("Серия 3"));
    assert!(torrents[0].t.url.ends_with("?e=film&id=46365"));
    assert_eq!(torrents[0].t.pir, 23);
    assert_eq!(torrents[0].t.relased, 2026);
}

#[test]
fn parse_detail_torrents_empty_html_returns_empty() {
    assert!(parse_detail_torrents("", "https://anistar.org/12-x.html", &["anime"]).is_empty());
    assert!(parse_detail_torrents("<html></html>", "https://anistar.org/12-x.html", &["anime"]).is_empty());
}
