//! Title/URL helpers of individual trackers needed by the data migrations.
//! Kept local so this crate does not depend on the tracker crates.

use chrono::{DateTime, Datelike, Utc};
use crab_core::{rx, time, util};

fn trim_end_chars(s: &str, chars: &[char]) -> String {
    s.trim_end_matches(|c| chars.contains(&c)).to_string()
}

pub mod knaben {
    use super::*;

    /// Strip metadata for the search key. Series: text before `S01E05`. Movies: year + metadata.
    pub fn clean_title_for_search(title: &str) -> String {
        if util::is_blank(title) {
            return title.to_string();
        }
        let mut t = title.trim().to_string();
        t = rx::replace(&t, r"\[[^\]]*\]", " ");
        let series = rx::captures(&t, r"(?i)^(.+?)\s+S\d{1,2}E\d{1,2}\b").and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
        match series.filter(|s| !s.is_empty()) {
            Some(s) => t = s.trim().to_string(),
            None => {
                if let Some(m) = rx::captures(&t, r"[\(\[](\d{4})[\)\]]").and_then(|c| c.get(0).map(|m| m.start())) {
                    if m > 0 {
                        t = t[..m].to_string();
                    }
                }
                t = rx::replace(&t, r"(?i)\b(S\d{1,2}E\d{1,2}|S\d{1,2}E?\d{0,2}|E\d{1,2}|\d{1,2}x\d{1,2})\b", "");
                t = rx::replace(&t, r"(?i)\b(Сезон|Season)\s*\d{1,2}(?!\d).*$", "");
            }
        }
        t = rx::replace(&t, r"(?i)\b(2160p|1080p|720p|480p)\b", "");
        t = rx::replace(&t, r"(?i)\b(HDR10?|DV|HDR|SDR|10bit)\b", "");
        t = rx::replace(&t, r"(?i)\b(WEB[-\s]?DL|WEB[-\s]?Rip|WEB\b|BDRip|BDRemux|HDRip|BluRay|BRRip|DVDRip|HDTV)\b", "");
        t = rx::replace(&t, r"(?i)\b(x264|x265|xvid|h\.?264|h\.?265|hevc|avc|aac|ac3|dts)\b", "");
        t = rx::replace(&t, r"(?i)\b(AMZN|NF|DS4K|DD\s*5\s*1|DD5\.?1|DDPA|DDP5\.?1|Atmos|DDP?\s*5\.?1|playWEB)\b", "");
        t = rx::replace(&t, r"(?i)\b(ESub|Sub)\b", "");
        t = rx::replace(&t, r"\.", " ");
        t = rx::replace(&t, r"[\[\]\|]", " ");
        t = trim_end_chars(rx::replace(&t, r"\s{2,}", " ").trim(), &[' ', '/', '-', '|']);
        t = rx::replace(&t, r"(?i)[.\s]+-\s*[A-Za-z0-9][A-Za-z0-9.-]*$", "");
        t = trim_end_chars(t.trim(), &[' ', '-']);
        if util::is_blank(&t) {
            title.to_string()
        } else {
            t
        }
    }

    /// Name + year. Supports `(2026)`, `[2026, ...]` and a standalone year.
    pub fn parse_name_and_year(title: &str) -> (String, i32) {
        if util::is_blank(title) {
            return (String::new(), 0);
        }
        let mut name = rx::replace(title.trim(), r"\s+\|\s+[^|]+$", "").trim().to_string();
        if util::is_blank(&name) {
            return (String::new(), 0);
        }
        let mut relased = 0;
        let m = rx::captures(&name, r"[\(\[](\d{4})[\)\],\s]").map(|c| (c.get(0).map(|m| m.start()).unwrap_or(0), c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default()));
        match m.and_then(|(start, y)| y.parse::<i32>().ok().map(|y| (start, y))) {
            Some((start, y)) => {
                relased = y;
                if start > 0 {
                    name = trim_end_chars(&name[..start], &[' ', '/', '-', '|']);
                }
            }
            None => {
                let y = rx::group(&name, r"\b(19|20)\d{2}\b", 0);
                if let Ok(y2) = y.parse::<i32>() {
                    relased = y2;
                    name = rx::replace(&name, r"\b(19|20)\d{2}\b", "").trim().to_string();
                }
            }
        }
        name = clean_title_for_search(&name);
        (if util::is_blank(&name) { title.trim().to_string() } else { name }, relased)
    }

