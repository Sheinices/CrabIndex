//! TorrServer client used by the analyzer: `POST /torrents` (list/add/get/rem) and `GET /ffp/{HASH}/{id}`,
//! plus server selection, per-hash locks and the global analysis concurrency gate.

use crab_core::conf;
use crab_core::models::FfprobeModel;
use dashmap::{DashMap, DashSet};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rand::Rng;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use super::logging::tlog;
use super::models::{self, TorrentInfo};

/// The caller's token was cancelled (host shutdown or overall analysis timeout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// `/ffp` request failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfpError {
    /// Caller token cancelled.
    Cancelled,
    /// Per-request timeout elapsed.
    Timeout,
    /// Transport error.
    Http(String),
    /// Body is not valid ffprobe JSON.
    Json(String),
}

pub(crate) enum Outcome<T> {
    Done(T),
    Timeout,
    Cancelled,
}

/// Run `fut` bounded by `dur` and the caller's token.
pub(crate) async fn guarded<F: Future>(token: &CancellationToken, dur: Duration, fut: F) -> Outcome<F::Output> {
    tokio::select! {
        biased;
        _ = token.cancelled() => Outcome::Cancelled,
        r = tokio::time::timeout(dur, fut) => match r {
            Ok(v) => Outcome::Done(v),
            Err(_) => Outcome::Timeout,
        },
    }
}

