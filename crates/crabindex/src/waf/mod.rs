//! Web application firewall: IP lists, bans, User-Agent / trap-path filters, per-IP rate
//! limit and the in-memory request log shown in the admin panel.
//!
//! Evaluated right after the client network is captured (so the real client IP is known),
//! before the admin panel, static files and routing:
//!
//! 1. loopback → allowed (LAN too with `waf.whitelistLan`);
//! 2. whitelist → allowed, nothing below applies;
//! 3. blacklist or active ban → `403 Forbidden`;
//! 4. `blockUserAgents` match → `403` + ban for `rateLimit.banMinutes` (reason `ua`);
//! 5. `trapPaths` prefix → `404` + ban for `trapBanMinutes` (reason `trap`);
//! 6. rate limit exceeded → `429` + `Retry-After` + ban for `rateLimit.banMinutes` (reason `rate`).
//!
//! Every request is recorded (when `waf.logRequests`) with its final status and duration.
//! The query string is never stored. Lists and bans persist in `Data/waf.json`; statistics
//! live in memory only.

pub mod api;
pub mod limiter;
pub mod net;
pub mod stats;
pub mod store;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use crab_core::config::WafSettings;
use futures::FutureExt;
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use regex::{RegexBuilder, RegexSet, RegexSetBuilder};
use std::net::IpAddr;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::security::network::{is_local_or_private, ClientNetworkContext};
use crate::security::request_network;
use limiter::RateLimiter;
use net::{is_loopback, unmap, IpNet};
use stats::{LogEntry, Reason, Stats};
use store::{ListKind, Lists};

/// Log category.
pub const CAT: &str = "waf";
/// Lists and bans file.
pub const WAF_FILE: &str = "Data/waf.json";
const MAX_PATH_LEN: usize = 512;
const MAX_UA_LEN: usize = 256;

fn store_path() -> PathBuf {
    #[cfg(test)]
    {
        let p = std::env::temp_dir().join(format!("crab-{}-{}", std::process::id(), WAF_FILE.replace('/', "-")));
        let _ = std::fs::remove_file(&p);
        p
    }
    #[cfg(not(test))]
    {
        PathBuf::from(WAF_FILE)
    }
}

/// Process-wide instance (lists loaded from `Data/waf.json` on first use).
pub static WAF: Lazy<Waf> = Lazy::new(|| Waf::open(store_path()));

/// Load the lists at startup and report what was loaded.
pub fn init() {
    let l = WAF.lists.read();
    crab_core::log::info(
        CAT,
        format!("{}: {} blacklist, {} whitelist, {} bans", WAF.path.display(), l.blacklist.len(), l.whitelist.len(), l.bans.len()),
    );
}

/// Drops expired entries / bans (persisting the change) and idle rate windows every minute.
pub fn spawn_maintenance(ct: CancellationToken) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = ct.cancelled() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}
            }
            if WAF.maintain(Utc::now()) {
                save_in_background();
            }
        }
    });
}

/// Persist the global lists off the async runtime threads.
pub fn save_in_background() {
    let run = || {
        if let Err(e) = WAF.save() {
            crab_core::log::error(CAT, format!("{}: {e}", WAF.path.display()));
        }
    };
    match tokio::runtime::Handle::try_current() {
        Ok(h) => {
            h.spawn_blocking(run);
        }
        Err(_) => run(),
    }
}

/// A blocked request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub reason: Reason,
    pub status: StatusCode,
    pub retry_after: Option<u64>,
    /// A new ban was added (the lists need saving).
    pub banned: bool,
}

#[derive(Default)]
struct UaCache {
    patterns: Vec<String>,
    set: Option<RegexSet>,
}

pub struct Waf {
    path: PathBuf,
    lists: RwLock<Lists>,
    save_lock: Mutex<()>,
    limiter: RateLimiter,
    pub stats: Stats,
    ua: RwLock<UaCache>,
}

fn minutes(m: i32) -> Duration {
    Duration::minutes(m.max(1) as i64)
}

