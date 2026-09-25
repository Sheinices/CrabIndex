//! cffetch jar / mitigation / block-list behaviour.

use crab_cloudflare::cffetch;
use crab_core::config::{set_current, AppOptions};
use parking_lot::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

fn setup() -> MutexGuard<'static, ()> {
    let g = LOCK.lock();
    cffetch::reset();
    set_conf(true);
    g
}

fn set_conf(enable: bool) {
    let mut c = AppOptions::default();
    c.cffetch.enable = enable;
    c.cffetch.url = "http://127.0.0.1:8192/fetch".into();
    set_current(c);
}

#[test]
fn clearance_lost_when_cf_mitigated() {
    let _g = setup();
    assert!(cffetch::clearance_lost(403, Some(""), true));
    assert!(cffetch::clearance_lost(503, Some(""), true));
}

#[test]
fn naked_tracker_403_is_not_clearance_loss() {
    let _g = setup();
    assert!(!cffetch::clearance_lost(403, Some("<html>Тема находится в закрытом разделе</html>"), false));
    assert!(!cffetch::clearance_lost(503, Some("<html>Форум временно недоступен</html>"), false));
}

#[test]
fn challenge_html_under_200_is_clearance_loss() {
    let _g = setup();
    assert!(cffetch::clearance_lost(200, Some("<html><head><title>Just a moment...</title>"), false));
    assert!(cffetch::clearance_lost(200, Some("<script>window._cf_chl_opt={};</script>"), false));
}

#[test]
fn ordinary_page_failure_does_not_drop_cookie() {
    let _g = setup();
    assert!(!cffetch::clearance_lost(404, Some("<html>Тема не найдена</html>"), false));
    assert!(!cffetch::clearance_lost(200, Some("<html><table class=\"tCenter\">раздачи</table></html>"), false));
    assert!(!cffetch::clearance_lost(0, None, false));
}

#[test]
fn remember_stores_per_host() {
    let _g = setup();
    let host = "fast-remember.test";
    cffetch::forget(host);
    assert!(cffetch::for_host(host).is_none());

    cffetch::remember(host, Some("cf_clearance=abc; bb_session=xyz"), Some("Mozilla/5.0 Chrome/148"));

    let got = cffetch::for_host(host).expect("clearance");
    assert_eq!(got.cookies.as_deref(), Some("cf_clearance=abc; bb_session=xyz"));
    assert_eq!(got.user_agent.as_deref(), Some("Mozilla/5.0 Chrome/148"));
}

#[test]
fn host_lookup_is_case_insensitive() {
    let _g = setup();
    cffetch::remember("Fast-Case.test", Some("cf_clearance=1"), Some("UA"));
    assert!(cffetch::for_host("fast-case.TEST").is_some());
}

#[test]
fn for_uncleared_returns_clearance_without_cookie() {
    let _g = setup();
    let host = "uncleared.test";
    cffetch::forget(host);
    assert!(cffetch::for_host(host).is_none());
    let got = cffetch::for_uncleared(host).expect("blind path");
    assert!(got.cookies.is_none());
}

#[test]
fn for_uncleared_none_when_cffetch_disabled() {
    let _g = setup();
    set_conf(false);
    assert!(cffetch::for_uncleared("uncleared-disabled.test").is_none());
    assert!(!cffetch::enabled());
    set_conf(true);
}

#[test]
fn for_uncleared_none_when_fast_path_blocked() {
    let _g = setup();
    let host = "uncleared-blocked.test";
    cffetch::block_fast_path(host);
    assert!(cffetch::fast_path_blocked(host));
    assert!(cffetch::for_uncleared(host).is_none());
    cffetch::reset();
}

#[test]
fn blocked_host_hides_remembered_clearance() {
    let _g = setup();
    let host = "fast-blocked.test";
    cffetch::remember(host, Some("cf_clearance=abc"), Some("UA"));
    cffetch::block_fast_path(host);
    assert!(cffetch::for_host(host).is_none());
}

#[test]
fn forget_removes_host() {
    let _g = setup();
    let host = "fast-forget.test";
    cffetch::remember(host, Some("cf_clearance=abc"), Some("Mozilla/5.0"));
    assert!(cffetch::for_host(host).is_some());
    cffetch::forget(host);
    assert!(cffetch::for_host(host).is_none());
}

#[test]
fn empty_cookie_is_not_remembered() {
    let _g = setup();
    let host = "fast-empty.test";
    cffetch::forget(host);
    cffetch::remember(host, Some(""), Some("Mozilla/5.0"));
    cffetch::remember(host, None, Some("Mozilla/5.0"));
    assert!(cffetch::for_host(host).is_none());
}

#[test]
fn remember_merges_jar_does_not_replace() {
    let _g = setup();
    let host = "fast-merge.test";
    cffetch::forget(host);
    cffetch::remember(host, Some("cf_clearance=abc; bb_session=old"), Some("Mozilla/5.0 Chrome/148"));
    cffetch::remember(host, Some("bb_session=new"), None);

    let got = cffetch::for_host(host).expect("clearance");
    let cookies = got.cookies.unwrap_or_default();
    assert!(cookies.contains("cf_clearance=abc"));
    assert!(cookies.contains("bb_session=new"));
    assert!(!cookies.contains("bb_session=old"));
    assert_eq!(got.user_agent.as_deref(), Some("Mozilla/5.0 Chrome/148"));
}

#[test]
fn merge_cookie_jars_keeps_order_and_first_name_casing() {
    assert_eq!(cffetch::merge_cookie_jars(Some("A=1; b=2"), Some("a=3; c=4")), "A=3; b=2; c=4");
    assert_eq!(cffetch::merge_cookie_jars(None, Some("x=1")), "x=1");
}

#[test]
fn should_drop_clearance_after_three_in_window() {
    let _g = setup();
    let host = "fast-mitigation.test";
    assert!(!cffetch::should_drop_clearance(host));
    assert!(!cffetch::should_drop_clearance(host));
    assert!(cffetch::should_drop_clearance(host));
    assert!(!cffetch::should_drop_clearance(host));
}

#[test]
fn hosts_do_not_share_jar() {
    let _g = setup();
    cffetch::remember("fast-a.test", Some("cf_clearance=a"), Some("UA-A"));
    cffetch::remember("fast-b.test", Some("cf_clearance=b"), Some("UA-B"));

    assert_eq!(cffetch::for_host("fast-a.test").and_then(|c| c.cookies).as_deref(), Some("cf_clearance=a"));
    assert_eq!(cffetch::for_host("fast-b.test").and_then(|c| c.cookies).as_deref(), Some("cf_clearance=b"));

    cffetch::forget("fast-a.test");
    assert!(cffetch::for_host("fast-a.test").is_none());
    assert!(cffetch::for_host("fast-b.test").is_some());
}
