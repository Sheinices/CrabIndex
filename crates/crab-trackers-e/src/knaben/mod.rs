//! Knaben API v1 sync - TV + Movies aggregated from TPB, 1337x, EZTV, Rutracker.
//!
//! * `parse`: latest pages (`from`, `size`, `pages`, `query`, `hours`, `orderBy`, `orderDirection`, `categories`).
//! * `backfill`: leaf subcategories asc → desc within the 10000 window; checkpoint in
//!   `Data/temp/knaben_backfill.json` (`reset=true` starts over).

pub mod backfill;
pub mod models;
pub mod parser;

use std::time::Instant;

use axum::extract::Query;
use axum::routing::any;
use axum::Router;
use crab_core::conf;
use crab_core::fdb;
use crab_core::models::TorrentDetails;
use crab_core::net::{self, PostBody, Req};
use crab_core::parsing::{bencode, parser_log as plog};
use crab_core::trackers::{self, Cancelled, ParseLock};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::common::{self, bool_text, kv, QueryMap};
use backfill::{KnabenFetchPage, KnabenPageOutcome, BACKFILL_CATEGORIES};
use models::{KnabenApiRequest, KnabenApiResponse, KnabenBackfillState};

pub const TRACKER_NAME: &str = parser::TRACKER_NAME;
const MIN_API_DELAY_MS: i32 = 500;
const MAX_SIZE: i32 = 300;
const MAX_PAGES: i32 = 10;
const MAX_FROM_WINDOW: i32 = 10000;
const BACKFILL_STATE_PATH: &str = "Data/temp/knaben_backfill.json";
const CANCELED_MESSAGE: &str = "The operation was canceled.";

const DEFAULT_CATEGORIES: [i32; 18] = [
    2000000, 2001000, 2002000, 2003000, 2004000, 2005000, 2006000, 2007000, 2008000, 3000000, 3001000, 3002000, 3003000, 3004000,
    3005000, 3006000, 3007000, 3008000,
];

static PARSE_LOCK: ParseLock = ParseLock::new();
static BACKFILL_LOCK: ParseLock = ParseLock::new();
static BACKFILL_STATE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// Failure while talking to the API: cancellation or a hard error (e.g. malformed JSON).
#[derive(Debug)]
enum KErr {
    Cancelled,
    Other(String),
}

impl From<Cancelled> for KErr {
    fn from(_: Cancelled) -> Self {
        KErr::Cancelled
    }
}

fn api_url() -> String {
    format!("{}/v1", conf().Knaben.host.trim_end_matches('/'))
}

fn api_delay_ms() -> u64 {
    MIN_API_DELAY_MS.max(conf().Knaben.parse_delay()) as u64
}

fn normalize_order_direction(d: &str) -> &'static str {
    if d.eq_ignore_ascii_case("asc") {
        "asc"
    } else {
        "desc"
    }
}

fn parse_categories(s: Option<&str>) -> Vec<i32> {
    let Some(s) = s.filter(|s| !s.trim().is_empty()) else { return DEFAULT_CATEGORIES.to_vec() };
    let parsed: Vec<i32> =
        s.split([',', ';', ' ']).filter(|p| !p.is_empty()).filter_map(|p| p.trim().parse::<i32>().ok()).collect();
    if parsed.is_empty() {
        DEFAULT_CATEGORIES.to_vec()
    } else {
        parsed
    }
}

#[derive(Clone, Debug)]
pub struct ParseArgs {
    pub from: i32,
    pub size: i32,
    pub pages: i32,
    pub query: Option<String>,
    pub hours: i32,
    pub order_by: String,
    pub order_direction: String,
    pub categories: Option<String>,
}

impl Default for ParseArgs {
    fn default() -> Self {
        ParseArgs {
            from: 0,
            size: 300,
            pages: 1,
            query: None,
            hours: 0,
            order_by: "date".into(),
            order_direction: "desc".into(),
            categories: None,
        }
    }
}

pub async fn parse(args: ParseArgs) -> String {
    trackers::run_parse(TRACKER_NAME, &PARSE_LOCK, false, || async move {
        let s = args.size.clamp(1, MAX_SIZE);
        let p = args.pages.clamp(1, MAX_PAGES);
        let cats = parse_categories(args.categories.as_deref());
        let dir = normalize_order_direction(&args.order_direction);
        let query = args.query.as_deref().map(|q| q.trim().to_string());
        parse_core(args.from, s, p, query, args.hours, &args.order_by, dir, cats, &trackers::app_stopping()).await
    })
    .await
}

