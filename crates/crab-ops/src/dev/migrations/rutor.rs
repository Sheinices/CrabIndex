//! rutor: rows from the movie categories that are actually series (season marker in the title,
//! typically cat 17 "Иностранные релизы" with UKR dubs) were stored as `movie` with an "[S01]"
//! tail in `originalname`, so serial card searches never saw them. Retype them to `serial`,
//! re-derive the names with the serial title patterns, recompute details (seasons) and move
//! rows whose bucket key changed. The parser itself now types such rows correctly; this only
//! heals rows parsed before that.

use std::cell::Cell;

use crab_core::parsing::has_season_marker;
use crab_core::{fdb, util};
use serde_json::{json, Value};

use super::names::walk;
use super::parsers::rutor;
use super::try_rebuild_fast_db;

pub fn fix_serial_types() -> Value {
    let retyped = Cell::new(0i64);
    let (processed, migrated) = walk(
        "rutor",
        |t| {
            let movie_only = t.types.len() == 1 && t.types[0] == "movie";
            if !movie_only || !has_season_marker(&t.title) {
                return false;
            }
            let (name, originalname, relased) = rutor::parse_serial_title(&t.title);
            if !util::is_blank(&name) {
                t.name = name;
            }
            if !util::is_blank(&originalname) {
                t.originalname = originalname;
            }
            if relased > 0 {
                t.relased = relased;
            }
            t._sn = util::search_name_or_empty(&t.name);
            t._so = util::search_name_or_empty(&t.originalname);
            t.types = vec!["serial".to_string()];
            // Seasons are only parsed for serial-like rows, so recompute now that the type changed.
            fdb::update_full_details(t);
            retyped.set(retyped.get() + 1);
            true
        },
        |changed, _| changed,
    );
    fdb::save_changes_to_file();
    try_rebuild_fast_db();
    json!({ "ok": true, "processed": processed, "retyped": retyped.get(), "migrated": migrated })
}
