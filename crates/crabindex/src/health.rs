// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Health signals for the admin overview and notifications (Telegram / webhook).
//!
//! [`issues`] is cheap (a few small files and in-memory counters) and is recomputed on every
//! overview request. [`spawn_notifier`] re-evaluates every 5 minutes and sends new and
//! resolved signals to the channels configured in `notify:`; a signal that flaps is not
//! repeated more often than `notify.cooldownMinutes`. Every appearance and resolution is also
//! appended to the history (`Data/temp/health_history.json`, see [`history`]), with or without
//! notification channels.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use crab_core::config::AppOptions;
use crab_core::log::{self, cat};
use crab_core::trackers::login_status;
use crab_core::{conf, util};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

/// Trackers that need an account to parse (see the tracker pages in the docs).
pub const AUTH_TRACKERS: [&str; 9] = ["kinozal", "selezen", "anifilm", "mazepa", "toloka", "baibako", "animelayer", "korsars", "rudub"];
/// A tracker this host parses itself with no new torrent for this long is stale.
const STALE_TRACKER_DAYS: i64 = 14;
/// FlareSolverr: minimum browser requests before the failure share counts.
const FS_MIN_REQUESTS: u64 = 20;
const FS_TAB_CRASH_LIMIT: u64 = 20;
/// A container using this share of its memory limit is about to have processes killed.
const CONTAINER_MEM_PERCENT: u64 = 90;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// Stable id, the panel maps it to a title and a text (`issue_<id>_title`).
    pub id: &'static str,
    /// Distinguishes several signals with the same id (tracker slug, host).
    pub key: String,
    /// `warn` or `error`.
    pub severity: &'static str,
    /// Panel route to open.
    pub link: &'static str,
    pub params: Value,
    /// Hidden by the operator (`POST {admin}/api/health/mute`); not notified. Cleared once the
    /// signal disappears, so a signal that comes back later is shown again.
    pub muted: bool,
}

impl Issue {
    fn new(id: &'static str, key: impl Into<String>, severity: &'static str, link: &'static str, params: Value) -> Self {
        Issue { id, key: key.into(), severity, link, params, muted: false }
    }
    pub fn uid(&self) -> String {
        if self.key.is_empty() {
            self.id.to_string()
        } else {
            format!("{}:{}", self.id, self.key)
        }
    }
}

/// Does this host parse `slug` itself (as opposed to taking it from `syncapi`)?
fn parsed_here(c: &AppOptions, slug: &str) -> bool {
    if c.is_tracker_disabled(slug) {
        return false;
    }
    let syncapi = c.syncapi.as_deref().unwrap_or("").trim();
    if syncapi.is_empty() {
        return true;
    }
    match &c.synctrackers {
        Some(list) => !list.iter().any(|t| t.eq_ignore_ascii_case(slug)),
        None => false,
    }
}

fn parse_stats_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%d.%m.%Y").ok()
}

