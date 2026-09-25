mod common;

use crab_core::models::TorrentDetails;
use crab_trackers_c::lostfilm::parser::*;

const HOST: &str = "https://www.lostfilm.tv";

// ---------------------------------------------------------------------------
// /new/ fixtures
// ---------------------------------------------------------------------------

#[test]
fn new_page_fixture_collects_episodes_with_ru_names_from_hor_breaker() {
    let html = common::read("lostfilm/new_page1.html");
    assert!(html.contains("LostFilm.TV"));

    let map = build_hor_breaker_name_map(&html);
    assert!(!map.is_empty());

    let mut list = Vec::new();
    collect_from_episode_links(&html, HOST, &mut list, 1, Some(&map));
    dedupe_list_by_url(&mut list);

    assert!(list.len() >= 5, "expected episodes, got {}", list.len());
    for t in &list {
        assert_eq!(t.trackerName, "lostfilm");
        assert_eq!(t.types, vec!["serial"]);
        assert!(!t.url.trim().is_empty());
        assert!(t.url.contains("/season_"));
        assert!(t.url.contains("/episode_"));
        assert!(!t.name.trim().is_empty());
        assert!(!t.originalname.trim().is_empty());
        assert!(!t.title.trim().is_empty());
        assert!(!crab_core::time::is_min(&t.createTime));
    }

    let with_ru = list.iter().filter(|t| has_ru_name_t(t)).count();
    assert!(with_ru >= 1, "expected at least one RU/EN name pair from hor-breaker map");
}

#[test]
fn extract_total_pages_from_new_page_html_caps_at_100() {
    let html = common::read("lostfilm/new_page1.html");
    assert_eq!(extract_total_pages_from_new_page_html(&html), 100);
    assert_eq!(extract_total_pages_from_new_page_html(""), 1);
    assert_eq!(extract_total_pages_from_new_page_html("<html>LostFilm.TV <a href=\"/new/page_3\">3</a></html>"), 3);
}

#[test]
fn synthetic_page_maps_ru_name_onto_episode_link() {
    let html = common::read("lostfilm/new_page_synthetic.html");
    let map = build_hor_breaker_name_map(&html);
    assert!(map.contains_key("series/Test_Show") || map.keys().any(|k| k.to_lowercase().contains("test_show")));

    let mut list = Vec::new();
    collect_from_episode_links(&html, HOST, &mut list, 1, Some(&map));
    assert_eq!(list.len(), 1);
    let t = &list[0];
    assert_eq!(t.name, "Тестовый сериал");
    assert_eq!(t.originalname, "Test Show");
    assert!(has_ru_name_t(t));
    assert!(!t.url.contains('#'));
}

#[test]
fn verify_parse_new_page_dates_uses_fixture() {
    let html = common::read("lostfilm/new_page1.html");
    let items = parse_new_page_dates(&html, HOST);
    assert!(items.len() >= 5);
    assert!(items.iter().all(|i| !i.dateStr.trim().is_empty()));
}

// ---------------------------------------------------------------------------
// PlayEpisode ids
// ---------------------------------------------------------------------------

#[test]
fn prefer_long_combined_id() {
    let html = common::read("lostfilm/episode_play_long_id.html");
    assert_eq!(try_extract_play_episode_id(&html).as_deref(), Some("780002005"));
}

#[test]
fn builds_id_from_three_arg_form() {
    let html = common::read("lostfilm/episode_play_three_arg.html");
    assert_eq!(try_extract_play_episode_id(&html).as_deref(), Some("780002005"));
}

#[test]
fn movie_or_episode_extracts_expected() {
    for (snippet, expected) in [
        ("PlayEpisode('12','3','4')", "12003004"),
        ("PlayEpisode(\"99\",\"1\",\"10\")", "99001010"),
        ("PlayMovie('1234567')", "1234567"),
        ("PlayEpisode('55','0','1')", "55000001"),
    ] {
        assert_eq!(try_extract_play_movie_or_episode_id(&format!("<script>{snippet}</script>")).as_deref(), Some(expected), "{snippet}");
    }
}

