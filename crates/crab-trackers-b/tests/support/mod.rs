#![allow(dead_code)]

/// Read `tests/fixtures/{relative}`.
pub fn read(relative: &str) -> String {
    assert!(!relative.starts_with('/'), "fixture path must be relative: {relative}");
    let path = format!("{}/tests/fixtures/{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture missing: {path}: {e}"))
}
