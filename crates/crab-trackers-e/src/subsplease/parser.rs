//! SubsPlease JSON API parser - keeps only 1080p magnets and maps release metadata.

use chrono::{DateTime, Utc};
use crab_core::models::{de, TorrentDetails};
use crab_core::rx;
use serde::Deserialize;
use serde_json::Value;

pub const TRACKER_NAME: &str = "subsplease";
pub const PREFERRED_RES: &str = "1080";

const XL_RE: &str = r"(?i)[?&]xl=(\d+)";
const BTIH_RE: &str = r"(?i)xt=urn:btih:([A-Za-z0-9]{32,40})";
const SHOW_SID_RE: &str = r#"(?i)id=["']show-release-table["'][^>]*\bsid=["'](\d+)["']|\bsid=["'](\d+)["'][^>]*id=["']show-release-table["']"#;
const SHOW_SID_LOOSE_RE: &str = r#"(?i)<table[^>]*id=["']show-release-table["'][^>]*\bsid=["'](\d+)["']"#;
const SHOW_LINK_RE: &str = r#"(?i)href=["']/shows/([^"'/]+)/?["']"#;

/// Release row with the API metadata kept for refreshes.
#[derive(Clone, Debug, Default)]
pub struct SubsPleaseDetails {
    pub t: TorrentDetails,
    pub show_sid: Option<String>,
    pub page: String,
    pub episode: String,
    pub is_batch: bool,
    pub info_hash: Option<String>,
    pub image_url: Option<String>,
    pub xdcc: Option<String>,
}

impl AsRef<TorrentDetails> for SubsPleaseDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for SubsPleaseDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct SubsPleaseReleaseDto {
    #[serde(rename = "show", deserialize_with = "de::opt_string")]
    pub show: Option<String>,
    #[serde(rename = "episode", deserialize_with = "de::opt_string")]
    pub episode: Option<String>,
    #[serde(rename = "page", deserialize_with = "de::opt_string")]
    pub page: Option<String>,
    #[serde(rename = "release_date", deserialize_with = "de::opt_string")]
    pub release_date: Option<String>,
    #[serde(rename = "time", deserialize_with = "de::opt_string")]
    pub time: Option<String>,
    #[serde(rename = "image_url", deserialize_with = "de::opt_string")]
    pub image_url: Option<String>,
    #[serde(rename = "xdcc", deserialize_with = "de::opt_string")]
    pub xdcc: Option<String>,
    #[serde(rename = "downloads", deserialize_with = "de::opt_vec")]
    pub downloads: Option<Vec<SubsPleaseDownloadDto>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct SubsPleaseDownloadDto {
    #[serde(rename = "res", deserialize_with = "de::opt_string")]
    pub res: Option<String>,
    #[serde(rename = "magnet", deserialize_with = "de::opt_string")]
    pub magnet: Option<String>,
    #[serde(rename = "torrent", deserialize_with = "de::opt_string")]
    pub torrent: Option<String>,
}

fn blank(s: &Option<String>) -> bool {
    s.as_deref().map(|x| x.trim().is_empty()).unwrap_or(true)
}

pub fn is_batch_episode(episode: &str) -> bool {
    if episode.trim().is_empty() {
        return false;
    }
    let ep = episode.trim();
    ep.contains('-') || rx::is_match(ep, r"^\d+\s*~\s*\d+$")
}

pub fn is_limit_reached(json: &str) -> bool {
    !json.trim().is_empty() && json.to_lowercase().contains("limit_reached")
}

fn parse_object(json: &str) -> Option<serde_json::Map<String, Value>> {
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(m)) => Some(m),
        _ => None,
    }
}

