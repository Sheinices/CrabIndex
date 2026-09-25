//! Seeds an isolated FileDB bucket + Alloha cache entries (no live HTTP), then searches
//! through the native API and the combined indexer path using tt/kp/tmdb ids.
//!
//! Everything runs inside one test function: the process working directory, config and
//! caches are global.

use std::time::Duration;

use chrono::Utc;
use crab_core::config::{self, AppOptions};
use crab_core::fdb;
use crab_core::models::TorrentDetails;
use crab_search::alloha::{self, AllohaResolveResult};
use crab_search::indexers::engine;
use crab_search::indexers::request::IndexerSearchRequest;
use crab_search::search::torrent_query::{self, TorrentsQuery};

const RU_NAME: &str = "Бойцовский клуб";
const EN_NAME: &str = "Fight Club";
const IMDB_ID: &str = "tt0137523";
const KP_ID: &str = "kp361";
const TMDB_ID: &str = "tmdb550";
const TEST_URL: &str = "https://example.test/alloha-fdb-search/fight-club";
const TEST_MAGNET: &str = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567";

fn resolved(year: i32) -> AllohaResolveResult {
    AllohaResolveResult {
        Search: Some(EN_NAME.into()),
        AltName: Some(RU_NAME.into()),
        Year: year,
        Type: Some("movie".into()),
        ImdbId: Some(IMDB_ID.into()),
        KpId: Some(KP_ID.into()),
        TmdbId: Some(TMDB_ID.into()),
        ..Default::default()
    }
}

fn seed_cache_with_resolve(id: &str) {
    alloha::cache_clear();
    let (canonical, _) = alloha::try_normalize_id(id).expect("resolvable id");
    let r = resolved(1999);
    for key in [canonical.as_str(), IMDB_ID, KP_ID, TMDB_ID] {
        alloha::cache_set(key, r.clone(), Duration::from_secs(3600));
    }
}

fn seed_file_db() -> String {
    let key = fdb::key_for_torrent(RU_NAME, EN_NAME);
    {
        let w = fdb::open_write(&key);
        let mut t = TorrentDetails::new("rutor", &["movie"], TEST_URL, format!("{RU_NAME} / {EN_NAME} (1999)"));
        t.name = RU_NAME.into();
        t.originalname = EN_NAME.into();
        t.magnet = TEST_MAGNET.into();
        t.sid = 10;
        t.pir = 1;
        t.sizeName = "1 GB".into();
        t.relased = 1999;
        t.createTime = Utc::now();
        t.updateTime = Utc::now();
        w.add_or_update(&t);
    }
    assert!(fdb::MASTER_DB.contains_key(&key), "bucket registered in masterDb");
    key
}

fn native_query(search: &str) -> TorrentsQuery {
    TorrentsQuery { search: Some(search.into()), sort: Some("sid".into()), ..Default::default() }
}

#[tokio::test]
async fn alloha_id_resolve_finds_seeded_torrent() {
    let dir = std::env::temp_dir().join(format!("crab-search-alloha-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Data/fdb")).unwrap();
    std::fs::create_dir_all(dir.join("Data/temp")).unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let mut opts = AppOptions::default();
    opts.alloha.enable = true;
    opts.alloha.filterByYear = true;
    opts.logFdb = false;
    config::set_current(opts);

    seed_file_db();

    // native query: id → titles → exact FileDB match
    for id in [IMDB_ID, KP_ID, TMDB_ID] {
        seed_cache_with_resolve(id);
        let rows = torrent_query::query_torrents(native_query(id)).await;
        assert!(
            rows.iter().any(|r| r.url.as_deref() == Some(TEST_URL) || r.magnet.as_deref() == Some(TEST_MAGNET)),
            "native {id}"
        );
        assert!(
            rows.iter().any(|r| r.originalname.as_deref() == Some(EN_NAME) || r.name.as_deref() == Some(RU_NAME)),
            "native names {id}"
        );
    }

    // combined indexer search in id mode
    for id in [IMDB_ID, KP_ID, "TT0137523", "KP361"] {
        seed_cache_with_resolve(id);
        let mut req = IndexerSearchRequest { query: Some(id.into()), card_mode: false, ..Default::default() };
        let results = engine::search_combined(&mut req).await;
        assert!(!results.is_empty(), "combined {id}");
        assert!(
            results.iter().any(|r| r.MagnetUri.as_deref().map(|m| m.eq_ignore_ascii_case(TEST_MAGNET)).unwrap_or(false)
                || r.info.as_ref().map(|i| i.originalname.as_deref() == Some(EN_NAME) || i.name.as_deref() == Some(RU_NAME)).unwrap_or(false)),
            "combined match {id}"
        );
    }

    // wrong year from Alloha filters the row out
    alloha::cache_clear();
    alloha::cache_set(IMDB_ID, resolved(2010), Duration::from_secs(3600));
    let rows = torrent_query::query_torrents(native_query(IMDB_ID)).await;
    assert!(rows.is_empty(), "wrong year must filter out");

    let _ = std::env::set_current_dir(std::env::temp_dir());
    let _ = std::fs::remove_dir_all(&dir);
}
