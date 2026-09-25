mod common;

use async_trait::async_trait;
use crab_core::models::{TaskMap, TaskParse};
use crab_core::trackers::{self, cycle, ParseAllStarter, WorkFlag};
use crab_ops::maintenance::resume;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

struct FakeStarter {
    name: &'static str,
    calls: AtomicUsize,
}

#[async_trait]
impl ParseAllStarter for FakeStarter {
    fn tracker_name(&self) -> &'static str {
        self.name
    }
    async fn parse_all_task(&self) -> String {
        self.calls.fetch_add(1, Ordering::SeqCst);
        trackers::OK_RESULT.into()
    }
}

fn slug() -> &'static str {
    Box::leak(format!("resume-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]).into_boxed_str())
}

fn seed_pending_cycle(slug: &str) {
    let c = cycle::create_cycle("fp", 1);
    cycle::save_state(&cycle::cycle_path_for_tracker(slug), &c);
    let mut map = TaskMap::new();
    map.insert("1".into(), vec![TaskParse::new(0)]);
    cycle::write_json_atomic(&cycle::task_parse_path_for_tracker(slug), &map).unwrap();
}

fn starter(name: &'static str) -> Arc<FakeStarter> {
    Arc::new(FakeStarter { name, calls: AtomicUsize::new(0) })
}

#[tokio::test]
async fn resume_is_idle_without_cycle_or_pending() {
    common::enter_temp_cwd("resume");
    let s = starter(slug());
    let json = resume::resume_with(vec![s.clone()]).await;
    assert_eq!(json["started"], 0);
    assert_eq!(json["jobs"][0]["result"], trackers::IDLE_RESULT);
    assert_eq!(s.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resume_starts_when_cycle_has_pending_pages() {
    common::enter_temp_cwd("resume");
    let name = slug();
    seed_pending_cycle(name);
    let s = starter(name);
    let json = resume::resume_with(vec![s.clone()]).await;
    assert_eq!(json["started"], 1);
    assert_eq!(json["jobs"][0]["result"], trackers::OK_RESULT);
    assert_eq!(json["jobs"][0]["pending"], 1);
    assert_eq!(json["jobs"][0]["mapCount"], 1);
    assert!(json["jobs"][0]["cycleId"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
    assert_eq!(s.calls.load(Ordering::SeqCst), 1);
}

static FLAG: WorkFlag = WorkFlag::new();

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_reports_work_when_parse_all_is_running() {
    common::enter_temp_cwd("resume");
    let name = slug();
    seed_pending_cycle(name);
    let s = starter(name);

    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel::<()>();
    let hold = tokio_util::sync::CancellationToken::new();
    let hold2 = hold.clone();
    let kick = trackers::run_parse_all_task_in_background(name, &FLAG, false, move |ct| async move {
        let _ = entered_tx.send(());
        tokio::select! {
            _ = hold2.cancelled() => {}
            _ = ct.cancelled() => {}
        }
        Ok(())
    });
    assert_eq!(kick, trackers::OK_RESULT);
    tokio::time::timeout(Duration::from_secs(5), entered_rx).await.unwrap().unwrap();

    let json = resume::resume_with(vec![s.clone()]).await;
    assert_eq!(json["started"], 0);
    assert_eq!(json["jobs"][0]["result"], trackers::WORK_RESULT);
    assert_eq!(json["jobs"][0]["running"], true);
    assert_eq!(s.calls.load(Ordering::SeqCst), 0);

    let status = resume::status_with(vec![s.clone()]);
    assert!(status[0].running);
    assert!(status[0].result.is_none());

    hold.cancel();
    for _ in 0..200 {
        if !FLAG.is_busy() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(!FLAG.is_busy());
}

#[test]
fn snapshot_omits_nulls() {
    let snap = resume::ParseAllResumeSnapshot {
        tracker: "rutor".into(),
        running: false,
        result: None,
        cycleId: None,
        pending: 0,
        mapCount: 0,
        cycleStartedAtUtc: None,
        pagesCompleted: None,
        pagesTotal: None,
    };
    assert_eq!(serde_json::to_string(&snap).unwrap(), r#"{"tracker":"rutor","running":false,"pending":0,"mapCount":0}"#);
}
