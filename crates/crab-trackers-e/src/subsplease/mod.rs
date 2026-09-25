//! SubsPlease sync - public JSON API, 1080p magnets only.
//!
//! * `parse`: `f=latest` pages.
//! * `parseshows`: schedule-prioritised catalog walk with `f=show` (includes Batches);
//!   checkpoint in `Data/temp/subsplease_shows.json`.

pub mod parser;

use std::collections::HashSet;
use std::time::{Duration, Instant};

use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use chrono::Utc;
use crab_core::conf;
use crab_core::fdb;
use crab_core::net::{self, Req};
use crab_core::parsing::parser_log as plog;
use crab_core::trackers::{self, ParseLock};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::common::{self, bool_text, kv, QueryMap};
use parser::SubsPleaseDetails;

pub const TRACKER_NAME: &str = parser::TRACKER_NAME;
const CHECKPOINT_PATH: &str = "Data/temp/subsplease_shows.json";
const DEFAULT_PAGES: i32 = 2;
const DEFAULT_SHOW_LIMIT: i32 = 50;
const MAX_PAGES: i32 = 50;
const MAX_SHOW_LIMIT: i32 = 200;

static PARSE_LOCK: ParseLock = ParseLock::new();
static SHOWS_LOCK: ParseLock = ParseLock::new();
static CHECKPOINT_FILE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ShowsCheckpoint {
    pub updatedAt: Option<String>,
    pub shows: Option<Vec<ShowCheckpointEntry>>,
    pub cursor: i32,
    pub schedulePrioritySlugs: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ShowCheckpointEntry {
    pub slug: Option<String>,
    pub sid: Option<String>,
    pub title: Option<String>,
    pub lastFetched: Option<String>,
    pub batchCount: i32,
    pub episodeCount: i32,
    pub lastInfoHashes1080: Option<Vec<String>>,
}

fn host() -> String {
    conf().SubsPlease.rq_host().trim_end_matches('/').to_string()
}

fn use_proxy() -> bool {
    conf().SubsPlease.useproxy
}

fn parse_delay_ms() -> u64 {
    conf().SubsPlease.parse_delay().max(0) as u64
}

async fn delay() {
    let ms = parse_delay_ms();
    if ms > 0 {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

fn now_o() -> String {
    common::iso_o(&Utc::now())
}

async fn get_json_text(url: &str) -> Option<String> {
    let req = Req::new().encoding(encoding_rs::UTF_8).header("Accept", "application/json").useproxy(use_proxy()).timeout(30);
    net::get(url, &req).await
}

async fn get_html(url: &str, timeout: u64) -> Option<String> {
    let req = Req::new().encoding(encoding_rs::UTF_8).useproxy(use_proxy()).timeout(timeout);
    net::get(url, &req).await
}

pub async fn parse(pages: i32) -> String {
    if host().is_empty() {
        return trackers::DISABLED_RESULT.to_string();
    }
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let sw = Instant::now();
        let page_cap = (if pages <= 0 { DEFAULT_PAGES } else { pages }).clamp(1, MAX_PAGES);
        let host = host();
        plog::write_kv(TRACKER_NAME, "Starting latest parse", &kv![("pages", page_cap), ("host", &host)]);

        let mut tot = Stats::default();
        for i in 0..page_cap {
            if i > 0 {
                delay().await;
            }
            let url = if i == 0 { format!("{host}/api/?f=latest&tz=UTC") } else { format!("{host}/api/?f=latest&tz=UTC&p={i}") };
            let Some(json) = get_json_text(&url).await.filter(|j| !j.trim().is_empty()) else {
                plog::write_kv(TRACKER_NAME, "latest empty response", &kv![("pageIndex", i)]);
                break;
            };
            if parser::is_limit_reached(&json) {
                plog::write_kv(TRACKER_NAME, "latest limit_reached", &kv![("pageIndex", i)]);
                break;
            }
            let torrents = parser::parse_latest_or_search_json(&json, &host);
            let st = upsert(&torrents);
            tot.add(&st);
            plog::write_kv(
                TRACKER_NAME,
                "latest page done",
                &kv![("pageIndex", i), ("parsed", st.parsed), ("added", st.added), ("updated", st.updated)],
            );
        }

        plog::write_kv(
            TRACKER_NAME,
            &format!("Parse completed successfully (took {}s)", common::secs(sw)),
            &kv![("parsed", tot.parsed), ("added", tot.added), ("updated", tot.updated), ("skipped", tot.skipped), ("failed", tot.failed)],
        );
        "ok".to_string()
    })
    .await
}

pub async fn parse_shows(limit: i32, reset: bool) -> String {
    if host().is_empty() {
        return trackers::DISABLED_RESULT.to_string();
    }
    let name = format!("{TRACKER_NAME}-shows");
    trackers::run_parse(&name, &SHOWS_LOCK, false, || async move {
        let sw = Instant::now();
        let take = (if limit <= 0 { DEFAULT_SHOW_LIMIT } else { limit }).clamp(1, MAX_SHOW_LIMIT);
        let host = host();
        let mut state = load_checkpoint(reset);

        plog::write_kv(
            TRACKER_NAME,
            "Starting ParseShows",
            &kv![
                ("limit", take),
                ("reset", bool_text(reset)),
                ("cursor", state.cursor),
                ("knownShows", state.shows.as_ref().map(|s| s.len()).unwrap_or(0)),
                ("host", &host)
            ],
        );

        // Schedule priority slugs
        let schedule_json = get_json_text(&format!("{host}/api/?f=schedule&tz=UTC")).await;
        let schedule_slugs = parser::parse_schedule_page_slugs(schedule_json.as_deref().unwrap_or(""));
        state.schedulePrioritySlugs = Some(schedule_slugs.clone());
        persist_checkpoint(&state);

        // Catalog index
        delay().await;
        let index_html = get_html(&format!("{host}/shows/"), 45).await;
        let catalog_slugs = parser::parse_show_slugs_from_index_html(index_html.as_deref().unwrap_or(""));

        // Merge: schedule first, then catalog order, unique
        let mut work_order: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for s in schedule_slugs.iter().chain(catalog_slugs.iter()) {
            if s.trim().is_empty() {
                continue;
            }
            if seen.insert(s.to_lowercase()) {
                work_order.push(s.clone());
            }
        }
        if work_order.is_empty() {
            plog::write(TRACKER_NAME, "ParseShows: empty show list");
            return "ok".to_string();
        }
        for slug in &work_order {
            ensure_show_entry(&mut state, slug);
        }

        let start = state.cursor.clamp(0, work_order.len() as i32) as usize;
        let mut tot = Stats::default();
        let mut processed = 0usize;

        let mut i = 0usize;
        while i < take as usize && start + i < work_order.len() {
            let slug = work_order[start + i].clone();
            i += 1;
            if processed > 0 {
                delay().await;
            }

            let mut sid = ensure_show_entry(&mut state, &slug).sid.clone().unwrap_or_default();
            if sid.trim().is_empty() {
                let show_html = get_html(&format!("{host}/shows/{slug}/"), 30).await;
                sid = parser::extract_show_sid_from_html(show_html.as_deref().unwrap_or("")).unwrap_or_default();
                if sid.trim().is_empty() {
                    plog::write_kv(TRACKER_NAME, "sid missing", &kv![("slug", &slug)]);
                    processed += 1;
                    continue;
                }
                ensure_show_entry(&mut state, &slug).sid = Some(sid.clone());
                persist_checkpoint(&state);
                delay().await;
            }

            let show_json = get_json_text(&format!("{host}/api/?f=show&tz=UTC&sid={}", urlencoding::encode(&sid))).await;
            let torrents = parser::parse_show_json(show_json.as_deref().unwrap_or(""), &host, &slug, &sid);
            let st = upsert(&torrents);
            tot.add(&st);

            let (batch_count, episode_count) = {
                let entry = ensure_show_entry(&mut state, &slug);
                if let Some(first) = torrents.first() {
                    entry.title = Some(first.t.name.clone());
                }
                entry.lastFetched = Some(now_o());
                entry.batchCount = torrents.iter().filter(|t| t.is_batch).count() as i32;
                entry.episodeCount = torrents.iter().filter(|t| !t.is_batch).count() as i32;
                let mut hs: Vec<String> = Vec::new();
                let mut seen_h = HashSet::new();
                for h in torrents.iter().filter_map(|t| t.info_hash.clone()).filter(|h| !h.trim().is_empty()) {
                    if seen_h.insert(h.to_lowercase()) {
                        hs.push(h);
                    }
                    if hs.len() >= 64 {
                        break;
                    }
                }
                entry.lastInfoHashes1080 = Some(hs);
                (entry.batchCount, entry.episodeCount)
            };

            processed += 1;
            state.cursor = (start + processed) as i32;
            state.updatedAt = Some(now_o());
            persist_checkpoint(&state);

            plog::write_kv(
                TRACKER_NAME,
                "show done",
                &kv![("slug", &slug), ("sid", &sid), ("parsed", st.parsed), ("batch", batch_count), ("episodes", episode_count)],
            );
        }

        if state.cursor as usize >= work_order.len() {
            state.cursor = 0;
        }
        state.updatedAt = Some(now_o());
        persist_checkpoint(&state);

        plog::write_kv(
            TRACKER_NAME,
            &format!("ParseShows completed (took {}s)", common::secs(sw)),
            &kv![
                ("processed", processed),
                ("cursor", state.cursor),
                ("catalog", work_order.len()),
                ("parsed", tot.parsed),
                ("added", tot.added),
                ("updated", tot.updated),
                ("skipped", tot.skipped),
                ("failed", tot.failed)
            ],
        );
        "ok".to_string()
    })
    .await
}

#[derive(Serialize)]
struct StatusSample {
    slug: Option<String>,
    sid: Option<String>,
    batchCount: i32,
    episodeCount: i32,
    lastFetched: Option<String>,
}

#[derive(Serialize)]
struct Status {
    ok: bool,
    updatedAt: Option<String>,
    cursor: i32,
    shows: usize,
    withSid: usize,
    schedulePriority: usize,
    sample: Option<Vec<StatusSample>>,
}

/// Checkpoint summary as indented JSON text.
pub fn parse_show_status() -> String {
    let state = load_checkpoint(false);
    let shows = state.shows.as_ref();
    let status = Status {
        ok: true,
        updatedAt: state.updatedAt.clone(),
        cursor: state.cursor,
        shows: shows.map(|s| s.len()).unwrap_or(0),
        withSid: shows.map(|s| s.iter().filter(|e| e.sid.as_deref().map(|x| !x.trim().is_empty()).unwrap_or(false)).count()).unwrap_or(0),
        schedulePriority: state.schedulePrioritySlugs.as_ref().map(|s| s.len()).unwrap_or(0),
        sample: shows.map(|s| {
            s.iter()
                .take(5)
                .map(|e| StatusSample {
                    slug: e.slug.clone(),
                    sid: e.sid.clone(),
                    batchCount: e.batchCount,
                    episodeCount: e.episodeCount,
                    lastFetched: e.lastFetched.clone(),
                })
                .collect()
        }),
    };
    serde_json::to_string_pretty(&status).unwrap_or_default()
}

#[derive(Default, Clone, Copy)]
struct Stats {
    parsed: i32,
    added: i32,
    updated: i32,
    skipped: i32,
    failed: i32,
}

impl Stats {
    fn add(&mut self, o: &Stats) {
        self.parsed += o.parsed;
        self.added += o.added;
        self.updated += o.updated;
        self.skipped += o.skipped;
        self.failed += o.failed;
    }
}

fn upsert(torrents: &[SubsPleaseDetails]) -> Stats {
    let mut st = Stats::default();
    if torrents.is_empty() {
        return st;
    }
    st.parsed = torrents.len() as i32;
    for (key, list) in common::group_by_bucket(torrents.iter().map(|d| d.t.clone()).collect()) {
        let w = fdb::open_write(&key);
        for t in list {
            if t.magnet.trim().is_empty() {
                st.failed += 1;
                plog::write_failed(TRACKER_NAME, &t, Some("empty magnet"));
                continue;
            }
            match common::cached_row(&w, &t.url) {
                Some(c) => {
                    let same_magnet = c.magnet.eq_ignore_ascii_case(&t.magnet);
                    let same_title = c.title.trim() == t.title.trim();
                    let same_size = c.sizeName == t.sizeName;
                    if same_magnet && same_title && same_size {
                        st.skipped += 1;
                        plog::write_skipped(TRACKER_NAME, &c, Some("no changes"));
                        continue;
                    }
                    st.updated += 1;
                    plog::write_updated(TRACKER_NAME, &t, Some("magnet/title/size refreshed"));
                }
                None => {
                    st.added += 1;
                    plog::write_added(TRACKER_NAME, &t);
                }
            }
            w.add_or_update(&t);
        }
    }
    st
}

fn ensure_show_entry<'a>(state: &'a mut ShowsCheckpoint, slug: &str) -> &'a mut ShowCheckpointEntry {
    let shows = state.shows.get_or_insert_with(Vec::new);
    let idx = match shows.iter().position(|s| s.slug.as_deref().map(|x| x.eq_ignore_ascii_case(slug)).unwrap_or(false)) {
        Some(i) => i,
        None => {
            shows.push(ShowCheckpointEntry { slug: Some(slug.to_string()), ..Default::default() });
            shows.len() - 1
        }
    };
    &mut shows[idx]
}

