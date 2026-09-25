mod support;

use chrono::{TimeZone, Utc};
use crab_core::models::TaskParse;
use crab_core::time;
use crab_trackers_d::anibelka::{categories, parser};
use parser::{AnibelkaListingItem, AnibelkaTopicInfo};

const HOST: &str = "https://anibelka.com";

#[test]
fn sections_are_anime_only() {
    assert_eq!(categories::MAP.len(), 5);
    let c = categories::get("33").expect("section 33");
    assert_eq!(c.types, &["anime"]);
    assert_eq!(parser::TRACKER_NAME, "anibelka");
}

#[test]
fn parse_listing_html_fixture_skips_service_topics() {
    let html = support::read("Anibelka/forum_f33.html");
    let items = parser::parse_listing_html(&html);
    assert!(!items.is_empty(), "no topics parsed");
    for it in &items {
        assert!(!it.topic_id.trim().is_empty());
        assert!(it.title.starts_with('['));
    }
    assert_eq!(items[0].topic_id, "2326");
    assert_eq!(
        items[0].title,
        "[rus] Операция: Семейка Ёдзакура / Yozakura-san Chi no Daisakusen [2TV+ONA][2024-2026, приключения, комедия, романтика]"
    );
}

#[test]
fn listing_fixtures_all_sections_parse() {
    for id in categories::ids() {
        let html = support::read(&format!("Anibelka/forum_f{id}.html"));
        assert!(!parser::parse_listing_html(&html).is_empty(), "section {id}");
    }
}

#[test]
fn parse_topic_html_rus_fixture_picks_torrent_not_poster() {
    let html = support::read("Anibelka/topic_rus.html");
    let info = parser::try_parse_topic_html(&html).expect("topic parsed");
    assert_eq!(info.torrent_id, "7316");
    assert_eq!(info.size_name, "4.75 ГБ");
    assert_eq!(info.sid, 5);
    assert_eq!(info.pir, 0);
    assert_eq!(info.create_time, Utc.with_ymd_and_hms(2026, 7, 23, 5, 56, 0).unwrap());
}

#[test]
fn parse_topic_html_feature_film_fixture_has_torrent() {
    let html = support::read("Anibelka/topic_mv.html");
    let info = parser::try_parse_topic_html(&html).expect("topic parsed");
    assert!(!info.torrent_id.trim().is_empty());
    assert!(!info.size_name.trim().is_empty());
}

#[test]
fn parse_title_cases() {
    let cases: &[(&str, &str, &str, i32)] = &[
        (
            "[rus] Фермерская жизнь в ином мире / Isekai Nonbiri Nouka [2TV][2023-2026, повседневность]",
            "Фермерская жизнь в ином мире",
            "Isekai Nonbiri Nouka",
            2023,
        ),
        ("[mv] Вторая страна / Ni no Kuni [R,S][2019, приключения, фэнтези]", "Вторая страна", "Ni no Kuni", 2019),
        (
            "[uni] Вампир не умеет правильно сосать / Chanto Suenai Kyuuketsuki-chan / Li'l Miss Vampire [TV][2024, комедия]",
            "Вампир не умеет правильно сосать",
            "Chanto Suenai Kyuuketsuki-chan",
            2024,
        ),
        (
            "[rus] Туалетный мальчик Ханако / Ханако после школы / Jibaku Shounen Hanako-kun / Houkago Shounen Hanako-kun [TV][2020, мистика]",
            "Туалетный мальчик Ханако",
            "Jibaku Shounen Hanako-kun",
            2020,
        ),
        ("[uni] P-15 / R-15 [TV+OVA][2011, комедия, школа, этти]", "P-15", "R-15", 2011),
        ("[psp] Хёка / Hyouka", "Хёка", "Hyouka", 0),
    ];
    for (title, name, orig, year) in cases {
        let (n, o, y) = parser::parse_title(title);
        assert_eq!((n.as_str(), o.as_str(), y), (*name, *orig, *year), "{title}");
    }
}

#[test]
fn category_tag_reads_prefix() {
    assert_eq!(parser::category_tag("[rus] X / Y"), "rus");
    assert_eq!(parser::category_tag("X"), "");
}