    /// Normalize a title for FileDB: lowercase `p` in resolutions, `.HDR` → ` HDR`, Dolby Vision / 10-bit → ` HDR`.
    pub fn build_title_for_file_db(original: &str) -> String {
        if util::is_blank(original) {
            return original.to_string();
        }
        let mut t = original.trim().to_string();
        t = rx::replace(&t, r"(?i)\b2160p\b", "2160p");
        t = rx::replace(&t, r"(?i)\b1080p\b", "1080p");
        t = rx::replace(&t, r"(?i)\b720p\b", "720p");
        t = rx::replace(&t, r"(?i)\.(HDR10?)\b", " $1");
        if rx::is_match(&t, r"(?i)(dolby\s*vision|10-?bit)") && !rx::is_match(&t, r"(?i)(\.|\[|,| )hdr") {
            t.push_str(" HDR");
        }
        t
    }
}

pub mod bitru {
    use super::*;

    /// Strip season / episode / quality noise from a title (for name / originalname).
    pub fn clean_title_for_search(title: &str) -> String {
        if util::is_blank(title) {
            return title.to_string();
        }
        let mut t = title.trim().to_string();
        if let Some(start) = rx::captures(&t, r"[\(\[](\d{4})[\)\]]").and_then(|c| c.get(0).map(|m| m.start())) {
            if start > 0 {
                t = t[..start].to_string();
            }
        }
        t = rx::replace(&t, r"(?i)\b(S\d{1,2}E\d{1,2}|S\d{1,2}E?\d{0,2}|E\d{1,2}|\d{1,2}x\d{1,2})\b", "");
        t = rx::replace(&t, r"(?i)\s*\d{1,2}(-\d{1,2})?\s*сезон\s*.*$", "");
        t = rx::replace(&t, r"(?i)\b(Сезон|Season)\s*\d{1,2}(?!\d).*$", "");
        t = rx::replace(&t, r"(?i)\b(2160p|1080p|720p|480p)\b", "");
        t = rx::replace(&t, r"(?i)\b(WEB[-\s]?DL|WEB[-\s]?Rip|BDRip|BDRemux|HDRip|BluRay|BRRip|DVDRip|HDTV)\b", "");
        t = rx::replace(&t, r"(?i)\b(x264|x265|h\.?264|h\.?265|hevc|avc|aac|ac3|dts)\b", "");
        t = rx::replace(&t, r"[\[\]\|]", " ");
        t = trim_end_chars(rx::replace(&t, r"\s{2,}", " ").trim(), &[' ', '/', '-', '|']);
        t = rx::replace(&t, r"(?i)[.\s]+-\s*[A-Za-z0-9][A-Za-z0-9.-]*$", "");
        trim_end_chars(t.trim(), &[' ', '-'])
    }
}

pub mod rudub {
    use super::*;

    pub const TRACKER_NAME: &str = "rudub";

    fn year_group(inner: &str) -> Option<i32> {
        if util::is_blank(inner) {
            return None;
        }
        let t = inner.trim();
        if !rx::is_match(t, r"^(?:19|20)[0-9]{2}(?:\s*[-–]\s*(?:19|20)[0-9]{2})?$") {
            return None;
        }
        let y: i32 = t.get(..4)?.parse().ok()?;
        (1900..=2100).contains(&y).then_some(y)
    }

