//! CrabIndex server entry point.
//!
//! `crabindex` - run the HTTP server.
//! `crabindex maintain [...]` - headless FileDB maintenance (exit code = result).
//! `crabindex admin` - print the admin panel entry URL and devkey, then exit.

mod admin;
mod app;
mod config_api;
mod controllers;
mod normalize;
mod openapi;
mod security;
mod static_files;
mod version;
mod waf;
#[cfg(test)]
mod test_support;
mod workers;

use crab_core::log::{self, cat};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Current configuration (loads init.yaml / init.conf on first use).
pub fn conf() -> std::sync::Arc<crab_core::config::AppOptions> {
    crab_core::conf()
}

/// How long in-flight requests may take to finish after a shutdown signal.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a.eq_ignore_ascii_case("maintain")).unwrap_or(false) {
        std::process::exit(crab_ops::maintain_cli(&args));
    }
    if args.first().map(|a| a.eq_ignore_ascii_case("admin")).unwrap_or(false) {
        std::process::exit(admin::bootstrap::print_cli());
    }

    version::print_banner();
    crab_core::ensure_data_dirs();
    install_panic_hook();

    admin::api::mark_started();

    // Generates admin.token / devkey on first start (writes them into the config file).
    admin::bootstrap::run_startup();
    // Loads init.yaml / init.conf and applies logging settings.
    let conf = crate::conf();

    // masterDb must be fully loaded before the listener accepts requests.
    crab_core::fdb::init_master_db();

    // Loads Data/waf.json (blacklist, whitelist, bans).
    waf::init();

    for err in security::registry::verify_registry() {
        log::warn("security", format!("registry mismatch: {err}"));
    }

    let addr = match listen_addr(&conf.listenip, conf.listenport) {
        Ok(a) => a,
        Err(e) => {
            log::error(cat::HOST, format!("[fatal] invalid listenip '{}': {e}", conf.listenip));
            std::process::exit(1);
        }
    };
    drop(conf);

    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            log::error(cat::HOST, format!("[fatal] tokio runtime: {e}"));
            std::process::exit(1);
        }
    };
    let code = rt.block_on(run(addr));
    rt.shutdown_timeout(Duration::from_secs(5));
    std::process::exit(code);
}

fn listen_addr(listenip: &str, port: u16) -> Result<SocketAddr, std::net::AddrParseError> {
    let ip = if listenip.trim().is_empty() || listenip.eq_ignore_ascii_case("any") {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        listenip.trim().parse::<IpAddr>()?
    };
    Ok(SocketAddr::new(ip, port))
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        log::error(cat::HOST, format!("[fatal] panic in thread '{}': {info}", thread.name().unwrap_or("<unnamed>")));
    }));
}

fn init_modules() {
    crab_cloudflare::init();
    crab_trackers_a::init();
    // kinozal recycles its browser session after repeated stale pages
    crab_trackers_a::set_recycle_session_hook(std::sync::Arc::new(|host: String| {
        Box::pin(async move { crab_cloudflare::recycle_session(&host).await })
    }));
    crab_trackers_b::init();
    crab_trackers_c::init();
    crab_trackers_d::init();
    crab_trackers_e::init();
    crab_search::init();
    crab_tracks::init();
    crab_ops::init();
}

fn spawn_workers(ct: &CancellationToken) {
    workers::spawn_config_reload(ct.clone());
    waf::spawn_maintenance(ct.clone());
    workers::spawn_fastdb_refresh(ct.clone());
    workers::spawn_filedb(ct.clone());
    crab_tracks::spawn_workers(ct.clone());
    crab_ops::spawn_workers(ct.clone());
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

async fn run(addr: SocketAddr) -> i32 {
    init_modules();

    let ct = crab_core::trackers::app_stopping();
    spawn_workers(&ct);

    let app = app::build_app(app::api_router());
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            log::error(cat::HOST, format!("[fatal] cannot listen on {addr}: {e}"));
            ct.cancel();
            return 1;
        }
    };
    log::info(cat::HOST, format!("listening on http://{addr}"));

    {
        let ct = ct.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            log::info(cat::HOST, "shutdown requested");
            ct.cancel();
        });
    }

    let server = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown({
        let ct = ct.clone();
        async move { ct.cancelled().await }
    });

    let mut code = 0;
    tokio::select! {
        r = async move { server.await } => {
            if let Err(e) = r {
                log::error(cat::HOST, format!("[fatal] server error: {e}"));
                code = 1;
            }
        }
        _ = async { ct.cancelled().await; tokio::time::sleep(SHUTDOWN_TIMEOUT).await } => {
            log::warn(cat::HOST, "shutdown timeout: abandoning in-flight requests");
        }
    }
    ct.cancel();

    // Persist dirty FileDB shards and masterDb.
    if let Err(e) = tokio::task::spawn_blocking(crab_core::fdb::flush_all).await {
        log::error(cat::FDB, format!("flush on shutdown failed: {e}"));
        code = 1;
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_address() {
        assert_eq!(listen_addr("any", 9117).unwrap().to_string(), "0.0.0.0:9117");
        assert_eq!(listen_addr("127.0.0.1", 1).unwrap().to_string(), "127.0.0.1:1");
        assert_eq!(listen_addr("::1", 80).unwrap().to_string(), "[::1]:80");
        assert!(listen_addr("nope", 80).is_err());
    }
}
