//! crab-trackers-d - anibelka, aniliberty, anifilm, leproduction, viruseproject, korsars.
#![allow(non_snake_case)]

pub mod common;

pub mod anibelka;
pub mod anifilm;
pub mod aniliberty;
pub mod korsars;
pub mod leproduction;
pub mod viruseproject;

use std::sync::Arc;

/// Slugs of the trackers in this crate that expose a resumable ParseAllTask.
pub const PARSE_ALL_TRACKERS: &[&str] = &["anibelka", "korsars"];

/// One-time registration (ParseAll starters). Called by the server at startup.
///
/// Torrent-id extraction from URLs for anibelka/korsars/leproduction/viruseproject is
/// built into FileDB; anifilm and aniliberty rows carry no numeric id.
pub fn init() {
    crab_core::trackers::register_parse_all_starter(Arc::new(anibelka::Starter));
    crab_core::trackers::register_parse_all_starter(Arc::new(korsars::Starter));
}

/// HTTP routes owned by this crate (paths registered in lowercase).
pub fn router() -> axum::Router {
    axum::Router::new()
        .merge(anibelka::router())
        .merge(aniliberty::router())
        .merge(anifilm::router())
        .merge(leproduction::router())
        .merge(viruseproject::router())
        .merge(korsars::router())
}
