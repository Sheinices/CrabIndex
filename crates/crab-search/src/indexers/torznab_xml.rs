//! Torznab / Newznab XML output (caps, indexer list, RSS items).

use chrono::{DateTime, Datelike, Utc};

use crab_core::models::api::Result;
use crab_core::util::{is_blank, md5};
use crab_core::{rx, time};

use super::filters;
use crate::magnet::html_encode;

fn esc(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        html_encode(value)
    }
}

pub fn caps_xml(api_url: &str) -> String {
    let api = esc(api_url.trim_end_matches('/'));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<caps>
  <server version="1.0" title="CrabIndex" strapline="Native Torznab API" email="info@localhost" url="{api}"/>
  <limits max="1000" default="100"/>
  <searching>
    <search available="yes" supportedParams="q,imdbid"/>
    <tv-search available="yes" supportedParams="q,imdbid,tvdbid,season,ep"/>
    <movie-search available="yes" supportedParams="q,imdbid"/>
  </searching>
  <categories>
    <category id="2000" name="Movies"/>
    <category id="5000" name="TV"/>
    <category id="5070" name="TV/Anime"/>
  </categories>
</caps>"#
    )
}

pub fn indexers_xml<S: AsRef<str>>(trackers: Option<&[S]>) -> String {
    let mut sb = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<indexers>
  <indexer id="all" configured="true">
    <title>CrabIndex (all trackers)</title>
    <description>Aggregated CrabIndex search across all configured trackers</description>
    <link>https://github.com/sheinices/crabindex</link>
    <language>ru-RU</language>
    <type>public</type>
  </indexer>"#,
    );
    for tracker in trackers.unwrap_or(&[]) {
        let e = esc(tracker.as_ref());
        sb.push_str(&format!(
            r#"
  <indexer id="{e}" configured="true">
    <title>{e}</title>
    <description>CrabIndex tracker: {e}</description>
    <link>https://github.com/sheinices/crabindex</link>
    <language>ru-RU</language>
    <type>public</type>
  </indexer>"#
        ));
    }
    sb.push_str("\n</indexers>");
    sb
}

pub fn wrap_rss(items_xml: &str, site_origin: &str, torznab_api_url: &str) -> String {
    let site = esc(site_origin.trim_end_matches('/'));
    let api = esc(torznab_api_url.trim_end_matches('/'));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:torznab="http://torznab.com/schemas/2015/feed">
    <channel>
        <atom:link href="{api}" rel="self" type="application/rss+xml" />
        <title>CrabIndex</title>
        <description>Torznab API</description>
        <link>{site}/</link>
        <language>en-us</language>
        <category>search</category>
        {items_xml}
    </channel>
</rss>"#
    )
}

pub fn items_xml(items: &[Result], assigned_cat: &str, enrich_titles: bool, cat_param: &str) -> String {
    let mut sb = String::new();
    for t in items {
        sb.push_str(&item_xml(t, assigned_cat, enrich_titles, cat_param));
    }
    sb
}

/// Title with `| [voice voice].rus` suffix when enrichment is on and voices are known.
pub fn display_title(torrent: &Result, enrich_titles: bool) -> String {
    let title = torrent.Title.clone().unwrap_or_else(|| "Unknown".to_string());
    let voices: Vec<String> =
        torrent.info.as_ref().and_then(|i| i.voices.as_ref()).map(|v| v.iter().cloned().collect()).unwrap_or_default();
    if enrich_titles && !voices.is_empty() {
        format!("{title} | [{}].rus", voices.join(" "))
    } else {
        title
    }
}

