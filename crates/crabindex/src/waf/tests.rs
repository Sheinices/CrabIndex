//! WAF unit tests (isolated `Waf` instances) and full-pipeline tests (`build_app` + `oneshot`
//! against the global instance). Pipeline tests use a distinct client IP each so they can run
//! in parallel against the shared state.

use super::*;
use crate::admin::{crypto, session, GATE_COOKIE, SESSION_COOKIE};
use crate::app::build_app;
use crate::test_support::{setup, DEVKEY, TOKEN};
use axum::extract::ConnectInfo;
use axum::http::Method;
use axum::routing::get;
use axum::Router;
use serde_json::Value;
use std::net::SocketAddr;
use tower::ServiceExt;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn temp_waf(tag: &str) -> Waf {
    let p = std::env::temp_dir().join(format!("crab-waf-unit-{tag}-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&p);
    Waf::open(p)
}

fn cfg() -> WafSettings {
    WafSettings { blockUserAgents: vec!["sqlmap".into(), "^curl/7\\.1".into()], ..WafSettings::default() }
}

// ---------------------------------------------------------------------------
// Unit
// ---------------------------------------------------------------------------

#[test]
fn evaluation_order_and_loopback() {
    let w = temp_waf("order");
    let c = cfg();
    let now = Utc::now();
    let pub1 = Some(ip("203.0.113.50"));
    assert_eq!(w.evaluate(&c, pub1, "/api/v1.0/torrents", "Mozilla", now), None);

    // blacklist beats UA / trap; whitelist beats blacklist
    w.upsert_rule(ListKind::Blacklist, IpNet::parse("203.0.113.0/24").unwrap(), String::new(), None, now);
    let b = w.evaluate(&c, pub1, "/.env", "sqlmap", now).unwrap();
    assert_eq!((b.reason, b.status, b.banned), (Reason::Blacklist, StatusCode::FORBIDDEN, false));
    w.upsert_rule(ListKind::Whitelist, IpNet::parse("203.0.113.50").unwrap(), String::new(), None, now);
    assert_eq!(w.evaluate(&c, pub1, "/.env", "sqlmap", now), None);

    // loopback is never blocked, even by a catch-all blacklist or a ban
    w.upsert_rule(ListKind::Blacklist, IpNet::parse("0.0.0.0/0").unwrap(), String::new(), None, now);
    w.upsert_rule(ListKind::Blacklist, IpNet::parse("::/0").unwrap(), String::new(), None, now);
    w.ban(ip("127.0.0.1"), "manual", now + Duration::hours(1), now);
    for lo in ["127.0.0.1", "::1", "::ffff:127.0.0.1"] {
        assert_eq!(w.evaluate(&c, Some(ip(lo)), "/.env", "sqlmap", now), None, "{lo}");
    }
    // LAN only with whitelistLan
    assert_eq!(w.evaluate(&c, Some(ip("192.168.1.5")), "/", "", now), None);
    let strict = WafSettings { whitelistLan: false, ..cfg() };
    assert_eq!(w.evaluate(&strict, Some(ip("192.168.1.5")), "/", "", now).unwrap().reason, Reason::Blacklist);
    assert_eq!(w.evaluate(&strict, Some(ip("::1")), "/", "", now), None);
    // no client address at all → allowed
    assert_eq!(w.evaluate(&c, None, "/.env", "sqlmap", now), None);
}

#[test]
fn ua_trap_and_ban_expiry() {
    let w = temp_waf("ua");
    let c = cfg();
    let now = Utc::now();
    let a = Some(ip("2001:db8::7"));
    let b = w.evaluate(&c, a, "/", "Mozilla/5.0 SQLMap/1.7", now).unwrap();
    assert_eq!((b.reason, b.status, b.banned), (Reason::Ua, StatusCode::FORBIDDEN, true));
    // banned for rateLimit.banMinutes
    assert_eq!(w.evaluate(&c, a, "/", "Mozilla", now + Duration::minutes(14)).unwrap().reason, Reason::Ban);
    assert_eq!(w.evaluate(&c, a, "/", "Mozilla", now + Duration::minutes(16)), None);
    assert_eq!(w.evaluate(&c, Some(ip("198.51.100.1")), "/", "curl/7.10", now).unwrap().reason, Reason::Ua);
    assert_eq!(w.evaluate(&c, Some(ip("198.51.100.2")), "/", "curl/8.0", now), None);

    for (n, p) in ["/.ENV", "/wp-admin/setup.php", "//wp-login.php", "/%2egit/config", "/phpMyAdmin/index.php"].iter().enumerate() {
        let who = Some(ip(&format!("198.51.100.{}", 10 + n)));
        let b = w.evaluate(&c, who, p, "Mozilla", now).unwrap();
        assert_eq!((b.reason, b.status), (Reason::Trap, StatusCode::NOT_FOUND), "{p}");
        let ban = w.lists.read().ban_of(who.unwrap(), now).cloned().unwrap();
        assert_eq!(ban.reason, "trap");
        assert_eq!((ban.expires - ban.created).num_minutes(), 1440);
    }
    assert!(!trap_hit(&c.trapPaths, "/.git"));
    assert!(!trap_hit(&c.trapPaths, "/api/.envelope"));
    assert!(trap_hit(&["wp-json".into()], "/wp-json/x"));

    // expired entries are pruned by maintenance
    assert!(!w.maintain(now));
    assert!(w.maintain(now + Duration::days(2)));
    assert!(w.lists.read().bans.is_empty());
}

#[test]
fn rate_limit_bans() {
    let w = temp_waf("rate");
    let c = WafSettings { rateLimit: crab_core::config::WafRateLimit { enable: true, perMinute: 5, banMinutes: 2 }, ..cfg() };
    let now = Utc::now();
    let a = Some(ip("192.0.2.77"));
    for _ in 0..5 {
        assert_eq!(w.evaluate(&c, a, "/", "", now), None);
    }
    let b = w.evaluate(&c, a, "/", "", now).unwrap();
    assert_eq!((b.reason, b.status, b.retry_after, b.banned), (Reason::Rate, StatusCode::TOO_MANY_REQUESTS, Some(120), true));
    assert_eq!(w.evaluate(&c, a, "/", "", now).unwrap().reason, Reason::Ban);
    // after the ban and a quiet minute the address is fine again
    assert_eq!(w.evaluate(&c, a, "/", "", now + Duration::minutes(3)), None);
    // LAN is never rate limited
    for _ in 0..20 {
        assert_eq!(w.evaluate(&c, Some(ip("10.1.2.3")), "/", "", now), None);
    }
    let off = WafSettings { rateLimit: crab_core::config::WafRateLimit { enable: false, ..c.rateLimit.clone() }, ..c.clone() };
    for _ in 0..20 {
        assert_eq!(w.evaluate(&off, Some(ip("192.0.2.78")), "/", "", now), None);
    }
}

#[test]
fn builtin_domains_block_everyone_but_loopback() {
    let w = temp_waf("builtin");
    let c = cfg();
    let now = Utc::now();
    let ev = |a: &str, d: &str| w.evaluate_request(&c, Some(ip(a)), "/api/v1.0/torrents", "Mozilla", Some(d), Some("crab.example"), now);
    let b = ev("203.0.113.60", "a.b.ndst.pw").unwrap();
    assert_eq!((b.reason, b.status, b.banned), (Reason::Domain, StatusCode::FORBIDDEN, false));
    assert_eq!(ev("203.0.113.60", "notndst.pw"), None);
    assert_eq!(ev("203.0.113.60", "myds.me").unwrap().reason, Reason::Domain);
    // whitelisted IP, LAN (whitelistLan) and the admin domain whitelist do not help
    w.upsert_rule(ListKind::Whitelist, IpNet::parse("203.0.113.61").unwrap(), String::new(), None, now);
    assert_eq!(ev("203.0.113.61", "lampa.stream").unwrap().reason, Reason::Domain);
    assert_eq!(ev("192.168.1.5", "x.lampa.stream").unwrap().reason, Reason::Domain);
    w.lists.write().domain_whitelist.push(store::ListEntry { value: "ndst.pw".into(), comment: String::new(), created: now, expires: None });
    assert_eq!(ev("203.0.113.62", "ndst.pw").unwrap().reason, Reason::Domain);
    // loopback always passes; no client address still gets the builtin check
    for lo in ["127.0.0.1", "::1", "::ffff:127.0.0.1"] {
        assert_eq!(ev(lo, "ndst.pw"), None, "{lo}");
    }
    assert_eq!(w.evaluate_request(&c, None, "/", "", Some("ndst.pw"), None, now).unwrap().reason, Reason::Domain);
    // no ban is recorded for domain blocks
    assert!(w.lists.read().bans.is_empty());
}

#[test]
fn admin_domain_lists_and_allowlist_only() {
    let w = temp_waf("domains");
    let c = cfg();
    let now = Utc::now();
    let pub1 = Some(ip("198.51.100.150"));
    let ev = |c: &WafSettings, d: Option<&str>, ua: &str| w.evaluate_request(c, pub1, "/api", ua, d, Some("crab.example"), now);

    w.upsert_domain(DomainKind::Blacklist, "spam.example".into(), String::new(), None, now);
    assert_eq!(ev(&c, Some("cdn.spam.example"), "Mozilla").unwrap().reason, Reason::Domain);
    assert_eq!(ev(&c, Some("antispam.example"), "Mozilla"), None);
    assert!(w.lists.read().bans.is_empty());
    // the IP blacklist / bans are checked before the domain lists
    w.upsert_rule(ListKind::Blacklist, IpNet::parse("198.51.100.150").unwrap(), String::new(), None, now);
    assert_eq!(ev(&c, Some("spam.example"), "").unwrap().reason, Reason::Blacklist);
    assert!(w.remove_rule(ListKind::Blacklist, IpNet::parse("198.51.100.150").unwrap()));

    // the domain whitelist skips UA / trap / rate, but never bans or the IP blacklist
    w.upsert_domain(DomainKind::Whitelist, "friend.example".into(), String::new(), None, now);
    assert_eq!(ev(&c, Some("app.friend.example"), "sqlmap"), None);
    assert_eq!(w.evaluate_request(&c, pub1, "/.env", "", Some("friend.example"), None, now), None);
    assert_eq!(ev(&c, None, "sqlmap").unwrap().reason, Reason::Ua);
    assert_eq!(ev(&c, Some("friend.example"), "Mozilla").unwrap().reason, Reason::Ban);
    w.unban(ip("198.51.100.150"));

    // allowlist-only: foreign domains 403, whitelisted / own host / no Origin pass
    let strict = WafSettings { domainAllowlistOnly: true, ..cfg() };
    assert_eq!(ev(&strict, Some("other.example"), "Mozilla").unwrap().reason, Reason::Domain);
    assert_eq!(ev(&strict, Some("friend.example"), "Mozilla"), None);
    assert_eq!(ev(&strict, Some("crab.example"), "Mozilla"), None);
    assert_eq!(ev(&strict, Some("sub.crab.example"), "Mozilla").unwrap().reason, Reason::Domain);
    assert_eq!(ev(&strict, None, "Mozilla"), None);
    assert_eq!(ev(&c, Some("other.example"), "Mozilla"), None);
    // expired entries stop matching
    w.upsert_domain(DomainKind::Blacklist, "temp.example".into(), String::new(), Some(now - Duration::minutes(1)), now - Duration::minutes(5));
    assert_eq!(ev(&c, Some("temp.example"), "Mozilla"), None);
    assert!(w.maintain(now));
    assert!(w.snapshot(now).domain_blacklist.iter().all(|e| e.value != "temp.example"));
}

#[test]
fn persistence_round_trip() {
    let w = temp_waf("persist");
    let now = Utc::now();
    w.upsert_rule(ListKind::Blacklist, IpNet::parse("203.0.113.9").unwrap(), "scanner".into(), None, now);
    w.upsert_rule(ListKind::Whitelist, IpNet::parse("2001:db8::/48").unwrap(), "office".into(), Some(now + Duration::hours(1)), now);
    w.ban(ip("192.0.2.10"), "manual", now + Duration::minutes(30), now);
    w.save().unwrap();

    let back = Waf::open(w.path.clone());
    let (a, b) = (w.snapshot(now), back.snapshot(now));
    assert_eq!(a.blacklist, b.blacklist);
    assert_eq!(a.whitelist, b.whitelist);
    assert_eq!(a.bans.len(), 1);
    assert_eq!(a.bans, b.bans);
    assert_eq!(b.bans[0].ip, "192.0.2.10");
    assert_eq!(back.evaluate(&cfg(), Some(ip("203.0.113.9")), "/", "", now).unwrap().reason, Reason::Blacklist);
    assert_eq!(back.evaluate(&cfg(), Some(ip("192.0.2.10")), "/", "", now).unwrap().reason, Reason::Ban);
    assert_eq!(back.evaluate(&cfg(), Some(ip("2001:db8:0:1::5")), "/.env", "sqlmap", now), None);
    let _ = std::fs::remove_file(&w.path);
}

#[test]
fn recorded_path_hides_token() {
    assert_eq!(record_path("/admin/api/waf/overview", TOKEN), "/admin/api/waf/overview");
    assert_eq!(record_path(&format!("/admin/{TOKEN}"), TOKEN), "/admin/***");
    assert_eq!(record_path("/x", ""), "/x");
    assert_eq!(record_path(&"é".repeat(400), "").len(), 512);
}

#[test]
fn self_protection_rules() {
    let you = Some(ip("203.0.113.5"));
    let n = |s: &str| IpNet::parse(s).unwrap();
    assert!(api::self_protection(&n("203.0.113.5"), you).is_err());
    assert!(api::self_protection(&n("203.0.113.0/24"), you).is_err());
    assert!(api::self_protection(&n("0.0.0.0/0"), None).is_err());
    assert!(api::self_protection(&n("127.0.0.2"), None).is_err());
    assert!(api::self_protection(&n("::/0"), None).is_err());
    assert!(api::self_protection(&n("203.0.113.6"), you).is_ok());
    assert!(api::self_protection(&n("2001:db8::/32"), you).is_ok());
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

fn routes() -> Router {
    Router::new()
        .route("/api/v1.0/torrents", get(|| async { "ok" }))
        .merge(crate::controllers::health::router())
        .fallback(|| async { StatusCode::NOT_FOUND })
}

struct Req {
    method: Method,
    uri: String,
    peer: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Req {
    fn new(method: Method, uri: &str, peer: &str) -> Self {
        Req { method, uri: uri.into(), peer: peer.into(), headers: vec![], body: String::new() }
    }
    fn get(uri: &str, peer: &str) -> Self {
        Req::new(Method::GET, uri, peer)
    }
    fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
    fn ua(self, v: &str) -> Self {
        self.header("user-agent", v)
    }
    fn body(mut self, b: &str) -> Self {
        self.body = b.into();
        self
    }
    /// Admin gate + session + CSRF header.
    fn admin(self) -> Self {
        let gate = crypto::gate_value("/admin", TOKEN);
        let s = session::create(DEVKEY, 2);
        self.header("cookie", &format!("{GATE_COOKIE}={gate}; {SESSION_COOKIE}={s}")).header("x-crab-admin", "1")
    }
    async fn send(self) -> Response {
        setup();
        let mut b = axum::http::Request::builder().method(self.method).uri(self.uri);
        for (k, v) in &self.headers {
            b = b.header(k.as_str(), v.as_str());
        }
        let mut req = b.body(Body::from(self.body)).unwrap();
        req.extensions_mut().insert(ConnectInfo::<SocketAddr>(SocketAddr::new(self.peer.parse().unwrap(), 5555)));
        build_app(routes()).oneshot(req).await.unwrap()
    }
}

async fn text(r: Response) -> String {
    String::from_utf8(axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap()
}

async fn json(r: Response) -> Value {
    serde_json::from_str(&text(r).await).unwrap()
}

fn logged(ip: &str) -> Vec<Value> {
    WAF.stats.requests(&stats::RequestFilter { ip: Some(ip.into()), limit: 10_000, ..Default::default() })
}

fn ban_reason(addr: &str) -> Option<String> {
    WAF.lists.read().ban_of(ip(addr), Utc::now()).map(|b| b.reason.clone())
}

#[tokio::test]
async fn pipeline_blacklist_is_403_and_recorded() {
    let now = Utc::now();
    WAF.upsert_rule(ListKind::Blacklist, IpNet::parse("203.0.113.77").unwrap(), String::new(), None, now);
    let r = Req::get("/health?secret=1", "203.0.113.77").send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(r.headers()[header::CONTENT_TYPE], "text/plain; charset=utf-8");
    assert_eq!(text(r).await, "Forbidden");
    let log = logged("203.0.113.77");
    assert_eq!(log[0]["path"], "/health");
    assert_eq!(log[0]["status"], 403);
    assert_eq!(log[0]["blocked"], "blacklist");
}

#[tokio::test]
async fn pipeline_whitelist_bypasses_everything() {
    let now = Utc::now();
    WAF.upsert_rule(ListKind::Whitelist, IpNet::parse("198.51.100.64/28").unwrap(), String::new(), None, now);
    WAF.ban(ip("198.51.100.70"), "manual", now + Duration::hours(1), now);
    let r = Req::get("/health", "198.51.100.70").ua("sqlmap/1.7").send().await;
    assert_eq!(r.status(), StatusCode::OK);
    let r = Req::get("/.env", "198.51.100.70").send().await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert!(logged("198.51.100.70").iter().all(|e| e["blocked"].is_null()));
}

#[tokio::test]
async fn pipeline_expired_ban_is_ignored() {
    let now = Utc::now();
    WAF.ban(ip("198.51.100.90"), "manual", now - Duration::seconds(1), now - Duration::minutes(10));
    assert_eq!(Req::get("/health", "198.51.100.90").send().await.status(), StatusCode::OK);
    WAF.ban(ip("198.51.100.90"), "manual", now + Duration::minutes(10), now);
    assert_eq!(Req::get("/health", "198.51.100.90").send().await.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn pipeline_rate_limit_429_then_ban() {
    setup();
    let peer = "203.0.113.80";
    let per_minute = crate::conf().waf.rateLimit.perMinute;
    for i in 0..per_minute {
        let r = Req::get("/api/v1.0/torrents", peer).send().await;
        assert_eq!(r.status(), StatusCode::OK, "request {i}");
    }
    let r = Req::get("/api/v1.0/torrents", peer).send().await;
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.headers()[header::RETRY_AFTER], "900");
    assert_eq!(ban_reason(peer).as_deref(), Some("rate"));
    let r = Req::get("/api/v1.0/torrents", peer).send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let log = logged(peer);
    assert_eq!(log[0]["blocked"], "ban");
    assert_eq!(log[1]["blocked"], "rate");
    assert_eq!(log[1]["status"], 429);
    assert_eq!(log[2]["status"], 200);
}

#[tokio::test]
async fn pipeline_trap_path_404_and_ban() {
    let peer = "203.0.113.81";
    let r = Req::get("/WP-Login.php?log=admin", peer).send().await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(ban_reason(peer).as_deref(), Some("trap"));
    let r = Req::get("/health", peer).send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let log = logged(peer);
    assert_eq!(log[1]["path"], "/WP-Login.php");
    assert_eq!(log[1]["blocked"], "trap");
}

#[tokio::test]
async fn pipeline_user_agent_block() {
    let peer = "2001:db8::81";
    let r = Req::get("/health", peer).ua("Mozilla/5.0 (Nikto/2.5)").send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(ban_reason(peer).as_deref(), Some("ua"));
    assert_eq!(logged(peer)[0]["ua"], "Mozilla/5.0 (Nikto/2.5)");
}

#[tokio::test]
async fn pipeline_loopback_and_lan_never_blocked_but_proxied_client_is() {
    for peer in ["127.0.0.1", "::1", "192.168.1.50"] {
        let r = Req::get("/.env", peer).ua("sqlmap").send().await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{peer}");
        let r = Req::get("/health", peer).ua("sqlmap").send().await;
        assert_eq!(r.status(), StatusCode::OK, "{peer}");
        assert!(ban_reason(peer).is_none());
    }
    // behind a same-host proxy the forwarded client is the one evaluated
    let r = Req::get("/health", "127.0.0.1").header("x-forwarded-for", "203.0.113.83").ua("sqlmap").send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(ban_reason("203.0.113.83").as_deref(), Some("ua"));
    assert!(ban_reason("127.0.0.1").is_none());
    // a LAN peer cannot spoof its address
    let r = Req::get("/health", "192.168.1.51").header("x-forwarded-for", "203.0.113.83").send().await;
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn pipeline_admin_token_never_logged() {
    let peer = "203.0.113.84";
    for uri in [format!("/admin?{TOKEN}"), format!("/admin/?token={TOKEN}"), format!("/admin/{TOKEN}"), format!("/admin/api/session?t={TOKEN}")] {
        Req::get(&uri, peer).send().await;
    }
    let log = logged(peer);
    assert_eq!(log.len(), 4);
    assert!(log.iter().any(|e| e["path"] == "/admin"));
    assert!(log.iter().any(|e| e["path"] == "/admin/***"));
    let all = WAF.stats.requests(&stats::RequestFilter { limit: 100_000, ..Default::default() });
    for e in all {
        assert!(!e.to_string().contains(TOKEN), "{e}");
        assert!(!e["path"].as_str().unwrap().contains('?'), "{e}");
    }
}

#[tokio::test]
async fn admin_api_rules_add_remove_validate_and_protect() {
    let me = "203.0.113.90";
    let post = |body: &str| Req::new(Method::POST, "/admin/api/waf/rules", me).admin().body(body).send();

    // validation
    for bad in [
        r#"{"list":"blacklist","value":"nope"}"#,
        r#"{"list":"graylist","value":"192.0.2.1"}"#,
        r#"{"list":"blacklist","value":"192.0.2.1","expiresMinutes":-5}"#,
        r#"{"list":"blacklist","value":"192.0.2.1/40"}"#,
        "not json",
    ] {
        let r = post(bad).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{bad}");
        let v = json(r).await;
        assert_eq!(v["ok"], false);
        assert!(v["error"].is_string());
    }
    // self-protection: own IP, a network containing it, loopback, everything
    for v in [me, "203.0.113.0/24", "127.0.0.1", "0.0.0.0/0", "::1"] {
        let r = post(&format!(r#"{{"list":"blacklist","value":"{v}"}}"#)).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{v}");
    }
    // whitelisting yourself is fine
    let r = post(&format!(r#"{{"list":"whitelist","value":"{me}","comment":"me","expiresMinutes":5}}"#)).await;
    assert_eq!(r.status(), StatusCode::OK);
    let r = post(r#"{"list":"blacklist","value":"192.0.2.202/30","comment":"bots"}"#).await;
    assert_eq!(json(r).await["ok"], true);
    // missing X-Crab-Admin is refused by the admin layer
    let gate = crypto::gate_value("/admin", TOKEN);
    let s = session::create(DEVKEY, 2);
    let r = Req::new(Method::POST, "/admin/api/waf/rules", me)
        .header("cookie", &format!("{GATE_COOKIE}={gate}; {SESSION_COOKIE}={s}"))
        .body(r#"{"list":"blacklist","value":"192.0.2.1"}"#)
        .send()
        .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    // no session → 401
    let r = Req::get("/admin/api/waf/rules", me).header("cookie", &format!("{GATE_COOKIE}={gate}")).send().await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    let rules = json(Req::get("/admin/api/waf/rules", me).admin().send().await).await;
    assert_eq!(rules["you"], me);
    assert_eq!(rules["config"]["rateLimit"]["perMinute"], 300);
    let bl = rules["blacklist"].as_array().unwrap();
    let e = bl.iter().find(|e| e["value"] == "192.0.2.200/30").unwrap();
    assert_eq!(e["comment"], "bots");
    assert!(e["expires"].is_null());
    let wl = rules["whitelist"].as_array().unwrap().iter().find(|e| e["value"] == me).unwrap().clone();
    assert!(wl["expires"].as_str().unwrap().ends_with('Z'));

    assert_eq!(Req::get("/health", "192.0.2.201").send().await.status(), StatusCode::FORBIDDEN);
    let del = |q: &str| Req::new(Method::DELETE, &format!("/admin/api/waf/rules?{q}"), me).admin().send();
    assert_eq!(del("list=blacklist&value=192.0.2.200%2F30").await.status(), StatusCode::OK);
    assert_eq!(del("list=blacklist&value=192.0.2.200%2F30").await.status(), StatusCode::NOT_FOUND);
    assert_eq!(del("list=blacklist&value=zzz").await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(Req::get("/health", "192.0.2.201").send().await.status(), StatusCode::OK);
    assert_eq!(del(&format!("list=whitelist&value={me}")).await.status(), StatusCode::OK);

    // persisted (written in the background)
    let path = WAF.path.clone();
    WAF.upsert_rule(ListKind::Blacklist, IpNet::parse("192.0.2.250").unwrap(), "persist".into(), None, Utc::now());
    WAF.save().unwrap();
    let on_disk = store::load(&path).unwrap();
    assert!(on_disk.blacklist.iter().any(|e| e.value == "192.0.2.250" && e.comment == "persist"));
}

#[tokio::test]
async fn pipeline_domain_blocking() {
    let now = Utc::now();
    // builtin: Origin host (any subdomain), Referer fallback, Origin wins
    let r = Req::get("/health", "203.0.113.120").header("origin", "https://App.NDST.pw:443").send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(text(r).await, "Forbidden");
    let r = Req::get("/health", "203.0.113.120").header("referer", "https://lampa.click/x?y=1").send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let r = Req::get("/health", "203.0.113.120").header("origin", "https://ok.example").header("referer", "https://ndst.pw/").send().await;
    assert_eq!(r.status(), StatusCode::OK);
    let r = Req::get("/health", "203.0.113.120").header("origin", "https://ndst.pw").header("referer", "https://ok.example/").send().await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(Req::get("/health", "203.0.113.120").header("origin", "https://notndst.pw").send().await.status(), StatusCode::OK);
    assert!(ban_reason("203.0.113.120").is_none());
    let log = logged("203.0.113.120");
    assert_eq!(log[1]["blocked"], "domain");
    assert_eq!(log[1]["origin"], "ndst.pw");
    assert_eq!(log[2]["origin"], "ok.example");
    assert!(log.iter().all(|e| e["blocked"].is_null() || e["blocked"] == "domain"));

    // whitelisted IP and LAN still blocked by the builtin list, loopback is not
    WAF.upsert_rule(ListKind::Whitelist, IpNet::parse("203.0.113.121").unwrap(), String::new(), None, now);
    assert_eq!(Req::get("/health", "203.0.113.121").header("origin", "https://xabb.ru").send().await.status(), StatusCode::FORBIDDEN);
    assert_eq!(Req::get("/health", "192.168.1.60").header("origin", "https://xabb.ru").send().await.status(), StatusCode::FORBIDDEN);
    assert_eq!(Req::get("/health", "127.0.0.1").header("origin", "https://xabb.ru").send().await.status(), StatusCode::OK);

    // admin domain blacklist / whitelist
    WAF.upsert_domain(DomainKind::Blacklist, "pipe-bad.example".into(), String::new(), None, now);
    assert_eq!(Req::get("/health", "203.0.113.122").header("origin", "https://x.pipe-bad.example").send().await.status(), StatusCode::FORBIDDEN);
    assert!(ban_reason("203.0.113.122").is_none());
    WAF.upsert_domain(DomainKind::Whitelist, "pipe-good.example".into(), String::new(), None, now);
    let r = Req::get("/.env", "203.0.113.123").header("origin", "https://pipe-good.example").ua("sqlmap").send().await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert!(ban_reason("203.0.113.123").is_none());
}

#[tokio::test]
async fn admin_api_domain_rules() {
    let me = "203.0.113.93";
    let post = |body: &str| Req::new(Method::POST, "/admin/api/waf/rules", me).admin().body(body).send();
    let del = |q: &str| Req::new(Method::DELETE, &format!("/admin/api/waf/rules?{q}"), me).admin().send();

    for (body, needle) in [
        (r#"{"list":"domainWhitelist","value":"ndst.pw"}"#, "заблокирован встроенным списком и не может быть разрешён"),
        (r#"{"list":"domainWhitelist","value":"https://a.MYDS.me/x"}"#, "заблокирован встроенным списком и не может быть разрешён"),
        (r#"{"list":"domainBlacklist","value":"lampa.land"}"#, "уже заблокирован встроенным списком"),
        (r#"{"list":"domainBlacklist","value":"no_dot"}"#, "некорректный домен"),
        (r#"{"list":"domainBlacklist","value":"localhost"}"#, "некорректный домен"),
        (r#"{"list":"domains","value":"a.example"}"#, "domainBlacklist"),
    ] {
        let r = post(body).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{body}");
        let v = json(r).await;
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains(needle), "{body}: {v}");
    }
    assert_eq!(del("list=domainBlacklist&value=ndst.pw").await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(del("list=domainWhitelist&value=usph.xyz").await.status(), StatusCode::BAD_REQUEST);

    let r = post(r#"{"list":"domainBlacklist","value":"*.API-Bad.example","comment":"парсер","expiresMinutes":60}"#).await;
    assert_eq!(json(r).await["ok"], true);
    assert_eq!(post(r#"{"list":"domainWhitelist","value":"https://api-good.example:8443/app"}"#).await.status(), StatusCode::OK);

    let rules = json(Req::get("/admin/api/waf/rules", me).admin().send().await).await;
    let builtin: Vec<&str> = rules["builtinDomains"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(builtin, domains::BUILTIN_BLOCKED_DOMAINS);
    let e = rules["domainBlacklist"].as_array().unwrap().iter().find(|e| e["value"] == "api-bad.example").unwrap().clone();
    assert_eq!(e["comment"], "парсер");
    assert!(e["expires"].is_string());
    assert!(rules["domainWhitelist"].as_array().unwrap().iter().any(|e| e["value"] == "api-good.example"));
    assert!(rules["config"]["domainAllowlistOnly"].is_boolean());

    assert_eq!(Req::get("/health", "203.0.113.124").header("origin", "http://x.api-bad.example").send().await.status(), StatusCode::FORBIDDEN);
    let r = json(Req::get("/admin/api/waf/requests?origin=API-BAD&blocked=domain", me).admin().send().await).await;
    let rows = r.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["origin"], "x.api-bad.example");
    let o = json(Req::get("/admin/api/waf/overview", me).admin().send().await).await;
    assert!(o["blockedByReason"]["domain"].as_u64().unwrap() >= 1);
    assert!(o["topOrigins"].as_array().unwrap().iter().any(|t| t["origin"] == "x.api-bad.example" && t["blocked"].as_u64() >= Some(1)));

    assert_eq!(del("list=domainBlacklist&value=api-bad.example").await.status(), StatusCode::OK);
    assert_eq!(del("list=domainBlacklist&value=api-bad.example").await.status(), StatusCode::NOT_FOUND);
    assert_eq!(del("list=domainWhitelist&value=%2A.api-good.example").await.status(), StatusCode::OK);
    assert_eq!(del("list=domainWhitelist&value=bad_value").await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(Req::get("/health", "203.0.113.124").header("origin", "http://x.api-bad.example").send().await.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_api_ban_and_lift() {
    let me = "203.0.113.91";
    let ban = |body: &str| Req::new(Method::POST, "/admin/api/waf/ban", me).admin().body(body).send();
    for bad in [
        format!(r#"{{"ip":"{me}","minutes":10}}"#),
        r#"{"ip":"127.0.0.1","minutes":10}"#.to_string(),
        r#"{"ip":"192.0.2.0/24","minutes":10}"#.to_string(),
        r#"{"ip":"192.0.2.230","minutes":0}"#.to_string(),
        r#"{"ip":"192.0.2.230"}"#.to_string(),
    ] {
        assert_eq!(ban(&bad).await.status(), StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(ban(r#"{"ip":"192.0.2.230","minutes":"10","reason":"abuse"}"#).await.status(), StatusCode::OK);
    assert_eq!(ban_reason("192.0.2.230").as_deref(), Some("abuse"));
    assert_eq!(Req::get("/health", "192.0.2.230").send().await.status(), StatusCode::FORBIDDEN);

    let ips = json(Req::get("/admin/api/waf/ips?sort=lastSeen&limit=500", me).admin().send().await).await;
    let row = ips.as_array().unwrap().iter().find(|r| r["ip"] == "192.0.2.230").unwrap().clone();
    assert_eq!(row["state"], "banned");
    assert!(row["banExpires"].is_string());
    assert_eq!(row["blocked"], 1);

    let lift = |q: &str| Req::new(Method::DELETE, &format!("/admin/api/waf/ban?{q}"), me).admin().send();
    assert_eq!(lift("ip=192.0.2.230").await.status(), StatusCode::OK);
    assert_eq!(lift("ip=192.0.2.230").await.status(), StatusCode::NOT_FOUND);
    assert_eq!(lift("ip=x").await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(Req::get("/health", "192.0.2.230").send().await.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_api_statistics_endpoints() {
    let me = "203.0.113.92";
    Req::get("/health", "198.51.100.200").send().await;
    Req::get("/nope?q=1", "198.51.100.200").send().await;

    let o = json(Req::get("/admin/api/waf/overview?window=60m", me).admin().send().await).await;
    assert_eq!(o["enabled"], true);
    assert!(o["totals"]["requests"].as_u64().unwrap() >= 2);
    for k in ["uniqueIps", "rps", "blocked"] {
        assert!(!o["totals"][k].is_null(), "{k}");
    }
    for k in ["2xx", "3xx", "4xx", "5xx"] {
        assert!(o["statusCodes"][k].is_u64(), "{k}");
    }
    for k in ["blacklist", "ban", "ua", "trap", "rate", "domain"] {
        assert!(o["blockedByReason"][k].is_u64(), "{k}");
    }
    assert_eq!(o["timeline"].as_array().unwrap().len(), 60);
    assert!(o["topIps"].is_array() && o["topPaths"].is_array() && o["topOrigins"].is_array() && o["since"].is_string());
    let o = json(Req::get("/admin/api/waf/overview?window=24h", me).admin().send().await).await;
    assert_eq!(o["timeline"].as_array().unwrap().len(), 144);

    let r = json(Req::get("/admin/api/waf/requests?ip=198.51.100.200&status=4xx&limit=5", me).admin().send().await).await;
    let rows = r.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], "/nope");
    assert_eq!(rows[0]["method"], "GET");
    assert!(rows[0]["blocked"].is_null());
    assert!(rows[0]["ms"].is_u64() && rows[0]["time"].is_string());
    let r = json(Req::get("/admin/api/waf/requests?path=nop&blocked=false", me).admin().send().await).await;
    assert!(r.as_array().unwrap().iter().all(|e| e["path"].as_str().unwrap().contains("nop")));

    let ips = json(Req::get("/admin/api/waf/ips", me).admin().send().await).await;
    let row = ips.as_array().unwrap().iter().find(|r| r["ip"] == "198.51.100.200").unwrap().clone();
    assert_eq!(row["state"], "normal");
    assert_eq!(row["errors"], 1);
    assert_eq!(row["lastPath"], "/nope");
    assert!(row["firstSeen"].is_string());

    assert_eq!(Req::get("/admin/api/waf/unknown", me).admin().send().await.status(), StatusCode::NOT_FOUND);
}
