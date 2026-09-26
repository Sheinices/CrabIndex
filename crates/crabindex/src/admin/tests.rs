//! Full-pipeline tests (`build_app` + `oneshot`) for the admin gate, session and API mapping.

use super::*;
use crate::app::build_app;
use axum::extract::{ConnectInfo, RawQuery};
use axum::http::Request as HttpRequest;
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;

use crate::test_support::{setup, DEVKEY, TOKEN};

fn routes() -> Router {
    Router::new()
        .route("/cron/rutor/parsealltask", get(|RawQuery(q): RawQuery| async move { format!("cron:{}", q.unwrap_or_default()) }))
        .merge(crate::config_api::router())
        .merge(crate::controllers::health::router())
        .fallback(|| async { StatusCode::NOT_FOUND })
}

struct Req<'a> {
    method: Method,
    uri: &'a str,
    peer: &'a str,
    headers: Vec<(&'a str, String)>,
    body: String,
}

impl<'a> Req<'a> {
    fn get(uri: &'a str) -> Self {
        Req { method: Method::GET, uri, peer: "203.0.113.5", headers: vec![], body: String::new() }
    }
    fn post(uri: &'a str, body: &str) -> Self {
        Req { method: Method::POST, uri, peer: "203.0.113.5", headers: vec![], body: body.to_string() }
    }
    fn peer(mut self, p: &'a str) -> Self {
        self.peer = p;
        self
    }
    fn header(mut self, k: &'a str, v: impl Into<String>) -> Self {
        self.headers.push((k, v.into()));
        self
    }
    fn gate(self) -> Self {
        let v = format!("{GATE_COOKIE}={}", crypto::gate_value("/admin", TOKEN));
        self.header("cookie", v)
    }
    fn csrf(self) -> Self {
        self.header("x-crab-admin", "1")
    }
    fn session(self, s: &str) -> Self {
        let v = format!("{GATE_COOKIE}={}; {SESSION_COOKIE}={s}", crypto::gate_value("/admin", TOKEN));
        self.header("cookie", v)
    }
    async fn send(self) -> Response {
        setup();
        let mut b = HttpRequest::builder().method(self.method).uri(self.uri);
        for (k, v) in &self.headers {
            b = b.header(*k, v.as_str());
        }
        let mut req = b.body(Body::from(self.body)).unwrap();
        req.extensions_mut().insert(ConnectInfo::<SocketAddr>(format!("{}:5555", self.peer).parse().unwrap()));
        build_app(routes()).oneshot(req).await.unwrap()
    }
}

