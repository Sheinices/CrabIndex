mod support;

use crab_core::{conf, rx, time};
use crab_trackers_a::torrentby::categories::{self, TorrentByTitleKind, MAP};
use crab_trackers_a::torrentby::{pagination, parser};
use support::{fixture, pages, types};
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------- categories

#[test]
fn map_has_expected_count_and_includes_series() {
    assert_eq!(MAP.len(), 9);
    assert!(MAP.contains_key("series"));
    assert_eq!(categories::ids().count(), MAP.len());
}

#[test]
fn map_types_match_expected() {
    for (cat, ty) in [
        ("films", "movie"),
        ("movies", "movie"),
        ("serials", "serial"),
        ("series", "serial"),
        ("tv", "tvshow"),
        ("humor", "tvshow"),
        ("anime", "anime"),
        ("sport", "sport"),
    ] {
        assert_eq!(MAP[cat].types, &[ty], "cat {cat}");
    }
    assert_eq!(MAP["cartoons"].types, &["multfilm", "multserial"]);
    assert_eq!(MAP["sport"].title_kind, TorrentByTitleKind::Sport);
}

#[test]
fn map_title_kind_match_expected() {
    for (cat, kind) in [
        ("films", TorrentByTitleKind::FilmsForeign),
        ("movies", TorrentByTitleKind::FilmsRu),
        ("serials", TorrentByTitleKind::SerialForeign),
        ("series", TorrentByTitleKind::SerialRu),
        ("tv", TorrentByTitleKind::ShowLike),
    ] {
        assert_eq!(MAP[cat].title_kind, kind);
    }
}

#[test]
fn map_does_not_include_non_video() {
    for id in ["music", "games", "books", "software", "soft", "other", "belarus"] {
        assert!(!MAP.contains_key(id), "unexpected cat {id}");
    }
}

// ---------------------------------------------------------------- pagination

#[test]
fn legacy_max_page_regex_does_not_match_films_fixture() {
    assert!(!rx::is_match(&fixture("torrentby/browse_films.html"), pagination::LEGACY_MAX_PAGE_REGEX));
}

#[test]
fn parse_pager_films_fixture_has_trailing_ellipsis_jump10() {
    let p = pagination::parse_pager(&fixture("torrentby/browse_films.html"));
    assert_eq!(p.current_display, 1);
    assert_eq!(p.max_page_index, 10);
    assert!(p.has_trailing_ellipsis);
    assert_eq!(p.ellipsis_jump_page, Some(10));
}

#[test]
fn parse_pager_anime_and_tv_missing_pager_returns_page_zero() {
    for f in ["browse_anime.html", "browse_tv.html"] {
        let p = pagination::parse_pager(&fixture(&format!("torrentby/{f}")));
        assert_eq!(p.max_page_index, 0);
        assert!(!p.has_trailing_ellipsis);
        assert_eq!(p.ellipsis_jump_page, None);
    }
}

#[test]
fn parse_pager_last_page_without_ellipsis_uses_current_display() {
    let html = r#"<center style="margin-top:5px;">Страницы:
<a href="?page=47"><span class="circle_page">48</span></a>
<a href="?page=48"><span class="circle_page">49</span></a>
<span class="circle_page" style="background:#BFDBFF;">50</span>
</center></td>"#;
    let p = pagination::parse_pager(html);
    assert_eq!(p.current_display, 50);
    assert_eq!(p.max_page_index, 49);
    assert!(!p.has_trailing_ellipsis);
    assert_eq!(p.ellipsis_jump_page, None);
}

#[test]
fn parse_pager_missing_pager_returns_page_zero() {
    let p = pagination::parse_pager("<table><tr class=\"ttable_col1\"></tr></table>");
    assert_eq!(p.max_page_index, 0);
    assert!(!p.has_trailing_ellipsis);
    assert_eq!(p.ellipsis_jump_page, None);
}

#[test]
fn prune_pages_beyond_max() {
    let mut tasks = pages(20);
    assert_eq!(pagination::prune_pages_beyond_max(&mut tasks, 11), 8);
    assert_eq!(tasks.len(), 12);
    assert_eq!(tasks.last().unwrap().page, 11);
    assert_eq!(pagination::prune_pages_beyond_max(&mut tasks, 11), 0);
    assert_eq!(pagination::prune_pages_beyond_max(&mut Vec::new(), 5), 0);

    let mut tasks = pages(5);
    assert_eq!(pagination::prune_pages_beyond_max(&mut tasks, 0), 4);
    assert_eq!(tasks.iter().map(|t| t.page).collect::<Vec<_>>(), vec![0]);
}

#[tokio::test]
async fn shrink_to_last_non_empty_stops_at_last_row() {
    let ct = CancellationToken::new();
    let last = pagination::shrink_to_last_non_empty(148, |p| async move { Some(p <= 53) }, &ct).await.unwrap();
    assert_eq!(last, 53);
}

#[tokio::test]
async fn shrink_to_last_non_empty_failed_probe_keeps_claimed() {
    let ct = CancellationToken::new();
    let last = pagination::shrink_to_last_non_empty(148, |_| async { None }, &ct).await.unwrap();
    assert_eq!(last, 148);
}

// ---------------------------------------------------------------- fixtures

