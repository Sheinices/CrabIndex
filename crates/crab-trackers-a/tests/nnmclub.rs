mod support;

use crab_core::models::TaskParse;
use crab_core::time;
use crab_trackers_a::nnmclub::categories::{self, NNMClubTitleKind, MAP, NON_VIDEO_IDS};
use crab_trackers_a::nnmclub::pagination::{self, PageParseStatus};
use crab_trackers_a::nnmclub::{parser, PAGE_SIZE};
use support::{fixture, types};

// ---------------------------------------------------------------- categories

#[test]
fn map_has_expected_count() {
    assert_eq!(MAP.len(), 13);
    assert_eq!(categories::ids().count(), MAP.len());
    for (k, v) in MAP.iter() {
        assert!(!k.trim().is_empty());
        assert!(!v.types.is_empty());
    }
}

#[test]
fn map_sample_entries_match_expected() {
    for (id, ty, kind) in [
        ("10", "movie", NNMClubTitleKind::ForeignCinema),
        ("13", "movie", NNMClubTitleKind::RuMovie),
        ("3", "serial", NNMClubTitleKind::ForeignSerial),
        ("4", "serial", NNMClubTitleKind::RuSerial),
        ("1", "anime", NNMClubTitleKind::Anime),
        ("24", "sport", NNMClubTitleKind::Sport),
        ("21", "tvshow", NNMClubTitleKind::ShowLike),
        ("27", "tvshow", NNMClubTitleKind::ShowLike),
    ] {
        assert_eq!(MAP[id].types, &[ty]);
        assert_eq!(MAP[id].title_kind, kind);
    }
}

#[test]
fn docs_kids_sport() {
    assert_eq!(MAP["22"].types, &["docuserial", "documovie"]);
    assert_eq!(MAP["22"].title_kind, NNMClubTitleKind::ShowLike);
    let kids = &MAP["7"];
    assert_eq!(kids.types, &["multfilm", "multserial"]);
    assert_eq!(kids.title_kind, NNMClubTitleKind::KidsMult);
    assert!(kids.require_mult_in_row);
    assert!(kids.skip_pdf_in_title);
    assert_eq!(MAP["24"].types, &["sport"]);
}

#[test]
fn map_does_not_include_non_video() {
    for id in NON_VIDEO_IDS {
        assert!(!MAP.contains_key(id), "unexpected cat {id}");
    }
    assert!(NON_VIDEO_IDS.contains(&"28"));
    assert_eq!(PAGE_SIZE, 25);
}

// ---------------------------------------------------------------- fixtures

#[test]
fn fixtures_yield_typed_torrents() {
    let mut cats: Vec<_> = MAP.keys().copied().collect();
    cats.sort_by_key(|c| c.parse::<i32>().unwrap());
    for cat in cats {
        let torrents = parser::parse_torrents_from_page(&fixture(&format!("nnmclub/portal_c{cat}.html")), cat);
        let min = if cat == "7" { 1 } else { 10 };
        assert!(torrents.len() >= min, "expected >={min} torrents for cat {cat}, got {}", torrents.len());
        for t in &torrents {
            assert_eq!(t.trackerName, "nnmclub");
            assert_eq!(types(t), MAP[cat].types);
            assert!(!t.name.trim().is_empty());
            assert!(!t.title.trim().is_empty());
            assert!(t.url.contains("/forum/viewtopic.php?t="));
            assert!(t.magnet.to_lowercase().starts_with("magnet:"));
            assert!(!t.sizeName.trim().is_empty());
            assert!(!time::is_min(&t.createTime));
            assert!(!t.title.to_lowercase().contains("трейлер"));
        }
    }
}

#[test]
fn sport_fixture_typed_as_sport() {
    let torrents = parser::parse_torrents_from_page(&fixture("nnmclub/portal_c24.html"), "24");
    assert!(torrents.len() >= 10);
    assert!(torrents.iter().all(|t| types(t) == ["sport"]));
}

#[test]
fn unknown_category_returns_empty() {
    assert!(parser::parse_torrents_from_page(&fixture("nnmclub/portal_c10.html"), "2").is_empty());
}

// ---------------------------------------------------------------- titles

fn portal_page(row: &str) -> String {
    format!(
        "<html><head><title>NNM-Club</title></head><body><td valign=\"top\" width=\"70%\">{row}\
         <div class=\"paginport nav\">pages</div></td></body></html>"
    )
}

fn pline_row(title: &str, extra: &str) -> String {
    format!(
        "<table width=\"100%\" class=\"pline\">\
         <tr><td><h2 class=\"substr\"><a class=\"pgenmed\" href=\"viewtopic.php?t=12345\">{title}</a></h2></td></tr>\
         <tr><td>{extra}<a href=\"magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01\">m</a>\
         <span title=\"Раздающих\">&nbsp;10</span><span title=\"Качают\">&nbsp;5</span>\
         <span class=\"pcomm bold\">1.5 GB</span>| 16 Июл 2026 16:16:38</span> | <span class=\"tit\">x</span>\
         </td></tr></table>"
    )
}

fn parse(title: &str, extra: &str, cat: &str) -> Vec<crab_core::models::TorrentDetails> {
    parser::parse_torrents_from_page(&portal_page(&pline_row(title, extra)), cat)
}

