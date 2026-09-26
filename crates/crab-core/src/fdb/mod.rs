//! FileDB - gzip JSON shards under `Data/fdb/` keyed by `search_name:search_originalname`,
//! plus the in-memory `masterDb` index persisted to `Data/masterDb.bz`.
//!
//! Shards are gzip'd JSON; an existing `Data/` directory in this format is used as-is.
//!
//! Usage from parsers:
//! ```ignore
//! crab_core::fdb::add_or_update(&torrents);                       // bulk upsert
//! crab_core::fdb::add_or_update_async(items, fdb::by_url, |t, cached| async { Some(t) }).await
//! let mut w = crab_core::fdb::open_write(&key); w.add_or_update(&t); // explicit shard
//! ```

mod details;
mod jsonstream;
mod master;
mod url_ids;

pub use details::{all_voices, rus_voices, ukr_voices, update_full_details};
pub use jsonstream::{read_gz_json, write_gz_json};
pub use master::*;
pub use url_ids::{register_id_extractor, torrent_id_from_url, IdExtractor};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::future::Future;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use crate::log::{self, cat};
use crate::models::TorrentDetails;
use crate::util::{is_blank, search_name, search_name_or_empty};
use crate::{conf, time};

/// Shard contents: url → row (insertion ordered).
pub type ShardMap = IndexMap<String, TorrentDetails>;

/// One opened shard.
pub struct FileDb {
    key: String,
    inner: Mutex<ShardState>,
}

struct ShardState {
    db: ShardMap,
    savechanges: bool,
}

/// Open-shard cache entry.
pub struct WriteTask {
    pub db: Arc<FileDb>,
    pub lastread: Mutex<DateTime<Utc>>,
    pub create: DateTime<Utc>,
    pub countread: AtomicI32,
    pub openconnection: AtomicI32,
}

pub(crate) static OPEN_WRITE_TASK: Lazy<DashMap<String, Arc<WriteTask>>> = Lazy::new(DashMap::new);

/// Read a shard file, skipping `null` or malformed rows instead of failing the whole shard.
pub fn read_shard(path: &str) -> Option<ShardMap> {
    if let Some(m) = read_gz_json::<IndexMap<String, Option<TorrentDetails>>>(path) {
        return Some(m.into_iter().filter_map(|(k, v)| v.map(|t| (k, t))).collect());
    }
    let raw = read_gz_json::<IndexMap<String, serde_json::Value>>(path)?;
    Some(raw.into_iter().filter(|(_, v)| v.is_object()).filter_map(|(k, v)| serde_json::from_value::<TorrentDetails>(v).ok().map(|t| (k, t))).collect())
}

/// `$"Data/fdb/{md5[0..2]}/{md5[2..]}"` (fdbPathLevels=2) or `Data/fdb/{md5[0]}/{md5}`.
pub fn path_db(key: &str) -> String {
    let md5key = crate::util::md5(key);
    if conf().fdbPathLevels == 2 {
        let dir = format!("Data/fdb/{}", &md5key[..2]);
        let _ = std::fs::create_dir_all(&dir);
        format!("{dir}/{}", &md5key[2..])
    } else {
        let dir = format!("Data/fdb/{}", &md5key[..1]);
        let _ = std::fs::create_dir_all(&dir);
        format!("{dir}/{md5key}")
    }
}

/// Bucket key for name/originalname.
pub fn key_db(name: &str, originalname: &str) -> String {
    let mut sn = search_name_or_empty(name);
    let mut so = search_name_or_empty(originalname);
    if sn.is_empty() {
        sn = so.clone();
    }
    if so.is_empty() {
        so = sn.clone();
    }
    format!("{sn}:{so}")
}

pub fn key_for_torrent(name: &str, originalname: &str) -> String {
    key_db(name, originalname)
}

pub fn path_for_key(key: &str) -> String {
    path_db(key)
}

