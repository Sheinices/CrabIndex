mod support;

use async_trait::async_trait;
use crab_core::models::TorrentDetails;
use crab_core::time;
use crab_core::trackers::Cancelled;
use crab_trackers_b::bitru::backfill::{self, BackfillSource, BitruBackfillPage, BitruBackfillProgress};
use crab_trackers_b::bitru::models::BitruApiResult;
use crab_trackers_b::bitru::{categories, pagination, parser};
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use tokio_util::sync::CancellationToken;

const HOST: &str = "https://bitru.org";

// ---------------------------------------------------------------------------
// pagination
// ---------------------------------------------------------------------------

#[test]
fn build_request_params_without_cursor_has_no_date_filter() {
    let p = pagination::build_request_params(100, None);
    assert_eq!(p["limit"], json!(100));
    assert_eq!(p["category"], json!(categories::REQUEST_CATEGORIES));
    assert!(!p.contains_key("after_date"));
    assert!(!p.contains_key("before_date"));
}

#[test]
fn build_request_params_older_page_uses_after_date_not_before_date() {
    let p = pagination::build_request_params(50, Some(1786263023));
    assert_eq!(p[pagination::AFTER_DATE_PARAM], json!("1786263023"));
    assert!(!p.contains_key("before_date"));
    assert_eq!(serde_json::to_string(&p).unwrap(), r#"{"limit":50,"category":["movie","serial","video"],"after_date":"1786263023"}"#);
}

#[test]
fn try_get_next_older_page_cursor_uses_before_date_as_next_after_date() {
    let r = BitruApiResult { before_date: json!(1786263023i64), after_date: json!(1786278241i64), items: None };
    assert_eq!(pagination::try_get_next_older_page_cursor(Some(&r), None), Some(1786263023));
}

#[test]
fn try_get_next_older_page_cursor_stops_when_unchanged() {
    let r = BitruApiResult { before_date: json!("100"), ..Default::default() };
    assert_eq!(pagination::try_get_next_older_page_cursor(Some(&r), Some(100)), None);
}

#[test]
fn try_get_next_older_page_cursor_missing_or_zero_fails() {
    assert_eq!(pagination::try_get_next_older_page_cursor(None, None), None);
    assert_eq!(pagination::try_get_next_older_page_cursor(Some(&BitruApiResult::default()), None), None);
    let r = BitruApiResult { before_date: json!(0), ..Default::default() };
    assert_eq!(pagination::try_get_next_older_page_cursor(Some(&r), None), None);
}

#[test]
fn is_duplicate_page_true_when_current_fully_contained() {
    let prev: HashSet<i64> = [1, 2, 3, 4, 5].into();
    let cur: HashSet<i64> = [2, 4, 5].into();
    assert!(pagination::is_duplicate_page(Some(&prev), Some(&cur)));
}

#[test]
fn is_duplicate_page_false_when_new_ids_present() {
    let prev: HashSet<i64> = [1, 2, 3].into();
    let cur: HashSet<i64> = [3, 4, 5].into();
    assert!(!pagination::is_duplicate_page(Some(&prev), Some(&cur)));
}

#[test]
fn try_extract_torrent_id_from_details_url() {
    assert_eq!(pagination::try_extract_torrent_id("https://bitru.org/details.php?id=729321"), Some(729321));
}

#[test]
fn clamp_pages_respects_hard_limit() {
    for (input, expected) in [(0, 1), (5, 5), (100, 50)] {
        assert_eq!(pagination::clamp_pages(input), expected);
    }
}

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

#[test]
fn parse_torrents_from_json_fixture_yields_typed_torrents() {
    for file in ["api_movie_serial_page1.json", "api_video_page1.json"] {
        let torrents = parser::parse_torrents_from_json(&support::read(&format!("Bitru/{file}")), HOST);
        assert!(torrents.len() >= 5, "expected >=5 torrents for {file}, got {}", torrents.len());
        for t in &torrents {
            assert_eq!(t.trackerName, "bitru");
            assert!(!t.types.is_empty());
            assert!(!t.name.trim().is_empty());
            assert!(!t.title.trim().is_empty());
            assert!(t.url.starts_with(&format!("{HOST}/")));
            assert!(!t.sizeName.trim().is_empty());
            assert!(!time::is_min(&t.createTime));
            assert!(t._sn.contains("api.php?download="), "{}", t._sn);
        }
    }
}

#[test]
fn movie_serial_fixture_contains_movie_and_serial() {
    let torrents = parser::parse_torrents_from_json(&support::read("Bitru/api_movie_serial_page1.json"), HOST);
    assert!(torrents.iter().any(|t| t.types == ["movie"]));
    assert!(torrents.iter().any(|t| t.types == ["serial"]));
}

#[test]
fn video_fixture_maps_known_subsections_and_drops_others() {
    let torrents = parser::parse_torrents_from_json(&support::read("Bitru/api_video_page1.json"), HOST);
    assert!(!torrents.is_empty());
    for t in &torrents {
        assert!(["documovie", "sport", "tvshow"].contains(&t.types[0].as_str()));
    }
}

#[test]
fn page2_fixture_parses_and_is_not_full_subset_of_page1() {
    let page1 = parser::parse_torrents_from_json(&support::read("Bitru/api_movie_serial_page1.json"), HOST);
    let page2 = parser::parse_torrents_from_json(&support::read("Bitru/api_movie_serial_page2.json"), HOST);
    assert!(!page2.is_empty());
    let ids1 = pagination::collect_torrent_ids(page1.iter().map(|t| t.url.as_str()));
    let ids2 = pagination::collect_torrent_ids(page2.iter().map(|t| t.url.as_str()));
    assert!(!pagination::is_duplicate_page(Some(&ids1), Some(&ids2)), "page2 must not be a full subset of page1");
    assert!(ids1.intersection(&ids2).next().is_none());
}

// ---------------------------------------------------------------------------
// parser units
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn item_json(category: &str, name: &str, orig: Option<&str>, year: Value, quality: Option<&str>, other: Option<&str>, subsections: Option<&[&str]>, file: Option<&str>) -> String {
    let id = 123;
    json!({
        "result": {
            "after_date": 1,
            "before_date": 1,
            "items": [{
                "item": {
                    "torrent": {
                        "id": id, "added": 1700000000i64, "size": 2147483648i64, "leechers": 2, "seeders": 10,
                        "file": file.map(|f| f.to_string()).unwrap_or_else(|| format!("{HOST}/api.php?download={id}"))
                    },
                    "info": { "name": name, "year": year },
                    "template": {
                        "category": category,
                        "subsection": subsections,
                        "orig_name": orig,
                        "other": other,
                        "video": { "quality": quality }
                    }
                }
            }]
        }
    })
    .to_string()
}

#[test]
fn parse_torrents_from_json_movie_maps_fields() {
    let list = parser::parse_torrents_from_json(&item_json("movie", "Робин Гуд", Some("Robin Hood"), json!(1991), Some("BDRip"), Some("D"), None, None), HOST);
    assert_eq!(list.len(), 1);
    let t = &list[0];
    assert_eq!(t.trackerName, "bitru");
    assert_eq!(t.types, ["movie"]);
    assert_eq!(t.url, format!("{HOST}/details.php?id=123"));
    assert!(t.title.contains("Робин Гуд") && t.title.contains("Robin Hood") && t.title.contains("(1991)"));
    assert_eq!(t.name, "Робин Гуд");
    assert_eq!(t.originalname, "Robin Hood");
    assert_eq!(t.relased, 1991);
    assert_eq!(t.sid, 10);
    assert_eq!(t.pir, 2);
    assert!(!t.sizeName.trim().is_empty());
    assert!(t._sn.contains("api.php?download=123"));
    assert!(!time::is_min(&t.createTime));
}

#[test]
fn parse_torrents_from_json_serial_and_year_range() {
    let list = parser::parse_torrents_from_json(&item_json("serial", "Сериал", None, json!("2011-2015"), Some("WEBRip"), None, None, None), HOST);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].types, ["serial"]);
    assert_eq!(list[0].relased, 2011);
    assert!(list[0].title.contains("(2011-2015)"));
}