fn load_checkpoint(reset: bool) -> ShowsCheckpoint {
    let _g = CHECKPOINT_FILE_LOCK.lock();
    if reset || !std::path::Path::new(CHECKPOINT_PATH).exists() {
        let fresh = ShowsCheckpoint { updatedAt: Some(now_o()), shows: Some(Vec::new()), cursor: 0, schedulePrioritySlugs: Some(Vec::new()) };
        try_write_checkpoint(&fresh);
        return fresh;
    }
    let parsed = std::fs::read_to_string(CHECKPOINT_PATH)
        .ok()
        .and_then(|s| serde_json::from_str::<Option<ShowsCheckpoint>>(s.trim_start_matches('\u{feff}')).ok());
    match parsed {
        Some(Some(mut state)) => {
            state.shows.get_or_insert_with(Vec::new);
            state.schedulePrioritySlugs.get_or_insert_with(Vec::new);
            state
        }
        _ => ShowsCheckpoint { shows: Some(Vec::new()), ..Default::default() },
    }
}

fn persist_checkpoint(state: &ShowsCheckpoint) {
    let _g = CHECKPOINT_FILE_LOCK.lock();
    try_write_checkpoint(state);
}

fn try_write_checkpoint(state: &ShowsCheckpoint) {
    if let Some(dir) = std::path::Path::new(CHECKPOINT_PATH).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(CHECKPOINT_PATH, json);
    }
}

pub fn router() -> Router {
    Router::new()
        .route(
            "/cron/subsplease/parse",
            get(|Query(q): Query<QueryMap>| async move { parse(common::q_i32(&q, "pages", DEFAULT_PAGES)).await }),
        )
        .route(
            "/cron/subsplease/parseshows",
            get(|Query(q): Query<QueryMap>| async move {
                parse_shows(common::q_i32(&q, "limit", DEFAULT_SHOW_LIMIT), common::q_bool(&q, "reset", false)).await
            }),
        )
        .route("/cron/subsplease/parseshowstatus", get(|| async { parse_show_status() }))
}
