//! Bulk FileDB rewrites: size, checkTime, derived details, search names.

use chrono::Duration;
use crab_core::models::TorrentDetails;
use crab_core::{fdb, time, util};
use serde_json::{json, Value};

use crate::maintenance::migration_target;
use crate::nulls;

pub use crab_core::fdb::size_from_name;

/// Walk every bucket in `order`, apply `f` to each row, always mark the shard dirty.
fn rewrite_all(keys: Vec<String>, mut f: impl FnMut(&str, &mut TorrentDetails)) {
    for key in keys {
        let (w, _) = nulls::open_write_clean(&key);
        w.modify(|db| {
            for t in db.values_mut() {
                f(&key, t);
            }
            true
        });
    }
    fdb::save_changes_to_file();
}

fn keys() -> Vec<String> {
    fdb::master_db_snapshot().into_iter().map(|(k, _)| k).collect()
}

pub fn update_size() -> Value {
    let mut ordered = fdb::master_db_snapshot();
    ordered.sort_by_key(|(_, s)| s.fileTime);
    rewrite_all(ordered.into_iter().map(|(k, _)| k).collect(), |key, t| {
        t.size = size_from_name(&t.sizeName) as f64;
        t.updateTime = time::now();
        fdb::set_shard(key, t.updateTime);
    });
    json!({ "ok": true })
}

/// Recompute `size` only where it is 0 but `sizeName` is readable (e.g. labels with a
/// non-breaking space that older builds could not parse). Touched rows get a fresh
/// `updateTime`, so sync clients receive them.
pub fn fix_zero_sizes() -> Value {
    let mut fixed = 0i64;
    let mut ordered = fdb::master_db_snapshot();
    ordered.sort_by_key(|(_, s)| s.fileTime);
    rewrite_all(ordered.into_iter().map(|(k, _)| k).collect(), |key, t| {
        if t.size > 0.0 || util::is_blank(&t.sizeName) {
            return;
        }
        let size = size_from_name(&t.sizeName);
        if size <= 0 {
            return;
        }
        t.size = size as f64;
        t.updateTime = time::now();
        fdb::set_shard(key, t.updateTime);
        fixed += 1;
    });
    json!({ "ok": true, "fixed": fixed })
}

pub fn reset_check_time() -> Value {
    let yesterday = time::today_local() - Duration::days(1);
    rewrite_all(keys(), |_, t| t.checkTime = yesterday);
    json!({ "ok": true })
}

pub fn update_details() -> Value {
    rewrite_all(keys(), |key, t| {
        fdb::update_full_details(t);
        t.languages.clear();
        t.updateTime = time::now();
        fdb::set_shard(key, t.updateTime);
    });
    json!({ "ok": true })
}

pub fn update_search_name() -> Value {
    for key in keys() {
        let (w, _) = nulls::open_write_clean(&key);
        let mut to_migrate: Vec<(TorrentDetails, String)> = Vec::new();
        w.modify(|db| {
            let mut remove = Vec::new();
            for (url, t) in db.iter_mut() {
                if util::is_blank(&t.name) {
                    t.name = t.title.clone();
                }
                if util::is_blank(&t.originalname) {
                    t.originalname = if t.title.is_empty() { t.name.clone() } else { t.title.clone() };
                }
                t._sn = util::search_name_or_empty(&t.name);
                t._so = util::search_name_or_empty(&t.originalname);
                if let Some(nk) = migration_target(t, &key) {
                    to_migrate.push((t.clone(), nk));
                    remove.push(url.clone());
                }
            }
            for u in remove {
                db.shift_remove(&u);
            }
            true
        });
        crate::dev::migrations::migrate_all(to_migrate);
        if w.is_empty() {
            fdb::remove_key_from_master_db(&key);
        }
    }
    fdb::save_changes_to_file();
    json!({ "ok": true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size_from_name("700 Mb"), 700 * 1_048_576);
        assert_eq!(size_from_name("1,5 GB"), (1.5 * 1024.0 * 1_048_576.0) as i64);
        assert_eq!(size_from_name("2 ТБ"), 2 * 1_048_576 * 1_048_576);
        assert_eq!(size_from_name("1.5 гб"), (1.5 * 1024.0 * 1_048_576.0) as i64);
        assert_eq!(size_from_name("0 GB"), 0);
        assert_eq!(size_from_name("big"), 0);
        assert_eq!(size_from_name(""), 0);
    }
}