#[test]
fn parse_torrents_from_json_video_documovie() {
    let list = parser::parse_torrents_from_json(
        &item_json("video", "Душа океана", Some("Soul of the Ocean"), json!("2022"), None, None, Some(&["Документальный"]), None),
        HOST,
    );
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].types, ["documovie"]);
}

#[test]
fn parse_torrents_from_json_unknown_category_empty() {
    assert!(parser::parse_torrents_from_json(&item_json("music", "Album", None, Value::Null, None, None, None, None), HOST).is_empty());
}

#[test]
fn parse_torrents_from_json_video_trailer_dropped() {
    assert!(parser::parse_torrents_from_json(&item_json("video", "Trailer", None, Value::Null, None, None, Some(&["Трейлер"]), None), HOST).is_empty());
}

#[test]
fn parse_torrents_from_json_string_error_returns_empty() {
    assert!(parser::parse_torrents_from_json(r#"{"error":"max limit 100"}"#, HOST).is_empty());
}

#[test]
fn parse_torrents_from_json_empty_file_falls_back_to_api_download() {
    let list = parser::parse_torrents_from_json(&item_json("movie", "Film", None, json!(2020), None, None, None, Some("")), HOST);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]._sn, format!("{HOST}/api.php?download=123"));
}

#[test]
fn clean_title_for_search_strips_noise() {
    for (input, expected) in [("Название (2020) WEB-DL 1080p", "Название"), ("Show S01E01 720p", "Show"), ("Сериал 1 сезон", "Сериал")] {
        assert_eq!(parser::clean_title_for_search(input), expected, "{input}");
    }
}

