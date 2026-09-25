//! In-memory request statistics: ring buffer log, per-IP / per-path aggregates and a
//! per-minute timeline covering the last 24 hours. All structures are bounded.

use chrono::{DateTime, TimeZone, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use super::store::iso;

/// Tracked client addresses (least recently seen are evicted beyond this).
pub const MAX_IPS: usize = 10_000;
/// Tracked paths (least recently seen are evicted beyond this).
pub const MAX_PATHS: usize = 5_000;
/// Eviction runs once the map exceeds the cap by this many entries.
const EVICT_SLACK: usize = 500;
/// Timeline length in minutes (24 h).
pub const TIMELINE_MINUTES: i64 = 1440;
const TOP_N: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Blacklist,
    Ban,
    Ua,
    Trap,
    Rate,
}

impl Reason {
    pub const ALL: [Reason; 5] = [Reason::Blacklist, Reason::Ban, Reason::Ua, Reason::Trap, Reason::Rate];

    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Blacklist => "blacklist",
            Reason::Ban => "ban",
            Reason::Ua => "ua",
            Reason::Trap => "trap",
            Reason::Rate => "rate",
        }
    }

    pub fn parse(s: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.as_str().eq_ignore_ascii_case(s.trim()))
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One recorded request.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub time: DateTime<Utc>,
    pub ip: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub ms: u64,
    pub ua: String,
    pub blocked: Option<Reason>,
}

impl LogEntry {
    pub fn to_json(&self) -> Value {
        json!({
            "time": self.time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "ip": self.ip,
            "method": self.method,
            "path": self.path,
            "status": self.status,
            "ms": self.ms,
            "ua": self.ua,
            "blocked": self.blocked.map(Reason::as_str),
        })
    }
}

#[derive(Clone, Debug)]
pub struct IpAgg {
    pub requests: u64,
    pub blocked: u64,
    pub errors: u64,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub last_path: String,
    pub ua: String,
}

#[derive(Clone, Debug)]
struct PathAgg {
    requests: u64,
    errors: u64,
    last_seen: DateTime<Utc>,
}

#[derive(Clone, Copy, Default)]
struct Slot {
    minute: i64,
    requests: u64,
    blocked: u64,
    /// 2xx, 3xx, 4xx, 5xx
    status: [u64; 4],
    reasons: [u64; 5],
}

/// Filters of `GET waf/requests`.
#[derive(Default, Debug)]
pub struct RequestFilter {
    pub ip: Option<String>,
    pub path: Option<String>,
    /// Exact code or class (`4xx`).
    pub status: Option<String>,
    /// `true`/`false` or a reason name.
    pub blocked: Option<String>,
    pub limit: usize,
}

impl RequestFilter {
    fn matches(&self, e: &LogEntry) -> bool {
        if let Some(ip) = self.ip.as_deref().filter(|s| !s.is_empty()) {
            let ok = match super::net::IpNet::parse(ip) {
                Some(net) => e.ip.parse().map(|a| net.contains(a)).unwrap_or(false),
                None => e.ip.contains(ip),
            };
            if !ok {
                return false;
            }
        }
        if let Some(p) = self.path.as_deref().filter(|s| !s.is_empty()) {
            if !e.path.to_ascii_lowercase().contains(&p.to_ascii_lowercase()) {
                return false;
            }
        }
        if let Some(s) = self.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let lower = s.to_ascii_lowercase();
            let ok = if lower.len() == 3 && lower.ends_with("xx") {
                lower.as_bytes()[0].is_ascii_digit() && (e.status / 100) as u8 == lower.as_bytes()[0] - b'0'
            } else {
                s.parse::<u16>().map(|c| c == e.status).unwrap_or(false)
            };
            if !ok {
                return false;
            }
        }
        if let Some(b) = self.blocked.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let ok = match b.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "any" => e.blocked.is_some(),
                "false" | "0" | "no" | "none" => e.blocked.is_none(),
                other => Reason::parse(other).map(|r| e.blocked == Some(r)).unwrap_or(false),
            };
            if !ok {
                return false;
            }
        }
        true
    }
}

pub struct Stats {
    since: Mutex<DateTime<Utc>>,
    log: Mutex<VecDeque<LogEntry>>,
    ips: DashMap<String, IpAgg>,
    paths: DashMap<String, PathAgg>,
    timeline: Mutex<Vec<Slot>>,
    evicting: AtomicBool,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            since: Mutex::new(Utc::now()),
            log: Mutex::new(VecDeque::new()),
            ips: DashMap::new(),
            paths: DashMap::new(),
            timeline: Mutex::new(vec![Slot::default(); TIMELINE_MINUTES as usize]),
            evicting: AtomicBool::new(false),
        }
    }
}

