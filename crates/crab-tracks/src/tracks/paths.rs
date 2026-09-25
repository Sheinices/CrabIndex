//! Track file layout under `Data/tracks`: `{aa}/{b}/{rest}.json` (lowercase hex infohash).
//! Legacy files without `.json` and uppercase layouts are read as a fallback and migrated on backfill.

use std::path::{Component, Path, PathBuf};

pub const TRACKS_DIR: &str = "Data/tracks";

pub fn normalize_infohash(infohash: &str) -> String {
    infohash.to_lowercase()
}

pub fn is_valid_infohash(infohash: &str) -> bool {
    infohash.len() == 40 && infohash.bytes().all(|c| c.is_ascii_hexdigit())
}

fn join(dir: &str, parts: &[&str]) -> String {
    let mut s = dir.trim_end_matches('/').to_string();
    for p in parts {
        if !s.is_empty() {
            s.push('/');
        }
        s.push_str(p);
    }
    s
}

/// Canonical path. `None` for an invalid infohash.
pub fn track_layout_path(tracks_dir: &str, infohash: &str, with_extension: bool) -> Option<String> {
    let h = normalize_infohash(infohash);
    if !is_valid_infohash(&h) {
        return None;
    }
    let file = if with_extension { format!("{}.json", &h[3..]) } else { h[3..].to_string() };
    Some(join(tracks_dir, &[&h[..2], &h[2..3], &file]))
}

/// Legacy uppercase layout (read fallback only).
pub fn uppercase_layout_path(tracks_dir: &str, infohash: &str, with_extension: bool) -> Option<String> {
    let h = normalize_infohash(infohash);
    if !is_valid_infohash(&h) {
        return None;
    }
    let u = h.to_uppercase();
    let file = if with_extension { format!("{}.json", &u[3..]) } else { u[3..].to_string() };
    Some(join(tracks_dir, &[&u[..2], &u[2..3], &file]))
}

/// Canonical path under `Data/tracks`, optionally creating its folder.
pub fn path_db(infohash: &str, create_folder: bool) -> Option<String> {
    let p = track_layout_path(TRACKS_DIR, infohash, true)?;
    if create_folder {
        if let Some(dir) = Path::new(&p).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    Some(p)
}

pub fn is_legacy_track_file(filename: &str) -> bool {
    !ends_with_json(filename)
}

fn ends_with_json(filename: &str) -> bool {
    filename.len() >= 5 && filename.is_char_boundary(filename.len() - 5) && filename[filename.len() - 5..].eq_ignore_ascii_case(".json")
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// A legacy (extension-less) file is skipped when a `.json` sibling or the canonical file exists.
pub fn should_skip_legacy_track_file(folder2: &Path, filename: &str, tracks_dir: &str) -> bool {
    if !is_legacy_track_file(filename) {
        return false;
    }
    if folder2.join(format!("{filename}.json")).exists() {
        return true;
    }
    let folder1 = folder2.parent().map(file_name).unwrap_or_default();
    let infohash = infohash_from_track_rel_path(&folder1, &file_name(folder2), filename);
    if is_valid_infohash(&infohash) {
        if let Some(p) = track_layout_path(tracks_dir, &infohash, true) {
            if Path::new(&p).exists() {
                return true;
            }
        }
    }
    false
}

pub fn resolve_track_json_path(infohash: &str, tracks_dir: &str) -> Option<String> {
    let p = track_layout_path(tracks_dir, infohash, true)?;
    if Path::new(&p).is_file() {
        return Some(p);
    }
    let p = uppercase_layout_path(tracks_dir, infohash, true)?;
    Path::new(&p).is_file().then_some(p)
}

pub fn resolve_legacy_track_path(infohash: &str, tracks_dir: &str) -> Option<String> {
    let p = track_layout_path(tracks_dir, infohash, false)?;
    if Path::new(&p).is_file() {
        return Some(p);
    }
    let p = uppercase_layout_path(tracks_dir, infohash, false)?;
    Path::new(&p).is_file().then_some(p)
}

/// Existing track file for a hash (canonical `.json`, uppercase, then legacy).
pub fn resolve_track_path(infohash: &str) -> Option<String> {
    resolve_track_json_path(infohash, TRACKS_DIR).or_else(|| resolve_legacy_track_path(infohash, TRACKS_DIR))
}

pub fn infohash_from_track_rel_path(prefix2: &str, prefix1: &str, filename: &str) -> String {
    let stem = if ends_with_json(filename) { &filename[..filename.len() - 5] } else { filename };
    normalize_infohash(&format!("{prefix2}{prefix1}{stem}"))
}

/// Sorted subdirectories (errors → empty).
pub fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    };
    v.sort();
    v
}

/// Sorted regular files (errors → empty).
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.flatten().filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false)).map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    };
    v.sort();
    v
}