#[test]
fn empty_html_returns_none() {
    assert_eq!(try_extract_play_episode_id(""), None);
    assert_eq!(try_extract_play_episode_id("<html></html>"), None);
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

#[test]
fn is_preferred_quality_1080_and_2160() {
    for (q, expected) in [
        ("1080", true),
        ("1080p", true),
        ("2160", true),
        ("2160p", true),
        ("720p", false),
        ("SD", false),
        ("1440p", false),
    ] {
        assert_eq!(is_preferred_quality(q), expected, "{q}");
    }
}

#[test]
fn normalize_quality_values() {
    for (input, expected) in [("1080", "1080p"), ("2160", "2160p"), ("720", "720p"), ("sd", "SD"), ("mp4", "720p"), ("2160p", "2160p")] {
        assert_eq!(normalize_quality(input), expected, "{input}");
    }
}

#[test]
fn shorten_series_name_strips_serial_boilerplate() {
    let og = "Капли Бога (Drops of God). Сериал Капли Бога канал: гид";
    assert_eq!(shorten_series_name(og), "Капли Бога");
}

#[test]
fn has_ru_name_detects_xx_bucket() {
    assert!(!has_ru_name("Ponies", "Ponies"));
    assert!(is_xx_name_bucket("Ponies", "Ponies"));
    assert!(has_ru_name("Пони", "Ponies"));
}

#[test]
fn apply_magnet_cache_does_not_overwrite_title_prefers_ru_names() {
    let mut incoming = TorrentDetails {
        url: "https://www.lostfilm.tv/series/X/season_1/episode_1/#1080p".into(),
        title: "Пони / Ponies / 1 сезон 1 серия [2026, 1080p]".into(),
        name: "Ponies".into(),
        originalname: "Ponies".into(),
        ..Default::default()
    };
    let cached = TorrentDetails {
        magnet: "magnet:?xt=urn:btih:ABC".into(),
        title: "OLD TITLE".into(),
        sizeName: "1 GB".into(),
        name: "Пони".into(),
        originalname: "Ponies".into(),
        ..Default::default()
    };
    apply_magnet_cache(&mut incoming, &cached);
    assert_eq!(incoming.magnet, "magnet:?xt=urn:btih:ABC");
    assert_eq!(incoming.sizeName, "1 GB");
    assert_eq!(incoming.title, "Пони / Ponies / 1 сезон 1 серия [2026, 1080p]");
    assert_eq!(incoming.name, "Пони");
    assert_eq!(incoming.originalname, "Ponies");
}

#[test]
fn clone_with_quality_uses_hash_suffix() {
    let src = TorrentDetails {
        trackerName: "lostfilm".into(),
        types: vec!["serial".into()],
        url: "https://www.lostfilm.tv/series/X/season_1/episode_1/".into(),
        title: "Имя / Name / 1 сезон 1 серия [2026]".into(),
        name: "Имя".into(),
        originalname: "Name".into(),
        relased: 2026,
        ..Default::default()
    };
    let clone = clone_with_quality(&src, "magnet:?xt=urn:btih:1", "1080", "2 GB");
    assert_eq!(clone.url, "https://www.lostfilm.tv/series/X/season_1/episode_1/#1080p");
    assert!(clone.title.contains("1080p"));
    assert_eq!(clone.magnet, "magnet:?xt=urn:btih:1");
    assert!(has_ru_name_t(&clone));
}

#[test]
fn stable_url_id_includes_quality_host_independent() {
    let a = stable_url_id("https://www.lostfilm.tv/series/X/season_1/episode_1/#1080p");
    let b = stable_url_id("https://mirror.example/series/X/season_1/episode_1/#1080p");
    let c = stable_url_id("https://www.lostfilm.tv/series/X/season_1/episode_1/#720p");
    assert!(a > 0);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn stable_url_id_registered_for_lostfilm() {
    crab_trackers_c::init();
    let url = "https://www.lostfilm.tv/series/X/season_1/episode_1/#1080p";
    assert_eq!(crab_core::fdb::torrent_id_from_url("lostfilm", url), stable_url_id(url));
}

#[test]
fn parse_v_page_quality_link_urls_only_1080_and_2160() {
    let html = common::read("lostfilm/v_page_qualities.html");
    let links = parse_v_page_quality_link_urls(&html);
    assert_eq!(links.len(), 2);
    assert!(links.iter().any(|x| x.1 == "1080p"));
    assert!(links.iter().any(|x| x.1 == "2160p"));
    assert!(links.iter().all(|x| is_preferred_quality(&x.1)));
}

#[test]
fn detects_missing_cookie() {
    for (cookie, expected) in [
        (Some("lf_loyal_person=0; lf_session=S; lf_udv=U; PHPSESSID=P"), true),
        (Some("PHPSESSID=abc"), true),
        (Some(""), false),
        (Some("   "), false),
        (None, false),
    ] {
        assert_eq!(has_auth_cookie(cookie), expected, "{cookie:?}");
    }
}
