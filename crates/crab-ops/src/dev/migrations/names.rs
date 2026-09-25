//! Re-derive name / originalname / year of stored rows with the current title parsing
//! of knaben, bitru and rudub, moving rows whose bucket key changes.

use crab_core::fdb;
use crab_core::models::TorrentDetails;
use crab_core::{rx, util};
use serde_json::{json, Value};

use super::parsers::{bitru, knaben, rudub};
use super::{migrate_all, try_rebuild_fast_db};
use crate::maintenance::migration_target;

/// Walk every bucket, patch rows of `tracker` with `patch` (returns true when changed),
/// migrate moved rows. `save_if(bucket_changed, migrated_total)` decides whether the shard
/// is marked dirty. Returns (processed, migrated); `patch` does its own counting.
fn walk<P, S>(tracker: &str, mut patch: P, save_if: S) -> (i64, i64)
where
    P: FnMut(&mut TorrentDetails) -> bool,
    S: Fn(bool, i64) -> bool,
{
    let (mut processed, mut migrated) = (0i64, 0i64);
    for (key, _) in fdb::master_db_snapshot() {
        let w = fdb::open_write(&key);
        let mut to_migrate: Vec<(TorrentDetails, String)> = Vec::new();
        let mut bucket_changed = false;
        w.modify(|db| {
            let mut remove = Vec::new();
            for (url, t) in db.iter_mut() {
                if !t.trackerName.eq_ignore_ascii_case(tracker) {
                    continue;
                }
                processed += 1;
                if !patch(t) {
                    continue;
                }
                bucket_changed = true;
                if let Some(nk) = migration_target(t, &key) {
                    to_migrate.push((t.clone(), nk));
                    remove.push(url.clone());
                }
            }
            for u in remove {
                db.shift_remove(&u);
            }
            false
        });
        migrated += migrate_all(to_migrate);
        if w.is_empty() {
            fdb::remove_key_from_master_db(&key);
        }
        if save_if(bucket_changed, migrated) {
            w.mark_changed();
        }
    }
    (processed, migrated)
}

/// knaben: name/year/title from the stored title.
pub fn fix_knaben_names() -> Value {
    let updated_ref = std::cell::Cell::new(0i64);
    let (processed, migrated) = walk(
        "knaben",
        |t| {
            let source = if !util::is_blank(&t.title) { t.title.clone() } else { t.name.clone() };
            if util::is_blank(&source) {
                return false;
            }
            let (new_name, new_relased) = knaben::parse_name_and_year(&source);
            if util::is_blank(&new_name) {
                return false;
            }
            let suffix = rx::captures(&source, r"\s+\|\s+[^|]+$").and_then(|c| c.get(0).map(|m| m.as_str().to_string())).unwrap_or_default();
            let new_title = knaben::build_title_for_file_db(source.trim_end()) + &suffix;
            let name_changed = new_name != t.name || new_name != t.originalname;
            let relased_changed = new_relased != t.relased;
            let title_changed = new_title != t.title;
            if !name_changed && !relased_changed && !title_changed {
                return false;
            }
            t._sn = util::search_name_or_empty(&new_name);
            t._so = t._sn.clone();
            t.originalname = new_name.clone();
            t.name = new_name;
            t.relased = new_relased;
            t.title = new_title;
            updated_ref.set(updated_ref.get() + 1);
            true
        },
        |_, migrated| updated_ref.get() > 0 || migrated > 0,
    );
    let updated = updated_ref.get();
    fdb::save_changes_to_file();
    try_rebuild_fast_db();
    json!({ "ok": true, "processed": processed, "updated": updated, "migrated": migrated })
}

/// bitru: strip season/quality noise from name/originalname.
pub fn fix_bitru_names() -> Value {
    let updated = std::cell::Cell::new(0i64);
    let (processed, migrated) = walk(
        "bitru",
        |t| {
            let mut new_name = bitru::clean_title_for_search(&t.name).trim().to_string();
            let mut new_orig = bitru::clean_title_for_search(&t.originalname).trim().to_string();
            if util::is_blank(&new_name) {
                new_name = t.name.trim().to_string();
            }
            if util::is_blank(&new_orig) {
                new_orig = t.originalname.trim().to_string();
            }
            if util::is_blank(&new_orig) {
                new_orig = new_name.clone();
            }
            if new_name == t.name && new_orig == t.originalname {
                return false;
            }
            t._sn = util::search_name_or_empty(&new_name);
            t._so = util::search_name_or_empty(&new_orig);
            t.name = new_name;
            t.originalname = new_orig;
            updated.set(updated.get() + 1);
            true
        },
        |_, migrated| updated.get() > 0 || migrated > 0,
    );
    fdb::save_changes_to_file();
    try_rebuild_fast_db();
    json!({ "ok": true, "processed": processed, "updated": updated.get(), "migrated": migrated })
}

