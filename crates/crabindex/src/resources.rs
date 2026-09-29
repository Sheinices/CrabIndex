// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! Resource usage for the admin overview (`GET {admin}/api/resources`): this process (RSS, CPU,
//! cgroup limit), the host (memory, load) and, when the Docker socket is mounted into the
//! container, every container's memory and CPU from the Docker API. Read-only, cached for
//! [`CACHE_SECS`]; Linux `/proc` and `/sys` files, nothing on other platforms.

use std::path::Path;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};

const CACHE_SECS: u64 = 10;
pub const DOCKER_SOCKET: &str = "/var/run/docker.sock";
const DOCKER_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_CONTAINERS: usize = 12;

// ---------------------------------------------------------------- process / host

#[derive(Clone, Copy)]
struct CpuSample {
    at: Instant,
    /// Process CPU time (user + system), clock ticks.
    ticks: u64,
}

static CPU_LAST: Lazy<Mutex<Option<CpuSample>>> = Lazy::new(|| Mutex::new(None));

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn first_number(s: &str) -> Option<u64> {
    s.split_whitespace().next().and_then(|x| x.parse().ok())
}

/// `memory.max` / `memory.limit_in_bytes`: `None` when unlimited or absent.
fn cgroup_limit() -> Option<u64> {
    let v2 = read("/sys/fs/cgroup/memory.max").and_then(|s| s.trim().parse::<u64>().ok());
    let v1 = || read("/sys/fs/cgroup/memory/memory.limit_in_bytes").and_then(|s| s.trim().parse::<u64>().ok());
    v2.or_else(v1).filter(|n| *n < (1u64 << 60))
}

fn cgroup_usage() -> Option<u64> {
    read("/sys/fs/cgroup/memory.current")
        .or_else(|| read("/sys/fs/cgroup/memory/memory.usage_in_bytes"))
        .and_then(|s| s.trim().parse().ok())
}

/// Resident set size of this process in bytes (`/proc/self/statm`, page size 4 KiB).
fn rss_bytes() -> Option<u64> {
    let s = read("/proc/self/statm")?;
    let pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

/// utime + stime from `/proc/self/stat` (fields 14 and 15, after the `)` of the command).
fn cpu_ticks() -> Option<u64> {
    let s = read("/proc/self/stat")?;
    let rest = &s[s.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // rest[0] is state (field 3), so utime (14) is index 11 and stime (15) is index 12
    Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
}

/// CPU percent of this process since the previous call (100 = one core), `None` on the first.
fn cpu_percent() -> Option<f64> {
    let ticks = cpu_ticks()?;
    let now = Instant::now();
    let mut last = CPU_LAST.lock();
    let prev = last.replace(CpuSample { at: now, ticks });
    let prev = prev?;
    let secs = now.duration_since(prev.at).as_secs_f64();
    if secs < 0.5 {
        return None;
    }
    let hz = 100.0; // CLK_TCK on Linux
    Some(((ticks.saturating_sub(prev.ticks)) as f64 / hz / secs * 1000.0).round() / 10.0)
}

fn meminfo() -> Option<(u64, u64)> {
    let s = read("/proc/meminfo")?;
    let mut total = None;
    let mut avail = None;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total = first_number(v).map(|k| k * 1024);
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            avail = first_number(v).map(|k| k * 1024);
        }
    }
    Some((total?, avail?))
}

