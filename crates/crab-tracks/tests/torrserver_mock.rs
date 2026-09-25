//! Local stand-in for the TorrServer `/torrents` (get) and `/ffp` endpoints.

mod common;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use crab_tracks::tracks::db::next_failure_attempt;
use crab_tracks::tracks::remote::{self, FfpError};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const HASH: &str = "aabbccddeeff00112233445566778899aabbccdd";

struct Mock {
    get_calls: AtomicUsize,
    ffp_calls: AtomicUsize,
    /// Peer-stats style `get` response (seeders/bytes_read/optional file_stats).
    peer_mode: AtomicBool,
    ready_on_second_get: AtomicBool,
    include_file_stats: AtomicBool,
    connected_seeders: AtomicI32,
    bytes_read: AtomicI64,
    ffp_status: AtomicI32,
    ffp_body: Mutex<String>,
    ffp_delay_ms: AtomicU64,
    /// When > 0: `/ffp/{hash}/{id}` succeeds only for this id, 400 otherwise.
    success_on_file_id: AtomicI32,
}

impl Default for Mock {
    fn default() -> Self {
        Mock {
            get_calls: AtomicUsize::new(0),
            ffp_calls: AtomicUsize::new(0),
            peer_mode: AtomicBool::new(false),
            ready_on_second_get: AtomicBool::new(false),
            include_file_stats: AtomicBool::new(true),
            connected_seeders: AtomicI32::new(0),
            bytes_read: AtomicI64::new(0),
            ffp_status: AtomicI32::new(200),
            ffp_body: Mutex::new(r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"aac","tags":{"language":"rus"}}]}"#.into()),
            ffp_delay_ms: AtomicU64::new(0),
            success_on_file_id: AtomicI32::new(0),
        }
    }
}

