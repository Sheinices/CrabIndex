//! le-production.online listing / detail page parsing.

use std::collections::HashSet;

use crab_core::models::TorrentDetails;
use crab_core::{rx, time, util};

use super::categories;
use crate::common::try_int;

pub const TRACKER_NAME: &str = "leproduction";

const SHORT_IMG_RE: &str = r#"(?i)<a\s+class="short-img"\s+href="((?:https?://[^"]+)?/[^"]+?\.html)""#;
const H3_LINK_RE: &str = r#"(?i)<h3>\s*<a\s+href="((?:https?://[^"]+)?/[^"]+?\.html)""#;
const PAGE_NUM_RE: &str = r"(?i)/page/([0-9]+)/";
const NAV_TO_NEXT_RE: &str = r#"(?i)class="navigation">([\s\S]*?)<span\s+class="pnext""#;
const NAME_RU_RE: &str = r#"(?is)Русское\s+название:\s*</div>\s*<div[^>]*class="info-desc"[^>]*>\s*([^<]+)\s*</div>"#;
const NAME_EN_RE: &str = r#"(?is)Оригинальное\s+название:\s*</div>\s*<div[^>]*class="info-desc"[^>]*>\s*([^<]+)\s*</div>"#;
const H1_RE: &str = r"(?i)<h1>([^<]+)</h1>";
const YEAR_RE: &str = r#"(?i)info-label">Год выпуска:</div>\s*<div[^>]*class="info-desc"[^>]*>\s*<a[^>]*>(\d{4})</a>"#;
const DOWNLOAD_ID_RE: &str = r"(?i)index\.php\?do=download&(?:amp;)?id=(\d+)";
const TORRENT_INFO_RE: &str = r#"(?i)id\s*=\s*"torrent_(\d+)_info""#;
const MAGNET_HREF_RE: &str = r#"(?i)href\s*=\s*"(magnet:[^"]+)""#;
const MAGNET_RAW_RE: &str = r#"(?i)(magnet:[^\s"'<]+)"#;
const FILE_NAME_RE: &str = r#"(?is)class="info_d1-le"[^>]*>\s*([^<]+)\s*</div>"#;
const SID_LE_RE: &str = r#"(?i)Раздают:\s*</b>\s*<span[^>]*class="li_distribute_m-le"[^>]*>\s*([0-9]+)\s*</span>"#;
const PIR_LE_RE: &str = r#"(?i)Качают:\s*</b>\s*<span[^>]*class="li_swing_m-le"[^>]*>\s*([0-9]+)\s*</span>"#;
const SIZE_LE_RE: &str = r"(?i)Размер:\s*<span[^>]*>\s*([0-9]+(?:[.,][0-9]+)?)\s*(Mb|Gb|Tb)\s*</span>";
const QUALITY_RE: &str = r"(?i)\b([0-9]{3,4}p)\b";
const EPISODE_RE: &str = r"(?i)\[([0-9]+(?:\s*[-,]\s*[0-9]+)*\s+из\s+[0-9]+)\]";
const CLEAN_SPACE_RE: &str = r"\s+";
const INLINE_SLASH_TAIL_RE: &str = r"\s*/\s*.*$";

pub fn try_get_types(cat: &str) -> Option<&'static [&'static str]> {
    categories::get(cat).map(|c| c.types)
}

/// Last listing page from `span.navigation` (before «Дальше»). Prefers
/// `/{section}/page/N/` so a stray `/page/N/` in scripts cannot inflate it.
pub fn detect_last_page(html: &str, section_path: Option<&str>) -> i32 {
    if html.is_empty() {
        return 1;
    }
    let section = section_path.unwrap_or("").trim_matches('/');
    let page_re = if section.is_empty() {
        PAGE_NUM_RE.to_string()
    } else {
        format!("(?i)/{}/page/([0-9]+)/", regex::escape(section))
    };

    let blocks = rx::all_groups(html, NAV_TO_NEXT_RE);
    for b in blocks.iter().rev() {
        let from_block = max_page_in(&b[1], &page_re);
        if from_block > 0 {
            return from_block;
        }
    }
    let fallback = max_page_in(html, &page_re);
    if fallback > 0 {
        fallback
    } else {
        1
    }
}

