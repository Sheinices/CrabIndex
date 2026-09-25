//! Fetch flow against local mock FlareSolverr / cffetch servers.

use axum::routing::post;
use axum::{Json, Router};
use crab_cloudflare::{cffetch, fetch_async};
use crab_core::config::{set_current, AppOptions};
use once_cell::sync::Lazy;
use parking_lot::Mutex as PMutex;
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq)]
enum FsMode {
    Ok,
    OkWithCookies,
    BrowserTimeout,
    Challenge,
}

#[derive(Clone, Copy, PartialEq)]
enum CfMode {
    Page,
    Error,
    Mitigated,
}

struct Mock {
    fs: FsMode,
    cf: CfMode,
    fs_calls: Vec<String>,
    cf_calls: Vec<Value>,
}

static MOCK: Lazy<PMutex<Mock>> = Lazy::new(|| PMutex::new(Mock { fs: FsMode::Ok, cf: CfMode::Error, fs_calls: vec![], cf_calls: vec![] }));
static SERIAL: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

async fn fs_handler(Json(v): Json<Value>) -> Json<Value> {
    let cmd = v["cmd"].as_str().unwrap_or_default().to_string();
    let mode = {
        let mut m = MOCK.lock();
        m.fs_calls.push(format!("{cmd}:{}", v["session"].as_str().unwrap_or_default()));
        m.fs
    };
    if cmd != "request.get" {
        return Json(json!({"status": "ok", "message": ""}));
    }
    Json(match mode {
        FsMode::Ok => json!({"status":"ok","solution":{"status":200,"response":"<html>browser page</html>","cookies":[]}}),
        FsMode::OkWithCookies => json!({"status":"ok","solution":{"status":200,"response":"<html>browser page</html>",
            "cookies":[{"name":"cf_clearance","value":"xyz"},{"name":"","value":"skip"}],"userAgent":"UA-FS"}}),
        FsMode::BrowserTimeout => json!({"status":"error","message":"Error: Error solving the challenge. Timeout after 60.0 seconds."}),
        FsMode::Challenge => json!({"status":"ok","solution":{"status":200,"response":"<title>Just a moment...</title>"}}),
    })
}

async fn cf_handler(Json(v): Json<Value>) -> Json<Value> {
    let mode = {
        let mut m = MOCK.lock();
        m.cf_calls.push(v.clone());
        m.cf
    };
    Json(match mode {
        CfMode::Page => json!({"status":200,"body":"<html>fast page</html>","cfMitigated":false}),
        CfMode::Error => json!({"status":0,"error":"boom"}),
        CfMode::Mitigated => json!({"status":403,"body":"","cfMitigated":true}),
    })
}

