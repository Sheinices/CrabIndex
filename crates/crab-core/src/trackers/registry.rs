//! ParseAll starters (trackers with a resumable ParseAllTask cycle).

use async_trait::async_trait;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::sync::Arc;

#[async_trait]
pub trait ParseAllStarter: Send + Sync {
    /// Tracker slug (lowercase), e.g. "rutor".
    fn tracker_name(&self) -> &'static str;
    /// Kick ParseAllTask (returns ok / work / disabled immediately).
    async fn parse_all_task(&self) -> String;
}

static STARTERS: Lazy<Mutex<Vec<Arc<dyn ParseAllStarter>>>> = Lazy::new(|| Mutex::new(Vec::new()));

pub fn register_parse_all_starter(s: Arc<dyn ParseAllStarter>) {
    let mut v = STARTERS.lock();
    if !v.iter().any(|x| x.tracker_name() == s.tracker_name()) {
        v.push(s);
    }
}

pub fn parse_all_starters() -> Vec<Arc<dyn ParseAllStarter>> {
    STARTERS.lock().clone()
}
