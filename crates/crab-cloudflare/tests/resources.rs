use axum::{extract::State, routing::post, Json, Router};
use crab_cloudflare::clearance::{
    close_if_idle, close_sessions, fetch_async, post_form_async, sessions_snapshot,
};
use crab_core::config::{set_current, AppOptions};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct Mock {
    sessions: Mutex<HashSet<String>>,
    peak_sessions: AtomicUsize,
    active_requests: AtomicUsize,
    peak_requests: AtomicUsize,
    fail_destroy: AtomicBool,
    fail_create: AtomicBool,
    fail_list: AtomicBool,
    fail_get: AtomicBool,
}

async fn handler(State(mock): State<Arc<Mock>>, Json(request): Json<Value>) -> Json<Value> {
    let name = request["session"].as_str().unwrap_or_default();
    match request["cmd"].as_str().unwrap_or_default() {
        "sessions.list" => {
            if mock.fail_list.load(Ordering::SeqCst) {
                return Json(json!({"status": "error", "message": "unavailable"}));
            }
            Json(
                json!({"status": "ok", "sessions": mock.sessions.lock().iter().cloned().collect::<Vec<_>>()}),
            )
        }
        "sessions.create" => {
            let mut sessions = mock.sessions.lock();
            sessions.insert(name.into());
            mock.peak_sessions
                .fetch_max(sessions.len(), Ordering::SeqCst);
            if mock.fail_create.load(Ordering::SeqCst) {
                return Json(json!({"status": "error", "message": "chrome not reachable"}));
            }
            Json(json!({"status": "ok"}))
        }
        "sessions.destroy" => {
            if mock.fail_destroy.load(Ordering::SeqCst) {
                return Json(json!({"status": "error", "message": "browser is stuck"}));
            }
            mock.sessions.lock().remove(name);
            Json(json!({"status": "ok"}))
        }
        "request.get" | "request.post" => {
            let active = mock.active_requests.fetch_add(1, Ordering::SeqCst) + 1;
            mock.peak_requests.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(30)).await;
            mock.active_requests.fetch_sub(1, Ordering::SeqCst);
            if mock.fail_get.load(Ordering::SeqCst) {
                return Json(json!({"status": "error", "message": "Timeout after 60.0 seconds."}));
            }
            Json(
                json!({"status": "ok", "solution": {"status": 200, "response": "<html>page</html>", "cookies": []}}),
            )
        }
        _ => Json(json!({"status": "error"})),
    }
}

async fn start(mock: Arc<Mock>) -> String {
    let app = Router::new().route("/v1", post(handler)).with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}/v1")
}

async fn fetch(host: &str) -> Option<String> {
    fetch_async(&format!("https://{host}/"), None, None, &[]).await
}