/// Current signals, most severe first.
pub fn issues(c: &AppOptions) -> Vec<Issue> {
    let now = Utc::now();
    let mut out = Vec::new();

    // --- sync ---------------------------------------------------------------------
    let syncapi = c.syncapi.as_deref().unwrap_or("").trim();
    if !syncapi.is_empty() {
        let last = read_checkpoint(crab_ops::sync::cron::LAST_SYNC_PATH).map(crab_core::time::from_file_time_utc);
        let limit_min = (c.timeSync.max(20) as i64 * 2).max(c.timeSync.max(20) as i64 + 30);
        match last {
            Some(t) if !crab_core::time::is_min(&t) => {
                let age = (now - t).num_minutes();
                if age > limit_min {
                    out.push(Issue::new("sync_stale", "", "error", "/", json!({ "minutes": age, "limit": limit_min })));
                }
            }
            _ => {
                // never synced: a fresh install still filling up is fine for a while
                if crab_ops::sync::cron::REMOTE_TORRENTS.load(std::sync::atomic::Ordering::Relaxed) < 0 && uptime_minutes() > 30 {
                    out.push(Issue::new("sync_unreachable", "", "error", "/", json!({ "syncapi": syncapi })));
                }
            }
        }
        if let Some(r) = crab_ops::sync::check::last_report() {
            if r["ok"].as_bool() == Some(false) {
                out.push(Issue::new("sync_check_failed", "", "warn", "/", json!({ "error": r["error"].as_str().unwrap_or("") })));
            } else if r["remaining"].as_i64().unwrap_or(0) > 0 {
                out.push(Issue::new("sync_check_backlog", "", "warn", "/", json!({ "remaining": r["remaining"] })));
            }
        }
    }

    // --- sync host: a known client went silent -------------------------------------------
    if c.opensync && c.syncClientStaleHours > 0 {
        let limit = chrono::Duration::hours(c.syncClientStaleHours as i64);
        for p in crab_ops::sync::peers::list() {
            // CrabIndex clients (version header) or keyed ones; anonymous third-party pulls are not tracked
            if p.version.is_empty() && p.name.is_empty() {
                continue;
            }
            let silent = now - p.lastSeen;
            if silent > limit {
                let who = if p.name.is_empty() { p.ip.clone() } else { p.name.clone() };
                out.push(Issue::new("client_stale", &who, "warn", "/clients", json!({ "client": who, "ip": p.ip, "hours": silent.num_hours(), "limit": c.syncClientStaleHours })));
            }
        }
    }

    // --- trackers: logins ---------------------------------------------------------------
    for slug in AUTH_TRACKERS {
        if !parsed_here(c, slug) {
            continue;
        }
        let Some(ts) = c.tracker(slug) else { continue };
        let configured = !util::is_blank(ts.cookie.as_deref().unwrap_or("")) || !util::is_blank(ts.login_u());
        if !configured {
            out.push(Issue::new("login_missing", slug, "warn", "/settings", json!({ "tracker": slug })));
            continue;
        }
        if let Some(st) = login_status::get(slug) {
            if !st.ok {
                out.push(Issue::new("login_failed", slug, "error", "/trackers", json!({ "tracker": slug, "error": st.error, "at": st.at })));
            }
        }
    }

    // --- trackers: no new torrents for a long time --------------------------------------
    if let Ok(rows) = serde_json::from_str::<Vec<Value>>(&crab_tracks::stats::read_all_json()) {
        for r in rows {
            let Some(slug) = r["trackerName"].as_str() else { continue };
            if !parsed_here(c, slug) || r["alltorrents"].as_i64().unwrap_or(0) == 0 {
                continue;
            }
            let Some(d) = r["lastnewtor"].as_str().and_then(parse_stats_date) else { continue };
            let days = (now.date_naive() - d).num_days();
            if days > STALE_TRACKER_DAYS {
                out.push(Issue::new("tracker_stale", slug, "warn", "/trackers", json!({ "tracker": slug, "days": days })));
            }
        }
    }

    // --- containers (Docker socket mounted, see resources.rs) ------------------------------
    if let Some(r) = crate::resources::last() {
        for ct in r["docker"]["containers"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            if let (Some(u), Some(l)) = (ct["memUsage"].as_u64(), ct["memLimit"].as_u64()) {
                if l > 0 && u * 100 / l >= CONTAINER_MEM_PERCENT {
                    let name = ct["name"].as_str().unwrap_or("");
                    out.push(Issue::new("container_memory", name, "warn", "/", json!({ "name": name, "percent": u * 100 / l, "limitMb": l / 1024 / 1024 })));
                }
            }
        }
    }

    // --- FlareSolverr -------------------------------------------------------------------
    if c.flaresolverr.enable {
        for b in crab_cloudflare::backoff::active() {
            out.push(Issue::new("fs_backoff", &b.host, "warn", "/cloudflare", json!({ "host": b.host, "until": b.until, "level": b.level + 1, "okPercent": (b.ok_ratio * 100.0).round() as i64, "requests": b.window_requests })));
        }
        for h in crab_cloudflare::stats::hosts() {
            if h.tab_crashed >= FS_TAB_CRASH_LIMIT {
                out.push(Issue::new("fs_tab_crashes", &h.host, "warn", "/cloudflare", json!({ "host": h.host, "crashes": h.tab_crashed })));
            } else if h.browser_requests >= FS_MIN_REQUESTS && h.browser_failed * 2 > h.browser_requests && h.fast_ok * 4 < h.fast_ok + h.fast_failed + h.browser_requests {
                // browser failing and the fast path not carrying the load either
                out.push(Issue::new("fs_failing", &h.host, "warn", "/cloudflare", json!({ "host": h.host, "failed": h.browser_failed, "requests": h.browser_requests })));
            }
        }
    }

    // --- WAF: real clients caught by bans -----------------------------------------------
    if c.waf.enable {
        let hit = crate::waf::WAF.stats.affected_clients(now, 24 * 60);
        if !hit.is_empty() {
            let sample: Vec<String> = hit.iter().take(3).filter_map(|v| v["ip"].as_str().map(str::to_string)).collect();
            out.push(Issue::new("waf_users_hit", "", "warn", "/waf/ips", json!({ "count": hit.len(), "sample": sample.join(", ") })));
        }
    }

    // --- data quality (weekly /dev/checkdata) --------------------------------------------
    if let Some(r) = crab_ops::dev::datacheck::last_report() {
        let issues = r["total"]["issues"].as_i64().unwrap_or(0);
        if issues > 0 {
            out.push(Issue::new("data_issues", "", "warn", "/maintenance", json!({ "count": issues, "at": r["at"] })));
        }
    }

    out.sort_by_key(|i| if i.severity == "error" { 0 } else { 1 });
    apply_mutes(&mut out);
    out
}

