//! Native `/api/v1.0/torrents` and `/api/v1.0/qualitys` queries over masterDb.

use chrono::{DateTime, Utc};
use indexmap::{IndexMap, IndexSet};
use serde::Serialize;
use serde_json::Value;

use crab_core::models::TorrentDetails;
use crab_core::util::{is_blank, search_name, search_name_or_empty};
use crab_core::{conf, fdb, hooks, time};

use super::result_builder::{opt, tracker_allowed, TorrentMap};
use crate::alloha;
use crate::indexers::tracker_matching;

/// Parameters of `/api/v1.0/torrents`.
#[derive(Clone, Debug, Default)]
pub struct TorrentsQuery {
    pub search: Option<String>,
    pub altname: Option<String>,
    pub exact: bool,
    pub kind: Option<String>,
    pub sort: Option<String>,
    pub tracker: Option<String>,
    pub voice: Option<String>,
    pub videotype: Option<String>,
    pub relased: i64,
    pub quality: i64,
    pub season: i64,
}

/// One row of the native torrents API.
#[derive(Clone, Debug, Serialize)]
pub struct TorrentRow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub size: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sizeName: Option<String>,
    #[serde(with = "time::net")]
    pub createTime: DateTime<Utc>,
    #[serde(with = "time::net")]
    pub updateTime: DateTime<Utc>,
    pub sid: i32,
    pub pir: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magnet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub originalname: Option<String>,
    pub relased: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videotype: Option<String>,
    pub quality: i32,
    pub voices: IndexSet<String>,
    pub seasons: IndexSet<i32>,
    pub types: Vec<String>,
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

fn add_row(torrents: &mut TorrentMap, t: TorrentDetails) {
    if !tracker_allowed(&t.trackerName) {
        return;
    }
    let replace = match torrents.get(&t.url) {
        None => true,
        Some(v) => t.updateTime > v.updateTime,
    };
    if replace {
        torrents.insert(t.url.clone(), t);
    }
}

fn master_keys(pred: impl Fn(&str) -> bool) -> Vec<String> {
    fdb::MASTER_DB.iter().filter(|e| pred(e.key())).map(|e| e.key().clone()).collect()
}

