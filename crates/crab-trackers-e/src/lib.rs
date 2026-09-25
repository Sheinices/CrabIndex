//! crab-trackers-e - ultradox, knaben, rudub and subsplease trackers.
#![allow(non_snake_case)]

pub mod common;
pub mod knaben;
pub mod rudub;
pub mod subsplease;
pub mod ultradox;

/// One-time registration (id extractors, ParseAll starters). Called by the server at startup.
pub fn init() {
    crab_core::trackers::register_parse_all_starter(ultradox::starter());
    crab_core::fdb::register_id_extractor(subsplease::TRACKER_NAME, subsplease::parser::torrent_id_from_url);
}

/// HTTP routes owned by this crate (paths registered in lowercase).
pub fn router() -> axum::Router {
    axum::Router::new()
        .merge(ultradox::router())
        .merge(knaben::router())
        .merge(rudub::router())
        .merge(subsplease::router())
}
