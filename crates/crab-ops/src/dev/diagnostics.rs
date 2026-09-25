//! Read-only FileDB diagnostics.

use crab_core::{fdb, util};
use serde_json::{json, Value};

use crate::maintenance::{is_xx_key, IssueBucket};
use crate::nulls;
use crate::query::s_or_null;

/// Rows stored as null, or missing name / originalname / trackerName.
pub fn find_corrupt(sample_size: i32) -> Value {
    let mut total = 0i64;
    let mut null_value = IssueBucket::new(sample_size);
    let mut missing_name = IssueBucket::new(sample_size);
    let mut missing_orig = IssueBucket::new(sample_size);
    let mut missing_tracker = IssueBucket::new(sample_size);

    for (key, _) in fdb::master_db_snapshot() {
        let (db, null_urls) = nulls::load_rows_lenient(&key);
        for url in null_urls {
            total += 1;
            null_value.add(json!({ "fdbKey": key, "url": url }));
        }
        for (url, t) in &db {
            total += 1;
            let r = || json!({ "fdbKey": key, "url": url, "title": s_or_null(&t.title) });
            if util::is_blank(&t.trackerName) {
                missing_tracker.add(r());
            }
            if util::is_blank(&t.name) {
                missing_name.add(r());
            }
            if util::is_blank(&t.originalname) {
                missing_orig.add(r());
            }
        }
    }

    json!({
        "ok": true,
        "totalFdbKeys": fdb::master_db().len(),
        "totalTorrents": total,
        "corrupt": {
            "nullValue": null_value.to_value(),
            "missingName": missing_name.to_value(),
            "missingOriginalname": missing_orig.to_value(),
            "missingTrackerName": missing_tracker.to_value()
        }
    })
}

/// `name:name` buckets, optionally only those holding rows of `tracker`.
pub fn find_duplicate_keys(tracker: Option<&str>, exclude_numeric: bool) -> Value {
    let mut keys = Vec::new();
    for (key, _) in fdb::master_db_snapshot() {
        if !is_xx_key(&key, exclude_numeric) {
            continue;
        }
        let db = fdb::open_read(&key, false, false);
        if let Some(tr) = tracker.filter(|t| !util::is_blank(t)) {
            let tr = tr.trim();
            if !db.values().any(|t| t.trackerName.eq_ignore_ascii_case(tr)) {
                continue;
            }
        }
        keys.push(json!({ "key": key, "count": db.len() }));
    }
    json!({ "ok": true, "count": keys.len(), "keys": keys })
}

/// Rows with empty `_sn` and/or `_so`.
pub fn find_empty_search_fields(sample_size: i32) -> Value {
    let mut total = 0i64;
    let mut empty_sn = IssueBucket::new(sample_size);
    let mut empty_so = IssueBucket::new(sample_size);
    let mut empty_both = IssueBucket::new(sample_size);

    for (key, _) in fdb::master_db_snapshot() {
        let (db, null_urls) = nulls::load_rows_lenient(&key);
        total += null_urls.len() as i64;
        for (url, t) in &db {
            total += 1;
            let r = || {
                json!({
                    "fdbKey": key, "url": url, "title": s_or_null(&t.title),
                    "name": s_or_null(&t.name), "originalname": s_or_null(&t.originalname)
                })
            };
            let (sn, so) = (util::is_blank(&t._sn), util::is_blank(&t._so));
            if sn && so {
                empty_both.add(r());
            } else if sn {
                empty_sn.add(r());
            } else if so {
                empty_so.add(r());
            }
        }
    }

    json!({
        "ok": true,
        "totalFdbKeys": fdb::master_db().len(),
        "totalTorrents": total,
        "emptySearchFields": {
            "emptySn": empty_sn.to_value(),
            "emptySo": empty_so.to_value(),
            "emptyBoth": empty_both.to_value(),
            "total": empty_sn.count + empty_so.count + empty_both.count
        }
    })
}