fn item_xml(torrent: &Result, assigned_cat: &str, enrich_titles: bool, cat_param: &str) -> String {
    let title = torrent.Title.clone().unwrap_or_else(|| "Unknown".to_string());
    let display = display_title(torrent, enrich_titles);

    let magnet = torrent.MagnetUri.clone().or_else(|| torrent.Details.clone()).unwrap_or_default();
    let details_url = torrent.Details.as_deref();
    let indexer = torrent.Tracker.clone().unwrap_or_else(|| "CrabIndex".to_string());
    let seeders = torrent.Seeders;
    let leechers = torrent.Peers;
    let peers = if leechers > 0 { seeders + leechers } else { seeders };

    let mut item_cat = assigned_cat.to_string();
    if item_cat.is_empty() {
        if let Some(first) = torrent.Category.as_ref().and_then(|c| c.first()) {
            item_cat = first.to_string();
        }
    }
    if item_cat.is_empty() && !is_blank(cat_param) {
        item_cat = cat_param.split(',').next().unwrap_or("").trim().to_string();
    }
    if item_cat.is_empty() {
        item_cat = "2000".to_string();
    }

    let infohash = extract_info_hash(&magnet);
    let guid = infohash.clone().unwrap_or_else(|| md5(&display));
    let (season, episode) = filters::attrs_from_result(torrent);
    let size_bytes = resolve_size_bytes(torrent);
    let pub_date = format_pub_date(&torrent.PublishDate);
    let enclosure_type = if magnet.len() >= 7 && magnet[..7].eq_ignore_ascii_case("magnet:") {
        "application/x-bittorrent;x-scheme-handler/magnet"
    } else {
        "application/x-bittorrent"
    };

    let mut attrs = String::new();
    append_attr(&mut attrs, "magneturl", &magnet);
    append_attr(&mut attrs, "size", &size_bytes.to_string());
    append_attr(&mut attrs, "seeders", &seeders.to_string());
    if leechers > 0 {
        append_attr(&mut attrs, "leechers", &leechers.to_string());
    }
    append_attr(&mut attrs, "peers", &peers.to_string());
    append_attr(&mut attrs, "infohash", infohash.as_deref().unwrap_or(""));
    append_attr(&mut attrs, "downloadvolumefactor", "1");
    append_attr(&mut attrs, "uploadvolumefactor", "1");
    append_attr(&mut attrs, "site", &indexer);
    append_attr(&mut attrs, "category", &item_cat);

    let lang_tag = if rx::is_match(&title, r"[а-яА-ЯёЁ]") {
        "ru-RU"
    } else if rx::is_match(&title, r"[a-zA-Z]") {
        "en-US"
    } else {
        "ru-RU"
    };
    let lang_code = if lang_tag.starts_with("en") { "en" } else { "ru" };
    append_attr(&mut attrs, "language", lang_tag);
    append_attr(&mut attrs, "lang", lang_code);

    if let Some(rel) = torrent.info.as_ref().map(|i| i.relased).filter(|&r| r > 0) {
        append_attr(&mut attrs, "year", &rel.to_string());
    }
    if let Some(s) = season {
        append_attr(&mut attrs, "season", &s.to_string());
    }
    if let Some(e) = episode {
        append_attr(&mut attrs, "ep", &e.to_string());
        append_attr(&mut attrs, "episode", &e.to_string());
    }

    let comments = match details_url {
        Some(d) if !is_blank(d) && d.len() >= 4 && d[..4].eq_ignore_ascii_case("http") => {
            format!("\n        <comments>{}</comments>", esc(d))
        }
        _ => String::new(),
    };

    format!(
        r#"
    <item>
        <title>{title}</title>
        <guid isPermaLink="false">{guid}</guid>
        <jackettindexer id="all">{indexer}</jackettindexer>
        <link>{link}</link>{comments}
        <pubDate>{pub_date}</pubDate>
        <category>{item_cat}</category>
        <size>{size_bytes}</size>
        <enclosure url="{link}" length="{size_bytes}" type="{enclosure_type}" />
{attrs}    </item>"#,
        title = esc(&display),
        indexer = esc(&indexer),
        link = esc(&magnet),
    )
}

fn append_attr(sb: &mut String, name: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    sb.push_str("        <torznab:attr name=\"");
    sb.push_str(name);
    sb.push_str("\" value=\"");
    sb.push_str(&esc(value));
    sb.push_str("\" />\n");
}

/// Lowercase `btih:` hash from a magnet (any length of hex digits).
pub fn extract_info_hash(magnet: &str) -> Option<String> {
    if is_blank(magnet) {
        return None;
    }
    let h = rx::group_i(magnet, r"btih:([a-fA-F0-9]+)", 1);
    if h.is_empty() {
        None
    } else {
        Some(h.to_lowercase())
    }
}

/// md5 hex of the text (stable guid when there is no infohash).
pub fn stable_guid(input: &str) -> String {
    md5(input)
}