/// Sleep that ends early with `Err(Cancelled)` when the token fires.
pub async fn sleep(dur: Duration, token: &CancellationToken) -> Result<(), Cancelled> {
    tokio::select! {
        biased;
        _ = token.cancelled() => Err(Cancelled),
        _ = tokio::time::sleep(dur) => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Timeouts from config
// ---------------------------------------------------------------------------

fn secs(v: i32) -> Duration {
    Duration::from_secs(v.max(1) as u64)
}

pub fn tracks_read_timeout() -> Duration {
    secs(conf().tracksreadtimeout)
}

pub fn tracks_peer_wait_timeout() -> Duration {
    secs(conf().trackspeerwaittimeout)
}

/// `/ffp` timeout: short when the torrent has data but no connected seeders, else by `sid`.
pub fn ffp_timeout(sid: i32, peer_info: Option<&TorrentInfo>) -> Duration {
    let c = conf();
    if let Some(p) = peer_info {
        if p.connected_seeders == 0 && p.bytes_read > 0 {
            return secs(c.tracksffptimeoutnosid);
        }
    }
    secs(if sid > 0 { c.tracksffptimeout } else { c.tracksffptimeoutnosid })
}

pub fn ffp_retry_extra() -> usize {
    conf().tracksffpretry.max(0) as usize
}

pub fn min_buffer_bytes() -> i64 {
    (conf().tracksminbufferkb as i64 * 1024).max(0)
}

pub fn analyze_overall_timeout(sid: i32) -> Duration {
    let extra = ffp_retry_extra() as u32;
    tracks_read_timeout() + tracks_peer_wait_timeout() * 2 + ffp_timeout(sid, None) * (1 + extra) + Duration::from_secs(30)
}

fn has_download_progress(info: Option<&TorrentInfo>) -> bool {
    info.map(|i| i.bytes_read > 0 || i.connected_seeders > 0).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Hash locks / in-flight hashes / concurrency gate
// ---------------------------------------------------------------------------

static HASH_LOCKS: Lazy<DashSet<String>> = Lazy::new(DashSet::new);
static IN_FLIGHT_HASHES: Lazy<DashSet<String>> = Lazy::new(DashSet::new);

/// Held while a hash is being analyzed; released on drop.
pub struct HashLock {
    key: Option<String>,
}

impl Drop for HashLock {
    fn drop(&mut self) {
        if let Some(k) = self.key.take() {
            HASH_LOCKS.remove(&k);
        }
    }
}

/// `None` when the same hash is already being analyzed (non-blocking).
pub fn try_acquire_hash_lock(infohash: &str) -> Option<HashLock> {
    if infohash.is_empty() {
        return Some(HashLock { key: None });
    }
    let key = infohash.to_lowercase();
    HASH_LOCKS.insert(key.clone()).then(|| HashLock { key: Some(key) })
}

pub fn get_in_flight_hashes() -> HashSet<String> {
    IN_FLIGHT_HASHES.iter().map(|h| h.clone()).collect()
}

pub fn register_in_flight(infohash: &str) {
    if !infohash.is_empty() {
        IN_FLIGHT_HASHES.insert(infohash.to_lowercase());
    }
}

pub fn unregister_in_flight(infohash: &str) {
    if !infohash.is_empty() {
        IN_FLIGHT_HASHES.remove(&infohash.to_lowercase());
    }
}

static ANALYZE_GATE: Lazy<Mutex<(Arc<Semaphore>, usize)>> = Lazy::new(|| Mutex::new((Arc::new(Semaphore::new(2)), 0)));

fn analyze_semaphore() -> Arc<Semaphore> {
    let limit = conf().tracksconcurrency.max(1) as usize;
    let mut g = ANALYZE_GATE.lock();
    if g.1 != limit {
        // Holders of the previous semaphore keep their permits until they finish.
        *g = (Arc::new(Semaphore::new(limit)), limit);
    }
    g.0.clone()
}

/// One of `tracksconcurrency` analysis slots.
pub async fn acquire_analyze_slot() -> Option<OwnedSemaphorePermit> {
    analyze_semaphore().acquire_owned().await.ok()
}

// ---------------------------------------------------------------------------
// HTTP clients
// ---------------------------------------------------------------------------

struct TsClient {
    client: reqwest::Client,
    base: String,
    auth: Option<(String, String)>,
}

static CLIENTS: Lazy<DashMap<String, Arc<TsClient>>> = Lazy::new(DashMap::new);

/// `scheme://authority/path` without trailing slash (lowercased: keys are case-insensitive).
pub fn normalize_tsuri_key(tsuri: &str) -> String {
    match url::Url::parse(tsuri) {
        Ok(u) => {
            let mut authority = String::new();
            if !u.username().is_empty() {
                authority.push_str(u.username());
                if let Some(p) = u.password() {
                    authority.push(':');
                    authority.push_str(p);
                }
                authority.push('@');
            }
            authority.push_str(u.host_str().unwrap_or(""));
            if let Some(p) = u.port() {
                authority.push_str(&format!(":{p}"));
            }
            format!("{}://{}{}", u.scheme(), authority, u.path()).trim_end_matches('/').to_lowercase()
        }
        Err(_) => tsuri.to_lowercase(),
    }
}

/// Split `user:pass@` out of the URL (sent as Basic auth instead).
fn split_userinfo(tsuri: &str) -> (String, Option<(String, String)>) {
    let Some(scheme_end) = tsuri.find("://") else { return (tsuri.to_string(), None) };
    let rest = &tsuri[scheme_end + 3..];
    let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let Some(at) = authority.rfind('@') else { return (tsuri.to_string(), None) };
    let userinfo = &authority[..at];
    let base = format!("{}{}{}", &tsuri[..scheme_end + 3], &authority[at + 1..], &rest[auth_end..]);
    let parts: Vec<&str> = userinfo.split(':').collect();
    let auth = (parts.len() == 2).then(|| (parts[0].to_string(), parts[1].to_string()));
    (base, auth)
}

fn ts_client(tsuri: &str) -> Arc<TsClient> {
    let key = normalize_tsuri_key(tsuri);
    if let Some(c) = CLIENTS.get(&key) {
        return c.clone();
    }
    let (base, auth) = split_userinfo(tsuri);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::ACCEPT, reqwest::header::HeaderValue::from_static("application/json"));
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .no_proxy()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let c = Arc::new(TsClient { client, base, auth });
    CLIENTS.entry(key).or_insert(c).clone()
}

impl TsClient {
    fn post_torrents(&self, body: serde_json::Value) -> reqwest::RequestBuilder {
        let mut rb = self
            .client
            .post(format!("{}/torrents", self.base))
            .header(reqwest::header::CONTENT_TYPE, "application/json; charset=utf-8")
            .body(body.to_string());
        if let Some((u, p)) = &self.auth {
            rb = rb.basic_auth(u, Some(p));
        }
        rb
    }

    fn get(&self, url: String) -> reqwest::RequestBuilder {
        let mut rb = self.client.get(url);
        if let Some((u, p)) = &self.auth {
            rb = rb.basic_auth(u, Some(p));
        }
        rb
    }
}

// ---------------------------------------------------------------------------
// Torrent list (short TTL cache) + server selection
// ---------------------------------------------------------------------------

const LIST_CACHE_TTL: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct ListCacheEntry {
    fetched_at: Instant,
    torrents: Option<Arc<Vec<TorrentInfo>>>,
    server_error: bool,
}

static LIST_CACHE: Lazy<Mutex<HashMap<String, ListCacheEntry>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static IN_FLIGHT_COUNTS: Lazy<Mutex<HashMap<String, i32>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn invalidate_list_cache(tsuri: &str) {
    LIST_CACHE.lock().remove(&normalize_tsuri_key(tsuri));
}

pub type ListResult = (Option<Arc<Vec<TorrentInfo>>>, bool);

/// TorrServer torrent list (cached 10s unless `force_refresh`). `(torrents, server_error)`.
pub async fn get_torrent_list(tsuri: &str, token: &CancellationToken, force_refresh: bool) -> Result<ListResult, Cancelled> {
    let key = normalize_tsuri_key(tsuri);
    if !force_refresh {
        if let Some(c) = LIST_CACHE.lock().get(&key) {
            if c.fetched_at.elapsed() < LIST_CACHE_TTL {
                return Ok((c.torrents.clone(), c.server_error));
            }
        }
    }

    let client = ts_client(tsuri);
    let fut = async {
        let resp = client.post_torrents(json!({ "action": "list" })).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let text = resp.text().await.ok()?;
        let list = serde_json::from_str::<Option<Vec<TorrentInfo>>>(&text).ok()?;
        Some(list.unwrap_or_default())
    };
    let torrents = match guarded(token, Duration::from_secs(10), fut).await {
        Outcome::Cancelled => return Err(Cancelled),
        Outcome::Timeout => None,
        Outcome::Done(v) => v,
    };
    let entry = ListCacheEntry {
        fetched_at: Instant::now(),
        server_error: torrents.is_none(),
        torrents: torrents.map(Arc::new),
    };
    LIST_CACHE.lock().insert(key, entry.clone());
    Ok((entry.torrents, entry.server_error))
}

pub fn find_torrent_in_list<'a>(torrents: &'a [TorrentInfo], infohash: &str) -> Option<&'a TorrentInfo> {
    if infohash.is_empty() {
        return None;
    }
    let lower = infohash.to_lowercase();
    torrents.iter().find(|t| {
        t.hash.as_deref().map(|h| !h.is_empty() && h.eq_ignore_ascii_case(infohash)).unwrap_or(false)
            || t.name.as_deref().map(|n| !n.is_empty() && n.to_lowercase().ends_with(&lower)).unwrap_or(false)
    })
}