impl FileDb {
    fn load(key: &str) -> FileDb {
        let path = path_db(key);
        let db = if std::path::Path::new(&path).exists() {
            read_shard(&path).unwrap_or_default()
        } else {
            ShardMap::new()
        };
        FileDb { key: key.to_string(), inner: Mutex::new(ShardState { db, savechanges: false }) }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// Copy of the shard (never expose the live map to concurrent readers).
    pub fn snapshot(&self) -> ShardMap {
        self.inner.lock().db.clone()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().db.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Run a closure with mutable access to the live map. Call [`FileDb::mark_changed`]
    /// (or return true) to persist.
    pub fn with_db<R>(&self, f: impl FnOnce(&mut ShardMap) -> R) -> R {
        let mut g = self.inner.lock();
        f(&mut g.db)
    }

    /// Like `with_db` but marks the shard dirty when the closure returns true.
    pub fn modify(&self, f: impl FnOnce(&mut ShardMap) -> bool) -> bool {
        let mut g = self.inner.lock();
        let changed = f(&mut g.db);
        if changed {
            g.savechanges = true;
        }
        changed
    }

    pub fn mark_changed(&self) {
        self.inner.lock().savechanges = true;
    }

    pub fn save_changes_if_needed(&self) {
        let g = self.inner.lock();
        if !g.db.is_empty() && g.savechanges {
            write_gz_json(&path_db(&self.key), &g.db);
        }
    }

    /// Persist unconditionally (also writes an empty shard - used by maintenance).
    pub fn save_now(&self) {
        let mut g = self.inner.lock();
        write_gz_json(&path_db(&self.key), &g.db);
        g.savechanges = false;
    }

    pub fn add_or_update(&self, torrent: &TorrentDetails) {
        let mut migrate: Option<(TorrentDetails, String, bool)> = None;
        {
            let mut g = self.inner.lock();
            add_or_update_core(&self.key, &mut g, torrent, &mut migrate);
        }
        if let Some((t, new_key, now_empty)) = migrate {
            migrate_torrent_to_new_key(&t, &new_key);
            if now_empty {
                remove_key_from_master_db(&self.key);
            }
        }
    }
}

fn upt(t: &mut TorrentDetails, st: &mut ShardState, update_full: &mut bool, uptfull: bool, updatetime: bool) {
    st.savechanges = true;
    if updatetime {
        t.updateTime = time::now();
        t.ffprobe_tryingdata = 0;
    }
    if uptfull {
        *update_full = true;
    }
}

fn is_lostfilm(t: &str) -> bool {
    t.eq_ignore_ascii_case("lostfilm")
}

fn drop_bare_lostfilm(st: &mut ShardState, t: &TorrentDetails) {
    if is_lostfilm(&t.trackerName) {
        if let Some(i) = t.url.find('#') {
            let bare = &t.url[..i];
            if !bare.is_empty() && st.db.shift_remove(bare).is_some() {
                st.savechanges = true;
            }
        }
    }
}

fn add_or_update_core(
    fdbkey: &str,
    st: &mut ShardState,
    torrent: &TorrentDetails,
    migrate: &mut Option<(TorrentDetails, String, bool)>,
) {
    let mut found_by_id = false;
    let mut existing: Option<TorrentDetails> = st.db.get(&torrent.url).cloned();

    if existing.is_none() {
        let torrent_id = torrent_id_from_url(&torrent.trackerName, &torrent.url);
        if torrent_id > 0 {
            let hit = st
                .db
                .iter()
                .filter(|(_, v)| v.trackerName.eq_ignore_ascii_case(&torrent.trackerName))
                .find(|(k, _)| torrent_id_from_url(&torrent.trackerName, k) == torrent_id)
                .map(|(k, _)| k.clone());
            if let Some(k) = hit {
                if let Some(mut t) = st.db.shift_remove(&k) {
                    t.url = torrent.url.clone();
                    existing = Some(t);
                    found_by_id = true;
                }
            }
        }
    }

    let c = conf();

    if let Some(mut t) = existing {
        let mut update_full = false;

        // types
        if !torrent.types.is_empty() {
            if t.types.is_empty() {
                t.types = torrent.types.clone();
                upt(&mut t, st, &mut update_full, true, true);
            } else {
                let added = torrent.types.iter().any(|ty| !ty.is_empty() && !t.types.contains(ty));
                if added {
                    upt(&mut t, st, &mut update_full, true, true);
                }
                t.types = torrent.types.clone();
            }
        }

        if torrent.trackerName != t.trackerName {
            t.trackerName = torrent.trackerName.clone();
            upt(&mut t, st, &mut update_full, true, true);
        }

        if torrent.title != t.title {
            t.title = torrent.title.clone();
            upt(&mut t, st, &mut update_full, true, true);
        }

        if !time::is_min(&torrent.createTime) && torrent.createTime > t.createTime {
            t.createTime = torrent.createTime;
            upt(&mut t, st, &mut update_full, false, false);
        }

        if !is_blank(&torrent.magnet) && torrent.magnet != t.magnet {
            t.ffprobe_tryingdata = 0;
            t.ffprobe = None;
            t.magnet = torrent.magnet.clone();
            upt(&mut t, st, &mut update_full, false, true);
        }

        if torrent.sid != t.sid {
            if t.sid == 0 && torrent.sid >= 2 && t.ffprobe_tryingdata >= c.tracksatempt {
                t.ffprobe_tryingdata = 0;
            }
            t.sid = torrent.sid;
            upt(&mut t, st, &mut update_full, false, false);
        }

        if torrent.pir != t.pir {
            t.pir = torrent.pir;
            upt(&mut t, st, &mut update_full, false, false);
        }

        if !is_blank(&torrent.sizeName) && torrent.sizeName != t.sizeName {
            t.sizeName = torrent.sizeName.clone();
            upt(&mut t, st, &mut update_full, true, true);
        }

        if !is_blank(&torrent.name) && torrent.name != t.name {
            t.name = torrent.name.clone();
            t._sn = search_name_or_empty(&t.name);
            upt(&mut t, st, &mut update_full, false, true);
        } else if is_blank(&t.name) && !is_blank(&torrent.title) {
            t.name = torrent.title.clone();
            t._sn = search_name_or_empty(&t.name);
            upt(&mut t, st, &mut update_full, false, true);
        }
        if is_blank(&t._sn) {
            if !is_blank(&t.name) {
                t._sn = search_name_or_empty(&t.name);
            } else if !is_blank(&torrent.title) {
                t._sn = search_name_or_empty(&torrent.title);
            }
            if !is_blank(&t._sn) {
                upt(&mut t, st, &mut update_full, false, true);
            }
        }

        if !is_blank(&torrent.originalname) && torrent.originalname != t.originalname {
            t.originalname = torrent.originalname.clone();
            t._so = search_name_or_empty(&t.originalname);
            upt(&mut t, st, &mut update_full, false, true);
        } else if is_blank(&t.originalname) {
            t.originalname = if !is_blank(&t.name) { t.name.clone() } else { torrent.title.clone() };
            t._so = search_name_or_empty(&t.originalname);
            upt(&mut t, st, &mut update_full, false, true);
        }
        if is_blank(&t._so) {
            if !is_blank(&t.originalname) {
                t._so = search_name_or_empty(&t.originalname);
            } else if !is_blank(&t.name) {
                t._so = search_name_or_empty(&t.name);
            } else if !is_blank(&torrent.title) {
                t._so = search_name_or_empty(&torrent.title);
            }
            if !is_blank(&t._so) {
                upt(&mut t, st, &mut update_full, false, true);
            }
        }

        if torrent.relased > 0 && torrent.relased != t.relased {
            t.relased = torrent.relased;
            upt(&mut t, st, &mut update_full, false, true);
        }

        if torrent.ffprobe.is_some() && t.ffprobe.is_none() {
            t.ffprobe = torrent.ffprobe.clone();
            upt(&mut t, st, &mut update_full, false, true);
        }

        if update_full {
            update_full_details(&mut t);
        }
        if c.logFdb {
            append_fdb_log(torrent, &t);
        }

        t.checkTime = time::now();

        drop_bare_lostfilm(st, &t);

        if is_lostfilm(&t.trackerName) {
            let new_key = key_db(&t.name, &t.originalname);
            if !new_key.is_empty() && new_key != fdbkey && new_key.find(':').map(|i| i > 0).unwrap_or(false) {
                st.db.shift_remove(&t.url);
                st.savechanges = true;
                let now_empty = st.db.is_empty();
                *migrate = Some((t, new_key, now_empty));
                return;
            }
        }

        add_or_update_master_db(&t);
        if found_by_id {
            st.db.entry(t.url.clone()).or_insert(t);
        } else {
            st.db.insert(t.url.clone(), t);
        }
    } else {
        if is_blank(&torrent.magnet) || torrent.types.is_empty() {
            return;
        }

        let mut name = if !torrent.name.is_empty() { torrent.name.clone() } else { torrent.title.clone() };
        let mut originalname = if !torrent.originalname.is_empty() { torrent.originalname.clone() } else { name.clone() };
        if is_blank(&name) && !is_blank(&torrent.title) {
            name = torrent.title.clone();
        }
        if is_blank(&originalname) {
            originalname = if !name.is_empty() { name.clone() } else { torrent.title.clone() };
        }

        let mut t = TorrentDetails {
            url: torrent.url.clone(),
            types: torrent.types.clone(),
            trackerName: torrent.trackerName.clone(),
            createTime: torrent.createTime,
            updateTime: torrent.updateTime,
            title: torrent.title.clone(),
            name,
            originalname,
            pir: torrent.pir,
            sid: torrent.sid,
            relased: torrent.relased,
            sizeName: torrent.sizeName.clone(),
            magnet: torrent.magnet.clone(),
            ffprobe: torrent.ffprobe.clone(),
            ..Default::default()
        };

        t._sn = search_name_or_empty(&t.name);
        if is_blank(&t._sn) && !is_blank(&t.title) {
            t._sn = search_name_or_empty(&t.title);
        }
        t._so = search_name_or_empty(&t.originalname);
        if is_blank(&t._so) {
            if !is_blank(&t.name) {
                t._so = search_name_or_empty(&t.name);
            } else if !is_blank(&t.title) {
                t._so = search_name_or_empty(&t.title);
            }
        }

        st.savechanges = true;
        update_full_details(&mut t);

        if c.logFdb {
            append_fdb_log(torrent, &t);
        }

        add_or_update_master_db(&t);
        drop_bare_lostfilm(st, &t);
        st.db.entry(t.url.clone()).or_insert(t);
    }
}

// ---------------------------------------------------------------------------
// Open read / write
// ---------------------------------------------------------------------------

/// Write handle; dropping it saves the shard (if dirty) and releases the refcount.
pub struct WriteGuard {
    db: Arc<FileDb>,
}

impl std::ops::Deref for WriteGuard {
    type Target = FileDb;
    fn deref(&self) -> &FileDb {
        &self.db
    }
}

impl Drop for WriteGuard {
    fn drop(&mut self) {
        self.db.save_changes_if_needed();
        let key = self.db.key.clone();
        let mut remove = false;
        if let Some(val) = OPEN_WRITE_TASK.get(&key) {
            if Arc::ptr_eq(&val.db, &self.db) {
                let mut remaining = val.openconnection.fetch_sub(1, Ordering::SeqCst) - 1;
                if remaining < 0 {
                    val.openconnection.store(0, Ordering::SeqCst);
                    remaining = 0;
                    log::warn(cat::FDB, format!("openconnection underflow for key={key}"));
                }
                if remaining <= 0 {
                    let c = conf();
                    if !c.evercache.enable || c.evercache.validHour > 0 {
                        remove = true;
                    }
                }
            }
        }
        if remove {
            OPEN_WRITE_TASK.remove_if(&key, |_, v| Arc::ptr_eq(&v.db, &self.db) && v.openconnection.load(Ordering::SeqCst) <= 0);
        }
    }
}

/// Snapshot of a shard.
pub fn open_read(key: &str, update_lastread: bool, cache: bool) -> ShardMap {
    if let Some(val) = OPEN_WRITE_TASK.get(key) {
        if update_lastread {
            val.countread.fetch_add(1, Ordering::Relaxed);
            *val.lastread.lock() = time::now();
        }
        return val.db.snapshot();
    }

    let fdb = Arc::new(FileDb::load(key));
    let c = conf();
    if c.evercache.enable && (cache || c.evercache.validHour == 0) {
        let wtm = Arc::new(WriteTask {
            db: fdb.clone(),
            lastread: Mutex::new(if update_lastread { time::now() } else { time::min() }),
            create: time::now(),
            countread: AtomicI32::new(if update_lastread { 1 } else { 0 }),
            openconnection: AtomicI32::new(0),
        });
        let entry = OPEN_WRITE_TASK.entry(key.to_string()).or_insert(wtm);
        return entry.db.snapshot();
    }
    fdb.snapshot()
}

/// Write handle to a shard (shared, refcounted).
pub fn open_write(key: &str) -> WriteGuard {
    loop {
        if let Some(existing) = OPEN_WRITE_TASK.get(key).map(|e| e.clone()) {
            existing.openconnection.fetch_add(1, Ordering::SeqCst);
            if let Some(again) = OPEN_WRITE_TASK.get(key) {
                if Arc::ptr_eq(&again, &existing) {
                    return WriteGuard { db: existing.db.clone() };
                }
            }
            let left = existing.openconnection.fetch_sub(1, Ordering::SeqCst) - 1;
            if left < 0 {
                existing.openconnection.store(0, Ordering::SeqCst);
            }
            continue;
        }

        let fdb = Arc::new(FileDb::load(key));
        let wtm = Arc::new(WriteTask {
            db: fdb.clone(),
            lastread: Mutex::new(time::min()),
            create: time::now(),
            countread: AtomicI32::new(0),
            openconnection: AtomicI32::new(1),
        });
        match OPEN_WRITE_TASK.entry(key.to_string()) {
            dashmap::mapref::entry::Entry::Vacant(v) => {
                v.insert(wtm);
                return WriteGuard { db: fdb };
            }
            dashmap::mapref::entry::Entry::Occupied(_) => continue,
        }
    }
}

/// Move a row into another bucket (after name/originalname change).
pub fn migrate_torrent_to_new_key(t: &TorrentDetails, new_key: &str) {
    let w = open_write(new_key);
    w.add_or_update(t);
}

/// Group by bucket key and upsert.
pub fn add_or_update<T: AsRef<TorrentDetails>>(torrents: &[T]) {
    let mut groups: IndexMap<String, Vec<&TorrentDetails>> = IndexMap::new();
    for t in torrents {
        let t = t.as_ref();
        groups.entry(key_db(&t.name, &t.originalname)).or_default().push(t);
    }
    for (key, list) in groups {
        let w = open_write(&key);
        for t in list {
            w.add_or_update(t);
        }
    }
}

/// Upsert where each row goes through an async step that owns it: the step sees the
/// stored row (found by `lookup`, usually [`by_url`]), may download a .torrent / fill
/// the magnet across `.await`s, and returns `Some(row)` to save or `None` to skip.
pub async fn add_or_update_async<T, L, F, Fut>(torrents: Vec<T>, lookup: L, mut step: F)
where
    T: AsRef<TorrentDetails>,
    L: Fn(&ShardMap, &T) -> Option<TorrentDetails>,
    F: FnMut(T, Option<TorrentDetails>) -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let mut groups: IndexMap<String, Vec<T>> = IndexMap::new();
    for t in torrents {
        let key = {
            let r = t.as_ref();
            key_db(&r.name, &r.originalname)
        };
        groups.entry(key).or_default().push(t);
    }
    for (key, list) in groups {
        let w = open_write(&key);
        for t in list {
            let cached = w.with_db(|db| lookup(db, &t));
            if let Some(t) = step(t, cached).await {
                w.add_or_update(t.as_ref());
            }
        }
    }
}

/// Default lookup for [`add_or_update_async`]: stored row with the same url.
pub fn by_url<T: AsRef<TorrentDetails>>(db: &ShardMap, t: &T) -> Option<TorrentDetails> {
    db.get(&t.as_ref().url).cloned()
}

/// Update ffprobe attempt counter / streams for a torrent found by magnet.
pub fn update_torrent_ffprobe_info(
    torrent_key: &str,
    magnet: &str,
    ffprobe_trying_data: i32,
    streams: Option<Vec<crate::models::FfStream>>,
) {
    if torrent_key.is_empty() || magnet.is_empty() {
        return;
    }
    let w = open_write(torrent_key);
    let mut touched: Option<TorrentDetails> = None;
    w.modify(|db| {
        let Some(t) = db.values_mut().find(|t| !t.magnet.is_empty() && t.magnet.eq_ignore_ascii_case(magnet)) else {
            return false;
        };
        let mut updated = false;
        if t.ffprobe_tryingdata != ffprobe_trying_data {
            t.ffprobe_tryingdata = ffprobe_trying_data;
            updated = true;
        }
        if let Some(s) = streams.as_ref().filter(|s| !s.is_empty()) {
            t.ffprobe = Some(s.clone());
            updated = true;
        }
        if updated {
            t.updateTime = time::now();
            touched = Some(t.clone());
        }
        updated
    });
    if let Some(t) = touched {
        add_or_update_master_db(&t);
    }
}

// ---------------------------------------------------------------------------
// fdb.YYYY-MM-DD.log
// ---------------------------------------------------------------------------

const FDB_LOG_DIR: &str = "Data/log";
const FDB_LOG_PREFIX: &str = "fdb.";

fn fdb_log_files() -> Vec<(std::path::PathBuf, u64, chrono::NaiveDate)> {
    let mut list = Vec::new();
    if let Ok(rd) = std::fs::read_dir(FDB_LOG_DIR) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_prefix(FDB_LOG_PREFIX).and_then(|s| s.strip_suffix(".log")) else {
                continue;
            };
            if let Ok(d) = chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d") {
                let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                list.push((e.path(), len, d));
            }
        }
    }
    list
}

