//! Gzip JSON persistence: atomic temp-file writes, lenient reads.

use dashmap::DashMap;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{BufReader, BufWriter, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::log::{self, cat};

static PATH_LOCKS: Lazy<DashMap<String, Arc<Mutex<()>>>> = Lazy::new(DashMap::new);

fn lock_for(path: &str) -> Arc<Mutex<()>> {
    let key = std::fs::canonicalize(path).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| path.to_string());
    PATH_LOCKS.entry(key).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

/// Read a gzip JSON file; None on any error.
pub fn read_gz_json<T: DeserializeOwned>(path: &str) -> Option<T> {
    let f = std::fs::File::open(path).ok()?;
    let dec = GzDecoder::new(BufReader::new(f));
    serde_json::from_reader(BufReader::new(dec)).ok()
}

/// Write gzip JSON atomically (temp + rename). Errors are logged at debug level.
pub fn write_gz_json<T: Serialize + ?Sized>(path: &str, value: &T) {
    let gate = lock_for(path);
    let _g = gate.lock();
    let started = Instant::now();
    let tmp = format!("{path}.tmp");
    let res = (|| -> std::io::Result<()> {
        if let Some(dir) = std::path::Path::new(path).parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let f = std::fs::File::create(&tmp)?;
        let mut enc = GzEncoder::new(BufWriter::new(f), Compression::default());
        serde_json::to_writer(&mut enc, value).map_err(std::io::Error::other)?;
        let mut w = enc.finish()?;
        w.flush()?;
        drop(w);
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        log::debug(cat::FDB, format!("gz json write failed path={path}: {e}"));
    }
    let el = started.elapsed();
    if el > Duration::from_secs(5) {
        log::warn(cat::FDB, format!("gz json write slow path={path} elapsed={:.1}s", el.as_secs_f64()));
    }
}