fn category_matches(t: &TorrentInfo, expected: &str) -> bool {
    t.category.as_deref().map(|c| !c.is_empty() && c.eq_ignore_ascii_case(expected)).unwrap_or(false)
}

/// Server with the fewest torrents in `trackscategory` (in-flight picks included, random tiebreak).
pub async fn select_best_server(token: &CancellationToken) -> Result<Option<String>, Cancelled> {
    let c = conf();
    if c.tsuri.is_empty() {
        return Ok(None);
    }
    let expected = c.trackscategory.clone();
    let futs = c.tsuri.iter().map(|server| {
        let expected = expected.clone();
        async move {
            let (torrents, err) = get_torrent_list(server, token, false).await?;
            Ok::<_, Cancelled>(match torrents {
                Some(list) if !err => Some((server.clone(), list.iter().filter(|t| category_matches(t, &expected)).count() as i64)),
                _ => None,
            })
        }
    });
    let results = futures::future::join_all(futs).await;
    let mut valid = Vec::new();
    for r in results {
        if let Some(v) = r? {
            valid.push(v);
        }
    }
    if valid.is_empty() {
        return Ok(None);
    }
    let mut counts = IN_FLIGHT_COUNTS.lock();
    let mut rng = rand::thread_rng();
    let mut ranked: Vec<(i64, u32, String)> = valid
        .into_iter()
        .map(|(s, n)| (n + *counts.get(&normalize_tsuri_key(&s)).unwrap_or(&0) as i64, rng.gen::<u32>(), s))
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let chosen = ranked.swap_remove(0).2;
    *counts.entry(normalize_tsuri_key(&chosen)).or_insert(0) += 1;
    Ok(Some(chosen))
}