#[test]
fn unix_from_date_is_start_of_day_not_epoch_zero() {
    let today = chrono::Utc::now().date_naive();
    let unix = parser::unix_from_date(today);
    assert!(unix > 0);
    assert_eq!(unix, today.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp());
}

// ---------------------------------------------------------------------------
// categories
// ---------------------------------------------------------------------------

#[test]
fn request_categories_are_movie_serial_video() {
    assert_eq!(categories::REQUEST_CATEGORIES, ["movie", "serial", "video"]);
}

#[test]
fn try_get_types_movie_and_serial() {
    for (cat, expected) in [("movie", "movie"), ("serial", "serial"), ("Movie", "movie")] {
        assert_eq!(categories::try_get_types(Some(cat), None), Some(&[expected][..]));
    }
}

#[test]
fn try_get_types_video_subsections() {
    let cases = [
        ("Документальный", "documovie"),
        ("Научный", "documovie"),
        ("Исторический", "documovie"),
        ("Биография", "documovie"),
        ("Спорт", "sport"),
        ("Шоу", "tvshow"),
        ("Клипы", "tvshow"),
        ("Концерт", "tvshow"),
    ];
    for (sub, expected) in cases {
        let subs = vec![sub.to_string()];
        assert_eq!(categories::try_get_types(Some("video"), Some(&subs)), Some(&[expected][..]), "{sub}");
    }
}

#[test]
fn try_get_types_video_dropped_subsections() {
    for sub in ["Трейлер", "Эротика", "Уроки", "Детское", "Неизвестный"] {
        let subs = vec![sub.to_string()];
        assert_eq!(categories::try_get_types(Some("video"), Some(&subs)), None, "{sub}");
    }
}

#[test]
fn try_get_types_non_video_returns_none() {
    for cat in ["music", "game", "soft", "xxx"] {
        assert_eq!(categories::try_get_types(Some(cat), None), None);
        assert!(categories::NON_VIDEO_IDS.contains(&cat));
    }
}

#[test]
fn video_first_matching_subsection_wins() {
    let subs = vec!["Трейлер".to_string(), "Спорт".to_string()];
    assert_eq!(categories::try_get_types(Some("video"), Some(&subs)), Some(&["sport"][..]));
}

// ---------------------------------------------------------------------------
// backfill commit loop
// ---------------------------------------------------------------------------