// ---------------------------------------------------------------- mutes

pub const MUTED_PATH: &str = "Data/temp/health_muted.json";

static MUTED: Lazy<Mutex<Option<std::collections::HashSet<String>>>> = Lazy::new(|| Mutex::new(None));

fn muted_set() -> std::collections::HashSet<String> {
    let mut g = MUTED.lock();
    if g.is_none() {
        let loaded: std::collections::HashSet<String> =
            std::fs::read_to_string(MUTED_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        *g = Some(loaded);
    }
    g.as_ref().cloned().unwrap_or_default()
}

fn save_muted(set: &std::collections::HashSet<String>) {
    let _ = std::fs::create_dir_all("Data/temp");
    let mut v: Vec<&String> = set.iter().collect();
    v.sort();
    if let Ok(s) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(MUTED_PATH, s);
    }
}

/// Hide a signal (`uid` = `id` or `id:key`) until it resolves.
pub fn mute(uid: &str) {
    let mut set = muted_set();
    if set.insert(uid.trim().to_string()) {
        save_muted(&set);
        *MUTED.lock() = Some(set);
    }
}

pub fn unmute(uid: &str) {
    let mut set = muted_set();
    if set.remove(uid.trim()) {
        save_muted(&set);
        *MUTED.lock() = Some(set);
    }
}

/// Mark muted signals and forget mutes of signals that are gone.
fn apply_mutes(issues: &mut [Issue]) {
    let set = muted_set();
    if set.is_empty() {
        return;
    }
    let present: std::collections::HashSet<String> = issues.iter().map(|i| i.uid()).collect();
    for i in issues.iter_mut() {
        i.muted = set.contains(&i.uid());
    }
    let alive: std::collections::HashSet<String> = set.iter().filter(|u| present.contains(*u)).cloned().collect();
    if alive.len() != set.len() {
        save_muted(&alive);
        *MUTED.lock() = Some(alive);
    }
}

fn read_checkpoint(path: &str) -> Option<i64> {
    std::fs::read_to_string(path).ok()?.trim_start_matches('\u{feff}').trim().parse().ok()
}

static STARTED: Lazy<std::time::Instant> = Lazy::new(std::time::Instant::now);

fn uptime_minutes() -> i64 {
    STARTED.elapsed().as_secs() as i64 / 60
}

// ---------------------------------------------------------------- history

pub const HISTORY_PATH: &str = "Data/temp/health_history.json";
const HISTORY_MAX: usize = 1000;
const HISTORY_DAYS: i64 = 90;

/// One history row: a signal appeared or went away.
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub at: DateTime<Utc>,
    /// `appeared`, `resolved`, `muted` or `unmuted`.
    pub event: String,
    pub id: String,
    pub key: String,
    pub severity: String,
    pub params: Value,
    /// For `resolved`: how long the signal was active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<i64>,
}

/// Event log with an in-memory copy; the file is read once and rewritten on every change.
pub struct HistoryStore {
    path: String,
    cache: Mutex<Option<Vec<Event>>>,
}

static HISTORY: Lazy<HistoryStore> = Lazy::new(|| HistoryStore::new(HISTORY_PATH));

impl HistoryStore {
    pub fn new(path: impl Into<String>) -> Self {
        HistoryStore { path: path.into(), cache: Mutex::new(None) }
    }

    fn all(&self) -> Vec<Event> {
        let mut g = self.cache.lock();
        if g.is_none() {
            let loaded: Vec<Event> = std::fs::read_to_string(&self.path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            *g = Some(loaded);
        }
        g.clone().unwrap_or_default()
    }

    fn save(&self, list: &[Event]) {
        *self.cache.lock() = Some(list.to_vec());
        if let Ok(s) = serde_json::to_string(list) {
            if let Some(dir) = std::path::Path::new(&self.path).parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&self.path, s);
        }
    }

    /// Newest first, at most `limit` rows.
    pub fn history(&self, limit: usize) -> Vec<Event> {
        let mut list = self.all();
        list.reverse();
        list.truncate(limit);
        list
    }