pub fn release_in_flight(tsuri: &str) {
    let mut counts = IN_FLIGHT_COUNTS.lock();
    let e = counts.entry(normalize_tsuri_key(tsuri)).or_insert(0);
    *e = (*e - 1).max(0);
}

/// `(exists, category, server_error)`.
async fn check_torrent_exists_with_category(
    tsuri: &str,
    infohash: &str,
    token: &CancellationToken,
    typetask: Option<i32>,
) -> Result<(bool, Option<String>, bool), Cancelled> {
    let (torrents, server_error) = get_torrent_list(tsuri, token, false).await?;
    if server_error {
        tlog("Сервер вернул ошибку при запросе списка торрентов", typetask);
        return Ok((false, None, true));
    }
    let Some(torrents) = torrents else {
        tlog("Получен пустой список торрентов", typetask);
        return Ok((false, None, false));
    };
    if infohash.is_empty() {
        return Ok((false, None, false));
    }
    Ok(match find_torrent_in_list(&torrents, infohash) {
        Some(t) => (true, Some(t.category.clone().unwrap_or_default()), false),
        None => (false, None, false),
    })
}

/// Result of an add attempt.
#[derive(Debug, Clone, Copy, Default)]
pub struct AddOutcome {
    pub added: bool,
    pub exists_in_correct_category: bool,
    pub server_error: bool,
    pub add_attempted: bool,
}

pub async fn add_torrent_to_server(
    tsuri: &str,
    magnet: &str,
    infohash: &str,
    expected_category: &str,
    token: &CancellationToken,
    typetask: Option<i32>,
) -> Result<AddOutcome, Cancelled> {
    let (exists, actual, server_error) = check_torrent_exists_with_category(tsuri, infohash, token, typetask).await?;
    if server_error {
        return Ok(AddOutcome { server_error: true, ..Default::default() });
    }
    if exists {
        let correct = actual.map(|a| a.eq_ignore_ascii_case(expected_category)).unwrap_or(false);
        return Ok(AddOutcome { exists_in_correct_category: correct, ..Default::default() });
    }

    let client = ts_client(tsuri);
    let body = json!({ "action": "add", "link": magnet, "save_to_db": false, "category": expected_category });
    let fut = async {
        let resp = client.post_torrents(body).send().await?;
        let status = resp.status();
        if status.is_success() {
            let _ = resp.bytes().await?;
        }
        Ok::<_, reqwest::Error>(status)
    };
    match guarded(token, Duration::from_secs(30), fut).await {
        Outcome::Cancelled => Err(Cancelled),
        Outcome::Timeout => {
            tlog("Таймаут при добавлении торрента на сервер", typetask);
            // The add may have landed before the timeout - the hash still needs a rem.
            Ok(AddOutcome { server_error: true, add_attempted: true, ..Default::default() })
        }
        Outcome::Done(Err(e)) => {
            tlog(format!("Ошибка при добавлении торрента на сервере: {e}"), typetask);
            Ok(AddOutcome { server_error: true, add_attempted: true, ..Default::default() })
        }
        Outcome::Done(Ok(status)) if status.is_success() => {
            invalidate_list_cache(tsuri);
            Ok(AddOutcome { added: true, add_attempted: true, ..Default::default() })
        }
        Outcome::Done(Ok(status)) => {
            tlog(format!("Ошибка при добавлении торрента ({})", status.as_u16()), typetask);
            Ok(AddOutcome { add_attempted: true, ..Default::default() })
        }
    }
}

