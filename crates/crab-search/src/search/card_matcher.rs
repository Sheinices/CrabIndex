//! Jackett-style FileDB lookup: exact card match (title / title_original / year / kind)
//! or fuzzy free-text match through the FastDb token index.

use indexmap::{IndexMap, IndexSet};
use once_cell::sync::Lazy;
use std::sync::Arc;
use std::time::Duration;

use crab_core::models::TorrentDetails;
use crab_core::util::{is_blank, search_name};
use crab_core::{conf, fdb, index, rx};

use super::result_builder::{add_torrent, TorrentMap};
use crate::cache::MemCache;
use crate::indexers::{filters, num_query};

static TORRENTS_SEARCH_KEYS: Lazy<MemCache<Arc<IndexSet<String>>>> = Lazy::new(MemCache::new);

fn nb(s: Option<&str>) -> bool {
    s.map(|x| !is_blank(x)).unwrap_or(false)
}

fn has(t: &TorrentDetails, kinds: &[&str]) -> bool {
    t.types.iter().any(|x| kinds.contains(&x.as_str()))
}

fn row_sn(t: &TorrentDetails) -> Option<String> {
    if t._sn.is_empty() {
        search_name(&t.name)
    } else {
        Some(t._sn.clone())
    }
}

fn row_so(t: &TorrentDetails) -> Option<String> {
    if t._so.is_empty() {
        search_name(&t.originalname)
    } else {
        Some(t._so.clone())
    }
}

fn read_limit_applies() -> bool {
    let c = conf();
    !c.evercache.enable || c.evercache.validHour > 0
}

