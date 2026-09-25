use std::sync::{Arc, Mutex};

use crab_core::trackers::Cancelled;
use crab_trackers_e::knaben::backfill::{self, KnabenFetchPage, KnabenPageOutcome};
use crab_trackers_e::knaben::models::{KnabenApiRequest, KnabenApiResponse, KnabenBackfillState};
use crab_trackers_e::knaben::parser;
use indexmap::IndexMap;
use tokio_util::sync::CancellationToken;

fn page(raw: i32, total: i32, relation: &str) -> KnabenFetchPage {
    KnabenFetchPage {
        is_valid: true,
        raw_hit_count: raw,
        total_value: Some(total),
        total_relation: Some(relation.into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn fetch_with_retry_empty_then_full_returns_full_on_second_attempt() {
    let attempts = Arc::new(Mutex::new(0));
    let delays = Arc::new(Mutex::new(Vec::<u64>::new()));
    let ct = CancellationToken::new();

    let a = attempts.clone();
    let d = delays.clone();
    let (p, outcome, n) = backfill::fetch_with_retry::<Cancelled, _, _, _, _>(
        move || {
            let a = a.clone();
            async move {
                let mut g = a.lock().unwrap();
                *g += 1;
                Ok(Some(if *g == 1 { page(0, 10000, "gte") } else { page(300, 10000, "gte") }))
            }
        },
        300,
        0,
        move |ms| {
            d.lock().unwrap().push(ms);
            async { Ok(()) }
        },
        &ct,
        backfill::MAX_ATTEMPTS,
        None,
    )
    .await
    .expect("ok");

    assert_eq!(outcome, KnabenPageOutcome::Full);
    assert_eq!(n, 2);
    assert_eq!(p.raw_hit_count, 300);
    assert_eq!(*delays.lock().unwrap(), vec![2000]);
}

#[tokio::test]
async fn fetch_with_retry_exhausts_attempts_with_backoff_ladder() {
    let delays = Arc::new(Mutex::new(Vec::<u64>::new()));
    let d = delays.clone();
    let ct = CancellationToken::new();
    let mut seen = Vec::new();
    let mut cb = |_: &KnabenFetchPage, o: KnabenPageOutcome, n: i32| seen.push((o, n));
    let (_, outcome, n) = backfill::fetch_with_retry::<Cancelled, _, _, _, _>(
        || async { Ok(None) },
        300,
        0,
        move |ms| {
            d.lock().unwrap().push(ms);
            async { Ok(()) }
        },
        &ct,
        3,
        Some(&mut cb),
    )
    .await
    .expect("ok");
    assert_eq!(outcome, KnabenPageOutcome::Retryable);
    assert_eq!(n, 3);
    assert_eq!(*delays.lock().unwrap(), vec![2000, 8000]);
    assert_eq!(seen.len(), 3);
    assert_eq!(outcome.to_string(), "Retryable");
}

#[test]
fn classify_full_raw_page_is_full_even_if_mapped_count_would_be_lower() {
    assert_eq!(backfill::classify(true, 300, 300, 0, Some(10000), Some("gte")), KnabenPageOutcome::Full);
    assert_eq!(backfill::classify(true, 280, 300, 0, Some(10000), Some("gte")), KnabenPageOutcome::Retryable);
}

#[test]
fn classify_eq_total_confirms_end_of_feed() {
    assert_eq!(backfill::classify(true, 171, 300, 4500, Some(4671), Some("eq")), KnabenPageOutcome::EndOfFeed);
    assert_eq!(backfill::classify(false, 300, 300, 0, None, None), KnabenPageOutcome::Retryable);
}

#[test]
fn advance_backfill_pass_desc_without_overlap_marks_partial() {
    let mut status = IndexMap::new();
    status.insert("3005000".to_string(), "pending".to_string());
    let mut state = KnabenBackfillState {
        CategoryIndex: 12,
        CategoryId: 3005000,
        Direction: "desc".into(),
        From: 10000,
        AscEdgeIds: vec!["asc-edge".into()],
        DescSawOverlap: false,
        CategoryStatus: status,
        ..Default::default()
    };
    backfill::advance_backfill_pass(&mut state, Some(&["desc-only".to_string()]), false);
    assert_eq!(state.CategoryStatus["3005000"], "partial");
    assert_eq!(state.CategoryId, 3006000);
    assert_eq!(state.Direction, "asc");
    assert_eq!(state.From, 0);
    assert!(!state.Finished);
}

#[test]
fn advance_backfill_pass_asc_window_switches_to_desc_and_keeps_edge() {
    let mut state = backfill::create_fresh_state();
    state.AscEdgeIds = vec!["x".into()];
    state.From = 10000;
    backfill::advance_backfill_pass(&mut state, None, false);
    assert_eq!(state.Direction, "desc");
    assert_eq!(state.From, 0);
    assert_eq!(state.AscEdgeIds, vec!["x".to_string()]);
    assert_eq!(state.CategoryId, 2001000);
}

#[test]
fn advance_backfill_pass_last_category_finishes() {
    let mut state = backfill::create_fresh_state();
    state.CategoryIndex = 15;
    state.CategoryId = 3008000;
    backfill::advance_backfill_pass(&mut state, None, true);
    assert!(state.Finished);
    assert_eq!(state.CategoryId, 0);
    assert_eq!(state.CategoryStatus["3008000"], "complete");
    assert!(backfill::format_backfill_progress(&state).starts_with("progress=1/16"));
}

#[test]
fn backfill_state_json_uses_pascal_case_and_roundtrips() {
    let state = backfill::create_fresh_state();
    let json = serde_json::to_string_pretty(&state).unwrap();
    assert!(json.contains("\"CategoryIndex\": 0"));
    assert!(json.contains("\"Direction\": \"asc\""));
    assert!(json.contains("\"UpdatedAt\""));
    let back: KnabenBackfillState = serde_json::from_str(&json).unwrap();
    assert_eq!(back.CategoryStatus.len(), 16);
    let lenient: KnabenBackfillState = serde_json::from_str(r#"{"CategoryStatus":null,"AscEdgeIds":null,"Direction":null}"#).unwrap();
    assert!(lenient.CategoryStatus.is_empty());
    assert!(lenient.AscEdgeIds.is_empty());
}

#[test]
fn api_request_omits_null_fields() {
    let req = KnabenApiRequest {
        categories: Some(vec![2001000]),
        order_by: Some("date".into()),
        order_direction: Some("desc".into()),
        from: 0,
        size: 300,
        hide_unsafe: true,
        hide_xxx: true,
        ..Default::default()
    };
    let json = serde_json::to_string(&req).unwrap();
    assert_eq!(
        json,
        r#"{"categories":[2001000],"order_by":"date","order_direction":"desc","from":0,"size":300,"hide_unsafe":true,"hide_xxx":true}"#
    );
}

#[test]
fn response_maps_hits_and_ids() {
    let json = r#"{"total":{"relation":"eq","value":2},"hits":[
        {"title":"Call the Midwife S15E08 1080p WEB-DL","bytes":2147483648,"seeders":5,"peers":2,"magnetUrl":"magnet:?xt=urn:btih:abc",
         "details":"https://example.org/t/1","categoryId":[2001000],"date":"2026-01-02T03:04:05+00:00","tracker":"EZTV","id":"h1"},
        {"title":"War.Machine.2026.2160P.WEB-DL.x265","bytes":1048576,"link":"https://example.org/dl/2.torrent","categoryId":[3003000],"id":"h2"}
    ]}"#;
    let resp: KnabenApiResponse = serde_json::from_str(json).unwrap();
    let p = KnabenFetchPage::from_response(Some(&resp));
    assert!(p.is_valid);
    assert_eq!(p.raw_hit_count, 2);
    assert_eq!(p.ids, vec!["h1", "h2"]);
    assert_eq!(p.torrents.len(), 2);

    let a = &p.torrents[0];
    assert_eq!(a.trackerName, "knaben");
    assert_eq!(a.types, vec!["serial"]);
    assert_eq!(a.name, "Call the Midwife");
    assert_eq!(a.originalname, "Call the Midwife");
    assert_eq!(a.sizeName, "2.00 GB");
    assert!(a.title.ends_with(" | EZTV"));
    assert_eq!(a.url, "https://example.org/t/1");

    let b = &p.torrents[1];
    assert_eq!(b.types, vec!["movie"]);
    assert_eq!(b.name, "War Machine");
    assert_eq!(b.relased, 2026);
    assert!(b.magnet.is_empty());
    assert_eq!(b._sn, "https://example.org/dl/2.torrent");
    assert_eq!(b.url, "https://example.org/dl/2.torrent");
    assert!(b.title.contains("2160p"));
    assert_eq!(b.sizeName, "1.00 Mb");
}

#[test]
fn missing_hits_is_invalid_page() {
    let resp: KnabenApiResponse = serde_json::from_str(r#"{"total":{"relation":"eq","value":0}}"#).unwrap();
    assert!(!KnabenFetchPage::from_response(Some(&resp)).is_valid);
    assert!(!KnabenFetchPage::from_response(None).is_valid);
}

#[test]
fn name_and_year_shapes() {
    assert_eq!(parser::parse_name_and_year("Some Movie (2024) 1080p"), (Some("Some Movie".to_string()), 2024));
    assert_eq!(parser::parse_name_and_year("Фильм [2026, драма, WEB-DL 1080p]"), (Some("Фильм".to_string()), 2026));
    assert_eq!(parser::parse_name_and_year("  "), (None, 0));
    assert_eq!(parser::clean_title_for_search("Show.Name.S01E02.720p.HDTV.x264-GRP"), "Show Name");
}

#[test]
fn build_title_for_filedb_normalises_hdr() {
    assert_eq!(parser::build_title_for_filedb("Movie 2160P.HDR10 x265"), "Movie 2160p HDR10 x265");
    assert_eq!(parser::build_title_for_filedb("Movie 1080p Dolby Vision"), "Movie 1080p Dolby Vision HDR");
    assert_eq!(parser::build_title_for_filedb("Movie [HDR] Dolby Vision"), "Movie [HDR] Dolby Vision");
}

#[test]
fn quality_and_types_from_category_ids() {
    assert_eq!(parser::quality_from_category_id(Some(&[2003000])), 2160);
    assert_eq!(parser::quality_from_category_id(Some(&[3001000])), 1080);
    assert_eq!(parser::quality_from_category_id(None), 480);
    assert_eq!(parser::types_from_category_id(None), &["movie", "serial"]);
    assert_eq!(parser::types_from_category_id(Some(&[5000000])), &["movie", "serial"]);
}
