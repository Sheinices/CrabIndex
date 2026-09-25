//! Anidub (DLE) listing parser.

use chrono::{Datelike, Duration};

use crab_core::models::TorrentDetails;
use crab_core::parsing::tparse;
use crab_core::util::{html_decode, is_blank};
use crab_core::{rx, time};

use crate::common::split_multi;

pub const TRACKER: &str = "anidub";

/// Marker of a valid listing page.
pub const VALIDATION_DLE_CONTENT: &str = "dle-content";

/// Listing row plus the url used to download the .torrent.
#[derive(Clone, Debug, Default)]
pub struct AnidubDetails {
    pub t: TorrentDetails,
    pub download_uri: String,
}

impl AsRef<TorrentDetails> for AnidubDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for AnidubDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

/// Release year from a detail page (`<b>Год: </b><span>…2020…</span>`), 0 when absent / implausible.
pub fn extract_relased(html: &str) -> i32 {
    if is_blank(html) {
        return 0;
    }
    let mut g = rx::groups_i(html, r"<b>Год:\s*</b>\s*<span>\s*<a[^>]*>([0-9]{4})</a>\s*</span>");
    if g[0].is_empty() {
        g = rx::groups_i(html, r"<b>Год:\s*</b>\s*<span>([0-9]{4})</span>");
    }
    if !g[0].is_empty() {
        if let Ok(year) = g[1].parse::<i32>() {
            if year > 1900 && year <= time::now().year() + 1 {
                return year;
            }
        }
    }
    0
}

fn m(row: &str, pattern: &str) -> String {
    let res = rx::group_i(row, pattern, 1);
    rx::replace(res.trim(), "[\n\r\t ]+", " ").trim().to_string()
}

pub fn parse_torrent_list_from_html(html: &str, host: &str, page: i32) -> Vec<AnidubDetails> {
    let mut torrents = Vec::new();
    let decoded = tparse::replace_bad_names(&html_decode(html));
    let rows: Vec<&str> = if html.contains("<article") {
        decoded.split("<article").skip(1).collect()
    } else {
        split_multi(&decoded, &["<div class=\"story", "<div class=\"rand", "<li><a href=\""]).into_iter().skip(1).collect()
    };

    for row in rows {
        if is_blank(row) {
            continue;
        }
        if !row.contains("href=\"") || !row.contains(".html") {
            continue;
        }

        let mut create_time = None;
        let date_str = m(row, "<li><b>Дата:</b> ([^<]+)</li>");
        if !is_blank(&date_str) {
            if date_str.contains("Сегодня") {
                create_time = Some(time::now());
            } else if date_str.contains("Вчера") {
                create_time = Some(time::now() - Duration::days(1));
            } else {
                let dm = rx::groups(&date_str, "([0-9]{1,2})-([0-9]{2})-([0-9]{4})");
                if !dm[0].is_empty() {
                    let day = format!("{:0>2}", dm[1]);
                    create_time = tparse::parse_create_time(&format!("{day}.{}.{}", dm[2], dm[3]), "dd.MM.yyyy");
                }
            }
        }
        let create_time = match create_time {
            Some(t) => t,
            None => {
                if page != 1 {
                    continue;
                }
                time::now()
            }
        };

        let g = rx::groups(row, "<a href=\"([^\"]+)\"[^>]*>([^<]+)</a>");
        if g[0].is_empty() {
            continue;
        }
        let url_path = g[1].clone();
        let title = g[2].clone();
        if is_blank(&url_path) || is_blank(&title) {
            continue;
        }
        if url_path.contains("/user/")
            || url_path.contains("/xfsearch/")
            || url_path.contains("/forum/")
            || url_path.contains("javascript:")
            || url_path.starts_with('#')
            || !url_path.contains(".html")
        {
            continue;
        }

        let full_url = if url_path.starts_with("http") {
            url_path.clone()
        } else {
            format!("{host}/{}", url_path.trim_start_matches('/'))
        };

        let title = html_decode(&title).trim().to_string();
        let title = rx::replace(&title, "[\n\r\t ]+", " ").trim().to_string();
        if is_blank(&title) {
            continue;
        }

        let mut name = String::new();
        let mut originalname = String::new();
        let nm = rx::groups(&title, r"^([^/]+)\s*/\s*([^\[]+)(?:\s*\[|$)");
        if !nm[0].is_empty() {
            name = nm[1].trim().to_string();
            originalname = nm[2].trim().to_string();
        } else {
            let sm = rx::groups(&title, r"^([^\[]+)(?:\s*\[|$)");
            if !sm[0].is_empty() {
                name = sm[1].trim().to_string();
            }
        }
        if is_blank(&name) {
            name = rx::split_i(&title, r"(\[|\/|\(|\|)").into_iter().next().unwrap_or_default().trim().to_string();
        }

        let types: Vec<String> = if url_path.contains("/dorama/") {
            vec!["dorama".into()]
        } else if url_path.contains("/anime_movie/") || url_path.contains("/anime-movie/") {
            vec!["anime".into(), "movie".into()]
        } else if url_path.contains("/anime_ova/") || url_path.contains("/anime-ova/") {
            vec!["anime".into(), "ova".into()]
        } else if url_path.contains("/anime_tv/") || url_path.contains("/anime-tv/") {
            vec!["anime".into(), "serial".into()]
        } else {
            vec!["anime".into()]
        };

        torrents.push(AnidubDetails {
            t: TorrentDetails {
                trackerName: TRACKER.into(),
                types,
                url: full_url.clone(),
                title,
                sid: 1,
                createTime: create_time,
                name,
                originalname,
                ..Default::default()
            },
            download_uri: full_url,
        });
    }
    torrents
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_article_rows() {
        let html = r#"<div id="dle-content"><article class="story"><a href="https://tr.anidub.com/anime_tv/full/123-name.html">Имя / Name [01 из 12]</a><ul><li><b>Дата:</b> 5-03-2023, 12:00</li></ul></article>
<article class="story"><a href="/dorama/77-dor.html">Дорама [Сериал]</a><li><b>Дата:</b> Вчера, 10:00</li></article></div>"#;
        let list = parse_torrent_list_from_html(html, "https://tr.anidub.com", 2);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].t.name, "Имя");
        assert_eq!(list[0].t.originalname, "Name");
        assert_eq!(list[0].t.types, vec!["anime", "serial"]);
        assert_eq!(list[0].t.createTime.format("%Y-%m-%d").to_string(), "2023-03-05");
        assert_eq!(list[1].t.url, "https://tr.anidub.com/dorama/77-dor.html");
        assert_eq!(list[1].t.types, vec!["dorama"]);
        assert_eq!(list[1].t.name, "Дорама");
    }

    #[test]
    fn relased_from_detail() {
        assert_eq!(extract_relased("<b>Год: </b><span><a href='/x'>2021</a></span>"), 2021);
        assert_eq!(extract_relased("<b>Год:</b> <span>2019</span>"), 2019);
        assert_eq!(extract_relased("<b>Год:</b> <span>1800</span>"), 0);
    }
}