fn single(title: &str, extra: &str, cat: &str) -> crab_core::models::TorrentDetails {
    let mut v = parse(title, extra, cat);
    assert_eq!(v.len(), 1, "title {title}");
    v.remove(0)
}

#[test]
fn foreign_cinema_parses_name_orig_year() {
    for (title, name, orig, year) in [
        ("Страна грёз / Dreamland (2019) BDRip", "Страна грез", "Dreamland", 2019),
        (
            "Академия монстров / Escuela de Miedo / Cranston Academy: Monster Zone (2020)",
            "Академия монстров",
            "Cranston Academy: Monster Zone",
            2020,
        ),
    ] {
        let t = single(title, "", "10");
        assert_eq!(types(&t), ["movie"]);
        assert_eq!(t.name, name);
        assert_eq!(t.originalname, orig);
        assert_eq!(t.relased, year);
        assert_eq!(t.trackerName, "nnmclub");
        assert!(t.url.contains("viewtopic.php?t=12345"));
        assert_eq!(t.createTime.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-07-16 16:16:38");
    }
}

#[test]
fn ru_movie_and_serial() {
    let t = single("Не одна дома (2024) WEB-DL", "", "13");
    assert_eq!((types(&t), t.name.as_str(), t.relased), (vec!["movie"], "Не одна дома", 2024));
    let t = single("Тайны следствия (2020) WEBRip", "", "4");
    assert_eq!((types(&t), t.name.as_str(), t.relased), (vec!["serial"], "Тайны следствия", 2020));
}

#[test]
fn foreign_serial_uses_foreign_cinema_parser() {
    let t = single("Тьма / The Dark (2026) WEB-DL", "", "3");
    assert_eq!(types(&t), ["serial"]);
    assert_eq!(t.name, "Тьма");
    assert_eq!(t.originalname, "The Dark");
    assert_eq!(t.relased, 2026);
}

#[test]
fn sport_parses_as_sport() {
    let t = single("MotoGP. Этап 11 (2026) WEBRip", "", "24");
    assert_eq!(types(&t), ["sport"]);
    assert_eq!(t.name, "MotoGP. Этап 11");
    assert_eq!(t.relased, 2026);
}

#[test]
fn skips_trailer_in_title() {
    assert!(parse("Фильм (2024) трейлер WEB-DL", "", "10").is_empty());
}

#[test]
fn kids_requires_cartoon_hint_and_skips_pdf() {
    let title = "Спина к спине (2020) WEBRip";
    assert!(parse(title, "просто книга", "7").is_empty());
    let t = single(title, "Продолжительность: 01:20:00 мультфильм", "7");
    assert_eq!(types(&t), ["multfilm", "multserial"]);
    assert_eq!(t.name, "Спина к спине");
    assert_eq!(t.relased, 2020);
    assert!(parse("Сказки PDF (2020)", "мульт", "7").is_empty());
    assert_eq!(single("Спина к спине (2020)", "Длительность: 90 мин", "7").name, "Спина к спине");
}

// ---------------------------------------------------------------- pagination

#[test]
fn clamp_task_page_count_respects_portal_limit() {
    for (max, expected) in [(2333, 500), (499, 500), (500, 500), (84, 85), (0, 1), (-1, 1)] {
        assert_eq!(pagination::clamp_task_page_count(max), expected, "max {max}");
    }
}

#[test]
fn portal_limit_faq_fixture() {
    let html = fixture("nnmclub/portal_limit_faq.html");
    assert!(pagination::is_portal_limit_faq(&html));
    assert_eq!(pagination::classify_page(Some(&html), 0), PageParseStatus::PortalLimitFaq);

    let normal = fixture("nnmclub/portal_c6.html");
    assert!(!pagination::is_portal_limit_faq(&normal));
    assert!(pagination::looks_like_portal_listing(&normal));
}

#[test]
fn classify_page_statuses() {
    let html = "<title>Cat :: NNM-Club</title><div class=\"paginport nav\">pages</div>";
    let s = pagination::classify_page(Some(html), 0);
    assert_eq!(s, PageParseStatus::EmptyPortal);
    assert!(pagination::should_settle_task(s));

    let s = pagination::classify_page(Some("<html></html>"), 0);
    assert_eq!(s, PageParseStatus::TransientError);
    assert!(!pagination::should_settle_task(s));

    let s = pagination::classify_page(Some(html), 3);
    assert_eq!(s, PageParseStatus::OkWithTorrents);
    assert!(pagination::should_settle_task(s));
}

#[test]
fn prune_tasks_beyond_portal_limit() {
    let mut tasks: Vec<TaskParse> = [0, 499, 500, 11371].into_iter().map(TaskParse::new).collect();
    assert_eq!(pagination::prune_tasks_beyond_portal_limit(&mut tasks), 2);
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|t| t.page < pagination::MAX_PORTAL_PAGES));

    let total = 11372;
    let mut all: Vec<TaskParse> = (0..total).map(TaskParse::new).collect();
    let pruned = pagination::prune_tasks_beyond_portal_limit(&mut all);
    assert_eq!(pruned, total - pagination::MAX_PORTAL_PAGES);
    assert_eq!(all.len() as i32, pagination::MAX_PORTAL_PAGES);
    assert!(pruned >= 6286);
    assert_eq!(pagination::clamp_task_page_count(2333), pagination::MAX_PORTAL_PAGES);
}