pub async fn backfill(size: i32, pages: i32, reset: bool) -> String {
    let name = format!("{TRACKER_NAME}-backfill");
    trackers::run_parse(&name, &BACKFILL_LOCK, false, || async move {
        let s = size.clamp(1, MAX_SIZE);
        let p = pages.clamp(1, MAX_PAGES);
        backfill_core(s, p, reset, &trackers::app_stopping()).await
    })
    .await
}

pub fn backfill_status() -> String {
    backfill::format_backfill_status(&load_backfill_state(false))
}

#[allow(clippy::too_many_arguments)]
async fn parse_core(
    from: i32,
    size: i32,
    pages: i32,
    query: Option<String>,
    hours: i32,
    order_by: &str,
    order_direction: &str,
    categories: Vec<i32>,
    ct: &CancellationToken,
) -> String {
    let sw = Instant::now();
    let mut total_fetched = 0;

    if from >= MAX_FROM_WINDOW {
        plog::write_kv(TRACKER_NAME, "from exceeds Knaben window", &kv![("from", from), ("max", MAX_FROM_WINDOW)]);
        return format!("error: from must be < {MAX_FROM_WINDOW} (Knaben from+size ≤ {MAX_FROM_WINDOW})");
    }

    let mut opts = kv![("from", from), ("size", size), ("pages", pages), ("orderDirection", order_direction)].to_vec();
    if let Some(q) = query.as_deref().filter(|q| !q.is_empty()) {
        opts.push(("query".into(), q.to_string()));
    }
    if hours > 0 {
        opts.push(("hours".into(), hours.to_string()));
    }
    opts.push(("orderBy".into(), order_by.to_string()));
    plog::write_kv(TRACKER_NAME, "Starting parse", &opts);

    let res: Result<(i32, i32, i32, i32), KErr> = async {
        let mut all: Vec<TorrentDetails> = Vec::new();
        let seconds_since = if hours > 0 { Some(hours * 3600) } else { None };
        for page in 0..pages {
            trackers::check(ct)?;
            let offset = from + page * size;
            if offset >= MAX_FROM_WINDOW {
                break;
            }
            let page_size = size.min(MAX_FROM_WINDOW - offset);
            if page_size <= 0 {
                break;
            }
            let batch = fetch_torrents_from_api(offset, page_size, seconds_since, query.as_deref(), order_by, order_direction, &categories, ct).await?;
            if !batch.is_valid {
                break;
            }
            if !batch.torrents.is_empty() {
                total_fetched += batch.torrents.len() as i32;
                all.extend(batch.torrents);
            }
            if batch.raw_hit_count < page_size {
                break;
            }
            if page < pages - 1 {
                trackers::sleep(api_delay_ms(), ct).await?;
            }
        }
        if all.is_empty() {
            return Ok((0, 0, 0, 0));
        }
        save_torrents(all, ct).await
    }
    .await;

    match res {
        Ok((added, updated, skipped, failed)) => {
            plog::write_kv(
                TRACKER_NAME,
                &format!("Parse completed successfully (took {}s)", common::secs(sw)),
                &kv![("fetched", total_fetched), ("added", added), ("updated", updated), ("skipped", skipped), ("failed", failed)],
            );
            format!("fetched={total_fetched} +{added} ~{updated} ={skipped} failed={failed}")
        }
        Err(KErr::Cancelled) => {
            plog::write_kv(TRACKER_NAME, "Canceled", &kv![("message", CANCELED_MESSAGE)]);
            "canceled".into()
        }
        Err(KErr::Other(msg)) => {
            plog::write(TRACKER_NAME, format!("Error: {msg}"));
            format!("error: {msg}")
        }
    }
}

fn page_log_fields(
    state: &KnabenBackfillState,
    page: &KnabenFetchPage,
    outcome: KnabenPageOutcome,
    attempts: i32,
    reason: Option<&str>,
) -> Vec<(String, String)> {
    let mut f = kv![
        ("cat", state.CategoryId),
        ("dir", &state.Direction),
        ("from", state.From),
        ("rawHits", page.raw_hit_count),
        ("mappedTorrents", page.torrents.len()),
        ("total.value", page.total_value.map(|v| v.to_string()).unwrap_or_default()),
        ("total.relation", page.total_relation.clone().unwrap_or_default()),
        ("outcome", outcome),
        ("attempts", attempts),
        ("valid", bool_text(page.is_valid))
    ]
    .to_vec();
    if let Some(r) = reason.filter(|r| !r.is_empty()) {
        f.push(("reason".into(), r.to_string()));
    }
    f
}