fn max_page_in(haystack: &str, page_re: &str) -> i32 {
    if haystack.is_empty() {
        return 0;
    }
    rx::all_groups(haystack, page_re).iter().filter_map(|g| try_int(&g[1])).fold(0, i32::max)
}

pub fn extract_post_urls(html: &str, host: &str) -> Vec<String> {
    let mut out = Vec::new();
    if html.is_empty() || util::is_blank(host) {
        return out;
    }
    let host = host.trim_end_matches('/');
    let mut seen: HashSet<String> = HashSet::new();
    for pattern in [SHORT_IMG_RE, H3_LINK_RE] {
        for g in rx::all_groups(html, pattern) {
            let mut u = g[1].clone();
            if util::is_blank(&u) {
                continue;
            }
            if u.starts_with('/') {
                u = format!("{host}{u}");
            }
            if seen.insert(u.to_lowercase()) {
                out.push(u);
            }
        }
    }
    out
}

pub fn extract_magnet(html: &str) -> Option<String> {
    if html.is_empty() {
        return None;
    }
    let m = rx::groups(html, MAGNET_HREF_RE);
    if !m[0].is_empty() {
        return Some(util::html_decode(&m[1]));
    }
    let m = rx::groups(html, MAGNET_RAW_RE);
    if !m[0].is_empty() {
        Some(util::html_decode(&m[1]))
    } else {
        None
    }
}

pub fn extract_torrent_id(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    let g = rx::groups(url, r"(?i)[?&]id=(\d+)");
    if g[0].is_empty() {
        None
    } else {
        Some(g[1].clone())
    }
}

