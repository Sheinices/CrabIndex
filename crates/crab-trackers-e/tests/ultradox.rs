mod common;

use std::collections::HashSet;

use chrono::Utc;
use common::fixture;
use crab_core::models::TaskParse;
use crab_core::time;
use crab_trackers_e::ultradox::categories;
use crab_trackers_e::ultradox::parser::{self, UltradoxDetailInfo, UltradoxListingItem, UltradoxMagnetVariant};

const HOST: &str = "https://ultradox.vip";

#[test]
fn sections_and_referer() {
    assert_eq!(categories::MAP.len(), 6);
    assert_eq!(parser::TRACKER_NAME, "ultradox");
    assert!(categories::contains("serial-hd"));
    assert!(parser::SEARCH_ENGINE_REFERER.contains("google."));
}

#[test]
fn parse_listing_html_fixture_has_18_rows() {
    let html = fixture("ultradox/listing_serial-hd.html");
    let items = parser::parse_listing_html(&html);
    assert_eq!(items.len(), 18);
    assert_eq!(items[0].detail_url, "/serial-hd/57542-oskolki-pravdy-1-sezon.html");
    assert!(items[0].title.contains("Осколки правды"));
    assert!(!time::is_min(&items[0].create_time));
    for it in &items {
        assert!(!it.title.trim().is_empty());
        assert!(it.detail_url.starts_with('/'));
        assert!(!it.title.contains("magnet:"));
    }
}

#[test]
fn listing_magnets_are_still_placeholders() {
    let html = fixture("ultradox/listing_serial-hd.html");
    assert!(parser::listing_magnets_are_placeholders(&html));
}

#[test]
fn try_parse_detail_html_three_quality_variants() {
    let html = fixture("ultradox/detail_serial.html");
    let (variants, info) = parser::try_parse_detail_html(&html).expect("variants");
    assert_eq!(variants.len(), 3);
    assert_eq!(info.year, 2026);
    assert_eq!(info.original, "Fragments of Truth");
    let qualities: HashSet<&str> = variants.iter().map(|v| v.quality.as_str()).collect();
    assert!(qualities.contains("1080p"));
    assert!(qualities.contains("720p"));
    assert!(qualities.contains("400p"));
    for v in &variants {
        assert_eq!(v.hash.len(), 40);
        assert!(!v.magnet.trim().is_empty());
        assert!(v.magnet.to_lowercase().contains("magnet:?xt=urn:btih:"));
        assert!(!v.magnet.contains("btih:&"));
    }
}

#[test]
fn extract_detail_year_fixture_is_2026() {
    let html = fixture("ultradox/detail_serial.html");
    assert_eq!(parser::extract_detail_year(&html), 2026);
}

#[test]
fn parse_row_date_shapes() {
    for (input, want_zero) in
        [("02-04-2025, 14:32", false), ("Сегодня, 10:06", false), ("Вчера, 22:05", false), ("позавчера", true), ("", true)]
    {
        let got = parser::parse_row_date(input);
        assert_eq!(time::is_min(&got), want_zero, "input={input:?}");
    }
    // Moscow → UTC
    let d = parser::parse_row_date("02-04-2025, 14:32");
    assert_eq!(d.format("%Y-%m-%d %H:%M").to_string(), "2025-04-02 11:32");
}

#[test]
fn parse_title_cases() {
    let cases: [(&str, &str, &str, i32); 11] = [
        ("Ип Ман: Битва кланов (2026) (ПМ) [BDRip]", "Ип Ман: Битва кланов", "", 2026),
        ("30 ночей с бывшим (2025) (Дубляж [Чистый звук]) [BDRip]", "30 ночей с бывшим", "", 2025),
        ("Мотор Сити (Автомобильный город) (2025) (Дубляж) [Telecine]", "Мотор Сити (Автомобильный город)", "", 2025),
        ("Трасса «Море - море» (2026) (Оригинал) [Telecine]", "Трасса «Море - море»", "", 2026),
        ("Эйфория (3 сезон) [+9 серия] [Ultradox]", "Эйфория", "", 0),
        ("Боевой петух (1 сезон) [+12 серия] (ПМ) [WEB-DL]", "Боевой петух", "", 0),
        ("Триган: Наблюдая за звёздами (2 сезон) [+12 серия] [Ultradox]", "Триган: Наблюдая за звёздами", "", 0),
        ("Проект Пуля/Пуля (1 сезон) [+12 серия] [Ultradox]", "Проект Пуля/Пуля", "", 0),
        ("Губка Боб квадратные штаны (17 сезон) [+6 серия] [Ultradox]", "Губка Боб квадратные штаны", "", 0),
        ("Некий сериал [+5 серия] [Ultradox]", "Некий сериал", "", 0),
        ("Астрид и Рафаэлла / Astrid et Raphaëlle (2025) [WEB-DL]", "Астрид и Рафаэлла", "Astrid et Raphaëlle", 2025),
    ];
    for (title, name, orig, year) in cases {
        let (n, o, y) = parser::parse_title(title);
        assert_eq!((n.as_str(), o.as_str(), y), (name, orig, year), "title={title}");
    }
}

