//! Merges result batches by infohash (fallback: md5 of title|magnet) and sorts by seeders/peers.

use crab_core::models::api::{Result, TorrentInfo};
use crab_core::util::md5;
use indexmap::{IndexMap, IndexSet};

use crate::magnet::MagnetLink;

pub fn merge_and_sort(batches: Vec<Vec<Result>>) -> Vec<Result> {
    let mut map: IndexMap<String, Result> = IndexMap::new();

    for batch in batches {
        for item in batch {
            let key = info_hash_key(&item).unwrap_or_else(|| {
                format!(
                    "md5:{}",
                    md5(&format!("{}|{}", item.Title.as_deref().unwrap_or(""), item.MagnetUri.as_deref().unwrap_or("")))
                )
            });

            let Some(existing) = map.get_mut(&key) else {
                map.insert(key, item);
                continue;
            };

            if item.Seeders > existing.Seeders {
                existing.Seeders = item.Seeders;
                existing.Peers = existing.Peers.max(item.Peers);
            }

            if let Some(voices) = item.info.as_ref().and_then(|i| i.voices.as_ref()) {
                let info = existing.info.get_or_insert_with(TorrentInfo::default);
                let ev = info.voices.get_or_insert_with(IndexSet::new);
                for v in voices {
                    ev.insert(v.clone());
                }
            }

            if existing.ffprobe.is_none() && item.ffprobe.is_some() {
                existing.ffprobe = item.ffprobe.clone();
            }

            if existing.languages.as_ref().map(|l| l.is_empty()).unwrap_or(true)
                && item.languages.as_ref().map(|l| !l.is_empty()).unwrap_or(false)
            {
                existing.languages = item.languages.clone();
            }

            if existing.info.is_none() && item.info.is_some() {
                existing.info = item.info;
            }
        }
    }

    let mut list: Vec<Result> = map.into_values().collect();
    list.sort_by(|a, b| b.Seeders.cmp(&a.Seeders).then(b.Peers.cmp(&a.Peers)));
    list
}

fn info_hash_key(item: &Result) -> Option<String> {
    let magnet = item.MagnetUri.as_deref()?;
    if magnet.trim().is_empty() {
        return None;
    }
    MagnetLink::parse(magnet).map(|m| m.v1_or_v2_hex())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexers::filters::empty_result;

    #[test]
    fn merges_by_infohash_and_sorts() {
        let h = "0123456789abcdef0123456789abcdef01234567";
        let a = Result {
            Title: Some("A".into()),
            MagnetUri: Some(format!("magnet:?xt=urn:btih:{h}")),
            Seeders: 1,
            Peers: 5,
            info: Some(TorrentInfo { voices: Some(IndexSet::from(["x".to_string()])), ..Default::default() }),
            ..empty_result()
        };
        let b = Result {
            Title: Some("B".into()),
            MagnetUri: Some(format!("magnet:?xt=urn:btih:{}", h.to_uppercase())),
            Seeders: 10,
            Peers: 2,
            info: Some(TorrentInfo { voices: Some(IndexSet::from(["y".to_string()])), ..Default::default() }),
            ..empty_result()
        };
        let c = Result { Title: Some("C".into()), Seeders: 3, ..empty_result() };
        let out = merge_and_sort(vec![vec![a], vec![b, c]]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].Title.as_deref(), Some("A"));
        assert_eq!(out[0].Seeders, 10);
        assert_eq!(out[0].Peers, 5);
        assert_eq!(out[0].info.as_ref().unwrap().voices.as_ref().unwrap().len(), 2);
        assert_eq!(out[1].Title.as_deref(), Some("C"));
    }
}