/// Rows of a shard that are eligible for Jackett output.
fn shard_rows(key: &str) -> Vec<TorrentDetails> {
    fdb::open_read(key, true, true)
        .into_values()
        .filter(|t| !t.types.is_empty() && !t.title.contains(" КПК"))
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn search(
    query: Option<&str>,
    title: Option<&str>,
    title_original: Option<&str>,
    year: i32,
    category: Option<&IndexMap<String, String>>,
    is_serial: i32,
    rqnum: bool,
) -> TorrentMap {
    let fastdb = index::get(false);
    let mut torrents = TorrentMap::new();

    let mut title = title.map(str::to_string);
    let mut title_original = title_original.map(str::to_string);
    let mut year = year;
    let mut is_serial = is_serial;

    // NUM / Lampa free-text: "Ru En Year", "Ru En" or "Ru Year" → card fields
    if rqnum && nb(query) && !nb(title.as_deref()) && !nb(title_original.as_deref()) {
        let parsed = num_query::parse(query);
        if parsed.matched {
            if nb(parsed.title.as_deref()) {
                title = parsed.title.clone();
            }
            if nb(parsed.title_original.as_deref()) {
                title_original = parsed.title_original.clone();
            }
            if parsed.year > 0 && year <= 0 {
                year = parsed.year;
            }
        }
    }

    if is_serial == 0 {
        if let Some(cat) = category.and_then(|c| c.values().next()) {
            if cat.contains("5020") || cat.contains("2010") {
                is_serial = 3;
            } else if cat.contains("5080") {
                is_serial = 4;
            } else if cat.contains("5070") {
                is_serial = 5;
            } else if cat.starts_with("20") {
                is_serial = 1;
            } else if cat.starts_with("50") {
                is_serial = 2;
            }
        }
    }

    if nb(title.as_deref()) || nb(title_original.as_deref()) {
        // exact search
        let n = title.as_deref().and_then(search_name);
        let o = title_original.as_deref().and_then(search_name);

        let mut keys: IndexSet<String> = IndexSet::new();
        for k in [&n, &o].into_iter().flatten() {
            if let Some(list) = fastdb.get(k) {
                keys.extend(list.iter().cloned());
            }
        }

        let maxread = conf().maxreadfile.max(0) as usize;
        if read_limit_applies() && keys.len() > maxread {
            keys = keys.into_iter().take(maxread).collect();
        }

        for key in &keys {
            for t in shard_rows(key) {
                let name = row_sn(&t);
                let originalname = row_so(&t);
                let hit = (n.is_some() && n == name) || (o.is_some() && o == originalname);
                if !hit {
                    continue;
                }
                match is_serial {
                    1 => {
                        if has(&t, &["movie", "multfilm", "anime", "documovie"]) {
                            if rx::is_match_i(&t.title, " (сезон|сери(и|я|й))") {
                                continue;
                            }
                            if filters::matches_card_year(t.relased, year, true) {
                                add_torrent(&mut torrents, &t);
                            }
                        }
                    }
                    2 => {
                        if has(&t, &["serial", "multserial", "anime", "docuserial", "tvshow"])
                            && filters::matches_card_year(t.relased, year, false)
                        {
                            add_torrent(&mut torrents, &t);
                        }
                    }
                    3 => {
                        if has(&t, &["tvshow"]) && filters::matches_card_year(t.relased, year, false) {
                            add_torrent(&mut torrents, &t);
                        }
                    }
                    4 => {
                        if has(&t, &["docuserial", "documovie"]) && filters::matches_card_year(t.relased, year, false) {
                            add_torrent(&mut torrents, &t);
                        }
                    }
                    5 => {
                        if has(&t, &["anime"]) && filters::matches_card_year(t.relased, year, false) {
                            add_torrent(&mut torrents, &t);
                        }
                    }
                    _ => {
                        let movie_like = has(&t, &["movie", "multfilm", "documovie"]);
                        if filters::matches_card_year(t.relased, year, movie_like) {
                            add_torrent(&mut torrents, &t);
                        }
                    }
                }
            }
        }
    } else if let Some(q) = query.filter(|q| !is_blank(q) && q.chars().count() > 1) {
        // plain search
        let s = search_name(q);

        let torrents_search = |torrents: &mut TorrentMap, exact: bool, exactdb: bool| {
            let Some(s) = s.as_deref() else {
                return;
            };

            let keys: Option<Arc<IndexSet<String>>> = if exactdb {
                fastdb.get(s).filter(|l| !l.is_empty()).map(|l| Arc::new(l.iter().cloned().collect()))
            } else {
                let mkey = format!("api:torrentsSearch:{s}");
                match TORRENTS_SEARCH_KEYS.get(&mkey) {
                    Some(k) => Some(k),
                    None => {
                        let limit = read_limit_applies();
                        let maxread = conf().maxreadfile.max(0) as usize;
                        let mut keys: IndexSet<String> = IndexSet::new();
                        for (k, list) in fastdb.iter() {
                            if !k.contains(s) {
                                continue;
                            }
                            keys.extend(list.iter().cloned());
                            if limit && keys.len() > maxread {
                                break;
                            }
                        }
                        let keys = Arc::new(keys);
                        TORRENTS_SEARCH_KEYS.set(mkey, keys.clone(), Duration::from_secs(3600));
                        Some(keys)
                    }
                }
            };

            let Some(keys) = keys else {
                return;
            };
            for key in keys.iter() {
                for t in shard_rows(key) {
                    if exact && row_sn(&t).as_deref() != Some(s) && row_so(&t).as_deref() != Some(s) {
                        continue;
                    }
                    let ok = match is_serial {
                        1 => has(&t, &["movie", "multfilm", "anime", "documovie"]),
                        2 => has(&t, &["serial", "multserial", "anime", "docuserial", "tvshow"]),
                        3 => has(&t, &["tvshow"]),
                        4 => has(&t, &["docuserial", "documovie"]),
                        5 => has(&t, &["anime"]),
                        _ => true,
                    };
                    if ok {
                        add_torrent(torrents, &t);
                    }
                }
            }
        };

        if is_serial == -1 {
            torrents_search(&mut torrents, false, true);
            if torrents.is_empty() {
                torrents_search(&mut torrents, false, false);
            }
        } else {
            torrents_search(&mut torrents, true, true);
            if torrents.is_empty() {
                torrents_search(&mut torrents, false, false);
            }
        }
    }

    torrents
}