async fn torrents(State(m): State<Arc<Mock>>, body: String) -> impl IntoResponse {
    let json_hdr = [(axum::http::header::CONTENT_TYPE, "application/json")];
    if !(body.contains("\"action\":\"get\"") || body.contains("\"action\": \"get\"")) {
        return (StatusCode::OK, json_hdr, "[]".to_string());
    }
    let calls = m.get_calls.fetch_add(1, Ordering::SeqCst) + 1;
    let json = if m.peer_mode.load(Ordering::SeqCst) {
        let fs = if m.include_file_stats.load(Ordering::SeqCst) { r#","file_stats":[{"id":1,"path":"a.mkv","length":10}]"# } else { "" };
        format!(
            r#"{{"hash":"{HASH}","stat":3,"category":"crabindex","connected_seeders":{},"bytes_read":{}{fs}}}"#,
            m.connected_seeders.load(Ordering::SeqCst),
            m.bytes_read.load(Ordering::SeqCst)
        )
    } else if !m.ready_on_second_get.load(Ordering::SeqCst) || calls >= 2 {
        format!(
            r#"{{"hash":"{HASH}","stat":3,"stat_string":"Torrent working","category":"crabindex","connected_seeders":1,"file_stats":[{{"id":1,"path":"a.mkv","length":10}}]}}"#
        )
    } else {
        format!(r#"{{"hash":"{HASH}","stat":1,"stat_string":"Torrent getting info","category":"crabindex"}}"#)
    };
    (StatusCode::OK, json_hdr, json)
}

async fn ffp(State(m): State<Arc<Mock>>, Path((_hash, id)): Path<(String, i32)>) -> impl IntoResponse {
    m.ffp_calls.fetch_add(1, Ordering::SeqCst);
    let delay = m.ffp_delay_ms.load(Ordering::SeqCst);
    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    let success_id = m.success_on_file_id.load(Ordering::SeqCst);
    let (status, body) = if success_id > 0 {
        if id == success_id {
            (200, r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264"}]}"#.to_string())
        } else {
            (400, "error".to_string())
        }
    } else {
        (m.ffp_status.load(Ordering::SeqCst), m.ffp_body.lock().clone())
    };
    (StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK), [(axum::http::header::CONTENT_TYPE, "application/json")], body)
}

async fn start(mock: Arc<Mock>) -> String {
    let app = Router::new().route("/torrents", post(torrents)).route("/ffp/:hash/:id", get(ffp)).with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

fn none() -> CancellationToken {
    CancellationToken::new()
}

// --- readiness / ffp basics ---

#[tokio::test]
async fn wait_torrent_ready_returns_true_when_file_stats_present() {
    common::cfg();
    let m = Arc::new(Mock::default());
    let base = start(m.clone()).await;
    let ready = remote::wait_torrent_ready(&base, HASH, &none(), None, Some(Duration::from_secs(5))).await.unwrap();
    assert!(ready);
    assert!(m.get_calls.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn wait_torrent_ready_polls_until_file_stats_appear() {
    common::cfg();
    let m = Arc::new(Mock::default());
    m.ready_on_second_get.store(true, Ordering::SeqCst);
    let base = start(m.clone()).await;
    let ready = remote::wait_torrent_ready(&base, HASH, &none(), None, Some(Duration::from_secs(10))).await.unwrap();
    assert!(ready);
    assert!(m.get_calls.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn analyze_with_external_api_deserializes_200_streams() {
    common::cfg();
    let m = Arc::new(Mock::default());
    let base = start(m).await;
    let (result, code) = remote::analyze_with_external_api(&base, HASH, &none(), None, None, 1).await.unwrap();
    assert_eq!(code, 200);
    let streams = result.unwrap().streams.unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].tags.as_ref().unwrap().language.as_deref(), Some("rus"));
}

#[tokio::test]
async fn analyze_with_external_api_returns_status_on_400_without_streams() {
    common::cfg();
    let m = Arc::new(Mock::default());
    m.ffp_status.store(400, Ordering::SeqCst);
    *m.ffp_body.lock() = "error getting data".into();
    let base = start(m).await;
    let (result, code) = remote::analyze_with_external_api(&base, HASH, &none(), None, None, 1).await.unwrap();
    assert_eq!(code, 400);
    assert!(result.is_none());
    assert_eq!(next_failure_attempt(0), 1);
}

#[tokio::test]
async fn analyze_with_external_api_reports_json_error_on_200_garbage() {
    common::cfg();
    let m = Arc::new(Mock::default());
    *m.ffp_body.lock() = "error getting data".into();
    let base = start(m).await;
    let r = remote::analyze_with_external_api(&base, HASH, &none(), None, None, 1).await;
    assert!(matches!(r, Err(FfpError::Json(_))));
}

// --- adaptive timeouts ---

fn peer_mock() -> Arc<Mock> {
    let m = Mock::default();
    m.peer_mode.store(true, Ordering::SeqCst);
    Arc::new(m)
}

#[tokio::test]
async fn wait_download_progress_false_without_seeders_and_bytes() {
    common::cfg();
    let m = peer_mock();
    let base = start(m).await;
    let ok = remote::wait_download_progress(&base, HASH, &none(), None, Some(Duration::from_secs(3))).await.unwrap();
    assert!(!ok);
}

#[tokio::test]
async fn wait_download_progress_true_when_bytes_read_positive() {
    common::cfg();
    let m = peer_mock();
    m.bytes_read.store(4096, Ordering::SeqCst);
    let base = start(m).await;
    let ok = remote::wait_download_progress(&base, HASH, &none(), None, Some(Duration::from_secs(3))).await.unwrap();
    assert!(ok);
}

#[tokio::test]
async fn wait_download_progress_true_when_connected_seeders_positive() {
    common::cfg();
    let m = peer_mock();
    m.connected_seeders.store(2, Ordering::SeqCst);
    let base = start(m).await;
    let ok = remote::wait_download_progress(&base, HASH, &none(), None, Some(Duration::from_secs(3))).await.unwrap();
    assert!(ok);
}

#[tokio::test]
async fn not_ready_and_no_peers_skips_ffp() {
    common::cfg();
    let m = peer_mock();
    m.include_file_stats.store(false, Ordering::SeqCst);
    let base = start(m).await;
    let ready = remote::wait_torrent_ready(&base, HASH, &none(), None, Some(Duration::from_secs(1))).await.unwrap();
    assert!(!ready);
    let can_probe = remote::wait_download_progress(&base, HASH, &none(), None, Some(Duration::from_secs(2))).await.unwrap();
    assert!(!can_probe);
}

#[tokio::test]
async fn analyze_with_external_api_respects_custom_timeout() {
    common::cfg();
    let m = peer_mock();
    m.ffp_delay_ms.store(5000, Ordering::SeqCst);
    let base = start(m).await;
    let sw = Instant::now();
    let r = remote::analyze_with_external_api(&base, HASH, &none(), None, Some(Duration::from_millis(800)), 1).await;
    assert!(matches!(r, Err(FfpError::Timeout)));
    let ms = sw.elapsed().as_millis();
    assert!((400..=3000).contains(&ms), "{ms}");
}

#[tokio::test]
async fn analyze_with_external_api_completes_within_custom_timeout() {
    common::cfg();
    let m = peer_mock();
    m.ffp_delay_ms.store(100, Ordering::SeqCst);
    *m.ffp_body.lock() = r#"{"streams":[{"index":0,"codec_type":"audio"}]}"#.into();
    let base = start(m).await;
    let (result, code) =
        remote::analyze_with_external_api(&base, HASH, &none(), None, Some(Duration::from_secs(2)), 1).await.unwrap();
    assert_eq!(code, 200);
    assert_eq!(result.unwrap().streams.unwrap().len(), 1);
}

#[tokio::test]
async fn caller_cancellation_is_distinct_from_timeout() {
    common::cfg();
    let m = peer_mock();
    m.ffp_delay_ms.store(5000, Ordering::SeqCst);
    let base = start(m).await;
    let token = CancellationToken::new();
    let t2 = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        t2.cancel();
    });
    let r = remote::analyze_with_external_api(&base, HASH, &token, None, Some(Duration::from_secs(10)), 1).await;
    assert!(matches!(r, Err(FfpError::Cancelled)));
}

// --- retries over file ids ---

#[tokio::test]
async fn probe_ffp_with_retries_succeeds_on_later_file_id() {
    common::cfg();
    let m = Arc::new(Mock::default());
    m.success_on_file_id.store(5, Ordering::SeqCst);
    let base = start(m.clone()).await;
    let (result, code, err) =
        remote::probe_ffp_with_retries(&base, HASH, &[1, 2, 5], Duration::from_secs(5), &none(), None).await.unwrap();
    assert!(err.is_none());
    assert_eq!(code, 200);
    assert_eq!(result.unwrap().streams.unwrap().len(), 1);
    assert_eq!(m.ffp_calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn probe_ffp_with_retries_reports_no_probeable_file() {
    common::cfg();
    let m = Arc::new(Mock::default());
    m.success_on_file_id.store(9, Ordering::SeqCst);
    let base = start(m).await;
    let (result, code, err) =
        remote::probe_ffp_with_retries(&base, HASH, &[1, 2], Duration::from_secs(5), &none(), None).await.unwrap();
    assert!(result.is_none());
    assert_eq!(code, 400);
    assert_eq!(err.as_deref(), Some("no probeable media file"));
}
