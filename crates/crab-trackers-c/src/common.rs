//! Small helpers shared by the trackers in this crate.

use std::time::Instant;

use crab_core::fdb::{self, WriteGuard};
use crab_core::models::TorrentDetails;
use indexmap::IndexMap;

/// Build `(key, value)` pairs for `parser_log::write_kv`.
macro_rules! kv {
    () => { Vec::<(String, String)>::new() };
    ($($k:expr => $v:expr),+ $(,)?) => {
        vec![$(($k.to_string(), $v.to_string())),+]
    };
}
pub(crate) use kv;

/// Boolean formatted the way the parser logs print it.
pub(crate) fn b(v: bool) -> &'static str {
    if v {
        "True"
    } else {
        "False"
    }
}

/// Elapsed seconds with one decimal ("12.3").
pub(crate) fn secs(start: Instant) -> String {
    format!("{:.1}", start.elapsed().as_secs_f64())
}

/// Lenient integer query parameter (unparsable → default).
pub(crate) fn int_param(v: &Option<String>, default: i32) -> i32 {
    v.as_deref().and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(default)
}

/// Group rows by FileDB bucket key, keeping first-seen order.
pub(crate) fn group_by_key<T: AsRef<TorrentDetails>>(list: Vec<T>) -> Vec<(String, Vec<T>)> {
    let mut groups: IndexMap<String, Vec<T>> = IndexMap::new();
    for t in list {
        let key = {
            let r = t.as_ref();
            fdb::key_db(&r.name, &r.originalname)
        };
        groups.entry(key).or_default().push(t);
    }
    groups.into_iter().collect()
}

/// Row currently stored under `url` in an opened shard.
pub(crate) fn cached(w: &WriteGuard, url: &str) -> Option<TorrentDetails> {
    w.with_db(|db| db.get(url).cloned())
}

/// Split on any of several literal separators (in order of appearance).
pub(crate) fn split_multi<'a>(s: &'a str, seps: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut last = 0;
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if let Some(sep) = seps.iter().find(|sep| !sep.is_empty() && rest.starts_with(**sep)) {
            out.push(&s[last..i]);
            i += sep.len();
            last = i;
            continue;
        }
        i += rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    out.push(&s[last..]);
    out
}

/// Trim + truncate a UTF-8 string to at most `max` chars.
pub(crate) fn take_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// A small expiring in-memory value (cookie cache).
pub(crate) struct Expiring {
    inner: parking_lot::Mutex<Option<(String, std::time::Instant)>>,
}

impl Expiring {
    pub(crate) const fn new() -> Self {
        Expiring { inner: parking_lot::const_mutex(None) }
    }

    pub(crate) fn get(&self) -> Option<String> {
        let mut g = self.inner.lock();
        match g.as_ref() {
            Some((v, until)) if Instant::now() < *until => Some(v.clone()),
            Some(_) => {
                *g = None;
                None
            }
            None => None,
        }
    }

    pub(crate) fn set(&self, value: String, ttl: std::time::Duration) {
        *self.inner.lock() = Some((value, Instant::now() + ttl));
    }

    pub(crate) fn remove(&self) {
        *self.inner.lock() = None;
    }
}

/// Plain reqwest client for login POSTs: no redirects, no cookie store, invalid certs accepted.
pub(crate) fn login_client(timeout_secs: u64) -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .ok()
}

/// Read a response body with a byte cap.
pub(crate) async fn read_capped(mut resp: reqwest::Response, max: usize) -> Result<String, String> {
    let mut buf = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(c)) => {
                buf.extend_from_slice(&c);
                if buf.len() > max {
                    return Err("Cannot write more bytes to the buffer than the configured maximum buffer size".into());
                }
            }
            Ok(None) => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_multi_works() {
        assert_eq!(split_multi("a<x>b<y>c", &["<x>", "<y>"]), vec!["a", "b", "c"]);
        assert_eq!(split_multi("абв", &["б"]), vec!["а", "в"]);
        assert_eq!(split_multi("abc", &["z"]), vec!["abc"]);
    }
}
