#![allow(dead_code)]

/// Read a fixture from `tests/fixtures/<relative>`.
pub fn fixture(relative: &str) -> String {
    assert!(!std::path::Path::new(relative).is_absolute(), "Fixture path must be relative: {relative}");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Fixture missing: {}: {e}", path.display()))
}

pub fn pages(n: i32) -> Vec<crab_core::models::TaskParse> {
    (0..n).map(crab_core::models::TaskParse::new).collect()
}

pub fn types(t: &crab_core::models::TorrentDetails) -> Vec<&str> {
    t.types.iter().map(|s| s.as_str()).collect()
}