/// One track file found while walking `{dir}/{aa}/{b}/*`.
pub struct TrackEntry {
    pub path: PathBuf,
    /// Normalized infohash derived from the relative path (may be invalid).
    pub infohash: String,
    /// Legacy file shadowed by a `.json` twin - callers skip it.
    pub skip: bool,
}

/// Walk the two-level layout and call `f` for every file.
pub fn walk_tracks_dir(tracks_dir: &str, mut f: impl FnMut(TrackEntry)) {
    let root = Path::new(tracks_dir);
    if !root.is_dir() {
        return;
    }
    for folder1 in subdirs(root) {
        let f1 = file_name(&folder1);
        for folder2 in subdirs(&folder1) {
            let f2 = file_name(&folder2);
            for file in files(&folder2) {
                let name = file_name(&file);
                let skip = should_skip_legacy_track_file(&folder2, &name, tracks_dir);
                let infohash = infohash_from_track_rel_path(&f1, &f2, &name);
                f(TrackEntry { path: file, infohash, skip });
            }
        }
    }
}

/// Legacy → canonical `.json`, uppercase/mixed → lowercase layout. Returns the number of files handled.
pub fn migrate_track_layout_in_place(tracks_dir: &str, dry_run: bool) -> i32 {
    let mut migrated = 0;
    let mut entries = Vec::new();
    walk_tracks_dir(tracks_dir, |e| entries.push(e));
    for e in entries {
        // Re-check skip now: earlier moves may have created canonical twins.
        let name = file_name(&e.path);
        let parent = e.path.parent().map(Path::to_path_buf).unwrap_or_default();
        if e.skip || should_skip_legacy_track_file(&parent, &name, tracks_dir) {
            continue;
        }
        if !is_valid_infohash(&e.infohash) {
            continue;
        }
        let Some(target) = track_layout_path(tracks_dir, &e.infohash, true) else { continue };
        if e.path.to_string_lossy() == target {
            continue;
        }
        let target_path = Path::new(&target);
        let name_differs = e.path.file_name() != target_path.file_name();
        if target_path.exists() && same_file(&e.path, target_path) && !name_differs {
            // Case-insensitive filesystem: only the folder casing differs - already canonical.
            continue;
        }
        if target_path.exists() && !same_file(&e.path, target_path) {
            if !dry_run {
                let _ = std::fs::remove_file(&e.path);
            }
            migrated += 1;
            continue;
        }
        if !dry_run {
            if let Some(dir) = target_path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::rename(&e.path, target_path).is_err() {
                continue;
            }
        }
        migrated += 1;
    }
    migrated
}

/// True when both paths point at the same inode (case-insensitive filesystems).
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Lexically normalized absolute path (relative paths resolved against cwd).
pub fn full_path(p: &str) -> Option<PathBuf> {
    let path = Path::new(p);
    let abs = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().ok()?.join(path) };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// `full_path` is `root` itself or inside it (case-insensitive).
pub fn is_path_within_directory(root: &str, full: &str) -> bool {
    let (Some(r), Some(p)) = (full_path(root), full_path(full)) else { return false };
    let r = r.to_string_lossy().trim_end_matches('/').to_lowercase();
    let p = p.to_string_lossy().trim_end_matches('/').to_lowercase();
    p == r || p.starts_with(&format!("{r}/"))
}

/// True when the first `streams` property in the file is a non-empty array.
pub fn track_file_has_streams(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else { return false };
    match find_streams(&v) {
        Some(serde_json::Value::Array(a)) => !a.is_empty(),
        _ => false,
    }
}

fn find_streams(v: &serde_json::Value) -> Option<&serde_json::Value> {
    match v {
        serde_json::Value::Object(m) => {
            for (k, val) in m {
                if k.eq_ignore_ascii_case("streams") {
                    return Some(val);
                }
                if let Some(x) = find_streams(val) {
                    return Some(x);
                }
            }
            None
        }
        serde_json::Value::Array(a) => a.iter().find_map(find_streams),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "aabbccddeeff00112233445566778899aabbccdd";

    #[test]
    fn layout() {
        assert_eq!(track_layout_path("Data/tracks", H, true).unwrap(), "Data/tracks/aa/b/bccddeeff00112233445566778899aabbccdd.json");
        assert_eq!(uppercase_layout_path("x", H, false).unwrap(), "x/AA/B/BCCDDEEFF00112233445566778899AABBCCDD");
        assert!(track_layout_path("x", "zz", true).is_none());
        assert_eq!(infohash_from_track_rel_path("AA", "B", "BCCDDEEFF00112233445566778899AABBCCDD.JSON"), H);
    }

    #[test]
    fn within_dir() {
        assert!(is_path_within_directory("Data", "Data/tracks-export"));
        assert!(is_path_within_directory("Data", "Data"));
        assert!(!is_path_within_directory("Data", "Data/../etc"));
        assert!(!is_path_within_directory("Data", "/etc"));
        assert!(!is_path_within_directory("Data", "DataX"));
    }
}