    /// Balanced top-level `( … )` groups: (byte offset of `(`, inner text).
    fn paren_groups(title: &str) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < title.len() {
            let Some(rel) = title[i..].find('(') else { break };
            let open = i + rel;
            let mut depth = 0i32;
            let mut close = None;
            for (j, c) in title[open..].char_indices() {
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + j);
                        break;
                    }
                }
            }
            let Some(close) = close else { break };
            out.push((open, title[open + 1..close].to_string()));
            i = close + 1;
        }
        out
    }

    fn strip_trailing_year_parens(prefix: &str) -> String {
        let mut p = prefix.to_string();
        while !util::is_blank(&p) {
            p = p.trim_end().to_string();
            if !p.ends_with(')') {
                break;
            }
            let Some(open) = p.rfind('(') else { break };
            let inner = &p[open + 1..p.len() - 1];
            if year_group(inner).is_none() {
                break;
            }
            p = p[..open].trim_end().to_string();
        }
        p
    }

    /// Split a listing title into (name, originalname, year). Year comes from a `(YYYY)`
    /// group, otherwise from `create_time`. Empty strings mean "not found".
    pub fn parse_title_fields(title: &str, create_time: &DateTime<Utc>) -> (String, String, i32) {
        let mut name: Option<String> = None;
        let mut original: Option<String> = None;
        let mut relased = 0;
        if !util::is_blank(title) {
            for (start, inner) in paren_groups(title) {
                if let Some(y) = year_group(&inner) {
                    if relased <= 0 {
                        relased = y;
                    }
                    continue;
                }
                if original.is_none() {
                    name = Some(strip_trailing_year_parens(title[..start].trim()));
                    original = Some(inner.trim().to_string());
                }
            }
            if name.as_deref().map(util::is_blank).unwrap_or(true) {
                let cut = title.find(['(', '/', '|']).unwrap_or(title.len());
                name = Some(title[..cut].trim().to_string());
            }
        }
        if relased <= 0 && !time::is_min(create_time) && create_time.year() > 1 {
            relased = create_time.year();
        }
        let norm = |s: Option<String>| s.filter(|x| !util::is_blank(x)).unwrap_or_default();
        (norm(name), norm(original), relased)
    }
}

pub mod ultradox {
    pub const TRACKER_NAME: &str = "ultradox";

    /// Host-independent path + query + fragment, lowercase (identity across domain changes).
    pub fn canonical_path_and_fragment(url: &str) -> String {
        if url.trim().is_empty() {
            return String::new();
        }
        let url = url.trim();
        if let Ok(u) = url::Url::parse(url) {
            if u.scheme() == "http" || u.scheme() == "https" {
                let mut s = u.path().to_string();
                if let Some(q) = u.query().filter(|q| !q.is_empty()) {
                    s.push('?');
                    s.push_str(q);
                }
                if let Some(f) = u.fragment().filter(|f| !f.is_empty()) {
                    s.push('#');
                    s.push_str(f);
                }
                return s.to_lowercase();
            }
        }
        let mut s = url.to_string();
        if !s.starts_with('/') {
            s.insert(0, '/');
        }
        s.to_lowercase()
    }

    pub fn canonical_torrent_url(host: &str, url: &str) -> String {
        let host = host.trim_end_matches('/');
        let mut path = canonical_path_and_fragment(url);
        if path.is_empty() {
            return String::new();
        }
        if !path.starts_with('/') {
            path.insert(0, '/');
        }
        format!("{host}{path}")
    }
}