#[test]
fn fixtures_yield_typed_torrents() {
    let host = conf().TorrentBy.host.trim_end_matches('/').to_string() + "/";
    let mut cats: Vec<_> = MAP.keys().copied().collect();
    cats.sort();
    for cat in cats {
        let torrents = parser::parse_torrents_from_html(&fixture(&format!("torrentby/browse_{cat}.html")), cat);
        assert!(torrents.len() >= 30, "expected >=30 torrents for cat {cat}, got {}", torrents.len());
        for t in &torrents {
            assert_eq!(t.trackerName, "torrentby");
            assert_eq!(types(t), MAP[cat].types);
            assert!(!t.name.trim().is_empty());
            assert!(!t.title.trim().is_empty());
            assert!(t.url.starts_with(&host));
            assert!(t.magnet.to_lowercase().starts_with("magnet:?xt="));
            assert!(!t.sizeName.trim().is_empty());
            assert!(!time::is_min(&t.createTime));
        }
    }
}

#[test]
fn unknown_category_returns_empty() {
    assert!(parser::parse_torrents_from_html(&fixture("torrentby/browse_films.html"), "music").is_empty());
}

#[test]
fn empty_listing_page_is_success_without_rows() {
    let html = fixture("torrentby/browse_series_empty.html");
    assert!(parser::is_listing_page(&html));
    assert!(!parser::has_listing_rows(&html));
    assert!(parser::parse_torrents_from_html(&html, "series").is_empty());
}

// ---------------------------------------------------------------- titles

fn browse_row(title: &str) -> String {
    format!(
        "<table><tr class=\"ttable_col1\">\
         <td style=\"white-space:nowrap;text-align:center;\">2024-07-16</td>\
         <td style=\"white-space:nowrap;\">\
         <a class=\"magnet\" href=\"magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01\">m</a> \
         <a name=\"search_select\" style=\"float:left;\" href=\"/123/slug\">{title}</a>\
         </td><td style=\"white-space:nowrap;\">1.5 GB</td>\
         <td><font color=\"green\">&uarr; 10</font> <font color=\"red\">&darr; 5</font></td></tr></table>"
    )
}

fn single(title: &str, cat: &str) -> crab_core::models::TorrentDetails {
    let mut v = parser::parse_torrents_from_html(&browse_row(title), cat);
    assert_eq!(v.len(), 1, "title {title}");
    v.remove(0)
}

#[test]
fn films_foreign_parses_name_orig_year() {
    for (title, name, orig, year) in [
        ("Запретный плод / Forbidden Fruits (2025) WEB-DLRip 720p от New-Team", "Запретный плод", "Forbidden Fruits", 2025),
        (
            "Только течёт река / He bian de cuo wu / Only the River Flows (2023) BDRip 720p",
            "Только течет река",
            "Only the River Flows",
            2023,
        ),
    ] {
        let t = single(title, "films");
        assert_eq!(types(&t), ["movie"]);
        assert_eq!(t.name, name);
        assert_eq!(t.originalname, orig);
        assert_eq!(t.relased, year);
    }
}

#[test]
fn films_ru_parses_name_and_year() {
    for (title, name, year) in [
        ("Не одна дома 3. Выпускной (2026) WEB-DL 1080p", "Не одна дома 3. Выпускной", 2026),
        ("Фронт в тылу врага / Серии: 1-2 из 2 [1981, драма, военный, WEBRip-AVC]", "Фронт в тылу врага", 1981),
    ] {
        let t = single(title, "movies");
        assert_eq!(types(&t), ["movie"]);
        assert_eq!(t.name, name);
        assert_eq!(t.relased, year);
    }
}

#[test]
fn serial_foreign_parses_name_orig_year() {
    for (title, name, orig, year) in [
        ("Дом Дракона / House of the Dragon (2026) WEB-DLRip [H.264/1080p]", "Дом Дракона", "House of the Dragon", 2026),
        ("Тьма / The Dark [S01] (2026) WEB-DLRip-AVC | L | RuDub", "Тьма", "The Dark", 2026),
    ] {
        let t = single(title, "serials");
        assert_eq!(types(&t), ["serial"]);
        assert_eq!(t.name, name);
        assert_eq!(t.originalname, orig);
        assert_eq!(t.relased, year);
    }
}

#[test]
fn serial_ru_parses_name_and_year() {
    for (title, name, year) in [
        ("Доктор, я боюсь / Сезон: 1 / Серии: 1-36 из 40 [2025-2026, мелодрама, WEBRip-AVC]", "Доктор, я боюсь", 2025),
        ("Холод [01х01 из 10] (2026) WEB-DL 1080p", "Холод", 2026),
    ] {
        let t = single(title, "series");
        assert_eq!(types(&t), ["serial"]);
        assert_eq!(t.name, name);
        assert_eq!(t.relased, year);
    }
}

#[test]
fn sport_parses_as_sport_with_name_and_year() {
    for (title, name, year) in [
        (
            "Футбол. Чемпионат Мира 2026. 1/2 финала. Англия – Аргентина + Превью [15.07] (2025) WEBRip 1080p",
            "Футбол. Чемпионат Мира 2026. 1/2 финала. Англия – Аргентина + Превью",
            2025,
        ),
        (
            "Футбол. Чемпионат Мира 2026. Лучшие голы чемпионата мира-2026 [14.07] (2026) WEBRip 1080р",
            "Футбол. Чемпионат Мира 2026. Лучшие голы чемпионата мира-2026",
            2026,
        ),
    ] {
        let t = single(title, "sport");
        assert_eq!(types(&t), ["sport"]);
        assert_eq!(t.name, name);
        assert_eq!(t.relased, year);
    }
}
