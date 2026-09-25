//! Continue incomplete ParseAll cycles (after restart / deploy).
//!
//! A tracker is kicked only when its cycle file exists and still has pending pages;
//! a running ParseAllTask reports `work`, nothing to do reports `idle`.

use chrono::{DateTime, Utc};
use crab_core::log::{self, cat};
use crab_core::trackers::{self, cycle, ParseAllStarter};
use futures::FutureExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Serialize, Debug, Clone)]
pub struct ParseAllResumeSnapshot {
    pub tracker: String,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycleId: Option<String>,
    pub pending: i64,
    pub mapCount: i64,
    #[serde(with = "crab_core::time::net_opt", skip_serializing_if = "Option::is_none")]
    pub cycleStartedAtUtc: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagesCompleted: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagesTotal: Option<i64>,
}

fn sorted(mut starters: Vec<Arc<dyn ParseAllStarter>>) -> Vec<Arc<dyn ParseAllStarter>> {
    starters.sort_by_key(|s| s.tracker_name().to_lowercase());
    starters
}

pub fn describe(tracker: &str) -> ParseAllResumeSnapshot {
    let (c, pending, map_count) = cycle::read_cycle_progress(tracker);
    let job = trackers::get_active_jobs()
        .into_iter()
        .find(|j| j.tracker.eq_ignore_ascii_case(tracker) && j.job_label.eq_ignore_ascii_case("ParseAllTask"));
    ParseAllResumeSnapshot {
        tracker: tracker.to_string(),
        running: job.is_some(),
        result: None,
        cycleId: c.as_ref().map(|c| c.CycleId.clone()),
        pending,
        mapCount: map_count,
        cycleStartedAtUtc: c.as_ref().map(|c| c.StartedAtUtc),
        pagesCompleted: job.as_ref().map(|j| j.pages_completed),
        pagesTotal: job.as_ref().map(|j| j.pages_total),
    }
}

pub fn status_with(starters: Vec<Arc<dyn ParseAllStarter>>) -> Vec<ParseAllResumeSnapshot> {
    sorted(starters).iter().map(|s| describe(s.tracker_name())).collect()
}

/// Cycle pending vs running ParseAll for every registered starter.
pub fn status() -> Vec<ParseAllResumeSnapshot> {
    status_with(trackers::parse_all_starters())
}

pub async fn resume_with(starters: Vec<Arc<dyn ParseAllStarter>>) -> Value {
    let mut items = Vec::new();
    let mut started = 0;
    for starter in sorted(starters) {
        let name = starter.tracker_name();
        let mut snap = describe(name);
        if snap.running {
            snap.result = Some(trackers::WORK_RESULT.into());
            items.push(snap);
            continue;
        }
        if snap.cycleId.as_deref().map(|s| s.is_empty()).unwrap_or(true) || snap.pending <= 0 {
            snap.result = Some(trackers::IDLE_RESULT.into());
            items.push(snap);
            continue;
        }
        let res = std::panic::AssertUnwindSafe(starter.parse_all_task()).catch_unwind().await;
        snap.result = Some(match res {
            Ok(r) => r,
            Err(_) => {
                log::error(cat::TRACKERS, format!("{name}: ResumeParseAll error: panic"));
                "error".into()
            }
        });
        if snap.result.as_deref() == Some(trackers::OK_RESULT) {
            started += 1;
        }
        items.push(snap);
    }
    log::info(cat::TRACKERS, format!("ResumeParseAll started={started} trackers={}", items.len()));
    json!({ "started": started, "jobs": items })
}

/// Resume every registered starter with pending work. Returns `{started, jobs}`.
pub async fn resume() -> Value {
    resume_with(trackers::parse_all_starters()).await
}

/// 45 s after startup, resume incomplete cycles once.
pub fn spawn_worker(shutdown: CancellationToken) {
    tokio::spawn(async move {
        if !crate::sleep_ct(Duration::from_secs(45), &shutdown).await {
            return;
        }
        log::info(cat::TRACKERS, "ParseAll resume after startup");
        let _ = resume().await;
    });
}
