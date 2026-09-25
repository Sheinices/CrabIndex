//! Tracker sync shared helpers - parse locks and cron guard patterns.
//!
//! * `parse` (hourly): [`ParseLock`] + [`run_parse`] (never blocked by ParseAll/UpdateTasks)
//! * `ParseAllTask` / `UpdateTasksParse`: [`WorkFlag`] + per-tracker backfill gate + [`run_in_background`]
//! * `ParseLatest`: [`LatestLock`] + backfill gate + [`run_parse_latest`]
//! * ParseAll/ParseLatest yield to hourly parse between pages and throttle with remainder delay.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use futures::FutureExt;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::log::{self, cat};
use crate::net::cf;

pub const DISABLED_RESULT: &str = "disabled";
pub const WORK_RESULT: &str = "work";
pub const OK_RESULT: &str = "ok";
pub const IDLE_RESULT: &str = "idle";

/// Cancel ParseAll if no activity (progress / yield) for this long.
pub const PARSE_ALL_STALL_TIMEOUT: Duration = Duration::from_secs(45 * 60);
/// Default wall-clock limit for background UpdateTasksParse jobs.
pub const DEFAULT_UPDATE_TASKS_MAX_DURATION: Duration = Duration::from_secs(30 * 60);
pub const PERSIST_EVERY_PAGES: i64 = 25;
const HOURLY_PARSE_POLL_MS: u64 = 250;

/// Error returned when a background job observed cancellation.
#[derive(Debug, Clone, Copy)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "operation cancelled")
    }
}
impl std::error::Error for Cancelled {}

/// `Err(Cancelled)` once the token is cancelled.
pub fn check(ct: &CancellationToken) -> Result<(), Cancelled> {
    if ct.is_cancelled() {
        Err(Cancelled)
    } else {
        Ok(())
    }
}

