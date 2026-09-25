//! Tracker infrastructure shared by all tracker crates.
pub mod cycle;
pub mod registry;
pub mod sync;

pub use cycle as parse_all;
pub use registry::{parse_all_starters, register_parse_all_starter, ParseAllStarter};
pub use sync::*;

/// Read `Data/temp/{tracker}_taskParse.json` style JSON (plain, not gzip). None on error.
pub fn read_json_file<T: serde::de::DeserializeOwned>(path: &str) -> Option<T> {
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()
}