#[test]
fn episode_counter_does_not_change_identity() {
    let (before, _, _) = parser::parse_title("Эйфория (3 сезон) [+9 серия] [Ultradox]");
    let (after, _, _) = parser::parse_title("Эйфория (3 сезон) [+10 серия] [Ultradox]");
    assert_eq!(before, after);
    assert_eq!(before, "Эйфория");
}

#[test]
fn original_from_filename_cases() {
    let cases = [
        ("Euphoria.US.S03.1080p.Ru.Ultradox.torrent", "Euphoria"),
        ("Taakstraf.S01.1080p.Ru.Ultradox.torrent", "Taakstraf"),
        ("SpongeBob.SquarePants.S17.720p.Ru.Ultradox.torrent", "SpongeBob SquarePants"),
        ("Trigun.Stargaze.S02.720p.Ru.Ultradox.torrent", "Trigun Stargaze"),
        ("Shumatsu.no.Valkyrie.S03.1080p.Ultradox.torrent", "Shumatsu no Valkyrie"),
        (
            "Life.Larry.and.the.Pursuit.of.Unhappiness.An.Almost.History.of.America.S01.1080p.Ru.Ultradox.torrent",
            "Life Larry and the Pursuit of Unhappiness An Almost History of America",
        ),
        ("The.Death.Of.Robin.Hood.2026.D.BDRip.avi.torrent", "The Death Of Robin Hood"),
        ("Yellow.Letters.2026.Pm.BDRip.1O8Op.mkv.torrent", "Yellow Letters"),
        ("30.Notti.con.il.mio.ex.2025.D.BDRip.avi.torrent", "30 Notti con il mio ex"),
        ("Game.of.Shark.2024.Pk.WEB-DL.1O8Op.mkv.torrent", "Game of Shark"),
        ("State.of.Ramadhani.Dharyu.Dhani.Nu.Thay.2026.Pk.TELECINE.avi.torrent", "State of Ramadhani Dharyu Dhani Nu Thay"),
        ("Trassa.more.more.2026O.TELECINE.1O8Op.mkv.torrent", "Trassa more more"),
        ("S01.1080p.Ru.Ultradox.torrent", ""),
        ("2026.D.BDRip.torrent", ""),
        ("", ""),
    ];
    for (dn, want) in cases {
        assert_eq!(parser::original_from_filename(dn), want, "dn={dn}");
    }
}

#[test]
fn original_is_stable_across_variants() {
    let groups: [&[&str]; 3] = [
        &["30.Notti.con.il.mio.ex.2025.D.BDRip.avi.torrent", "30.Notti.con.il.mio.ex.2025.D.BDRip.1O8Op.mkv.torrent"],
        &["Svoya.v.dosku.2026.O.WEB-DLRip.avi.torrent", "Svoya.v.dosku.2026.O.WEB-DL.1O8Op.mkv.torrent"],
        &[
            "Euphoria.US.S03.1080p.Ru.Ultradox.torrent",
            "Euphoria.US.S03.720p.Ru.Ultradox.torrent",
            "Euphoria.US.S03.400p.Ru.Ultradox.torrent",
        ],
    ];
    for g in groups {
        let first = parser::original_from_filename(g[0]);
        for dn in &g[1..] {
            assert_eq!(parser::original_from_filename(dn), first);
        }
    }
}

fn variant(hash: &str, magnet: &str, bytes: i64, dn: &str, quality: &str) -> UltradoxMagnetVariant {
    UltradoxMagnetVariant { hash: hash.into(), magnet: magnet.into(), bytes, dn: dn.into(), quality: quality.into() }
}

