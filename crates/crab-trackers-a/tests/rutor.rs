mod support;

use crab_core::conf;
use crab_core::time;
use crab_trackers_a::rutor::categories::{self, RutorTitleKind, MAP};
use crab_trackers_a::rutor::parser;
use support::{fixture, pages, types};

// ---------------------------------------------------------------- categories

#[test]
fn map_has_expected_count() {
    assert_eq!(MAP.len(), 11);
    let mut ids: Vec<_> = categories::ids().collect();
    ids.dedup();
    assert_eq!(ids.len(), MAP.len());
}

#[test]
fn map_every_id_has_types() {
    for (k, v) in MAP.iter() {
        assert!(!k.trim().is_empty());
        assert!(!v.types.is_empty());
        assert!(v.types.iter().all(|t| !t.trim().is_empty()));
    }
}

#[test]
fn map_sample_entries_match_expected() {
    let cases = [
        ("1", "movie", RutorTitleKind::ForeignMovie, false),
        ("17", "movie", RutorTitleKind::ForeignMovie, true),
        ("5", "movie", RutorTitleKind::RuMovie, false),
        ("4", "serial", RutorTitleKind::ForeignSerial, false),
        ("16", "serial", RutorTitleKind::RuSerial, false),
        ("10", "anime", RutorTitleKind::ShowLike, false),
        ("13", "sport", RutorTitleKind::ShowLike, false),
    ];
    for (id, ty, kind, ukr) in cases {
        let meta = &MAP[id];
        assert_eq!(meta.types, &[ty]);
        assert_eq!(meta.title_kind, kind);
        assert_eq!(meta.require_ukr_in_title, ukr);
    }
}

#[test]
fn docs_cartoons_sport_types() {
    assert_eq!(MAP["12"].types, &["docuserial", "documovie"]);
    assert_eq!(MAP["12"].title_kind, RutorTitleKind::ShowLike);
    assert_eq!(MAP["7"].types, &["multfilm", "multserial"]);
    assert_eq!(MAP["13"].types, &["sport"]);
}

#[test]
fn only_cat17_requires_ukr() {
    for (k, v) in MAP.iter() {
        assert_eq!(v.require_ukr_in_title, *k == "17", "cat {k}");
    }
}

#[test]
fn map_does_not_include_non_video() {
    for id in ["2", "3", "8", "9", "11", "14"] {
        assert!(!MAP.contains_key(id), "unexpected cat {id}");
    }
}

// ---------------------------------------------------------------- fixtures

fn fixture_cases() -> Vec<(&'static str, String)> {
    let mut v: Vec<_> = MAP.keys().map(|k| (*k, format!("browse_{k}.html"))).collect();
    v.sort_by_key(|(k, _)| k.parse::<i32>().unwrap());
    v
}

#[test]
fn fixtures_yield_typed_torrents() {
    let host = conf().Rutor.host.trim_end_matches('/').to_string() + "/";
    assert_eq!(fixture_cases().len(), MAP.len());
    for (cat, file) in fixture_cases() {
        let html = fixture(&format!("rutor/{file}"));
        let torrents = parser::parse_torrents_from_page(&html, cat);
        let min = if cat == "17" { 1 } else { 40 };
        assert!(torrents.len() >= min, "expected >={min} torrents for cat {cat}, got {}", torrents.len());
        for t in &torrents {
            assert_eq!(t.trackerName, "rutor");
            assert_eq!(types(t), MAP[cat].types);
            assert!(!t.name.trim().is_empty());
            assert!(!t.title.trim().is_empty());
            assert!(t.url.starts_with(&host));
            assert!(t.magnet.to_lowercase().starts_with("magnet:?xt="));
            assert!(!t.sizeName.trim().is_empty());
            assert!(!time::is_min(&t.createTime));
            if cat == "17" {
                assert!(t.title.contains(" UKR"));
            }
        }
    }
}

#[test]
fn sport_fixture_typed_as_sport() {
    let torrents = parser::parse_torrents_from_page(&fixture("rutor/browse_13.html"), "13");
    assert!(torrents.len() >= 40);
    assert!(torrents.iter().all(|t| types(t) == ["sport"]));
}

#[test]
fn unknown_category_returns_empty() {
    assert!(parser::parse_torrents_from_page(&fixture("rutor/browse_1.html"), "2").is_empty());
}

#[test]
fn last_page_from_html_browse10_is_72() {
    assert_eq!(parser::last_page_from_html(&fixture("rutor/browse_10.html")), 72);
    assert_eq!(parser::last_page_from_html(""), 0);
    assert_eq!(parser::last_page_from_html("<p>no pager</p>"), 0);
}