/// Fresh list for the orphan sweep.
pub async fn get_torrent_list_for_cleanup(tsuri: &str, token: &CancellationToken) -> Result<ListResult, Cancelled> {
    get_torrent_list(tsuri, token, true).await
}

/// `action=rem` with one retry after 2s.
pub async fn rem_torrent_on_server(tsuri: &str, infohash: &str, typetask: Option<i32>) -> bool {
    let client = ts_client(tsuri);
    let post_rem = || async {
        let body = json!({ "action": "rem", "hash": infohash });
        match tokio::time::timeout(Duration::from_secs(30), client.post_torrents(body).send()).await {
            Ok(Ok(resp)) => Ok(resp.status().is_success()),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err("The operation was canceled.".to_string()),
        }
    };
    let res = async {
        if post_rem().await? {
            invalidate_list_cache(tsuri);
            return Ok(true);
        }
        tlog(format!("rem {infohash}: retry after failure"), typetask);
        tokio::time::sleep(Duration::from_secs(2)).await;
        if post_rem().await? {
            invalidate_list_cache(tsuri);
            return Ok(true);
        }
        invalidate_list_cache(tsuri);
        Ok::<bool, String>(false)
    }
    .await;
    match res {
        Ok(v) => v,
        Err(e) => {
            tlog(format!("rem {infohash} ошибка: {e}"), typetask);
            false
        }
    }
}

/// `GET {tsuri}/ffp/{HASH}/{fileId}` → `(model, status)`.
/// Non-success statuses and empty bodies give `(None, status)`.
pub async fn analyze_with_external_api(
    tsuri: &str,
    infohash: &str,
    token: &CancellationToken,
    typetask: Option<i32>,
    ffp_timeout: Option<Duration>,
    file_id: i32,
) -> Result<(Option<FfprobeModel>, i32), FfpError> {
    let timeout = ffp_timeout.unwrap_or_else(|| self::ffp_timeout(1, None));
    let api_url = format!("{}/ffp/{}/{file_id}", ts_client(tsuri).base, infohash.to_uppercase());
    tlog(format!("Запрос /ffp/{file_id} для {infohash} (таймаут {:.0}s)...", timeout.as_secs_f64()), typetask);

    let client = ts_client(tsuri);
    let fut = async {
        let resp = client.get(api_url).send().await?;
        let status = resp.status().as_u16() as i32;
        if !resp.status().is_success() {
            return Ok((None, status));
        }
        let body = resp.text().await?;
        Ok::<_, reqwest::Error>((Some(body), status))
    };
    let (body, status) = match guarded(token, timeout, fut).await {
        Outcome::Cancelled => return Err(FfpError::Cancelled),
        Outcome::Timeout => return Err(FfpError::Timeout),
        Outcome::Done(Err(e)) => return Err(FfpError::Http(e.to_string())),
        Outcome::Done(Ok(v)) => v,
    };
    let Some(body) = body.filter(|b| !crab_core::util::is_blank(b)) else { return Ok((None, status)) };
    match models::parse_ffprobe(&body) {
        Ok(model) => Ok((model, status)),
        Err(e) => Err(FfpError::Json(e.to_string())),
    }
}

/// Try each file id until one returns streams. HTTP 400 moves on to the next id;
/// any other status stops. `(model, status, error_message)`.
pub async fn probe_ffp_with_retries(
    tsuri: &str,
    infohash: &str,
    file_ids: &[i32],
    ffp_timeout: Duration,
    token: &CancellationToken,
    typetask: Option<i32>,
) -> Result<(Option<FfprobeModel>, i32, Option<String>), FfpError> {
    let ids: Vec<i32> = if file_ids.is_empty() { vec![1] } else { file_ids.to_vec() };
    let mut last_code = 0;
    for file_id in ids {
        if token.is_cancelled() {
            return Err(FfpError::Cancelled);
        }
        match analyze_with_external_api(tsuri, infohash, token, typetask, Some(ffp_timeout), file_id).await {
            Ok((result, code)) => {
                last_code = code;
                if result.as_ref().and_then(|r| r.streams.as_ref()).map(|s| !s.is_empty()).unwrap_or(false) {
                    return Ok((result, code, None));
                }
                if code != 400 {
                    return Ok((result, code, Some("Нет данных о треках".into())));
                }
            }
            Err(FfpError::Timeout) => return Ok((None, 504, Some("ffp timeout".into()))),
            Err(e) => return Err(e),
        }
    }
    Ok((None, if last_code > 0 { last_code } else { 400 }, Some("no probeable media file".into())))
}

