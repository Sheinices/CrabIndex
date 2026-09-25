//! Case-insensitive multi-value query string collection.
//!
//! Keys are compared case-insensitively (stored lowercase), values keep their
//! order. `get` joins repeated values with `,`; a missing key yields `""`.

use indexmap::IndexMap;

#[derive(Clone, Debug, Default)]
pub struct QueryCollection {
    map: IndexMap<String, Vec<String>>,
    /// Raw query string including the leading `?` (empty when there is none).
    raw: String,
}

impl QueryCollection {
    /// Parse a raw query string (with or without the leading `?`).
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.strip_prefix('?').unwrap_or(raw);
        let mut map: IndexMap<String, Vec<String>> = IndexMap::new();
        for (k, v) in url::form_urlencoded::parse(trimmed.as_bytes()) {
            if k.is_empty() && v.is_empty() {
                continue;
            }
            map.entry(k.to_lowercase()).or_default().push(v.into_owned());
        }
        let raw = if trimmed.is_empty() { String::new() } else { format!("?{trimmed}") };
        QueryCollection { map, raw }
    }

    /// Build from key/value pairs (tests and internal callers).
    pub fn from_pairs<K: AsRef<str>, V: AsRef<str>>(pairs: &[(K, V)]) -> Self {
        let mut map: IndexMap<String, Vec<String>> = IndexMap::new();
        let mut ser = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in pairs {
            map.entry(k.as_ref().to_lowercase()).or_default().push(v.as_ref().to_string());
            ser.append_pair(k.as_ref(), v.as_ref());
        }
        let s = ser.finish();
        QueryCollection { map, raw: if s.is_empty() { String::new() } else { format!("?{s}") } }
    }

    /// Raw query string with the leading `?` (or empty).
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(&key.to_lowercase())
    }

    /// All values joined by `,`; `""` when missing.
    pub fn get(&self, key: &str) -> String {
        self.map.get(&key.to_lowercase()).map(|v| v.join(",")).unwrap_or_default()
    }

    /// Joined value when present and not blank.
    pub fn get_non_blank(&self, key: &str) -> Option<String> {
        let v = self.get(key);
        if v.trim().is_empty() {
            None
        } else {
            Some(v)
        }
    }

    pub fn values(&self, key: &str) -> &[String] {
        self.map.get(&key.to_lowercase()).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.map.iter()
    }

    /// Integer value (whitespace tolerant); `None` when missing or unparsable.
    pub fn get_i32(&self, key: &str) -> Option<i32> {
        if !self.contains_key(key) {
            return None;
        }
        parse_int(&self.get(key))
    }
}

/// Lenient integer parse: surrounding whitespace and a leading sign are allowed.
pub fn parse_int(s: &str) -> Option<i32> {
    s.trim().parse::<i32>().ok()
}

/// Lenient i64 parse.
pub fn parse_i64(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

/// Lenient boolean parse (`true`/`false`, case-insensitive).
pub fn parse_bool(s: &str) -> Option<bool> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("true") {
        Some(true)
    } else if t.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_values_and_case() {
        let q = QueryCollection::parse("?Cat=2000&cat=5000&q=Fight+Club&tvdbid=&x=%D0%B0");
        assert_eq!(q.get("cat"), "2000,5000");
        assert_eq!(q.values("CAT").len(), 2);
        assert_eq!(q.get("q"), "Fight Club");
        assert!(q.contains_key("tvdbid"));
        assert_eq!(q.get("tvdbid"), "");
        assert_eq!(q.get("missing"), "");
        assert_eq!(q.get("x"), "а");
        assert!(q.raw().starts_with("?Cat=2000"));
    }
}
