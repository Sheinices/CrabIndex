//! crab-ops - sync API + sync workers, tracker announce list worker, FileDB save,
//! integrity maintenance, ParseAll resume, dev diagnostics/maintenance/migrations
//! and the headless `maintain` CLI.
#![allow(non_snake_case)]

pub mod cli;
pub mod db;
pub mod dev;
pub mod maintenance;
pub mod nulls;
pub mod query;
pub mod sync;
pub mod trackers_cron;

use tokio_util::sync::CancellationToken;

/// One-time registration. Nothing to register for this crate: the sync cache and the
/// last maintenance report are loaded lazily on first use.
pub fn init() {}

/// HTTP routes owned by this crate (paths registered in lowercase).
pub fn router() -> axum::Router {
    axum::Router::new()
        .merge(sync::router())
        .merge(db::router())
        .merge(maintenance::router())
        .merge(dev::router())
}

/// Start background workers: sync (torrents + spidr + sync cache refresh),
/// tracker announce list, ParseAll resume after startup.
pub fn spawn_workers(shutdown: CancellationToken) {
    sync::spawn_workers(shutdown.clone());
    trackers_cron::spawn_worker(shutdown.clone());
    maintenance::resume::spawn_worker(shutdown);
}

/// Headless `crabindex maintain [--mode=report|safe|full] …`; returns process exit code.
pub fn maintain_cli(args: &[String]) -> i32 {
    cli::run(args)
}

/// Cancellable sleep; returns false when `ct` fired first.
pub(crate) async fn sleep_ct(dur: std::time::Duration, ct: &CancellationToken) -> bool {
    tokio::select! {
        _ = ct.cancelled() => false,
        _ = tokio::time::sleep(dur) => true,
    }
}
