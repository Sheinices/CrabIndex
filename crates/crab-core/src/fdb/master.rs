//! masterDb: bucket key → last update time; persisted to `Data/masterDb.bz` with daily backups.

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::{jsonstream::*, OPEN_WRITE_TASK};
use crate::log::{self, cat};
use crate::models::{MasterDbShard, TorrentDetails};
use crate::{conf, time};

/// Global masterDb. Loaded lazily on first access.
pub static MASTER_DB: Lazy<DashMap<String, MasterDbShard>> = Lazy::new(load_master_db);

static DIRTY: AtomicBool = AtomicBool::new(false);

fn mark_dirty() {
    DIRTY.store(true, Ordering::SeqCst);
}

fn dated_name(days_back: i64) -> String {
    let d = chrono::Local::now().date_naive() - Duration::days(days_back);
    format!("Data/masterDb_{}.bz", d.format("%d-%m-%Y"))
}

fn load_master_db() -> DashMap<String, MasterDbShard> {
    let main = "Data/masterDb.bz";
    if std::path::Path::new(main).exists() {
        if let Some(m) = read_gz_json::<HashMap<String, MasterDbShard>>(main) {
            return m.into_iter().collect();
        }
    }
    for back in [0, 1] {
        let p = dated_name(back);
        if std::path::Path::new(&p).exists() {
            if let Some(m) = read_gz_json::<HashMap<String, MasterDbShard>>(&p) {
                return m.into_iter().collect();
            }
        }
    }
    // legacy format (before 29.08.2023): key → DateTime
    if std::path::Path::new(main).exists() {
        if let Some(old) = read_gz_json::<HashMap<String, serde_json::Value>>(main) {
            let map: DashMap<String, MasterDbShard> = DashMap::new();
            for (k, v) in old {
                if let Some(dt) = v.as_str().and_then(time::parse_net) {
                    map.insert(k, MasterDbShard { updateTime: dt, fileTime: time::to_file_time_utc(&dt) });
                }
            }
            if !map.is_empty() {
                let snapshot: HashMap<String, MasterDbShard> = map.iter().map(|e| (e.key().clone(), e.value().clone())).collect();
                write_gz_json(main, &snapshot);
                return map;
            }
        }
    }
    let _ = std::fs::remove_file("Data/temp/lastsync.txt");
    DashMap::new()
}

/// Force masterDb load (call at startup before serving requests).
pub fn init_master_db() -> usize {
    MASTER_DB.len()
}

pub fn master_db() -> &'static DashMap<String, MasterDbShard> {
    &MASTER_DB
}

/// Snapshot of masterDb as Vec (key, shard).
pub fn master_db_snapshot() -> Vec<(String, MasterDbShard)> {
    MASTER_DB.iter().map(|e| (e.key().clone(), e.value().clone())).collect()
}

/// The single write point for masterDb entries.
pub fn set_shard(key: &str, update_time: DateTime<Utc>) {
    if key.is_empty() {
        return;
    }
    MASTER_DB.insert(key.to_string(), MasterDbShard { updateTime: update_time, fileTime: time::to_file_time_utc(&update_time) });
    mark_dirty();
}

/// Insert a shard record verbatim (sync import keeps remote fileTime).
pub fn set_shard_raw(key: &str, shard: MasterDbShard) {
    if key.is_empty() {
        return;
    }
    MASTER_DB.insert(key.to_string(), shard);
    mark_dirty();
}

pub fn remove_key_from_master_db(key: &str) {
    if key.is_empty() {
        return;
    }
    if MASTER_DB.remove(key).is_some() {
        mark_dirty();
    }
}

pub(crate) fn add_or_update_master_db(t: &TorrentDetails) {
    let key = super::key_db(&t.name, &t.originalname);
    if let Some(info) = MASTER_DB.get(&key) {
        if t.updateTime <= info.updateTime {
            return;
        }
    }
    set_shard(&key, t.updateTime);
}

