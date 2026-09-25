mod support;

use chrono::{TimeZone, Utc};
use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::{conf, time};
use crab_trackers_b::rutracker::categories::{self, TitleKind};
use crab_trackers_b::rutracker::parser;

// ---------------------------------------------------------------------------
// categories
// ---------------------------------------------------------------------------

#[test]
fn map_has_expected_counts() {
    assert_eq!(categories::MAP.len(), 242);
    assert_eq!(categories::quick_parse_ids().len(), 98);
    let ids = categories::ids();
    let uniq: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(uniq.len(), categories::MAP.len());
    assert!(categories::quick_parse_ids().iter().all(|id| categories::MAP.contains_key(id)));
}

#[test]
fn formerly_missing_sections_are_in_quick_parse() {
    for id in [
        "1171", "2366", "189", "2100", "812", "718", "1106", "1669", "2393", "625", "1949", "173", "820", "1242", "717", "2412", "1463", "84",
        "498", "272", "775",
    ] {
        assert!(categories::MAP.contains_key(id), "{id}");
        assert!(categories::quick_parse_ids().contains(&id), "{id}");
    }
}

#[test]
fn map_every_id_has_types_and_title_kind() {
    for (k, v) in categories::MAP.iter() {
        assert!(!k.trim().is_empty());
        assert!(!v.types.is_empty());
        assert!(v.types.iter().all(|t| !t.trim().is_empty()));
    }
}

#[test]
fn former_sport_orphans_are_typed_sport_and_not_quick_parse() {
    for id in ["1392", "2475", "2493", "2113", "2482"] {
        let meta = categories::get(id).expect(id);
        assert_eq!(meta.types, ["sport"]);
        assert_eq!(meta.title_kind, TitleKind::NonStandard);
        assert!(!meta.quick_parse);
    }
}

#[test]
fn map_sample_entries_match_expected() {
    let cases = [
        ("1950", "movie", TitleKind::Movie, true),
        ("842", "serial", TitleKind::Serial, true),
        ("1105", "anime", TitleKind::NonStandard, true),
        ("709", "documovie", TitleKind::Movie, false),
        ("24", "tvshow", TitleKind::NonStandard, false),
        ("915", "serial", TitleKind::NonStandard, true),
        ("1669", "serial", TitleKind::Serial, true),
        ("820", "serial", TitleKind::NonStandard, true),
        ("84", "multfilm", TitleKind::Movie, true),
        ("498", "multserial", TitleKind::Serial, true),
        ("272", "movie", TitleKind::Movie, true),
    ];
    for (id, ty, kind, quick) in cases {
        let meta = categories::get(id).expect(id);
        assert_eq!(meta.types, [ty], "{id}");
        assert_eq!(meta.title_kind, kind, "{id}");
        assert_eq!(meta.quick_parse, quick, "{id}");
    }
}

#[test]
fn doc_serial_has_docuserial_and_documovie() {
    let meta = categories::get("46").unwrap();
    assert_eq!(meta.types, ["docuserial", "documovie"]);
    assert_eq!(meta.title_kind, TitleKind::NonStandard);
    assert!(!meta.quick_parse);
}

#[test]
fn forum_tree_snapshot_p0_leaves_are_in_quick_parse() {
    let json: serde_json::Value = serde_json::from_str(&support::read("Rutracker/forum_tree_snapshot.json")).unwrap();
    let mut p0: Vec<String> = json["forums"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["priority"].as_str() == Some("p0"))
        .filter_map(|f| f["id"].as_str().map(|s| s.to_string()))
        .collect();
    p0.dedup();
    let uniq: std::collections::BTreeSet<_> = p0.iter().cloned().collect();
    assert_eq!(uniq.len(), 14);
    assert!(uniq.contains("1669"));
    let quick = categories::quick_parse_ids();
    for id in &uniq {
        assert!(categories::MAP.contains_key(id.as_str()), "P0 forum {id} missing from map");
        assert!(quick.contains(&id.as_str()));
    }
}

#[test]
fn emptied_sport_archives_are_not_in_map() {
    for id in ["261", "1609", "1999", "2000"] {
        assert!(!categories::MAP.contains_key(id));
    }
}

