mod common;

use crab_tracks::tracks::db::{next_failure_attempt, the_bad};
use crab_tracks::tracks::models::{self, format_utc, TorrentFileStat, TorrentInfo};
use crab_tracks::tracks::selector::{is_video_candidate, select_file_ids};
use crab_tracks::tracks::{cron, remote};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).expect("fixture")
}

// --- ffprobe body ---

#[test]
fn ffprobe_deserializes_streams_and_tags() {
    let model = models::parse_ffprobe(&fixture("ffp_probe_full.json")).unwrap().unwrap();
    let streams = model.streams.unwrap();
    assert_eq!(streams.len(), 2);

    let video = &streams[0];
    assert_eq!(video.codec_type.as_deref(), Some("video"));
    assert_eq!(video.codec_name.as_deref(), Some("h264"));
    assert_eq!(video.width, Some(1920));
    assert_eq!(video.height, Some(1080));

    let audio = &streams[1];
    assert_eq!(audio.codec_type.as_deref(), Some("audio"));
    assert_eq!(audio.codec_name.as_deref(), Some("aac"));
    let tags = audio.tags.as_ref().unwrap();
    assert_eq!(tags.language.as_deref(), Some("rus"));
    assert_eq!(tags.title.as_deref(), Some("LostFilm"));
    assert_eq!(tags.BPS.as_deref(), Some("192000"));
}

#[test]
fn ffprobe_ignores_unknown_format_chapters_fields() {
    let model = models::parse_ffprobe(&fixture("ffp_probe_unknown_fields.json")).unwrap().unwrap();
    let streams = model.streams.unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].tags.as_ref().unwrap().language.as_deref(), Some("eng"));
}

#[test]
fn ffprobe_tolerates_bom_and_null() {
    let text = format!("\u{feff}{}", fixture("ffp_probe_unknown_fields.json"));
    assert!(models::parse_ffprobe(&text).unwrap().is_some());
    assert!(models::parse_ffprobe("null").unwrap().is_none());
    assert!(models::parse_ffprobe("error getting data").is_err());
}

#[test]
fn track_file_json_keeps_all_properties() {
    common::cfg();
    let model = models::parse_ffprobe(&fixture("ffp_probe_unknown_fields.json")).unwrap().unwrap();
    let json = models::ffprobe_to_json(&model);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let s = &v["streams"][0];
    assert!(s.get("width").unwrap().is_null());
    assert!(s["tags"].get("BPS").unwrap().is_null());
    let keys: Vec<&str> = s.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys.first(), Some(&"index"));
    assert_eq!(keys.last(), Some(&"tags"));
    assert!(json.contains("\n  \"streams\": ["));
    // round trip
    let back = models::parse_ffprobe(&json).unwrap().unwrap();
    assert_eq!(back.streams, model.streams);
}

#[test]
fn utc_date_format() {
    common::cfg();
    use chrono::TimeZone;
    let d = chrono::Utc.with_ymd_and_hms(2024, 5, 1, 12, 0, 0).unwrap();
    assert_eq!(format_utc(&d), "2024-05-01T12:00:00Z");
    let d = d + chrono::Duration::nanoseconds(123_400_000);
    assert_eq!(format_utc(&d), "2024-05-01T12:00:00.1234Z");
}

// --- TorrServer status ---

#[test]
fn torrent_info_deserializes_file_stats() {
    common::cfg();
    let info: TorrentInfo = serde_json::from_str(&fixture("ts_get_file_stats.json")).unwrap();
    assert_eq!(info.stat, 3);
    assert_eq!(info.category.as_deref(), Some("crabindex"));
    let fs = info.file_stats.unwrap();
    assert_eq!(fs.len(), 2);
    assert_eq!(fs[0].id, 1);
    assert_eq!(fs[0].path.as_deref(), Some("Example.mkv"));
    assert_eq!(fs[0].length, 123_456_789);
}

#[test]
fn torrent_info_deserializes_peer_stats() {
    common::cfg();
    let info: TorrentInfo = serde_json::from_str(&fixture("ts_get_peer_stats.json")).unwrap();
    assert_eq!(info.connected_seeders, 2);
    assert_eq!(info.active_peers, 5);
    assert_eq!(info.download_speed, 1024);
    assert_eq!(info.bytes_read, 65536);
}