    /// [`record_all`](Self::record_all) without mute rows.
    #[cfg(test)]
    pub fn record(&self, new: &[Issue], resolved: &[Issue], now: DateTime<Utc>) {
        self.record_all(new, resolved, &[], &[], now)
    }

    /// Append `appeared` rows for `new`, `resolved` rows for `resolved` (with the active duration
    /// taken from the matching `appeared` row) and `muted` / `unmuted` rows for signals the
    /// operator hid or showed again while they stayed active; then trim to 90 days / 1000 rows.
    pub fn record_all(&self, new: &[Issue], resolved: &[Issue], muted: &[Issue], unmuted: &[Issue], now: DateTime<Utc>) {
        if new.is_empty() && resolved.is_empty() && muted.is_empty() && unmuted.is_empty() {
            return;
        }
        let mut list = self.all();
        for (event, items) in [("muted", muted), ("unmuted", unmuted)] {
            for i in items {
                list.push(Event { at: now, event: event.into(), id: i.id.into(), key: i.key.clone(), severity: i.severity.into(), params: i.params.clone(), minutes: None });
            }
        }
        for i in resolved {
            let since = list.iter().rev().find(|e| e.event == "appeared" && e.id == i.id && e.key == i.key).map(|e| e.at);
            list.push(Event {
                at: now,
                event: "resolved".into(),
                id: i.id.into(),
                key: i.key.clone(),
                severity: i.severity.into(),
                params: i.params.clone(),
                minutes: since.map(|t| (now - t).num_minutes().max(0)),
            });
        }
        for i in new {
            list.push(Event { at: now, event: "appeared".into(), id: i.id.into(), key: i.key.clone(), severity: i.severity.into(), params: i.params.clone(), minutes: None });
        }
        let cutoff = now - chrono::Duration::days(HISTORY_DAYS);
        list.retain(|e| e.at >= cutoff);
        if list.len() > HISTORY_MAX {
            let drop = list.len() - HISTORY_MAX;
            list.drain(..drop);
        }
        self.save(&list);
    }
}

/// Newest first, at most `limit` rows (`GET {admin}/api/health/history`).
pub fn history(limit: usize) -> Vec<Event> {
    HISTORY.history(limit)
}

pub fn record_history(new: &[Issue], resolved: &[Issue], muted: &[Issue], unmuted: &[Issue], now: DateTime<Utc>) {
    HISTORY.record_all(new, resolved, muted, unmuted, now)
}

// ---------------------------------------------------------------- notifications

struct NotifyState {
    /// Non-muted signals seen last time (notifications).
    active: HashMap<String, Issue>,
    /// uid → when a message about it was last sent (cooldown).
    sent_at: HashMap<String, DateTime<Utc>>,
    /// Every signal seen last time, muted ones included, with its muted flag (history).
    seen: HashMap<String, (Issue, bool)>,
}

static STATE: Lazy<Mutex<NotifyState>> = Lazy::new(|| Mutex::new(NotifyState { active: HashMap::new(), sent_at: HashMap::new(), seen: HashMap::new() }));

/// History diff over all signals: `(appeared, resolved, muted, unmuted)`. A signal hidden by
/// the operator stays "active" here, so hiding is not mistaken for the problem going away.
fn diff_all(state: &mut NotifyState, all: &[Issue]) -> (Vec<Issue>, Vec<Issue>, Vec<Issue>, Vec<Issue>) {
    let cur: HashMap<String, (Issue, bool)> = all.iter().map(|i| (i.uid(), (i.clone(), i.muted))).collect();
    let (mut appeared, mut muted, mut unmuted) = (Vec::new(), Vec::new(), Vec::new());
    for (uid, (i, m)) in &cur {
        match state.seen.get(uid) {
            None => appeared.push(i.clone()),
            Some((_, was)) if was != m => {
                if *m {
                    muted.push(i.clone())
                } else {
                    unmuted.push(i.clone())
                }
            }
            _ => {}
        }
    }
    let resolved: Vec<Issue> = state.seen.iter().filter(|(uid, _)| !cur.contains_key(*uid)).map(|(_, (i, _))| i.clone()).collect();
    state.seen = cur;
    (appeared, resolved, muted, unmuted)
}

