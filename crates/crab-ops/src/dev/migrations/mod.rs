//! One-off FileDB data migrations exposed under `/dev/*`.

pub mod aniliberty;
pub mod animelayer;
pub mod cleanup;
pub mod domain_dups;
pub mod names;
pub mod parsers;

use crab_core::fdb;
use crab_core::models::TorrentDetails;

/// Rebuild the fast search index, ignoring failures.
pub fn try_rebuild_fast_db() {
    let _ = std::panic::catch_unwind(crab_core::index::rebuild);
}

/// Move rows into their new buckets (after the source shard dropped them).
pub(crate) fn migrate_all(list: Vec<(TorrentDetails, String)>) -> i64 {
    let n = list.len() as i64;
    for (t, nk) in list {
        fdb::migrate_torrent_to_new_key(&t, &nk);
    }
    n
}
