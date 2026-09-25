//! CrabIndex core: configuration, models, FileDB, HTTP networking, parsing helpers
//! and tracker sync primitives shared by every other crate.
#![allow(non_snake_case)]

pub mod config;
pub mod fdb;
pub mod hooks;
pub mod index;
pub mod log;
pub mod models;
pub mod net;
pub mod parsing;
pub mod rx;
pub mod time;
pub mod trackers;
pub mod util;

pub use config::conf;

/// Create the Data/* directory layout used by the app (relative to cwd).
pub fn ensure_data_dirs() {
    for d in ["Data/fdb", "Data/temp", "Data/log", "Data/tracks"] {
        let _ = std::fs::create_dir_all(d);
    }
}