#[test]
fn quality_variants_share_bucket_identity_distinct_urls() {
    let item = UltradoxListingItem {
        title: "Эйфория (3 сезон) [+9 серия] [Ultradox]".into(),
        detail_url: "/serial-hd/54741-jejforija-3-sezon.html".into(),
        create_time: Utc::now(),
        imdb: String::new(),
    };
    let info = UltradoxDetailInfo { year: 2026, original: "Euphoria".into() };
    let variants = [
        variant("0474f44b58fbec31ec145d610a74488a8231f214", "magnet:?a", 21648023723, "x.1080p.torrent", "1080p"),
        variant("e89466561abc2312894f75d39e6783d4712fa0e4", "magnet:?b", 13940626498, "x.720p.torrent", "720p"),
        variant("19ca78e954b198c8c86d5b7f76cf1aa625514e3e", "magnet:?c", 8619991040, "x.400p.torrent", "400p"),
    ];
    let mut keys = HashSet::new();
    let mut urls = HashSet::new();
    for v in &variants {
        let rec = parser::build_torrent(HOST, "serial-hd", &["serial"], &item, v, Some(&info)).expect("record");
        assert_eq!(rec.name, "Эйфория");
        assert_eq!(rec.originalname, "Euphoria");
        assert_eq!(rec.relased, 2026);
        assert_eq!(rec.sid, 1);
        assert_eq!(rec.pir, 1);
        assert!(rec.title.to_lowercase().contains(&v.quality.to_lowercase()));
        keys.insert(format!("{}|{}", rec.name, rec.originalname));
        urls.insert(rec.url);
    }
    assert_eq!(keys.len(), 1);
    assert_eq!(urls.len(), 3);
}

#[test]
fn rufilm_keeps_original_empty() {
    let item = UltradoxListingItem {
        title: "Своя в доску (2026) (Оригинал) [WEB-DL]".into(),
        detail_url: "/rufilm/1-x.html".into(),
        create_time: Utc::now(),
        imdb: String::new(),
    };
    let v = variant("abc1234567890", "magnet:?x", 0, "Svoya.v.dosku.2026.O.WEB-DL.1O8Op.mkv.torrent", "1080p");
    let info = UltradoxDetailInfo { year: 2026, original: "Svoya v dosku".into() };
    let ru = parser::build_torrent(HOST, "rufilm", &["movie"], &item, &v, Some(&info)).expect("ru");
    assert_eq!(ru.originalname, "");
    let hd = parser::build_torrent(HOST, "hd", &["movie"], &item, &v, Some(&info)).expect("hd");
    assert_eq!(hd.originalname, "Svoya v dosku");
}

#[test]
fn last_page_uses_in_content_pager_not_inflated_footer() {
    let html = fixture("ultradox/listing_serial-hd.html");
    assert_eq!(parser::last_page_from_html(&html, None), 317);
    assert_eq!(parser::last_page_from_html(&html, Some("serial-hd")), 317);
    assert_eq!(parser::last_page_from_html(&html, Some("webrips")), 1);
}

#[test]
fn last_page_ignores_script_page_numbers_and_takes_min_section_pager() {
    let html = r#"<script>var junk="/page/2613/";</script>
<div class="pages ultrabold"><a href="https://x/webrips/page/2613/">2613</a></div>
<div class="pages ultrabold"><a href="https://x/webrips/page/12/">12</a></div>"#;
    assert_eq!(parser::last_page_from_html(html, Some("webrips")), 12);
    assert_eq!(parser::last_page_from_html(html, None), 12);
    assert_eq!(parser::last_page_from_html("", None), 1);
}

#[test]
fn last_page_live_shaped_footer_inflation_takes_in_content() {
    let html = r#"<div class="pages ultrabold"><a href="/serial-hd/page/320/">320</a></div>
<div class="pages ultrabold"><a href="/serial-hd/page/429/">429</a></div>"#;
    assert_eq!(parser::last_page_from_html(html, Some("serial-hd")), 320);
}

#[test]
fn last_page_webrips_fixture_uses_in_content_pager() {
    let html = fixture("ultradox/listing_webrips.html");
    assert_eq!(parser::last_page_from_html(&html, Some("webrips")), 2323);
    assert_eq!(parser::last_page_from_html(&html, None), 2323);
    assert_eq!(parser::last_page_from_html(&html, Some("serial-hd")), 1);
}