/// Percent-decoded, lowercased path with repeated slashes collapsed (trap matching only).
fn trap_view(path: &str) -> String {
    let decoded = urlencoding::decode(path).map(|s| s.into_owned()).unwrap_or_else(|_| path.to_string());
    let mut out = String::with_capacity(decoded.len());
    for ch in decoded.chars() {
        if ch == '/' && out.ends_with('/') {
            continue;
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

pub fn trap_hit(traps: &[String], path: &str) -> bool {
    if traps.is_empty() {
        return false;
    }
    let p = trap_view(path);
    traps.iter().map(|t| t.trim()).filter(|t| !t.is_empty() && *t != "/").any(|t| {
        let t = t.to_ascii_lowercase();
        if t.starts_with('/') {
            p.starts_with(&t)
        } else {
            p.starts_with(&format!("/{t}"))
        }
    })
}

impl Waf {
    /// Instance backed by `path` (missing file = empty lists; a broken file is logged and ignored).
    pub fn open(path: PathBuf) -> Waf {
        let now = Utc::now();
        let lists = match store::load(&path) {
            Ok(f) => Lists::from_file(f, now),
            Err(e) => {
                crab_core::log::error(CAT, format!("{e}; starting with empty lists"));
                Lists::default()
            }
        };
        Waf {
            path,
            lists: RwLock::new(lists),
            save_lock: Mutex::new(()),
            limiter: RateLimiter::default(),
            stats: Stats::default(),
            ua: RwLock::new(UaCache::default()),
        }
    }

    /// Write the current lists (atomically). Concurrent calls are serialised and the last
    /// one writes the latest state.
    pub fn save(&self) -> std::io::Result<()> {
        let _g = self.save_lock.lock();
        let doc = self.lists.read().to_file();
        store::save(&self.path, &doc)
    }

    /// Periodic cleanup; true when the lists changed.
    pub fn maintain(&self, now: DateTime<Utc>) -> bool {
        self.limiter.prune(now.timestamp());
        let expired = self.lists.read().has_expired(now);
        expired && self.lists.write().prune(now)
    }

    fn ua_blocked(&self, patterns: &[String], ua: &str) -> bool {
        if patterns.is_empty() {
            return false;
        }
        {
            let c = self.ua.read();
            if c.patterns == patterns {
                return c.set.as_ref().map(|s| s.is_match(ua)).unwrap_or(false);
            }
        }
        let valid: Vec<&str> = patterns
            .iter()
            .map(|p| p.as_str())
            .filter(|p| !p.trim().is_empty() && RegexBuilder::new(p).case_insensitive(true).size_limit(1 << 20).build().is_ok())
            .collect();
        if valid.len() != patterns.len() {
            crab_core::log::warn(CAT, "waf.blockUserAgents: invalid or empty patterns are ignored");
        }
        let set = if valid.is_empty() {
            None
        } else {
            RegexSetBuilder::new(valid).case_insensitive(true).size_limit(1 << 22).build().ok()
        };
        let hit = set.as_ref().map(|s| s.is_match(ua)).unwrap_or(false);
        *self.ua.write() = UaCache { patterns: patterns.to_vec(), set };
        hit
    }

    fn auto_ban(&self, ip: IpAddr, reason: Reason, m: i32, now: DateTime<Utc>) -> bool {
        self.lists.write().ban(ip, reason.as_str(), now + minutes(m), now)
    }

    /// Run the filter chain for one request; `None` = allowed.
    pub fn evaluate(&self, cfg: &WafSettings, ip: Option<IpAddr>, path: &str, ua: &str, now: DateTime<Utc>) -> Option<Block> {
        let ip = unmap(ip?);
        if is_loopback(ip) || (cfg.whitelistLan && is_local_or_private(Some(ip))) {
            return None;
        }
        let forbidden = |reason| Block { reason, status: StatusCode::FORBIDDEN, retry_after: None, banned: false };
        {
            let l = self.lists.read();
            if l.in_list(ListKind::Whitelist, ip, now) {
                return None;
            }
            if l.in_list(ListKind::Blacklist, ip, now) {
                return Some(forbidden(Reason::Blacklist));
            }
            if l.ban_of(ip, now).is_some() {
                return Some(forbidden(Reason::Ban));
            }
        }
        if self.ua_blocked(&cfg.blockUserAgents, ua) {
            let banned = self.auto_ban(ip, Reason::Ua, cfg.rateLimit.banMinutes, now);
            return Some(Block { banned, ..forbidden(Reason::Ua) });
        }
        if trap_hit(&cfg.trapPaths, path) {
            let banned = self.auto_ban(ip, Reason::Trap, cfg.trapBanMinutes, now);
            return Some(Block { reason: Reason::Trap, status: StatusCode::NOT_FOUND, retry_after: None, banned });
        }
        if cfg.rateLimit.enable {
            let n = self.limiter.hit(ip, now.timestamp());
            if n > cfg.rateLimit.perMinute.max(1) as u32 {
                let banned = self.auto_ban(ip, Reason::Rate, cfg.rateLimit.banMinutes, now);
                return Some(Block {
                    reason: Reason::Rate,
                    status: StatusCode::TOO_MANY_REQUESTS,
                    retry_after: Some(minutes(cfg.rateLimit.banMinutes).num_seconds() as u64),
                    banned,
                });
            }
        }
        None
    }

    // --- list management (admin API) -------------------------------------------------

    pub fn upsert_rule(&self, kind: ListKind, net: IpNet, comment: String, expires: Option<DateTime<Utc>>, now: DateTime<Utc>) {
        self.lists.write().upsert(kind, net, comment, expires, now);
    }

    pub fn remove_rule(&self, kind: ListKind, net: IpNet) -> bool {
        self.lists.write().remove(kind, net)
    }

    pub fn ban(&self, ip: IpAddr, reason: &str, expires: DateTime<Utc>, now: DateTime<Utc>) {
        self.lists.write().set_ban(ip, reason, expires, now);
    }

    pub fn unban(&self, ip: IpAddr) -> bool {
        self.limiter.clear(unmap(ip));
        self.lists.write().unban(ip)
    }

    /// `(blacklist, whitelist, bans)` without expired entries.
    pub fn snapshot(&self, now: DateTime<Utc>) -> store::WafFile {
        let mut f = self.lists.read().to_file();
        f.blacklist.retain(|e| e.expires.map(|x| x > now).unwrap_or(true));
        f.whitelist.retain(|e| e.expires.map(|x| x > now).unwrap_or(true));
        f.bans.retain(|b| b.expires > now);
        f
    }

    /// `(state, banExpires)` of an address for the IP table.
    pub fn state_of(&self, cfg: &WafSettings, ip: IpAddr, now: DateTime<Utc>) -> (&'static str, Option<DateTime<Utc>>) {
        let l = self.lists.read();
        let ban = l.ban_of(ip, now).map(|b| b.expires);
        if is_loopback(ip) || (cfg.whitelistLan && is_local_or_private(Some(ip))) || l.in_list(ListKind::Whitelist, ip, now) {
            ("whitelisted", ban)
        } else if l.in_list(ListKind::Blacklist, ip, now) {
            ("blacklisted", ban)
        } else if ban.is_some() {
            ("banned", ban)
        } else {
            ("normal", None)
        }
    }
}

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

/// Real client IP (trusted proxy headers only from a loopback peer).
pub fn client_ip(req: &Request) -> Option<IpAddr> {
    let net = request_network(req);
    ClientNetworkContext::from_request(&net, req.headers()).client_ip.map(unmap)
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Path as recorded: no query string, bounded length, the admin token never stored.
pub fn record_path(path: &str, admin_token: &str) -> String {
    let p = truncate(path, MAX_PATH_LEN);
    if !admin_token.is_empty() && p.contains(admin_token) {
        p.replace(admin_token, "***")
    } else {
        p.to_string()
    }
}

fn blocked_response(b: &Block) -> Response {
    let text = match b.status {
        StatusCode::NOT_FOUND => "",
        StatusCode::TOO_MANY_REQUESTS => "Too Many Requests",
        _ => "Forbidden",
    };
    let mut r = (b.status, Body::from(text)).into_response();
    let h = r.headers_mut();
    if !text.is_empty() {
        h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(s) = b.retry_after {
        h.insert(header::RETRY_AFTER, HeaderValue::from(s));
    }
    r
}

struct Pending {
    entry: LogEntry,
    start: Instant,
    history: usize,
}

impl Pending {
    fn commit(mut self, status: u16) {
        self.entry.status = status;
        self.entry.ms = self.start.elapsed().as_millis() as u64;
        WAF.stats.record(self.entry, self.history);
    }
}

/// Records the request even when the response future is dropped (client went away → 499).
struct RecordGuard(Option<Pending>);

impl RecordGuard {
    fn finish(mut self, status: u16) {
        if let Some(p) = self.0.take() {
            p.commit(status);
        }
    }
}

impl Drop for RecordGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            p.commit(499);
        }
    }
}

pub async fn waf_mw(req: Request, next: Next) -> Response {
    let c = crate::conf();
    let cfg = &c.waf;
    if !cfg.enable && !cfg.logRequests {
        return next.run(req).await;
    }
    let start = Instant::now();
    let now = Utc::now();
    let ip = client_ip(&req);
    let ua = req.headers().get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let ua = truncate(ua, MAX_UA_LEN).to_string();

    let block = if cfg.enable { WAF.evaluate(cfg, ip, req.uri().path(), &ua, now) } else { None };
    if !cfg.logRequests && block.is_none() {
        return next.run(req).await;
    }
    let ip_text = ip.map(|i| i.to_string()).unwrap_or_else(|| "unknown".into());
    let entry = LogEntry {
        time: now,
        ip: ip_text,
        method: req.method().as_str().to_string(),
        path: record_path(req.uri().path(), &c.admin.token),
        status: 0,
        ms: 0,
        ua,
        blocked: block.map(|b| b.reason),
    };
    let history = cfg.historySize.clamp(0, crate::config_api::schema::MAX_WAF_HISTORY) as usize;

    if let Some(b) = block {
        crab_core::log::debug(
            CAT,
            format!("{} {} {} → {} ({})", entry.ip, entry.method, entry.path, b.status.as_u16(), b.reason.as_str()),
        );
        if b.banned {
            crab_core::log::debug(CAT, format!("{} banned ({})", entry.ip, b.reason.as_str()));
            save_in_background();
        }
        let resp = blocked_response(&b);
        if cfg.logRequests {
            Pending { entry, start, history }.commit(b.status.as_u16());
        }
        return resp;
    }

    let guard = RecordGuard(Some(Pending { entry, start, history }));
    match AssertUnwindSafe(next.run(req)).catch_unwind().await {
        Ok(resp) => {
            guard.finish(resp.status().as_u16());
            resp
        }
        Err(panic) => {
            guard.finish(500);
            std::panic::resume_unwind(panic)
        }
    }
}

#[cfg(test)]
mod tests;
