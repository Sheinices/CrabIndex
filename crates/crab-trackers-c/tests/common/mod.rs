// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Fixture loading for integration tests (`tests/fixtures/<tracker>/<file>`).

pub fn read(relative: &str) -> String {
    assert!(!std::path::Path::new(relative).is_absolute(), "Fixture path must be relative: {relative}");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(relative);
    assert!(path.exists(), "Fixture missing: {}", path.display());
    std::fs::read_to_string(&path).expect("fixture readable")
}
