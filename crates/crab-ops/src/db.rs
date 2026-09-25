//! `/jsondb/save` - persist masterDb in the background.

use axum::routing::any;
use axum::Router;
use crab_core::log::{self, cat};
use crab_core::{conf, fdb, util};
use std::sync::atomic::{AtomicBool, Ordering};

static SAVE_DB_WORK: AtomicBool = AtomicBool::new(false);

pub fn router() -> Router {
    Router::new().route("/jsondb/save", any(save))
}

/// Returns `syncapi` (instance is a sync client), `work` (save already running) or `ok`.
pub async fn save() -> String {
    if !util::is_blank(conf().syncapi.as_deref().unwrap_or("")) {
        return "syncapi".into();
    }
    if SAVE_DB_WORK.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return "work".into();
    }
    tokio::task::spawn_blocking(|| {
        let res = std::panic::catch_unwind(fdb::save_changes_to_file);
        match res {
            Ok(()) => log::info(cat::FDB, "jsondb/save completed (background)"),
            Err(_) => log::error(cat::FDB, "jsondb/save error: panic"),
        }
        SAVE_DB_WORK.store(false, Ordering::SeqCst);
    });
    "ok".into()
}