#[test]
fn looks_like_browse_listing_hole_pager_without_rows() {
    let hole = "<html><p><a href=\"/browse/2109/1/0/0\"><b>210901&nbsp;-&nbsp;210908</b></a></p>\
                <table><tr><th>Добавлен</th></tr></table></html>";
    assert!(parser::looks_like_browse_listing(hole));
    assert!(parser::parse_torrents_from_page(hole, "1").is_empty());
    assert!(parser::looks_like_browse_listing(&fixture("rutor/browse_1.html")));
    assert!(!parser::looks_like_browse_listing(""));
    assert!(!parser::looks_like_browse_listing("Just a moment..."));
    assert!(parser::looks_like_browse_listing("<tr class=\"gai\"><td>row</td></tr>"));
}

#[test]
fn prune_pages_beyond_max_drops_ghost_tail() {
    let mut tasks = pages(20);
    assert_eq!(parser::prune_pages_beyond_max(&mut tasks, 11), 8);
    assert_eq!(tasks.len(), 12);
    assert_eq!(tasks.last().unwrap().page, 11);
    assert_eq!(parser::prune_pages_beyond_max(&mut tasks, 11), 0);
    assert_eq!(parser::prune_pages_beyond_max(&mut Vec::new(), 5), 0);
}

// ---------------------------------------------------------------- titles

fn browse_row(title: &str) -> String {
    format!(
        "<table><tr class=\"gai\"><td>16.07.24</td><td><a class=\"downgif\" href=\"/download/1\"></a>\
         <a href=\"magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01\">m</a> \
         <a href=\"/torrent/123/slug\">{title}</a></td>\
         <td align=\"right\">1.5&nbsp;GB</td>\
         <td><span class=\"green\"><img src=\"x.gif\" alt=\"S\" />&nbsp;10</span> \
         <span class=\"red\">&nbsp;5</span></td></tr></table>"
    )
}

fn single(title: &str, cat: &str) -> crab_core::models::TorrentDetails {
    let mut v = parser::parse_torrents_from_page(&browse_row(title), cat);
    assert_eq!(v.len(), 1, "title {title}");
    v.remove(0)
}

#[test]
fn foreign_movie_parses_name_orig_year() {
    for (title, name, orig, year) in [
        ("Опасное небо / Top Gunner (2020) WEB-DL 1080p | P", "Опасное небо", "Top Gunner", 2020),
        (
            "Только течёт река / He bian de cuo wu / Only the River Flows (2023) BDRip 720p",
            "Только течёт река",
            "Only the River Flows",
            2023,
        ),
    ] {
        let t = single(title, "1");
        assert_eq!(types(&t), ["movie"]);
        assert_eq!(t.name, name);
        assert_eq!(t.originalname, orig);
        assert_eq!(t.relased, year);
        assert_eq!(t.sizeName, "1.5 GB");
        assert_eq!((t.sid, t.pir), (10, 5));
    }
}

#[test]
fn ru_movie_parses_name_and_year() {
    let t = single("Не одна дома 3. Выпускной (2026) WEB-DL 1080p", "5");
    assert_eq!(types(&t), ["movie"]);
    assert_eq!(t.name, "Не одна дома 3. Выпускной");
    assert_eq!(t.relased, 2026);
}

#[test]
fn foreign_serial_parses_name_orig_year() {
    let t = single("Дом Дракона / House of the Dragon [03x01-04 из 08] (2026) WEB-DL 1080p", "4");
    assert_eq!(types(&t), ["serial"]);
    assert_eq!(t.name, "Дом Дракона");
    assert_eq!(t.originalname, "House of the Dragon");
    assert_eq!(t.relased, 2026);
}

#[test]
fn ru_serial_parses_name_and_year() {
    let t = single("Холод [01х01 из 10] (2026) WEB-DL 1080p", "16");
    assert_eq!(types(&t), ["serial"]);
    assert_eq!(t.name, "Холод");
    assert_eq!(t.relased, 2026);
}

#[test]
fn sport_parses_as_sport_with_name_and_year() {
    let t = single("Футбол. Чемпионат Мира 2026. Финал. Англия – Аргентина [15.07] (2026) WEBRip 1080p", "13");
    assert_eq!(types(&t), ["sport"]);
    assert_eq!(t.name, "Футбол. Чемпионат Мира 2026. Финал. Англия – Аргентина");
    assert_eq!(t.relased, 2026);
}

#[test]
fn cat17_requires_ukr_in_title() {
    assert!(parser::parse_torrents_from_page(&browse_row("Some Film / Some Film (2024) WEB-DL"), "17").is_empty());
    let t = single("Some Film / Some Film (2024) WEB-DL | UKR", "17");
    assert_eq!(types(&t), ["movie"]);
    assert!(t.title.contains(" UKR"));
}

#[test]
fn skips_trailer_and_kpk() {
    assert!(parser::parse_torrents_from_page(&browse_row("Фильм (2024) трейлер WEB-DL"), "5").is_empty());
    assert!(parser::parse_torrents_from_page(&browse_row("Фильм (2024) КПК WEB-DL"), "5").is_empty());
}
