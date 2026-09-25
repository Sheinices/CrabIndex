//! Small in-process expiring cache (per value type).

use dashmap::DashMap;
use std::time::{Duration, Instant};

pub struct MemCache<T: Clone> {
    map: DashMap<String, (Instant, T)>,
}

impl<T: Clone> Default for MemCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> MemCache<T> {
    pub fn new() -> Self {
        MemCache { map: DashMap::new() }
    }

    pub fn get(&self, key: &str) -> Option<T> {
        let now = Instant::now();
        let hit = self.map.get(key).map(|e| (e.0, e.1.clone()));
        match hit {
            Some((exp, v)) if exp > now => Some(v),
            Some(_) => {
                self.map.remove_if(key, |_, v| v.0 <= now);
                None
            }
            None => None,
        }
    }

    pub fn set(&self, key: impl Into<String>, value: T, ttl: Duration) {
        if self.map.len() > 50_000 {
            self.purge();
        }
        self.map.insert(key.into(), (Instant::now() + ttl, value));
    }

    pub fn remove(&self, key: &str) {
        self.map.remove(key);
    }

    pub fn clear(&self) {
        self.map.clear();
    }

    fn purge(&self) {
        let now = Instant::now();
        self.map.retain(|_, v| v.0 > now);
    }
}
