//! anifilm.pro listing / detail page parsing.

use chrono::{DateTime, Utc};

use crab_core::models::TorrentDetails;
use crab_core::{rx, time, util};

use crate::common::try_int;

pub const TRACKER_NAME: &str = "anifilm";

/// Marker that must be present on a real listing page.
pub const VALIDATION_MARKER: &str = "AniFilm";

const ITEM_SPLIT_RE: &str = r#"(?i)class="releases__item"#;
const URL_RE: &str = r#"(?i)<a[^>]+href="/(releases/[^"]+)""#;
const NAME_RU_RE: &str = r#"(?i)class="releases__title-russian"[^>]*>([^<]+)</a>"#;
const NAME_ORIG_RE: &str = r#"(?i)class="releases__title-original"[^>]*>([^<]+)</span>"#;
const EPISODES_RE: &str = r"(?i)([0-9]+(-[0-9]+)?)\s*из\s*[0-9]+\s*эп";
const YEAR_RE: &str = r#"(?i)href="/releases/[^"]*">([0-9]{4})</a>"#;
const YEAR_ALT_RE: &str = r"(?i)table-list__value[^>]*>[^<]*(\d{4})";
const TID_RE: &str = r#"(?i)href="/(releases/download-torrent/[0-9]+)"[^>]*>скачать</a>"#;
const CLEAN_SPACE_RE: &str = r"[\n\r\t ]+";

/// Row plus the relative torrent path (`releases/download-torrent/N`).
#[derive(Clone, Debug, Default)]
pub struct AnifilmDetails {
    pub t: TorrentDetails,
    pub download_id: String,
}

impl AsRef<TorrentDetails> for AnifilmDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for AnifilmDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

pub fn parse_listing_html(body: &str, host: &str, types: &[&str], create_time: DateTime<Utc>) -> Vec<AnifilmDetails> {
    let mut out = Vec::new();
    if util::is_blank(body) || util::is_blank(host) || types.is_empty() {
        return out;
    }
    if !body.contains(VALIDATION_MARKER) {
        return out;
    }
    let host = host.trim_end_matches('/');
    let chunks = rx::split(body, ITEM_SPLIT_RE);
    if chunks.len() < 2 {
        return out;
    }
    let now = time::now();

    for row in chunks.iter().skip(1) {
        if util::is_blank(row) {
            continue;
        }
        let url_path = extract(URL_RE, row);
        let mut name = extract(NAME_RU_RE, row);
        let mut originalname = extract(NAME_ORIG_RE, row);
        let episodes = extract(EPISODES_RE, row);

        if util::is_blank(&url_path) || util::is_blank(&name) {
            continue;
        }
        if util::is_blank(&originalname) {
            originalname = name.clone();
        }

        let full_url = format!("{host}/{}", url_path.trim_start_matches('/'));
        let mut title = name.clone();
        if originalname != name {
            title = format!("{name} / {originalname}");
        }
        if !util::is_blank(&episodes) {
            title.push_str(&format!(" ({episodes})"));
        }

        if let Some(paren) = name.find('(') {
            if paren > 0 {
                name = name[..paren].trim().to_string();
            }
        }

        let mut year_str = extract(YEAR_RE, row);
        if util::is_blank(&year_str) {
            year_str = extract(YEAR_ALT_RE, row);
        }
        let relased = try_int(&year_str).unwrap_or(0);

        let mut t = TorrentDetails::new(TRACKER_NAME, types, full_url, title);
        t.sid = 1;
        t.pir = 0;
        t.createTime = create_time;
        t.updateTime = now;
        t.name = name;
        t.originalname = originalname;
        t.relased = relased;
        out.push(AnifilmDetails { t, download_id: String::new() });
    }
    out
}

/// Prefer a 1080p torrent block; otherwise the first download-torrent link.
/// Returns the relative path (`releases/download-torrent/N`) and whether 1080p was selected.
pub fn extract_torrent_download_path(detail_html: &str) -> (Option<String>, bool) {
    if util::is_blank(detail_html) {
        return (None, false);
    }
    for block in detail_html.split("<li class=\"release__torrents-item\">") {
        let lower = block.to_lowercase();
        if !lower.contains("1080p") {
            continue;
        }
        if !lower.contains("href=\"/releases/download-torrent/") {
            continue;
        }
        let m = rx::groups(block, TID_RE);
        if !m[0].is_empty() {
            return (Some(m[1].clone()), true);
        }
    }
    let m = rx::groups(detail_html, TID_RE);
    if !m[0].is_empty() {
        return (Some(m[1].clone()), false);
    }
    (None, false)
}

fn extract(pattern: &str, row: &str) -> String {
    let g = rx::groups(row, pattern);
    if g[0].is_empty() || g.len() < 2 {
        return String::new();
    }
    let s = util::html_decode(&g[1]);
    rx::replace(s.trim(), CLEAN_SPACE_RE, " ").trim().to_string()
}
