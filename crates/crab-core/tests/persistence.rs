use crab_core::config::{self, AppOptions};
use crab_core::fdb;
use crab_core::models::TorrentDetails;

#[test]
fn failed_writes_retry_and_clean_shards_are_not_rewritten() {
    let original_dir = std::env::current_dir().unwrap();
    let dir = std::env::temp_dir().join(format!("crab-persistence-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let mut options = AppOptions::default();
    options.evercache.enable = true;
    options.evercache.validHour = 0;
    options.logFdb = false;
    config::set_current(options);

    let key = fdb::key_for_torrent("Persistence Test", "Persistence Test");
    let torrent = TorrentDetails::new(
        "rutor",
        &["movie"],
        "https://example.test/persistence",
        "Persistence Test",
    );
    let guard = fdb::open_write(&key);
    guard.modify(|rows| {
        rows.insert(torrent.url.clone(), torrent);
        true
    });
    fdb::set_shard(&key, crab_core::time::now());
    let path = fdb::path_for_key(&key);
    std::fs::create_dir(&path).unwrap();
    guard.save_changes_if_needed();
    std::fs::remove_dir(&path).unwrap();
    guard.save_changes_if_needed();
    assert_eq!(fdb::read_shard(&path).unwrap().len(), 1);

    std::fs::remove_file(&path).unwrap();
    guard.save_changes_if_needed();
    assert!(
        !std::path::Path::new(&path).exists(),
        "a clean shard must not be rewritten"
    );
    guard.save_now();
    assert_eq!(fdb::read_shard(&path).unwrap().len(), 1);

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    guard.save_now();
    std::fs::remove_dir(&path).unwrap();
    guard.save_changes_if_needed();
    assert_eq!(
        fdb::read_shard(&path).unwrap().len(),
        1,
        "failed forced writes must remain dirty"
    );
    drop(guard);

    std::fs::create_dir("Data/masterDb.bz").unwrap();
    assert!(!fdb::save_changes_if_dirty());
    assert!(fdb::is_master_db_dirty());
    std::fs::remove_dir("Data/masterDb.bz").unwrap();
    assert!(fdb::save_changes_if_dirty());
    assert!(!fdb::is_master_db_dirty());
    let saved: std::collections::HashMap<String, crab_core::models::MasterDbShard> =
        fdb::read_gz_json("Data/masterDb.bz").unwrap();
    assert!(saved.contains_key(&key));
    assert!(!fdb::save_changes_if_dirty());

    std::env::set_current_dir(original_dir).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
