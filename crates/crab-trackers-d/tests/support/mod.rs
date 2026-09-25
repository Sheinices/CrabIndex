//! Fixture loading for parser tests.
#![allow(dead_code)]

pub fn read(rel: &str) -> String {
    assert!(!std::path::Path::new(rel).is_absolute(), "fixture path must be relative: {rel}");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture missing: {} ({e})", path.display()))
}
