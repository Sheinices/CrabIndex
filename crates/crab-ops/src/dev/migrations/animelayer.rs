//! animelayer: move `http://` rows to `https://`, dropping http duplicates.

use crab_core::fdb;
use crab_core::rx;
use serde_json::{json, Value};

const TRACKER: &str = "animelayer";

pub fn fix_duplicates() -> Value {
    let (mut processed, mut fixed, mut removed) = (0i64, 0i64, 0i64);
    let mut errors: Vec<String> = Vec::new();

    for (key, _) in fdb::master_db_snapshot() {
        for (url, t) in fdb::open_read(&key, false, false) {
            if !t.trackerName.eq_ignore_ascii_case(TRACKER) {
                continue;
            }
            processed += 1;
            if !rx::is_match(&url, r"(?i)/torrent/([a-f0-9]+)/?") {
                errors.push(format!("Could not extract ID from URL: {url}"));
            }
        }
    }

    for (key, _) in fdb::master_db_snapshot() {
        let w = fdb::open_write(&key);
        w.modify(|db| {
            let mut to_remove: Vec<String> = Vec::new();
            let mut to_update: Vec<(String, String)> = Vec::new();
            for (url, t) in db.iter() {
                if !t.trackerName.eq_ignore_ascii_case(TRACKER) {
                    continue;
                }
                let lower = url.to_lowercase();
                if lower.starts_with("https://") || !lower.starts_with("http://") {
                    continue;
                }
                let new_url = rx::replace(url, "(?i)http://", "https://");
                if db.contains_key(&new_url) {
                    to_remove.push(url.clone());
                    removed += 1;
                } else {
                    to_update.push((url.clone(), new_url));
                    fixed += 1;
                }
            }
            let any = !to_remove.is_empty() || !to_update.is_empty();
            for u in to_remove {
                db.shift_remove(&u);
            }
            for (old, new_url) in to_update {
                if let Some(mut t) = db.shift_remove(&old) {
                    t.url = new_url.clone();
                    db.insert(new_url, t);
                }
            }
            any
        });
    }

    fdb::save_changes_to_file();
    let total_errors = errors.len();
    errors.truncate(10);
    json!({
        "ok": true,
        "totalProcessed": processed,
        "totalFixed": fixed,
        "totalRemoved": removed,
        "totalErrors": total_errors,
        "errors": errors
    })
}
