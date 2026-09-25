//! Turns matched FileDB rows into Jackett result rows: tracker allow/deny, duplicate
//! merge by infohash, ffprobe/language enrichment and category mapping.

use indexmap::{IndexMap, IndexSet};

use crab_core::models::api::{Result, TorrentInfo};
use crab_core::models::{FfStream, TorrentDetails};
use crab_core::{conf, hooks, util::is_blank};

use crate::magnet::{url_encode_form, MagnetLink};

/// Rows keyed by url.
pub type TorrentMap = IndexMap<String, TorrentDetails>;

pub(crate) fn opt(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// True when `synctrackers` / `disable_trackers` allow this tracker.
pub fn tracker_allowed(tracker_name: &str) -> bool {
    let c = conf();
    if let Some(sync) = c.synctrackers.as_ref() {
        if !sync.iter().any(|s| s == tracker_name) {
            return false;
        }
    }
    !c.disable_trackers.iter().any(|s| s == tracker_name)
}

/// Add a row unless its tracker is filtered out; keeps the most recently updated row per url.
pub fn add_torrent(torrents: &mut TorrentMap, t: &TorrentDetails) {
    if !tracker_allowed(&t.trackerName) {
        return;
    }
    match torrents.get(&t.url) {
        Some(val) => {
            if t.updateTime > val.updateTime {
                torrents.insert(t.url.clone(), t.clone());
            }
        }
        None => {
            torrents.insert(t.url.clone(), t.clone());
        }
    }
}

/// Newznab category ids + description for the row's types.
pub fn category_ids(t: &TorrentDetails) -> (IndexSet<i32>, Option<String>) {
    let mut desc: Option<&str> = None;
    let mut ids = IndexSet::new();
    for kind in &t.types {
        match kind.as_str() {
            "movie" => {
                desc = Some("Movies");
                ids.insert(2000);
            }
            "serial" => {
                desc = Some("TV");
                ids.insert(5000);
            }
            "documovie" | "docuserial" => {
                desc = Some("TV/Documentary");
                ids.insert(5080);
            }
            "tvshow" => {
                desc = Some("TV/Foreign");
                ids.insert(5020);
                ids.insert(2010);
            }
            "anime" => {
                desc = Some("TV/Anime");
                ids.insert(5070);
            }
            _ => {}
        }
    }
    (ids, desc.map(str::to_string))
}

struct MergeEntry {
    torrent: TorrentDetails,
    title: Option<String>,
    name: Option<String>,
    announce_urls: Vec<String>,
}

fn rebuild_magnet(hex: &str, e: &mut MergeEntry) {
    let mut magnet = format!("magnet:?xt=urn:btih:{}", hex.to_lowercase());
    if let Some(n) = e.name.as_deref().filter(|n| !is_blank(n)) {
        magnet.push_str(&format!("&dn={}", url_encode_form(n)));
    }
    let mut added: IndexSet<String> = IndexSet::new();
    for announce in &e.announce_urls {
        let tr = if announce.contains('/') || announce.contains(':') { url_encode_form(announce) } else { announce.clone() };
        if added.insert(tr.clone()) {
            magnet.push_str(&format!("&tr={tr}"));
        }
    }
    e.torrent.magnet = magnet;
}

fn rebuild_title(e: &mut MergeEntry) {
    let Some(title) = e.title.as_deref().filter(|t| !is_blank(t)) else {
        return;
    };
    let mut title = title.to_string();
    if !e.torrent.voices.is_empty() {
        title.push_str(&format!(" | {}", e.torrent.voices.iter().cloned().collect::<Vec<_>>().join(" | ")));
    }
    e.torrent.title = title;
}

/// Merge rows that share an infohash (when enabled for this client kind).
pub fn merge_duplicates(torrents: &TorrentMap, rqnum: bool) -> Vec<TorrentDetails> {
    let c = conf();
    if !((!rqnum && c.mergeduplicates) || (rqnum && c.mergenumduplicates)) {
        return torrents.values().cloned().collect();
    }

    let mut ordered: Vec<&TorrentDetails> = torrents.values().collect();
    ordered.sort_by(|a, b| {
        b.createTime.cmp(&a.createTime).then((a.trackerName == "selezen").cmp(&(b.trackerName == "selezen")))
    });

    let mut temp: IndexMap<String, MergeEntry> = IndexMap::new();
    for torrent in ordered {
        let Some(link) = MagnetLink::parse(&torrent.magnet) else {
            // malformed magnets must not break the whole merge batch
            continue;
        };
        let hex = link.v1_or_v2_hex();

        let Some(e) = temp.get_mut(&hex) else {
            temp.insert(
                hex,
                MergeEntry {
                    torrent: torrent.clone(),
                    title: if torrent.trackerName == "kinozal" { Some(torrent.title.clone()) } else { None },
                    name: link.name.clone(),
                    announce_urls: link.announce_urls.clone(),
                },
            );
            continue;
        };

        if !e.torrent.trackerName.contains(torrent.trackerName.as_str()) {
            e.torrent.trackerName = format!("{}, {}", e.torrent.trackerName, torrent.trackerName);
        }

        if e.name.as_deref().map(is_blank).unwrap_or(true) && link.name.as_deref().map(|n| !is_blank(n)).unwrap_or(false) {
            e.name = link.name.clone();
            rebuild_magnet(&hex, e);
        }

        if !link.announce_urls.is_empty() {
            e.announce_urls.extend(link.announce_urls.iter().cloned());
            rebuild_magnet(&hex, e);
        }

        if torrent.trackerName == "kinozal" {
            e.title = Some(torrent.title.clone());
            rebuild_title(e);
        }

        if !torrent.voices.is_empty() {
            for v in &torrent.voices {
                e.torrent.voices.insert(v.clone());
            }
            rebuild_title(e);
        }

        if torrent.trackerName != "selezen" {
            if torrent.sid > e.torrent.sid {
                e.torrent.sid = torrent.sid;
            }
            if torrent.pir > e.torrent.pir {
                e.torrent.pir = torrent.pir;
            }
        }

        if torrent.createTime > e.torrent.createTime {
            e.torrent.createTime = torrent.createTime;
        }

        for v in &torrent.languages {
            e.torrent.languages.insert(v.clone());
        }

        if e.torrent.ffprobe.is_none() && torrent.ffprobe.is_some() {
            e.torrent.ffprobe = torrent.ffprobe.clone();
        }
    }

    temp.into_values().map(|e| e.torrent).collect()
}

/// ffprobe streams + audio languages for a row (tracks DB when enabled).
pub fn ffprobe_and_languages(t: &TorrentDetails) -> (Option<Vec<FfStream>>, Option<IndexSet<String>>) {
    if t.ffprobe.is_some() || !conf().tracks {
        let langs = hooks::tracks_languages(t, t.ffprobe.as_deref());
        return (t.ffprobe.clone(), langs);
    }
    let streams = hooks::tracks_get(&t.magnet, &t.types);
    let langs = hooks::tracks_languages(t, streams.as_deref().or(t.ffprobe.as_deref()));
    (streams, langs)
}

pub fn build(torrents: &TorrentMap, apikey: Option<&str>, rqnum: bool) -> Vec<Result> {
    let mut rows = merge_duplicates(torrents, rqnum);

    if apikey == Some("rus") {
        rows.retain(|i| {
            i.languages.contains("rus") || i.types.iter().any(|t| t == "sport" || t == "tvshow" || t == "docuserial")
        });
    }

    let mut results = Vec::with_capacity(rows.len());
    for i in rows {
        let (ffprobe, languages) = if rqnum { (None, None) } else { ffprobe_and_languages(&i) };
        let (cats, desc) = category_ids(&i);
        results.push(Result {
            Tracker: opt(&i.trackerName),
            Details: if i.url.starts_with("http") { Some(i.url.clone()) } else { None },
            Title: opt(&i.title),
            Size: i.size,
            PublishDate: i.createTime,
            Category: Some(cats),
            CategoryDesc: desc,
            Seeders: i.sid,
            Peers: i.pir,
            MagnetUri: opt(&i.magnet),
            ffprobe,
            languages,
            info: if rqnum {
                None
            } else {
                Some(TorrentInfo {
                    name: opt(&i.name),
                    originalname: opt(&i.originalname),
                    sizeName: opt(&i.sizeName),
                    relased: i.relased,
                    videotype: opt(&i.videotype),
                    quality: i.quality,
                    voices: Some(i.voices.clone()),
                    seasons: if i.seasons.is_empty() { None } else { Some(i.seasons.clone()) },
                    types: Some(i.types.clone()),
                })
            },
        });
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn row(tracker: &str, url: &str, magnet: &str, sid: i32, age_min: i64) -> TorrentDetails {
        let mut t = TorrentDetails::new(tracker, &["movie"], url, format!("{tracker} title"));
        t.magnet = magnet.into();
        t.sid = sid;
        t.createTime = Utc::now() - Duration::minutes(age_min);
        t
    }

    #[test]
    fn merge_combines_trackers_magnets_and_voices() {
        crate::test_conf();
        let h = "0123456789abcdef0123456789abcdef01234567";
        let mut a = row("rutor", "http://a/1", &format!("magnet:?xt=urn:btih:{h}&tr=udp://t1:80"), 5, 1);
        a.voices.insert("LostFilm".into());
        let mut b = row("kinozal", "http://b/1", &format!("magnet:?xt=urn:btih:{h}&dn=Name+X&tr=udp://t2:80"), 9, 10);
        b.voices.insert("HDRezka".into());
        b.languages.insert("rus".into());
        let c = row("selezen", "http://c/1", "not a magnet", 100, 0);

        let mut map = TorrentMap::new();
        for t in [&a, &b, &c] {
            map.insert(t.url.clone(), t.clone());
        }
        let merged = merge_duplicates(&map, false);
        assert_eq!(merged.len(), 1);
        let m = &merged[0];
        assert_eq!(m.trackerName, "rutor, kinozal");
        assert_eq!(m.sid, 9);
        assert_eq!(m.magnet, format!("magnet:?xt=urn:btih:{h}&dn=Name+X&tr=udp%3a%2f%2ft1%3a80&tr=udp%3a%2f%2ft2%3a80"));
        assert_eq!(m.title, "kinozal title | LostFilm | HDRezka");
        assert!(m.languages.contains("rus"));
    }

    #[test]
    fn categories_for_types() {
        let mut t = TorrentDetails::new("x", &["tvshow", "anime"], "u", "t");
        let (ids, desc) = category_ids(&t);
        assert_eq!(ids.into_iter().collect::<Vec<_>>(), vec![5020, 2010, 5070]);
        assert_eq!(desc.as_deref(), Some("TV/Anime"));
        t.types.clear();
        assert!(category_ids(&t).0.is_empty());
    }
}
