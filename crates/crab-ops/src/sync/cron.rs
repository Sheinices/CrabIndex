//! Sync workers pulling `/sync/fdb/torrents` from `syncapi`.
//!
//! * torrents mode: incremental by `fileTime`; checkpoints in `Data/temp/lastsync.txt`
//!   (position of the last imported bucket) and `Data/temp/starsync.txt` (end of the last
//!   complete pass, sent as `start` so older rows come back slim).
//! * spidr mode: full pass of slim rows (sid/pir/url) every `timeSyncSpidr` minutes.
//!
//! While `nextread` is true masterDb + lastsync are saved every `saveCheckpointEveryNBatches`
//! batches or at least every 5 minutes.

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
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::sleep_ct;

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
pub const LAST_SYNC_PATH: &str = "Data/temp/lastsync.txt";
pub const STAR_SYNC_PATH: &str = "Data/temp/starsync.txt";

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

/// `Some(flag)` from the remote `/sync/conf`, `None` when the host did not answer (down,
/// restarting, 5xx, network) - that is not the same as an old host without the flag.
async fn remote_conf_flag(syncapi: &str, flag: &str) -> Option<bool> {
    let v = net::get_json::<serde_json::Value>(&format!("{syncapi}/sync/conf"), &Req::new()).await?;
    Some(v.get(flag).and_then(|x| x.as_bool()).unwrap_or(false))
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
    let mut end = CycleEnd::Done;
    log::info(cat::SYNC, format!("start / {}", now_str()));

    if st.lastsync == -1 && std::path::Path::new(LAST_SYNC_PATH).exists() {
        st.lastsync = read_checkpoint(LAST_SYNC_PATH)?;
    }

    let fbd = remote_conf_flag(syncapi, "fbd").await;
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
                log::info(
                    cat::SYNC,
                    format!(
                        "[{batch_index}] time={} ({}) | {n} torrents, nextread={}, {}",
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
        format!("end / {} (cycle added {cycle_total} torrents in {})", now_str(), format_elapsed(cycle_start.elapsed())),
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
    if remote_conf_flag(syncapi, "spidr").await != Some(true) {
        return Ok(());
    }
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

        cycle_total += batch_count;
        log::info(
            cat::SYNC_SPIDR,
            format!(
                "[{batch_index}] time={lastsync_spidr} ({}) | {} collections, {batch_count} torrents, nextread={}, {}",
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
        format!("end / {} (cycle added {cycle_total} torrents in {})", now_str(), format_elapsed(cycle_start.elapsed())),
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
