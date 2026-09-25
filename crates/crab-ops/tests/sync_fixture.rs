use crab_core::config::AppOptions;
use crab_ops::sync::cron::{filter_incoming, RootIn};

mod common;

#[test]
fn remote_page_fixture_parses_and_filters() {
    let root: RootIn = serde_json::from_str(&common::fixture("sync_torrents_page.json")).unwrap();
    assert!(root.nextread);
    assert_eq!(root.countread, 4);
    let cols = root.collections.as_ref().unwrap();
    assert_eq!(cols.len(), 2);
    assert_eq!(cols[1].Value.as_ref().unwrap().fileTime, 133591752000000000);
    let kz = &cols[0].Value.as_ref().unwrap().torrents.as_ref().unwrap()["https://kinozal.tv/details.php?id=1"];
    assert_eq!(kz.sid, 5);
    assert_eq!(kz.pir, 0);

    // defaults: every tracker, sport allowed
    let c = AppOptions::default();
    let (rows, t, s) = filter_incoming(&root, &c);
    assert_eq!((rows.len(), t, s), (4, 0, 0));

    // synctrackers keeps rows without trackerName (slim spidr rows)
    let mut c = AppOptions::default();
    c.synctrackers = Some(vec!["rutor".into()]);
    c.syncsport = false;
    let (rows, t, s) = filter_incoming(&root, &c);
    assert_eq!((t, s), (1, 1));
    let urls: Vec<&str> = rows.iter().map(|r| r.url.as_str()).collect();
    assert_eq!(urls, vec!["http://rutor.info/torrent/100", "http://rutor.info/torrent/201"]);
}
