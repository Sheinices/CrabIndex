//! Per-IP sliding-minute request counter (60 one-second buckets per address).

use dashmap::DashMap;
use std::net::IpAddr;

/// Upper bound on tracked addresses; idle windows are pruned first, then everything is reset.
const MAX_TRACKED: usize = 100_000;

struct Window {
    buckets: [u32; 60],
    /// Unix second of the most recent hit.
    last: i64,
}

impl Window {
    fn new(now: i64) -> Self {
        Window { buckets: [0; 60], last: now }
    }

    fn advance(&mut self, now: i64) {
        if now <= self.last {
            return;
        }
        if now - self.last >= 60 {
            self.buckets = [0; 60];
        } else {
            for s in self.last + 1..=now {
                self.buckets[s.rem_euclid(60) as usize] = 0;
            }
        }
        self.last = now;
    }

    fn total(&self) -> u32 {
        self.buckets.iter().fold(0u32, |a, b| a.saturating_add(*b))
    }
}

#[derive(Default)]
pub struct RateLimiter {
    map: DashMap<IpAddr, Window>,
}

impl RateLimiter {
    /// Count one request at unix second `now`; returns the requests within the last 60 seconds.
    pub fn hit(&self, ip: IpAddr, now: i64) -> u32 {
        if self.map.len() >= MAX_TRACKED && !self.map.contains_key(&ip) {
            self.prune(now);
            if self.map.len() >= MAX_TRACKED {
                self.map.clear();
            }
        }
        let mut w = self.map.entry(ip).or_insert_with(|| Window::new(now));
        w.advance(now);
        let slot = &mut w.buckets[now.rem_euclid(60) as usize];
        *slot = slot.saturating_add(1);
        w.total()
    }

    pub fn clear(&self, ip: IpAddr) {
        self.map.remove(&ip);
    }

    /// Forget addresses idle for a minute or more.
    pub fn prune(&self, now: i64) {
        self.map.retain(|_, w| now - w.last < 60);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliding_minute() {
        let l = RateLimiter::default();
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let t0 = 1_700_000_000;
        for i in 0..10 {
            assert_eq!(l.hit(ip, t0), i + 1);
        }
        assert_eq!(l.hit(ip, t0 + 30), 11);
        // t0 bucket (10 hits) slides out after 60 s
        assert_eq!(l.hit(ip, t0 + 60), 2);
        assert_eq!(l.hit(ip, t0 + 89), 3);
        assert_eq!(l.hit(ip, t0 + 91), 3);
        assert_eq!(l.hit(ip, t0 + 500), 1);
        l.prune(t0 + 1000);
        assert_eq!(l.len(), 0);
    }
}
