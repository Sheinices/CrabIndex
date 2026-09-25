//! Page-by-page backfill: fetch → save → commit cursor.
//! The cursor advances only after a page is fully saved; a saved page with no next cursor
//! is the archive end and persists the `finished` sentinel.

use async_trait::async_trait;
use crab_core::models::TorrentDetails;
use crab_core::trackers::{self, Cancelled};
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;

use super::pagination;

pub const FINISHED_SENTINEL: &str = "finished";

#[derive(Clone, Debug, Default)]
pub struct BitruBackfillPage {
    pub torrents: Vec<TorrentDetails>,
    pub next_cursor: Option<i64>,
    pub stop: bool,
    pub ids: Option<HashSet<i64>>,
}

impl BitruBackfillPage {
    pub fn halt() -> Self {
        BitruBackfillPage { stop: true, ..Default::default() }
    }

    pub fn ok(torrents: Vec<TorrentDetails>, next_cursor: Option<i64>, ids: Option<HashSet<i64>>) -> Self {
        BitruBackfillPage { torrents, next_cursor, stop: false, ids }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BitruBackfillProgress {
    pub fetched_pages: i32,
    pub committed_pages: i32,
    pub saved_count: i32,
    pub last_committed_cursor: Option<i64>,
    pub finished: bool,
}

fn format_cursor(c: Option<i64>) -> String {
    c.map(|v| v.to_string()).unwrap_or_else(|| "none".into())
}

impl BitruBackfillProgress {
    pub fn format_log(&self) -> String {
        let cursor = format_cursor(self.last_committed_cursor);
        if self.finished && self.saved_count == 0 && self.committed_pages == 0 {
            return format!("finished, fetchedPages={}, committedPages={}, cursor={cursor}", self.fetched_pages, self.committed_pages);
        }
        if self.saved_count == 0 && self.committed_pages == 0 {
            return format!("no items, fetchedPages={}, committedPages={}, cursor={cursor}", self.fetched_pages, self.committed_pages);
        }
        let suffix = if self.finished { ", finished" } else { "" };
        format!(
            "saved {}, fetchedPages={}, committedPages={}, cursor={cursor}{suffix}",
            self.saved_count, self.fetched_pages, self.committed_pages
        )
    }

    pub fn format_canceled_log(&self) -> String {
        format!(
            "canceled, saved={}, fetchedPages={}, committedPages={}, cursor={}",
            self.saved_count,
            self.fetched_pages,
            self.committed_pages,
            format_cursor(self.last_committed_cursor)
        )
    }
}

/// Page source/sink driven by [`run`].
#[async_trait]
pub trait BackfillSource: Send {
    async fn fetch_page(&mut self, cursor: Option<i64>, ct: &CancellationToken) -> anyhow::Result<BitruBackfillPage>;
    async fn save_page(&mut self, torrents: &[TorrentDetails], ct: &CancellationToken) -> anyhow::Result<()>;
    fn commit_cursor(&mut self, unix: i64);
    fn commit_finished(&mut self);
}

/// Returns `Err` (downcastable to [`Cancelled`] on cancellation); `progress` reflects the work done.
pub async fn run<S: BackfillSource + ?Sized>(
    max_pages: i32,
    start_cursor: Option<i64>,
    source: &mut S,
    progress: &mut BitruBackfillProgress,
    ct: &CancellationToken,
) -> anyhow::Result<()> {
    if progress.last_committed_cursor.is_none() && start_cursor.is_some() {
        progress.last_committed_cursor = start_cursor;
    }
    let max_pages = pagination::clamp_pages(max_pages);
    let mut request_cursor = start_cursor;

    for _ in 0..max_pages {
        trackers::check(ct).map_err(anyhow::Error::from)?;

        let fetched = source.fetch_page(request_cursor, ct).await?;
        if fetched.stop {
            break;
        }
        progress.fetched_pages += 1;

        source.save_page(&fetched.torrents, ct).await?;
        progress.saved_count += fetched.torrents.len() as i32;
        progress.committed_pages += 1;

        let Some(next) = fetched.next_cursor else {
            source.commit_finished();
            progress.finished = true;
            break;
        };
        source.commit_cursor(next);
        progress.last_committed_cursor = Some(next);
        request_cursor = Some(next);
    }
    Ok(())
}

pub fn is_cancelled(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Cancelled>().is_some()
}

pub fn write_cursor_atomic(path: &str, unix: i64) -> std::io::Result<()> {
    write_atomic(path, &unix.to_string())
}

pub fn write_finished_atomic(path: &str) -> std::io::Result<()> {
    write_atomic(path, FINISHED_SENTINEL)
}

pub fn is_finished(path: &str) -> bool {
    match std::fs::read_to_string(path) {
        Ok(t) => t.trim().eq_ignore_ascii_case(FINISHED_SENTINEL),
        Err(_) => false,
    }
}

pub fn read_cursor(path: &str) -> Option<i64> {
    if is_finished(path) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    text.trim().parse::<i64>().ok().filter(|u| *u > 0)
}

/// Cursor for this backfill kick; `None` starts from the newest page
/// (missing file or the previous pass wrote [`FINISHED_SENTINEL`]).
pub fn read_start_cursor(path: &str) -> Option<i64> {
    read_cursor(path)
}

fn write_atomic(path: &str, text: &str) -> std::io::Result<()> {
    let p = std::path::Path::new(path);
    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}
