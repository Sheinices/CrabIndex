//! crab-trackers-b - rutracker, toloka, mazepa, selezen and bitru sync.
#![allow(non_snake_case)]

use std::sync::Arc;

pub mod bitru;
pub mod common;
pub mod mazepa;
pub mod rutracker;
pub mod selezen;
pub mod toloka;

/// One-time registration (ParseAll starters for rutracker and toloka). Called at startup.
pub fn init() {
    crab_core::trackers::register_parse_all_starter(Arc::new(rutracker::Starter));
    crab_core::trackers::register_parse_all_starter(Arc::new(toloka::Starter));
}

/// HTTP routes owned by this crate (paths registered in lowercase).
pub fn router() -> axum::Router {
    axum::Router::new()
        .merge(rutracker::router())
        .merge(toloka::router())
        .merge(mazepa::router())
        .merge(selezen::router())
        .merge(bitru::router())
}