fn poll_delay(deadline: Instant) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(Instant::now())?;
    if remaining.is_zero() {
        return None;
    }
    Some(remaining.min(Duration::from_secs(2)))
}

/// Poll `action=get` until `file_stats` is present or the timeout elapses.
pub async fn wait_torrent_ready(
    tsuri: &str,
    infohash: &str,
    token: &CancellationToken,
    typetask: Option<i32>,
    ready_timeout: Option<Duration>,
) -> Result<bool, Cancelled> {
    let timeout = ready_timeout.unwrap_or_else(tracks_read_timeout);
    let deadline = Instant::now() + timeout;
    tlog(format!("Ожидание готовности торрента {infohash} (до {:.0}s)...", timeout.as_secs_f64()), typetask);

    while Instant::now() < deadline {
        if token.is_cancelled() {
            return Err(Cancelled);
        }
        let info = get_torrent_from_server(tsuri, infohash, token, typetask).await?;
        if let Some(i) = &info {
            if let Some(fs) = i.file_stats.as_ref().filter(|f| !f.is_empty()) {
                tlog(format!("Торрент {infohash} готов: file_stats={}, stat={}", fs.len(), i.stat), typetask);
                return Ok(true);
            }
            if i.stat == 3 {
                tlog(
                    format!("Торрент {infohash} в состоянии Working (stat=3), file_stats ещё пуст - продолжаем ожидание"),
                    typetask,
                );
            }
        }
        let Some(d) = poll_delay(deadline) else { break };
        sleep(d, token).await?;
    }

    tlog(
        format!("Торрент {infohash} не получил file_stats за {:.0}s - проверяем сиды перед /ffp", timeout.as_secs_f64()),
        typetask,
    );
    Ok(false)
}

/// Poll until seeders or downloaded bytes appear.
pub async fn wait_download_progress(
    tsuri: &str,
    infohash: &str,
    token: &CancellationToken,
    typetask: Option<i32>,
    progress_timeout: Option<Duration>,
) -> Result<bool, Cancelled> {
    let timeout = progress_timeout.unwrap_or_else(tracks_peer_wait_timeout);
    let deadline = Instant::now() + timeout;
    tlog(format!("Проверка прогресса загрузки {infohash} (до {:.0}s)...", timeout.as_secs_f64()), typetask);

    while Instant::now() < deadline {
        if token.is_cancelled() {
            return Err(Cancelled);
        }
        let info = get_torrent_from_server(tsuri, infohash, token, typetask).await?;
        if has_download_progress(info.as_ref()) {
            if let Some(i) = &info {
                tlog(
                    format!("Торрент {infohash}: прогресс загрузки (seeders={}, bytes_read={})", i.connected_seeders, i.bytes_read),
                    typetask,
                );
            }
            return Ok(true);
        }
        let Some(d) = poll_delay(deadline) else { break };
        sleep(d, token).await?;
    }

    tlog(format!("Торрент {infohash}: нет сидов/данных - пропуск /ffp"), typetask);
    Ok(false)
}