/// Host of an absolute URL (lowercase), None when unparsable.
pub fn host_of(url: &str) -> Option<String> {
    if url.trim().is_empty() {
        return None;
    }
    url::Url::parse(url.trim()).ok().and_then(|u| u.host_str().map(|h| h.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn rudub_title_fields() {
        let created = Utc.with_ymd_and_hms(2026, 9, 7, 19, 2, 58).unwrap();
        assert_eq!(
            rudub::parse_title_fields("Фонари (Lanterns) Сезон 1 Серии 01-04 (HD1080p WEBRip)", &created),
            ("Фонари".into(), "Lanterns".into(), 2026)
        );
        let created = Utc.with_ymd_and_hms(2026, 9, 2, 19, 3, 28).unwrap();
        assert_eq!(
            rudub::parse_title_fields("Мистер Килл (Mr. Kill) Сезон 1 Серии 01-09 (HD1080p WEBRip)", &created),
            ("Мистер Килл".into(), "Mr. Kill".into(), 2026)
        );
        let created = Utc.with_ymd_and_hms(2026, 7, 29, 0, 0, 0).unwrap();
        assert_eq!(
            rudub::parse_title_fields("Гнев (2026) (Furia (Wrath)) Сезон 1 Серии 01-06 (HD1080p WEBRip)", &created),
            ("Гнев".into(), "Furia (Wrath)".into(), 2026)
        );
        let created = Utc.with_ymd_and_hms(2026, 9, 8, 0, 0, 0).unwrap();
        assert_eq!(
            rudub::parse_title_fields(
                "Мой любимый сотрудник (My Bias, My Boss (Choeaeui sawon)) Сезон 1 Серии 01-08 (HD1080p WEBRip)",
                &created
            ),
            ("Мой любимый сотрудник".into(), "My Bias, My Boss (Choeaeui sawon)".into(), 2026)
        );
    }

    #[test]
    fn ultradox_canonical_path_is_host_independent() {
        let path = "/serial-hd/57542-oskolki-pravdy-1-sezon.html#h=a1b2c3d4";
        assert_eq!(ultradox::canonical_path_and_fragment(&format!("https://ultradox.onl{path}")), path);
        assert_eq!(ultradox::canonical_path_and_fragment(&format!("https://001.ultradox.vip{path}")), path);
        assert_eq!(ultradox::canonical_path_and_fragment(&format!("https://002.ultradox.vip{path}")), path);
        assert_eq!(
            ultradox::canonical_torrent_url("https://ultradox.vip", &format!("https://ultradox.onl{path}")),
            format!("https://ultradox.vip{path}")
        );
        let a = ultradox::canonical_path_and_fragment("https://ultradox.onl/serial-hd/x.html#h=aaaa1111");
        let b = ultradox::canonical_path_and_fragment("https://ultradox.vip/serial-hd/x.html#h=bbbb2222");
        assert_ne!(a, b);
    }

    #[test]
    fn bitru_clean_title() {
        assert_eq!(bitru::clean_title_for_search("Название (2020) WEB-DL 1080p"), "Название");
        assert_eq!(bitru::clean_title_for_search("Show S01E01 720p"), "Show");
        assert_eq!(bitru::clean_title_for_search("Сериал 1 сезон"), "Сериал");
    }

    #[test]
    fn knaben_name_and_year() {
        assert_eq!(knaben::parse_name_and_year("The Movie (2024) 1080p WEB-DL | 1337x"), ("The Movie".into(), 2024));
        assert_eq!(knaben::parse_name_and_year("Show.Name.S01E05.1080p.WEB.h264"), ("Show Name".into(), 0));
        assert_eq!(knaben::parse_name_and_year("Film 2019 2160p"), ("Film".into(), 2019));
        assert_eq!(knaben::build_title_for_file_db("Movie 1080P.HDR10 x265"), "Movie 1080p HDR10 x265");
        assert_eq!(knaben::build_title_for_file_db("Movie Dolby Vision 2160p"), "Movie Dolby Vision 2160p HDR");
    }

    #[test]
    fn hosts() {
        assert_eq!(host_of("https://Kinozal.Guru/details.php?id=1").as_deref(), Some("kinozal.guru"));
        assert_eq!(host_of("kinozal.guru"), None);
        assert_eq!(host_of(""), None);
    }
}
