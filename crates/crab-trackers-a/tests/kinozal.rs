mod support;

use chrono::{TimeZone, Utc};
use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::{conf, time};
use crab_trackers_a::kinozal::categories::{self, KinozalTitleKind, MAP};
use crab_trackers_a::kinozal::{self, parser};
use support::{fixture, types};

fn host() -> String {
    conf().Kinozal.host.trim_end_matches('/').to_string()
}

// ---------------------------------------------------------------- categories

#[test]
fn map_has_expected_count_and_no_duplicate_ids() {
    assert_eq!(MAP.len(), 25);
    assert_eq!(categories::ids().count(), MAP.len());
}

#[test]
fn map_types_match_expected() {
    let cases: [(&str, &[&str]); 9] = [
        ("18", &["docuserial", "documovie"]),
        ("37", &["sport"]),
        ("45", &["serial"]),
        ("46", &["serial"]),
        ("20", &["anime"]),
        ("49", &["tvshow"]),
        ("8", &["movie"]),
        ("21", &["multfilm", "multserial"]),
        ("22", &["multfilm", "multserial"]),
    ];
    for (cat, ty) in cases {
        assert_eq!(MAP[cat].types, ty, "cat {cat}");
    }
}

#[test]
fn map_title_kind_match_expected() {
    for (cat, kind) in [
        ("8", KinozalTitleKind::Movie),
        ("18", KinozalTitleKind::Movie),
        ("37", KinozalTitleKind::Movie),
        ("45", KinozalTitleKind::SerialRu),
        ("22", KinozalTitleKind::SerialRu),
        ("46", KinozalTitleKind::SerialEn),
        ("21", KinozalTitleKind::SerialEn),
        ("20", KinozalTitleKind::SerialEn),
        ("49", KinozalTitleKind::TvShow),
        ("50", KinozalTitleKind::TvShow),
    ] {
        assert_eq!(MAP[cat].title_kind, kind, "cat {cat}");
    }
}

#[test]
fn map_does_not_include_music_software_theatre_or_concert() {
    for id in ["1", "2", "3", "4", "5", "23", "32", "38", "40", "41", "42", "48", "1001", "1002"] {
        assert!(!MAP.contains_key(id), "unexpected cat {id}");
    }
    assert!(!MAP["37"].types.contains(&"movie"));
}

// ---------------------------------------------------------------- fixtures

#[test]
fn fixtures_yield_typed_torrents() {
    let mut cats: Vec<_> = MAP.keys().copied().collect();
    cats.sort_by_key(|c| c.parse::<i32>().unwrap());
    for cat in cats {
        let torrents = parser::parse_torrents_from_page(&fixture(&format!("kinozal/browse_c{cat}.html")), cat);
        assert!(torrents.len() >= 40, "expected >=40 torrents for cat {cat}, got {}", torrents.len());
        for t in &torrents {
            assert_eq!(t.trackerName, "kinozal");
            assert_eq!(types(t), MAP[cat].types);
            assert!(!t.name.trim().is_empty());
            assert!(!t.title.trim().is_empty());
            assert!(t.url.starts_with(&(host() + "/")));
            assert!(t.url.contains("/details.php?id="));
            assert!(!t.url.to_lowercase().contains("userdetails"));
            assert!(!t.sizeName.trim().is_empty());
            assert!(!time::is_min(&t.createTime));
            assert!(t.sid >= 0 && t.pir >= 0);
        }
    }
}

#[test]
fn c50_parses_terabyte_sizes() {
    let torrents = parser::parse_torrents_from_page(&fixture("kinozal/browse_c50.html"), "50");
    let tb: Vec<_> = torrents.iter().filter(|t| t.sizeName.contains("ТБ")).collect();
    assert_eq!(tb.len(), 1);
    assert_eq!(tb[0].sizeName, "2.278 ТБ");
    assert!(tb[0].title.contains("МастерШеф"));
}

#[test]
fn unknown_category_and_empty_html_return_empty() {
    assert!(parser::parse_torrents_from_page(&fixture("kinozal/browse_c8.html"), "9999").is_empty());
    assert!(parser::parse_torrents_from_page("", "8").is_empty());
    assert!(parser::parse_torrents_from_page("<html></html>", "8").is_empty());
}