#[test]
fn listing_fixtures_for_all_sections_parse() {
    for section in categories::ids() {
        let html = fixture(&format!("ultradox/listing_{section}.html"));
        let items = parser::parse_listing_html(&html);
        assert!(!items.is_empty(), "section={section}");
        assert!(parser::last_page_from_html(&html, Some(section)) >= 1);
    }
}

#[test]
fn prune_pages_beyond_max_drops_ghost_tail() {
    let mut tasks: Vec<TaskParse> = (1..=20).map(TaskParse::new).collect();
    assert_eq!(parser::prune_pages_beyond_max(Some(&mut tasks), 12), 8);
    assert_eq!(tasks.len(), 12);
    assert_eq!(tasks[tasks.len() - 1].page, 12);
    assert_eq!(parser::prune_pages_beyond_max(Some(&mut tasks), 12), 0);
    assert_eq!(parser::prune_pages_beyond_max(None, 5), 0);
}

#[test]
fn parse_listing_html_empty_returns_empty() {
    assert!(parser::parse_listing_html("").is_empty());
    assert!(parser::parse_listing_html("<html></html>").is_empty());
}

#[test]
fn absolute_on_host_rewrites_numbered_mirror_onto_configured_host() {
    assert_eq!(
        parser::absolute_on_host("https://ultradox.vip", "https://002.ultradox.vip/serial-hd/57542-x.html"),
        "https://ultradox.vip/serial-hd/57542-x.html"
    );
    assert_eq!(parser::absolute_on_host("https://ultradox.vip/", "/nerufilm/57686-x.html"), "https://ultradox.vip/nerufilm/57686-x.html");
}

#[test]
fn canonical_path_and_fragment_is_host_independent_and_keeps_hash_quality() {
    let path = "/serial-hd/57542-oskolki-pravdy-1-sezon.html#h=a1b2c3d4";
    assert_eq!(parser::canonical_path_and_fragment(&format!("https://ultradox.onl{path}")), path);
    assert_eq!(parser::canonical_path_and_fragment(&format!("https://001.ultradox.vip{path}")), path);
    assert_eq!(parser::canonical_path_and_fragment(&format!("https://002.ultradox.vip{path}")), path);
    assert_eq!(parser::canonical_torrent_url("https://ultradox.vip", &format!("https://ultradox.onl{path}")), format!("https://ultradox.vip{path}"));
    let a = parser::canonical_path_and_fragment("https://ultradox.onl/serial-hd/x.html#h=aaaa1111");
    let b = parser::canonical_path_and_fragment("https://ultradox.vip/serial-hd/x.html#h=bbbb2222");
    assert_ne!(a, b);
}

#[test]
fn build_torrent_absolute_mirror_href_stores_configured_host() {
    let item = UltradoxListingItem {
        title: "Тест (2026)".into(),
        detail_url: "https://002.ultradox.vip/serial-hd/57542-x.html".into(),
        create_time: Utc::now(),
        imdb: String::new(),
    };
    let v = variant("abcdef0123456789", "magnet:?xt=urn:btih:abcdef0123456789", 1, "x.1080p.torrent", "1080p");
    let info = UltradoxDetailInfo { year: 2026, original: String::new() };
    let rec = parser::build_torrent(HOST, "serial-hd", &["serial"], &item, &v, Some(&info)).expect("record");
    assert_eq!(rec.url, "https://ultradox.vip/serial-hd/57542-x.html#h=abcdef01");
}

#[test]
fn human_size_formats() {
    assert_eq!(parser::human_size(0), "");
    assert_eq!(parser::human_size(512), "512 B");
    assert_eq!(parser::human_size(21648023723), "20.16 GB");
}

#[test]
fn parse_all_starter_is_registered_with_cycle_paths() {
    crab_trackers_e::init();
    let names: Vec<&str> = crab_core::trackers::parse_all_starters().iter().map(|s| s.tracker_name()).collect();
    assert!(names.contains(&"ultradox"));
    assert!(!names.contains(&"knaben"));
    assert!(!names.contains(&"rudub"));
    assert!(!names.contains(&"subsplease"));
    assert_eq!(crab_core::trackers::cycle::cycle_path_for_tracker("ultradox"), "Data/temp/ultradox_parseAllCycle.json");
    assert_eq!(crab_core::trackers::cycle::task_parse_path_for_tracker("ultradox"), "Data/temp/ultradox_taskParse.json");
}
