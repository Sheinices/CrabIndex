//! Background workers owned by the server: FastDB index refresh, FileDB cron and config hot reload.

use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crab_core::log::{self, cat};

async fn rebuild_index(context: &'static str) {
    if let Err(e) = tokio::task::spawn_blocking(crab_core::index::rebuild).await {
        log::error("fastdb", format!("{context}: {e}"));
    }
}

/// Rebuilds the search-name index at startup and every 10 minutes.
pub fn spawn_fastdb_refresh(ct: CancellationToken) {
    tokio::spawn(async move {
        println!("fastdb worker started");
        rebuild_index("fastdb startup rebuild").await;
        loop {
            tokio::select! {
                _ = ct.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(600)) => {}
            }
            rebuild_index("fastdb periodic rebuild").await;
        }
    });
}

/// FileDB cache eviction / masterDb persistence loops.
pub fn spawn_filedb(ct: CancellationToken) {
    println!("fdb worker started");
    let c1 = ct.clone();
    tokio::spawn(async move {
        if let Err(e) = tokio::spawn(crab_core::fdb::cron(c1)).await {
            log::error(cat::FDB, format!("fdb cron worker terminated unexpectedly: {e}"));
        }
    });
    tokio::spawn(async move {
        if let Err(e) = tokio::spawn(crab_core::fdb::cron_fast(ct)).await {
            log::error(cat::FDB, format!("fdb cron fast worker terminated unexpectedly: {e}"));
        }
    });
}

/// Polls init.yaml / init.conf every 10 seconds for hot reload.
pub fn spawn_config_reload(ct: CancellationToken) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = ct.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(10)) => {}
            }
            let _ = tokio::task::spawn_blocking(|| crab_core::config::refresh_if_changed(None)).await;
        }
    });
}