/// Human text for a signal (notifications are plain text; the panel has its own translations).
pub fn describe(i: &Issue) -> String {
    let p = &i.params;
    match i.id {
        "sync_stale" => format!("Синхронизация не проходила {} мин (порог {} мин)", p["minutes"], p["limit"]),
        "sync_unreachable" => format!("Сервер синхронизации {} не отвечает", p["syncapi"].as_str().unwrap_or("")),
        "sync_check_failed" => format!("Сверка с сервером не удалась: {}", p["error"].as_str().unwrap_or("")),
        "sync_check_backlog" => format!("Сверка не завершена, осталось бакетов: {}", p["remaining"]),
        "login_missing" => format!("{}: нужен логин или cookie, иначе раздачи не парсятся", p["tracker"].as_str().unwrap_or("")),
        "login_failed" => format!("{}: вход не удался ({})", p["tracker"].as_str().unwrap_or(""), p["error"].as_str().unwrap_or("")),
        "tracker_stale" => format!("{}: нет новых раздач {} дней", p["tracker"].as_str().unwrap_or(""), p["days"]),
        "fs_tab_crashes" => format!("FlareSolverr: у {} упало вкладок: {}", p["host"].as_str().unwrap_or(""), p["crashes"]),
        "fs_failing" => format!("FlareSolverr: {} не проходит проверку ({} из {} запросов)", p["host"].as_str().unwrap_or(""), p["failed"], p["requests"]),
        "waf_users_hit" => format!("WAF: под бан попали клиенты поиска: {} ({})", p["count"], p["sample"].as_str().unwrap_or("")),
        "data_issues" => format!("Проверка данных нашла записей к исправлению: {}", p["count"]),
        "container_memory" => format!("Контейнер {} занял {}% лимита памяти ({} МБ)", p["name"].as_str().unwrap_or(""), p["percent"], p["limitMb"]),
        "client_stale" => format!("Клиент sync {} не приходил {} ч (порог {} ч)", p["client"].as_str().unwrap_or(""), p["hours"], p["limit"]),
        "fs_backoff" => format!("FlareSolverr: {} на паузе до {} (успешных {}% из {})", p["host"].as_str().unwrap_or(""), p["until"].as_str().map(|u| &u[11..16]).unwrap_or(""), p["okPercent"], p["requests"]),
        "test_message" => "Тестовое уведомление CrabIndex".to_string(),
        other => format!("{other}: {p}"),
    }
}

/// Diff against the previous evaluation: `(appeared, notify, resolved)`. `appeared` is every
/// signal not active last time (history); `notify` is the subset outside the cooldown.
fn diff(state: &mut NotifyState, current: &[Issue], now: DateTime<Utc>, cooldown_min: i64) -> (Vec<Issue>, Vec<Issue>, Vec<Issue>) {
    let cur: HashMap<String, Issue> = current.iter().map(|i| (i.uid(), i.clone())).collect();
    let mut appeared = Vec::new();
    let mut notify = Vec::new();
    for (uid, i) in &cur {
        if state.active.contains_key(uid) {
            continue;
        }
        appeared.push(i.clone());
        let recently = state.sent_at.get(uid).map(|t| (now - *t).num_minutes() < cooldown_min).unwrap_or(false);
        if !recently {
            notify.push(i.clone());
            state.sent_at.insert(uid.clone(), now);
        }
    }
    let resolved: Vec<Issue> = state.active.iter().filter(|(uid, _)| !cur.contains_key(*uid)).map(|(_, i)| i.clone()).collect();
    state.active = cur;
    (appeared, notify, resolved)
}

fn host_label(c: &AppOptions) -> String {
    let h = std::env::var("HOSTNAME").unwrap_or_default();
    if h.trim().is_empty() {
        format!("{}:{}", c.listenip, c.listenport)
    } else {
        h
    }
}

async fn send_telegram(token: &str, chat_id: &str, text: &str) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let r = client.post(&url).json(&json!({ "chat_id": chat_id, "text": text, "disable_web_page_preview": true })).send().await.map_err(|e| e.to_string())?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!("telegram HTTP {}", r.status()))
    }
}

async fn send_webhook(url: &str, body: &Value) -> Result<(), String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let r = client.post(url).json(body).send().await.map_err(|e| e.to_string())?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!("webhook HTTP {}", r.status()))
    }
}

