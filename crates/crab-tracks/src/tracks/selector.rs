//! Picks TorrServer file ids (1-based) most likely to be probeable video.

use super::models::TorrentFileStat;

const VIDEO_EXTENSIONS: [&str; 10] = [".mkv", ".mp4", ".avi", ".m2ts", ".ts", ".wmv", ".webm", ".mov", ".mpg", ".mpeg"];

const EXCLUDED_PATH_FRAGMENTS: [&str; 7] = [".sample.", "/sample/", "\\sample\\", "proof", "trailer", "preview", "screenshot"];

/// Up to `max_count` file ids ordered by probe priority (largest video first).
/// Falls back to the largest file, then to id 1.
pub fn select_file_ids(file_stats: Option<&[TorrentFileStat]>, max_count: usize) -> Vec<i32> {
    let Some(files) = file_stats.filter(|f| !f.is_empty()) else { return vec![1] };
    let limit = max_count.max(1);

    let mut ranked: Vec<&TorrentFileStat> = files
        .iter()
        .filter(|f| f.id > 0 && f.path.as_deref().map(|p| !p.is_empty()).unwrap_or(false))
        .filter(|f| is_video_candidate(f.path.as_deref().unwrap_or("")))
        .collect();
    ranked.sort_by(|a, b| {
        b.length
            .cmp(&a.length)
            .then_with(|| path_len(a).cmp(&path_len(b)))
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut ids: Vec<i32> = Vec::new();
    for f in ranked {
        if !ids.contains(&f.id) {
            ids.push(f.id);
            if ids.len() >= limit {
                break;
            }
        }
    }
    if !ids.is_empty() {
        return ids;
    }

    let mut all: Vec<&TorrentFileStat> = files.iter().filter(|f| f.id > 0).collect();
    all.sort_by(|a, b| b.length.cmp(&a.length).then_with(|| a.id.cmp(&b.id)));
    match all.first() {
        Some(f) => vec![f.id],
        None => vec![1],
    }
}

fn path_len(f: &TorrentFileStat) -> usize {
    f.path.as_deref().map(|p| p.encode_utf16().count()).unwrap_or(0)
}

pub fn is_video_candidate(path: &str) -> bool {
    if crab_core::util::is_blank(path) {
        return false;
    }
    let lower = path.replace('\\', "/").to_lowercase();
    if EXCLUDED_PATH_FRAGMENTS.iter().any(|f| lower.contains(f)) {
        return false;
    }
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    match file.rfind('.') {
        Some(i) if i + 1 < file.len() => VIDEO_EXTENSIONS.contains(&&file[i..]),
        _ => false,
    }
}
