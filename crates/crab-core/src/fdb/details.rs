//! Derive size/quality/videotype/voices/languages/seasons from the title.

#[path = "voices.rs"]
mod voices;

use indexmap::IndexSet;
use once_cell::sync::Lazy;

use crate::models::TorrentDetails;
use crate::rx;

static ALL_VOICES_LOWER: Lazy<Vec<(&'static str, String)>> =
    Lazy::new(|| voices::ALL_VOICES.iter().filter(|v| v.chars().count() > 4).map(|v| (*v, v.to_lowercase())).collect());

pub fn all_voices() -> &'static [&'static str] {
    voices::ALL_VOICES
}
pub fn rus_voices() -> &'static [&'static str] {
    voices::RUS_VOICES
}
pub fn ukr_voices() -> &'static [&'static str] {
    voices::UKR_VOICES
}

fn size_from_name(size_name: &str) -> i64 {
    if size_name.trim().is_empty() {
        return 0;
    }
    let g = rx::groups_i(size_name, r"([0-9\.,]+) (Mb|МБ|GB|ГБ|TB|ТБ)");
    if g[2].is_empty() {
        return 0;
    }
    let Ok(mut size) = g[1].replace(',', ".").parse::<f64>() else { return 0 };
    if size == 0.0 {
        return 0;
    }
    let unit = g[2].to_lowercase();
    if unit == "gb" || unit == "гб" {
        size *= 1024.0;
    }
    if unit == "tb" || unit == "тб" {
        size *= 1_048_576.0;
    }
    (size * 1_048_576.0) as i64
}

const DUB_RE: &str = "( |x)(d|dub|дб|дуб|дубляж)(,| )";