fn status_class(status: u16) -> Option<usize> {
    match status {
        200..=299 => Some(0),
        300..=399 => Some(1),
        400..=499 => Some(2),
        500..=599 => Some(3),
        _ => None,
    }
}

fn minute_of(t: DateTime<Utc>) -> i64 {
    t.timestamp().div_euclid(60)
}

fn minute_iso(minute: i64) -> String {
    Utc.timestamp_opt(minute * 60, 0).single().map(|t| iso(&t)).unwrap_or_default()
}

impl Stats {
    pub fn since(&self) -> DateTime<Utc> {
        *self.since.lock()
    }

    /// Drop everything and restart the statistics clock.
    pub fn reset(&self) {
        self.log.lock().clear();
        self.ips.clear();
        self.paths.clear();
        self.timeline.lock().iter_mut().for_each(|s| *s = Slot::default());
        *self.since.lock() = Utc::now();
    }

    pub fn record(&self, e: LogEntry, history: usize) {
        let error = e.status >= 400;
        let blocked = e.blocked.is_some();
        {
            let minute = minute_of(e.time);
            let mut tl = self.timeline.lock();
            let slot = &mut tl[minute.rem_euclid(TIMELINE_MINUTES) as usize];
            if slot.minute != minute {
                *slot = Slot { minute, ..Slot::default() };
            }
            slot.requests += 1;
            if let Some(r) = e.blocked {
                slot.blocked += 1;
                slot.reasons[r.index()] += 1;
            }
            if let Some(c) = status_class(e.status) {
                slot.status[c] += 1;
            }
        }
        {
            let mut a = self.ips.entry(e.ip.clone()).or_insert_with(|| IpAgg {
                requests: 0,
                blocked: 0,
                errors: 0,
                first_seen: e.time,
                last_seen: e.time,
                last_path: String::new(),
                ua: String::new(),
            });
            a.requests += 1;
            a.blocked += blocked as u64;
            a.errors += error as u64;
            a.last_seen = a.last_seen.max(e.time);
            a.last_path.clone_from(&e.path);
            if !e.ua.is_empty() {
                a.ua.clone_from(&e.ua);
            }
        }
        {
            let mut p = self.paths.entry(e.path.clone()).or_insert_with(|| PathAgg { requests: 0, errors: 0, last_seen: e.time });
            p.requests += 1;
            p.errors += error as u64;
            p.last_seen = p.last_seen.max(e.time);
        }
        if self.ips.len() > MAX_IPS + EVICT_SLACK || self.paths.len() > MAX_PATHS + EVICT_SLACK {
            self.evict();
        }
        let mut log = self.log.lock();
        if history == 0 {
            log.clear();
            return;
        }
        while log.len() >= history {
            log.pop_front();
        }
        log.push_back(e);
    }

    /// Trim the aggregates back to their caps, least recently seen first (one thread at a time).
    fn evict(&self) {
        if self.evicting.swap(true, Ordering::AcqRel) {
            return;
        }
        if self.ips.len() > MAX_IPS {
            let mut v: Vec<(DateTime<Utc>, String)> = self.ips.iter().map(|r| (r.value().last_seen, r.key().clone())).collect();
            v.sort();
            let excess = v.len().saturating_sub(MAX_IPS);
            for (_, k) in v.into_iter().take(excess) {
                self.ips.remove(&k);
            }
        }
        if self.paths.len() > MAX_PATHS {
            let mut v: Vec<(DateTime<Utc>, String)> = self.paths.iter().map(|r| (r.value().last_seen, r.key().clone())).collect();
            v.sort();
            let excess = v.len().saturating_sub(MAX_PATHS);
            for (_, k) in v.into_iter().take(excess) {
                self.paths.remove(&k);
            }
        }
        self.evicting.store(false, Ordering::Release);
    }

    /// Newest first.
    pub fn requests(&self, f: &RequestFilter) -> Vec<Value> {
        let log = self.log.lock();
        log.iter().rev().filter(|e| f.matches(e)).take(f.limit).map(LogEntry::to_json).collect()
    }

    pub fn ip_aggregates(&self) -> Vec<(String, IpAgg)> {
        self.ips.iter().map(|r| (r.key().clone(), r.value().clone())).collect()
    }