/// Send one message about `new` / `resolved` to every configured channel. Returns per-channel errors.
pub async fn notify(c: &AppOptions, new: &[Issue], resolved: &[Issue], active: &[Issue]) -> Vec<String> {
    let n = &c.notify;
    let mut errors = Vec::new();
    let host = host_label(c);
    let telegram = !util::is_blank(&n.telegramToken) && !util::is_blank(&n.telegramChatId);
    let webhook = !util::is_blank(&n.webhookUrl);
    if !telegram && !webhook {
        return vec!["no channel configured (notify.telegramToken + telegramChatId or notify.webhookUrl)".into()];
    }
    if telegram {
        let mut text = format!("CrabIndex · {host}\n");
        for i in new {
            text.push_str(&format!("{} {}\n", if i.severity == "error" { "🔴" } else { "🟡" }, describe(i)));
        }
        for i in resolved {
            text.push_str(&format!("✅ Закрыто: {}\n", describe(i)));
        }
        if let Err(e) = send_telegram(&n.telegramToken, &n.telegramChatId, text.trim_end()).await {
            errors.push(e);
        }
    }
    if webhook {
        let body = json!({
            "host": host,
            "at": Utc::now(),
            "new": new.iter().map(|i| json!({ "id": i.id, "key": i.key, "severity": i.severity, "params": i.params, "text": describe(i) })).collect::<Vec<_>>(),
            "resolved": resolved.iter().map(|i| json!({ "id": i.id, "key": i.key, "text": describe(i) })).collect::<Vec<_>>(),
            "active": active.iter().map(|i| json!({ "id": i.id, "key": i.key, "severity": i.severity })).collect::<Vec<_>>(),
        });
        if let Err(e) = send_webhook(&n.webhookUrl, &body).await {
            errors.push(e);
        }
    }
    errors
}

/// Admin panel "send test": a synthetic signal to every channel.
pub async fn send_test(c: &AppOptions) -> Vec<String> {
    let probe = Issue::new("test", "", "warn", "/", json!({}));
    let mut errs = notify(c, &[], &[], &[]).await;
    if errs.iter().any(|e| e.starts_with("no channel")) {
        return errs;
    }
    errs.clear();
    let text_issue = Issue { id: "test_message", ..probe };
    errs.extend(notify(c, &[text_issue], &[], &[]).await);
    errs
}

/// Non-muted signals as `(errors, warnings)`, for the status the sync client reports to its host.
pub fn counts() -> (usize, usize) {
    let c = conf();
    let list = issues(&c);
    let errors = list.iter().filter(|i| !i.muted && i.severity == "error").count();
    let warns = list.iter().filter(|i| !i.muted && i.severity != "error").count();
    (errors, warns)
}

/// Every signal id the checks can produce; history rows are mapped back to these.
const ISSUE_IDS: [&str; 14] = [
    "sync_stale", "sync_unreachable", "sync_check_failed", "sync_check_backlog", "login_missing", "login_failed", "tracker_stale",
    "fs_tab_crashes", "fs_failing", "waf_users_hit", "data_issues", "container_memory", "client_stale", "fs_backoff",
];

/// Signals whose last history row is `appeared`: still active when the process stopped. Seeding
/// the notifier with them keeps a restart from logging (and notifying) every open signal again;
/// the ones that are gone get a proper `resolved` row instead.
fn open_from_history() -> HashMap<String, (Issue, bool)> {
    // newest row per signal decides whether it is still open; `muted` / `unmuted` rows keep it
    // open and only set the flag
    let mut last: HashMap<String, Event> = HashMap::new();
    for e in HISTORY.history(HISTORY_MAX) {
        let uid = if e.key.is_empty() { e.id.clone() } else { format!("{}:{}", e.id, e.key) };
        last.entry(uid).or_insert(e);
    }
    last.into_iter()
        .filter(|(_, e)| e.event != "resolved")
        .filter_map(|(uid, e)| {
            let id = ISSUE_IDS.iter().find(|x| **x == e.id)?;
            let severity = if e.severity == "error" { "error" } else { "warn" };
            let muted = e.event == "muted";
            let mut issue = Issue::new(id, e.key, severity, "/", e.params);
            issue.muted = muted;
            Some((uid, (issue, muted)))
        })
        .collect()
}