#[test]
fn parse_info_hash() {
    let html = "<html><head></head><body><ul><li>Инфо хеш: 7C4FCE77B05BC2711C8445C0B1E47011CF4214CE</li>\
                <li>Размер части торрента: 1 МБ</li></ul></body></html>";
    assert_eq!(parser::parse_info_hash(Some(html)).as_deref(), Some("7C4FCE77B05BC2711C8445C0B1E47011CF4214CE"));

    let big = "x".repeat(9000) + "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(parser::parse_info_hash(Some(&big)), None);

    assert_eq!(parser::parse_info_hash(None), None);
    assert_eq!(parser::parse_info_hash(Some("")), None);
    assert_eq!(parser::parse_info_hash(Some("Торрент файл не найден.")), None);
}

#[test]
fn is_transient_browse_failure_null_and_nginx503() {
    assert!(parser::is_transient_browse_failure(None));
    assert!(parser::is_transient_browse_failure(Some("")));
    assert!(parser::is_transient_browse_failure(Some("<html><head><title>503 Service Temporarily Unavailable</title></head></html>")));
    assert!(parser::is_transient_browse_failure(Some("<title>Just a moment...</title>")));
    assert!(!parser::is_transient_browse_failure(Some(&fixture("kinozal/browse_c22.html"))));
}

#[test]
fn is_login_wall_and_logged_in_from_fixture() {
    let listing = fixture("kinozal/browse_c22.html");
    assert!(parser::is_logged_in(&listing));
    assert!(!parser::is_login_wall(Some(&listing)));
    assert!(parser::is_login_wall(Some("<form action=\"/takelogin.php\"><input name=\"username\">")));
    assert!(!parser::is_login_wall(Some("<title>503 Service Temporarily Unavailable</title>")));
}

#[test]
fn chromium_double_quoted_rows_yield_torrents() {
    let torrents = parser::parse_torrents_from_page(&fixture("kinozal/browse_chromium_quoted.html"), "8");
    assert_eq!(torrents.len(), 2);
    assert_eq!(torrents[0].url, format!("{}/details.php?id=2153071", host()));
    assert_eq!(torrents[1].url, format!("{}/details.php?id=2153065", host()));
    for t in &torrents {
        assert_eq!(t.trackerName, "kinozal");
        assert_eq!(types(t), ["movie"]);
        assert!(!t.sizeName.trim().is_empty());
        assert!(!time::is_min(&t.createTime));
    }
    assert_eq!(torrents[0].name, "Молодожены");
    assert_eq!(torrents[0].originalname, "Just Married");
    assert_eq!(torrents[0].relased, 2003);
}

#[test]
fn has_torrent_listing_links_ignores_userdetails() {
    assert!(!parser::has_torrent_listing_links(Some("<a href=\"/userdetails.php?id=191355\">profile</a><a href=\"#\">Выход</a>")));
    assert!(parser::has_torrent_listing_links(Some("<a href=\"/details.php?id=2153071\" class=\"r0\">title</a>")));
    assert_eq!(parser::try_get_details_id("https://kinozal.guru/userdetails.php?id=191355"), None);
    assert_eq!(parser::try_get_details_id("https://kinozal.guru/userdetails.php?id=2153071"), None);
    assert_eq!(parser::try_get_details_id("https://kinozal.guru/details.php?id=2153071"), Some(2153071));
    assert_eq!(parser::count_torrent_listing_links(Some(&fixture("kinozal/browse_chromium_quoted.html"))), 2);
    assert!(!parser::has_torrent_listing_links(None));
}

#[test]
fn url_id_extractor_registered() {
    crab_trackers_a::init();
    assert_eq!(crab_core::fdb::torrent_id_from_url("kinozal", "https://kinozal.guru/details.php?id=2153071"), 2153071);
    assert_eq!(crab_core::fdb::torrent_id_from_url("kinozal", "https://kinozal.guru/userdetails.php?id=2153071"), 0);
    assert_eq!(kinozal::url_id("https://kinozal.guru/details.php?id=5"), 5);
}

