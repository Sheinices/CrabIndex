mod common;

use chrono::{TimeZone, Utc};
use crab_core::models::{FfStream, FfprobeModel, TorrentDetails};
use crab_tracks::stats::{self, StatsFdbScanResult};
use crab_tracks::tracks::{db, index, models, paths};
use std::path::{Path, PathBuf};

const H: &str = "aabbccddeeff00112233445566778899aabbccdd";

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("crab-tracks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn model(n: usize) -> FfprobeModel {
    FfprobeModel {
        streams: Some((0..n).map(|i| FfStream { index: i as i32, codec_type: Some("audio".into()), ..Default::default() }).collect()),
    }
}

#[test]
fn stats_json_shape_and_order() {
    common::cfg();
    let today = Utc.with_ymd_and_hms(2024, 5, 1, 0, 0, 0).unwrap();
    let mut scan = StatsFdbScanResult::default();
    let mk = |tracker: &str, created: chrono::DateTime<Utc>, magnet: &str, types: &[&str]| {
        let mut t = TorrentDetails::new(tracker, types, "u", "t");
        t.createTime = created;
        t.updateTime = created;
        t.checkTime = created;
        t.magnet = magnet.into();
        t
    };
    let old = Utc.with_ymd_and_hms(2024, 4, 1, 10, 0, 0).unwrap();
    let new = Utc.with_ymd_and_hms(2024, 5, 1, 10, 0, 0).unwrap();
    stats::accumulate_tracker(&mut scan, &mk("rutor", old, "", &["movie"]), today);
    stats::accumulate_tracker(&mut scan, &mk("kinozal", old, "", &["movie"]), today);
    stats::accumulate_tracker(&mut scan, &mk("Kinozal", new, "", &["movie"]), today);
    let mut with_ff = mk("kinozal", old, &format!("magnet:?xt=urn:btih:{H}"), &["movie"]);
    with_ff.ffprobe = Some(vec![FfStream::default()]);
    stats::accumulate_tracker(&mut scan, &with_ff, today);
    stats::accumulate_tracker(&mut scan, &mk("kinozal", old, &format!("magnet:?xt=urn:btih:{H}"), &["sport"]), today);

    let v = stats::tracker_stats_json(&scan.trackers);
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["trackerName"], "kinozal");
    assert_eq!(arr[0]["alltorrents"], 4);
    assert_eq!(arr[0]["newtor"], 1);
    assert_eq!(arr[0]["update"], 1);
    assert_eq!(arr[0]["check"], 1);
    assert_eq!(arr[0]["lastnewtor"], "01.05.2024");
    assert_eq!(arr[0]["tracks"]["confirm"], 1);
    assert_eq!(arr[0]["tracks"]["wait"], 0);
    assert_eq!(arr[0]["tracks"]["skip"], 0);
    assert_eq!(arr[1]["trackerName"], "rutor");
    let keys: Vec<&str> = arr[0].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["trackerName", "lastnewtor", "newtor", "update", "check", "alltorrents", "tracks"]);
}

