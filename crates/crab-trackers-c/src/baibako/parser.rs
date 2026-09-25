//! Baibako `browse.php` listing parser (1080p / 720p releases only).

use crab_core::models::TorrentDetails;
use crab_core::parsing::tparse;
use crab_core::util::{html_decode, is_blank};
use crab_core::{rx, time};

pub const TRACKER: &str = "baibako";
const TYPE_SERIAL: &str = "serial";
const TYPE_MOVIE: &str = "movie";
const ENDPOINT_DOWNLOAD: &str = "/download.php";

/// Marker of a valid (logged-in) listing page.
pub const VALIDATION_NAV_TOP: &str = "id=\"navtop\"";

const SERIAL_PATTERN_1: &str = r"(?i)/s\d+e\d+";
const SERIAL_PATTERNS_LOWER: [&str; 8] = [
    r"\d+[\-й]?\s*сезон",
    r"сезон\s+повністю",
    r"сезон\s+полностью",
    r"полный\s+\d+\s+сезон",
    r"повній\s+\d+[\-й]?\s*сезон",
    r"\d+[\-й]?\s*сезон\s+повністю",
    r"\d+[\-й]?\s*сезон\s+полностью",
    r"сезон\s+\d+",
];
const DOWNLOAD_ID_RE: &str = r#"(?i)href=["']/?(?:download\.php\?id=|download\.php&amp;id=)([0-9]+)["']"#;
const TITLE_FORMAT_RE: &str = r"([^/\(]+)[^/]+/([^/\(]+)";
const QUALITY_FILTER_RE: &str = "(1080p|720p)";

/// Listing row plus its `download.php?id=` url.
#[derive(Clone, Debug, Default)]
pub struct BaibakoDetails {
    pub t: TorrentDetails,
    pub download_uri: String,
}

impl AsRef<TorrentDetails> for BaibakoDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for BaibakoDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

fn extract_and_clean(text: &str, pattern: &str) -> String {
    let res = rx::group_i(text, pattern, 1);
    rx::replace(res.trim(), "[\\n\\r\\t ]+", " ").trim().to_string()
}

pub fn parse_torrent_list_from_html(html: &str, host: &str, page: i32) -> Vec<BaibakoDetails> {
    let mut torrents = Vec::new();
    let decoded = tparse::replace_bad_names(&html_decode(&html.replace("&nbsp;", "")));
    for row in decoded.split("<tr").skip(1) {
        if is_blank(row) {
            continue;
        }
        let date = extract_and_clean(row, "<small>(?:Загружена|Обновлена): ([0-9]+ [^ ]+ [0-9]{4}) в [^<]+</small>");
        let create_time = match tparse::parse_create_time(&date, "dd.MM.yyyy") {
            Some(t) => t,
            None => {
                if page != 0 {
                    continue;
                }
                time::now()
            }
        };

        let g = rx::groups(row, "<a href=\"/?(details.php\\?id=[0-9]+)[^\"]+\">([^<]+)</a>");
        let url = g[1].clone();
        let title = g[2].clone();
        if is_blank(&url) || is_blank(&title) {
            continue;
        }

        let title = title.replace("(Обновляемая)", "").replace("(Золото)", "").replace("(Оновлюється)", "");
        let title = rx::replace(&title, "/( +| )?$", "").trim().to_string();
        if !rx::is_match(&title, QUALITY_FILTER_RE) {
            continue;
        }

        let url = format!("{host}/{url}");
        let mut name = String::new();
        let mut originalname = String::new();
        let g = rx::groups(&title, TITLE_FORMAT_RE);
        if !is_blank(&g[1]) && !is_blank(&g[2]) {
            name = g[1].trim().to_string();
            originalname = g[2].trim().to_string();
        }
        if is_blank(&name) {
            name = rx::split_i(&title, r"(\[|\/|\(|\|)").into_iter().next().unwrap_or_default().trim().to_string();
        }
        if is_blank(&name) {
            continue;
        }
        let download_id = rx::group(row, DOWNLOAD_ID_RE, 1);
        if is_blank(&download_id) {
            continue;
        }
        let types = detect_content_type(&title);
        torrents.push(BaibakoDetails {
            t: TorrentDetails {
                trackerName: TRACKER.into(),
                types,
                url,
                title,
                sid: 1,
                createTime: create_time,
                name,
                originalname,
                ..Default::default()
            },
            download_uri: format!("{host}{ENDPOINT_DOWNLOAD}?id={download_id}"),
        });
    }
    torrents
}

pub fn types_equal(a: &[String], b: &[String]) -> bool {
    a == b
}

/// A .torrent must start with a bencoded dictionary.
pub fn is_valid_bencoded_torrent(data: Option<&[u8]>) -> bool {
    match data {
        Some(d) if !d.is_empty() => d[0] == b'd',
        _ => false,
    }
}

pub fn detect_content_type(title: &str) -> Vec<String> {
    let lower = title.to_lowercase();
    let serial = rx::is_match(title, SERIAL_PATTERN_1) || SERIAL_PATTERNS_LOWER.iter().any(|p| rx::is_match(&lower, p));
    vec![if serial { TYPE_SERIAL } else { TYPE_MOVIE }.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_row() {
        let html = r#"<table id="navtop"><tr><td><a href="/details.php?id=123&amp;hit=1">Вызов / The Call (Сезон 2, серии 1-5) 1080p (Обновляемая) /</a>
<small>Загружена: 12 марта 2024 в 10:00</small> <a href="download.php?id=555">dl</a></td></tr>
<tr><td><a href="/details.php?id=124&hit=1">Фильм / Movie [SD]</a><small>Загружена: 12 марта 2024 в 10:00</small></td></tr></table>"#;
        let list = parse_torrent_list_from_html(html, "http://baibako.tv", 1);
        assert_eq!(list.len(), 1);
        let t = &list[0];
        assert_eq!(t.t.url, "http://baibako.tv/details.php?id=123");
        assert_eq!(t.t.title, "Вызов / The Call (Сезон 2, серии 1-5) 1080p");
        assert_eq!(t.t.name, "Вызов");
        assert_eq!(t.t.originalname, "The Call");
        assert_eq!(t.t.types, vec!["serial"]);
        assert_eq!(t.download_uri, "http://baibako.tv/download.php?id=555");
        assert_eq!(t.t.createTime.format("%Y-%m-%d").to_string(), "2024-03-12");
    }

    #[test]
    fn content_types() {
        assert_eq!(detect_content_type("Name / Orig /s01e02 720p"), vec!["serial"]);
        assert_eq!(detect_content_type("Фильм / Movie 1080p"), vec!["movie"]);
        assert!(is_valid_bencoded_torrent(Some(b"d8:announce")));
        assert!(!is_valid_bencoded_torrent(Some(b"<html>")));
        assert!(!is_valid_bencoded_torrent(None));
    }
}