/// Unix seconds of the last retention / size cleanup (at most once a minute, not per line).
static FDB_LOG_CLEANUP_AT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
const FDB_LOG_CLEANUP_EVERY_SECS: i64 = 60;

fn append_fdb_log(torrent: &TorrentDetails, t: &TorrentDetails) {
    use std::io::Write;
    let c = conf();
    let _ = std::fs::create_dir_all(FDB_LOG_DIR);
    let path = format!("{FDB_LOG_DIR}/{FDB_LOG_PREFIX}{}.log", Utc::now().format("%Y-%m-%d"));
    if let Ok(line) = serde_json::to_string(&[torrent, t]) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(line.as_bytes());
            let _ = f.write_all(b"\n");
        }
    }
    let now = Utc::now().timestamp();
    let last = FDB_LOG_CLEANUP_AT.load(std::sync::atomic::Ordering::Relaxed);
    if now - last >= FDB_LOG_CLEANUP_EVERY_SECS
        && FDB_LOG_CLEANUP_AT.compare_exchange(last, now, std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::Relaxed).is_ok()
    {
        cleanup_fdb_logs(c.logFdbRetentionDays, c.logFdbMaxSizeMb, c.logFdbMaxFiles);
    }
}

/// Retention by age, then size / count limits (oldest files first).
pub fn cleanup_fdb_logs(retention_days: i32, max_size_mb: i32, max_files: i32) {
    if retention_days > 0 {
        let cutoff = Utc::now().date_naive() - chrono::Duration::days(retention_days as i64);
        for (p, _, d) in fdb_log_files() {
            if d < cutoff {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    purge_fdb_log(max_size_mb, max_files);
}

/// FileDB change journals on disk: (files, total bytes, oldest day, newest day).
pub fn fdb_log_usage() -> (usize, u64, Option<chrono::NaiveDate>, Option<chrono::NaiveDate>) {
    let list = fdb_log_files();
    let total = list.iter().map(|x| x.1).sum();
    let oldest = list.iter().map(|x| x.2).min();
    let newest = list.iter().map(|x| x.2).max();
    (list.len(), total, oldest, newest)
}

/// Delete all FileDB change journals; returns (files removed, bytes freed).
pub fn clear_fdb_logs() -> (usize, u64) {
    let mut n = 0;
    let mut bytes = 0;
    for (p, len, _) in fdb_log_files() {
        if std::fs::remove_file(p).is_ok() {
            n += 1;
            bytes += len;
        }
    }
    (n, bytes)
}

fn purge_fdb_log(max_size_mb: i32, max_files: i32) {
    if max_size_mb <= 0 && max_files <= 0 {
        return;
    }
    let max_bytes = if max_size_mb > 0 { max_size_mb as u64 * 1024 * 1024 } else { u64::MAX };
    let max_files = if max_files > 0 { max_files as usize } else { usize::MAX };
    let mut list = fdb_log_files();
    list.sort_by_key(|x| x.2);
    let mut total: u64 = list.iter().map(|x| x.1).sum();
    let mut count = list.len();
    for (p, len, _) in list {
        if total <= max_bytes && count <= max_files {
            break;
        }
        if std::fs::remove_file(p).is_ok() {
            total = total.saturating_sub(len);
            count -= 1;
        }
    }
}

/// Number of cached open shards (diagnostics).
pub fn open_write_task_count() -> usize {
    OPEN_WRITE_TASK.len()
}

/// Search helper: `search_name` re-export for callers building keys.
#[cfg(test)]
mod tests {
    #[test]
    fn shard_with_null_row_is_readable() {
        let dir = std::env::temp_dir().join(format!("crab-shard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s").to_string_lossy().to_string();
        let v = serde_json::json!({"a": null, "b": {"url": "b", "title": "T"}, "c": 5});
        super::write_gz_json(&p, &v);
        let m = super::read_shard(&p).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m["b"].title, "T");
        let _ = std::fs::remove_dir_all(dir);
    }
}

pub fn sn(s: &str) -> Option<String> {
    search_name(s)
}