/// RFC 1123 date; missing or pre-2000 dates fall back to now.
pub fn format_pub_date(publish_date: &DateTime<Utc>) -> String {
    let d = if time::is_min(publish_date) || publish_date.year() < 2000 { time::now() } else { *publish_date };
    d.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

pub fn resolve_size_bytes(torrent: &Result) -> i64 {
    if torrent.Size > 0.0 {
        return torrent.Size as i64;
    }
    parse_size_name_to_bytes(torrent.info.as_ref().and_then(|i| i.sizeName.as_deref()))
}

fn parse_size_name_to_bytes(size_name: Option<&str>) -> i64 {
    let Some(size_name) = size_name.filter(|s| !is_blank(s)) else {
        return 0;
    };
    let Some(c) = rx::captures_i(size_name, r"([0-9\.,]+)\s*(Mb|МБ|GB|ГБ|TB|ТБ|KB|КБ|B|Б)?") else {
        return 0;
    };
    let num = c.get(1).map(|m| m.as_str()).unwrap_or("").replace(',', ".");
    let Ok(value) = num.parse::<f64>() else {
        return 0;
    };
    if value <= 0.0 {
        return 0;
    }
    let unit = c.get(2).map(|m| m.as_str().to_lowercase()).unwrap_or_default();
    match unit.as_str() {
        "kb" | "кб" => (value * 1024.0) as i64,
        "mb" | "мб" => (value * 1_048_576.0) as i64,
        "gb" | "гб" => (value * 1_073_741_824.0) as i64,
        "tb" | "тб" => (value * 1_099_511_627_776.0) as i64,
        "b" | "б" | "" => value as i64,
        _ => (value * 1_048_576.0) as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crab_core::models::api::TorrentInfo;
    use chrono::TimeZone;
    use indexmap::IndexSet;

    #[test]
    fn indexers_xml_includes_all_and_per_tracker_escapes_xml() {
        let xml = indexers_xml(Some(&["rutracker", "a&b"][..]));
        assert!(xml.contains("id=\"all\""));
        assert!(xml.contains("id=\"rutracker\""));
        assert!(xml.contains("id=\"a&amp;b\""));
        assert!(xml.contains("<title>a&amp;b</title>"));
        assert!(xml.contains("</indexers>"));
    }

    #[test]
    fn indexers_xml_none_only_all() {
        let xml = indexers_xml::<&str>(None);
        assert!(xml.contains("id=\"all\""));
        assert!(!xml.contains("CrabIndex tracker:"));
    }

    #[test]
    fn item_xml_layout() {
        let r = Result {
            Tracker: Some("rutor".into()),
            Details: Some("http://rutor.info/torrent/1".into()),
            Title: Some("Сериал S01E02".into()),
            Size: 0.0,
            PublishDate: Utc.with_ymd_and_hms(2024, 5, 1, 12, 0, 0).unwrap(),
            Category: Some(IndexSet::from([5000])),
            MagnetUri: Some("magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&tr=a&b".into()),
            Seeders: 3,
            Peers: 0,
            info: Some(TorrentInfo {
                relased: 2023,
                sizeName: Some("1,5 GB".into()),
                voices: Some(IndexSet::from(["LostFilm".to_string()])),
                ..Default::default()
            }),
            ..filters::empty_result()
        };
        let xml = items_xml(&[r], "", true, "");
        let expected = "
    <item>
        <title>Сериал S01E02 | [LostFilm].rus</title>
        <guid isPermaLink=\"false\">abcdef0123456789abcdef0123456789abcdef01</guid>
        <jackettindexer id=\"all\">rutor</jackettindexer>
        <link>magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&amp;tr=a&amp;b</link>
        <comments>http://rutor.info/torrent/1</comments>
        <pubDate>Wed, 01 May 2024 12:00:00 GMT</pubDate>
        <category>5000</category>
        <size>1610612736</size>
        <enclosure url=\"magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&amp;tr=a&amp;b\" length=\"1610612736\" type=\"application/x-bittorrent;x-scheme-handler/magnet\" />
        <torznab:attr name=\"magneturl\" value=\"magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&amp;tr=a&amp;b\" />
        <torznab:attr name=\"size\" value=\"1610612736\" />
        <torznab:attr name=\"seeders\" value=\"3\" />
        <torznab:attr name=\"peers\" value=\"3\" />
        <torznab:attr name=\"infohash\" value=\"abcdef0123456789abcdef0123456789abcdef01\" />
        <torznab:attr name=\"downloadvolumefactor\" value=\"1\" />
        <torznab:attr name=\"uploadvolumefactor\" value=\"1\" />
        <torznab:attr name=\"site\" value=\"rutor\" />
        <torznab:attr name=\"category\" value=\"5000\" />
        <torznab:attr name=\"language\" value=\"ru-RU\" />
        <torznab:attr name=\"lang\" value=\"ru\" />
        <torznab:attr name=\"year\" value=\"2023\" />
        <torznab:attr name=\"season\" value=\"1\" />
        <torznab:attr name=\"ep\" value=\"2\" />
        <torznab:attr name=\"episode\" value=\"2\" />
    </item>";
        assert_eq!(xml, expected);
    }

    #[test]
    fn caps_and_rss() {
        assert!(caps_xml("http://h/torznab/api/").contains("url=\"http://h/torznab/api\""));
        let rss = wrap_rss("", "http://h/", "http://h/torznab/api");
        assert!(rss.contains("<link>http://h/</link>"));
        assert!(rss.contains("<atom:link href=\"http://h/torznab/api\""));
    }
}
