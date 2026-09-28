//! Sync workers pulling `/sync/fdb/torrents` from `syncapi`.
//!
//! * torrents mode: incremental by `fileTime`; checkpoints in `Data/temp/lastsync.txt`
//!   (position of the last imported bucket) and `Data/temp/starsync.txt` (end of the last
//!   complete pass, sent as `start` so older rows come back slim).
//! * spidr mode: full pass of slim rows (sid/pir/url) every `timeSyncSpidr` minutes.
//!
//! While `nextread` is true masterDb + lastsync are saved every `saveCheckpointEveryNBatches`
//! batches or at least every 5 minutes.
//!
//! Unknown slim rows: rows older than `start` come slim (sid/pir/url). When such a url is not
//! in the local bucket the host renamed or moved the row after the client's last full pass and
//! the full row never came (a slim row cannot be created locally). The bucket is then fetched
//! in full through `/sync/fdb?key=` and imported.
//!
//! Deletions: a page carries whole buckets, so for every tracker the host says it serves
//! (`trackers` in `/sync/conf`) the bucket content is authoritative - local rows of such a
//! tracker that the incoming bucket no longer has (removed, merged as duplicates, moved to
//! another bucket by a rename) are dropped after the import.

use anyhow::anyhow;
use chrono::{Local, TimeZone};
use crab_core::config::AppOptions;
use crab_core::fdb;
use crab_core::log::{self, cat};
use crab_core::models::{de, TorrentDetails};
use crab_core::net::{self, Req};
use crab_core::trackers::Cancelled;
use crab_core::{conf, time, util};
use indexmap::IndexMap;
use rand::Rng;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::sleep_ct;

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
pub const LAST_SYNC_PATH: &str = "Data/temp/lastsync.txt";
pub const STAR_SYNC_PATH: &str = "Data/temp/starsync.txt";

/// Torrent count reported by the sync host's `/sync/conf` (`-1` = not known yet).
/// Used only for the admin fill-progress display, never for sync logic.
pub static REMOTE_TORRENTS: AtomicI64 = AtomicI64::new(-1);