#[test]
fn sport_is_never_in_quick_parse() {
    let sports: Vec<_> = categories::MAP.iter().filter(|(_, v)| v.types == ["sport"]).collect();
    assert_eq!(sports.len(), 95);
    for (_, v) in sports {
        assert!(!v.quick_parse);
        assert_eq!(v.title_kind, TitleKind::NonStandard);
    }
}

// ---------------------------------------------------------------------------
// listing fixtures
// ---------------------------------------------------------------------------

const SAMPLE_FIXTURES: [(&str, &str); 6] = [
    ("1950", "forum_1950.html"),
    ("842", "forum_842.html"),
    ("1105", "forum_1105.html"),
    ("1392", "forum_1392.html"),
    ("709", "forum_709.html"),
    ("24", "forum_24.html"),
];

#[test]
fn fixture_cases_cover_representative_sample() {
    assert_eq!(SAMPLE_FIXTURES.len(), 6);
    assert!(SAMPLE_FIXTURES.iter().any(|x| x.0 == "1392"));
}

#[test]
fn parse_torrents_from_page_fixture_yields_typed_torrents() {
    let host_prefix = format!("{}/", conf().Rutracker.host.trim_end_matches('/'));
    for (cat, file) in SAMPLE_FIXTURES {
        let expected = categories::get(cat).unwrap().types;
        let html = support::read(&format!("Rutracker/{file}"));
        let torrents = parser::parse_torrents_from_page(&html, cat);
        assert!(torrents.len() >= 8, "expected >=8 torrents for cat {cat}, got {}", torrents.len());
        for t in &torrents {
            assert_eq!(t.trackerName, "rutracker");
            assert_eq!(t.types, expected);
            assert!(!t.name.trim().is_empty());
            assert!(!t.title.trim().is_empty());
            assert!(t.url.starts_with(&host_prefix), "{}", t.url);
            assert!(!t.sizeName.trim().is_empty());
            assert!(!time::is_min(&t.createTime));
        }
    }
}

#[test]
fn sport_orphan_fixture_is_typed_sport_not_dropped() {
    let torrents = parser::parse_torrents_from_page(&support::read("Rutracker/forum_1392.html"), "1392");
    assert!(torrents.len() >= 20, "expected sport yield, got {}", torrents.len());
    for t in &torrents {
        assert_eq!(t.types, ["sport"]);
    }
}

#[test]
fn parse_torrents_from_page_unknown_category_returns_empty() {
    assert!(parser::parse_torrents_from_page(&support::read("Rutracker/forum_1950.html"), "999999").is_empty());
}

#[test]
fn dry_run_sample_fixtures_report_parse_rates() {
    for (cat, file) in SAMPLE_FIXTURES {
        let torrents = parser::parse_torrents_from_page(&support::read(&format!("Rutracker/{file}")), cat);
        let with_year = torrents.iter().filter(|t| t.relased > 0).count();
        let with_orig = torrents.iter().filter(|t| !t.originalname.trim().is_empty()).count();
        println!("DRY-RUN cat={cat:<6} parsed={:>3} withYear={with_year:>3} withOriginal={with_orig:>3}", torrents.len());
        assert!(torrents.len() >= 8, "dry-run cat {cat}: low yield {}", torrents.len());
    }
}

#[test]
fn last_page_from_html_forum1950_is_152() {
    let html = support::read("Rutracker/forum_1950.html");
    assert_eq!(parser::last_page_from_html(&html), 152);
    assert_eq!(parser::last_page_from_html(""), 0);
    assert_eq!(parser::last_page_from_html("<html>no pager</html>"), 0);
    assert!(parser::looks_like_forum_listing(&html));
    assert_eq!(parser::effective_page_count(&html), 152);
}

#[test]
fn effective_page_count_empty_archive_pager_is_one_page() {
    let html = r#"
            <title>Архив (Спорт) [стр. 1] :: Спорт :: RuTracker.org</title>
            <p style="float: left">Страница <b>1</b> из <b>475</b></p>
            <table class="forumline forum"></table>
            "#;
    assert_eq!(parser::last_page_from_html(html), 475);
    assert_eq!(parser::topic_row_count(html), 0);
    assert!(parser::looks_like_forum_listing(html));
    assert_eq!(parser::effective_page_count(html), 1);
    assert!(!parser::looks_like_forum_listing(""));
    assert!(!parser::looks_like_forum_listing("<html>Just a moment...</html>"));
    assert_eq!(parser::effective_page_count(""), 0);
    assert_eq!(parser::effective_page_count("<html>Just a moment...</html>"), 0);
}