pub fn parse_latest_or_search_json(json: &str, host: &str) -> Vec<SubsPleaseDetails> {
    let mut list = Vec::new();
    if json.trim().is_empty() || json.trim() == "[]" || is_limit_reached(json) {
        return list;
    }
    let Some(root) = parse_object(json) else { return list };
    for (_, v) in root {
        if !v.is_object() {
            continue;
        }
        let Ok(release) = serde_json::from_value::<SubsPleaseReleaseDto>(v) else { continue };
        list.extend(build_from_release(&release, host, None, None));
    }
    list
}

pub fn parse_show_json(json: &str, host: &str, page_slug: &str, show_sid: &str) -> Vec<SubsPleaseDetails> {
    let mut list = Vec::new();
    if json.trim().is_empty() || json.trim() == "[]" {
        return list;
    }
    let Some(root) = parse_object(json) else { return list };
    for section in ["batch", "episode"] {
        let Some(Value::Object(bag)) = root.get(section) else { continue };
        for (_, v) in bag {
            if !v.is_object() {
                continue;
            }
            let Ok(mut release) = serde_json::from_value::<SubsPleaseReleaseDto>(v.clone()) else { continue };
            if blank(&release.page) {
                release.page = Some(page_slug.to_string());
            }
            list.extend(build_from_release(&release, host, Some(show_sid), Some(section)));
        }
    }
    list
}

pub fn parse_show_slugs_from_index_html(html: &str) -> Vec<String> {
    let mut slugs: Vec<String> = Vec::new();
    if html.trim().is_empty() {
        return slugs;
    }
    let mut seen = std::collections::HashSet::new();
    for g in rx::all_groups(html, SHOW_LINK_RE) {
        let slug = g[1].trim().to_string();
        if slug.is_empty() || slug.eq_ignore_ascii_case("shows") {
            continue;
        }
        if seen.insert(slug.to_lowercase()) {
            slugs.push(slug);
        }
    }
    slugs
}

pub fn extract_show_sid_from_html(html: &str) -> Option<String> {
    if html.trim().is_empty() {
        return None;
    }
    let loose = rx::group(html, SHOW_SID_LOOSE_RE, 1);
    if !loose.is_empty() {
        return Some(loose);
    }
    let g = rx::groups(html, SHOW_SID_RE);
    if g[0].is_empty() {
        return None;
    }
    Some(if !g[1].is_empty() { g[1].clone() } else { g[2].clone() })
}

pub fn parse_schedule_page_slugs(json: &str) -> Vec<String> {
    let mut slugs: Vec<String> = Vec::new();
    if json.trim().is_empty() {
        return slugs;
    }
    let Some(root) = parse_object(json) else { return slugs };
    let Some(Value::Object(days)) = root.get("schedule") else { return slugs };
    let mut seen = std::collections::HashSet::new();
    for (_, day) in days {
        let Value::Array(arr) = day else { continue };
        for item in arr {
            let Some(page) = item.as_object().and_then(|o| o.get("page")) else { continue };
            let page = match page {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => continue,
            };
            if page.trim().is_empty() {
                continue;
            }
            let p = page.trim().to_string();
            if seen.insert(p.to_lowercase()) {
                slugs.push(p);
            }
        }
    }
    slugs
}

pub fn format_size(bytes: i64) -> Option<String> {
    if bytes <= 0 {
        return None;
    }
    Some(if bytes < 1_073_741_824 {
        format!("{:.2} Mb", bytes as f64 / 1_048_576.0)
    } else if bytes < 1_099_511_627_776 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.2} TB", bytes as f64 / 1_099_511_627_776.0)
    })
}

pub fn try_parse_xl(magnet: &str) -> Option<i64> {
    if magnet.trim().is_empty() {
        return None;
    }
    rx::group(magnet, XL_RE, 1).parse::<i64>().ok().filter(|x| *x > 0)
}

pub fn try_parse_info_hash(magnet: &str) -> Option<String> {
    if magnet.trim().is_empty() {
        return None;
    }
    let h = rx::group(magnet, BTIH_RE, 1);
    if h.is_empty() {
        None
    } else {
        Some(h.to_uppercase())
    }
}

