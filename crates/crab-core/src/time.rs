//! Date/time helpers.
//!
//! FileDB shards contain ISO dates in three flavours: `...Z` (Utc), `...+03:00` (Local) and no offset (Unspecified).
//! Everything is normalised to `DateTime<Utc>`; `0001-01-01T00:00:00` is the
//! "no date" value and is represented by [`min`].

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone, Utc};

/// The "no date" value, `0001-01-01T00:00:00`.
pub fn min() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(1, 1, 1, 0, 0, 0).unwrap()
}

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// True for `default(DateTime)`.
pub fn is_min(dt: &DateTime<Utc>) -> bool {
    dt.year() <= 1
}

/// Midnight today in local time, as a UTC instant.
pub fn today_local() -> DateTime<Utc> {
    let d = chrono::Local::now().date_naive();
    chrono::Local
        .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|x| x.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

/// Windows file time: 100ns ticks since 1601-01-01 UTC.
pub fn to_file_time_utc(dt: &DateTime<Utc>) -> i64 {
    const EPOCH_DIFF_SECS: i64 = 11_644_473_600;
    let secs = dt.timestamp() + EPOCH_DIFF_SECS;
    if secs < 0 {
        return 0;
    }
    secs * 10_000_000 + (dt.timestamp_subsec_nanos() / 100) as i64
}

pub fn from_file_time_utc(ft: i64) -> DateTime<Utc> {
    const EPOCH_DIFF_SECS: i64 = 11_644_473_600;
    let secs = ft / 10_000_000 - EPOCH_DIFF_SECS;
    let nanos = ((ft % 10_000_000) * 100) as u32;
    Utc.timestamp_opt(secs, nanos).single().unwrap_or_else(min)
}

/// Parse any ISO-8601 flavour found in stored data. Unspecified offset → treated as UTC.
pub fn parse_net(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(Utc.from_utc_datetime(&n));
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap()));
    }
    // "/Date(1234567890000)/"
    if let Some(ms) = s.strip_prefix("/Date(").and_then(|x| x.strip_suffix(")/")) {
        let digits: String = ms.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
        if let Ok(ms) = digits.parse::<i64>() {
            return Utc.timestamp_millis_opt(ms).single();
        }
    }
    None
}

pub fn format_net(dt: &DateTime<Utc>) -> String {
    if is_min(dt) {
        return "0001-01-01T00:00:00".to_string();
    }
    dt.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// serde adapter: `#[serde(with = "crab_core::time::net")]`
pub mod net {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(dt: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format_net(dt))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(match v {
            serde_json::Value::String(s) => parse_net(&s).unwrap_or_else(min),
            serde_json::Value::Number(n) => n.as_i64().and_then(|ms| Utc.timestamp_millis_opt(ms).single()).unwrap_or_else(min),
            _ => min(),
        })
    }
}

/// serde adapter for `Option<DateTime<Utc>>`.
pub mod net_opt {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(dt: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match dt {
            Some(d) => s.serialize_str(&format_net(d)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(match v {
            serde_json::Value::String(s) => parse_net(&s),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_filetime() {
        let dt = Utc.with_ymd_and_hms(2024, 5, 1, 12, 0, 0).unwrap();
        assert_eq!(from_file_time_utc(to_file_time_utc(&dt)), dt);
    }

    #[test]
    fn parse_variants() {
        assert!(parse_net("2023-08-29T10:11:12.1234567Z").is_some());
        assert!(parse_net("2023-08-29T10:11:12.1234567+03:00").is_some());
        assert!(is_min(&parse_net("0001-01-01T00:00:00").unwrap()));
    }
}