/// Wait until `tracksminbufferkb` is buffered, or the download stalls.
pub async fn wait_media_buffer(
    tsuri: &str,
    infohash: &str,
    token: &CancellationToken,
    typetask: Option<i32>,
    buffer_timeout: Option<Duration>,
) -> Result<bool, Cancelled> {
    let min_bytes = min_buffer_bytes();
    if min_bytes <= 0 {
        return Ok(true);
    }
    let timeout = buffer_timeout.unwrap_or_else(tracks_peer_wait_timeout);
    let deadline = Instant::now() + timeout;
    let mut last_loaded: i64 = -1;
    let mut stall_polls = 0;

    tlog(format!("Ожидание буфера {infohash} (мин. {min_bytes} bytes, до {:.0}s)...", timeout.as_secs_f64()), typetask);

    while Instant::now() < deadline {
        if token.is_cancelled() {
            return Err(Cancelled);
        }
        let info = get_torrent_from_server(tsuri, infohash, token, typetask).await?;
        let loaded = info.as_ref().map(|i| i.loaded_size.max(i.bytes_read)).unwrap_or(0).max(0);

        if loaded >= min_bytes {
            tlog(format!("Торрент {infohash}: буфер готов (loaded={loaded})"), typetask);
            return Ok(true);
        }

        match &info {
            Some(i) if i.connected_seeders == 0 && i.download_speed == 0 && loaded > 0 => {
                stall_polls += 1;
                if stall_polls >= 3 {
                    tlog(format!("Торрент {infohash}: загрузка остановилась (loaded={loaded})"), typetask);
                    return Ok(loaded > 0);
                }
            }
            _ => stall_polls = 0,
        }

        if loaded == last_loaded && info.as_ref().map(|i| i.connected_seeders == 0).unwrap_or(false) && loaded == 0 {
            break;
        }
        last_loaded = loaded;

        let Some(d) = poll_delay(deadline) else { break };
        sleep(d, token).await?;
    }

    tlog(format!("Торрент {infohash}: буфер не достигнут ({min_bytes} bytes)"), typetask);
    Ok(false)
}

/// `action=get`. `Ok(None)` on 404, errors and empty bodies; `Err` only when the token is cancelled.
pub async fn get_torrent_from_server(
    tsuri: &str,
    infohash: &str,
    token: &CancellationToken,
    typetask: Option<i32>,
) -> Result<Option<TorrentInfo>, Cancelled> {
    let client = ts_client(tsuri);
    let fut = async {
        let resp = client.post_torrents(json!({ "action": "get", "hash": infohash })).send().await.map_err(|e| e.to_string())?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            tlog(format!("get torrent вернул {}", status.as_u16()), typetask);
            return Ok(None);
        }
        let text = resp.text().await.map_err(|e| e.to_string())?;
        if crab_core::util::is_blank(&text) {
            return Ok(None);
        }
        serde_json::from_str::<Option<TorrentInfo>>(&text).map_err(|e| e.to_string())
    };
    match guarded(token, Duration::from_secs(15), fut).await {
        Outcome::Cancelled => Err(Cancelled),
        Outcome::Timeout => {
            tlog("Ошибка get torrent: The operation was canceled.", typetask);
            Ok(None)
        }
        Outcome::Done(Err(e)) => {
            tlog(format!("Ошибка get torrent: {e}"), typetask);
            Ok(None)
        }
        Outcome::Done(Ok(v)) => Ok(v),
    }
}

/// Remove the torrent from TorrServer when we own it (added it or it was already in our category).
pub async fn cleanup_torrent(tsuri: &str, infohash: &str, typetask: Option<i32>, owned_in_category: bool) {
    if !owned_in_category {
        tlog(format!("Торрент {infohash}: rem пропущен (не добавляли / чужая категория)."), typetask);
        return;
    }
    if rem_torrent_on_server(tsuri, infohash, typetask).await {
        tlog(format!("Торрент {infohash} успешно удален с сервера"), typetask);
    } else {
        tlog(format!("Ошибка при удалении торрента {infohash}"), typetask);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn userinfo_split() {
        let (base, auth) = split_userinfo("http://user:pass@127.0.0.1:8090");
        assert_eq!(base, "http://127.0.0.1:8090");
        assert_eq!(auth, Some(("user".into(), "pass".into())));
        let (base, auth) = split_userinfo("http://127.0.0.1:8090/ts");
        assert_eq!(base, "http://127.0.0.1:8090/ts");
        assert!(auth.is_none());
    }

    #[test]
    fn tsuri_key() {
        assert_eq!(normalize_tsuri_key("http://127.0.0.1:8090/"), "http://127.0.0.1:8090");
        assert_eq!(normalize_tsuri_key("HTTP://Host:8090/X/"), "http://host:8090/x");
    }
}
