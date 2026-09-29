// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! rutracker rows parsed while the title patterns could not see through a nested director
//! block, e.g. `Матрица / The Matrix (Братья Вачовски (Энди, Ларри) / The Wachowski Brothers (...))
//! [1999, ...]`: they got `originalname` = the Russian name and `relased` = 0. Re-derive the
//! names with the shared title parser, move rows whose bucket key changed and bump `updateTime`
//! so sync clients pick the fix up. Rows whose parse result equals the stored fields are left
//! untouched.

use std::cell::Cell;

use crab_core::parsing::rutracker_title;
use crab_core::{fdb, time, util};
use serde_json::{json, Value};

use super::names::walk;
use super::try_rebuild_fast_db;

pub fn fix_rutracker_names() -> Value {
    let fixed = Cell::new(0i64);
    let (processed, migrated) = walk(
        "rutracker",
        |t| {
            let kind = rutracker_title::guess_kind(&t.title);
            let (name, originalname, relased, skip) = rutracker_title::parse(kind, &t.title);
            if skip {
                return false;
            }
            let Some(name) = name.filter(|n| !util::is_blank(n)) else { return false };
            let originalname = originalname.filter(|o| !util::is_blank(o)).unwrap_or_else(|| name.clone());
            // A movie-pattern title with no original name keeps `originalname == name`, which is
            // what the parser stores too - so only real differences count as a fix.
            let same_names = t.name == name && t.originalname == originalname;
            let same_year = relased == 0 || t.relased == relased;
            if same_names && same_year {
                return false;
            }
            t.name = name;
            t.originalname = originalname;
            if relased > 0 {
                t.relased = relased;
            }
            t._sn = util::search_name_or_empty(&t.name);
            t._so = util::search_name_or_empty(&t.originalname);
            fdb::update_full_details(t);
            t.updateTime = time::now();
            fixed.set(fixed.get() + 1);
            true
        },
        |changed, _| changed,
    );
    fdb::save_changes_to_file();
    try_rebuild_fast_db();
    json!({ "ok": true, "processed": processed, "fixed": fixed.get(), "migrated": migrated })
}
