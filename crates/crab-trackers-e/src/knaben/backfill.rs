//! Archive backfill page classification, retry loop and pass/category state machine.
//!
//! End-of-feed is decided from the raw hit count and `total.relation == "eq"`,
//! never from the number of mapped torrents.

use std::future::Future;

use crab_core::models::TorrentDetails;
use crab_core::trackers::Cancelled;
use tokio_util::sync::CancellationToken;

use super::models::{KnabenApiResponse, KnabenBackfillState};
use super::parser;

/// Leaf TV/Movies subcategories crawled by the backfill (no parents 2000000 / 3000000).
pub const BACKFILL_CATEGORIES: [i32; 16] = [
    2001000, 2002000, 2003000, 2004000, 2005000, 2006000, 2007000, 2008000, 3001000, 3002000, 3003000, 3004000, 3005000, 3006000,
    3007000, 3008000,
];

pub const MAX_ATTEMPTS: i32 = 3;
pub const RETRY_BACKOFF_MS: [u64; 2] = [2000, 8000];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnabenPageOutcome {
    Full,
    EndOfFeed,
    Retryable,
}

impl std::fmt::Display for KnabenPageOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            KnabenPageOutcome::Full => "Full",
            KnabenPageOutcome::EndOfFeed => "EndOfFeed",
            KnabenPageOutcome::Retryable => "Retryable",
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct KnabenFetchPage {
    pub is_valid: bool,
    pub raw_hit_count: i32,
    pub total_value: Option<i32>,
    pub total_relation: Option<String>,
    pub torrents: Vec<TorrentDetails>,
    pub ids: Vec<String>,
}

impl KnabenFetchPage {
    pub fn invalid() -> Self {
        KnabenFetchPage::default()
    }

    pub fn from_response(resp: Option<&KnabenApiResponse>) -> Self {
        let Some(resp) = resp else { return Self::invalid() };
        let Some(hits) = resp.hits.as_ref() else { return Self::invalid() };
        let ids = hits.iter().map(|h| h.id.clone()).filter(|id| !id.trim().is_empty()).collect();
        let torrents = hits.iter().filter_map(parser::map_to_torrent_details).collect();
        KnabenFetchPage {
            is_valid: true,
            raw_hit_count: hits.len() as i32,
            total_value: resp.total.as_ref().map(|t| t.value),
            total_relation: resp.total.as_ref().and_then(|t| t.relation.clone()),
            torrents,
            ids,
        }
    }
}

pub fn classify(is_valid: bool, raw_hits: i32, page_size: i32, from: i32, total_value: Option<i32>, total_relation: Option<&str>) -> KnabenPageOutcome {
    if !is_valid {
        return KnabenPageOutcome::Retryable;
    }
    if raw_hits == page_size {
        return KnabenPageOutcome::Full;
    }
    if total_relation.map(|r| r.eq_ignore_ascii_case("eq")).unwrap_or(false) {
        if let Some(total) = total_value {
            if from + raw_hits >= total {
                return KnabenPageOutcome::EndOfFeed;
            }
        }
    }
    KnabenPageOutcome::Retryable
}

pub fn classify_page(page: Option<&KnabenFetchPage>, page_size: i32, from: i32) -> KnabenPageOutcome {
    match page {
        None => KnabenPageOutcome::Retryable,
        Some(p) => classify(p.is_valid, p.raw_hit_count, page_size, from, p.total_value, p.total_relation.as_deref()),
    }
}

/// Called after every fetch attempt with (page, outcome, attempt number).
pub type AttemptCallback<'a> = &'a mut (dyn FnMut(&KnabenFetchPage, KnabenPageOutcome, i32) + Send);