pub fn build_url(host: &str, page: &str, episode: &str) -> String {
    let host = host.trim_end_matches('/');
    let page = page.trim().trim_matches('/');
    let ep = urlencoding::encode(episode);
    format!("{host}/shows/{page}/?ep={ep}&res={PREFERRED_RES}")
}

/// Stable positive id from episode + res (FNV-1a over UTF-16 code units), used as the FileDB url id.
pub fn stable_url_id(episode: &str) -> i32 {
    let mut hash: u32 = 2166136261;
    let key = format!("{episode}|{PREFERRED_RES}");
    for c in key.encode_utf16() {
        hash ^= c as u32;
        hash = hash.wrapping_mul(16777619);
    }
    let id = (hash & 0x7FFF_FFFF) as i32;
    if id == 0 {
        1
    } else {
        id
    }
}

/// FileDB url-id extractor: `[?&]ep=` value → [`stable_url_id`].
pub fn torrent_id_from_url(url: &str) -> i32 {
    let raw = crab_core::rx::group_i(url, r"[?&]ep=([^&]+)", 1);
    if raw.is_empty() {
        return 0;
    }
    let ep = urlencoding::decode(&raw).map(|c| c.into_owned()).unwrap_or(raw);
    stable_url_id(&ep)
}

pub fn build_title(show: &str, episode: &str, is_batch: bool) -> String {
    let ep = if episode.trim().is_empty() { "?" } else { episode.trim() };
    let mut title = format!("[SubsPlease] {} - {ep} ({PREFERRED_RES}p)", show.trim());
    if is_batch {
        title.push_str(" [Batch]");
    }
    title
}

fn build_from_release(release: &SubsPleaseReleaseDto, host: &str, show_sid: Option<&str>, section: Option<&str>) -> Option<SubsPleaseDetails> {
    let downloads = release.downloads.as_ref().filter(|d| !d.is_empty())?;
    let show = release.show.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
    let episode = release.episode.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
    let page = release.page.as_deref().map(str::trim).filter(|s| !s.is_empty())?;

    let is_batch = section.map(|s| s.eq_ignore_ascii_case("batch")).unwrap_or(false) || is_batch_episode(episode);
    let dl = downloads.iter().find(|d| d.res.as_deref().map(|r| r.eq_ignore_ascii_case(PREFERRED_RES)).unwrap_or(false))?;
    let magnet = dl.magnet.as_deref().filter(|m| !m.trim().is_empty())?.trim().to_string();

    let xl = try_parse_xl(&magnet);
    let info_hash = try_parse_info_hash(&magnet);
    let create_time = parse_release_date(release.release_date.as_deref()).unwrap_or_else(Utc::now);

    let mut t = TorrentDetails::new(TRACKER_NAME, &["anime"], build_url(host, page, episode), build_title(show, episode, is_batch));
    t.name = show.to_string();
    t.originalname = show.to_string();
    t.sid = 1;
    t.pir = 0;
    t.quality = 1080;
    t.sizeName = xl.and_then(format_size).unwrap_or_default();
    t.createTime = create_time;
    t.updateTime = Utc::now();
    t.magnet = magnet;
    t._sn = dl.torrent.as_deref().map(str::trim).unwrap_or("").to_string();

    Some(SubsPleaseDetails {
        t,
        show_sid: show_sid.map(str::to_string),
        page: page.to_string(),
        episode: episode.to_string(),
        is_batch,
        info_hash,
        image_url: release.image_url.clone(),
        xdcc: release.xdcc.clone(),
    })
}

fn parse_release_date(s: Option<&str>) -> Option<DateTime<Utc>> {
    let s = s?.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(d) = DateTime::parse_from_rfc2822(s) {
        return Some(d.with_timezone(&Utc));
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.with_timezone(&Utc));
    }
    crab_core::time::parse_net(s)
}
