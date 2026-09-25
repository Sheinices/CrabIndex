//! BitRu API category request filter and category/subsection → types mapping.

pub const REQUEST_CATEGORIES: [&str; 3] = ["movie", "serial", "video"];

/// Categories intentionally not requested or mapped.
pub const NON_VIDEO_IDS: [&str; 7] = ["music", "game", "soft", "literature", "audiobook", "image", "xxx"];

const DOCUMOVIE_SUBSECTIONS: [&str; 4] = ["Документальный", "Научный", "Исторический", "Биография"];
const SPORT_SUBSECTIONS: [&str; 1] = ["Спорт"];
const TV_SHOW_SUBSECTIONS: [&str; 3] = ["Шоу", "Клипы", "Концерт"];

fn eq_ci(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

pub fn try_get_types(category: Option<&str>, subsections: Option<&[String]>) -> Option<&'static [&'static str]> {
    let cat = category.unwrap_or("").trim().to_lowercase();
    if cat.is_empty() || NON_VIDEO_IDS.contains(&cat.as_str()) {
        return None;
    }
    match cat.as_str() {
        "movie" => Some(&["movie"]),
        "serial" => Some(&["serial"]),
        "video" => try_get_video_types(subsections),
        _ => None,
    }
}

fn try_get_video_types(subsections: Option<&[String]>) -> Option<&'static [&'static str]> {
    let subs = subsections.filter(|s| !s.is_empty())?;
    for sub in subs {
        if sub.trim().is_empty() {
            continue;
        }
        if DOCUMOVIE_SUBSECTIONS.iter().any(|x| eq_ci(x, sub)) {
            return Some(&["documovie"]);
        }
        if SPORT_SUBSECTIONS.iter().any(|x| eq_ci(x, sub)) {
            return Some(&["sport"]);
        }
        if TV_SHOW_SUBSECTIONS.iter().any(|x| eq_ci(x, sub)) {
            return Some(&["tvshow"]);
        }
    }
    // Трейлер, Эротика, Уроки, Детское, unknown → dropped
    None
}
