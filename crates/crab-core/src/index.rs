//! Fast search index: search-name token → bucket keys, built from masterDb.

use arc_swap::ArcSwapOption;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use crate::fdb::MASTER_DB;
use crate::log;

pub type FastDb = HashMap<String, Vec<String>>;

static FASTDB: Lazy<ArcSwapOption<FastDb>> = Lazy::new(|| ArcSwapOption::from(None));
static BUILD_LOCK: Mutex<()> = parking_lot::const_mutex(());

/// Current index (built on first use). `update=true` forces a rebuild.
pub fn get(update: bool) -> Arc<FastDb> {
    if !update {
        if let Some(f) = FASTDB.load_full() {
            return f;
        }
    }
    let _g = BUILD_LOCK.lock();
    if !update {
        if let Some(f) = FASTDB.load_full() {
            return f;
        }
    }
    if update {
        log::info("fastdb", format!("rebuild start / {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
    }
    let mut fastdb: FastDb = HashMap::new();
    for e in MASTER_DB.iter() {
        for k in e.key().split(':') {
            if k.is_empty() {
                continue;
            }
            fastdb.entry(k.to_string()).or_default().push(e.key().clone());
        }
    }
    let arc = Arc::new(fastdb);
    FASTDB.store(Some(arc.clone()));
    if update {
        log::info("fastdb", format!("rebuild end / {} keys={}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), arc.len()));
    }
    arc
}

/// Number of index keys when the index has been built (never triggers a build).
pub fn current_len() -> Option<usize> {
    FASTDB.load_full().map(|f| f.len())
}

pub fn rebuild() {
    get(true);
}