#[test]
fn write_text_atomic_replaces_file() {
    common::cfg();
    let d = temp_dir("atomic");
    let p = d.join("sub/stats.json");
    let ps = p.to_string_lossy().to_string();
    stats::write_text_atomic(&ps, "[1]").unwrap();
    stats::write_text_atomic(&ps, "[2]").unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "[2]");
    assert!(!Path::new(&format!("{ps}.tmp")).exists());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn track_file_written_with_bom_and_detected() {
    common::cfg();
    let d = temp_dir("trackfile");
    let dir = d.to_string_lossy().to_string();
    let path = paths::track_layout_path(&dir, H, true).unwrap();
    std::fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
    models::write_track_file(Path::new(&path), &model(2)).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..3], b"\xEF\xBB\xBF");
    assert!(paths::track_file_has_streams(Path::new(&path)));
    assert_eq!(paths::resolve_track_json_path(H, &dir).as_deref(), Some(path.as_str()));

    let empty = paths::track_layout_path(&dir, "00bbccddeeff00112233445566778899aabbccdd", true).unwrap();
    std::fs::create_dir_all(Path::new(&empty).parent().unwrap()).unwrap();
    std::fs::write(&empty, r#"{"streams":[]}"#).unwrap();
    assert!(!paths::track_file_has_streams(Path::new(&empty)));

    let mut found = Vec::new();
    index::scan_tracks_dir_for_index(&dir, &mut found);
    assert_eq!(found, vec![H.to_string()]);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn migrate_legacy_and_uppercase_layouts() {
    common::cfg();
    let d = temp_dir("migrate");
    let dir = d.to_string_lossy().to_string();
    // legacy: no extension
    let legacy = paths::track_layout_path(&dir, H, false).unwrap();
    std::fs::create_dir_all(Path::new(&legacy).parent().unwrap()).unwrap();
    std::fs::write(&legacy, models::ffprobe_to_json(&model(1))).unwrap();
    // uppercase .json of another hash
    let h2 = "11bbccddeeff00112233445566778899aabbccdd";
    let upper = paths::uppercase_layout_path(&dir, h2, true).unwrap();
    std::fs::create_dir_all(Path::new(&upper).parent().unwrap()).unwrap();
    std::fs::write(&upper, models::ffprobe_to_json(&model(1))).unwrap();

    assert_eq!(paths::migrate_track_layout_in_place(&dir, true), 2);
    assert!(Path::new(&legacy).exists());

    assert_eq!(paths::migrate_track_layout_in_place(&dir, false), 2);
    let canonical = paths::track_layout_path(&dir, H, true).unwrap();
    assert!(Path::new(&canonical).exists());
    assert!(!Path::new(&legacy).exists());
    let canonical2 = paths::track_layout_path(&dir, h2, true).unwrap();
    assert!(paths::resolve_track_json_path(h2, &dir).is_some());
    let listed: Vec<String> = std::fs::read_dir(Path::new(&canonical2).parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(listed, vec![Path::new(&canonical2).file_name().unwrap().to_string_lossy().to_string()]);

    // nothing left to do
    assert_eq!(paths::migrate_track_layout_in_place(&dir, false), 0);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn languages_merge_torrent_and_audio_streams() {
    common::cfg();
    let mut t = TorrentDetails::default();
    t.languages.insert("ukr".into());
    let streams = vec![
        FfStream {
            codec_type: Some("audio".into()),
            tags: Some(crab_core::models::FfTags { language: Some("rus".into()), ..Default::default() }),
            ..Default::default()
        },
        FfStream {
            codec_type: Some("subtitle".into()),
            tags: Some(crab_core::models::FfTags { language: Some("eng".into()), ..Default::default() }),
            ..Default::default()
        },
    ];
    let langs = db::languages(&t, Some(&streams)).unwrap();
    assert_eq!(langs.into_iter().collect::<Vec<_>>(), vec!["ukr".to_string(), "rus".to_string()]);
    assert!(db::languages(&TorrentDetails::default(), None).is_none());
}

#[test]
fn get_skips_bad_types_and_invalid_magnets() {
    common::cfg();
    let bad = vec!["sport".to_string()];
    assert!(db::get(&format!("magnet:?xt=urn:btih:{H}"), Some(&bad), false).is_none());
    assert!(db::get("not a magnet", None, false).is_none());
    // memory-only lookups never touch disk
    assert!(db::get(&format!("magnet:?xt=urn:btih:{H}"), None, true).is_none());
}

#[test]
fn track_row_ffprobe_counts_as_track() {
    common::cfg();
    let mut t = TorrentDetails::default();
    assert!(!db::has_track_for_torrent(&t));
    t.ffprobe = Some(vec![FfStream::default()]);
    assert!(db::has_track_for_torrent(&t));
}
