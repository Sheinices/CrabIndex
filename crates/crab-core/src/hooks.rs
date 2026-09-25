//! Late-bound integration points so leaf crates can plug into core without cycles.
//!
//! The tracks module (`crab-tracks`) registers a [`TracksLookup`] at startup; FileDB
//! (`update_full_details`), search and sync use it through the free functions below
//! (stream lookup, languages, excluded types, track presence).

use indexmap::IndexSet;
use once_cell::sync::OnceCell;
use std::sync::Arc;

use crate::models::{FfStream, TorrentDetails};

pub trait TracksLookup: Send + Sync {
    /// Cached ffprobe streams for a magnet (memory, then disk unless `memory_only`).
    fn get(&self, magnet: &str, types: &[String], memory_only: bool) -> Option<Vec<FfStream>>;
    /// Audio languages from streams + torrent.
    fn languages(&self, t: &TorrentDetails, streams: Option<&[FfStream]>) -> Option<IndexSet<String>>;
    /// Types excluded from track analysis.
    fn the_bad(&self, types: &[String]) -> bool;
    /// Whether track data exists for the torrent.
    fn has_track_for_torrent(&self, t: &TorrentDetails) -> bool;
}

static TRACKS: OnceCell<Arc<dyn TracksLookup>> = OnceCell::new();

pub fn register_tracks(lookup: Arc<dyn TracksLookup>) {
    let _ = TRACKS.set(lookup);
}

pub fn tracks_get(magnet: &str, types: &[String]) -> Option<Vec<FfStream>> {
    tracks_get_ex(magnet, types, false)
}

pub fn tracks_get_ex(magnet: &str, types: &[String], memory_only: bool) -> Option<Vec<FfStream>> {
    if magnet.is_empty() {
        return None;
    }
    TRACKS.get().and_then(|t| t.get(magnet, types, memory_only))
}

pub fn tracks_languages(t: &TorrentDetails, streams: Option<&[FfStream]>) -> Option<IndexSet<String>> {
    match TRACKS.get() {
        Some(h) => h.languages(t, streams),
        None => (!t.languages.is_empty()).then(|| t.languages.clone()),
    }
}

pub fn tracks_the_bad(types: &[String]) -> bool {
    TRACKS.get().map(|h| h.the_bad(types)).unwrap_or(false)
}

pub fn tracks_has_track_for_torrent(t: &TorrentDetails) -> bool {
    if t.ffprobe.as_ref().map(|f| !f.is_empty()).unwrap_or(false) {
        return true;
    }
    TRACKS.get().map(|h| h.has_track_for_torrent(t)).unwrap_or(false)
}