#[test]
fn prune_pages_beyond_page_count_drops_exclusive_tail() {
    let mut pages = vec![TaskParse::new(0), TaskParse::new(8), TaskParse::new(9), TaskParse::new(10)];
    assert_eq!(parser::prune_pages_beyond_page_count(Some(&mut pages), 9), 2);
    assert_eq!(pages.iter().map(|p| p.page).collect::<Vec<_>>(), vec![0, 8]);
    assert_eq!(parser::prune_pages_beyond_page_count(Some(&mut pages), 9), 0);
    assert_eq!(parser::prune_pages_beyond_page_count(None, 9), 0);
}

// ---------------------------------------------------------------------------
// topic fetch skip / magnet
// ---------------------------------------------------------------------------

const TITLE: &str = "Укрытие / Бункер / Silo / Сезон: 2 / Серии: 1-10 из 10";
const SIZE: &str = "101.97 GB";
const MAGNET: &str = "magnet:?xt=urn:btih:EE33E008E78DDE68559CDA46AA36C0A6B301DB58&tr=http://bt4.t-ru.org/ann?magnet";

fn cached(title: &str, size: &str, magnet: &str) -> TorrentDetails {
    let d = Utc.with_ymd_and_hms(2026, 2, 4, 12, 8, 3).unwrap();
    TorrentDetails { title: title.into(), sizeName: size.into(), magnet: magnet.into(), createTime: d, updateTime: d, ..Default::default() }
}

fn listing(title: &str, size: &str, create: Option<chrono::DateTime<Utc>>) -> TorrentDetails {
    TorrentDetails {
        title: title.into(),
        sizeName: size.into(),
        createTime: create.unwrap_or_else(|| Utc.with_ymd_and_hms(2026, 2, 4, 1, 51, 0).unwrap()),
        ..Default::default()
    }
}

#[test]
fn should_skip_topic_fetch_same_title_size_and_fresh_magnet_skips() {
    assert!(parser::should_skip_topic_fetch(Some(&cached(TITLE, SIZE, MAGNET)), &listing(TITLE, SIZE, None)));
}

#[test]
fn should_skip_topic_fetch_title_changed_fetches() {
    let l = listing(&format!("{TITLE} [4k]"), SIZE, None);
    assert!(!parser::should_skip_topic_fetch(Some(&cached(TITLE, SIZE, MAGNET)), &l));
}

#[test]
fn should_skip_topic_fetch_size_changed_fetches() {
    assert!(!parser::should_skip_topic_fetch(Some(&cached(TITLE, SIZE, MAGNET)), &listing(TITLE, "120 GB", None)));
}

#[test]
fn should_skip_topic_fetch_listing_newer_than_magnet_write_fetches() {
    let l = listing(TITLE, SIZE, Some(Utc.with_ymd_and_hms(2026, 7, 5, 1, 51, 0).unwrap()));
    assert!(!parser::should_skip_topic_fetch(Some(&cached(TITLE, SIZE, MAGNET)), &l));
}

#[test]
fn should_skip_topic_fetch_empty_magnet_fetches() {
    assert!(!parser::should_skip_topic_fetch(Some(&cached(TITLE, SIZE, "")), &listing(TITLE, SIZE, None)));
    assert!(!parser::should_skip_topic_fetch(None, &listing(TITLE, SIZE, None)));
}

#[test]
fn should_skip_topic_fetch_nbsp_size_still_equal() {
    assert!(parser::should_skip_topic_fetch(Some(&cached(TITLE, SIZE, MAGNET)), &listing(TITLE, "101.97\u{00a0}GB", None)));
}

#[test]
fn apply_topic_page_details_html_decodes_magnet_ampersands() {
    let mut t = TorrentDetails::default();
    let html = "<a class=\"p-link small\" href=\"viewtopic.php?t=6601495\">05-07-26 01:51</a>\
        <a href=\"magnet:?xt=urn:btih:DD734DA14142D211010A82D540431A86C737498E&amp;tr=http%3A%2F%2Fbt4.t-ru.org%2Fann%3Fmagnet&amp;dn=Silo\" class=\"magnet-link\">";
    assert!(parser::apply_topic_page_details(&mut t, Some(html)));
    assert_eq!(t.magnet, "magnet:?xt=urn:btih:DD734DA14142D211010A82D540431A86C737498E&tr=http%3A%2F%2Fbt4.t-ru.org%2Fann%3Fmagnet&dn=Silo");
}