#[test]
fn userdetails_first_in_row_stores_details_url() {
    let html = "<title>Раздачи :: Кинозал.GURU</title>\
        <table class=\"t_peer\"><tr class=\"bg\">\
        <td class=\"sl\"><a href=\"/userdetails.php?id=191355\" class=\"u6\">uploader</a></td>\
        <td class=\"nam\"><a href=\"/details.php?id=2153071\" class=\"r0\">\
        Молодожены / Just Married / 2003 / ДБ / BDRip (720p)</a></td>\
        <td class=\"s\">0</td><td class=\"s\">7.11 ГБ</td>\
        <td class=\"sl_s\">1</td><td class=\"sl_p\">2</td>\
        <td class=\"s\">16.07.2024 в 12:00</td></tr></table>";
    let torrents = parser::parse_torrents_from_page(html, "8");
    assert_eq!(torrents.len(), 1);
    assert_eq!(torrents[0].url, format!("{}/details.php?id=2153071", host()));
    assert_eq!(torrents[0].name, "Молодожены");
}

#[test]
fn userdetails_only_row_returns_empty() {
    let html = "<table class=\"t_peer\"><tr class=\"bg\">\
        <td class=\"nam\"><a href=\"/userdetails.php?id=191355\" class=\"r0\">uploader</a></td>\
        <td class=\"s\">0</td><td class=\"s\">7.11 ГБ</td>\
        <td class=\"sl_s\">1</td><td class=\"sl_p\">2</td>\
        <td class=\"s\">16.07.2024 в 12:00</td></tr></table>";
    assert!(parser::parse_torrents_from_page(html, "8").is_empty());
}

#[test]
fn is_valid_browse_page_requires_t_peer_and_kinozal_title() {
    assert!(parser::is_valid_browse_page(Some(&fixture("kinozal/browse_c8.html"))));
    assert!(parser::is_valid_browse_page(Some(&fixture("kinozal/browse_chromium_quoted.html"))));
    assert!(!parser::is_valid_browse_page(Some("<title>RuTracker.org :: forum</title><a href=\"/userdetails.php?id=1\">x</a>")));
    assert!(!parser::is_valid_browse_page(Some("<title>Раздачи :: Кинозал.GURU</title><a href=\"#\">Выход</a>")));
}

const SHELL: &str = "<title>Раздачи :: Кинозал.GURU</title><a href=\"/userdetails.php?id=191355\">profile</a><a href=\"#\">Выход</a>";

#[test]
fn format_browse_diag_length_t_peer_title() {
    let diag = parser::format_browse_diag(Some(SHELL));
    assert!(diag.contains("len="));
    assert!(diag.contains("t_peer=False"));
    assert!(diag.contains("Кинозал.GURU"));
    assert!(!diag.contains("userdetails"));
    assert_eq!(parser::format_browse_diag(None), "len=0");
    assert!(parser::format_browse_diag(Some(&fixture("kinozal/browse_c22.html"))).contains("t_peer=True"));
}

#[test]
fn update_tasks_parse_delay_caps_at_two_seconds() {
    assert_eq!(parser::update_tasks_parse_delay_ms(0), 0);
    assert_eq!(parser::update_tasks_parse_delay_ms(1500), 1500);
    assert_eq!(parser::update_tasks_parse_delay_ms(10000), 2000);
    assert_eq!(parser::update_tasks_parse_delay_ms(-5), 0);
}

#[test]
fn is_stale_listing_html_logged_in_shell_without_t_peer() {
    assert!(parser::is_logged_in(SHELL));
    assert!(parser::is_stale_listing_html(Some(SHELL)));
    assert!(!parser::is_valid_browse_page(Some(SHELL)));
    assert!(!parser::is_login_wall(Some(SHELL)));
    assert!(!parser::is_stale_listing_html(Some(&fixture("kinozal/browse_c22.html"))));
    assert!(!parser::is_stale_listing_html(Some("<title>Just a moment...</title>")));
    assert!(!parser::is_stale_listing_html(Some("<form action=\"/takelogin.php\"><input name=\"username\">")));
}

