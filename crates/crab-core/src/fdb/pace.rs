// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Disk pacing for full passes over the FileDB. A scan (id index, statistics, data check,
//! migrations, maintenance) reads every shard: hundreds of thousands of small files in a
//! row, which on a VPS with an IOPS cap can stall the whole machine. Threads that hold a
//! [`ScanGuard`] have their shard reads throttled to `scanReadsPerSecond` in total; search
//! requests and sync are never touched.

use std::cell::Cell;
use std::sync::Mutex;
use std::time::{Duration, Instant};

thread_local! {
    static IN_SCAN: Cell<u32> = const { Cell::new(0) };
}

/// Marks the current thread as running a scan until dropped.
pub struct ScanGuard(());

/// Enter scan mode on this thread: shard reads made while the guard lives are paced.
pub fn scan() -> ScanGuard {
    IN_SCAN.with(|c| c.set(c.get() + 1));
    ScanGuard(())
}

impl Drop for ScanGuard {
    fn drop(&mut self) {
        IN_SCAN.with(|c| c.set(c.get().saturating_sub(1)));
    }
}

pub fn in_scan() -> bool {
    IN_SCAN.with(|c| c.get() > 0)
}

struct Bucket {
    window: Instant,
    reads: u32,
}

static BUCKET: Mutex<Option<Bucket>> = Mutex::new(None);

/// Called before every shard read from disk. Outside a scan it returns at once; inside, all
/// scanning threads together stay under the configured reads per second (a 100 ms window).
pub fn before_read() {
    if !in_scan() {
        return;
    }
    let per_sec = crate::conf().scanReadsPerSecond;
    if per_sec <= 0 {
        return;
    }
    let slot = (per_sec as u32).div_ceil(10).max(1);
    loop {
        let wait = {
            let mut g = BUCKET.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let b = g.get_or_insert(Bucket { window: now, reads: 0 });
            if now.duration_since(b.window) >= Duration::from_millis(100) {
                b.window = now;
                b.reads = 0;
            }
            if b.reads < slot {
                b.reads += 1;
                None
            } else {
                Some(Duration::from_millis(100).saturating_sub(now.duration_since(b.window)))
            }
        };
        match wait {
            None => return,
            Some(d) => std::thread::sleep(d.max(Duration::from_millis(1))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{set_current, AppOptions};

    #[test]
    fn paced_only_inside_a_scan() {
        let mut c = AppOptions::default();
        c.scanReadsPerSecond = 100; // 10 per 100 ms window
        set_current(c);
        *BUCKET.lock().unwrap() = None;
        let t = Instant::now();
        for _ in 0..50 {
            before_read();
        }
        assert!(t.elapsed() < Duration::from_millis(20), "no scan: no pacing");
        let g = scan();
        assert!(in_scan());
        let t = Instant::now();
        for _ in 0..25 {
            before_read();
        }
        // 25 reads at 10 per 100 ms need at least two extra windows
        assert!(t.elapsed() >= Duration::from_millis(150), "paced: {:?}", t.elapsed());
        drop(g);
        assert!(!in_scan());
    }
}