async fn body(r: Response) -> String {
    String::from_utf8(axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap()
}

async fn json_body(r: Response) -> Value {
    serde_json::from_str(&body(r).await).unwrap()
}

fn set_cookie(r: &Response, name: &str) -> Option<String> {
    r.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(str::to_string)
}

fn cookie_value(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_string()
}

async fn snapshot(r: Response) -> (StatusCode, Vec<(String, String)>, String) {
    let status = r.status();
    let mut headers: Vec<(String, String)> =
        r.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect();
    headers.sort();
    (status, headers, body(r).await)
}

async fn login(peer: &str) -> String {
    let r = Req::post("/admin/api/login", &format!("{{\"devkey\":\"{DEVKEY}\"}}")).peer(peer).gate().csrf().send().await;
    assert_eq!(r.status(), StatusCode::OK);
    cookie_value(&set_cookie(&r, SESSION_COOKIE).unwrap())
}

#[tokio::test]
async fn valid_token_sets_gate_cookie_and_redirects() {
    for uri in [format!("/admin?{TOKEN}"), format!("/admin?token={TOKEN}")] {
        let r = Req::get(&uri).send().await;
        assert_eq!(r.status(), StatusCode::FOUND, "{uri}");
        assert_eq!(r.headers()[header::LOCATION], "/admin/");
        let c = set_cookie(&r, GATE_COOKIE).unwrap();
        assert!(c.contains("Path=/admin") && c.contains("HttpOnly") && c.contains("SameSite=Strict"), "{c}");
        assert!(!c.contains("Secure"));
        assert_eq!(cookie_value(&c), crypto::gate_value("/admin", TOKEN));
    }
}

#[tokio::test]
async fn gate_cookie_is_secure_behind_https_proxy() {
    let uri = format!("/admin?{TOKEN}");
    let r = Req::get(&uri).peer("127.0.0.1").header("x-forwarded-proto", "https").send().await;
    assert!(set_cookie(&r, GATE_COOKIE).unwrap().contains("; Secure"));
    // X-Forwarded-Proto from an untrusted peer is ignored
    let r = Req::get(&uri).peer("203.0.113.9").header("x-forwarded-proto", "https").send().await;
    assert!(!set_cookie(&r, GATE_COOKIE).unwrap().contains("Secure"));
}

#[tokio::test]
async fn invalid_requests_look_like_unknown_routes() {
    let unknown = snapshot(Req::get("/definitely-not-a-route").send().await).await;
    assert_eq!(unknown.0, StatusCode::NOT_FOUND);
    for uri in [
        "/admin",
        "/admin?wrong-token-000000",
        "/admin?token=nope",
        "/admin/",
        "/admin/index.html",
        "/admin/assets/index.js",
        "/admin/api/session",
        "/admin/api/config",
    ] {
        let got = snapshot(Req::get(uri).send().await).await;
        assert_eq!(got, unknown, "{uri}");
    }
    let wrong_gate = Req::get("/admin/").header("cookie", format!("{GATE_COOKIE}=deadbeef")).send().await;
    assert_eq!(snapshot(wrong_gate).await, unknown);
    let post = Req::post("/admin/api/login", "{}").csrf().send().await;
    let unknown_post = Req::post("/definitely-not-a-route", "{}").csrf().send().await;
    assert_eq!(snapshot(post).await, snapshot(unknown_post).await);
}

#[tokio::test]
async fn gated_shell_is_served_without_caching() {
    let r = Req::get("/admin/").gate().send().await;
    assert_ne!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(r.headers()[header::CACHE_CONTROL], "no-store");
    for uri in ["/admin/trackers", "/admin/jobs", "/admin/settings", "/admin/maintenance", "/admin/logs", "/admin/settings/trackers"] {
        let r = Req::get(uri).gate().send().await;
        assert_ne!(r.status(), StatusCode::NOT_FOUND, "{uri}: SPA routes fall back to the shell");
        assert_eq!(r.headers()[header::CACHE_CONTROL], "no-store");
    }
    let r = Req::get("/admin").gate().send().await;
    assert_eq!(r.status(), StatusCode::FOUND);
    assert_eq!(r.headers()[header::LOCATION], "/admin/");
    assert_eq!(Req::get("/admin/missing.js").gate().send().await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn meta_injection() {
    let html = "<!doctype html><html><HEAD lang=x><title>A</title></head><body></body></html>";
    let out = inject_base_meta(html, "/panel");
    assert!(
        out.contains("<HEAD lang=x>\n    <base href=\"/panel/\"><meta name=\"crab-admin-base\" content=\"/panel\"><title>"),
        "{out}"
    );
    assert!(inject_base_meta("<header></header>", "/a1").starts_with("<base href=\"/a1/\"><meta name=\"crab-admin-base\" content=\"/a1\">"));
}

#[tokio::test]
async fn session_endpoint_and_login_flow() {
    let r = Req::get("/admin/api/session").gate().send().await;
    assert_eq!(r.status(), StatusCode::OK);
    let v = json_body(r).await;
    assert_eq!(v["authenticated"], false);
    assert_eq!(v["loginEnabled"], true);
    assert_eq!(v["version"], version::VERSION);

    let peer = "198.51.100.10";
    let r = Req::post("/admin/api/login", &format!("{{\"devkey\":\"{DEVKEY}\"}}")).peer(peer).gate().csrf().send().await;
    assert_eq!(r.status(), StatusCode::OK);
    let c = set_cookie(&r, SESSION_COOKIE).unwrap();
    assert!(c.contains("Path=/admin") && c.contains("HttpOnly") && c.contains("SameSite=Strict") && c.contains("Max-Age=7200"), "{c}");
    assert_eq!(json_body(r).await, json!({ "ok": true }));
    let s = cookie_value(&c);

    let v = json_body(Req::get("/admin/api/session").session(&s).send().await).await;
    assert_eq!(v["authenticated"], true);

    let r = Req::post("/admin/api/logout", "").session(&s).csrf().send().await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(set_cookie(&r, SESSION_COOKIE).unwrap().contains("Max-Age=0"));
    let v = json_body(Req::get("/admin/api/session").session(&s).send().await).await;
    assert_eq!(v["authenticated"], false);
}

#[tokio::test]
async fn wrong_key_and_rate_limit() {
    let peer = "198.51.100.20";
    for _ in 0..session::MAX_FAILURES {
        let r = Req::post("/admin/api/login", r#"{"devkey":"wrong"}"#).peer(peer).gate().csrf().send().await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(json_body(r).await, json!({ "ok": false, "error": "invalid key" }));
    }
    let r = Req::post("/admin/api/login", &format!("{{\"devkey\":\"{DEVKEY}\"}}")).peer(peer).gate().csrf().send().await;
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS, "limit applies even to the right key");
    assert!(set_cookie(&r, SESSION_COOKIE).is_none());
    // other clients are unaffected
    login("198.51.100.21").await;
    let r = Req::post("/admin/api/login", "not json").peer("198.51.100.22").gate().csrf().send().await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn csrf_header_is_required_for_mutations() {
    let r = Req::post("/admin/api/login", &format!("{{\"devkey\":\"{DEVKEY}\"}}")).peer("198.51.100.30").gate().send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let s = login("198.51.100.31").await;
    let r = Req::post("/admin/api/config/validate", r#"{"data":{}}"#).session(&s).send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let r = Req::post("/admin/api/config/validate", r#"{"data":{}}"#).session(&s).header("x-crab-admin", "0").send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let r = Req::post("/admin/api/config/validate", r#"{"data":{"tracksmod":1}}"#).session(&s).csrf().send().await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(json_body(r).await["ok"], true);
    let r = Req::get("/admin/api/session").gate().header("sec-fetch-site", "cross-site").send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let r = Req::get("/admin/api/session").gate().header("sec-fetch-site", "same-origin").send().await;
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_api_requires_session() {
    for uri in ["/admin/api/overview", "/admin/api/logs", "/admin/api/logs/rutor.log", "/admin/api/config", "/admin/api/cron/rutor/parsealltask"] {
        let r = Req::get(uri).gate().send().await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(json_body(r).await, json!({ "error": "unauthorized" }));
    }
    let r = Req::get("/admin/api/overview").session(&"0".repeat(64)).send().await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_paths_map_to_existing_handlers() {
    let s = login("198.51.100.40").await;
    let r = Req::get("/admin/api/config/schema").session(&s).send().await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()[header::CACHE_CONTROL], "no-store");
    let v = json_body(r).await;
    assert_eq!(v["ok"], true);
    assert!(v["schema"]["groups"].as_array().unwrap().iter().any(|g| g["id"] == "admin"));

    let v = json_body(Req::get("/admin/api/config").session(&s).send().await).await;
    assert_eq!(v["data"]["admin"]["token"], TOKEN);

    // cron handlers need no devkey from a public peer when reached through the admin API
    let r = Req::get("/admin/api/cron/Rutor/ParseAllTask?Page=2").session(&s).send().await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(body(r).await, "cron:page=2");

    let r = Req::get("/admin/api/health/background-jobs").session(&s).send().await;
    assert_eq!(json_body(r).await["jobs"], json!([]));

    let r = Req::get("/admin/api/nope").session(&s).send().await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(r).await, json!({ "error": "not found" }));

    let r = Req::get("/admin/api/overview").session(&s).send().await;
    assert_eq!(r.status(), StatusCode::OK);
    let v = json_body(r).await;
    assert_eq!(v["version"], version::VERSION);
    assert_eq!(v["trackers"].as_array().unwrap().len(), 25);
    assert_eq!(v["trackers"][0]["slug"], "anibelka");
    assert_eq!(v["trackers"][0]["name"], "Anibelka");
    assert!(v["sync"].is_object() && v["config"].is_object());

    let r = Req::get("/admin/api/logs").session(&s).send().await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(json_body(r).await.is_array());
    for bad in ["/admin/api/logs/..%2f..%2fCargo.toml", "/admin/api/logs/../x.log", "/admin/api/logs/Cargo.toml", "/admin/api/logs/missing.log"] {
        let r = Req::get(bad).session(&s).send().await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{bad}");
    }

    // FileDB change journal: state, validation, clear (the route wins over logs/{file})
    let v = json_body(Req::get("/admin/api/logs/fdb").session(&s).send().await).await;
    for k in ["enabled", "retentionDays", "maxSizeMb", "maxFiles", "files", "totalBytes"] {
        assert!(!v[k].is_null(), "{k}");
    }
    let r = Req::post("/admin/api/logs/fdb", r#"{"maxSizeMb":-5}"#).session(&s).csrf().send().await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    let r = Req::post("/admin/api/logs/fdb", "[1]").session(&s).csrf().send().await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    let r = Req::post("/admin/api/logs/fdb/clear", "").session(&s).csrf().send().await;
    assert_eq!(json_body(r).await["ok"], true);
    let r = Req::post("/admin/api/logs/fdb/clear", "").session(&s).send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "CSRF header required");
}

#[tokio::test]
async fn public_config_api_is_gone() {
    for peer in ["127.0.0.1", "192.168.1.10", "203.0.113.5"] {
        for uri in ["/api/v1.0/config", "/api/v1.0/config/schema"] {
            let r = Req::get(uri).peer(peer).header("x-dev-key", DEVKEY).send().await;
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{peer} {uri}");
        }
    }
    // LAN access to /cron keeps working
    assert_eq!(Req::get("/cron/rutor/parsealltask").peer("192.168.1.10").send().await.status(), StatusCode::OK);
}

#[test]
fn path_matching_and_mapping() {
    assert_eq!(classify("/admin", "/admin"), Some(Target::Entry));
    assert_eq!(classify("/admin/", "/admin"), Some(Target::Inner("")));
    assert_eq!(classify("/admin/api/x", "/admin"), Some(Target::Inner("api/x")));
    assert_eq!(classify("/administrator", "/admin"), None);
    assert_eq!(classify("/Admin", "/admin"), None);
    // the prefix comes from the live config: a new path applies on the next request
    let mut c = AppOptions::default();
    c.admin.path = "/door-7".into();
    assert_eq!(classify("/door-7/api/session", &c.admin_path()), Some(Target::Inner("api/session")));
    assert_eq!(classify("/admin/api/session", &c.admin_path()), None);

    assert_eq!(internal_path("config").as_deref(), Some("/api/v1.0/config"));
    assert_eq!(internal_path("config/schema").as_deref(), Some("/api/v1.0/config/schema"));
    assert_eq!(internal_path("cron/rutor/parse").as_deref(), Some("/cron/rutor/parse"));
    assert_eq!(internal_path("dev/findcorrupt").as_deref(), Some("/dev/findcorrupt"));
    assert_eq!(internal_path("jsondb/save").as_deref(), Some("/jsondb/save"));
    assert_eq!(internal_path("stats/torrents").as_deref(), Some("/stats/torrents"));
    assert_eq!(internal_path("health/background-jobs").as_deref(), Some("/health/background-jobs"));
    for none in ["cron", "cron/", "dev", "stats", "health", "health/x", "sync/conf", "", "api/v1.0/torrents"] {
        assert!(internal_path(none).is_none(), "{none}");
    }
    assert_eq!(query_token(Some("abc")).as_deref(), Some("abc"));
    assert_eq!(query_token(Some("token=abc&x=1")).as_deref(), Some("abc"));
    assert_eq!(query_token(Some("x=1")), None);
    assert_eq!(query_token(None), None);
}
