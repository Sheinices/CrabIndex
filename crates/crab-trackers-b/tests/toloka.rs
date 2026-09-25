mod support;

use crab_core::models::TaskParse;
use crab_core::{conf, rx, time};
use crab_trackers_b::toloka::parser;

const MOANA_TOPIC: &str = "t700099";

fn host() -> String {
    conf().Toloka.host.trim_end_matches('/').to_string()
}

const ROW_HTML: &str = "<html lang=\"uk\"><table><tr></tr>\n\
<tr>\n\
  <td><a href=\"t700099\" class=\"topictitle\">Ваяна / Moana (2026) WEB-DL</a></td>\n\
  <td><span class=\"seedmed\"><b>29</b></span><span class=\"leechmed\"><b>12</b></span></td>\n\
  <td><a href=\"download.php?id=715016\">21.39&nbsp;GB</a></td>\n\
  <td><span class=\"postdetails\">2026-09-08 16:58</span></td>\n\
</tr>\n\
</table></html>";

#[test]
fn parse_torrents_from_page_fixture_yields_typed_torrents_with_size_and_peers() {
    let html = support::read("Toloka/browse_f96.html");
    let torrents = parser::parse_torrents_from_page(&html, "96");
    assert!(torrents.len() >= 40, "expected >=40 torrents, got {}", torrents.len());

    let prefix = format!("{}/", host());
    for d in &torrents {
        let t = &d.t;
        assert_eq!(t.trackerName, "toloka");
        assert_eq!(t.types, ["movie"]);
        assert!(!t.name.trim().is_empty());
        assert!(!t.title.trim().is_empty());
        assert!(t.url.starts_with(&prefix));
        assert!(t.url.contains("/t"));
        assert!(!d.download_id.is_empty() && d.download_id.chars().all(|c| c.is_ascii_digit()));
        assert!(!t.sizeName.trim().is_empty());
        assert!(!t.sizeName.contains('\u{00A0}'));
        assert!(!t.sizeName.to_lowercase().contains("&nbsp;"));
        assert!(rx::is_match_i(&t.sizeName, r"[0-9][0-9\.,]* (MB|GB|TB|МБ|ГБ|ТБ)"), "{}", t.sizeName);
        assert_ne!(t.sizeName, "0 B");
        assert!(!t.sizeName.contains("Завантажити"));
        assert!(!time::is_min(&t.createTime));
        assert!(t.sid >= 0 && t.pir >= 0);
    }

    let moana: Vec<_> = torrents.iter().filter(|d| d.t.url.ends_with(&format!("/{MOANA_TOPIC}"))).collect();
    assert_eq!(moana.len(), 1);
    let m = moana[0];
    assert_eq!(m.download_id, "715016");
    assert_eq!(m.t.sizeName, "21.39 GB");
    assert_eq!(m.t.sid, 36);
    assert_eq!(m.t.pir, 5);
    assert!(m.t.title.contains("Moana"));
    assert_eq!(m.t.url, format!("{}/{MOANA_TOPIC}", host()));
}

#[test]
fn parse_torrents_from_page_relative_hrefs_yields_topic_and_download_id() {
    let torrents = parser::parse_torrents_from_page(ROW_HTML, "96");
    assert_eq!(torrents.len(), 1);
    let d = &torrents[0];
    assert_eq!(d.t.url, format!("{}/t700099", host()));
    assert_eq!(d.download_id, "715016");
    assert_eq!(d.t.sizeName, "21.39 GB");
    assert_eq!(d.t.sid, 29);
    assert_eq!(d.t.pir, 12);
    assert_eq!(d.t.name, "Ваяна");
    assert_eq!(d.t.originalname, "Moana");
    assert_eq!(d.t.relased, 2026);
}

#[test]
fn parse_torrents_from_page_cat140_hd_docs_yields_doc_types() {
    let torrents = parser::parse_torrents_from_page(ROW_HTML, "140");
    assert_eq!(torrents.len(), 1);
    assert_eq!(torrents[0].t.types, ["docuserial", "documovie"]);
    assert_eq!(torrents[0].t.name, "Ваяна");
    assert_eq!(torrents[0].download_id, "715016");
}

#[test]
fn parse_torrents_from_page_unknown_category_returns_empty() {
    assert!(parser::parse_torrents_from_page(&support::read("Toloka/browse_f96.html"), "999").is_empty());
}

#[test]
fn parse_torrents_from_page_empty_html_returns_empty() {
    assert!(parser::parse_torrents_from_page("", "96").is_empty());
    assert!(parser::parse_torrents_from_page("<html lang=\"uk\"></html>", "96").is_empty());
}

#[test]
fn looks_like_forum_listing_uk_html_without_topics_is_listing() {
    let empty = "<html lang=\"uk\"></html>";
    assert!(parser::looks_like_forum_listing(empty));
    assert!(parser::parse_torrents_from_page(empty, "96").is_empty());
    assert!(parser::looks_like_forum_listing(&support::read("Toloka/browse_f96.html")));
    assert!(!parser::looks_like_forum_listing(""));
    assert!(!parser::looks_like_forum_listing("<html></html>"));
}

#[test]
fn last_page_from_html_browse_f96_is_293() {
    assert_eq!(parser::last_page_from_html(&support::read("Toloka/browse_f96.html")), 293);
    assert_eq!(parser::last_page_from_html(""), 0);
    assert_eq!(parser::last_page_from_html("<html>no pager</html>"), 0);
}

#[test]
fn prune_pages_beyond_page_count_drops_exclusive_tail() {
    let mut pages = vec![TaskParse::new(0), TaskParse::new(8), TaskParse::new(9), TaskParse::new(10)];
    assert_eq!(parser::prune_pages_beyond_page_count(Some(&mut pages), 9), 2);
    assert_eq!(pages.iter().map(|p| p.page).collect::<Vec<_>>(), vec![0, 8]);
    assert_eq!(parser::prune_pages_beyond_page_count(Some(&mut pages), 9), 0);
    assert_eq!(parser::prune_pages_beyond_page_count(None, 9), 0);
}
