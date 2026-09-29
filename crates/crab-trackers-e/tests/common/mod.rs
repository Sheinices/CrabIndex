// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

/// Read a fixture under `tests/fixtures/` (path must be relative).
pub fn fixture(relative: &str) -> String {
    assert!(!std::path::Path::new(relative).is_absolute(), "Fixture path must be relative: {relative}");
    let path = format!("{}/tests/fixtures/{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Fixture missing: {path}: {e}"))
}