async fn start() -> (String, String) {
    let app = Router::new().route("/v1", post(fs_handler)).route("/fetch", post(cf_handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/v1"), format!("http://{addr}/fetch"))
}

fn configure(fs_url: &str, cf_url: &str, cffetch_enabled: bool, crawl_url: &str) {
    let mut c = AppOptions::default();
    c.flaresolverr.enable = true;
    c.flaresolverr.url = fs_url.into();
    c.flaresolverr.crawlUrl = crawl_url.into();
    c.flaresolverr.browserTimeoutRetries = 0;
    c.flaresolverr.recycleAfterTimeouts = 2;
    c.flaresolverr.sessionIdleMinutes = 0;
    c.cffetch.enable = cffetch_enabled;
    c.cffetch.url = cf_url.into();
    set_current(c);
    cffetch::reset();
    let mut m = MOCK.lock();
    m.fs_calls.clear();
    m.cf_calls.clear();
}

fn set_modes(fs: FsMode, cf: CfMode) {
    let mut m = MOCK.lock();
    m.fs = fs;
    m.cf = cf;
}

fn fs_calls() -> Vec<String> {
    MOCK.lock().fs_calls.clone()
}

#[tokio::test]
async fn browser_session_is_created_once_and_reused() {
    let _s = SERIAL.lock().await;
    let (fs, cf) = start().await;
    configure(&fs, &cf, false, "");
    set_modes(FsMode::Ok, CfMode::Error);

    assert_eq!(fetch_async("https://one.test/a", Some("uid=1"), None, &[]).await.as_deref(), Some("<html>browser page</html>"));
    assert_eq!(fetch_async("https://one.test/b", None, None, &[]).await.as_deref(), Some("<html>browser page</html>"));
    assert_eq!(fs_calls(), vec!["sessions.create:crabindex-one_test", "request.get:crabindex-one_test", "request.get:crabindex-one_test"]);
}

#[tokio::test]
async fn fast_path_serves_page_with_forwarded_headers() {
    let _s = SERIAL.lock().await;
    let (fs, cf) = start().await;
    configure(&fs, &cf, true, "");
    set_modes(FsMode::Ok, CfMode::Page);

    let hdrs = vec![("Accept".to_string(), "text/html".to_string()), ("Cookie".to_string(), "no".to_string())];
    let got = fetch_async("https://fast.test/a", Some("uid=1"), Some("https://ref/"), &hdrs).await;
    assert_eq!(got.as_deref(), Some("<html>fast page</html>"));
    assert!(fs_calls().is_empty());
    let call = MOCK.lock().cf_calls[0].clone();
    assert_eq!(call["cookies"], "uid=1");
    assert_eq!(call["headers"], json!({"Referer":"https://ref/","Accept":"text/html"}));
    assert_eq!(call["impersonate"], "chrome136");
}

#[tokio::test]
async fn browser_cookies_are_validated_and_remembered() {
    let _s = SERIAL.lock().await;
    let (fs, cf) = start().await;
    configure(&fs, &cf, true, "");
    // cffetch down: fast path unavailable, validation treats "unreachable" as valid.
    set_modes(FsMode::OkWithCookies, CfMode::Error);

    assert!(fetch_async("https://jar.test/a", None, None, &[]).await.is_some());
    let c = cffetch::for_host("jar.test").expect("remembered");
    assert_eq!(c.cookies.as_deref(), Some("cf_clearance=xyz"));
    assert_eq!(c.user_agent.as_deref(), Some("UA-FS"));
}

#[tokio::test]
async fn mitigated_validation_blocks_fast_path() {
    let _s = SERIAL.lock().await;
    let (fs, cf) = start().await;
    configure(&fs, &cf, true, "");
    set_modes(FsMode::OkWithCookies, CfMode::Mitigated);

    assert!(fetch_async("https://blk.test/a", None, None, &[]).await.is_some());
    assert!(cffetch::for_host("blk.test").is_none());
    assert!(cffetch::fast_path_blocked("blk.test"));
}

#[tokio::test]
async fn browser_timeouts_recycle_session_after_threshold() {
    let _s = SERIAL.lock().await;
    let (fs, cf) = start().await;
    configure(&fs, &cf, false, "");
    set_modes(FsMode::BrowserTimeout, CfMode::Error);

    // 1st timeout (1/2): session kept.
    assert!(fetch_async("https://slow.test/a", None, None, &[]).await.is_none());
    assert_eq!(fs_calls(), vec!["sessions.create:crabindex-slow_test", "request.get:crabindex-slow_test"]);

    // 2nd timeout (2/2): destroy + create + one more request.
    assert!(fetch_async("https://slow.test/a", None, None, &[]).await.is_none());
    assert_eq!(
        fs_calls()[2..],
        ["request.get:crabindex-slow_test", "sessions.destroy:crabindex-slow_test", "sessions.create:crabindex-slow_test", "request.get:crabindex-slow_test"]
    );
}

#[tokio::test]
async fn interstitial_solution_is_page_failure() {
    let _s = SERIAL.lock().await;
    let (fs, cf) = start().await;
    configure(&fs, &cf, false, "");
    set_modes(FsMode::Challenge, CfMode::Error);
    assert!(fetch_async("https://chl.test/a", None, None, &[]).await.is_none());
    assert_eq!(fs_calls().len(), 2);
}

#[tokio::test]
async fn crawl_lane_uses_crawl_url() {
    let _s = SERIAL.lock().await;
    let (fs, cf) = start().await;
    // main URL unreachable; the crawl lane must use crawlUrl.
    configure("http://127.0.0.1:9/v1", &cf, false, &fs);
    set_modes(FsMode::Ok, CfMode::Error);
    let got = crab_core::net::cf::with_crawl_lane(fetch_async("https://lane.test/a", None, None, &[])).await;
    assert_eq!(got.as_deref(), Some("<html>browser page</html>"));
    assert!(fetch_async("https://lane.test/a", None, None, &[]).await.is_none());
}

#[tokio::test]
async fn disabled_flaresolverr_returns_none() {
    let _s = SERIAL.lock().await;
    let mut c = AppOptions::default();
    c.flaresolverr.enable = false;
    set_current(c);
    assert!(fetch_async("https://off.test/a", None, None, &[]).await.is_none());
}

#[tokio::test]
async fn warmup_reports_result_json() {
    let _s = SERIAL.lock().await;
    let (fs, cf) = start().await;
    configure(&fs, &cf, false, "");
    set_modes(FsMode::Ok, CfMode::Error);
    let r = crab_cloudflare::controller::warmup("https://warm.test/x").await;
    assert!(r.ok);
    assert_eq!(r.host.as_deref(), Some("warm.test"));
    assert_eq!(r.length, "<html>browser page</html>".len());
    let v = serde_json::to_value(&r).expect("json");
    assert!(v.get("tookSeconds").is_some());

    let bad = crab_cloudflare::controller::warmup("not a url").await;
    assert!(!bad.ok);
    assert_eq!(serde_json::to_value(&bad).expect("json"), json!({"ok": false, "length": 0, "tookSeconds": 0.0}));
}