#[test]
fn apply_topic_page_details_med_magnet_link_class_matches() {
    let mut t = TorrentDetails::default();
    assert!(parser::apply_topic_page_details(&mut t, Some("<a href=\"magnet:?xt=urn:btih:AABB\" class=\"med magnet-link\">")));
    assert_eq!(t.magnet, "magnet:?xt=urn:btih:AABB");
}

// ---------------------------------------------------------------------------
// title layouts
// ---------------------------------------------------------------------------

fn forum_row(title: &str) -> String {
    format!(
        "class=\"torTopic\"\n<a id=\"tt-12345\" href=\"viewtopic.php?t=12345\">{title}</a>\n\
         <span class=\"seedmed\" title=\"Seeders\"><b>10</b></span>\n\
         <span class=\"leechmed\" title=\"Leechers\"><b>5</b></span>\n\
         <a class=\"dl-stub\">1.5&nbsp;GB</a>\n<p>2024-07-16 12:00</p>\n"
    )
}

fn single(title: &str, cat: &str) -> TorrentDetails {
    let mut v = parser::parse_torrents_from_page(&forum_row(title), cat);
    assert_eq!(v.len(), 1, "{title}");
    v.remove(0)
}

#[test]
fn movie_parses_name_orig_year() {
    let cases = [
        ("1950", "Белый тигр / The White Tiger (Рамин Бахрани / Ramin Bahrani) [2021, Индия, США, драма, WEB-DLRip]", "Белый тигр", "The White Tiger", 2021),
        ("709", "Гунда / Gunda (Виктор Косаковский) [2020, Норвегия, США, документальный, HDRip]", "Гунда", "Gunda", 2020),
    ];
    for (cat, title, name, orig, year) in cases {
        let t = single(title, cat);
        assert_eq!(t.types, categories::get(cat).unwrap().types);
        assert_eq!(t.name, name);
        assert_eq!(t.originalname, orig);
        assert_eq!(t.relased, year);
    }
}

#[test]
fn serial_parses_name_orig_year() {
    let cases = [
        ("842", "Укрытие / Silo / Сезон: 3 / Серии: 1-2 из 10 (Берт) [2026, США, фантастика, WEB-DLRip]", "Укрытие", "Silo", 2026),
        (
            "1669",
            "Укрытие / Бункер / Silo / Сезон: 2 / Серии: 1-10 из 10 (Майкл Диннер, Арик Авелино) [2024, США, фантастика, драма, триллер, Dolby Vision, HDR10+, WEB-DL 2160p, 4k]",
            "Укрытие",
            "Silo",
            2024,
        ),
    ];
    for (cat, title, name, orig, year) in cases {
        let t = single(title, cat);
        assert_eq!(t.types, ["serial"]);
        assert_eq!(t.name, name);
        assert_eq!(t.originalname, orig);
        assert_eq!(t.relased, year);
    }
}

#[test]
fn non_standard_parses_name_and_year() {
    let cases = [
        ("1105", "Благоухающий цветок / Kaoru Hana wa Rin to Saku [2025, TV, 1080p]", "Благоухаюший цветок", 2025),
        ("24", "Шоу Бенни Хилла - 76 выпусков (Benny Hill Show) [1979, Комедийное шоу, DVDRip]", "Шоу Бенни Хилла - 76 выпусков", 1979),
    ];
    for (cat, title, name, year) in cases {
        let t = single(title, cat);
        assert_eq!(t.types, categories::get(cat).unwrap().types);
        assert_eq!(t.name, name);
        assert_eq!(t.relased, year);
    }
}

#[test]
fn sport_parses_as_sport_with_year_in_brackets() {
    let cases = [
        ("Футбол. Чемпионат мира [2026, Спорт, WEBRip]", "Футбол. Чемпионат мира", 2026),
        ("XXXII Летние Олимпийские Игры / Церемония закрытия [2021, Олимпиада, IPTV]", "XXXII Летние Олимпийские Игры", 2021),
    ];
    for (title, name, year) in cases {
        let t = single(title, "1392");
        assert_eq!(t.types, ["sport"]);
        assert_eq!(t.name, name);
        assert_eq!(t.relased, year);
    }
}
