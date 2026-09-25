//! Hourly announce-list builder: collects `tr=` announce URLs from every magnet in
//! FileDB, probes them (HTTP HEAD-ish GET / UDP send) and writes the reachable ones to
//! `wwwroot/trackers.txt`. Runs only when every shard is kept in memory
//! (`evercache.enable` with `validHour <= 0`).

use crab_core::log::{self, cat};
use crab_core::{conf, fdb, rx, util};
use indexmap::IndexSet;
use std::collections::HashMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::sleep_ct;

pub fn spawn_worker(shutdown: CancellationToken) {
    tokio::spawn(async move {
        log::info(cat::TRACKERS, "trackers worker started");
        run(shutdown).await;
    });
}

/// Announce URLs worth probing from one magnet (already decoded / lowercased / filtered).
pub fn candidate_trackers(magnet: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !magnet.contains('&') {
        return out;
    }
    for g in rx::all_groups(magnet, "tr=([^&]+)") {
        let raw = g.get(1).map(|s| s.as_str()).unwrap_or("");
        let first = raw.split('?').next().unwrap_or("");
        let tracker = util::url_decode(first).trim().to_lowercase();
        if util::is_blank(&tracker)
            || tracker.contains('[')
            || !tracker.replace("://", "").contains(':')
            || tracker.contains(' ')
            || tracker.contains("torrentsmd.eu")
        {
            continue;
        }
        if rx::is_match(&tracker, "[^/]+/[^/]+/announce") {
            continue;
        }
        out.push(tracker);
    }
    out
}

async fn check(tracker: &str) -> bool {
    if util::is_blank(tracker) || tracker.contains('[') {
        return false;
    }
    if tracker.starts_with("http") {
        let client = match reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(7))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };
        return client.get(tracker).send().await.is_ok();
    }
    if tracker.starts_with("udp:") {
        let t = tracker.replace("udp://", "");
        let host = t.split(':').next().unwrap_or("").split('/').next().unwrap_or("").to_string();
        let port: u16 = if t.contains(':') {
            match t.split(':').nth(1).unwrap_or("").split('/').next().unwrap_or("").parse() {
                Ok(p) => p,
                Err(_) => return false,
            }
        } else {
            6969
        };
        let uri = rx::group(&t, "^[^/]/(.*)", 1);
        let payload = format!("GET /{uri} HTTP/1.1\r\nHost: {host}\r\n\r\n");
        let fut = async move {
            let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()?;
            sock.connect((host.as_str(), port)).await.ok()?;
            sock.send(payload.as_bytes()).await.ok()
        };
        return matches!(tokio::time::timeout(Duration::from_secs(7), fut).await, Ok(Some(_)));
    }
    false
}

async fn build_list(ct: &CancellationToken) -> std::io::Result<usize> {
    let mut trackers: IndexSet<String> = IndexSet::new();
    // one probe per distinct announce URL per run
    let mut probed: HashMap<String, bool> = HashMap::new();
    for (key, _) in fdb::master_db_snapshot() {
        if ct.is_cancelled() {
            break;
        }
        for t in fdb::open_read(&key, false, true).values() {
            if t.magnet.is_empty() {
                continue;
            }
            for tracker in candidate_trackers(&t.magnet) {
                let ok = match probed.get(&tracker) {
                    Some(v) => *v,
                    None => {
                        let v = check(&tracker).await;
                        probed.insert(tracker.clone(), v);
                        v
                    }
                };
                if ok {
                    trackers.insert(tracker);
                }
            }
        }
    }
    std::fs::create_dir_all("wwwroot")?;
    let mut body = String::new();
    for t in &trackers {
        body.push_str(t);
        body.push('\n');
    }
    std::fs::write("wwwroot/trackers.txt", body)?;
    Ok(trackers.len())
}

pub async fn run(ct: CancellationToken) {
    if !sleep_ct(Duration::from_secs(20), &ct).await {
        return;
    }
    while !ct.is_cancelled() {
        let c = conf();
        if !c.evercache.enable || c.evercache.validHour > 0 {
            if !sleep_ct(Duration::from_secs(60), &ct).await {
                return;
            }
            continue;
        }
        if !sleep_ct(Duration::from_secs(3600), &ct).await {
            return;
        }
        log::info(cat::TRACKERS, format!("start / {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
        match build_list(&ct).await {
            Ok(n) => log::info(
                cat::TRACKERS,
                format!("end / {} wrote {n} trackers to wwwroot/trackers.txt", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
            ),
            Err(e) => log::error(cat::TRACKERS, format!("error / {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_are_filtered() {
        let m = "magnet:?xt=urn:btih:abc&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce\
                 &tr=http%3A%2F%2FBT.Example.com%3A80%2Fannounce%3Fpasskey%3Dx\
                 &tr=http%3A%2F%2Fnoport.example.com%2Fannounce\
                 &tr=http%3A%2F%2Fa.com%3A80%2Fx%2Fannounce\
                 &tr=udp%3A%2F%2F%5B2001%3Adb8%3A%3A1%5D%3A80\
                 &tr=udp%3A%2F%2Ftorrentsmd.eu%3A8080%2Fannounce";
        let c = candidate_trackers(m);
        assert_eq!(c, vec!["udp://tracker.opentrackr.org:1337/announce", "http://bt.example.com:80/announce?passkey=x"]);
        assert!(candidate_trackers("magnet:?xt=urn:btih:abc").is_empty());
    }
}
