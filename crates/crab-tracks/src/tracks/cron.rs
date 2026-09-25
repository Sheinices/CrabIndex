//! Periodic track analysis over FileDB and the TorrServer orphan sweep.
//!
//! typetask: 1 - last day, 2 - last month, 3 - last year, 4 - older, 5 - old but recently updated.

use chrono::{DateTime, Months, Utc};
use crab_core::conf;
use crab_core::models::TorrentDetails;
use indexmap::IndexMap;
use rand::Rng;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::logging::tlog;
use super::remote::{self, sleep, Cancelled};
use super::{db, workflow};

/// Inter-item delay from `tracksdelay` with ±10% jitter (min 0).
pub fn get_inter_item_delay_ms() -> i32 {
    let base = conf().tracksdelay.max(0);
    if base == 0 {
        return 0;
    }
    let jitter = (base / 10).max(1);
    base + rand::thread_rng().gen_range(-jitter..=jitter)
}

fn sub_months(now: DateTime<Utc>, m: u32) -> DateTime<Utc> {
    now.checked_sub_months(Months::new(m)).unwrap_or(now)
}

/// Whether a row belongs to the given typetask window.
pub fn matches_typetask(t: &TorrentDetails, typetask: i32, now: DateTime<Utc>) -> bool {
    let day = now - chrono::Duration::days(1);
    let month = sub_months(now, 1);
    let year = sub_months(now, 12);
    match typetask {
        1 => t.createTime >= day,
        2 => t.createTime < day && t.createTime >= month,
        3 => t.createTime < month && t.createTime >= year,
        4 => !(t.createTime >= year || t.updateTime >= month),
        5 => t.createTime < year && t.updateTime >= month,
        _ => false,
    }
}

fn collect_candidates(typetask: i32) -> Vec<TorrentDetails> {
    let c = conf();
    let now = Utc::now();
    let mut torrents = Vec::new();
    for (key, _) in crab_core::fdb::master_db_snapshot() {
        for t in crab_core::fdb::open_read(&key, false, false).into_values() {
            if t.magnet.is_empty() || !matches_typetask(&t, typetask, now) {
                continue;
            }
            if db::the_bad(&t.types) || db::has_track_for_torrent(&t) {
                continue;
            }
            if t.ffprobe_tryingdata >= c.tracksatempt {
                continue;
            }
            if typetask == 1 || typetask == 2 || t.sid > 0 {
                torrents.push(t);
            }
        }
    }
    torrents
}

/// One row per infohash (latest update wins), newest first.
fn unique_by_hash(torrents: Vec<TorrentDetails>) -> Vec<TorrentDetails> {
    let mut groups: IndexMap<String, TorrentDetails> = IndexMap::new();
    for t in torrents {
        let key = db::infohash_from_magnet(&t.magnet).unwrap_or_else(|| {
            if !t.magnet.is_empty() {
                t.magnet.clone()
            } else {
                uuid_like()
            }
        });
        match groups.get(&key) {
            Some(existing) if existing.updateTime >= t.updateTime => {}
            _ => {
                groups.insert(key, t);
            }
        }
    }
    let mut list: Vec<TorrentDetails> = groups.into_values().collect();
    list.sort_by(|a, b| b.updateTime.cmp(&a.updateTime));
    list
}

fn uuid_like() -> String {
    format!("{:032x}", rand::thread_rng().gen::<u128>())
}

/// Loop for one typetask until `shutdown` is cancelled.
pub async fn run(typetask: i32, shutdown: CancellationToken) {
    let mut first_run = typetask == 1;
    loop {
        if shutdown.is_cancelled() {
            return;
        }
        if !first_run {
            let c = conf();
            let minutes = if typetask == 1 { c.TracksInterval.task1 } else { c.TracksInterval.task0 + typetask };
            if sleep(Duration::from_secs(minutes.max(0) as u64 * 60), &shutdown).await.is_err() {
                return;
            }
        }
        first_run = false;

        let c = conf();
        if !c.tracks {
            continue;
        }
        if c.tracksmod == 1 && (typetask == 3 || typetask == 4) {
            continue;
        }
        if run_once(typetask, &shutdown).await.is_err() {
            return;
        }
    }
}

