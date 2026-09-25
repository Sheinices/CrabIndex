//! Lenient query-string binding and small JSON helpers shared by the handlers.
//!
//! Query names are matched case-insensitively (the server lowercases them, but direct
//! router use in tests may not). Unparsable values fall back to the parameter default.

use axum::extract::Query;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Raw query map with lowercase keys.
#[derive(Debug, Default, Clone)]
pub struct Params(pub HashMap<String, String>);

impl Params {
    pub fn from_query(q: Query<HashMap<String, String>>) -> Params {
        Params(q.0.into_iter().map(|(k, v)| (k.to_ascii_lowercase(), v)).collect())
    }

    pub fn str(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(|s| s.as_str())
    }

    pub fn i32(&self, name: &str, default: i32) -> i32 {
        self.str(name).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(default)
    }

    pub fn i64(&self, name: &str, default: i64) -> i64 {
        self.str(name).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(default)
    }

    pub fn bool(&self, name: &str, default: bool) -> bool {
        match self.str(name).map(|v| v.trim().to_ascii_lowercase()) {
            Some(v) if v == "true" => true,
            Some(v) if v == "false" => false,
            _ => default,
        }
    }
}

/// `""` → JSON null (string fields that are null in the data model).
pub fn s_or_null(s: &str) -> Value {
    if s.is_empty() {
        Value::Null
    } else {
        Value::String(s.to_string())
    }
}

/// Recursively drop `null` object members (API responses omit nulls).
pub fn strip_nulls(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                if !x.is_null() {
                    out.insert(k, strip_nulls(x));
                }
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(strip_nulls).collect()),
        x => x,
    }
}

/// A double that serializes like a general-format number (`12` instead of `12.0`).
pub fn num_f64(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9.0e15 {
        Value::from(x as i64)
    } else {
        serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
    }
}

/// JSON response with nulls omitted.
pub fn json(v: Value) -> axum::Json<Value> {
    axum::Json(strip_nulls(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn params_are_lenient() {
        let mut m = HashMap::new();
        m.insert("SampleSize".to_string(), "abc".to_string());
        m.insert("excludenumeric".to_string(), "False".to_string());
        m.insert("time".to_string(), " 42 ".to_string());
        let p = Params::from_query(Query(m));
        assert_eq!(p.i32("samplesize", 20), 20);
        assert!(!p.bool("excludenumeric", true));
        assert_eq!(p.i64("time", 0), 42);
        assert_eq!(p.i64("start", -1), -1);
    }

    #[test]
    fn strip_nulls_recurses() {
        let v = strip_nulls(json!({"a": null, "b": {"c": null, "d": 1}, "e": [{"f": null}]}));
        assert_eq!(v, json!({"b": {"d": 1}, "e": [{}]}));
    }

    #[test]
    fn num_f64_formats_integers_without_fraction() {
        assert_eq!(num_f64(12.0).to_string(), "12");
        assert_eq!(num_f64(12.3).to_string(), "12.3");
    }
}