fn dummy_torrents(count: usize) -> Vec<TorrentDetails> {
    (0..count).map(|i| TorrentDetails { title: format!("t{i}"), ..Default::default() }).collect()
}

struct Mock {
    pages: VecDeque<BitruBackfillPage>,
    repeat: Option<BitruBackfillPage>,
    committed: Option<i64>,
    finished: bool,
    saved: bool,
    save_calls: i32,
    cancel_on_save: Option<(i32, CancellationToken)>,
}

impl Mock {
    fn new(pages: Vec<BitruBackfillPage>) -> Self {
        Mock { pages: pages.into(), repeat: None, committed: None, finished: false, saved: false, save_calls: 0, cancel_on_save: None }
    }
}

#[async_trait]
impl BackfillSource for Mock {
    async fn fetch_page(&mut self, _cursor: Option<i64>, _ct: &CancellationToken) -> anyhow::Result<BitruBackfillPage> {
        if let Some(p) = &self.repeat {
            return Ok(p.clone());
        }
        Ok(self.pages.pop_front().unwrap_or_else(BitruBackfillPage::halt))
    }

    async fn save_page(&mut self, _torrents: &[TorrentDetails], ct: &CancellationToken) -> anyhow::Result<()> {
        self.saved = true;
        self.save_calls += 1;
        if let Some((n, cts)) = &self.cancel_on_save {
            if self.save_calls == *n {
                cts.cancel();
                if ct.is_cancelled() {
                    return Err(Cancelled.into());
                }
            }
        }
        Ok(())
    }

    fn commit_cursor(&mut self, unix: i64) {
        self.committed = Some(unix);
    }

    fn commit_finished(&mut self) {
        self.finished = true;
    }
}

#[tokio::test]
async fn run_two_pages_saved_commits_last_cursor() {
    let mut m = Mock::new(vec![BitruBackfillPage::ok(dummy_torrents(2), Some(200), None), BitruBackfillPage::ok(dummy_torrents(3), Some(100), None)]);
    let mut progress = BitruBackfillProgress::default();
    backfill::run(5, Some(300), &mut m, &mut progress, &CancellationToken::new()).await.unwrap();

    assert_eq!(m.committed, Some(100));
    assert_eq!(progress.fetched_pages, 2);
    assert_eq!(progress.committed_pages, 2);
    assert_eq!(progress.saved_count, 5);
    assert_eq!(progress.last_committed_cursor, Some(100));
    assert_eq!(progress.format_log(), "saved 5, fetchedPages=2, committedPages=2, cursor=100");
}

#[tokio::test]
async fn run_cancel_during_second_page_save_keeps_first_cursor() {
    let cts = CancellationToken::new();
    let mut m = Mock::new(vec![BitruBackfillPage::ok(dummy_torrents(2), Some(200), None), BitruBackfillPage::ok(dummy_torrents(3), Some(100), None)]);
    m.cancel_on_save = Some((2, cts.clone()));
    let mut progress = BitruBackfillProgress { last_committed_cursor: Some(300), ..Default::default() };

    let err = backfill::run(5, Some(300), &mut m, &mut progress, &cts).await.unwrap_err();
    assert!(backfill::is_cancelled(&err));
    assert_eq!(m.committed, Some(200));
    assert_eq!(progress.fetched_pages, 2);
    assert_eq!(progress.committed_pages, 1);
    assert_eq!(progress.saved_count, 2);
    assert_eq!(progress.last_committed_cursor, Some(200));
    assert_eq!(progress.format_canceled_log(), "canceled, saved=2, fetchedPages=2, committedPages=1, cursor=200");
}

#[tokio::test]
async fn run_cancel_during_first_page_save_does_not_write_cursor() {
    let cts = CancellationToken::new();
    let mut m = Mock::new(vec![]);
    m.repeat = Some(BitruBackfillPage::ok(dummy_torrents(2), Some(200), None));
    m.cancel_on_save = Some((1, cts.clone()));
    let mut progress = BitruBackfillProgress { last_committed_cursor: Some(300), ..Default::default() };

    let err = backfill::run(5, Some(300), &mut m, &mut progress, &cts).await.unwrap_err();
    assert!(backfill::is_cancelled(&err));
    assert_eq!(m.committed, None);
    assert_eq!(progress.fetched_pages, 1);
    assert_eq!(progress.committed_pages, 0);
    assert_eq!(progress.saved_count, 0);
    assert_eq!(progress.last_committed_cursor, Some(300));
    assert_eq!(progress.format_canceled_log(), "canceled, saved=0, fetchedPages=1, committedPages=0, cursor=300");
}

