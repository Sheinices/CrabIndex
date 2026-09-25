//! crab-trackers-c - lostfilm, animelayer, anidub, anistar, baibako.
#![allow(non_snake_case)]

mod common;

pub mod anidub;
pub mod animelayer;
pub mod anistar;
pub mod baibako;
pub mod lostfilm;

/// One-time registration (id extractors). None of these trackers has a resumable ParseAll cycle.
pub fn init() {
    crab_core::fdb::register_id_extractor(lostfilm::parser::TRACKER, lostfilm::parser::stable_url_id);
}

/// HTTP routes owned by this crate (paths registered in lowercase).
pub fn router() -> axum::Router {
    axum::Router::new()
        .merge(lostfilm::router())
        .merge(animelayer::router())
        .merge(anidub::router())
        .merge(anistar::router())
        .merge(baibako::router())
}
