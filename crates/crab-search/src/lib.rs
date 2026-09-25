//! crab-search - torrent search APIs over FileDB: Jackett JSON, Torznab/Newznab XML,
//! Prowlarr Search Feed and the native `/api/v1.0/*` endpoints, plus the Alloha
//! external-id title resolver.
#![allow(non_snake_case)]

pub mod alloha;
pub mod cache;
pub mod indexers;
pub mod magnet;
pub mod query;
pub mod routes;
pub mod search;

/// One-time registration. Nothing to register: state lives in lazily initialised statics.
pub fn init() {}

/// HTTP routes owned by this crate (paths registered in lowercase).
pub fn router() -> axum::Router {
    routes::router()
}

/// Install default configuration for unit tests (avoids loading a config file from disk).
#[cfg(test)]
pub(crate) fn test_conf() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| crab_core::config::set_current(crab_core::config::AppOptions::default()));
}
