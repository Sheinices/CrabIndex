//! Lenient JSON accessors (numbers as strings and vice versa).

use serde_json::Value;

/// Strings as-is, numbers/bools stringified, null/absent → `None`.
pub fn val_str(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "True".into() } else { "False".into() }),
        _ => None,
    }
}

/// Integer, also from a numeric string.
pub fn val_i32(v: Option<&Value>) -> Option<i32> {
    match v? {
        Value::Number(n) => n.as_i64().map(|i| i as i32).or_else(|| n.as_f64().map(|f| f as i32)),
        Value::String(s) => s.trim().parse::<i32>().ok(),
        Value::Bool(b) => Some(*b as i32),
        _ => None,
    }
}

/// Boolean, also from "true"/"false" strings or numbers.
pub fn val_bool(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => s.trim().parse::<bool>().ok().or_else(|| match s.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0),
        _ => None,
    }
}

/// Short failure kind for log lines.
pub fn error_type_name(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "Timeout"
    } else if e.is_decode() {
        "InvalidJson"
    } else {
        "HttpError"
    }
}
