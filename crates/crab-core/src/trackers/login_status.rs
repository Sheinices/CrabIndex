//! Login state of trackers that need an account, for the admin panel and the health card.
//!
//! A tracker reports the outcome of every login attempt with [`report`]; trackers with a
//! login flow also register a checker ([`register_checker`]) so the panel's "check login"
//! button can run it on demand.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    pub ok: bool,
    pub at: DateTime<Utc>,
    /// Failure reason, empty when `ok`.
    pub error: String,
}

pub type CheckFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
pub type Checker = Arc<dyn Fn() -> CheckFuture + Send + Sync>;

static STATUS: Lazy<DashMap<String, LoginStatus>> = Lazy::new(DashMap::new);
static CHECKERS: Lazy<DashMap<String, Checker>> = Lazy::new(DashMap::new);

fn key(tracker: &str) -> String {
    tracker.trim().to_ascii_lowercase()
}

/// Record a login outcome (`Ok(())` or the failure reason).
pub fn report(tracker: &str, result: Result<(), String>) {
    let (ok, error) = match result {
        Ok(()) => (true, String::new()),
        Err(e) => (false, e.trim().chars().take(300).collect()),
    };
    STATUS.insert(key(tracker), LoginStatus { ok, at: Utc::now(), error });
}

pub fn get(tracker: &str) -> Option<LoginStatus> {
    STATUS.get(&key(tracker)).map(|s| s.clone())
}

/// Register the on-demand login check of a tracker (runs the real login flow).
pub fn register_checker(tracker: &str, f: Checker) {
    CHECKERS.insert(key(tracker), f);
}

pub fn has_checker(tracker: &str) -> bool {
    CHECKERS.contains_key(&key(tracker))
}

/// Run the tracker's login check now; `None` when the tracker has no checker.
pub async fn check(tracker: &str) -> Option<Result<(), String>> {
    let f = CHECKERS.get(&key(tracker)).map(|c| c.clone())?;
    let r = f().await;
    report(tracker, r.clone());
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_and_get() {
        report("Kinozal", Err("  bad password  ".into()));
        let s = get("kinozal").expect("status");
        assert!(!s.ok);
        assert_eq!(s.error, "bad password");
        report("kinozal", Ok(()));
        assert!(get("KINOZAL").expect("status").ok);
        assert!(get("nope").is_none());
    }

    #[tokio::test]
    async fn checker_runs_and_records() {
        register_checker("demo", Arc::new(|| Box::pin(async { Err("no cookie".to_string()) })));
        assert!(has_checker("demo"));
        assert_eq!(check("demo").await, Some(Err("no cookie".to_string())));
        assert_eq!(get("demo").map(|s| s.error), Some("no cookie".to_string()));
        assert_eq!(check("none").await, None);
    }
}
