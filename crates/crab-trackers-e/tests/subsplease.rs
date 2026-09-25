mod common;

use common::fixture;
use crab_trackers_e::subsplease::parser;

const HOST: &str = "https://subsplease.org";

#[test]
fn constants_match_contract() {
    assert_eq!(parser::TRACKER_NAME, "subsplease");
    assert_eq!(parser::PREFERRED_RES, "1080");
}

#[test]
fn latest_json_fixture_keeps_only_1080() {
    let json = fixture("subsplease/latest.json");
    let items = parser::parse_latest_or_search_json(&json, HOST);
    assert!(!items.is_empty());
    for d in &items {
        let t = &d.t;
        assert_eq!(t.trackerName, "subsplease");
        assert_eq!(t.types, vec!["anime"]);
        assert_eq!(t.quality, 1080);
        assert!(t.url.to_lowercase().starts_with(&format!("{HOST}/shows/")));
        assert!(t.url.contains("res=1080"));
        assert!(t.url.contains("ep="));
        assert!(t.magnet.to_lowercase().starts_with("magnet:?xt=urn:btih:"));
        assert!(!t.name.trim().is_empty());
        assert!(!t.sizeName.trim().is_empty());
        assert!(d.info_hash.as_deref().map(|h| !h.trim().is_empty()).unwrap_or(false));
        assert!(t.title.contains("(1080p)"));
        assert!(!t.title.contains("(720p)"));
        assert!(!t.title.contains("(480p)"));
    }
}

#[test]
fn show_json_sid11_includes_batches() {
    let json = fixture("subsplease/show_sid11.json");
    let items = parser::parse_show_json(&json, HOST, "100-man-no-inochi-no-ue-ni-ore-wa-tatte-iru", "11");
    assert!(items.len() >= 2);
    assert!(items.iter().any(|t| t.is_batch && t.episode == "13-24"));
    assert!(items.iter().any(|t| t.is_batch && t.episode == "01-12"));
    assert!(items.iter().any(|t| !t.is_batch && t.episode == "24"));

    let batch = items.iter().find(|t| t.episode == "13-24").expect("batch");
    assert!(batch.t.title.contains("[Batch]"));
    assert_eq!(batch.show_sid.as_deref(), Some("11"));
    assert_eq!(batch.page, "100-man-no-inochi-no-ue-ni-ore-wa-tatte-iru");
    assert!(!batch.t._sn.trim().is_empty());
    assert!(parser::try_parse_xl(&batch.t.magnet).unwrap_or(0) > 10_000_000_000);
    assert_eq!(batch.t.url, format!("{HOST}/shows/100-man-no-inochi-no-ue-ni-ore-wa-tatte-iru/?ep=13-24&res=1080"));
}

#[test]
fn schedule_show_index_and_sid_html() {
    let schedule = parser::parse_schedule_page_slugs(&fixture("subsplease/schedule.json"));
    assert!(!schedule.is_empty());

    let slugs = parser::parse_show_slugs_from_index_html(&fixture("subsplease/shows_index_snippet.html"));
    assert!(slugs.len() >= 10);
    assert!(slugs.iter().any(|s| s == "100-man-no-inochi-no-ue-ni-ore-wa-tatte-iru"));

    let sid = parser::extract_show_sid_from_html(&fixture("subsplease/show_page_sid11.html"));
    assert_eq!(sid.as_deref(), Some("11"));
}

#[test]
fn sid_attribute_before_id_is_found() {
    let html = r#"<div sid="77" data-x="1" id="show-release-table"></div>"#;
    assert_eq!(parser::extract_show_sid_from_html(html).as_deref(), Some("77"));
    assert_eq!(parser::extract_show_sid_from_html("<p>none</p>"), None);
}

#[test]
fn is_batch_episode_detects_ranges() {
    for (ep, expect) in [("13-24", true), ("01-12", true), ("06", false), ("Movie", false), ("1 ~ 12", true)] {
        assert_eq!(parser::is_batch_episode(ep), expect, "ep={ep}");
    }
}

#[test]
fn stable_url_id_is_deterministic_positive() {
    let a = parser::stable_url_id("13-24");
    let b = parser::stable_url_id("13-24");
    let c = parser::stable_url_id("01-12");
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert!(a > 0);
}

#[test]
fn url_id_extractor_uses_unescaped_episode() {
    let url = parser::build_url(HOST, "some-show", "Movie ~ Part 1");
    assert!(url.contains("ep=Movie%20~%20Part%201&res=1080"));
    assert_eq!(parser::torrent_id_from_url(&url), parser::stable_url_id("Movie ~ Part 1"));
    assert_eq!(parser::torrent_id_from_url("https://subsplease.org/shows/x/"), 0);

    crab_trackers_e::init();
    assert_eq!(crab_core::fdb::torrent_id_from_url("subsplease", &url), parser::stable_url_id("Movie ~ Part 1"));
}

#[test]
fn is_limit_reached_detects_marker() {
    assert!(parser::is_limit_reached("{\"limit_reached\":true}"));
    assert!(!parser::is_limit_reached("{\"a\":1}"));
}

#[test]
fn latest_empty_returns_empty() {
    assert!(parser::parse_latest_or_search_json("", HOST).is_empty());
    assert!(parser::parse_latest_or_search_json("[]", HOST).is_empty());
    assert!(parser::parse_latest_or_search_json("not json", HOST).is_empty());
}

#[test]
fn format_size_shapes() {
    assert_eq!(parser::format_size(0), None);
    assert_eq!(parser::format_size(1_048_576).as_deref(), Some("1.00 Mb"));
    assert_eq!(parser::format_size(2_147_483_648).as_deref(), Some("2.00 GB"));
}

#[test]
fn crate_router_builds_without_route_conflicts() {
    let _ = crab_trackers_e::router();
}
