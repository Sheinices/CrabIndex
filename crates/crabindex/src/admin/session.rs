//! Admin sessions (in memory, reset on restart) and the login failure limiter.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::crypto::{ct_eq, devkey_fingerprint, random_bytes};
use sha2::{Digest, Sha256};

/// Failed logins allowed per client IP within [`FAILURE_WINDOW`].
pub const MAX_FAILURES: usize = 5;
pub const FAILURE_WINDOW: Duration = Duration::from_secs(600);
const MAX_SESSIONS: usize = 1000;

struct Session {
    expires: Instant,
    devkey_fp: [u8; 32],
}

/// Keyed by SHA-256 of the cookie value, so the map never holds usable tokens.
static SESSIONS: Lazy<Mutex<HashMap<[u8; 32], Session>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static FAILURES: Lazy<Mutex<HashMap<String, Vec<Instant>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn session_key(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// New session bound to the current devkey; returns the cookie value (64 hex chars).
pub fn create(devkey: &str, hours: i32) -> String {
    let token = hex::encode(random_bytes::<32>());
    let ttl = Duration::from_secs(hours.clamp(1, 8760) as u64 * 3600);
    let now = Instant::now();
    let mut m = SESSIONS.lock();
    m.retain(|_, s| s.expires > now);
    if m.len() >= MAX_SESSIONS {
        if let Some(oldest) = m.iter().min_by_key(|(_, s)| s.expires).map(|(k, _)| *k) {
            m.remove(&oldest);
        }
    }
    m.insert(session_key(&token), Session { expires: now + ttl, devkey_fp: devkey_fingerprint(devkey) });
    token
}

/// Valid when known, not expired and created with the devkey that is configured now.
pub fn is_valid(token: &str, devkey: &str) -> bool {
    if token.len() != 64 || devkey.is_empty() {
        return false;
    }
    let key = session_key(token);
    let mut m = SESSIONS.lock();
    match m.get(&key) {
        Some(s) if s.expires > Instant::now() && ct_eq(&s.devkey_fp, &devkey_fingerprint(devkey)) => true,
        Some(_) => {
            m.remove(&key);
            false
        }
        None => false,
    }
}

pub fn destroy(token: &str) {
    SESSIONS.lock().remove(&session_key(token));
}

fn prune(v: &mut Vec<Instant>, now: Instant) {
    v.retain(|t| now.duration_since(*t) < FAILURE_WINDOW);
}

/// True when the client already used up its failed attempts.
pub fn is_rate_limited(client: &str) -> bool {
    let now = Instant::now();
    let mut m = FAILURES.lock();
    match m.get_mut(client) {
        Some(v) => {
            prune(v, now);
            v.len() >= MAX_FAILURES
        }
        None => false,
    }
}

pub fn record_failure(client: &str) {
    let now = Instant::now();
    let mut m = FAILURES.lock();
    m.retain(|_, v| {
        prune(v, now);
        !v.is_empty()
    });
    m.entry(client.to_string()).or_default().push(now);
}

pub fn clear_failures(client: &str) {
    FAILURES.lock().remove(client);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lifecycle() {
        let t = create("key-1", 1);
        assert_eq!(t.len(), 64);
        assert!(is_valid(&t, "key-1"));
        assert!(!is_valid(&t, "key-2"), "devkey change must end the session");
        assert!(!is_valid(&t, "key-1"), "invalidated session is removed");
        let t = create("key-1", 1);
        destroy(&t);
        assert!(!is_valid(&t, "key-1"));
        assert!(!is_valid("nope", "key-1"));
    }

    #[test]
    fn failure_limiter() {
        let ip = "limiter-unit-test";
        for _ in 0..MAX_FAILURES {
            assert!(!is_rate_limited(ip));
            record_failure(ip);
        }
        assert!(is_rate_limited(ip));
        clear_failures(ip);
        assert!(!is_rate_limited(ip));
    }
}