#[tokio::test]
async fn run_halt_on_first_page_does_not_save_or_commit() {
    let mut m = Mock::new(vec![]);
    let mut progress = BitruBackfillProgress { last_committed_cursor: Some(300), ..Default::default() };
    backfill::run(5, Some(300), &mut m, &mut progress, &CancellationToken::new()).await.unwrap();

    assert!(!m.saved);
    assert_eq!(m.committed, None);
    assert_eq!(progress.fetched_pages, 0);
    assert_eq!(progress.committed_pages, 0);
    assert_eq!(progress.format_log(), "no items, fetchedPages=0, committedPages=0, cursor=300");
    assert!(!m.finished);
    assert!(!progress.finished);
}

#[tokio::test]
async fn run_non_empty_page_without_next_cursor_writes_finished() {
    let mut m = Mock::new(vec![]);
    m.repeat = Some(BitruBackfillPage::ok(dummy_torrents(95), None, None));
    let mut progress = BitruBackfillProgress { last_committed_cursor: Some(1376988004), ..Default::default() };
    backfill::run(5, Some(1376988004), &mut m, &mut progress, &CancellationToken::new()).await.unwrap();

    assert!(m.finished);
    assert!(progress.finished);
    assert_eq!(m.committed, None);
    assert_eq!(progress.fetched_pages, 1);
    assert_eq!(progress.committed_pages, 1);
    assert_eq!(progress.saved_count, 95);
    assert_eq!(progress.last_committed_cursor, Some(1376988004));
    assert_eq!(progress.format_log(), "saved 95, fetchedPages=1, committedPages=1, cursor=1376988004, finished");
}

fn temp_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("crab-bitru-backfill-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

#[test]
fn write_cursor_atomic_replaces_existing_file() {
    let dir = temp_dir();
    let path = dir.join("bitru_backfill_cursor.txt").to_string_lossy().to_string();
    backfill::write_cursor_atomic(&path, 1770045331).unwrap();
    backfill::write_cursor_atomic(&path, 1769339212).unwrap();
    assert_eq!(backfill::read_cursor(&path), Some(1769339212));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "1769339212");
    assert!(!std::path::Path::new(&format!("{path}.tmp")).exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn write_finished_atomic_then_is_finished_skips_cursor() {
    let dir = temp_dir();
    let path = dir.join("bitru_backfill_cursor.txt").to_string_lossy().to_string();
    backfill::write_cursor_atomic(&path, 1376988004).unwrap();
    assert!(!backfill::is_finished(&path));
    assert_eq!(backfill::read_cursor(&path), Some(1376988004));

    backfill::write_finished_atomic(&path).unwrap();
    assert!(backfill::is_finished(&path));
    assert_eq!(backfill::read_cursor(&path), None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), backfill::FINISHED_SENTINEL);
    assert!(!std::path::Path::new(&format!("{path}.tmp")).exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn read_start_cursor_missing_unix_or_finished() {
    let dir = temp_dir();
    let path = dir.join("start_cursor.txt").to_string_lossy().to_string();
    assert_eq!(backfill::read_start_cursor(&path), None);
    assert!(!backfill::is_finished(&path));

    backfill::write_cursor_atomic(&path, 1376988004).unwrap();
    assert_eq!(backfill::read_start_cursor(&path), Some(1376988004));
    assert!(!backfill::is_finished(&path));

    backfill::write_finished_atomic(&path).unwrap();
    assert!(backfill::is_finished(&path));
    assert_eq!(backfill::read_start_cursor(&path), None);
    let _ = std::fs::remove_dir_all(dir);
}