pub async fn query_torrents(p: TorrentsQuery) -> Vec<TorrentRow> {
    let mut search = p.search.clone();
    let mut altname = p.altname.clone();
    let mut kind = p.kind.clone();
    let mut exact = p.exact;
    let mut alloha_year = 0;
    let mut alloha_alt_title: Option<String> = None;

    if search.as_deref().map(alloha::is_resolvable_id).unwrap_or(false) {
        let resolved = alloha::resolve(search.as_deref(), altname.as_deref()).await;
        search = resolved.Search.clone();
        altname = resolved.AltName.clone();
        alloha_year = resolved.Year;
        alloha_alt_title = resolved.AlternativeName.clone();
        if kind.as_deref().map(is_blank).unwrap_or(true) && resolved.Type.as_deref().map(|t| !is_blank(t)).unwrap_or(false) {
            kind = resolved.Type.clone();
        }
        exact = true;
    }

    let Some(search_text) = search.as_deref().filter(|s| !is_blank(s) && s.chars().count() != 1) else {
        return Vec::new();
    };

    let s = search_name(search_text);
    let alt = altname.as_deref().and_then(search_name);
    let alt2 = alloha_alt_title.as_deref().and_then(search_name);
    if s.is_none() && alt.is_none() && alt2.is_none() {
        return Vec::new();
    }

    let kind_ok = |t: &TorrentDetails| kind.as_deref().map(|k| is_blank(k) || t.has_type(k)).unwrap_or(true);
    let mut torrents = TorrentMap::new();

    if exact {
        let keys = master_keys(|k| {
            s.as_deref().map(|s| k.starts_with(&format!("{s}:")) || k.ends_with(&format!(":{s}"))).unwrap_or(false)
                || alt.as_deref().map(|a| k.contains(a)).unwrap_or(false)
                || alt2.as_deref().map(|a| k.contains(a)).unwrap_or(false)
        });
        for key in keys {
            for t in fdb::open_read(&key, true, true).into_values() {
                if t.types.is_empty() || !kind_ok(&t) {
                    continue;
                }
                let n = row_sn(&t);
                let o = row_so(&t);
                if n == s
                    || o == s
                    || (alt.is_some() && (n == alt || o == alt))
                    || (alt2.is_some() && (n == alt2 || o == alt2))
                {
                    add_row(&mut torrents, t);
                }
            }
        }
    } else {
        let c = conf();
        let mut keys = master_keys(|k| {
            s.as_deref().map(|s| k.contains(s)).unwrap_or(false) || alt.as_deref().map(|a| k.contains(a)).unwrap_or(false)
        });
        if !c.evercache.enable || c.evercache.validHour > 0 {
            keys.truncate(c.maxreadfile.max(0) as usize);
        }
        for key in keys {
            for t in fdb::open_read(&key, true, true).into_values() {
                if !t.types.is_empty() && kind_ok(&t) {
                    add_row(&mut torrents, t);
                }
            }
        }
    }

    if torrents.is_empty() {
        return Vec::new();
    }

    let mut rows: Vec<TorrentDetails> = torrents.into_values().collect();
    match p.sort.as_deref().unwrap_or("") {
        "sid" => rows.sort_by(|a, b| b.sid.cmp(&a.sid)),
        "pir" => rows.sort_by(|a, b| b.pir.cmp(&a.pir)),
        "size" => rows.sort_by(|a, b| b.size.partial_cmp(&a.size).unwrap_or(std::cmp::Ordering::Equal)),
        "create" => rows.sort_by(|a, b| b.createTime.cmp(&a.createTime)),
        "update" => rows.sort_by(|a, b| b.updateTime.cmp(&a.updateTime)),
        _ => {}
    }

    if let Some(tr) = p.tracker.as_deref().filter(|t| !is_blank(t)) {
        let allowed = tracker_matching::to_allow_set(&tracker_matching::parse_list(tr));
        if !allowed.is_empty() {
            rows.retain(|i| tracker_matching::matches(Some(&i.trackerName), &allowed));
        }
    }

    let c = conf();
    if p.relased > 0 {
        rows.retain(|i| i.relased as i64 == p.relased);
    } else if alloha_year > 0 && c.alloha.filterByYear {
        let y = alloha_year;
        rows.retain(|i| i.relased <= 0 || i.relased == y || i.relased == y - 1 || i.relased == y + 1);
    }
    if p.quality > 0 {
        rows.retain(|i| i.quality as i64 == p.quality);
    }
    if let Some(v) = p.videotype.as_deref().filter(|v| !is_blank(v)) {
        rows.retain(|i| i.videotype == v);
    }
    if let Some(v) = p.voice.as_deref().filter(|v| !is_blank(v)) {
        rows.retain(|i| i.voices.contains(v));
    }
    if p.season > 0 {
        let s = p.season as i32;
        rows.retain(|i| i.seasons.contains(&s));
    }

    rows.into_iter()
        .take(2000)
        .map(|i| TorrentRow {
            tracker: opt(&i.trackerName),
            url: if i.url.starts_with("http") { Some(i.url.clone()) } else { None },
            title: opt(&i.title),
            size: i.size,
            sizeName: opt(&i.sizeName),
            createTime: i.createTime,
            updateTime: i.updateTime,
            sid: i.sid,
            pir: i.pir,
            magnet: opt(&i.magnet),
            name: opt(&i.name),
            originalname: opt(&i.originalname),
            relased: i.relased,
            videotype: opt(&i.videotype),
            quality: i.quality,
            voices: i.voices,
            seasons: i.seasons,
            types: i.types,
        })
        .collect()
}

