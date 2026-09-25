//! Anistar (DLE) listing / detail parsers.

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use indexmap::IndexSet;

use crab_core::models::TorrentDetails;
use crab_core::util::{html_decode, is_blank};
use crab_core::{rx, time};

pub const TRACKER: &str = "anistar";

const POST_URL_ABS_RE: &str = r#"https?://[^"'>]+/\d{2,}-[^"'>]+?\.html"#;
const POST_URL_REL_RE: &str = r#"/\d{2,}-[^"'>]+?\.html"#;
const PAGE_NUM_RE: &str = r"/page/([0-9]+)/";
const PAGES_BLOCK_RE: &str = r#"(?i)<div\s+class="[^"]*\bpages\b[^"]*">([\s\S]*?)</div>"#;
const H1_RE: &str = r"(?is)<h1[^>]*>\s*(.*?)\s*</h1>";
const TORRENT_BLOCK_RE: &str = r#"(?i)<div id="torrent_(\d+)_info"\s+class="torrent""#;
const INFO_D1_RE: &str = r#"(?is)<div class="info_d1">\s*([^<]+?)\s*</div>"#;
const DATE_RE: &str = r"\b(\d{2})-(\d{2})-(\d{4})\b";
const SID_RE: &str = r#"(?i)<div class="li_distribute">\s*([0-9]+)\s*</div>"#;
const PIR_RE: &str = r#"(?i)<div class="li_swing">\s*([0-9]+)\s*</div>"#;
const SERIES_RANGE_RE: &str = r"(?i)сери[яи]\s+(\d{1,4})\s*-\s*(\d{1,4})";
const SERIES_SINGLE_RE: &str = r"(?i)серия\s+(\d{1,4})";
const FILM_RE: &str = r"(?i)^\s*фильм\b";
const CLEAN_SPACE_RE: &str = r"\s+";

/// Detail row plus the torrent id used by `engine/gettorrent.php?id=`.
#[derive(Clone, Debug, Default)]
pub struct AnistarDetails {
    pub t: TorrentDetails,
    pub download_id: String,
}

impl AsRef<TorrentDetails> for AnistarDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for AnistarDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn max_page_in(haystack: &str, page_re: &str) -> i32 {
    if haystack.is_empty() {
        return 0;
    }
    rx::all_groups(haystack, page_re).iter().filter_map(|m| m[1].parse::<i32>().ok()).fold(0, i32::max)
}

/// Last listing page from the DLE `div.pages` pager (prefers `/{section}/page/N/`), else 1.
pub fn detect_last_page(list_html: &str, section_path: Option<&str>) -> i32 {
    if is_blank(list_html) {
        return 1;
    }
    let section = section_path.unwrap_or("").trim_matches('/');
    let page_re = if section.is_empty() {
        PAGE_NUM_RE.to_string()
    } else {
        format!("(?i)/{}/page/([0-9]+)/", escape(section))
    };
    let blocks = rx::all_groups(list_html, PAGES_BLOCK_RE);
    for b in blocks.iter().rev() {
        let n = max_page_in(&b[1], &page_re);
        if n > 0 {
            return n;
        }
    }
    let fallback = max_page_in(list_html, &page_re);
    if fallback > 0 {
        fallback
    } else {
        1
    }
}

/// Absolute post urls on the canonical host (mirror links are rewritten), de-duplicated.
pub fn extract_post_urls(list_html: &str, canon_host: &str) -> Vec<String> {
    let mut out = Vec::new();
    if is_blank(list_html) {
        return out;
    }
    let canon = canon_host.trim_end_matches('/');
    let mut seen: IndexSet<String> = IndexSet::new();
    let mut add = |path: &str, out: &mut Vec<String>| {
        if is_blank(path) {
            return;
        }
        let abs = format!("{canon}{path}");
        if seen.insert(abs.to_lowercase()) {
            out.push(abs);
        }
    };
    for m in rx::all_groups(list_html, POST_URL_ABS_RE) {
        let p = rx::group(&m[0], POST_URL_REL_RE, 0);
        if !p.is_empty() {
            add(&p, &mut out);
        }
    }
    for m in rx::all_groups(list_html, POST_URL_REL_RE) {
        add(&m[0], &mut out);
    }
    out
}