#[derive(Default)]
struct CallStats {
    fetched: i32,
    added: i32,
    updated: i32,
    skipped: i32,
    failed: i32,
}

async fn backfill_core(size: i32, pages: i32, reset: bool, ct: &CancellationToken) -> String {
    let sw = Instant::now();
    let mut call = CallStats::default();

    let mut state = load_backfill_state(reset);
    if state.Finished {
        let status = backfill::format_backfill_status(&state);
        plog::write_kv(TRACKER_NAME, "Backfill already finished", &kv![("status", &status)]);
        return format!("{status} (finished)");
    }

    plog::write_kv(
        TRACKER_NAME,
        "Starting backfill",
        &kv![
            ("size", size),
            ("pages", pages),
            ("reset", bool_text(reset)),
            ("cat", state.CategoryId),
            ("dir", &state.Direction),
            ("from", state.From)
        ],
    );

    let res: Result<(), KErr> = backfill_loop(&mut state, &mut call, size, pages, ct).await;

    match res {
        Ok(()) => {
            plog::write_kv(
                TRACKER_NAME,
                &format!("Backfill step completed (took {}s)", common::secs(sw)),
                &kv![
                    ("fetched", call.fetched),
                    ("added", call.added),
                    ("updated", call.updated),
                    ("skipped", call.skipped),
                    ("failed", call.failed),
                    ("cat", state.CategoryId),
                    ("dir", &state.Direction),
                    ("from", state.From),
                    ("finished", bool_text(state.Finished))
                ],
            );
            format!(
                "cat={} dir={} from={} fetched={} +{} ~{} ={} failed={} {}{}",
                state.CategoryId,
                state.Direction,
                state.From,
                call.fetched,
                call.added,
                call.updated,
                call.skipped,
                call.failed,
                backfill::format_backfill_progress(&state),
                if state.Finished { " finished" } else { "" }
            )
        }
        Err(KErr::Cancelled) => {
            plog::write_kv(TRACKER_NAME, "Backfill canceled", &kv![("message", CANCELED_MESSAGE)]);
            "canceled".into()
        }
        Err(KErr::Other(msg)) => {
            plog::write(TRACKER_NAME, format!("Backfill error: {msg}"));
            format!("error: {msg}")
        }
    }
}

async fn backfill_loop(state: &mut KnabenBackfillState, call: &mut CallStats, size: i32, pages: i32, ct: &CancellationToken) -> Result<(), KErr> {
    let mut pages_done = 0;
    while pages_done < pages && !state.Finished {
        trackers::check(ct)?;

        if state.From >= MAX_FROM_WINDOW {
            backfill::advance_backfill_pass(state, None, false);
            persist_backfill_state(state);
            continue;
        }
        let page_size = size.min(MAX_FROM_WINDOW - state.From);
        if page_size <= 0 {
            backfill::advance_backfill_pass(state, None, false);
            persist_backfill_state(state);
            continue;
        }

        let from = state.From;
        let cat = state.CategoryId;
        let direction = state.Direction.clone();
        let log_state = state.clone();
        let mut on_attempt = |page: &KnabenFetchPage, outcome: KnabenPageOutcome, attempt: i32| {
            plog::write_kv(TRACKER_NAME, "Backfill fetch", &page_log_fields(&log_state, page, outcome, attempt, None));
        };
        let (batch, outcome, attempts) = backfill::fetch_with_retry::<KErr, _, _, _, _>(
            || {
                let direction = direction.clone();
                async move {
                    fetch_torrents_from_api(from, page_size, None, None, "date", &direction, &[cat], ct).await.map(Some)
                }
            },
            page_size,
            from,
            |ms| async move { trackers::sleep(ms, ct).await.map_err(KErr::from) },
            ct,
            backfill::MAX_ATTEMPTS,
            Some(&mut on_attempt),
        )
        .await?;

        if outcome == KnabenPageOutcome::Retryable {
            if !batch.torrents.is_empty() {
                let n = batch.torrents.len() as i32;
                let (added, updated, skipped, failed) = save_torrents(batch.torrents.clone(), ct).await?;
                call.fetched += n;
                call.added += added;
                call.updated += updated;
                call.skipped += skipped;
                call.failed += failed;
            }
            plog::write_kv(
                TRACKER_NAME,
                "Backfill page retry exhausted, holding checkpoint",
                &page_log_fields(state, &batch, outcome, attempts, Some("retryHold")),
            );
            break;
        }

        if !batch.torrents.is_empty() {
            let n = batch.torrents.len() as i32;
            let (added, updated, skipped, failed) = save_torrents(batch.torrents.clone(), ct).await?;
            call.fetched += n;
            call.added += added;
            call.updated += updated;
            call.skipped += skipped;
            call.failed += failed;
            state.TotalFetched += n;
            state.TotalAdded += added;
            state.TotalUpdated += updated;
        }

        let early_end = outcome == KnabenPageOutcome::EndOfFeed;
        let is_asc = state.Direction == "asc";
        let ids = batch.ids.clone();

        if is_asc && !ids.is_empty() {
            state.AscEdgeIds = ids.clone();
        }
        if !is_asc && !state.AscEdgeIds.is_empty() && ids.iter().any(|id| state.AscEdgeIds.contains(id)) {
            state.DescSawOverlap = true;
        }

        state.From += page_size;
        pages_done += 1;

        if early_end || state.From >= MAX_FROM_WINDOW {
            let was_asc = state.Direction == "asc";
            let overlap = !was_asc && (state.DescSawOverlap || ids.iter().any(|id| state.AscEdgeIds.contains(id)));
            let reason = if was_asc {
                if early_end {
                    "endOfFeed"
                } else {
                    "window"
                }
            } else if overlap {
                "overlap"
            } else {
                "partial"
            };
            let pass_log = page_log_fields(state, &batch, outcome, attempts, Some(reason));
            backfill::advance_backfill_pass(state, Some(&ids), early_end);
            plog::write_kv(TRACKER_NAME, "Backfill pass ended", &pass_log);
        }

        persist_backfill_state(state);

        if pages_done < pages && !state.Finished {
            trackers::sleep(api_delay_ms(), ct).await?;
        }
    }
    Ok(())
}