pub fn spawn_notifier(ct: CancellationToken) {
    crab_core::hooks::register_health_counts(counts);
    crab_core::hooks::register_app_version(crate::version::VERSION);
    tokio::spawn(async move {
        if let Ok(open) = tokio::task::spawn_blocking(open_from_history).await {
            let mut st = STATE.lock();
            if st.seen.is_empty() {
                st.active = open.iter().filter(|(_, (_, m))| !m).map(|(u, (i, _))| (u.clone(), i.clone())).collect();
                st.seen = open;
            }
        }
        tokio::select! {
            _ = ct.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_secs(120)) => {}
        }
        loop {
            let c = conf();
            let all = issues(&c);
            let current: Vec<Issue> = all.iter().filter(|i| !i.muted).cloned().collect();
            let now = Utc::now();
            // notifications: non-muted signals with the cooldown; history: every signal
            let (new, resolved, h) = {
                let mut st = STATE.lock();
                let (_, new, resolved) = diff(&mut st, &current, now, c.notify.cooldownMinutes.max(0) as i64);
                let h = diff_all(&mut st, &all);
                (new, resolved, h)
            };
            let (appeared, gone, muted, unmuted) = h;
            if !appeared.is_empty() || !gone.is_empty() || !muted.is_empty() || !unmuted.is_empty() {
                for i in &appeared {
                    log::warn(cat::HOST, format!("health: {}", describe(i)));
                }
                for i in &gone {
                    log::info(cat::HOST, format!("health: resolved - {}", describe(i)));
                }
                for i in &muted {
                    log::info(cat::HOST, format!("health: muted - {}", describe(i)));
                }
                for i in &unmuted {
                    log::info(cat::HOST, format!("health: shown again - {}", describe(i)));
                }
                let _ = tokio::task::spawn_blocking(move || record_history(&appeared, &gone, &muted, &unmuted, now)).await;
            }
            if c.notify.enable && (!new.is_empty() || !resolved.is_empty()) {
                let has_channel = !util::is_blank(&c.notify.webhookUrl) || (!util::is_blank(&c.notify.telegramToken) && !util::is_blank(&c.notify.telegramChatId));
                if has_channel {
                    for e in notify(&c, &new, &resolved, &current).await {
                        log::warn(cat::HOST, format!("health: notification failed: {e}"));
                    }
                }
            }
            tokio::select! {
                _ = ct.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(300)) => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(id: &'static str, key: &str) -> Issue {
        Issue::new(id, key, "warn", "/", json!({}))
    }

    #[test]
    fn diff_reports_new_and_resolved_with_cooldown() {
        let t0 = Utc::now();
        let mut st = NotifyState { active: HashMap::new(), sent_at: HashMap::new(), seen: HashMap::new() };
        let (a, n, r) = diff(&mut st, &[issue("a", ""), issue("b", "x")], t0, 60);
        assert_eq!((a.len(), n.len(), r.len()), (2, 2, 0));
        let (a, n, r) = diff(&mut st, &[issue("a", ""), issue("b", "x")], t0, 60);
        assert_eq!((a.len(), n.len(), r.len()), (0, 0, 0));
        let (a, n, r) = diff(&mut st, &[issue("a", "")], t0, 60);
        assert_eq!((a.len(), n.len()), (0, 0));
        assert_eq!(r[0].uid(), "b:x");
        // Flapping inside the cooldown: shown in history (appeared) but not notified.
        let (a, n, _) = diff(&mut st, &[issue("a", ""), issue("b", "x")], t0 + chrono::Duration::minutes(10), 60);
        assert_eq!((a.len(), n.len()), (1, 0));
        let (_, _, _) = diff(&mut st, &[issue("a", "")], t0 + chrono::Duration::minutes(20), 60);
        let (a, n, _) = diff(&mut st, &[issue("a", ""), issue("b", "x")], t0 + chrono::Duration::minutes(90), 60);
        assert_eq!((a.len(), n.len()), (1, 1));
    }

    #[test]
    fn history_diff_tells_muting_from_resolving() {
        let mut st = NotifyState { active: HashMap::new(), sent_at: HashMap::new(), seen: HashMap::new() };
        let (a, r, m, u) = diff_all(&mut st, &[issue("a", ""), issue("b", "x")]);
        assert_eq!((a.len(), r.len(), m.len(), u.len()), (2, 0, 0, 0));
        let mut hidden = issue("b", "x");
        hidden.muted = true;
        let (a, r, m, u) = diff_all(&mut st, &[issue("a", ""), hidden.clone()]);
        assert_eq!((a.len(), r.len(), m.len(), u.len()), (0, 0, 1, 0));
        assert_eq!(m[0].uid(), "b:x");
        let (a, r, m, u) = diff_all(&mut st, &[issue("a", ""), issue("b", "x")]);
        assert_eq!((a.len(), r.len(), m.len(), u.len()), (0, 0, 0, 1));
        let (a, r, _, _) = diff_all(&mut st, &[issue("a", "")]);
        assert_eq!((a.len(), r.len()), (0, 1));
        assert_eq!(r[0].uid(), "b:x");
        // a hidden signal that stays hidden across ticks is neither new nor resolved
        let (a, r, m, u) = diff_all(&mut st, &[issue("a", ""), hidden.clone()]);
        assert_eq!((a.len(), r.len(), m.len(), u.len()), (1, 0, 0, 0));
        let (a, r, m, u) = diff_all(&mut st, &[issue("a", ""), hidden]);
        assert_eq!((a.len(), r.len(), m.len(), u.len()), (0, 0, 0, 0));
    }

    #[test]
    fn open_signals_come_back_from_history() {
        let path = std::env::temp_dir().join(format!("crab_health_open_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = HistoryStore::new(path.to_string_lossy().to_string());
        let t0 = Utc::now() - chrono::Duration::minutes(10);
        store.record(&[issue("login_missing", "mazepa"), issue("data_issues", ""), issue("unknown_id", "")], &[], t0);
        store.record(&[], &[issue("data_issues", "")], t0 + chrono::Duration::minutes(5));
        let mut last: HashMap<String, Event> = HashMap::new();
        for e in store.history(HISTORY_MAX) {
            let uid = if e.key.is_empty() { e.id.clone() } else { format!("{}:{}", e.id, e.key) };
            last.entry(uid).or_insert(e);
        }
        let open: Vec<String> = last.iter().filter(|(_, e)| e.event != "resolved" && ISSUE_IDS.contains(&e.id.as_str())).map(|(u, _)| u.clone()).collect();
        assert_eq!(open, vec!["login_missing:mazepa".to_string()]);
        // a muted row keeps the signal open with the flag set
        store.record_all(&[], &[], &[issue("login_missing", "mazepa")], &[], t0 + chrono::Duration::minutes(6));
        let newest = store.history(1).remove(0);
        assert_eq!((newest.event.as_str(), newest.key.as_str()), ("muted", "mazepa"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn history_records_duration_and_trims() {
        let path = std::env::temp_dir().join(format!("crab_health_history_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = HistoryStore::new(path.to_string_lossy().to_string());
        let t0 = Utc::now() - chrono::Duration::minutes(45);
        store.record(&[issue("a", ""), issue("b", "x")], &[], t0);
        store.record(&[], &[issue("b", "x")], t0 + chrono::Duration::minutes(30));
        let h = store.history(10);
        assert_eq!(h.len(), 3);
        assert_eq!((h[0].event.as_str(), h[0].id.as_str(), h[0].minutes), ("resolved", "b", Some(30)));
        assert_eq!(h[2].event, "appeared");
        // Reload from disk.
        let again = HistoryStore::new(path.to_string_lossy().to_string());
        assert_eq!(again.history(10).len(), 3);
        assert_eq!(again.history(1).len(), 1);
        // Old rows are dropped on the next write.
        let mut list = again.all();
        list[0].at = Utc::now() - chrono::Duration::days(HISTORY_DAYS + 1);
        again.save(&list);
        again.record(&[issue("c", "")], &[], Utc::now());
        assert_eq!(again.history(10).len(), 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mutes_follow_signals() {
        let _guard = std::env::temp_dir();
        *MUTED.lock() = Some(std::collections::HashSet::new());
        let mut list = vec![issue("a", ""), issue("b", "x")];
        MUTED.lock().as_mut().unwrap().insert("b:x".into());
        MUTED.lock().as_mut().unwrap().insert("gone:z".into());
        apply_mutes(&mut list);
        assert_eq!(list.iter().map(|i| i.muted).collect::<Vec<_>>(), vec![false, true]);
        // the mute of a signal that is not present any more is forgotten
        assert_eq!(muted_set().len(), 1);
        assert!(muted_set().contains("b:x"));
    }

    #[test]
    fn parsed_here_follows_sync_settings() {
        let mut c = AppOptions::default();
        assert!(parsed_here(&c, "kinozal"));
        c.syncapi = Some("https://sync.example".into());
        assert!(!parsed_here(&c, "kinozal"));
        c.synctrackers = Some(vec!["rutor".into()]);
        assert!(parsed_here(&c, "kinozal"));
        assert!(!parsed_here(&c, "rutor"));
        c.disable_trackers = vec!["kinozal".into()];
        assert!(!parsed_here(&c, "kinozal"));
    }

    #[test]
    fn describe_covers_every_id() {
        for id in ["sync_stale", "sync_unreachable", "sync_check_failed", "sync_check_backlog", "login_missing", "login_failed", "tracker_stale", "fs_tab_crashes", "fs_failing", "waf_users_hit", "data_issues", "container_memory", "client_stale", "fs_backoff"] {
            let i = Issue::new(id, "k", "warn", "/", json!({ "minutes": 1, "limit": 2, "syncapi": "s", "error": "e", "remaining": 3, "tracker": "t", "days": 4, "host": "h", "crashes": 5, "failed": 6, "requests": 7 }));
            assert!(!describe(&i).contains(id), "{id} should read as text");
        }
        assert_eq!(parse_stats_date("27.09.2026"), NaiveDate::from_ymd_opt(2026, 9, 27));
    }
}
