//! Per-host counters for the browser (FlareSolverr) and cffetch paths, plus a ring of
//! recent browser errors. In memory only, shown in the admin panel (`/cron/cloudflare/status`).

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;

const RECENT_MAX: usize = 200;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// Chromium tab killed, usually by the container memory limit.
    TabCrashed,
    /// Browser did not answer in time.
    BrowserTimeout,
    /// FlareSolverr could not pass the challenge.
    ChallengeFailed,
    /// Session missing / broken.
    SessionError,
    /// FlareSolverr unreachable or answered garbage.
    Unreachable,
    /// Page loaded but was not usable (non-200, interstitial, origin 503).
    PageFailed,
    Other,
}

/// Error class of a FlareSolverr message.
pub fn classify(message: &str) -> ErrorKind {
    let l = message.to_lowercase();
    if l.contains("tab crashed") {
        ErrorKind::TabCrashed
    } else if l.contains("read timed out") || l.contains("httpconnectionpool") || l.contains("timeout after") || l.contains("timed out") {
        ErrorKind::BrowserTimeout
    } else if l.contains("unreachable") || l.contains("empty response") {
        ErrorKind::Unreachable
    } else if l.contains("session") {
        ErrorKind::SessionError
    } else if l.contains("challenge") {
        ErrorKind::ChallengeFailed
    } else if l.starts_with("http ") || l.contains("origin 503") {
        ErrorKind::PageFailed
    } else {
        ErrorKind::Other
    }
}

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HostStats {
    pub host: String,
    pub browser_requests: u64,
    pub browser_ok: u64,
    pub browser_failed: u64,
    pub tab_crashed: u64,
    pub browser_timeouts: u64,
    pub challenge_failed: u64,
    pub session_errors: u64,
    pub unreachable: u64,
    pub page_failed: u64,
    pub other_errors: u64,
    pub sessions_created: u64,
    pub sessions_recycled: u64,
    pub sessions_closed_idle: u64,
    pub fast_ok: u64,
    pub fast_failed: u64,
    pub clearance_renewals: u64,
    pub total_browser_ms: u64,
    pub max_browser_ms: u64,
    pub last_ok_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEntry {
    pub at: DateTime<Utc>,
    pub host: String,
    pub kind: ErrorKind,
    pub message: String,
}

static HOSTS: Lazy<DashMap<String, HostStats>> = Lazy::new(DashMap::new);
static RECENT: Lazy<Mutex<VecDeque<ErrorEntry>>> = Lazy::new(|| Mutex::new(VecDeque::with_capacity(RECENT_MAX)));
static SINCE: Lazy<Mutex<DateTime<Utc>>> = Lazy::new(|| Mutex::new(Utc::now()));

fn with(host: &str, f: impl FnOnce(&mut HostStats)) {
    let key = host.trim().to_lowercase();
    if key.is_empty() {
        return;
    }
    let mut e = HOSTS.entry(key.clone()).or_insert_with(|| HostStats { host: key, ..Default::default() });
    f(e.value_mut());
}

fn short(message: &str) -> String {
    let one_line = message.replace("\\n", " ").replace('\n', " ");
    let t = one_line.trim();
    if t.chars().count() > 300 {
        format!("{}…", t.chars().take(300).collect::<String>())
    } else {
        t.to_string()
    }
}

/// One browser request finished (`error` = FlareSolverr message / failure reason).
pub fn browser(host: &str, ms: u64, error: Option<&str>) {
    let now = Utc::now();
    with(host, |s| {
        s.browser_requests += 1;
        s.total_browser_ms += ms;
        s.max_browser_ms = s.max_browser_ms.max(ms);
        match error {
            None => {
                s.browser_ok += 1;
                s.last_ok_at = Some(now);
            }
            Some(msg) => {
                s.browser_failed += 1;
                match classify(msg) {
                    ErrorKind::TabCrashed => s.tab_crashed += 1,
                    ErrorKind::BrowserTimeout => s.browser_timeouts += 1,
                    ErrorKind::ChallengeFailed => s.challenge_failed += 1,
                    ErrorKind::SessionError => s.session_errors += 1,
                    ErrorKind::Unreachable => s.unreachable += 1,
                    ErrorKind::PageFailed => s.page_failed += 1,
                    ErrorKind::Other => s.other_errors += 1,
                }
                s.last_error_at = Some(now);
                s.last_error = Some(short(msg));
            }
        }
    });
    if let Some(msg) = error {
        let mut r = RECENT.lock();
        if r.len() >= RECENT_MAX {
            r.pop_front();
        }
        r.push_back(ErrorEntry { at: now, host: host.to_lowercase(), kind: classify(msg), message: short(msg) });
    }
}

pub fn session_created(host: &str) {
    with(host, |s| s.sessions_created += 1);
}

pub fn session_recycled(host: &str) {
    with(host, |s| s.sessions_recycled += 1);
}

pub fn session_closed_idle(host: &str) {
    with(host, |s| s.sessions_closed_idle += 1);
}

pub fn fast(host: &str, ok: bool) {
    with(host, |s| if ok { s.fast_ok += 1 } else { s.fast_failed += 1 });
}

pub fn clearance_renewal(host: &str) {
    with(host, |s| s.clearance_renewals += 1);
}

/// Hosts sorted by browser failures, then requests.
pub fn hosts() -> Vec<HostStats> {
    let mut v: Vec<HostStats> = HOSTS.iter().map(|e| e.value().clone()).collect();
    v.sort_by(|a, b| b.browser_failed.cmp(&a.browser_failed).then(b.browser_requests.cmp(&a.browser_requests)).then(a.host.cmp(&b.host)));
    v
}

/// Newest first.
pub fn recent_errors(limit: usize) -> Vec<ErrorEntry> {
    RECENT.lock().iter().rev().take(limit).cloned().collect()
}

pub fn since() -> DateTime<Utc> {
    *SINCE.lock()
}

pub fn reset() {
    HOSTS.clear();
    RECENT.lock().clear();
    *SINCE.lock() = Utc::now();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_known_messages() {
        assert_eq!(classify("Error: Error solving the challenge. Message: tab crashed\\n  (Session info: chrome=152)"), ErrorKind::TabCrashed);
        assert_eq!(classify("HTTPConnectionPool(host='localhost', port=1): Read timed out. (read timeout=120)"), ErrorKind::BrowserTimeout);
        assert_eq!(classify("Error: Session not found"), ErrorKind::SessionError);
        assert_eq!(classify("empty response / unreachable"), ErrorKind::Unreachable);
        assert_eq!(classify("Error solving the challenge. Timeout after 60.0 seconds."), ErrorKind::BrowserTimeout);
        assert_eq!(classify("Error: Error solving the challenge."), ErrorKind::ChallengeFailed);
        assert_eq!(classify("http 403"), ErrorKind::PageFailed);
    }

    #[test]
    fn counters_and_ring() {
        reset();
        browser("Example.org", 1200, None);
        browser("example.org", 800, Some("Message: tab crashed"));
        fast("example.org", true);
        let h = hosts().into_iter().find(|h| h.host == "example.org").unwrap();
        assert_eq!((h.browser_requests, h.browser_ok, h.browser_failed, h.tab_crashed, h.fast_ok), (2, 1, 1, 1, 1));
        assert_eq!(h.max_browser_ms, 1200);
        let r = recent_errors(10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, ErrorKind::TabCrashed);
    }
}