    /// `GET waf/overview` payload (without `enabled`).
    pub fn overview(&self, window_minutes: i64, now: DateTime<Utc>) -> Value {
        let since = self.since();
        let now_min = minute_of(now);
        let first_min = now_min - window_minutes + 1;
        let start = Utc.timestamp_opt(first_min * 60, 0).single().unwrap_or(since);
        let step: i64 = if window_minutes > 60 { 10 } else { 1 };

        let mut requests = 0u64;
        let mut blocked = 0u64;
        let mut status = [0u64; 4];
        let mut reasons = [0u64; 5];
        let buckets = ((window_minutes + step - 1) / step) as usize;
        let bucket0 = (now_min.div_euclid(step) - buckets as i64 + 1) * step;
        let mut timeline: Vec<(i64, u64, u64)> = (0..buckets).map(|i| (bucket0 + i as i64 * step, 0, 0)).collect();
        {
            let tl = self.timeline.lock();
            for m in bucket0.min(first_min)..=now_min {
                let s = &tl[m.rem_euclid(TIMELINE_MINUTES) as usize];
                if s.minute != m || s.requests == 0 {
                    continue;
                }
                if m >= first_min {
                    requests += s.requests;
                    blocked += s.blocked;
                    for i in 0..4 {
                        status[i] += s.status[i];
                    }
                    for i in 0..5 {
                        reasons[i] += s.reasons[i];
                    }
                }
                let bi = (m - bucket0).div_euclid(step);
                if (0..buckets as i64).contains(&bi) {
                    let b = &mut timeline[bi as usize];
                    b.1 += s.requests;
                    b.2 += s.blocked;
                }
            }
        }

        let mut top_ips: Vec<(String, IpAgg)> = self.ips.iter().filter(|r| r.value().last_seen >= start).map(|r| (r.key().clone(), r.value().clone())).collect();
        let unique_ips = top_ips.len();
        top_ips.sort_by(|a, b| b.1.requests.cmp(&a.1.requests).then_with(|| b.1.last_seen.cmp(&a.1.last_seen)));
        top_ips.truncate(TOP_N);
        let mut top_paths: Vec<(String, PathAgg)> = self.paths.iter().filter(|r| r.value().last_seen >= start).map(|r| (r.key().clone(), r.value().clone())).collect();
        top_paths.sort_by(|a, b| b.1.requests.cmp(&a.1.requests).then_with(|| a.0.cmp(&b.0)));
        top_paths.truncate(TOP_N);

        let span = (now - since.max(start)).num_milliseconds().max(1000) as f64 / 1000.0;
        let span = span.min(window_minutes as f64 * 60.0);
        let rps = (requests as f64 / span * 100.0).round() / 100.0;
        let mut by_reason = serde_json::Map::new();
        for r in Reason::ALL {
            by_reason.insert(r.as_str().into(), json!(reasons[r.index()]));
        }
        json!({
            "since": iso(&since),
            "totals": { "requests": requests, "blocked": blocked, "uniqueIps": unique_ips, "rps": rps },
            "statusCodes": { "2xx": status[0], "3xx": status[1], "4xx": status[2], "5xx": status[3] },
            "blockedByReason": by_reason,
            "timeline": timeline.iter().map(|(m, r, b)| json!({ "t": minute_iso(*m), "requests": r, "blocked": b })).collect::<Vec<_>>(),
            "topIps": top_ips.iter().map(|(ip, a)| json!({ "ip": ip, "requests": a.requests, "blocked": a.blocked, "lastSeen": iso(&a.last_seen) })).collect::<Vec<_>>(),
            "topPaths": top_paths.iter().map(|(p, a)| json!({ "path": p, "requests": a.requests, "errors": a.errors })).collect::<Vec<_>>(),
        })
    }