/// Incoming page (lenient: missing/null collections are distinguishable from empty).
#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct RootIn {
    #[serde(deserialize_with = "de::bool")]
    pub nextread: bool,
    #[serde(deserialize_with = "de::i32")]
    pub countread: i32,
    pub collections: Option<Vec<CollectionIn>>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct CollectionIn {
    #[serde(deserialize_with = "de::string")]
    pub Key: String,
    pub Value: Option<ValueIn>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct ValueIn {
    #[serde(deserialize_with = "de::i64")]
    pub fileTime: i64,
    pub torrents: Option<IndexMap<String, TorrentDetails>>,
}

fn now_str() -> String {
    Local::now().format(TIME_FORMAT).to_string()
}

/// Local-time rendering of a file time for logs (`-` for negative).
pub fn format_file_time(file_time: i64) -> String {
    if file_time < 0 {
        return "-".into();
    }
    let dt = time::from_file_time_utc(file_time);
    if time::is_min(&dt) && file_time != 0 {
        return file_time.to_string();
    }
    Local.from_utc_datetime(&dt.naive_utc()).format(TIME_FORMAT).to_string()
}

/// `hh:mm:ss.t` elapsed format used in sync logs.
pub fn format_elapsed(d: Duration) -> String {
    let total = d.as_secs();
    format!("{:02}:{:02}:{:02}.{}", total / 3600, (total / 60) % 60, total % 60, d.subsec_millis() / 100)
}

fn checkpoint_every(c: &AppOptions) -> i32 {
    if c.saveCheckpointEveryNBatches > 0 {
        c.saveCheckpointEveryNBatches
    } else {
        0
    }
}

/// Save by batch count (`every` > 0) or when 5 minutes passed since the last save.
pub fn should_save_checkpoint(every: i32, batch_index: i32, last_save: &mut Instant) -> bool {
    let by_batch = every > 0 && batch_index % every == 0;
    let by_time = last_save.elapsed() > Duration::from_secs(5 * 60);
    if !by_batch && !by_time {
        return false;
    }
    *last_save = Instant::now();
    true
}

/// Rows of a torrents-mode page after `synctrackers` / `syncsport` filters.
pub fn filter_incoming(root: &RootIn, c: &AppOptions) -> (Vec<TorrentDetails>, i32, i32) {
    let mut out = Vec::with_capacity(root.countread.max(0) as usize);
    let (mut by_tracker, mut by_sport) = (0, 0);
    for col in root.collections.iter().flatten() {
        let Some(torrents) = col.Value.as_ref().and_then(|v| v.torrents.as_ref()) else { continue };
        for t in torrents.values() {
            if let Some(st) = &c.synctrackers {
                if !t.trackerName.is_empty() && !st.iter().any(|x| x == &t.trackerName) {
                    by_tracker += 1;
                    continue;
                }
            }
            if !c.syncsport && t.types.iter().any(|x| x == "sport") {
                by_sport += 1;
                continue;
            }
            out.push(t.clone());
        }
    }
    (out, by_tracker, by_sport)
}

fn read_checkpoint(path: &str) -> anyhow::Result<i64> {
    let s = std::fs::read_to_string(path)?;
    let t = s.trim_start_matches('\u{feff}').trim();
    t.parse::<i64>().map_err(|_| anyhow!("The input string '{t}' was not in a correct format."))
}

fn write_checkpoint(path: &str, v: i64) {
    let _ = std::fs::create_dir_all("Data/temp");
    let _ = std::fs::write(path, v.to_string());
}

async fn save_master() {
    let _ = tokio::task::spawn_blocking(fdb::save_changes_to_file).await;
}

/// Trackers a `/sync/conf` answer says the host serves (lowercase); `None` for hosts that
/// predate the field - then nothing is ever dropped.
fn served_from_conf(v: Option<&serde_json::Value>) -> Option<HashSet<String>> {
    let list = v?.get("trackers")?.as_array()?;
    Some(list.iter().filter_map(|x| x.as_str()).map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect())
}

/// Local rows of bucket `key` to drop: tracker served by the host and allowed here, url absent
/// from the incoming bucket. Rows of trackers this client parses itself (not served) stay.
pub fn prune_plan(local: &fdb::ShardMap, incoming_urls: &HashSet<String>, served: &HashSet<String>, c: &AppOptions) -> Vec<String> {
    local
        .iter()
        .filter(|(url, t)| {
            let tracker = t.trackerName.to_lowercase();
            served.contains(&tracker)
                && c.synctrackers.as_ref().map(|st| st.iter().any(|x| x.eq_ignore_ascii_case(&tracker))).unwrap_or(true)
                && !c.disable_trackers.iter().any(|d| d.eq_ignore_ascii_case(&tracker))
                && !incoming_urls.contains(url.as_str())
        })
        .map(|(url, _)| url.clone())
        .collect()
}

/// A row the host sent slim (no title/tracker): only sid, pir and url.
fn is_slim(t: &TorrentDetails) -> bool {
    t.title.is_empty() && t.trackerName.is_empty()
}

/// Urls of slim rows in `incoming` that the local bucket does not have.
pub fn unknown_slim_urls(local: &fdb::ShardMap, incoming: &IndexMap<String, TorrentDetails>) -> Vec<String> {
    incoming.iter().filter(|(u, t)| is_slim(t) && !local.contains_key(u.as_str())).map(|(u, _)| u.clone()).collect()
}

/// `synctrackers` / `syncsport` as applied by [`filter_incoming`], for one row.
fn row_allowed(t: &TorrentDetails, c: &AppOptions) -> bool {
    if let Some(st) = &c.synctrackers {
        if !t.trackerName.is_empty() && !st.iter().any(|x| x == &t.trackerName) {
            return false;
        }
    }
    c.syncsport || !t.types.iter().any(|x| x == "sport")
}

/// One item of `/sync/fdb?key=`.
#[derive(Deserialize, Default, Debug)]
#[serde(default)]
struct BucketIn {
    #[serde(deserialize_with = "de::string")]
    Key: String,
    value: Option<IndexMap<String, TorrentDetails>>,
}

/// Buckets of a page whose slim rows include urls unknown here (checked against the local copy).
async fn buckets_with_unknown_slim(cols: &[CollectionIn]) -> Vec<String> {
    let pairs: Vec<(String, IndexMap<String, TorrentDetails>)> = cols
        .iter()
        .filter(|c| !c.Key.is_empty())
        .filter_map(|c| c.Value.as_ref().and_then(|v| v.torrents.as_ref()).map(|t| (c.Key.clone(), t.clone())))
        .filter(|(_, t)| t.values().any(is_slim))
        .collect();
    if pairs.is_empty() {
        return Vec::new();
    }
    tokio::task::spawn_blocking(move || {
        pairs
            .into_iter()
            .filter(|(key, incoming)| {
                let local = if fdb::MASTER_DB.contains_key(key) { fdb::open_read(key, false, false) } else { fdb::ShardMap::new() };
                !unknown_slim_urls(&local, incoming).is_empty()
            })
            .map(|(key, _)| key)
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Fetch the named buckets in full and import their rows. Returns `(buckets fetched, rows imported)`.
async fn refetch_buckets(syncapi: &str, keys: Vec<String>, c: &AppOptions, ct: &CancellationToken) -> (usize, usize) {
    let (mut fetched, mut rows) = (0usize, 0usize);
    for key in keys {
        if ct.is_cancelled() {
            break;
        }
        let url = format!("{syncapi}/sync/fdb?key={}", urlencoding::encode(&key));
        let req = Req::new().timeout(120).max_size(100_000_000).cancel(ct);
        let Some(items) = net::get_json::<Vec<BucketIn>>(&url, &req).await else { continue };
        let Some(item) = items.into_iter().find(|i| i.Key == key) else { continue };
        fetched += 1;
        let torrents: Vec<TorrentDetails> = item.value.unwrap_or_default().into_values().filter(|t| !is_slim(t) && row_allowed(t, c)).collect();
        rows += torrents.len();
        import(torrents).await;
    }
    (fetched, rows)
}

/// Heal buckets whose slim rows are unknown here; returns `(buckets, rows)` imported.
async fn heal_unknown_slim(syncapi: &str, cols: &[CollectionIn], c: &AppOptions, ct: &CancellationToken) -> (usize, usize) {
    let keys = buckets_with_unknown_slim(cols).await;
    if keys.is_empty() {
        return (0, 0);
    }
    refetch_buckets(syncapi, keys, c, ct).await
}

/// Apply [`prune_plan`] to every bucket of a page; returns the number of dropped rows.
async fn prune_missing(cols: &[CollectionIn], served: Option<&HashSet<String>>, c: &AppOptions) -> usize {
    let Some(served) = served else { return 0 };
    let mut buckets: Vec<(String, HashSet<String>)> = Vec::new();
    for col in cols {
        let Some(torrents) = col.Value.as_ref().and_then(|v| v.torrents.as_ref()) else { continue };
        if col.Key.is_empty() || torrents.is_empty() {
            continue;
        }
        buckets.push((col.Key.clone(), torrents.keys().cloned().collect()));
    }
    if buckets.is_empty() {
        return 0;
    }
    let served = served.clone();
    let c = c.clone();
    tokio::task::spawn_blocking(move || {
        let mut removed = 0usize;
        for (key, urls) in buckets {
            if !fdb::MASTER_DB.contains_key(&key) {
                continue;
            }
            let plan = prune_plan(&fdb::open_read(&key, false, false), &urls, &served, &c);
            if plan.is_empty() {
                continue;
            }
            let drop: HashSet<String> = plan.into_iter().collect();
            removed += fdb::retain_rows(&key, |t| !drop.contains(&t.url));
        }
        removed
    })
    .await
    .unwrap_or(0)
}

async fn import(torrents: Vec<TorrentDetails>) {
    if torrents.is_empty() {
        return;
    }
    let _ = tokio::task::spawn_blocking(move || fdb::add_or_update(&torrents)).await;
}

async fn fetch_page(url: &str, ct: &CancellationToken) -> Option<RootIn> {
    let req = Req::new().timeout(300).max_size(100_000_000).cancel(ct);
    net::get_json::<RootIn>(url, &req).await
}

/// The whole remote `/sync/conf` JSON, or `None` when the host did not answer.
async fn remote_conf(syncapi: &str) -> Option<serde_json::Value> {
    net::get_json::<serde_json::Value>(&format!("{syncapi}/sync/conf"), &Req::new()).await
}

/// Retry delay after a failed cycle: 1, 2, 5, 10 minutes, never longer than `timeSync`.
pub fn retry_delay(failures: u32, time_sync_minutes: u64) -> Duration {
    let minutes = match failures {
        0 | 1 => 1,
        2 => 2,
        3 => 5,
        _ => 10,
    };
    Duration::from_secs(60 * minutes.min(time_sync_minutes.max(1)))
}

/// How a torrents cycle ended.
#[derive(Debug, PartialEq, Eq)]
pub enum CycleEnd {
    /// Reached the end of the remote feed (or nothing to do).
    Done,
    /// The sync host did not answer / the feed broke off: retry soon, not after `timeSync`.
    Unavailable,
}

/// Persistent position of the torrents worker.
#[derive(Debug)]
pub struct SyncState {
    pub lastsync: i64,
    pub starsync: i64,
}

impl Default for SyncState {
    fn default() -> Self {
        SyncState { lastsync: -1, starsync: -1 }
    }
}

fn save_torrents_checkpoint(st: &SyncState) {
    fdb::save_changes_to_file();
    if st.lastsync > 0 {
        write_checkpoint(LAST_SYNC_PATH, st.lastsync);
    }
    log::info(cat::SYNC, "saved state (lastsync.txt)");
}

async fn torrents_cycle(c: &AppOptions, syncapi: &str, st: &mut SyncState, ct: &CancellationToken) -> anyhow::Result<CycleEnd> {
    let cycle_start = Instant::now();
    let mut cycle_total = 0usize;
    let mut cycle_dropped = 0usize;
    let mut cycle_healed = 0usize;
    let mut end = CycleEnd::Done;
    log::info(cat::SYNC, format!("start / {}", now_str()));

    if st.lastsync == -1 && std::path::Path::new(LAST_SYNC_PATH).exists() {
        st.lastsync = read_checkpoint(LAST_SYNC_PATH)?;
    }

    let conf_json = remote_conf(syncapi).await;
    if let Some(v) = &conf_json {
        // Best-effort: record the host total so the admin can show fill progress.
        if let Some(n) = v.get("count").and_then(|x| x.as_i64()) {
            REMOTE_TORRENTS.store(n, Ordering::Relaxed);
        }
    }
    let served = served_from_conf(conf_json.as_ref());
    let fbd = conf_json.as_ref().map(|v| v.get("fbd").and_then(|x| x.as_bool()).unwrap_or(false));
    if fbd.is_none() {
        log::warn(cat::SYNC, format!("{syncapi} is not answering (/sync/conf) - will retry in a few minutes"));
        end = CycleEnd::Unavailable;
    } else if fbd == Some(false) {
        log::warn(cat::SYNC, "remote /sync/conf missing fbd - upgrade syncapi host");
    } else {
        if st.starsync == -1 && std::path::Path::new(STAR_SYNC_PATH).exists() {
            st.starsync = read_checkpoint(STAR_SYNC_PATH)?;
        }
        log::info(
            cat::SYNC,
            format!(
                "loaded state lastsync={} ({}) starsync={} ({})",
                st.lastsync,
                format_file_time(st.lastsync),
                st.starsync,
                format_file_time(st.starsync)
            ),
        );

        let mut reset = true;
        let mut last_save = Instant::now();
        let mut batch_index = 0i32;
        loop {
            batch_index += 1;
            let batch_start = Instant::now();
            let url = format!("{syncapi}/sync/fdb/torrents?time={}&start={}", st.lastsync, st.starsync);
            let root = fetch_page(&url, ct).await;
            if ct.is_cancelled() {
                return Err(Cancelled.into());
            }

            let Some(root) = root.filter(|r| r.collections.is_some()) else {
                if reset {
                    reset = false;
                    if !sleep_ct(Duration::from_secs(60), ct).await {
                        return Err(Cancelled.into());
                    }
                    continue;
                }
                log::warn(cat::SYNC, format!("{syncapi}: feed request failed twice - will retry in a few minutes"));
                end = CycleEnd::Unavailable;
                break;
            };

            let count = root.collections.as_ref().map(|v| v.len()).unwrap_or(0);
            if count > 0 {
                reset = true;
                let (torrents, by_tracker, by_sport) = filter_incoming(&root, c);
                if by_tracker > 0 || by_sport > 0 {
                    log::info(
                        cat::SYNC,
                        format!("  incoming {}; filtered out {by_tracker} by tracker, {by_sport} by sport", root.countread),
                    );
                }
                let n = torrents.len();
                import(torrents).await;
                cycle_total += n;
                let cols = root.collections.as_deref().unwrap_or(&[]);
                let (healed_b, healed_r) = heal_unknown_slim(syncapi, cols, c, ct).await;
                cycle_healed += healed_r;
                let dropped = prune_missing(cols, served.as_ref(), c).await;
                cycle_dropped += dropped;
                log::info(
                    cat::SYNC,
                    format!(
                        "[{batch_index}] time={} ({}) | {n} torrents, refetched {healed_r} rows/{healed_b} buckets, dropped {dropped}, nextread={}, {}",
                        st.lastsync,
                        format_file_time(st.lastsync),
                        if root.nextread { "True" } else { "False" },
                        format_elapsed(batch_start.elapsed())
                    ),
                );

                if let Some(ft) = root.collections.as_ref().and_then(|v| v.last()).and_then(|c| c.Value.as_ref()).map(|v| v.fileTime) {
                    st.lastsync = ft;
                }

                if root.nextread {
                    if should_save_checkpoint(checkpoint_every(c), batch_index, &mut last_save) {
                        let snapshot = SyncState { lastsync: st.lastsync, starsync: st.starsync };
                        let _ = tokio::task::spawn_blocking(move || save_torrents_checkpoint(&snapshot)).await;
                    }
                    continue;
                }
            }

            st.starsync = st.lastsync;
            write_checkpoint(STAR_SYNC_PATH, st.starsync);
            log::info(cat::SYNC, "saved state (starsync.txt)");
            break;
        }
    }

    save_master().await;
    write_checkpoint(LAST_SYNC_PATH, st.lastsync);
    log::info(
        cat::SYNC,
        format!("end / {} (cycle added {cycle_total} torrents, refetched {cycle_healed}, dropped {cycle_dropped} in {})", now_str(), format_elapsed(cycle_start.elapsed())),
    );
    Ok(end)
}

/// Torrents-mode worker loop.
pub async fn torrents(ct: CancellationToken) {
    if !sleep_ct(Duration::from_secs(20), &ct).await {
        return;
    }
    let mut st = SyncState::default();
    let mut failures: u32 = 0;
    while !ct.is_cancelled() {
        let c = conf();
        let syncapi = c.syncapi.clone().unwrap_or_default();
        if util::is_blank(&syncapi) {
            if !sleep_ct(Duration::from_secs(60), &ct).await {
                return;
            }
            continue;
        }

        let end = match torrents_cycle(&c, &syncapi, &mut st, &ct).await {
            Ok(end) => end,
            Err(e) => {
                if e.downcast_ref::<Cancelled>().is_some() || ct.is_cancelled() {
                    return;
                }
                if st.lastsync > 0 {
                    save_master().await;
                    write_checkpoint(LAST_SYNC_PATH, st.lastsync);
                }
                log::error(cat::SYNC, format!("error / {} / {e}", now_str()));
                CycleEnd::Unavailable
            }
        };
        if end == CycleEnd::Unavailable {
            failures += 1;
            let delay = retry_delay(failures, conf().timeSync.max(20) as u64);
            log::info(cat::SYNC, format!("next attempt in {} min (failure {failures})", delay.as_secs() / 60));
            if !sleep_ct(delay, &ct).await {
                return;
            }
            continue;
        }
        failures = 0;

        let jitter = rand::thread_rng().gen_range(60..300u64);
        if !sleep_ct(Duration::from_secs(jitter), &ct).await {
            return;
        }
        let minutes = conf().timeSync.max(20) as u64;
        if !sleep_ct(Duration::from_secs(60 * minutes), &ct).await {
            return;
        }
    }
}

async fn spidr_cycle(syncapi: &str, c: &AppOptions, ct: &CancellationToken) -> anyhow::Result<()> {
    let mut lastsync_spidr: i64 = -1;
    let conf_json = remote_conf(syncapi).await;
    if conf_json.as_ref().and_then(|v| v.get("spidr")).and_then(|x| x.as_bool()) != Some(true) {
        return Ok(());
    }
    let served = served_from_conf(conf_json.as_ref());
    let mut cycle_dropped = 0usize;
    let mut cycle_healed = 0usize;
    let cycle_start = Instant::now();
    let mut cycle_total = 0usize;
    let mut batch_index = 0i32;
    let mut last_save = Instant::now();
    log::info(cat::SYNC_SPIDR, format!("start / {}", now_str()));

    loop {
        batch_index += 1;
        let batch_start = Instant::now();
        let url = format!("{syncapi}/sync/fdb/torrents?time={lastsync_spidr}&spidr=true");
        let root = fetch_page(&url, ct).await;
        if ct.is_cancelled() {
            return Err(Cancelled.into());
        }
        let Some(root) = root.filter(|r| r.collections.as_ref().map(|v| !v.is_empty()).unwrap_or(false)) else {
            break;
        };
        let cols = root.collections.as_ref().map(|v| v.as_slice()).unwrap_or(&[]);
        let batch_count: usize = cols.iter().map(|c| c.Value.as_ref().and_then(|v| v.torrents.as_ref()).map(|t| t.len()).unwrap_or(0)).sum();
        let rows: Vec<TorrentDetails> =
            cols.iter().filter_map(|c| c.Value.as_ref().and_then(|v| v.torrents.as_ref())).flat_map(|t| t.values().cloned()).collect();
        import(rows).await;
        let (healed_b, healed_r) = heal_unknown_slim(syncapi, cols, c, ct).await;
        cycle_healed += healed_r;
        let dropped = prune_missing(cols, served.as_ref(), c).await;
        cycle_dropped += dropped;

        cycle_total += batch_count;
        log::info(
            cat::SYNC_SPIDR,
            format!(
                "[{batch_index}] time={lastsync_spidr} ({}) | {} collections, {batch_count} torrents, refetched {healed_r} rows/{healed_b} buckets, dropped {dropped}, nextread={}, {}",
                format_file_time(lastsync_spidr),
                cols.len(),
                if root.nextread { "True" } else { "False" },
                format_elapsed(batch_start.elapsed())
            ),
        );
        if let Some(v) = cols.last().and_then(|c| c.Value.as_ref()) {
            lastsync_spidr = v.fileTime;
        }
        if root.nextread {
            if should_save_checkpoint(checkpoint_every(c), batch_index, &mut last_save) {
                save_master().await;
                log::info(cat::SYNC_SPIDR, "saved state (masterDb)");
            }
            continue;
        }
        break;
    }

    save_master().await;
    log::info(
        cat::SYNC_SPIDR,
        format!("end / {} (cycle added {cycle_total} torrents, refetched {cycle_healed}, dropped {cycle_dropped} in {})", now_str(), format_elapsed(cycle_start.elapsed())),
    );
    Ok(())
}

/// Spidr-mode worker loop.
pub async fn spidr(ct: CancellationToken) {
    while !ct.is_cancelled() {
        let c = conf();
        let minutes = c.timeSyncSpidr.max(20) as u64;
        if !sleep_ct(Duration::from_secs(60 * minutes), &ct).await {
            return;
        }
        let c = conf();
        let syncapi = c.syncapi.clone().unwrap_or_default();
        if util::is_blank(&syncapi) || !c.syncspidr {
            if !sleep_ct(Duration::from_secs(60), &ct).await {
                return;
            }
            continue;
        }
        if let Err(e) = spidr_cycle(&syncapi, &c, &ct).await {
            if e.downcast_ref::<Cancelled>().is_some() || ct.is_cancelled() {
                return;
            }
            log::error(cat::SYNC_SPIDR, format!("error / {} / {e}", now_str()));
        }
    }
}

/// Both sync loops.
pub async fn run_worker(ct: CancellationToken) {
    log::info(cat::SYNC, "sync worker started");
    tokio::join!(torrents(ct.clone()), spidr(ct));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(tracker: &str, url: &str) -> TorrentDetails {
        TorrentDetails { trackerName: tracker.into(), url: url.into(), ..Default::default() }
    }

    #[test]
    fn unknown_slim_urls_only_for_slim_rows_missing_locally() {
        let mut local = fdb::ShardMap::new();
        local.insert("known".into(), row("rutor", "known"));
        let mut incoming: IndexMap<String, TorrentDetails> = IndexMap::new();
        incoming.insert("known".into(), TorrentDetails { url: "known".into(), sid: 1, ..Default::default() });
        incoming.insert("moved".into(), TorrentDetails { url: "moved".into(), sid: 2, ..Default::default() });
        let mut full = row("rutor", "full-new");
        full.title = "t".into();
        incoming.insert("full-new".into(), full);
        assert_eq!(unknown_slim_urls(&local, &incoming), vec!["moved".to_string()]);

        let mut c = AppOptions::default();
        let mut sport = row("rutor", "s");
        sport.types = vec!["sport".into()];
        assert!(row_allowed(&sport, &c));
        c.syncsport = false;
        assert!(!row_allowed(&sport, &c));
        c.synctrackers = Some(vec!["kinozal".into()]);
        assert!(!row_allowed(&row("rutor", "x"), &c));
    }

    #[test]
    fn prune_plan_drops_only_served_rows_missing_from_bucket() {
        let mut local = fdb::ShardMap::new();
        local.insert("r1".into(), row("rutor", "r1"));
        local.insert("r2".into(), row("rutor", "r2"));
        local.insert("k1".into(), row("kinozal", "k1"));
        local.insert("own".into(), row("lostfilm", "own"));
        let incoming: HashSet<String> = ["r1".to_string()].into_iter().collect();
        let served: HashSet<String> = ["rutor".to_string(), "kinozal".to_string()].into_iter().collect();

        let mut c = AppOptions::default();
        let mut plan = prune_plan(&local, &incoming, &served, &c);
        plan.sort();
        assert_eq!(plan, vec!["k1".to_string(), "r2".to_string()]);

        c.synctrackers = Some(vec!["rutor".into()]);
        assert_eq!(prune_plan(&local, &incoming, &served, &c), vec!["r2".to_string()]);

        c.synctrackers = None;
        c.disable_trackers = vec!["KINOZAL".into()];
        assert_eq!(prune_plan(&local, &incoming, &served, &c), vec!["r2".to_string()]);

        assert!(served_from_conf(Some(&serde_json::json!({"fbd": true}))).is_none());
        let s = served_from_conf(Some(&serde_json::json!({"trackers": ["Rutor", " kinozal "]}))).expect("list");
        assert!(s.contains("rutor") && s.contains("kinozal") && s.len() == 2);
    }

    #[test]
    fn retry_delay_backs_off_and_caps() {
        let m = |f, t| retry_delay(f, t).as_secs() / 60;
        assert_eq!((m(1, 120), m(2, 120), m(3, 120), m(4, 120), m(9, 120)), (1, 2, 5, 10, 10));
        assert_eq!(m(4, 5), 5, "never longer than timeSync");
        assert_eq!(m(1, 0), 1);
    }

    #[test]
    fn elapsed_format() {
        assert_eq!(format_elapsed(Duration::from_millis(3_723_450)), "01:02:03.4");
        assert_eq!(format_elapsed(Duration::from_millis(59)), "00:00:00.0");
    }

    #[test]
    fn file_time_format_negative() {
        assert_eq!(format_file_time(-1), "-");
        let ft = time::to_file_time_utc(&chrono::Utc::now());
        assert_eq!(format_file_time(ft).len(), 19);
    }

    #[test]
    fn checkpoint_by_batch_and_time() {
        let mut last = Instant::now();
        assert!(!should_save_checkpoint(5, 3, &mut last));
        assert!(should_save_checkpoint(5, 5, &mut last));
        assert!(!should_save_checkpoint(0, 5, &mut last));
        let mut old = Instant::now() - Duration::from_secs(301);
        assert!(should_save_checkpoint(0, 1, &mut old));
    }

    #[test]
    fn incoming_filters() {
        let json = r#"{"nextread":true,"countread":3,"take":2000,"collections":[
            {"Key":"a:a","Value":{"time":"2024-01-01T00:00:00Z","fileTime":10,"torrents":{
                "u1":{"trackerName":"rutor","url":"u1","types":["movie"]},
                "u2":{"trackerName":"kinozal","url":"u2","types":["movie"]},
                "u3":{"trackerName":"rutor","url":"u3","types":["sport"]}}}}]}"#;
        let root: RootIn = serde_json::from_str(json).unwrap();
        let mut c = AppOptions::default();
        c.synctrackers = Some(vec!["rutor".into()]);
        c.syncsport = false;
        let (rows, by_tracker, by_sport) = filter_incoming(&root, &c);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "u1");
        assert_eq!((by_tracker, by_sport), (1, 1));
    }

    #[test]
    fn null_collections_is_none() {
        let root: RootIn = serde_json::from_str(r#"{"nextread":false}"#).unwrap();
        assert!(root.collections.is_none());
        let root: RootIn = serde_json::from_str(r#"{"nextread":false,"collections":[]}"#).unwrap();
        assert_eq!(root.collections.map(|v| v.len()), Some(0));
    }
}