/// Sleep that aborts on cancellation.
pub async fn sleep(ms: u64, ct: &CancellationToken) -> Result<(), Cancelled> {
    tokio::select! {
        _ = ct.cancelled() => Err(Cancelled),
        _ = tokio::time::sleep(Duration::from_millis(ms)) => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Locks
// ---------------------------------------------------------------------------

/// Per-tracker exclusive parse lock (hourly parse).
pub struct ParseLock {
    started: Mutex<Option<DateTime<Utc>>>,
}

impl ParseLock {
    pub const fn new() -> Self {
        ParseLock { started: parking_lot::const_mutex(None) }
    }
    pub fn try_start(&self) -> bool {
        let mut g = self.started.lock();
        if g.is_some() {
            return false;
        }
        *g = Some(Utc::now());
        true
    }
    pub fn end(&self) {
        *self.started.lock() = None;
    }
    pub fn is_busy(&self) -> bool {
        self.started.lock().is_some()
    }
    /// How long the lock has been held (None when free).
    pub fn held_for(&self) -> Option<chrono::Duration> {
        self.started.lock().map(|s| Utc::now() - s)
    }
}

impl Default for ParseLock {
    fn default() -> Self {
        Self::new()
    }
}

/// Work flag for secondary jobs (ParseAllTask / UpdateTasksParse).
pub struct WorkFlag(AtomicBool);

impl WorkFlag {
    pub const fn new() -> Self {
        WorkFlag(AtomicBool::new(false))
    }
    pub fn try_start(&self) -> bool {
        self.0.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
    pub fn end(&self) {
        self.0.store(false, Ordering::SeqCst)
    }
    pub fn is_busy(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Default for WorkFlag {
    fn default() -> Self {
        Self::new()
    }
}

/// One concurrent ParseLatest per tracker.
pub struct LatestLock(AtomicBool);

impl LatestLock {
    pub const fn new() -> Self {
        LatestLock(AtomicBool::new(false))
    }
    pub fn try_enter(&self) -> bool {
        self.0.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
    pub fn exit(&self) {
        self.0.store(false, Ordering::SeqCst)
    }
}

impl Default for LatestLock {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Active background jobs
// ---------------------------------------------------------------------------

pub struct JobInfo {
    pub key: String,
    pub tracker: String,
    pub job_label: String,
    pub started_at_utc: DateTime<Utc>,
    pub pages_completed: AtomicI64,
    pub pages_total: AtomicI64,
    /// Unix millis of last activity.
    pub last_activity_ms: AtomicI64,
    pub cancel_reason: Mutex<Option<String>>,
    pub current_category: Mutex<Option<String>>,
    pub current_page: Mutex<Option<i32>>,
}

/// Plain snapshot of [`JobInfo`].
#[derive(Clone, Debug, serde::Serialize)]
pub struct JobSnapshot {
    pub key: String,
    pub tracker: String,
    pub job_label: String,
    pub started_at_utc: DateTime<Utc>,
    pub pages_completed: i64,
    pub pages_total: i64,
    pub last_activity_utc: Option<DateTime<Utc>>,
    pub current_category: Option<String>,
    pub current_page: Option<i32>,
}

static ACTIVE_JOBS: Lazy<DashMap<String, Arc<JobInfo>>> = Lazy::new(DashMap::new);
static BACKFILL_GATES: Lazy<DashMap<String, Arc<WorkFlag>>> = Lazy::new(DashMap::new);
static RATE_STAMPS: Lazy<DashMap<String, i64>> = Lazy::new(DashMap::new);
static APP_STOPPING: Lazy<CancellationToken> = Lazy::new(CancellationToken::new);

/// Process-wide shutdown token (cancelled by the server on SIGINT/SIGTERM).
pub fn app_stopping() -> CancellationToken {
    APP_STOPPING.clone()
}

fn backfill_gate(tracker: &str) -> Arc<WorkFlag> {
    BACKFILL_GATES.entry(tracker.to_ascii_lowercase()).or_insert_with(|| Arc::new(WorkFlag::new())).clone()
}

fn job_key(tracker: &str, label: &str) -> String {
    format!("{}:{}", tracker.to_ascii_lowercase(), label.to_ascii_lowercase())
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn get_active_jobs() -> Vec<JobSnapshot> {
    let mut v: Vec<JobSnapshot> = ACTIVE_JOBS
        .iter()
        .map(|j| {
            let la = j.last_activity_ms.load(Ordering::SeqCst);
            JobSnapshot {
                key: j.key.clone(),
                tracker: j.tracker.clone(),
                job_label: j.job_label.clone(),
                started_at_utc: j.started_at_utc,
                pages_completed: j.pages_completed.load(Ordering::SeqCst),
                pages_total: j.pages_total.load(Ordering::SeqCst),
                last_activity_utc: if la > 0 { DateTime::from_timestamp_millis(la) } else { None },
                current_category: j.current_category.lock().clone(),
                current_page: *j.current_page.lock(),
            }
        })
        .collect();
    v.sort_by(|a, b| a.tracker.to_lowercase().cmp(&b.tracker.to_lowercase()).then(a.job_label.to_lowercase().cmp(&b.job_label.to_lowercase())));
    v
}

/// True if this tracker has an in-process ParseAll/UpdateTasks job.
pub fn has_active_job(tracker: &str, except_job_label: Option<&str>) -> bool {
    if tracker.trim().is_empty() {
        return false;
    }
    ACTIVE_JOBS.iter().any(|j| {
        j.tracker.eq_ignore_ascii_case(tracker) && !except_job_label.map(|e| j.job_label.eq_ignore_ascii_case(e)).unwrap_or(false)
    })
}

pub fn report_progress(tracker: &str, label: &str, done: i64, total: i64, category: Option<&str>, page: Option<i32>) {
    let Some(info) = ACTIVE_JOBS.get(&job_key(tracker, label)).map(|x| x.clone()) else { return };
    info.pages_completed.store(done, Ordering::SeqCst);
    info.pages_total.store(total, Ordering::SeqCst);
    info.last_activity_ms.store(now_ms(), Ordering::SeqCst);
    if let Some(c) = category {
        *info.current_category.lock() = Some(c.to_string());
    }
    if let Some(p) = page {
        *info.current_page.lock() = Some(p);
    }
    if done == 0 || done == total || done % PERSIST_EVERY_PAGES == 0 {
        let cc = info.current_category.lock().clone();
        let cp = *info.current_page.lock();
        let loc = if cc.is_none() && cp.is_none() {
            String::new()
        } else {
            format!(" (category={} page={})", cc.unwrap_or_default(), cp.map(|p| p.to_string()).unwrap_or_default())
        };
        log::info(cat::TRACKERS, format!("{tracker}: {label} progress={done}/{total}{loc}"));
    }
}

pub fn percent(done: i64, total: i64) -> Option<i32> {
    if total <= 0 {
        None
    } else {
        Some(((100.0 * done as f64 / total as f64).round() as i32).min(100))
    }
}

pub fn format_summary(job: &JobSnapshot) -> String {
    if job.pages_total <= 0 {
        return "running".into();
    }
    let mut s = format!("{}/{} pages", job.pages_completed, job.pages_total);
    if let Some(c) = job.current_category.as_ref().filter(|c| !c.is_empty()) {
        s.push_str(&format!(" · category {c}"));
    }
    if let Some(p) = job.current_page {
        s.push_str(&format!(" · page {p}"));
    }
    s
}

pub fn is_tracker_disabled(tracker: &str) -> bool {
    crate::conf().is_tracker_disabled(tracker)
}

pub fn log_parse_skipped(tracker: &str, reason: &str) {
    log::debug(cat::TRACKERS, format!("{tracker}: parse skipped ({reason})"));
}

pub fn note_job_activity(tracker: &str, label: &str) {
    if let Some(info) = ACTIVE_JOBS.get(&job_key(tracker, label)) {
        info.last_activity_ms.store(now_ms(), Ordering::SeqCst);
    }
}

/// Pause ParseAll/ParseLatest between pages while hourly parse holds `lock`.
pub async fn wait_while_hourly_parse_busy(lock: &ParseLock, ct: &CancellationToken, tracker: &str) -> Result<(), Cancelled> {
    while lock.is_busy() {
        check(ct)?;
        note_job_activity(tracker, "ParseAllTask");
        sleep(HOURLY_PARSE_POLL_MS, ct).await?;
    }
    Ok(())
}

/// Sleep only the remainder of `delay_ms` since the last [`note_request`].
pub async fn throttle(tracker: &str, delay_ms: i32, ct: &CancellationToken) -> Result<(), Cancelled> {
    if delay_ms <= 0 || tracker.trim().is_empty() {
        return Ok(());
    }
    let last = RATE_STAMPS.get(&tracker.to_ascii_lowercase()).map(|x| *x).unwrap_or(0);
    if last <= 0 {
        return Ok(());
    }
    let remain = delay_ms as i64 - (now_ms() - last);
    if remain > 0 {
        sleep(remain as u64, ct).await?;
    }
    Ok(())
}

pub fn note_request(tracker: &str) {
    if tracker.trim().is_empty() {
        return;
    }
    RATE_STAMPS.insert(tracker.to_ascii_lowercase(), now_ms());
}

pub async fn yield_to_hourly_parse_and_throttle(lock: &ParseLock, tracker: &str, delay_ms: i32, ct: &CancellationToken) -> Result<(), Cancelled> {
    wait_while_hourly_parse_busy(lock, ct, tracker).await?;
    throttle(tracker, delay_ms, ct).await
}

pub fn is_stalled(last_activity_ms: i64, now: DateTime<Utc>, timeout: Duration) -> bool {
    if last_activity_ms <= 0 || timeout.is_zero() {
        return false;
    }
    now.timestamp_millis() - last_activity_ms > timeout.as_millis() as i64
}

pub fn should_persist_checkpoint(completed: i64, total: i64) -> bool {
    if completed <= 0 {
        return false;
    }
    if total > 0 && completed >= total {
        return true;
    }
    completed % PERSIST_EVERY_PAGES == 0
}

fn format_cancel_message(tracker: &str, label: &str, info: &JobInfo) -> String {
    let reason = info.cancel_reason.lock().clone().unwrap_or_else(|| if APP_STOPPING.is_cancelled() { "shutdown".into() } else { "cancelled".into() });
    let reason_text = match reason.as_str() {
        "stall" => "no progress (stall)".to_string(),
        "shutdown" => "shutdown".to_string(),
        "wall" => "wall-clock limit".to_string(),
        r => r.to_string(),
    };
    let completed = info.pages_completed.load(Ordering::SeqCst);
    let slot_total = info.pages_total.load(Ordering::SeqCst);
    let pending_left = (slot_total - completed).max(0);
    let cycle = if label.eq_ignore_ascii_case("ParseAllTask") {
        super::cycle::load_state(&super::cycle::cycle_path_for_tracker(tracker))
    } else {
        None
    };
    let prefix = format!("{tracker}: {label} cancelled ({reason_text})");
    if slot_total <= 0 && cycle.as_ref().map(|c| c.MapCount <= 0).unwrap_or(true) {
        return prefix;
    }
    format!("{prefix}; {}", super::cycle::format_cancel_log(cycle.as_ref(), pending_left, slot_total))
}

// ---------------------------------------------------------------------------
// Runners
// ---------------------------------------------------------------------------

/// Hourly parse: exclusive per tracker; returns the action's log or `work`/`disabled`.
pub async fn run_parse<F, Fut>(tracker: &str, lock: &ParseLock, check_disabled: bool, action: F) -> String
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = String>,
{
    if check_disabled && is_tracker_disabled(tracker) {
        log_parse_skipped(tracker, DISABLED_RESULT);
        return DISABLED_RESULT.into();
    }
    if !lock.try_start() {
        let held = lock.held_for().map(|d| format!("{WORK_RESULT}, lock held for {}s", d.num_seconds())).unwrap_or_else(|| WORK_RESULT.into());
        log_parse_skipped(tracker, &held);
        return WORK_RESULT.into();
    }
    struct Release<'a>(&'a ParseLock);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            self.0.end();
        }
    }
    let _r = Release(lock);
    action().await
}

/// Start `action` in the background (crawl lane) and return `ok` / `work` / `disabled` immediately.
/// `max_duration` is a wall clock (UpdateTasks); ParseAllTask without it gets a stall watchdog.
pub fn run_in_background<F, Fut>(
    tracker: &str,
    label: &str,
    flag: &'static WorkFlag,
    check_disabled: bool,
    action: F,
    max_duration: Option<Duration>,
) -> String
where
    F: FnOnce(CancellationToken) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    if check_disabled && is_tracker_disabled(tracker) {
        log_parse_skipped(tracker, DISABLED_RESULT);
        return DISABLED_RESULT.into();
    }
    if !flag.try_start() {
        log_parse_skipped(tracker, WORK_RESULT);
        return WORK_RESULT.into();
    }
    let backfill = backfill_gate(tracker);
    if !backfill.try_start() {
        flag.end();
        log_parse_skipped(tracker, WORK_RESULT);
        return WORK_RESULT.into();
    }

    let key = job_key(tracker, label);
    let info = Arc::new(JobInfo {
        key: key.clone(),
        tracker: tracker.to_string(),
        job_label: label.to_string(),
        started_at_utc: Utc::now(),
        pages_completed: AtomicI64::new(0),
        pages_total: AtomicI64::new(0),
        last_activity_ms: AtomicI64::new(now_ms()),
        cancel_reason: Mutex::new(None),
        current_category: Mutex::new(None),
        current_page: Mutex::new(None),
    });
    ACTIVE_JOBS.insert(key.clone(), info.clone());

    let tracker = tracker.to_string();
    let label = label.to_string();
    tokio::spawn(cf::with_crawl_lane(async move {
        let ct = APP_STOPPING.child_token();
        if let Some(limit) = max_duration.filter(|d| !d.is_zero()) {
            let ct2 = ct.clone();
            let info2 = info.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = ct2.cancelled() => {}
                    _ = tokio::time::sleep(limit) => {
                        info2.cancel_reason.lock().get_or_insert_with(|| "wall".into());
                        ct2.cancel();
                    }
                }
            });
        }
        let stall_watch = if label.eq_ignore_ascii_case("ParseAllTask") && max_duration.map(|d| d.is_zero()).unwrap_or(true) {
            let ct2 = ct.clone();
            let info2 = info.clone();
            Some(tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = ct2.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_secs(30)) => {}
                    }
                    if is_stalled(info2.last_activity_ms.load(Ordering::SeqCst), Utc::now(), PARSE_ALL_STALL_TIMEOUT) {
                        *info2.cancel_reason.lock() = Some("stall".into());
                        log::warn(
                            cat::TRACKERS,
                            format!("{}: ParseAllTask stall watchdog - no activity for {}m", info2.tracker, PARSE_ALL_STALL_TIMEOUT.as_secs() / 60),
                        );
                        ct2.cancel();
                        return;
                    }
                }
            }))
        } else {
            None
        };
        let limit_label = match max_duration.filter(|d| !d.is_zero()) {
            Some(d) => format!("limit={}s", d.as_secs()),
            None => "no wall-clock; stall watchdog on".into(),
        };
        log::info(cat::TRACKERS, format!("{tracker}: {label} started (background, {limit_label})"));

        // awaited in place (not spawned) so the crawl-lane task-local stays visible
        let res = std::panic::AssertUnwindSafe(action(ct.clone())).catch_unwind().await;
        match res {
            Ok(Ok(())) if !ct.is_cancelled() => log::info(cat::TRACKERS, format!("{tracker}: {label} finished")),
            Ok(Ok(())) => log::warn(cat::TRACKERS, format_cancel_message(&tracker, &label, &info)),
            Ok(Err(e)) => {
                if ct.is_cancelled() || e.downcast_ref::<Cancelled>().is_some() {
                    log::warn(cat::TRACKERS, format_cancel_message(&tracker, &label, &info));
                } else {
                    log::error(cat::TRACKERS, format!("{tracker}: {label} error: {e}"));
                }
            }
            Err(_) => log::error(cat::TRACKERS, format!("{tracker}: {label} error: panic")),
        }
        ct.cancel();
        if let Some(w) = stall_watch {
            let _ = w.await;
        }
        ACTIVE_JOBS.remove(&key);
        flag.end();
        backfill.end();
    }));

    OK_RESULT.into()
}

