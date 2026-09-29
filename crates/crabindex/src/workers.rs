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

/// Global (tracker, id) → bucket index: first scan 30 s after start, then rebuilt daily to
/// drop entries of rows that migrations removed.
pub fn spawn_id_index(ct: CancellationToken) {
    tokio::spawn(async move {
        tokio::select! {
            _ = ct.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_secs(30)) => {}
        }
        loop {
            let started = std::time::Instant::now();
            let n = tokio::task::spawn_blocking(crab_core::fdb::id_index::build).await.unwrap_or(0);
            log::info(cat::FDB, format!("id index: {n} rows in {:.0}s", started.elapsed().as_secs_f64()));
            tokio::select! {
                _ = ct.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(24 * 3600)) => {}
            }
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

/// Refreshes the GitHub release check in the background (every 3 hours, after a short
/// startup delay) so the panel's "update available" indicator is current without anyone
/// opening the Update page. The result is cached in [`crate::update`]; failures are ignored.
pub fn spawn_update_check(ct: CancellationToken) {
    tokio::spawn(async move {
        tokio::select! {
            _ = ct.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_secs(60)) => {}
        }
        loop {
            let _ = crate::update::check(true).await;
            tokio::select! {
                _ = ct.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(3 * 3600)) => {}
            }
        }
    });
}