/// Persist masterDb + daily backup, drop the 3-days-old backup.
pub fn save_changes_to_file() {
    let snapshot: HashMap<String, MasterDbShard> = MASTER_DB.iter().map(|e| (e.key().clone(), e.value().clone())).collect();
    write_gz_json("Data/masterDb.bz", &snapshot);
    DIRTY.store(false, Ordering::SeqCst);
    let today = dated_name(0);
    if !std::path::Path::new(&today).exists() {
        let _ = std::fs::copy("Data/masterDb.bz", &today);
    }
    let old = dated_name(3);
    if std::path::Path::new(&old).exists() {
        let _ = std::fs::remove_file(old);
    }
}

pub fn save_changes_if_dirty() -> bool {
    if !DIRTY.load(Ordering::SeqCst) {
        return false;
    }
    save_changes_to_file();
    true
}

pub fn is_master_db_dirty() -> bool {
    DIRTY.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// Cron (cache eviction + periodic masterDb persistence)
// ---------------------------------------------------------------------------

fn try_evict_cache_entry(key: &str) -> bool {
    let removed = OPEN_WRITE_TASK.remove_if(key, |_, v| v.openconnection.load(Ordering::SeqCst) <= 0);
    if let Some((_, wtm)) = removed {
        wtm.db.save_changes_if_needed();
        return true;
    }
    false
}

fn warn_stuck_open_connections() {
    let now = time::now();
    let stuck: Vec<(String, i32, i64)> = OPEN_WRITE_TASK
        .iter()
        .filter(|e| e.openconnection.load(Ordering::SeqCst) > 0 && now > e.create + Duration::minutes(30))
        .take(5)
        .map(|e| (e.key().clone(), e.openconnection.load(Ordering::SeqCst), (now - e.create).num_minutes()))
        .collect();
    for (k, n, age) in stuck {
        log::warn(cat::FDB, format!("stuck openconnection={n} key={k} ageMin={age}"));
    }
}

/// Every 10 minutes: persist masterDb when dirty, evict stale cache entries.
pub async fn cron(shutdown: tokio_util::sync::CancellationToken) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(600)) => {}
        }
        if save_changes_if_dirty() {
            log::info(cat::FDB, format!("masterDb persisted ({} keys) / {}", MASTER_DB.len(), chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
        }
        let c = conf();
        if !c.evercache.enable || c.evercache.validHour <= 0 {
            continue;
        }
        warn_stuck_open_connections();
        let now = time::now();
        let valid = Duration::hours(c.evercache.validHour as i64);
        let keys: Vec<String> = OPEN_WRITE_TASK.iter().filter(|e| now > *e.lastread.lock() + valid).map(|e| e.key().clone()).collect();
        let evicted = keys.iter().filter(|k| try_evict_cache_entry(k)).count();
        if evicted > 0 {
            log::warn(cat::FDB, format!("evicted {evicted} cache entries (validHour) / {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
        }
    }
}

/// Every 20 seconds: drop least-read cache entries above maxOpenWriteTask.
pub async fn cron_fast(shutdown: tokio_util::sync::CancellationToken) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(20)) => {}
        }
        let c = conf();
        if !c.evercache.enable || c.evercache.validHour <= 0 {
            continue;
        }
        if OPEN_WRITE_TASK.len() as i32 > c.evercache.maxOpenWriteTask {
            let now = time::now();
            let mut cand: Vec<(String, i32, DateTime<Utc>)> = OPEN_WRITE_TASK
                .iter()
                .filter(|e| now > e.create + Duration::minutes(10))
                .map(|e| (e.key().clone(), e.countread.load(Ordering::Relaxed), *e.lastread.lock()))
                .collect();
            cand.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)));
            let dropped = cand.iter().take(c.evercache.dropCacheTake.max(0) as usize).filter(|x| try_evict_cache_entry(&x.0)).count();
            if dropped > 0 {
                log::warn(cat::FDB, format!("dropped {dropped} cache entries (maxOpenWriteTask) / {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
            }
        }
    }
}

/// Flush every dirty cached shard + masterDb (shutdown).
pub fn flush_all() {
    let entries: Vec<Arc<super::WriteTask>> = OPEN_WRITE_TASK.iter().map(|e| e.value().clone()).collect();
    for e in entries {
        e.db.save_changes_if_needed();
    }
    save_changes_if_dirty();
}