#[test]
fn is_empty_search_result_not_stale_marks_page_done() {
    let empty = fixture("kinozal/browse_empty_search.html");
    assert!(parser::is_logged_in(&empty));
    assert!(parser::is_empty_search_result(Some(&empty)));
    assert!(!parser::is_stale_listing_html(Some(&empty)));
    assert!(!parser::is_valid_browse_page(Some(&empty)));
    assert!(!parser::is_login_wall(Some(&empty)));
    assert!(parser::should_mark_page_done(0, 0, parser::count_torrent_listing_links(Some(&empty))));
    assert!(!parser::is_empty_search_result(Some(&fixture("kinozal/browse_c22.html"))));
    assert!(!parser::is_empty_search_result(Some("<title>Раздачи :: Кинозал.GURU</title><a href=\"#\">Выход</a>")));
}

#[test]
fn browse_filters_mismatch_selected_cat_and_year() {
    let listing = fixture("kinozal/browse_c22.html");
    assert_eq!(parser::try_get_selected_browse_filter(&listing, "c").as_deref(), Some("22"));
    assert!(!parser::browse_filters_mismatch(Some(&listing), "22", None));
    assert!(parser::browse_filters_mismatch(Some(&listing), "13", None));
    assert!(!parser::browse_filters_mismatch(Some(&listing), "22", Some("&d=2020&t=1")));

    let empty = fixture("kinozal/browse_empty_search.html");
    assert!(!parser::browse_filters_mismatch(Some(&empty), "13", Some("&d=2020&t=1")));
    assert!(parser::browse_filters_mismatch(Some(&empty), "15", Some("&d=2020&t=1")));
    assert!(parser::browse_filters_mismatch(Some(&empty), "13", Some("&d=2021&t=1")));
    assert!(!parser::browse_filters_mismatch(Some("<title>Кинозал.GURU</title><a href=\"#\">Выход</a>"), "13", Some("&d=2020&t=1")));

    let all_years = "<select name=\"d\"><option selected=selected value=0>все года</option></select>\
                     <select name=\"c\"><option selected=selected value=13>x</option></select>";
    assert!(!parser::browse_filters_mismatch(Some(all_years), "13", Some("&d=2020&t=1")));
}

#[test]
fn year_task_page_count_pager_digit_is_exclusive_upper_bound() {
    assert_eq!(parser::year_task_page_count_digit(0), 1);
    assert_eq!(parser::year_task_page_count_digit(-1), 1);
    assert_eq!(parser::year_task_page_count_digit(15), 15);
    assert_eq!(parser::year_task_page_count(""), 1);
    assert_eq!(
        parser::year_task_page_count("<li><a href=\"?c=45&amp;page=14\">15</a></li><li><a rel=\"next\" href=\"?page=15\">Вперед</a>"),
        15
    );
    assert_eq!(parser::year_task_page_count(&fixture("kinozal/browse_c22.html")), 100);
}

#[test]
fn prune_pages_beyond_year_count_drops_inclusive_tail() {
    let mut pages: Vec<TaskParse> = [0, 8, 9, 10].into_iter().map(TaskParse::new).collect();
    assert_eq!(parser::prune_pages_beyond_year_count(&mut pages, 9), 2);
    assert_eq!(pages.iter().map(|p| p.page).collect::<Vec<_>>(), vec![0, 8]);
    assert_eq!(parser::prune_pages_beyond_year_count(&mut pages, 9), 0);
    assert_eq!(parser::prune_pages_beyond_year_count(&mut Vec::new(), 9), 0);
}

#[test]
fn should_mark_page_done_empty_or_fully_resolved() {
    assert!(parser::should_mark_page_done(0, 0, 0));
    assert!(!parser::should_mark_page_done(0, 0, 50));
    assert!(parser::should_mark_page_done(10, 10, 10));
    assert!(!parser::should_mark_page_done(10, 0, 10));
    assert!(!parser::should_mark_page_done(10, 9, 10));
}

