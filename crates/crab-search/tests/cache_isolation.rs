use crab_core::config::{self, AppOptions};
use crab_core::fdb;
use crab_core::models::TorrentDetails;
use crab_search::search::jackett_service::search_results;

#[test]
fn cached_results_preserve_language_filters_client_modes_and_field_boundaries() {
    let original_dir = std::env::current_dir().unwrap();
    let dir = std::env::temp_dir().join(format!("crab-cache-isolation-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let mut options = AppOptions::default();
    options.evercache.enable = true;
    options.evercache.validHour = 0;
    options.logFdb = false;
    config::set_current(options);

    for (title, language, infohash) in [
        (
            "Cache Isolation Russian",
            "rus",
            "0123456789abcdef0123456789abcdef01234567",
        ),
        (
            "Cache Isolation English",
            "eng",
            "1123456789abcdef0123456789abcdef01234567",
        ),
    ] {
        let mut torrent = TorrentDetails::new(
            "rutor",
            &["movie"],
            format!("https://example.test/{language}"),
            title,
        );
        torrent.name = "Cache Isolation".into();
        torrent.originalname = "Cache Isolation".into();
        torrent.magnet = format!("magnet:?xt=urn:btih:{infohash}");
        torrent.languages.insert(language.into());
        let guard = fdb::open_write(&fdb::key_for_torrent(&torrent.name, &torrent.originalname));
        guard.modify(|rows| {
            rows.insert(torrent.url.clone(), torrent.clone());
            true
        });
        fdb::set_shard(guard.key(), torrent.updateTime);
    }

    let search = |apikey, rqnum| {
        search_results(
            apikey,
            None,
            Some("Cache Isolation"),
            None,
            0,
            None,
            0,
            rqnum,
        )
    };
    let ordinary = search(None, false);
    assert_eq!(ordinary.len(), 2);
    assert!(ordinary.iter().all(|row| row.info.is_some()));
    let russian = search(Some("rus"), false);
    assert_eq!(russian.len(), 1);
    assert!(russian[0].Title.as_deref().unwrap().contains("Russian"));
    let num = search(None, true);
    assert_eq!(num.len(), 2);
    assert!(num.iter().all(|row| row.info.is_none()));
    let ordinary_again = search(None, false);
    assert_eq!(ordinary_again.len(), 2);
    assert!(ordinary_again.iter().all(|row| row.info.is_some()));
    assert_eq!(search(Some("rus"), true).len(), 1);

    let reverse_search = |apikey| {
        search_results(
            apikey,
            Some("reverse"),
            Some("Cache Isolation"),
            None,
            0,
            None,
            0,
            false,
        )
    };
    assert_eq!(reverse_search(Some("rus")).len(), 1);
    assert_eq!(reverse_search(None).len(), 2);

    let matching = search_results(
        None,
        Some("X:Y"),
        Some("Cache Isolation"),
        None,
        0,
        None,
        0,
        false,
    );
    assert_eq!(matching.len(), 2);
    let missing = search_results(
        None,
        Some("X"),
        Some("Y:Cache Isolation"),
        None,
        0,
        None,
        0,
        false,
    );
    assert!(
        missing.is_empty(),
        "delimiters in search fields must not cause cache collisions"
    );

    std::env::set_current_dir(original_dir).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