/// Fetch a page up to `max_attempts` times, backing off between retryable outcomes.
/// Errors from `fetch`/`delay` (and cancellation) abort the loop.
pub async fn fetch_with_retry<E, F, Fut, D, DFut>(
    mut fetch: F,
    page_size: i32,
    from: i32,
    mut delay: D,
    ct: &CancellationToken,
    max_attempts: i32,
    mut on_attempt: Option<AttemptCallback<'_>>,
) -> Result<(KnabenFetchPage, KnabenPageOutcome, i32), E>
where
    E: From<Cancelled>,
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<KnabenFetchPage>, E>>,
    D: FnMut(u64) -> DFut,
    DFut: Future<Output = Result<(), E>>,
{
    let mut last = KnabenFetchPage::invalid();
    let mut outcome = KnabenPageOutcome::Retryable;
    let mut attempts = 0;
    let retries = max_attempts.max(1);

    for i in 0..retries {
        if ct.is_cancelled() {
            return Err(Cancelled.into());
        }
        last = fetch().await?.unwrap_or_else(KnabenFetchPage::invalid);
        attempts += 1;
        outcome = classify_page(Some(&last), page_size, from);
        if let Some(cb) = on_attempt.as_mut() {
            cb(&last, outcome, attempts);
        }
        if outcome != KnabenPageOutcome::Retryable {
            return Ok((last, outcome, attempts));
        }
        if i < retries - 1 {
            let backoff = RETRY_BACKOFF_MS[(i as usize).min(RETRY_BACKOFF_MS.len() - 1)];
            delay(backoff).await?;
        }
    }
    Ok((last, outcome, attempts))
}

/// Finish the current pass: asc end-of-feed completes the category, asc window-hit flips to
/// desc; a finished desc pass is `complete` when it overlapped the asc edge, else `partial`.
pub fn advance_backfill_pass(state: &mut KnabenBackfillState, last_page_ids: Option<&[String]>, early_end: bool) {
    let is_asc = state.Direction == "asc";
    if is_asc {
        if early_end {
            let id = state.CategoryId;
            set_category_status(state, id, "complete");
            move_to_next_category(state);
        } else {
            // Hit the 10k window - keep AscEdgeIds, switch to desc.
            state.Direction = "desc".into();
            state.From = 0;
            state.DescSawOverlap = false;
        }
        return;
    }

    let overlap = state.DescSawOverlap || last_page_ids.map(|ids| ids.iter().any(|id| state.AscEdgeIds.contains(id))).unwrap_or(false);
    let id = state.CategoryId;
    set_category_status(state, id, if overlap { "complete" } else { "partial" });
    move_to_next_category(state);
}

fn move_to_next_category(state: &mut KnabenBackfillState) {
    state.CategoryIndex += 1;
    state.AscEdgeIds = Vec::new();
    state.DescSawOverlap = false;
    state.Direction = "asc".into();
    state.From = 0;

    if state.CategoryIndex < 0 || state.CategoryIndex as usize >= BACKFILL_CATEGORIES.len() {
        state.Finished = true;
        state.CategoryId = 0;
        return;
    }
    state.CategoryId = BACKFILL_CATEGORIES[state.CategoryIndex as usize];
    let key = state.CategoryId.to_string();
    state.CategoryStatus.entry(key).or_insert_with(|| "pending".into());
}

fn set_category_status(state: &mut KnabenBackfillState, category_id: i32, status: &str) {
    state.CategoryStatus.insert(category_id.to_string(), status.to_string());
}

pub fn create_fresh_state() -> KnabenBackfillState {
    let mut state = KnabenBackfillState {
        CategoryIndex: 0,
        CategoryId: BACKFILL_CATEGORIES[0],
        Direction: "asc".into(),
        From: 0,
        Finished: false,
        UpdatedAt: chrono::Utc::now(),
        ..Default::default()
    };
    for cat in BACKFILL_CATEGORIES {
        state.CategoryStatus.insert(cat.to_string(), "pending".into());
    }
    state
}

pub fn format_backfill_progress(state: &KnabenBackfillState) -> String {
    let done = state.CategoryStatus.values().filter(|v| *v == "complete" || *v == "partial").count();
    let partial = state.CategoryStatus.values().filter(|v| *v == "partial").count();
    let mut s = format!("progress={done}/{}", BACKFILL_CATEGORIES.len());
    if partial > 0 {
        s.push_str(&format!(" partial={partial}"));
    }
    s
}

pub fn format_backfill_status(state: &KnabenBackfillState) -> String {
    format!(
        "finished={} cat={} dir={} from={} totalFetched={} +{} ~{} {} updatedAt={}",
        crate::common::bool_text(state.Finished),
        state.CategoryId,
        state.Direction,
        state.From,
        state.TotalFetched,
        state.TotalAdded,
        state.TotalUpdated,
        format_backfill_progress(state),
        crate::common::iso_o(&state.UpdatedAt)
    )
}
