//! crab-trackers-a - rutor, megapeer, torrentby, kinozal and nnmclub crawlers
//! (parsers, category maps, task maps, sync jobs and `/cron/<tracker>/<action>` routes).
#![allow(non_snake_case)]

use std::sync::Arc;

pub mod common;
pub mod kinozal;
pub mod megapeer;
pub mod nnmclub;
pub mod rutor;
pub mod torrentby;

pub use common::set_recycle_session_hook;

/// One-time registration (ParseAll starters, url id extractors). Called by the server at startup.
pub fn init() {
    use crab_core::trackers::register_parse_all_starter;
    register_parse_all_starter(Arc::new(torrentby::Starter));
    register_parse_all_starter(Arc::new(megapeer::Starter));
    register_parse_all_starter(Arc::new(rutor::Starter));
    register_parse_all_starter(Arc::new(nnmclub::Starter));
    register_parse_all_starter(Arc::new(kinozal::Starter));
    crab_core::fdb::register_id_extractor(kinozal::TRACKER_NAME, kinozal::url_id);
}

/// HTTP routes owned by this crate (paths registered in lowercase).
pub fn router() -> axum::Router {
    axum::Router::new()
        .merge(rutor::router())
        .merge(megapeer::router())
        .merge(torrentby::router())
        .merge(kinozal::router())
        .merge(nnmclub::router())
}
