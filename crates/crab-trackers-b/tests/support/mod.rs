#![allow(dead_code)]
// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors


/// Read `tests/fixtures/{relative}`.
pub fn read(relative: &str) -> String {
    assert!(!relative.starts_with('/'), "fixture path must be relative: {relative}");
    let path = format!("{}/tests/fixtures/{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture missing: {path}: {e}"))
}
