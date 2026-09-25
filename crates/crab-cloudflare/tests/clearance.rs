//! Session naming, forwarded browser headers, origin-503 fallthrough, challenge detection.

use crab_cloudflare::clearance::header_ci;
use crab_cloudflare::{extra_browser_headers, session_name_for, should_skip_fast_path_for_origin_503};
use crab_core::net::cf;
use reqwest::header::{HeaderMap, HeaderValue};

#[test]
fn session_name_sanitizes_host_per_tracker() {
    assert_eq!(session_name_for(Some("kinozal.guru")), "crabindex-kinozal_guru");
    assert_eq!(session_name_for(Some("rutracker.org")), "crabindex-rutracker_org");
    assert_eq!(session_name_for(Some("anibelka.com")), "crabindex-anibelka_com");
    assert_ne!(session_name_for(Some("kinozal.guru")), session_name_for(Some("rutracker.org")));
}

#[test]
fn session_name_empty_is_bare_prefix() {
    assert_eq!(session_name_for(None), "crabindex");
    assert_eq!(session_name_for(Some("")), "crabindex");
    assert_eq!(session_name_for(Some("   ")), "crabindex");
}

#[test]
fn session_name_is_case_insensitive_and_charset_safe() {
    assert_eq!(session_name_for(Some("Kinozal.GURU")), session_name_for(Some("kinozal.guru")));
    let name = session_name_for(Some("v30.astar.bz"));
    assert!(name.starts_with("crabindex-"));
    assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    assert!(session_name_for(Some("хост.рф")).chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

#[test]
fn extra_browser_headers_forward_referer_skip_cookie_and_sec_fetch() {
    let extra = pairs(&[
        ("Accept", "text/html"),
        ("Accept-Language", "ru"),
        ("Cookie", "secret=1"),
        ("User-Agent", "ignored"),
        ("Sec-Fetch-Site", "cross-site"),
        ("Referer", "https://www.yandex.ru/"),
    ]);
    let h = extra_browser_headers(Some("https://www.google.com/"), &extra);

    assert_eq!(header_ci(&h, "Referer").map(String::as_str), Some("https://www.yandex.ru/"));
    assert_eq!(header_ci(&h, "Accept").map(String::as_str), Some("text/html"));
    assert_eq!(header_ci(&h, "Accept-Language").map(String::as_str), Some("ru"));
    assert!(header_ci(&h, "Cookie").is_none());
    assert!(header_ci(&h, "User-Agent").is_none());
    assert!(header_ci(&h, "Sec-Fetch-Site").is_none());
    assert_eq!(h.len(), 3);
}

#[test]
fn extra_browser_headers_referer_only_when_no_extra() {
    let h = extra_browser_headers(Some("https://www.google.com/"), &[]);
    assert_eq!(h.len(), 1);
    assert_eq!(h.get("Referer").map(String::as_str), Some("https://www.google.com/"));
    assert!(extra_browser_headers(None, &[]).is_empty());
    // lower-case override keeps a single entry
    let h = extra_browser_headers(Some(" https://a/ "), &pairs(&[("referer", "https://b/")]));
    assert_eq!(h.len(), 1);
    assert_eq!(h.get("Referer").map(String::as_str), Some("https://b/"));
}

#[test]
fn origin_503_with_search_referer_falls_through_to_browser() {
    let g = Some("https://www.google.com/");
    assert!(should_skip_fast_path_for_origin_503(503, Some("<html>503 Service Temporarily Unavailable</html>"), g));
    assert!(should_skip_fast_path_for_origin_503(503, Some(""), g));
    assert!(should_skip_fast_path_for_origin_503(503, None, g));
    assert!(!should_skip_fast_path_for_origin_503(503, Some("<html>503</html>"), None));
    assert!(!should_skip_fast_path_for_origin_503(404, Some("503 Service Temporarily Unavailable"), g));
    assert!(!should_skip_fast_path_for_origin_503(403, Some("forbidden"), g));
}

fn headers(v: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, val) in v {
        h.insert(*k, HeaderValue::from_static(val));
    }
    h
}

#[test]
fn cf_ray_alone_is_not_a_challenge() {
    assert!(!cf::is_challenge(503, &headers(&[("cf-ray", "a23b3a1cf9f8dbcb-FRA")])));
    assert!(!cf::is_challenge(403, &headers(&[("cf-ray", "a23b3a1cf9f8dbcb-FRA")])));
}

#[test]
fn cf_mitigated_is_a_challenge() {
    assert!(cf::is_challenge(403, &headers(&[("cf-mitigated", "challenge")])));
    assert!(cf::is_challenge(503, &headers(&[("cf-mitigated", "challenge")])));
}

#[test]
fn successful_response_is_not_a_challenge() {
    assert!(!cf::is_challenge(200, &headers(&[("cf-mitigated", "challenge")])));
}

#[test]
fn challenge_markup_in_body_is_detected() {
    for body in [
        "<html><head><title>Just a moment...</title></head></html>",
        "<html><head><title>Один момент...</title></head></html>",
        "<div class=\"cf-browser-verification\"></div>",
        "window._cf_chl_opt = {}",
        "/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1",
    ] {
        assert!(cf::is_challenge_body(body), "{body}");
    }
}

#[test]
fn ordinary_failure_page_is_not_a_challenge() {
    for body in [
        "<html><body>Форум временно недоступен</body></html>",
        "504 Gateway Time-out",
        "",
        "a.src='/cdn-cgi/challenge-platform/scripts/jsd/main.js';",
    ] {
        assert!(!cf::is_challenge_body(body), "{body}");
    }
}

#[test]
fn real_listing_with_jsd_is_not_a_challenge() {
    let body = "<title>Фильмы до 1990 года</title>a.src='/cdn-cgi/challenge-platform/scripts/jsd/main.js';class=\"torTopic\" id=\"tt-123\"";
    assert!(!cf::is_challenge_body(body));
}

#[test]
fn large_body_is_not_inspected() {
    let body = "a".repeat(300_000) + "Just a moment";
    assert!(!cf::is_challenge_body(&body));
}
