// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Per-host browser backoff. When most browser requests to a site keep failing (challenge not
//! solved, tabs crashing, timeouts), hammering FlareSolverr only burns CPU: the host goes into
//! backoff and browser requests are skipped for a while (the cffetch fast path still runs). The
//! pause doubles on every failed probe, up to `backoffMaxMinutes`, and one successful request
//! ends it. Decisions use a sliding window of recent outcomes, not the lifetime counters.

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::VecDeque;

/// Window the failure ratio is computed over.
const WINDOW_MINUTES: i64 = 30;
/// Fewer outcomes than this in the window: no decision.
const MIN_REQUESTS: usize = 8;
/// Backoff starts when at most this share of the window succeeded.
const MAX_OK_RATIO: f64 = 0.25;
const KEEP: usize = 200;

#[derive(Default)]
struct HostState {
    outcomes: VecDeque<(DateTime<Utc>, bool)>,
    /// Skip browser requests until then; `None` when not backing off.
    until: Option<DateTime<Utc>>,
    /// Doublings applied so far (0 = first pause).
    level: u32,
    since: Option<DateTime<Utc>>,
    /// True while the one probe allowed after `until` is in flight.
    probing: bool,
    entered: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackoffInfo {
    pub host: String,
    pub since: DateTime<Utc>,
    pub until: DateTime<Utc>,
    pub level: u32,
    /// Successful share of the last window when the pause was entered / extended.
    pub ok_ratio: f64,
    pub window_requests: usize,
    pub entered: u64,
}

static HOSTS: Lazy<DashMap<String, HostState>> = Lazy::new(DashMap::new);

fn settings() -> (bool, i64, i64) {
    let c = crab_core::conf();
    let f = &c.flaresolverr;
    (f.backoff, f.backoffMinutes.max(1) as i64, f.backoffMaxMinutes.max(f.backoffMinutes.max(1)) as i64)
}

fn window(st: &mut HostState, now: DateTime<Utc>) -> (usize, usize) {
    let cutoff = now - Duration::minutes(WINDOW_MINUTES);
    while st.outcomes.front().map(|(t, _)| *t < cutoff).unwrap_or(false) {
        st.outcomes.pop_front();
    }
    let ok = st.outcomes.iter().filter(|(_, ok)| *ok).count();
    (st.outcomes.len(), ok)
}

/// Should a browser request to `host` be skipped right now? `Some(until)` while backing off;
/// after `until` one probe is let through (the next call skips again until it reports).
pub fn skip(host: &str) -> Option<DateTime<Utc>> {
    let (enabled, _, _) = settings();
    if !enabled {
        return None;
    }
    let key = host.trim().to_lowercase();
    let mut st = HOSTS.entry(key).or_default();
    let until = st.until?;
    let now = Utc::now();
    if now < until {
        return Some(until);
    }
    if st.probing {
        return Some(until);
    }
    st.probing = true;
    None
}

/// Record a browser outcome (also the probe's); returns `true` when the host just entered or
/// extended a backoff, so the caller can log it once.
pub fn note(host: &str, ok: bool) -> bool {
    let (enabled, base_min, max_min) = settings();
    let key = host.trim().to_lowercase();
    if key.is_empty() {
        return false;
    }
    let now = Utc::now();
    let mut st = HOSTS.entry(key).or_default();
    st.outcomes.push_back((now, ok));
    while st.outcomes.len() > KEEP {
        st.outcomes.pop_front();
    }
    if !enabled {
        st.until = None;
        st.probing = false;
        return false;
    }
    if ok {
        // one good answer ends the pause and resets the ladder
        st.until = None;
        st.since = None;
        st.level = 0;
        st.probing = false;
        return false;
    }
    let was_probe = st.probing;
    st.probing = false;
    if st.until.is_some() && was_probe {
        // probe failed: double the pause
        st.level = (st.level + 1).min(16);
        let mins = (base_min << st.level).min(max_min);
        st.until = Some(now + Duration::minutes(mins));
        st.entered += 1;
        return true;
    }
    if st.until.is_some() {
        return false;
    }
    let (n, oks) = window(&mut st, now);
    if n >= MIN_REQUESTS && (oks as f64) / (n as f64) <= MAX_OK_RATIO {
        st.level = 0;
        st.until = Some(now + Duration::minutes(base_min.min(max_min)));
        st.since = Some(now);
        st.entered += 1;
        return true;
    }
    false
}

/// Hosts currently backing off (admin panel, health).
pub fn active() -> Vec<BackoffInfo> {
    let now = Utc::now();
    let mut out = Vec::new();
    for mut e in HOSTS.iter_mut() {
        let host = e.key().clone();
        let st = e.value_mut();
        let Some(until) = st.until else { continue };
        let (n, oks) = window(st, now);
        out.push(BackoffInfo {
            host,
            since: st.since.unwrap_or(until),
            until,
            level: st.level,
            ok_ratio: if n > 0 { (oks as f64 / n as f64 * 100.0).round() / 100.0 } else { 0.0 },
            window_requests: n,
            entered: st.entered,
        });
    }
    out.sort_by(|a, b| a.host.cmp(&b.host));
    out
}

/// End the pause for `host` now (panel button).
pub fn clear(host: &str) -> bool {
    let key = host.trim().to_lowercase();
    match HOSTS.get_mut(&key) {
        Some(mut st) if st.until.is_some() => {
            st.until = None;
            st.since = None;
            st.level = 0;
            st.probing = false;
            true
        }
        _ => false,
    }
}

pub fn reset() {
    HOSTS.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crab_core::config::{set_current, AppOptions};

    // the state is global: tests of this module run one at a time
    static SERIAL: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    fn configure(enabled: bool) {
        let mut c = AppOptions::default();
        c.flaresolverr.backoff = enabled;
        c.flaresolverr.backoffMinutes = 15;
        c.flaresolverr.backoffMaxMinutes = 120;
        set_current(c);
        reset();
    }

    #[test]
    fn enters_after_a_bad_window_and_doubles_on_failed_probes() {
        let _g = SERIAL.lock();
        configure(true);
        let h = "bad.test";
        for _ in 0..7 {
            assert!(!note(h, false));
        }
        assert!(skip(h).is_none(), "no decision below MIN_REQUESTS");
        assert!(note(h, false), "8th failure with 0% ok enters backoff");
        let until = skip(h).expect("backing off");
        assert!(until > Utc::now() + Duration::minutes(14));
        let a = active();
        assert_eq!((a.len(), a[0].level, a[0].window_requests), (1, 0, 8));

        // time passes: emulate by moving `until` into the past
        HOSTS.get_mut(h).unwrap().until = Some(Utc::now() - Duration::seconds(1));
        assert!(skip(h).is_none(), "one probe allowed");
        assert!(skip(h).is_some(), "second caller still waits while the probe is out");
        assert!(note(h, false), "failed probe extends");
        let a = active();
        assert_eq!(a[0].level, 1);
        assert!(a[0].until > Utc::now() + Duration::minutes(29));

        HOSTS.get_mut(h).unwrap().until = Some(Utc::now() - Duration::seconds(1));
        assert!(skip(h).is_none());
        assert!(!note(h, true), "a good probe ends the pause");
        assert!(skip(h).is_none());
        assert!(active().is_empty());
    }

    #[test]
    fn mostly_good_hosts_never_back_off_and_disable_switch_works() {
        let _g = SERIAL.lock();
        configure(true);
        for i in 0..20 {
            note("ok.test", i % 3 != 0);
        }
        assert!(skip("ok.test").is_none());
        configure(false);
        for _ in 0..20 {
            note("off.test", false);
        }
        assert!(skip("off.test").is_none());
        assert!(!clear("off.test"));
    }
}
