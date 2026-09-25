//! Bulk FileDB rewrites: size, checkTime, derived details, search names.

use chrono::Duration;
use crab_core::models::TorrentDetails;
use crab_core::{fdb, rx, time, util};
use serde_json::{json, Value};

use crate::maintenance::migration_target;
use crate::nulls;

/// Bytes from a size label like `1,5 GB` / `700 МБ` (0 when unknown).
pub fn size_from_name(size_name: &str) -> i64 {
    if util::is_blank(size_name) {
        return 0;
    }
    let g = rx::groups(size_name, r"(?i)([0-9\.,]+) (Mb|МБ|GB|ГБ|TB|ТБ)");
    let (num, unit) = (g.get(1).cloned().unwrap_or_default(), g.get(2).cloned().unwrap_or_default());
    if util::is_blank(&unit) {
        return 0;
    }
    let Ok(mut size) = num.replace(',', ".").parse::<f64>() else { return 0 };
    if size == 0.0 {
        return 0;
    }
    let u = unit.to_lowercase();
    if u == "gb" || u == "гб" {
        size *= 1024.0;
    }
    if u == "tb" || u == "тб" {
        size *= 1_048_576.0;
    }
    (size * 1_048_576.0) as i64
}

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