#[test]
fn parse_ru_date_moscow_to_utc() {
    assert_eq!(parser::parse_ru_date("23 июл 2026, 08:56"), Utc.with_ymd_and_hms(2026, 7, 23, 5, 56, 0).unwrap());
    assert!(time::is_min(&parser::parse_ru_date("вчера")));
    assert!(time::is_min(&parser::parse_ru_date("")));
    assert!(time::is_min(&parser::parse_ru_date("32 abc 2026")));
    for mon in ["янв", "фев", "мар", "апр", "май", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"] {
        assert!(!time::is_min(&parser::parse_ru_date(&format!("01 {mon} 2026, 00:00"))), "{mon}");
    }
}

#[test]
fn last_page_from_html_fixture_is_40() {
    let html = support::read("Anibelka/forum_f33.html");
    assert_eq!(parser::last_page_from_html(&html), 40);
    assert_eq!(parser::last_page_from_html("<html>no pagination</html>"), 0);
}

#[test]
fn last_page_from_html_ignores_script_start_numbers() {
    let html = support::read("Anibelka/forum_f33.html") + "<script>var junk='?start=99999'; location='?start=88888';</script>";
    assert_eq!(parser::last_page_from_html(&html), 40);
}

#[test]
fn prune_pages_beyond_max_drops_ghost_tail() {
    let mut tasks: Vec<TaskParse> = (0..20).map(TaskParse::new).collect();
    assert_eq!(parser::prune_pages_beyond_max(&mut tasks, 11), 8);
    assert_eq!(tasks.len(), 12);
    assert_eq!(tasks.last().map(|t| t.page), Some(11));
    assert_eq!(parser::prune_pages_beyond_max(&mut tasks, 11), 0);
    assert_eq!(parser::prune_pages_beyond_max(&mut Vec::new(), 5), 0);
}

#[test]
fn build_torrent_sets_anime_fields() {
    let item = AnibelkaListingItem {
        topic_id: "1849".into(),
        title: "[rus] Фермерская жизнь в ином мире / Isekai Nonbiri Nouka [2TV][2023-2026, повседневность]".into(),
    };
    let info = AnibelkaTopicInfo { torrent_id: "7316".into(), size_name: "4.75 ГБ".into(), sid: 5, pir: 0, create_time: Utc::now() };
    let magnet = "magnet:?xt=urn:btih:a2e092da06e84fe18b9dc5ca20bf5cc896fceaeb";

    let rec = parser::build_torrent(HOST, &item, &info, magnet).expect("record");
    assert_eq!(rec.t.url, format!("{HOST}/viewtopic.php?t=1849"));
    assert_eq!(rec.t.name, "Фермерская жизнь в ином мире");
    assert_eq!(rec.t.originalname, "Isekai Nonbiri Nouka");
    assert_eq!(rec.t.sid, 5);
    assert_eq!(rec.t.types, vec!["anime".to_string()]);
    assert_eq!(rec.download_id, "7316");

    assert!(parser::build_torrent(HOST, &item, &info, "").is_none());
}

#[test]
fn sync_sources_never_log_in() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("anibelka");
    for file in ["mod.rs", "parser.rs"] {
        let src = std::fs::read_to_string(dir.join(file)).expect("source file");
        for forbidden in ["take_login", "ucp.php?mode=login", "login_u", "login_p", ".cookie("] {
            assert!(!src.contains(forbidden), "{file} contains {forbidden}");
        }
    }
}

#[test]
fn parse_listing_html_empty_returns_empty() {
    assert!(parser::parse_listing_html("").is_empty());
    assert!(parser::parse_listing_html("<html></html>").is_empty());
}

#[test]
fn urls() {
    assert_eq!(parser::forum_url("https://h/", "33", 0), "https://h/viewforum.php?f=33");
    assert_eq!(parser::forum_url("https://h", "33", 2), "https://h/viewforum.php?f=33&start=30");
    assert_eq!(parser::torrent_download_url("https://h", "7"), "https://h/download/file.php?id=7");
}
