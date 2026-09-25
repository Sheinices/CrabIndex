//! Generic cleanups: null rows, bucket removal/merge, empty search fields.

use crab_core::fdb;
use crab_core::models::TorrentDetails;
use crab_core::util;
use serde_json::{json, Value};

use super::{migrate_all, try_rebuild_fast_db};
use crate::maintenance::{fix_search_fields, migration_target};
use crate::nulls;

/// Drop rows stored as `null`. Returns `{ok, removed, affectedFiles}`.
pub fn remove_null_values() -> Value {
    let mut total_removed = 0usize;
    let mut affected_files = 0;
    for (key, _) in fdb::master_db_snapshot() {
        let n = nulls::purge_null_rows(&key);
        if n > 0 {
            total_removed += n;
            affected_files += 1;
        }
    }
    fdb::save_changes_to_file();
    json!({ "ok": true, "removed": total_removed, "affectedFiles": affected_files })
}

/// Delete a bucket, or move all its rows under `migrate_name:migrate_originalname`.
pub fn remove_bucket(key: Option<&str>, migrate_name: Option<&str>, migrate_originalname: Option<&str>) -> Value {
    let key = match key {
        Some(k) if !util::is_blank(k) && k.contains(':') => k.trim().to_string(),
        _ => return json!({ "error": "key required, format: name:originalname (e.g. ponies:ponies)" }),
    };
    if !fdb::MASTER_DB.contains_key(&key) {
        return json!({ "error": "key not found", "key": key });
    }
    let mname = migrate_name.filter(|s| !util::is_blank(s)).map(|s| s.to_string());
    let moname = migrate_originalname.filter(|s| !util::is_blank(s)).map(|s| s.to_string());
    let do_migrate = mname.is_some() && moname.is_some();
    let new_key = if do_migrate {
        fdb::key_for_torrent(mname.as_deref().unwrap_or(""), moname.as_deref().unwrap_or(""))
    } else {
        String::new()
    };

    let (w, nulls_removed) = nulls::open_write_clean(&key);
    let mut to_migrate: Vec<(TorrentDetails, String)> = Vec::new();
    let mut removed = nulls_removed as i64;
    w.modify(|db| {
        let urls: Vec<String> = db.keys().cloned().collect();
        for url in urls {
            if do_migrate {
                if let Some(t) = db.get_mut(&url) {
                    let (n, o) = (mname.clone().unwrap_or_default(), moname.clone().unwrap_or_default());
                    t._sn = util::search_name_or_empty(&n);
                    t._so = util::search_name_or_empty(&o);
                    t.name = n;
                    t.originalname = o;
                    to_migrate.push((t.clone(), new_key.clone()));
                }
            } else {
                removed += 1;
            }
            db.shift_remove(&url);
        }
        true
    });
    let migrated = migrate_all(to_migrate);
    if w.is_empty() {
        fdb::remove_key_from_master_db(&key);
    }
    drop(w);
    fdb::save_changes_to_file();
    json!({
        "ok": true,
        "key": key,
        "migrated": migrated,
        "removed": removed,
        "newKey": if do_migrate { Value::String(new_key) } else { Value::Null },
    })
}

/// Fill empty `_sn/_so` and migrate rows whose bucket key changed.
pub fn fix_empty_search_fields() -> Value {
    let (mut total_fixed, mut sn_fixed, mut so_fixed, mut migrated, mut affected) = (0i64, 0i64, 0i64, 0i64, 0i64);
    for (key, _) in fdb::master_db_snapshot() {
        let (w, nulls_removed) = nulls::open_write_clean(&key);
        let mut to_migrate: Vec<(TorrentDetails, String)> = Vec::new();
        let mut bucket_changed = false;
        w.modify(|db| {
            let mut remove = Vec::new();
            for (url, t) in db.iter_mut() {
                let (sn, so) = fix_search_fields(t);
                if sn || so {
                    total_fixed += 1;
                    if sn {
                        sn_fixed += 1;
                    }
                    if so {
                        so_fixed += 1;
                    }
                    if let Some(nk) = migration_target(t, &key) {
                        to_migrate.push((t.clone(), nk));
                        remove.push(url.clone());
                        bucket_changed = true;
                    }
                }
            }
            for u in remove {
                db.shift_remove(&u);
            }
            false
        });
        let had_migrations = !to_migrate.is_empty();
        migrated += migrate_all(to_migrate);
        if w.is_empty() {
            fdb::remove_key_from_master_db(&key);
            bucket_changed = true;
        }
        if bucket_changed || had_migrations || nulls_removed > 0 {
            affected += 1;
            w.mark_changed();
        }
    }
    fdb::save_changes_to_file();
    try_rebuild_fast_db();
    json!({
        "ok": true,
        "totalFixed": total_fixed,
        "snFixed": sn_fixed,
        "soFixed": so_fixed,
        "migrated": migrated,
        "affectedBuckets": affected
    })
}
