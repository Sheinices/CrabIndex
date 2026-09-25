//! aniliberty: `?hash=` URLs and duplicate rows sharing an infohash.

use chrono::{DateTime, Utc};
use crab_core::fdb;
use crab_core::models::TorrentDetails;
use crab_core::{rx, time, util};
use indexmap::IndexMap;
use serde_json::{json, Value};

const TRACKER: &str = "aniliberty";

fn btih(magnet: &str) -> Option<String> {
    if util::is_blank(magnet) {
        return None;
    }
    let h = rx::group(magnet, r"(?i)urn:btih:([a-fA-F0-9]{40})", 1);
    (!h.is_empty()).then_some(h)
}

/// Append `hash=<btih>` to every aniliberty URL that lacks it.
pub fn migrate_urls() -> Value {
    let (mut processed, mut updated, mut skipped, mut error_count) = (0i64, 0i64, 0i64, 0i64);
    let mut errors: Vec<String> = Vec::new();
    for (key, _) in fdb::master_db_snapshot() {
        let w = fdb::open_write(&key);
        w.modify(|db| {
            let mut to_update: Vec<(String, String)> = Vec::new();
            for (url, t) in db.iter() {
                if !t.trackerName.eq_ignore_ascii_case(TRACKER) {
                    continue;
                }
                processed += 1;
                if url.contains("?hash=") {
                    skipped += 1;
                    continue;
                }
                let Some(hash) = btih(&t.magnet) else {
                    error_count += 1;
                    errors.push(format!("No hash found in magnet for URL: {url}"));
                    continue;
                };
                let new_url = if url.contains('?') { format!("{url}&hash={hash}") } else { format!("{url}?hash={hash}") };
                if &new_url == url {
                    skipped += 1;
                    continue;
                }
                to_update.push((url.clone(), new_url));
            }
            let any = !to_update.is_empty();
            for (old, new_url) in to_update {
                if let Some(mut t) = db.shift_remove(&old) {
                    t.url = new_url.clone();
                    db.insert(new_url, t);
                    updated += 1;
                }
            }
            any
        });
    }
    fdb::save_changes_to_file();
    errors.truncate(10);
    json!({
        "ok": true,
        "totalProcessed": processed,
        "totalUpdated": updated,
        "totalSkipped": skipped,
        "totalErrors": error_count,
        "errors": errors
    })
}

struct Entry {
    bucket: String,
    url: String,
    title: String,
    update_time: DateTime<Utc>,
}

/// Keep the newest row per infohash, delete the rest.
pub fn remove_duplicates() -> Value {
    let mut processed = 0i64;
    let mut removed = 0i64;
    let mut info: Vec<Value> = Vec::new();
    let mut by_hash: IndexMap<String, Vec<Entry>> = IndexMap::new();

    for (key, _) in fdb::master_db_snapshot() {
        for (url, t) in fdb::open_read(&key, false, false) {
            let t: TorrentDetails = t;
            if !t.trackerName.eq_ignore_ascii_case(TRACKER) {
                continue;
            }
            processed += 1;
            let Some(hash) = btih(&t.magnet).map(|h| h.to_lowercase()) else { continue };
            by_hash.entry(hash).or_default().push(Entry { bucket: key.clone(), url, title: t.title, update_time: t.updateTime });
        }
    }

    for (hash, mut list) in by_hash.into_iter().filter(|(_, v)| v.len() > 1) {
        list.sort_by(|a, b| b.update_time.cmp(&a.update_time).then_with(|| a.url.cmp(&b.url)));
        let keep = &list[0];
        let rest = &list[1..];
        info.push(json!({
            "hash": hash,
            "title": crate::query::s_or_null(&keep.title),
            "keepUrl": keep.url,
            "keepBucket": keep.bucket,
            "keepUpdateTime": time::format_net(&keep.update_time),
            "removeCount": rest.len(),
            "removeUrls": rest.iter().map(|x| json!({
                "url": x.url, "bucket": x.bucket, "updateTime": time::format_net(&x.update_time)
            })).collect::<Vec<_>>()
        }));
        for x in rest {
            let w = fdb::open_write(&x.bucket);
            if w.modify(|db| db.shift_remove(&x.url).is_some()) {
                removed += 1;
            }
        }
    }

    fdb::save_changes_to_file();
    let found = info.len();
    info.truncate(50);
    json!({
        "ok": true,
        "totalProcessed": processed,
        "totalRemoved": removed,
        "duplicatesFound": found,
        "duplicates": info
    })
}
