// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Series stored as films. rutor (cat 17 "Иностранные релизы"), torrentby (`films`) and selezen
//! list series in their film sections; the old parsers typed those rows `movie` and, using the
//! film title patterns, kept an "[S01]" tail in `originalname` - so serial card searches never
//! saw them and matching by the original title failed. selezen's serial rows carried the same
//! tail even when typed correctly.
//!
//! For rows of those trackers whose title has a season marker: retype `movie` rows to `serial`,
//! re-derive the names with the serial title patterns (or at least strip the bracket block from
//! `originalname`), recompute details (seasons), move rows whose bucket key changed and bump
//! `updateTime` so sync clients pick the fix up. The parsers now do this at parse time; this
//! only heals rows parsed before.

use std::cell::Cell;

use crab_core::parsing::has_season_marker;
use crab_core::{fdb, rx, time, util};
use serde_json::{json, Value};

use super::names::walk;
use super::parsers::serial_title;
use super::try_rebuild_fast_db;

const TRACKERS: [&str; 3] = ["rutor", "torrentby", "selezen"];

pub fn fix_serial_types() -> Value {
    let retyped = Cell::new(0i64);
    let renamed = Cell::new(0i64);
    let mut per_tracker = serde_json::Map::new();
    let (mut processed_all, mut migrated_all) = (0i64, 0i64);

    for tracker in TRACKERS {
        let (before_retyped, before_renamed) = (retyped.get(), renamed.get());
        let (processed, migrated) = walk(
            tracker,
            |t| {
                if !has_season_marker(&t.title) {
                    return false;
                }
                let movie_only = t.types.len() == 1 && t.types[0] == "movie";
                let tail_in_orig = t.originalname.contains('[');
                if !movie_only && !tail_in_orig {
                    return false;
                }

                let (name, originalname, relased) = serial_title::parse(&t.title);
                if !util::is_blank(&name) {
                    t.name = name;
                }
                if !util::is_blank(&originalname) {
                    t.originalname = originalname;
                } else if tail_in_orig {
                    t.originalname = rx::replace(&t.originalname, r"\s*\[[^\]]*\]", "").trim().to_string();
                }
                if relased > 0 {
                    t.relased = relased;
                }
                t._sn = util::search_name_or_empty(&t.name);
                t._so = util::search_name_or_empty(&t.originalname);

                if movie_only {
                    t.types = vec!["serial".to_string()];
                    retyped.set(retyped.get() + 1);
                } else {
                    renamed.set(renamed.get() + 1);
                }
                // Seasons are only parsed for serial-like rows; recompute now that type/names changed.
                fdb::update_full_details(t);
                // Sync clients only receive rows newer than their cursor (`/sync/fdb/torrents`), so a
                // fix made on the sync server must bump updateTime or it never leaves this host.
                t.updateTime = time::now();
                true
            },
            |changed, _| changed,
        );
        processed_all += processed;
        migrated_all += migrated;
        per_tracker.insert(
            tracker.to_string(),
            json!({ "processed": processed, "retyped": retyped.get() - before_retyped, "renamed": renamed.get() - before_renamed, "migrated": migrated }),
        );
    }

    fdb::save_changes_to_file();
    try_rebuild_fast_db();
    json!({
        "ok": true,
        "processed": processed_all,
        "retyped": retyped.get(),
        "renamed": renamed.get(),
        "migrated": migrated_all,
        "trackers": per_tracker,
    })
}