#[tokio::test]
async fn browser_resources_are_bounded_and_failed_cleanup_is_retried() {
    let mock = Arc::new(Mock::default());
    let url = start(mock.clone()).await;
    let mut options = AppOptions::default();
    options.flaresolverr.url = url.clone();
    options.flaresolverr.maxSessions = 2;
    options.flaresolverr.sessionIdleMinutes = 0;
    options.flaresolverr.backoff = false;
    options.flaresolverr.browserTimeoutRetries = 0;
    options.cffetch.enable = false;
    set_current(options.clone());

    assert!(fetch("a.test").await.is_some());
    assert!(fetch("b.test").await.is_some());
    assert!(fetch("a.test").await.is_some());
    assert!(fetch("c.test").await.is_some());
    assert_eq!(
        *mock.sessions.lock(),
        HashSet::from(["crabindex-a_test".into(), "crabindex-c_test".into()])
    );
    assert_eq!(mock.peak_sessions.load(Ordering::SeqCst), 2);

    let last_use = sessions_snapshot()
        .into_iter()
        .find(|session| session["host"] == "a.test")
        .unwrap()["lastUse"]
        .clone();
    mock.fail_get.store(true, Ordering::SeqCst);
    assert!(fetch("a.test").await.is_none());
    assert!(fetch("a.test").await.is_none());
    assert_eq!(
        sessions_snapshot()
            .into_iter()
            .find(|session| session["host"] == "a.test")
            .unwrap()["lastUse"],
        last_use
    );
    mock.fail_get.store(false, Ordering::SeqCst);

    let caller = tokio::spawn(fetch("cancel.test"));
    tokio::time::timeout(Duration::from_secs(2), async {
        while mock.active_requests.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    caller.abort();
    assert!(fetch("after-cancel.test").await.is_some());
    assert_eq!(
        mock.peak_requests.load(Ordering::SeqCst),
        1,
        "caller cancellation must not release the browser queue early"
    );

    let (get, login) = tokio::join!(
        fetch("d.test"),
        post_form_async("https://e.test/login", "user=test")
    );
    assert!(get.is_some() && login.is_some());
    assert_eq!(mock.peak_requests.load(Ordering::SeqCst), 1);
    assert_eq!(mock.peak_sessions.load(Ordering::SeqCst), 2);

    mock.fail_destroy.store(true, Ordering::SeqCst);
    assert!(fetch("blocked.test").await.is_none());
    assert_eq!(mock.sessions.lock().len(), 2);
    assert!(!mock.sessions.lock().contains("crabindex-blocked_test"));
    assert_eq!(close_sessions(None).await.0, 0);
    assert!(sessions_snapshot()
        .iter()
        .any(|session| session["cleanupPending"] == true));
    mock.fail_destroy.store(false, Ordering::SeqCst);
    close_if_idle().await;
    assert!(
        mock.sessions.lock().is_empty(),
        "failed closes retry even with idle cleanup disabled"
    );

    mock.fail_create.store(true, Ordering::SeqCst);
    assert!(fetch("broken.test").await.is_none());
    assert_eq!(mock.sessions.lock().len(), 1);
    mock.fail_create.store(false, Ordering::SeqCst);
    close_if_idle().await;
    assert!(
        mock.sessions.lock().is_empty(),
        "a failed create can still leave a browser behind"
    );

    mock.fail_list.store(true, Ordering::SeqCst);
    assert!(fetch("unlisted.test").await.is_none());
    assert!(mock.sessions.lock().is_empty());
    mock.fail_list.store(false, Ordering::SeqCst);

    mock.sessions
        .lock()
        .insert("crabindex-pending-orphan".into());
    mock.fail_destroy.store(true, Ordering::SeqCst);
    assert_eq!(close_sessions(None).await.0, 0);
    mock.fail_destroy.store(false, Ordering::SeqCst);
    close_if_idle().await;
    assert!(
        mock.sessions.lock().is_empty(),
        "orphan cleanup must also retry failed destroys"
    );

    mock.sessions.lock().extend([
        "crabindex-orphan1".into(),
        "crabindex-orphan2".into(),
        "crabindex-orphan3".into(),
        "another-app".into(),
    ]);
    assert!(fetch("fresh.test").await.is_some());
    assert_eq!(
        mock.sessions
            .lock()
            .iter()
            .filter(|name| name.starts_with("crabindex"))
            .count(),
        2
    );
    assert!(mock.sessions.lock().contains("another-app"));
    close_sessions(None).await;

    let crawl = Arc::new(Mock::default());
    options.flaresolverr.crawlUrl = start(crawl.clone()).await;
    options.flaresolverr.maxSessions = 1;
    set_current(options.clone());
    let (main_result, crawl_result) = tokio::join!(
        fetch("main.test"),
        crab_core::net::cf::with_crawl_lane(fetch("crawl.test"))
    );
    assert!(main_result.is_some() && crawl_result.is_some());
    assert!(mock.sessions.lock().contains("crabindex-main_test"));
    assert_eq!(
        *crawl.sessions.lock(),
        HashSet::from(["crabindex-crawl_test".into()])
    );
    close_sessions(None).await;

    options.flaresolverr.backoff = true;
    options.flaresolverr.crawlUrl.clear();
    set_current(options);
    mock.fail_create.store(true, Ordering::SeqCst);
    for _attempt in 0..8 {
        assert!(fetch("create-backoff.test").await.is_none());
    }
    assert!(crab_cloudflare::backoff::active()
        .iter()
        .any(|entry| entry.host == "create-backoff.test"));
    mock.fail_create.store(false, Ordering::SeqCst);
    close_if_idle().await;
}