fn clean(s: &str) -> String {
    rx::replace(html_decode(s).trim(), CLEAN_SPACE_RE, " ").trim().to_string()
}

/// `"Имя / Original"` → (name, original); original is "" when absent.
pub fn parse_title_names(h1: &str) -> Option<(String, String)> {
    if is_blank(h1) {
        return None;
    }
    let h1 = clean(h1);
    let (name, original) = match h1.find(" / ") {
        Some(i) => (h1[..i].trim().to_string(), h1[i + 3..].trim().to_string()),
        None => (h1.clone(), String::new()),
    };
    if is_blank(&name) {
        return None;
    }
    Some((name, original))
}

/// Episode label from `info_d1` (the size in `Фильм (3.05 Gb)` is not an episode number).
pub fn parse_episode_label(info: &str) -> (String, String) {
    if is_blank(info) {
        return ("Серия 1".into(), "1".into());
    }
    let info = info.trim();
    let r = rx::groups(info, SERIES_RANGE_RE);
    if !r[0].is_empty() {
        return (format!("Серии {}-{}", r[1], r[2]), r[1].clone());
    }
    let s = rx::groups(info, SERIES_SINGLE_RE);
    if !s[0].is_empty() {
        return (format!("Серия {}", s[1]), s[1].clone());
    }
    if rx::is_match(info, FILM_RE) {
        return ("Фильм".into(), "film".into());
    }
    ("Серия 1".into(), "1".into())
}

/// One row per `torrent_{id}_info` block of a post page.
pub fn parse_detail_torrents(post_html: &str, post_url: &str, types: &[&str]) -> Vec<AnistarDetails> {
    let mut torrents = Vec::new();
    if is_blank(post_html) || is_blank(post_url) {
        return torrents;
    }
    let h1 = rx::groups(post_html, H1_RE);
    let h1 = if h1[0].is_empty() { String::new() } else { clean(&h1[1]) };
    let Some((name, original)) = parse_title_names(&h1) else { return torrents };
    let title_base = if is_blank(&original) { name.clone() } else { format!("{name} / {original}") };

    let re = rx::re(TORRENT_BLOCK_RE);
    for caps in re.captures_iter(post_html).filter_map(|c| c.ok()) {
        let (Some(whole), Some(tid)) = (caps.get(0), caps.get(1)) else { continue };
        let tid = tid.as_str().to_string();
        let around: String = post_html[whole.start()..].chars().take(4000).collect();

        let mut ep = ("Серия 1".to_string(), "1".to_string());
        let info = rx::groups(&around, INFO_D1_RE);
        if !info[0].is_empty() {
            ep = parse_episode_label(&html_decode(&info[1]));
        }

        let mut create_time = time::now();
        let mut relased = create_time.year();
        let d = rx::groups(&around, DATE_RE);
        if !d[0].is_empty() {
            if let (Ok(day), Ok(month), Ok(year)) = (d[1].parse::<u32>(), d[2].parse::<u32>(), d[3].parse::<i32>()) {
                if day > 0 && month > 0 && year > 0 {
                    if let Some(nd) = NaiveDate::from_ymd_opt(year, month, day).and_then(|x| x.and_hms_opt(0, 0, 0)) {
                        create_time = Utc.from_utc_datetime(&nd);
                        relased = year;
                    }
                }
            }
        }

        let sid = rx::group(&around, SID_RE, 1).parse().unwrap_or(0);
        let pir = rx::group(&around, PIR_RE, 1).parse().unwrap_or(0);

        torrents.push(AnistarDetails {
            t: TorrentDetails {
                trackerName: TRACKER.into(),
                types: types.iter().map(|s| s.to_string()).collect(),
                url: format!("{post_url}?e={}&id={tid}", ep.1),
                title: format!("{title_base} — {}", ep.0),
                sid,
                pir,
                createTime: create_time,
                name: name.clone(),
                originalname: if is_blank(&original) { name.clone() } else { original.clone() },
                relased,
                ..Default::default()
            },
            download_id: tid,
        });
    }
    torrents
}