pub fn update_full_details(t: &mut TorrentDetails) {
    t.size = size_from_name(&t.sizeName) as f64;

    // quality
    t.quality = 480;
    if t.title.contains("720p") {
        t.quality = 720;
    } else if t.title.contains("1080p") {
        t.quality = 1080;
    } else if rx::is_match(&t.title.to_lowercase(), r"(4k|uhd)( |\]|,|$)") || t.title.contains("2160p") {
        t.quality = 2160;
    }

    let titlelower = t.title.to_lowercase();

    // videotype
    t.videotype = "sdr".into();
    if (rx::is_match(&titlelower, r"(\[|,| )hdr(10| |\]|,|$)") || rx::is_match(&titlelower, "(10-bit|10 bit|10-бит|10 бит|hdr10)"))
        && !rx::is_match(&titlelower, r"(\[|,| )sdr( |\]|,|$)")
    {
        t.videotype = "hdr".into();
    }

    // voices
    t.voices = IndexSet::new();
    if t.trackerName == "lostfilm" {
        t.voices.insert("LostFilm".into());
    } else if t.trackerName == "hdrezka" {
        t.voices.insert("HDRezka".into());
    }
    if rx::is_match(&titlelower, DUB_RE) {
        t.voices.insert("Дубляж".into());
    }
    for (v, vl) in ALL_VOICES_LOWER.iter() {
        if titlelower.contains(vl.as_str()) {
            t.voices.insert((*v).to_string());
        }
    }
    if let Some(streams) = crate::hooks::tracks_get(&t.magnet, &t.types) {
        for s in streams.iter().filter(|s| s.codec_type.as_deref() == Some("audio")) {
            let Some(title) = s.tags.as_ref().and_then(|tg| tg.title.as_deref()).filter(|x| !x.is_empty()) else {
                continue;
            };
            let tl = title.to_lowercase();
            for (v, vl) in ALL_VOICES_LOWER.iter() {
                if tl.contains(vl.as_str()) {
                    t.voices.insert((*v).to_string());
                }
            }
            if rx::is_match(&tl, DUB_RE) {
                t.voices.insert("Дубляж".into());
            }
        }
    }

    // languages
    t.languages = IndexSet::new();
    if titlelower.contains("ukr") || titlelower.contains("українськ") || titlelower.contains("украинск") || t.trackerName == "toloka" {
        t.languages.insert("ukr".into());
    }
    if t.trackerName == "lostfilm" {
        t.languages.insert("rus".into());
    }
    if !t.languages.contains("ukr") && voices::UKR_VOICES.iter().any(|v| t.voices.contains(*v)) {
        t.languages.insert("ukr".into());
    }
    if !t.languages.contains("rus") && voices::RUS_VOICES.iter().any(|v| t.voices.contains(*v)) {
        t.languages.insert("rus".into());
    }

    // seasons
    t.seasons = IndexSet::new();
    let serial_like = ["serial", "multserial", "docuserial", "tvshow", "anime"].iter().any(|x| t.has_type(x));
    if !serial_like {
        return;
    }
    let title = t.title.clone();
    let seasons = &mut t.seasons;
    let pi = |p: &str, i: usize| rx::group_i(&title, p, i).parse::<i32>().unwrap_or(0);

    if !rx::is_match_i(&title, r"([0-9]+(\-[0-9]+)?x[0-9]+|сезон|s[0-9]+)") {
        return;
    }
    if rx::is_match_i(&title, r"([0-9]+\-[0-9]+x[0-9]+|[0-9]+\-[0-9]+ сезон|s[0-9]+\-[0-9]+)") {
        let (mut start, mut end) = (0, 0);
        if rx::is_match_i(&title, "[0-9]+x[0-9]+") {
            start = pi(r"([0-9]+)\-([0-9]+)x", 1);
            end = pi(r"([0-9]+)\-([0-9]+)x", 2);
        } else if rx::is_match_i(&title, "[0-9]+ сезон") {
            start = pi(r"([0-9]+)\-([0-9]+) сезон", 1);
            end = pi(r"([0-9]+)\-([0-9]+) сезон", 2);
        } else if rx::is_match_i(&title, "s[0-9]+") {
            start = pi(r"s([0-9]+)\-([0-9]+)", 1);
            end = pi(r"s([0-9]+)\-([0-9]+)", 2);
        }
        if start > 0 && end > start {
            for s in start..=end {
                seasons.insert(s);
            }
        }
    }
    if rx::is_match_i(&title, "[0-9]+ сезон") {
        let s = pi("([0-9]+) сезон", 1);
        if s > 0 {
            seasons.insert(s);
        }
    } else if rx::is_match_i(&title, r"сезон(ы|и)?:? [0-9]+\-[0-9]+") {
        let start = pi(r"сезон(ы|и)?:? ([0-9]+)\-([0-9]+)", 2);
        let end = pi(r"сезон(ы|и)?:? ([0-9]+)\-([0-9]+)", 3);
        if start > 0 && end > start {
            for s in start..=end {
                seasons.insert(s);
            }
        }
    } else if rx::is_match_i(&title, "[0-9]+x[0-9]+") {
        let s = pi("([0-9]+)x", 1);
        if s > 0 {
            seasons.insert(s);
        }
    } else if rx::is_match_i(&title, "сезон(ы|и)?:? [0-9]+") {
        let s = pi("сезон(ы|и)?:? ([0-9]+)", 2);
        if s > 0 {
            seasons.insert(s);
        }
    } else if rx::is_match_i(&title, "s[0-9]+") {
        let s = pi("s([0-9]+)", 1);
        if s > 0 {
            seasons.insert(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_from_title() {
        let mut t = TorrentDetails::new("rutor", &["serial"], "u", "Сериал / Show [S01-03] (2020) WEB-DL 1080p | LostFilm");
        t.sizeName = "10.5 GB".into();
        update_full_details(&mut t);
        assert_eq!(t.quality, 1080);
        assert_eq!(t.size, (10.5 * 1024.0 * 1_048_576.0) as i64 as f64);
        assert!(t.voices.contains("LostFilm"));
        assert!(t.languages.contains("rus"));
        assert_eq!(t.seasons.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn season_word() {
        let mut t = TorrentDetails::new("x", &["serial"], "u", "Сериал (2 сезон) 720p");
        update_full_details(&mut t);
        assert_eq!(t.seasons.iter().copied().collect::<Vec<_>>(), vec![2]);
        assert_eq!(t.quality, 720);
    }
}