fn loadavg() -> Option<[f64; 3]> {
    let s = read("/proc/loadavg")?;
    let mut it = s.split_whitespace().map(|x| x.parse::<f64>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

/// Free / total bytes of the filesystem holding `Data` (`df -Pk`).
fn disk() -> Option<(u64, u64)> {
    let out = std::process::Command::new("df").args(["-Pk", "Data"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().nth(1)?;
    let f: Vec<&str> = line.split_whitespace().collect();
    let total: u64 = f.get(1)?.parse().ok()?;
    let avail: u64 = f.get(3)?.parse().ok()?;
    Some((avail * 1024, total * 1024))
}

fn process_and_host() -> Value {
    let (mem_total, mem_avail) = meminfo().map(|(t, a)| (Some(t), Some(a))).unwrap_or((None, None));
    let (disk_free, disk_total) = disk().map(|(f, t)| (Some(f), Some(t))).unwrap_or((None, None));
    json!({
        "process": {
            "rss": rss_bytes(),
            "cpuPercent": cpu_percent(),
            "cgroupUsage": cgroup_usage(),
            "cgroupLimit": cgroup_limit(),
            "cpus": std::thread::available_parallelism().map(|n| n.get()).ok(),
        },
        "host": {
            "memTotal": mem_total,
            "memAvailable": mem_avail,
            "load": loadavg(),
            "diskFree": disk_free,
            "diskTotal": disk_total,
        },
    })
}

// ---------------------------------------------------------------- docker

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_usage: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f64>,
}

/// One HTTP/1.1 GET over the Docker unix socket; the body with chunked encoding decoded.
#[cfg(unix)]
async fn docker_get(path: &str) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::UnixStream::connect(DOCKER_SOCKET).await.map_err(|e| e.to_string())?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: docker\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.map_err(|e| e.to_string())?;
    parse_http(&buf)
}

#[cfg(not(unix))]
async fn docker_get(_path: &str) -> Result<String, String> {
    Err("no unix sockets".into())
}

/// Split status/headers/body; decode `Transfer-Encoding: chunked`; error on non-2xx.
fn parse_http(raw: &[u8]) -> Result<String, String> {
    let sep = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("bad response")?;
    let head = String::from_utf8_lossy(&raw[..sep]).to_string();
    let body = &raw[sep + 4..];
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let chunked = head.to_ascii_lowercase().contains("transfer-encoding: chunked");
    let body = if chunked { dechunk(body) } else { body.to_vec() };
    let text = String::from_utf8_lossy(&body).to_string();
    if !(200..300).contains(&status) {
        return Err(format!("docker api {status}: {}", text.chars().take(120).collect::<String>()));
    }
    Ok(text)
}

fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(nl) = b.windows(2).position(|w| w == b"\r\n") {
        let size_line = String::from_utf8_lossy(&b[..nl]);
        let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
        b = &b[nl + 2..];
        if size == 0 || b.len() < size {
            break;
        }
        out.extend_from_slice(&b[..size]);
        b = b.get(size + 2..).unwrap_or(&[]);
    }
    out
}

/// CPU percent from a stats sample (Docker's own formula, 100 = one core).
fn docker_cpu_percent(st: &Value) -> Option<f64> {
    let cpu = st["cpu_stats"]["cpu_usage"]["total_usage"].as_f64()?;
    let pre = st["precpu_stats"]["cpu_usage"]["total_usage"].as_f64()?;
    let sys = st["cpu_stats"]["system_cpu_usage"].as_f64()?;
    let presys = st["precpu_stats"]["system_cpu_usage"].as_f64()?;
    let ncpu = st["cpu_stats"]["online_cpus"].as_f64().filter(|n| *n > 0.0).unwrap_or(1.0);
    let (d, ds) = (cpu - pre, sys - presys);
    if d <= 0.0 || ds <= 0.0 {
        return Some(0.0);
    }
    Some((d / ds * ncpu * 1000.0).round() / 10.0)
}

/// Docker's `docker stats` memory: usage minus inactive file cache.
fn docker_mem(st: &Value) -> (Option<u64>, Option<u64>) {
    let m = &st["memory_stats"];
    let usage = m["usage"].as_u64().map(|u| {
        let cache = m["stats"]["inactive_file"].as_u64().or_else(|| m["stats"]["total_inactive_file"].as_u64()).unwrap_or(0);
        u.saturating_sub(cache)
    });
    let limit = m["limit"].as_u64().filter(|l| *l < (1u64 << 60));
    (usage, limit)
}

pub fn container_from(list_entry: &Value, stats: Option<&Value>) -> Container {
    let name = list_entry["Names"].as_array().and_then(|a| a.first()).and_then(|n| n.as_str()).unwrap_or("").trim_start_matches('/').to_string();
    let (mem_usage, mem_limit) = stats.map(docker_mem).unwrap_or((None, None));
    Container {
        name,
        image: list_entry["Image"].as_str().unwrap_or("").to_string(),
        state: list_entry["State"].as_str().unwrap_or("").to_string(),
        status: list_entry["Status"].as_str().unwrap_or("").to_string(),
        mem_usage,
        mem_limit,
        cpu_percent: stats.and_then(docker_cpu_percent),
    }
}

/// `{ available, error?, containers }`. Stats are taken in parallel, one 1-second sample each.
async fn docker() -> Value {
    if !Path::new(DOCKER_SOCKET).exists() {
        return json!({ "available": false });
    }
    let list = match tokio::time::timeout(DOCKER_TIMEOUT, docker_get("/containers/json?all=1")).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return json!({ "available": false, "error": e }),
        Err(_) => return json!({ "available": false, "error": "docker api timeout" }),
    };
    let entries: Vec<Value> = serde_json::from_str(&list).unwrap_or_default();
    let entries: Vec<Value> = entries.into_iter().take(MAX_CONTAINERS).collect();
    let stats = futures::future::join_all(entries.iter().map(|e| async move {
        if e["State"].as_str() != Some("running") {
            return None;
        }
        let id = e["Id"].as_str()?;
        let s = tokio::time::timeout(DOCKER_TIMEOUT, docker_get(&format!("/containers/{id}/stats?stream=false"))).await.ok()?.ok()?;
        serde_json::from_str::<Value>(&s).ok()
    }))
    .await;
    let mut containers: Vec<Container> = entries.iter().zip(stats.iter()).map(|(e, s)| container_from(e, s.as_ref())).collect();
    containers.sort_by(|a, b| b.mem_usage.unwrap_or(0).cmp(&a.mem_usage.unwrap_or(0)).then_with(|| a.name.cmp(&b.name)));
    json!({ "available": true, "containers": containers })
}