#[test]
fn should_skip_hash_fetch_rules() {
    let mut cached = TorrentDetails::new("kinozal", &["movie"], "u", "T");
    cached.sizeName = "1 ГБ".into();
    cached.createTime = Utc.with_ymd_and_hms(2024, 7, 16, 12, 0, 0).unwrap();
    let mut parsed = cached.clone();
    assert!(!parser::should_skip_hash_fetch(&cached, &parsed)); // no magnet cached
    cached.magnet = "magnet:?xt=urn:btih:AA".into();
    assert!(parser::should_skip_hash_fetch(&cached, &parsed));
    parsed.createTime = Utc.with_ymd_and_hms(2024, 7, 16, 12, 1, 0).unwrap();
    assert!(!parser::should_skip_hash_fetch(&cached, &parsed));
}

#[test]
fn cookie_from_set_cookie_headers() {
    let lines = vec!["uid=123; path=/; domain=.kinozal.guru".to_string(), "pass=abc; path=/".to_string()];
    assert_eq!(kinozal::cookie_from_set_cookies(&lines).as_deref(), Some("uid=123; pass=abc;"));
    assert_eq!(kinozal::cookie_from_set_cookies(&["uid=1".to_string()]), None);
    assert_eq!(kinozal::extract_cookie_value(&["x=1; pass=zz".to_string()], "pass").as_deref(), Some("zz"));
}

// ---------------------------------------------------------------- listing time

#[test]
fn parse_listing_update_time_absolute() {
    for (raw, y, m, d, h, min) in [("16.07.2024 в 12:34", 2024, 7, 16, 12, 34), ("01.01.2020 в 00:00", 2020, 1, 1, 0, 0)] {
        assert_eq!(parser::parse_listing_update_time(Some(raw)), Some(Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()));
    }
}

#[test]
fn parse_listing_update_time_relative_and_invalid() {
    assert!(parser::parse_listing_update_time(Some("сегодня в 09:15")).is_some());
    assert!(parser::parse_listing_update_time(Some("вчера в 23:59")).is_some());
    for raw in [None, Some(""), Some("   "), Some("not-a-date")] {
        assert_eq!(parser::parse_listing_update_time(raw), None);
    }
}

// ---------------------------------------------------------------- sport titles

fn row(title: &str, q: &str) -> String {
    let a = |cls: &str| format!("class={q}{cls}{q}");
    format!(
        "<table><tr class={q}bg{q}><td class=\"nam\"><a href=\"/details.php?id=1\" class=\"r0\">{title}</a></td>\
         <td {s}>0</td><td {s}>1.5 ГБ</td><td {sls}>10</td><td {slp}>5</td><td {s}>16.07.2024 в 12:00</td></tr></table>",
        s = a("s"),
        sls = a("sl_s"),
        slp = a("sl_p"),
    )
}

#[test]
fn sport_titles_parse_as_sport_with_name_and_year() {
    for (title, name, year) in [
        ("Велоспорт. Тур де Франс 2026 (12-й этап) / 2026 / РУ / WEB-DL (1080p)", "Велоспорт. Тур де Франс 2026 (12-й этап)", 2026),
        (
            "Футбол. Лига чемпионов 2026/27 (1-й раунд, 2-й матч) Сутьеска (Черногория) - Кайрат (Казахстан) / 2026 / РУ / WEB-DL (1080p)",
            "Футбол. Лига чемпионов 2026/27 (1-й раунд, 2-й матч) Сутьеска (Черногория) - Кайрат (Казахстан)",
            2026,
        ),
    ] {
        for q in ["'", "\""] {
            let html = if q == "'" { row(title, "'").replace("class='bg'", "class=bg") } else { row(title, q) };
            let torrents = parser::parse_torrents_from_page(&html, "37");
            assert_eq!(torrents.len(), 1, "quote {q} title {title}");
            assert_eq!(types(&torrents[0]), ["sport"]);
            assert_eq!(torrents[0].name, name);
            assert_eq!(torrents[0].relased, year);
        }
    }
}

#[test]
fn sport_fixture_typed_as_sport_and_most_have_year() {
    let torrents = parser::parse_torrents_from_page(&fixture("kinozal/browse_c37.html"), "37");
    assert!(torrents.len() >= 40);
    assert!(torrents.iter().all(|t| types(t) == ["sport"]));
    let with_year = torrents.iter().filter(|t| t.relased > 0).count();
    assert!(with_year >= 40, "expected most sport torrents to have year, got {with_year}/{}", torrents.len());
}