pub fn run_parse_all_task_in_background<F, Fut>(tracker: &str, flag: &'static WorkFlag, check_disabled: bool, action: F) -> String
where
    F: FnOnce(CancellationToken) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    run_in_background(tracker, "ParseAllTask", flag, check_disabled, action, None)
}

pub fn run_update_tasks_parse_in_background<F, Fut>(tracker: &str, flag: &'static WorkFlag, check_disabled: bool, action: F) -> String
where
    F: FnOnce(CancellationToken) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    run_in_background(tracker, "UpdateTasksParse", flag, check_disabled, action, Some(DEFAULT_UPDATE_TASKS_MAX_DURATION))
}

/// ParseLatest: one run per tracker, excluded with ParseAll/UpdateTasks via the backfill gate.
pub async fn run_parse_latest<F, Fut>(tracker: &str, lock: &LatestLock, check_disabled: bool, build_log: F) -> String
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = String>,
{
    if check_disabled && is_tracker_disabled(tracker) {
        log_parse_skipped(tracker, DISABLED_RESULT);
        return DISABLED_RESULT.into();
    }
    let backfill = backfill_gate(tracker);
    if !backfill.try_start() {
        log_parse_skipped(tracker, WORK_RESULT);
        return WORK_RESULT.into();
    }
    if !lock.try_enter() {
        backfill.end();
        log_parse_skipped(tracker, WORK_RESULT);
        return WORK_RESULT.into();
    }
    struct Release<'a>(&'a LatestLock, Arc<WorkFlag>);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            self.0.exit();
            self.1.end();
        }
    }
    let _r = Release(lock, backfill);
    let text = cf::with_crawl_lane(build_log()).await;
    if text.trim().is_empty() {
        OK_RESULT.into()
    } else {
        text
    }
}