// ---------------------------------------------------------------- cache + api

struct Cached {
    at: Instant,
    value: Value,
}

static CACHE: Lazy<tokio::sync::Mutex<Option<Cached>>> = Lazy::new(|| tokio::sync::Mutex::new(None));

/// The full snapshot, at most once per [`CACHE_SECS`].
pub async fn snapshot() -> Value {
    let mut g = CACHE.lock().await;
    if let Some(c) = g.as_ref() {
        if c.at.elapsed() < Duration::from_secs(CACHE_SECS) {
            return c.value.clone();
        }
    }
    let mut v = tokio::task::spawn_blocking(process_and_host).await.unwrap_or(Value::Null);
    v["docker"] = docker().await;
    v["at"] = json!(chrono::Utc::now());
    *g = Some(Cached { at: Instant::now(), value: v.clone() });
    v
}

/// Last snapshot without refreshing (for health signals), `None` before the first request.
pub fn last() -> Option<Value> {
    CACHE.try_lock().ok().and_then(|g| g.as_ref().map(|c| c.value.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_body_is_decoded() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\n5\r\n[{\"a\"\r\n4\r\n:1}]\r\n0\r\n\r\n";
        assert_eq!(parse_http(raw).unwrap(), "[{\"a\":1}]");
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(parse_http(plain).unwrap(), "{}");
        assert!(parse_http(b"HTTP/1.1 404 Not Found\r\n\r\nno such container").unwrap_err().contains("404"));
    }

    #[test]
    fn container_stats_follow_docker_formulas() {
        let entry = json!({ "Names": ["/flaresolverr"], "Image": "ghcr.io/flaresolverr/flaresolverr:latest", "State": "running", "Status": "Up 3 days" });
        let stats = json!({
            "cpu_stats": { "cpu_usage": { "total_usage": 2_000_000_000u64 }, "system_cpu_usage": 10_000_000_000u64, "online_cpus": 4 },
            "precpu_stats": { "cpu_usage": { "total_usage": 1_000_000_000u64 }, "system_cpu_usage": 6_000_000_000u64 },
            "memory_stats": { "usage": 1_500_000_000u64, "limit": 6_442_450_944u64, "stats": { "inactive_file": 500_000_000u64 } }
        });
        let c = container_from(&entry, Some(&stats));
        assert_eq!(c.name, "flaresolverr");
        assert_eq!(c.mem_usage, Some(1_000_000_000));
        assert_eq!(c.mem_limit, Some(6_442_450_944));
        assert_eq!(c.cpu_percent, Some(100.0));
        let stopped = container_from(&json!({ "Names": ["/warp"], "State": "exited", "Status": "Exited (0)" }), None);
        assert_eq!((stopped.mem_usage, stopped.cpu_percent), (None, None));
    }

    #[test]
    fn proc_stat_fields_are_parsed_after_the_command() {
        // cpu_ticks reads /proc/self/stat; on Linux it must parse, elsewhere it is None.
        let s = "1234 (crab index) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 75 0 0 20 0 9 0 12345 1000 200 18446744073709551615";
        let rest = &s[s.rfind(')').unwrap() + 2..];
        let f: Vec<&str> = rest.split_whitespace().collect();
        assert_eq!((f[11], f[12]), ("250", "75"));
    }
}