async fn run_once(typetask: i32, shutdown: &CancellationToken) -> Result<(), Cancelled> {
    tlog(format!("start typetask={typetask}"), None);
    let start = Instant::now();

    let torrents = match tokio::task::spawn_blocking(move || collect_candidates(typetask)).await {
        Ok(t) => t,
        Err(e) => {
            tlog(format!("tracks: error typetask={typetask} / {e}"), None);
            return Ok(());
        }
    };
    tlog(format!("typetask={typetask} collected {} torrents to process", torrents.len()), None);

    let unique = unique_by_hash(torrents);
    let max_in_flight = conf().tracksconcurrency.max(1) as usize;
    let mut in_flight: JoinSet<()> = JoinSet::new();
    let ten_days = Duration::from_secs(10 * 24 * 3600);
    let month = Duration::from_secs(30 * 24 * 3600);

    for t in unique {
        if !conf().tracks {
            tlog(format!("end typetask={typetask} Tracks off in settings"), None);
            break;
        }
        if typetask == 2 && start.elapsed() > ten_days {
            break;
        }
        if (typetask == 3 || typetask == 4 || typetask == 5) && start.elapsed() > month {
            break;
        }
        if db::has_track_for_torrent(&t) {
            continue;
        }

        let torrent_key = crab_core::fdb::key_for_torrent(&t.name, &t.originalname);
        let delay = get_inter_item_delay_ms();
        if delay > 0 {
            if let Err(c) = sleep(Duration::from_millis(delay as u64), shutdown).await {
                in_flight.shutdown().await;
                return Err(c);
            }
        }

        while in_flight.len() >= max_in_flight {
            if let Some(Err(e)) = in_flight.join_next().await {
                tlog(format!("typetask={typetask} process error: {e}"), Some(typetask));
            }
        }

        let magnet = t.magnet.clone();
        let types = t.types.clone();
        let (attempt, sid) = (t.ffprobe_tryingdata, t.sid);
        in_flight.spawn(async move {
            workflow::add(&magnet, attempt, Some(&types), Some(&torrent_key), typetask, sid).await;
        });
    }

    while let Some(r) = in_flight.join_next().await {
        if let Err(e) = r {
            tlog(format!("typetask={typetask} process error: {e}"), Some(typetask));
        }
    }

    tlog(format!("end typetask={typetask} (elapsed {:.1}m)", start.elapsed().as_secs_f64() / 60.0), None);
    Ok(())
}

/// Remove TorrServer torrents left in `trackscategory` after a failed rem or a crash.
pub async fn orphan_cleanup_loop(shutdown: CancellationToken) {
    loop {
        let interval = conf().tracksorphansweepmin.max(1) as u64;
        if sleep(Duration::from_secs(interval * 60), &shutdown).await.is_err() {
            return;
        }
        if !conf().tracks {
            continue;
        }
        if orphan_sweep_once(&shutdown).await.is_err() {
            return;
        }
    }
}

/// One sweep over all `tsuri` servers.
pub async fn orphan_sweep_once(token: &CancellationToken) -> Result<(), Cancelled> {
    let c = conf();
    if c.tsuri.is_empty() || c.trackscategory.is_empty() {
        return Ok(());
    }
    let category = c.trackscategory.clone();
    let in_flight = remote::get_in_flight_hashes();
    let mut removed = 0;

    for tsuri in c.tsuri.iter() {
        if crab_core::util::is_blank(tsuri) {
            continue;
        }
        let (torrents, server_error) = remote::get_torrent_list_for_cleanup(tsuri, token).await?;
        let Some(torrents) = torrents.filter(|_| !server_error) else { continue };
        for t in torrents.iter() {
            let Some(hash) = t.hash.as_deref().filter(|h| !h.is_empty()) else { continue };
            let in_category = t.category.as_deref().map(|c| !c.is_empty() && c.eq_ignore_ascii_case(&category)).unwrap_or(false);
            if !in_category || in_flight.contains(&hash.to_lowercase()) {
                continue;
            }
            if remote::rem_torrent_on_server(tsuri, hash, None).await {
                removed += 1;
                tlog(format!("orphan sweep: rem {hash}"), None);
            }
        }
    }
    if removed > 0 {
        tlog(format!("orphan sweep done: removed {removed}"), None);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(created_days_ago: i64, updated_days_ago: i64) -> TorrentDetails {
        let now = Utc::now();
        TorrentDetails {
            createTime: now - chrono::Duration::days(created_days_ago),
            updateTime: now - chrono::Duration::days(updated_days_ago),
            ..Default::default()
        }
    }

    #[test]
    fn typetask_windows() {
        let now = Utc::now();
        assert!(matches_typetask(&row(0, 0), 1, now));
        assert!(!matches_typetask(&row(0, 0), 2, now));
        assert!(matches_typetask(&row(10, 10), 2, now));
        assert!(matches_typetask(&row(100, 100), 3, now));
        assert!(matches_typetask(&row(800, 800), 4, now));
        assert!(!matches_typetask(&row(800, 5), 4, now));
        assert!(matches_typetask(&row(800, 5), 5, now));
    }
}