/// Apply the rudub title parsing to a stored row. Returns (changed, yearUpdated, namesUpdated).
pub fn rudub_try_patch(t: &mut TorrentDetails) -> (bool, bool, bool) {
    if util::is_blank(&t.title) {
        return (false, false, false);
    }
    let (name, original, relased) = rudub::parse_title_fields(&t.title, &t.createTime);
    let mut year_updated = false;
    let mut names_updated = false;
    if relased > 0 && t.relased != relased {
        t.relased = relased;
        year_updated = true;
    }
    if !util::is_blank(&name) && name != t.name {
        t._sn = util::search_name_or_empty(&name);
        t.name = name;
        names_updated = true;
    }
    if !util::is_blank(&original) && original != t.originalname {
        t._so = util::search_name_or_empty(&original);
        t.originalname = original;
        names_updated = true;
    }
    (year_updated || names_updated, year_updated, names_updated)
}

/// rudub: backfill `relased` (and truncated names) from stored titles.
pub fn fix_rudub_relased() -> Value {
    let year_updated = std::cell::Cell::new(0i64);
    let names_updated = std::cell::Cell::new(0i64);
    let (processed, migrated) = walk(
        rudub::TRACKER_NAME,
        |t| {
            let (changed, y, n) = rudub_try_patch(t);
            if y {
                year_updated.set(year_updated.get() + 1);
            }
            if n {
                names_updated.set(names_updated.get() + 1);
            }
            changed
        },
        |bucket_changed, _| bucket_changed,
    );
    fdb::save_changes_to_file();
    if names_updated.get() > 0 || migrated > 0 {
        try_rebuild_fast_db();
    }
    json!({
        "ok": true,
        "processed": processed,
        "yearUpdated": year_updated.get(),
        "namesUpdated": names_updated.get(),
        "migrated": migrated
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn row(title: &str, name: &str, orig: &str, relased: i32, created: (i32, u32, u32)) -> TorrentDetails {
        TorrentDetails {
            trackerName: "rudub".into(),
            title: title.into(),
            name: name.into(),
            originalname: orig.into(),
            relased,
            createTime: Utc.with_ymd_and_hms(created.0, created.1, created.2, 0, 0, 0).unwrap(),
            ..Default::default()
        }
    }

    #[test]
    fn rudub_patch_fills_missing_year_from_create_time() {
        let mut t = row("Фонари (Lanterns) Сезон 1 Серии 01-04 (HD1080p WEBRip)", "Фонари", "Lanterns", 0, (2026, 9, 7));
        assert_eq!(rudub_try_patch(&mut t), (true, true, false));
        assert_eq!(t.relased, 2026);
        assert_eq!(t.name, "Фонари");
        assert_eq!(t.originalname, "Lanterns");
    }

    #[test]
    fn rudub_patch_fixes_year_stolen_as_originalname() {
        let mut t = row("Гнев (2026) (Furia (Wrath)) Сезон 1 Серии 01-06 (HD1080p WEBRip)", "Гнев", "2026", 0, (2026, 7, 29));
        assert_eq!(rudub_try_patch(&mut t), (true, true, true));
        assert_eq!(t.relased, 2026);
        assert_eq!(t.name, "Гнев");
        assert_eq!(t.originalname, "Furia (Wrath)");
        assert_eq!(t._so, "furiawrath");
    }

    #[test]
    fn rudub_patch_unchanged_returns_false() {
        let mut t = row("Фонари (Lanterns) Сезон 1 Серии 01-04 (HD1080p WEBRip)", "Фонари", "Lanterns", 2026, (2026, 9, 7));
        t.trackerName.clear();
        assert_eq!(rudub_try_patch(&mut t), (false, false, false));
    }
}