pub fn parse_detail_html(html: &str, post_url: &str, types: &[&str]) -> Vec<TorrentDetails> {
    let mut out = Vec::new();
    if util::is_blank(html) || util::is_blank(post_url) || types.is_empty() {
        return out;
    }

    let mut name_ru = extract_match(NAME_RU_RE, html);
    let name_en = extract_match(NAME_EN_RE, html);
    if name_ru.is_none() {
        if let Some(h1) = extract_match(H1_RE, html) {
            let n = rx::replace(&h1, INLINE_SLASH_TAIL_RE, "").trim().to_string();
            name_ru = Some(n);
        }
    }
    let Some(name_ru) = name_ru.filter(|n| !util::is_blank(n)) else { return out };

    let g = rx::groups(html, YEAR_RE);
    let relased = if g[0].is_empty() { 0 } else { try_int(&g[1]).unwrap_or(0) };

    let decoded = util::html_decode(html);
    let mut ids = unique_matches(DOWNLOAD_ID_RE, &decoded);
    if ids.is_empty() {
        ids = unique_matches(DOWNLOAD_ID_RE, html);
    }
    if ids.is_empty() {
        ids = unique_matches(TORRENT_INFO_RE, html);
    }
    if ids.is_empty() {
        return out;
    }

    let page_magnets = collect_magnets(html);
    let now = time::now();

    for (i, tid) in ids.iter().enumerate() {
        let needle = format!("torrent_{tid}_info");
        let mut around = take_around(&decoded, &needle, 20000);
        if around.is_empty() {
            around = take_around(html, &needle, 20000);
        }

        let sid = try_int(&rx::group(&around, SID_LE_RE, 1)).unwrap_or(0);
        let pir = try_int(&rx::group(&around, PIR_LE_RE, 1)).unwrap_or(0);

        let mut size_name = String::new();
        let mut size_bytes = 0.0;
        let sz = rx::groups(&around, SIZE_LE_RE);
        if !sz[0].is_empty() {
            let num_raw = sz[1].replace(',', ".");
            let unit = &sz[2];
            size_name = format!("{num_raw} {unit}");
            if let Ok(mut num) = num_raw.parse::<f64>() {
                match unit.to_lowercase().as_str() {
                    "tb" => num *= 1024.0 * 1024.0,
                    "gb" => num *= 1024.0,
                    _ => {}
                }
                size_bytes = num * 1_048_576.0;
            }
        }

        let mut q = String::new();
        let mut ep = String::new();
        if let Some(fname) = extract_match(FILE_NAME_RE, &around) {
            q = rx::group(&fname, QUALITY_RE, 1);
            ep = rx::group(&fname, EPISODE_RE, 1);
        }

        let mut q_digits = "0".to_string();
        let mut quality = 0;
        if !util::is_blank(&q) {
            q_digits = q.to_lowercase().replace('p', "");
            quality = try_int(&q_digits).unwrap_or(0);
        }

        let mut magnet = extract_magnet(&around).unwrap_or_default();
        if util::is_blank(&magnet) && i < page_magnets.len() && page_magnets.len() == ids.len() {
            magnet = page_magnets[i].clone();
        }

        let mut title = name_ru.clone();
        if let Some(en) = name_en.as_deref().filter(|e| !util::is_blank(e)) {
            title = format!("{name_ru} / {en}");
        }
        if !util::is_blank(&ep) {
            title.push_str(&format!(" [{ep}]"));
        }
        if relased > 0 {
            title.push_str(&format!(" {relased}"));
        }
        if !util::is_blank(&q) {
            title.push_str(&format!(" [{q}]"));
        }

        let url = format!("{post_url}?q={q_digits}&id={tid}");
        let original = match name_en.as_deref().filter(|e| !util::is_blank(e)) {
            Some(en) => en.to_string(),
            None => name_ru.clone(),
        };

        let mut t = TorrentDetails::new(TRACKER_NAME, types, url, clean_spaces(&title).unwrap_or_default());
        t.sid = sid;
        t.pir = pir;
        t.sizeName = size_name;
        t.size = size_bytes;
        t.createTime = now;
        t.updateTime = now;
        t.name = name_ru.clone();
        t.originalname = original;
        t.relased = relased;
        t.magnet = magnet;
        t.quality = quality;
        out.push(t);
    }
    out
}

fn extract_match(pattern: &str, body: &str) -> Option<String> {
    let g = rx::groups(body, pattern);
    if g[0].is_empty() || g.len() < 2 {
        return None;
    }
    clean_spaces(&util::html_decode(&g[1]))
}

fn unique_matches(pattern: &str, body: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    if body.is_empty() {
        return out;
    }
    for g in rx::all_groups(body, pattern) {
        if g.len() < 2 {
            continue;
        }
        if seen.insert(g[1].clone()) {
            out.push(g[1].clone());
        }
    }
    out
}

fn collect_magnets(body: &str) -> Vec<String> {
    if body.is_empty() {
        return Vec::new();
    }
    rx::all_groups(body, MAGNET_HREF_RE).iter().map(|g| util::html_decode(&g[1])).collect()
}

/// Text within `radius` characters of the first case-insensitive occurrence of `needle`.
fn take_around(text: &str, needle: &str, radius: usize) -> String {
    if text.is_empty() || needle.is_empty() {
        return String::new();
    }
    // ASCII lowercasing keeps byte offsets intact.
    let Some(idx) = text.to_ascii_lowercase().find(&needle.to_ascii_lowercase()) else {
        return String::new();
    };
    let before = &text[..idx];
    let start = before.char_indices().rev().nth(radius.saturating_sub(1)).map(|(i, _)| i).unwrap_or(0);
    let after_start = idx + needle.len();
    let after = &text[after_start..];
    let end = after.char_indices().nth(radius).map(|(i, _)| after_start + i).unwrap_or(text.len());
    text[start..end].to_string()
}

fn clean_spaces(s: &str) -> Option<String> {
    if util::is_blank(s) {
        return None;
    }
    Some(rx::replace(s.trim(), CLEAN_SPACE_RE, " "))
}
