//! End-to-end checks against a real (temporary) Data/ directory. Everything touches the
//! process-wide masterDb, so it runs as one sequential test.

mod common;

use axum::body::Body;
use axum::http::Request;
use crab_core::fdb;
use crab_core::models::TorrentDetails;
use serde_json::Value;
use tower::ServiceExt;

fn row(tracker: &str, url: &str, name: &str, orig: &str) -> TorrentDetails {
    TorrentDetails {
        trackerName: tracker.into(),
        types: vec!["movie".into()],
        url: url.into(),
        title: format!("{name} / {orig}"),
        name: name.into(),
        originalname: orig.into(),
        magnet: format!("magnet:?xt=urn:btih:{}", crab_core::util::md5(url) + "abcdefgh"),
        sid: 3,
        _sn: crab_core::util::search_name_or_empty(name),
        _so: crab_core::util::search_name_or_empty(orig),
        ..Default::default()
    }
}

async fn get(path: &str) -> (u16, String) {
    let app = crab_ops::router();
    let res = app.oneshot(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn get_json(path: &str) -> Value {
    let (status, body) = get(path).await;
    assert_eq!(status, 200, "{path}: {body}");
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("{path}: {e}: {body}"))
}

async fn post(path: &str) -> (u16, String) {
    let app = crab_ops::router();
    let res = app.oneshot(Request::builder().method("POST").uri(path).body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fdb_operations_end_to_end() {
    common::enter_temp_cwd("fdb");
    crab_ops::init();

    fdb::add_or_update(&[
        row("rutor", "http://rutor.info/torrent/1", "Матрица", "The Matrix"),
        row("rutor", "http://rutor.info/torrent/2", "Пони", "Пони"),
        row("kinozal", "https://kinozal.guru/details.php?id=5", "Дюна", "Dune"),
    ]);
    {
        // old-domain duplicate written directly (normal upserts would merge it by id)
        let w = fdb::open_write("дюна:dune");
        w.modify(|db| {
            let url = "https://kinozal.tv/details.php?id=5";
            db.insert(url.into(), row("kinozal", url, "Дюна", "Dune"));
            true
        });
    }
    fdb::save_changes_to_file();
    assert_eq!(fdb::master_db().len(), 3);

    // ---- sync API ----
    let conf = get_json("/sync/conf").await;
    assert_eq!(serde_json::to_string(&conf).unwrap(), r#"{"fbd":true,"spidr":true,"version":2}"#);

    let empty = get_json("/sync/fdb/torrents?time=0").await;
    assert_eq!(serde_json::to_string(&empty).unwrap(), r#"{"nextread":false,"collections":[]}"#);

    let page = get_json("/sync/fdb/torrents?time=-1").await;
    assert_eq!(page["nextread"], false);
    assert_eq!(page["take"], 2000);
    assert_eq!(page["countread"], 4);
    let cols = page["collections"].as_array().unwrap();
    assert_eq!(cols.len(), 3);
    let ft: Vec<i64> = cols.iter().map(|c| c["Value"]["fileTime"].as_i64().unwrap()).collect();
    assert!(ft.windows(2).all(|w| w[0] <= w[1]), "ordered by fileTime");
    let first_key = cols[0]["Key"].as_str().unwrap().to_string();
    let t = cols.iter().find(|c| c["Key"] == "матрица:thematrix").unwrap()["Value"]["torrents"]["http://rutor.info/torrent/1"].clone();
    assert_eq!(t["trackerName"], "rutor");
    assert_eq!(t["name"], "Матрица");
    assert!(t.get("magnet").is_some());

    // cursor past the first bucket
    let after = get_json(&format!("/sync/fdb/torrents?time={}", ft[0])).await;
    assert!(after["collections"].as_array().unwrap().iter().all(|c| c["Key"] != first_key.as_str()) || ft[0] == ft[1]);

    let spidr = get_json("/sync/fdb/torrents?time=-1&spidr=true").await;
    let slim = &spidr["collections"][0]["Value"]["torrents"];
    let (_, slim_row) = slim.as_object().unwrap().iter().next().unwrap();
    assert!(slim_row.get("trackerName").is_none());
    assert!(slim_row.get("magnet").is_none());
    assert!(slim_row.get("sid").is_some() && slim_row.get("url").is_some());

    let by_key = get_json("/sync/fdb?key=матрица").await;
    assert_eq!(by_key.as_array().unwrap().len(), 1);
    assert!(by_key[0]["path"].as_str().unwrap().starts_with("Data/fdb/"));
    assert!(by_key[0]["value"]["http://rutor.info/torrent/1"].is_object());

    let legacy = get_json("/sync/torrents").await;
    assert_eq!(legacy["error"], "use GET /sync/fdb/torrents");

    // ---- jsondb/save (GET and POST) ----
    let (s, body) = post("/jsondb/save").await;
    assert_eq!((s, body.as_str()), (200, "ok"));

    // ---- dev diagnostics ----
    let dups = get_json("/dev/findduplicatekeys").await;
    assert_eq!(dups["count"], 1);
    assert_eq!(dups["keys"][0]["key"], "пони:пони");
    let dups = get_json("/dev/findduplicatekeys?tracker=kinozal").await;
    assert_eq!(dups["count"], 0);

    // a shard with a null row + an orphan file + a row with empty _sn/_so
    let null_key = "нуль:null";
    let mut raw = indexmap::IndexMap::<String, Value>::new();
    raw.insert("http://x/1".into(), Value::Null);
    raw.insert("http://x/2".into(), serde_json::to_value(row("rutor", "http://x/2", "Нуль", "Null")).unwrap());
    fdb::write_gz_json(&fdb::path_for_key(null_key), &raw);
    fdb::set_shard(null_key, chrono::Utc::now());
    std::fs::create_dir_all("Data/fdb/zz").unwrap();
    std::fs::write("Data/fdb/zz/orphan", b"x").unwrap();
    {
        let w = fdb::open_write("матрица:thematrix");
        w.modify(|db| {
            for t in db.values_mut() {
                t._sn.clear();
                t._so.clear();
            }
            true
        });
    }

    let corrupt = get_json("/dev/findcorrupt?samplesize=5").await;
    assert_eq!(corrupt["corrupt"]["nullValue"]["count"], 1);
    assert_eq!(corrupt["totalTorrents"], 6);
    let empty_sf = get_json("/dev/findemptysearchfields").await;
    assert_eq!(empty_sf["emptySearchFields"]["emptyBoth"]["count"], 1);
    assert!(empty_sf["emptySearchFields"]["emptyBoth"]["sample"][0].get("title").is_some());

    // ---- maintenance: report ----
    let ct = tokio_util::sync::CancellationToken::new();
    let ok = tokio::task::spawn_blocking(move || crab_ops::maintenance::run("report", 20, true, &ct, false)).await.unwrap();
    assert!(matches!(ok, Ok(true)));
    let report: Value = serde_json::from_str(&std::fs::read_to_string(crab_ops::maintenance::REPORT_PATH).unwrap()).unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["mode"], "report");
    assert_eq!(report["totals"]["fdbKeys"], 4);
    assert_eq!(report["issues"]["nullValue"]["count"], 1);
    assert_eq!(report["issues"]["xxKeys"]["count"], 1);
    assert_eq!(report["issues"]["orphanShardFiles"]["count"], 1);
    assert_eq!(report["issues"]["emptySearchFields"]["emptyBoth"]["count"], 1);
    assert_eq!(report["fixed"]["orphansDeleted"], 0);

    let status = get_json("/cron/maintenance/status").await;
    assert_eq!(status["running"], false);
    assert!(status.get("progress").is_none());
    assert_eq!(status["last"]["mode"], "report");

    // ---- maintenance: full ----
    let ct = tokio_util::sync::CancellationToken::new();
    let ok = tokio::task::spawn_blocking(move || crab_ops::maintenance::run("full", 20, true, &ct, false)).await.unwrap();
    assert!(matches!(ok, Ok(true)));
    let report: Value = serde_json::from_str(&std::fs::read_to_string(crab_ops::maintenance::REPORT_PATH).unwrap()).unwrap();
    assert_eq!(report["fixed"]["nullRemoved"], 1);
    assert_eq!(report["fixed"]["searchFieldsFixed"], 1);
    assert_eq!(report["fixed"]["orphansDeleted"], 1);
    assert!(!std::path::Path::new("Data/fdb/zz/orphan").exists());
    let shard = fdb::open_read(null_key, false, false);
    assert_eq!(shard.len(), 1);
    let fixed_row = &fdb::open_read("матрица:thematrix", false, false)["http://rutor.info/torrent/1"];
    assert_eq!(fixed_row._sn, "матрица");
    assert_eq!(fixed_row._so, "thematrix");

    // ---- background check endpoint ----
    let (s, body) = get("/cron/maintenance/check?mode=safe&samplesize=abc").await;
    assert_eq!((s, body.as_str()), (200, "ok"));
    for _ in 0..200 {
        if get_json("/cron/maintenance/status").await["running"] == false && !crab_core::trackers::has_active_job("maintenance", None) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    // ---- dev migrations ----
    let kz = get_json("/dev/fixkinozaldomainduplicates").await;
    assert_eq!(kz["ok"], true);
    assert_eq!(kz["scanned"], 2);
    assert_eq!(kz["merged"], 1);
    assert_eq!(kz["canonicalHost"], "kinozal.guru");
    let dune = fdb::open_read("дюна:dune", false, false);
    assert_eq!(dune.keys().collect::<Vec<_>>(), vec!["https://kinozal.guru/details.php?id=5"]);

    let rb = get_json("/dev/removebucket").await;
    assert_eq!(rb["error"], "key required, format: name:originalname (e.g. ponies:ponies)");
    let rb = get_json("/dev/removebucket?key=nope:nope").await;
    assert_eq!(rb["error"], "key not found");
    let rb = get_json(&format!("/dev/removebucket?key={}&migrateName=Pony&migrateOriginalname=Pony", urlencoding::encode("пони:пони"))).await;
    assert_eq!(rb["migrated"], 1);
    assert_eq!(rb["newKey"], "pony:pony");
    assert!(!fdb::master_db().contains_key("пони:пони"));
    assert_eq!(fdb::open_read("pony:pony", false, false).len(), 1);
    let rb = get_json("/dev/removebucket?key=pony:pony").await;
    assert_eq!(rb["removed"], 1);
    assert!(rb.get("newKey").is_none());

    let upd = get_json("/dev/updatesize").await;
    assert_eq!(upd["ok"], true);
    let rn = get_json("/dev/removenullvalues").await;
    assert_eq!(rn["removed"], 0);
}