    #[cfg(test)]
    pub fn tracked(&self) -> (usize, usize, usize) {
        (self.ips.len(), self.paths.len(), self.log.lock().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(t: DateTime<Utc>, ip: &str, path: &str, status: u16, blocked: Option<Reason>) -> LogEntry {
        LogEntry { time: t, ip: ip.into(), method: "GET".into(), path: path.into(), status, ms: 1, ua: "ua".into(), blocked }
    }

    #[test]
    fn ring_buffer_and_filters() {
        let s = Stats::default();
        let now = Utc::now();
        for i in 0..10u16 {
            s.record(entry(now, &format!("192.0.2.{i}"), &format!("/p{i}"), 200 + i * 30, None), 5);
        }
        s.record(entry(now, "2001:db8::1", "/.env", 404, Some(Reason::Trap)), 5);
        assert_eq!(s.tracked().2, 5);
        let all = s.requests(&RequestFilter { limit: 100, ..Default::default() });
        assert_eq!(all.len(), 5);
        assert_eq!(all[0]["path"], "/.env");
        assert_eq!(all[0]["blocked"], "trap");
        let f = |ip: Option<&str>, path: Option<&str>, status: Option<&str>, blocked: Option<&str>| {
            s.requests(&RequestFilter {
                ip: ip.map(Into::into),
                path: path.map(Into::into),
                status: status.map(Into::into),
                blocked: blocked.map(Into::into),
                limit: 100,
            })
            .len()
        };
        assert_eq!(f(Some("2001:db8::/32"), None, None, None), 1);
        assert_eq!(f(Some("192.0.2"), None, None, None), 4);
        assert_eq!(f(None, Some("P9"), None, None), 1);
        assert_eq!(f(None, None, Some("4xx"), None), 4); // 410, 440, 470, 404
        assert_eq!(f(None, None, Some("470"), None), 1);
        assert_eq!(f(None, None, None, Some("true")), 1);
        assert_eq!(f(None, None, None, Some("false")), 4);
        assert_eq!(f(None, None, None, Some("rate")), 0);
        assert_eq!(s.requests(&RequestFilter { limit: 2, ..Default::default() }).len(), 2);
    }

    #[test]
    fn overview_windows() {
        let s = Stats::default();
        let now = Utc::now();
        s.record(entry(now, "192.0.2.1", "/a", 200, None), 100);
        s.record(entry(now, "192.0.2.1", "/a", 500, None), 100);
        s.record(entry(now, "192.0.2.2", "/.env", 404, Some(Reason::Trap)), 100);
        s.record(entry(now - chrono::Duration::minutes(90), "192.0.2.3", "/old", 200, None), 100);
        s.record(entry(now - chrono::Duration::hours(30), "192.0.2.4", "/ancient", 200, None), 100);

        let o = s.overview(60, now);
        assert_eq!(o["totals"]["requests"], 3);
        assert_eq!(o["totals"]["blocked"], 1);
        assert_eq!(o["totals"]["uniqueIps"], 2);
        assert_eq!(o["statusCodes"]["5xx"], 1);
        assert_eq!(o["statusCodes"]["4xx"], 1);
        assert_eq!(o["blockedByReason"]["trap"], 1);
        assert_eq!(o["blockedByReason"]["rate"], 0);
        let tl = o["timeline"].as_array().unwrap();
        assert_eq!(tl.len(), 60);
        assert_eq!(tl[59]["requests"], 3);
        assert_eq!(o["topIps"][0]["ip"], "192.0.2.1");
        assert_eq!(o["topPaths"][0]["path"], "/a");
        assert_eq!(o["topPaths"][0]["errors"], 1);

        let o = s.overview(1440, now);
        assert_eq!(o["totals"]["requests"], 4);
        let tl = o["timeline"].as_array().unwrap();
        assert_eq!(tl.len(), 144);
        assert_eq!(tl.iter().map(|b| b["requests"].as_u64().unwrap()).sum::<u64>(), 4);
        assert!(tl[143]["t"].as_str().unwrap().ends_with('Z'));

        s.reset();
        assert_eq!(s.overview(60, now)["totals"]["requests"], 0);
        assert_eq!(s.tracked(), (0, 0, 0));
    }

    #[test]
    fn aggregates_are_bounded() {
        let s = Stats::default();
        let now = Utc::now();
        for i in 0..(MAX_IPS + EVICT_SLACK + 10) {
            let t = now + chrono::Duration::milliseconds(i as i64);
            s.record(entry(t, &format!("ip{i}"), &format!("/x/{i}"), 200, None), 10);
        }
        let (ips, paths, log) = s.tracked();
        assert!(ips <= MAX_IPS + EVICT_SLACK, "{ips}");
        assert!(paths <= MAX_PATHS + EVICT_SLACK, "{paths}");
        assert_eq!(log, 10);
        // the most recent survives, the oldest is gone
        assert!(s.ips.contains_key(&format!("ip{}", MAX_IPS + EVICT_SLACK + 9)));
        assert!(!s.ips.contains_key("ip0"));
    }
}
