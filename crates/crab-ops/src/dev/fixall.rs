// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! **Fix all**: run every migration the last data-quality report ([`super::datacheck`]) asks
//! for, one after another in the background, then re-run the report. `/dev/fixall` starts it,
//! `/dev/fixallstatus` (and the `fixAll` field of `/dev/checkdatastatus`) shows the progress.

use std::time::Instant;

use chrono::{DateTime, Utc};
use crab_core::log::{self, cat};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};

use super::migrations::{domain_dups, rutracker_names, slug_dups};
use super::{datacheck, maintenance};

/// A migration the report can call for, keyed by its `/dev` path (as the report's `fixes`).
#[derive(Clone, Copy)]
pub struct Fix {
    pub path: &'static str,
    pub run: fn() -> Value,
}

/// Every fix the plan may contain, in execution order: domain merges first (they remove rows),
/// then id duplicates, then names and sizes.
const FIXES: [Fix; 7] = [
    Fix { path: "dev/fixkinozaldomainduplicates", run: domain_dups::fix_kinozal },
    Fix { path: "dev/fixrutrackerdomainduplicates", run: domain_dups::fix_rutracker },
    Fix { path: "dev/fixselezendomainduplicates", run: domain_dups::fix_selezen },
    Fix { path: "dev/fixultradoxdomainduplicates", run: domain_dups::fix_ultradox },
    Fix { path: "dev/fixslugduplicates", run: slug_dups::fix_slug_duplicates },
    Fix { path: "dev/fixrutrackernames", run: rutracker_names::fix_rutracker_names },
    Fix { path: "dev/fixzerosizes", run: maintenance::fix_zero_sizes },
];

/// The `/dev` paths a report needs, in [`FIXES`] order: a column fix when its total is above
/// zero, a domain fix for every tracker with foreign hosts that has one.
pub fn plan(report: &Value) -> Vec<&'static str> {
    let total = &report["total"];
    let fixes = &report["fixes"];
    let mut wanted: Vec<String> = Vec::new();
    for col in ["zeroSize", "dupIds", "badNames"] {
        if total[col].as_i64().unwrap_or(0) > 0 {
            if let Some(p) = fixes[col].as_str() {
                wanted.push(p.to_string());
            }
        }
    }
    for row in report["trackers"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        if row["foreignHost"].as_i64().unwrap_or(0) > 0 {
            if let Some(p) = row["tracker"].as_str().and_then(|t| fixes["foreignHost"][t].as_str()) {
                wanted.push(p.to_string());
            }
        }
    }
    FIXES.iter().filter(|f| wanted.iter().any(|w| w.eq_ignore_ascii_case(f.path))).map(|f| f.path).collect()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub path: &'static str,
    /// `pending`, `running`, `done` or `failed`.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub took_sec: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub steps: Vec<Step>,
    /// After the steps: the data check runs again (`checking`), then `done`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<&'static str>,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

pub fn is_running() -> bool {
    STATE.lock().running
}

/// Progress of the current or last run plus the `plan` for the last report when idle.
pub fn status() -> Value {
    let st = STATE.lock().clone();
    let mut v = serde_json::to_value(&st).unwrap_or(Value::Null);
    if !st.running {
        v["plan"] = json!(datacheck::last_report().map(|r| plan(&r)).unwrap_or_default());
    }
    v
}

/// Start the run in the background. `{ ok, steps }`, or `{ ok: false, error }` when a run or a
/// data check is in progress, there is no report yet, or the report needs nothing.
pub fn start() -> Value {
    let Some(report) = datacheck::last_report() else {
        return json!({ "ok": false, "error": "run the data check first" });
    };
    if datacheck::is_running() {
        return json!({ "ok": false, "error": "data check is running, wait for it" });
    }
    let steps = plan(&report);
    if steps.is_empty() {
        return json!({ "ok": false, "error": "the last report needs no fixes" });
    }
    {
        let mut st = STATE.lock();
        if st.running {
            return json!({ "ok": false, "error": "fix all is already running" });
        }
        *st = State {
            running: true,
            started_at: Some(Utc::now()),
            finished_at: None,
            steps: steps.iter().map(|p| Step { path: p, status: "pending", took_sec: None, result: None }).collect(),
            phase: Some("fixing"),
        };
    }
    log::info(cat::FDB, format!("fix all: {} step(s): {}", steps.len(), steps.join(", ")));
    let planned = steps.clone();
    tokio::spawn(async move {
        for (idx, path) in steps.iter().enumerate() {
            let fix = FIXES.iter().find(|f| f.path == *path).copied();
            let Some(fix) = fix else { continue };
            STATE.lock().steps[idx].status = "running";
            let sw = Instant::now();
            let res = tokio::task::spawn_blocking(fix.run).await;
            let took = sw.elapsed().as_secs();
            let mut st = STATE.lock();
            match res {
                Ok(v) => {
                    let ok = v["ok"].as_bool().unwrap_or(true);
                    st.steps[idx] = Step { path: fix.path, status: if ok { "done" } else { "failed" }, took_sec: Some(took), result: Some(v) };
                }
                Err(_) => {
                    st.steps[idx] = Step { path: fix.path, status: "failed", took_sec: Some(took), result: Some(json!({ "ok": false, "error": "migration panicked" })) };
                }
            }
        }
        STATE.lock().phase = Some("checking");
        let _ = tokio::task::spawn_blocking(|| datacheck::run_with_trigger("fixall")).await;
        let mut st = STATE.lock();
        st.running = false;
        st.finished_at = Some(Utc::now());
        st.phase = Some("done");
        let failed = st.steps.iter().filter(|s| s.status == "failed").count();
        log::info(cat::FDB, format!("fix all: finished, steps {} failed {}", st.steps.len(), failed));
    });
    json!({ "ok": true, "steps": planned })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_follows_the_report_in_fixed_order() {
        let report = json!({
            "total": { "zeroSize": 3, "dupIds": 0, "badNames": 7 },
            "trackers": [
                { "tracker": "rutracker", "foreignHost": 2 },
                { "tracker": "knaben", "foreignHost": 0 },
                { "tracker": "toloka", "foreignHost": 5 }
            ],
            "fixes": {
                "zeroSize": "dev/fixzerosizes", "dupIds": "dev/fixslugduplicates", "badNames": "dev/fixrutrackernames",
                "foreignHost": { "rutracker": "dev/fixrutrackerdomainduplicates", "selezen": "dev/fixselezendomainduplicates" }
            }
        });
        assert_eq!(plan(&report), vec!["dev/fixrutrackerdomainduplicates", "dev/fixrutrackernames", "dev/fixzerosizes"]);
        assert!(plan(&json!({ "total": { "zeroSize": 0 } })).is_empty());
    }

    #[test]
    fn status_reports_plan_when_idle() {
        let v = status();
        assert_eq!(v["running"], false);
        assert!(v["plan"].is_array());
    }
}