/// Per-release quality/language summary for `/api/v1.0/qualitys`.
#[derive(Clone, Debug, Serialize)]
pub struct TorrentQuality {
    pub qualitys: IndexSet<i32>,
    pub types: IndexSet<String>,
    pub languages: IndexSet<String>,
    #[serde(with = "time::net")]
    pub createTime: DateTime<Utc>,
    #[serde(with = "time::net")]
    pub updateTime: DateTime<Utc>,
}

pub type QualityMap = IndexMap<String, IndexMap<i32, TorrentQuality>>;

pub fn query_qualitys(name: Option<&str>, originalname: Option<&str>, kind: Option<&str>, page: i32, take: i32) -> Value {
    serde_json::to_value(query_qualitys_map(name, originalname, kind, page, take)).unwrap_or(Value::Null)
}

pub fn query_qualitys_map(name: Option<&str>, originalname: Option<&str>, kind: Option<&str>, page: i32, take: i32) -> QualityMap {
    let s = name.and_then(search_name).unwrap_or_default();
    let so = originalname.and_then(search_name).unwrap_or_default();
    let mut torrents: QualityMap = IndexMap::new();
    if s.is_empty() && so.is_empty() {
        return torrents;
    }

    let tracks = conf().tracks;
    let mut add = |t: &TorrentDetails| {
        if t.types.is_empty() || t.has_type("sport") || t.relased == 0 {
            return;
        }
        if let Some(k) = kind.filter(|k| !k.is_empty()) {
            if !t.has_type(k) {
                return;
            }
        }

        let key = format!("{}:{}", search_name_or_empty(&t.name), search_name_or_empty(&t.originalname));
        let langs = if t.ffprobe.is_some() || !tracks {
            hooks::tracks_languages(t, t.ffprobe.as_deref())
        } else {
            let streams = hooks::tracks_get(&t.magnet, &t.types);
            hooks::tracks_languages(t, streams.as_deref().or(t.ffprobe.as_deref()))
        };

        let entry = torrents.entry(key).or_default();
        match entry.get_mut(&t.relased) {
            Some(md) => {
                if let Some(l) = &langs {
                    md.languages.extend(l.iter().cloned());
                }
                md.types.extend(t.types.iter().cloned());
                md.qualitys.insert(t.quality);
                if md.createTime > t.createTime {
                    md.createTime = t.createTime;
                }
                if t.updateTime > md.updateTime {
                    md.updateTime = t.updateTime;
                }
            }
            None => {
                entry.insert(
                    t.relased,
                    TorrentQuality {
                        qualitys: IndexSet::from([t.quality]),
                        types: t.types.iter().cloned().collect(),
                        languages: langs.unwrap_or_default(),
                        createTime: t.createTime,
                        updateTime: t.updateTime,
                    },
                );
            }
        }
    };

    let mut mdb: Vec<(String, DateTime<Utc>)> = fdb::MASTER_DB
        .iter()
        .filter(|e| {
            let k = e.key();
            match (s.is_empty(), so.is_empty()) {
                (false, false) => k.contains(&s) || k.contains(&so),
                (false, true) => k.contains(&s),
                _ => k.contains(&so),
            }
        })
        .map(|e| (e.key().clone(), e.value().updateTime))
        .collect();
    mdb.sort_by(|a, b| b.1.cmp(&a.1));
    let c = conf();
    if !c.evercache.enable || c.evercache.validHour > 0 {
        mdb.truncate(c.maxreadfile.max(0) as usize);
    }

    for (key, _) in &mdb {
        for t in fdb::open_read(key, true, true).values() {
            add(t);
        }
    }

    if take == -1 {
        return torrents;
    }

    let mut ordered: Vec<(String, IndexMap<i32, TorrentQuality>)> = torrents.into_iter().collect();
    let max_update = |v: &IndexMap<i32, TorrentQuality>| v.values().map(|x| x.updateTime).max().unwrap_or_else(time::min);
    ordered.sort_by(|a, b| max_update(&b.1).cmp(&max_update(&a.1)));

    let skip = ((page as i64 - 1) * take as i64).max(0) as usize;
    let take = take.max(0) as usize;
    ordered.into_iter().skip(skip).take(take).collect()
}