#[test]
fn torrent_info_ready_when_file_stats_non_empty() {
    common::cfg();
    let not_ready = TorrentInfo { stat: 1, file_stats: None, ..Default::default() };
    let ready = TorrentInfo { stat: 2, file_stats: Some(vec![TorrentFileStat::new(1, "a.mkv", 1)]), ..Default::default() };
    assert!(ready.file_stats.as_ref().is_some_and(|f| !f.is_empty()));
    assert!(not_ready.file_stats.as_ref().is_none_or(|f| f.is_empty()));
}

// --- media file selection ---

#[test]
fn select_picks_largest_mkv_not_first_txt() {
    common::cfg();
    let files = vec![
        TorrentFileStat::new(1, "readme.txt", 100),
        TorrentFileStat::new(2, "Sample.mkv", 50_000_000),
        TorrentFileStat::new(3, "Movie.mkv", 8_000_000_000),
        TorrentFileStat::new(4, "subs.srt", 5000),
    ];
    let ids = select_file_ids(Some(&files), 3);
    assert_eq!(ids[0], 3);
    assert_eq!(ids[1], 2);
    assert!(!ids.contains(&1));
}

#[test]
fn select_excludes_sample_in_path() {
    common::cfg();
    let files = vec![TorrentFileStat::new(1, "release.sample.mkv", 10_000_000), TorrentFileStat::new(2, "film.mkv", 5_000_000_000)];
    let ids = select_file_ids(Some(&files), 2);
    assert_eq!(ids, vec![2]);
}

#[test]
fn select_falls_back_to_largest_without_video_ext() {
    common::cfg();
    let files = vec![TorrentFileStat::new(1, "a.bin", 100), TorrentFileStat::new(2, "b.bin", 9000)];
    assert_eq!(select_file_ids(Some(&files), 1)[0], 2);
}

#[test]
fn select_empty_returns_id_1() {
    common::cfg();
    assert_eq!(select_file_ids(None, 3), vec![1]);
    assert_eq!(select_file_ids(Some(&[]), 3), vec![1]);
}

#[test]
fn video_candidate_recognizes_mkv() {
    common::cfg();
    assert!(is_video_candidate("Season 1/Episode.mkv"));
    assert!(!is_video_candidate("info.nfo"));
    assert!(!is_video_candidate("extras/trailer.mkv"));
}

// --- type filter / attempts / delay ---

#[test]
fn the_bad_filters_unsupported_types() {
    common::cfg();
    let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(the_bad(&[]));
    assert!(the_bad(&v(&["sport"])));
    assert!(the_bad(&v(&["tvshow"])));
    assert!(the_bad(&v(&["docuserial"])));
    assert!(!the_bad(&v(&["movie"])));
    assert!(!the_bad(&v(&["serial"])));
    assert!(the_bad(&v(&["movie", "sport"])));
}

#[test]
fn next_failure_attempt_increments_by_one() {
    common::cfg();
    assert_eq!(next_failure_attempt(0), 1);
    assert_eq!(next_failure_attempt(1), 2);
    assert_eq!(next_failure_attempt(19), 20);
}

#[test]
fn inter_item_delay_uses_tracksdelay_with_jitter_bounds() {
    common::cfg();
    let base = crab_core::conf().tracksdelay;
    if base <= 0 {
        assert_eq!(cron::get_inter_item_delay_ms(), 0);
        return;
    }
    let jitter = (base / 10).max(1);
    for _ in 0..20 {
        let d = cron::get_inter_item_delay_ms();
        assert!(d >= base - jitter && d <= base + jitter, "{d}");
    }
}

// --- hash lock ---

#[test]
fn hash_lock_blocks_second_caller() {
    common::cfg();
    const HASH: &str = "aabbccddeeff00112233445566778899aabbccdd";
    {
        let first = remote::try_acquire_hash_lock(HASH);
        assert!(first.is_some());
        assert!(remote::try_acquire_hash_lock(HASH).is_none());
        assert!(remote::try_acquire_hash_lock(&HASH.to_uppercase()).is_none());
    }
    assert!(remote::try_acquire_hash_lock(HASH).is_some());
}