fn load_backfill_state(reset: bool) -> KnabenBackfillState {
    let _g = BACKFILL_STATE_LOCK.lock();
    if reset || !std::path::Path::new(BACKFILL_STATE_PATH).exists() {
        let fresh = backfill::create_fresh_state();
        try_write_backfill_state(&fresh);
        return fresh;
    }
    let parsed = std::fs::read_to_string(BACKFILL_STATE_PATH)
        .ok()
        .and_then(|s| serde_json::from_str::<Option<KnabenBackfillState>>(s.trim_start_matches('\u{feff}')).ok());
    let mut state = match parsed {
        Some(Some(s)) => s,
        Some(None) => return backfill::create_fresh_state(),
        None => {
            let fresh = backfill::create_fresh_state();
            try_write_backfill_state(&fresh);
            return fresh;
        }
    };
    if state.Direction.trim().is_empty() {
        state.Direction = "asc".into();
    }
    state.Direction = normalize_order_direction(&state.Direction).to_string();
    if !state.Finished && (state.CategoryId == 0 || !BACKFILL_CATEGORIES.contains(&state.CategoryId)) {
        if state.CategoryIndex >= 0 && (state.CategoryIndex as usize) < BACKFILL_CATEGORIES.len() {
            state.CategoryId = BACKFILL_CATEGORIES[state.CategoryIndex as usize];
        } else {
            let fresh = backfill::create_fresh_state();
            try_write_backfill_state(&fresh);
            return fresh;
        }
    }
    state
}

fn persist_backfill_state(state: &mut KnabenBackfillState) {
    let _g = BACKFILL_STATE_LOCK.lock();
    state.UpdatedAt = chrono::Utc::now();
    try_write_backfill_state(state);
}

fn try_write_backfill_state(state: &KnabenBackfillState) {
    if let Some(dir) = std::path::Path::new(BACKFILL_STATE_PATH).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(BACKFILL_STATE_PATH, json);
    }
}

async fn api_request(req: &KnabenApiRequest, ct: &CancellationToken) -> Result<Option<KnabenApiResponse>, KErr> {
    let json = serde_json::to_string(req).map_err(|e| KErr::Other(e.to_string()))?;
    trackers::check(ct)?;
    let opts = Req::new().timeout(15).useproxy(conf().Knaben.useproxy);
    let response = net::post(&api_url(), &PostBody::Json(json), &opts).await;
    let Some(response) = response.filter(|r| !r.trim().is_empty()) else {
        plog::write(TRACKER_NAME, "API empty response");
        return Ok(None);
    };
    serde_json::from_str::<Option<KnabenApiResponse>>(&response).map_err(|e| KErr::Other(e.to_string()))
}

#[allow(clippy::too_many_arguments)]
async fn fetch_torrents_from_api(
    from: i32,
    size: i32,
    seconds_since: Option<i32>,
    query: Option<&str>,
    order_by: &str,
    order_direction: &str,
    categories: &[i32],
    ct: &CancellationToken,
) -> Result<KnabenFetchPage, KErr> {
    if from >= MAX_FROM_WINDOW || size <= 0 {
        return Ok(KnabenFetchPage::invalid());
    }
    let clamped = size.min(MAX_FROM_WINDOW - from);
    if clamped <= 0 {
        return Ok(KnabenFetchPage::invalid());
    }
    let mut req = KnabenApiRequest {
        categories: Some(categories.to_vec()),
        order_by: Some(if order_by == "seeders" || order_by == "peers" { order_by.to_string() } else { "date".to_string() }),
        order_direction: Some(normalize_order_direction(order_direction).to_string()),
        from,
        size: clamped,
        hide_unsafe: true,
        hide_xxx: true,
        ..Default::default()
    };
    if let Some(q) = query.filter(|q| !q.trim().is_empty()) {
        req.query = Some(q.to_string());
        req.search_field = Some("title".into());
    }
    req.seconds_since_last_seen = seconds_since;

    trackers::sleep(api_delay_ms(), ct).await?;
    let resp = api_request(&req, ct).await?;
    Ok(KnabenFetchPage::from_response(resp.as_ref()))
}

/// Upsert rows; rows without a magnet get one from the `.torrent` link.
async fn save_torrents(torrents: Vec<TorrentDetails>, ct: &CancellationToken) -> Result<(i32, i32, i32, i32), KErr> {
    let (mut added, mut updated, mut skipped, mut failed) = (0, 0, 0, 0);
    for (key, list) in common::group_by_bucket(torrents) {
        let w = fdb::open_write(&key);
        for mut t in list {
            let cached = common::cached_row(&w, &t.url);
            if let Some(c) = &cached {
                if c.title == t.title && c.magnet.trim().eq_ignore_ascii_case(t.magnet.trim()) {
                    skipped += 1;
                    plog::write_skipped(TRACKER_NAME, c, Some("no changes"));
                    continue;
                }
            }
            let exists = cached.is_some();

            if !t.magnet.trim().is_empty() {
                if exists {
                    updated += 1;
                    plog::write_updated(TRACKER_NAME, &t, Some("sid/pir/magnet"));
                } else {
                    added += 1;
                    plog::write_added(TRACKER_NAME, &t);
                }
                w.add_or_update(&t);
                continue;
            }

            let download_url = t._sn.clone();
            if download_url.trim().is_empty() || !download_url.to_ascii_lowercase().starts_with("http") {
                failed += 1;
                plog::write_failed(TRACKER_NAME, &t, Some("no magnet, no link"));
                continue;
            }

            trackers::sleep(api_delay_ms(), ct).await?;
            let mut opts = Req::new().timeout(15).useproxy(conf().Knaben.useproxy);
            if !t.url.trim().is_empty() {
                opts = opts.referer(t.url.clone());
            }
            let data = net::download(&download_url, &opts).await;
            let magnet = data.as_deref().and_then(bencode::magnet).unwrap_or_default();

            if !magnet.trim().is_empty() {
                t.magnet = magnet;
                t._sn = String::new();
                if exists {
                    updated += 1;
                    plog::write_updated(TRACKER_NAME, &t, Some("magnet from link"));
                } else {
                    added += 1;
                    plog::write_added(TRACKER_NAME, &t);
                }
                w.add_or_update(&t);
                continue;
            }

            failed += 1;
            plog::write_failed(TRACKER_NAME, &t, Some("could not get magnet from link"));
        }
    }
    Ok((added, updated, skipped, failed))
}

pub fn router() -> Router {
    Router::new()
        .route(
            "/cron/knaben/parse",
            any(|Query(q): Query<QueryMap>| async move {
                let d = ParseArgs::default();
                let args = ParseArgs {
                    from: common::q_i32(&q, "from", d.from),
                    size: common::q_i32(&q, "size", d.size),
                    pages: common::q_i32(&q, "pages", d.pages),
                    query: common::q_str(&q, "query"),
                    hours: common::q_i32(&q, "hours", d.hours),
                    order_by: common::q_str(&q, "orderby").unwrap_or(d.order_by),
                    order_direction: common::q_str(&q, "orderdirection").unwrap_or(d.order_direction),
                    categories: common::q_str(&q, "categories"),
                };
                parse(args).await
            }),
        )
        .route(
            "/cron/knaben/backfill",
            any(|Query(q): Query<QueryMap>| async move {
                backfill(common::q_i32(&q, "size", 300), common::q_i32(&q, "pages", 10), common::q_bool(&q, "reset", false)).await
            }),
        )
        .route("/cron/knaben/backfillstatus", any(|| async { backfill_status() }))
}
