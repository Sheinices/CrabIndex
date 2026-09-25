//! Structural diff between the current and a proposed configuration document.

use serde::Serialize;
use serde_json::Value;

use super::schema::is_sensitive_field;
use super::validator::normalize_config_value;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ConfigDiffEntry {
    pub path: String,
    #[serde(rename = "oldValue")]
    pub old_value: Option<String>,
    #[serde(rename = "newValue")]
    pub new_value: Option<String>,
    pub sensitive: bool,
    pub change: String,
}

/// Replace non-empty values of sensitive keys with `***` (recursively).
pub fn redact_sensitive(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, val) in m.iter_mut() {
                if is_sensitive_field(k) && !val.is_null() {
                    if !scalar_to_string(val).is_empty() {
                        *val = Value::String("***".into());
                    }
                } else {
                    redact_sensitive(val);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(redact_sensitive),
        _ => {}
    }
}

pub fn compute_config_diff(current: &Value, proposed: &Value, redact: bool) -> Vec<ConfigDiffEntry> {
    let mut current = current.clone();
    let mut proposed = normalize_config_value(proposed);
    if redact {
        redact_sensitive(&mut current);
        redact_sensitive(&mut proposed);
    }
    compute_diff(Some(&current), Some(&proposed), "")
}

#[derive(PartialEq)]
enum Kind {
    Null,
    Bool,
    Integer,
    Float,
    String,
    Array,
    Object,
}

fn kind(v: &Value) -> Kind {
    match v {
        Value::Null => Kind::Null,
        Value::Bool(_) => Kind::Bool,
        Value::Number(n) if n.is_f64() => Kind::Float,
        Value::Number(_) => Kind::Integer,
        Value::String(_) => Kind::String,
        Value::Array(_) => Kind::Array,
        Value::Object(_) => Kind::Object,
    }
}

fn is_scalar(v: &Value) -> bool {
    !matches!(v, Value::Array(_) | Value::Object(_))
}

fn trim_dot(s: &str) -> String {
    s.trim_end_matches('.').to_string()
}

pub fn compute_diff(current: Option<&Value>, proposed: Option<&Value>, prefix: &str) -> Vec<ConfigDiffEntry> {
    let mut diffs = Vec::new();
    let (cur, prop) = match (current, proposed) {
        (None, None) => return diffs,
        (None, Some(p)) => {
            diffs.push(entry(trim_dot(prefix), None, Some(p), false, "added"));
            return diffs;
        }
        (Some(c), None) => {
            diffs.push(entry(trim_dot(prefix), Some(c), None, false, "removed"));
            return diffs;
        }
        (Some(c), Some(p)) => (c, p),
    };

    if let (Value::Object(co), Value::Object(po)) = (cur, prop) {
        let mut keys: Vec<&String> = co.keys().collect();
        for k in po.keys() {
            if !co.contains_key(k) {
                keys.push(k);
            }
        }
        for key in keys {
            let child = if prefix.is_empty() { key.clone() } else { format!("{prefix}{key}") };
            let cv = co.get(key);
            let pv = po.get(key);
            match (cv, pv) {
                (None, None) => continue,
                (Some(c), Some(p)) if kind(c) == kind(p) && !(is_scalar(c) && is_scalar(p)) => {
                    diffs.extend(compute_diff(Some(c), Some(p), &format!("{child}.")));
                }
                _ => {
                    if !token_value_equals(cv, pv) {
                        let change = if cv.is_none() {
                            "added"
                        } else if pv.is_none() {
                            "removed"
                        } else {
                            "changed"
                        };
                        let sensitive = is_sensitive_path(&child);
                        diffs.push(entry(child, cv, pv, sensitive, change));
                    }
                }
            }
        }
        return diffs;
    }

    if let (Value::Array(ca), Value::Array(pa)) = (cur, prop) {
        let path = trim_dot(prefix);
        if !array_value_equals(&path, ca, pa) {
            diffs.push(entry(path, Some(cur), Some(prop), is_sensitive_path(prefix), "changed"));
        }
        return diffs;
    }

    if !token_value_equals(Some(cur), Some(prop)) {
        diffs.push(entry(trim_dot(prefix), Some(cur), Some(prop), is_sensitive_path(prefix), "changed"));
    }
    diffs
}

fn entry(path: String, old: Option<&Value>, new: Option<&Value>, sensitive: bool, change: &str) -> ConfigDiffEntry {
    ConfigDiffEntry { path, old_value: Some(format_token(old)), new_value: Some(format_token(new)), sensitive, change: change.into() }
}

fn deep_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x.is_f64() || y.is_f64() {
                x.as_f64() == y.as_f64()
            } else {
                x == y
            }
        }
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| deep_equals(a, b)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).map(|w| deep_equals(v, w)).unwrap_or(false))
        }
        _ => a == b,
    }
}

fn is_empty_scalar(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

fn token_value_equals(a: Option<&Value>, b: Option<&Value>) -> bool {
    let null = Value::Null;
    if deep_equals(a.unwrap_or(&null), b.unwrap_or(&null)) {
        return true;
    }
    if is_empty_scalar(a) && is_empty_scalar(b) {
        return true;
    }
    match (a, b) {
        (Some(Value::Number(n)), Some(Value::String(s))) | (Some(Value::String(s)), Some(Value::Number(n))) if !n.is_f64() => {
            match (s.parse::<i64>(), n.as_i64()) {
                (Ok(x), Some(y)) => x == y,
                _ => false,
            }
        }
        _ => false,
    }
}

fn array_value_equals(path: &str, a: &[Value], b: &[Value]) -> bool {
    if a.len() == b.len() && a.iter().zip(b).all(|(x, y)| deep_equals(x, y)) {
        return true;
    }
    if is_tracker_slug_list(path) {
        let norm = |v: &[Value]| {
            let mut s: Vec<String> = v.iter().map(|x| scalar_to_string(x).to_lowercase()).collect();
            s.sort();
            s
        };
        return norm(a) == norm(b);
    }
    false
}

fn is_tracker_slug_list(path: &str) -> bool {
    let last = path.rsplit('.').next().unwrap_or(path);
    last.eq_ignore_ascii_case("synctrackers") || last.eq_ignore_ascii_case("disable_trackers")
}

fn is_sensitive_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let last = path.rsplit('.').next().unwrap_or(path);
    is_sensitive_field(last)
}

/// Scalar display: strings verbatim, booleans as `True`/`False`, containers as compact JSON.
fn scalar_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn format_token(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "-".into(),
        Some(v) => scalar_to_string(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scalar_changes() {
        let d = compute_diff(Some(&json!({"a": 1, "b": "x", "c": true})), Some(&json!({"a": "1", "b": "y", "c": false})), "");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].path, "b");
        assert_eq!(d[1].old_value.as_deref(), Some("True"));
        assert_eq!(d[1].new_value.as_deref(), Some("False"));
    }

    #[test]
    fn nested_added_removed_and_sensitive() {
        let d = compute_diff(
            Some(&json!({"Rutor": {"login": {"u": "a"}}, "gone": 1})),
            Some(&json!({"Rutor": {"login": {"u": "b"}}, "new": null, "x": [1]})),
            "",
        );
        assert_eq!(d[0], ConfigDiffEntry {
            path: "Rutor.login.u".into(),
            old_value: Some("a".into()),
            new_value: Some("b".into()),
            sensitive: true,
            change: "changed".into()
        });
        assert_eq!(d[1].change, "removed");
        assert_eq!(d[1].new_value.as_deref(), Some("-"));
        // "new": null vs missing → both empty scalars → no diff
        assert_eq!(d.len(), 3);
        assert_eq!(d[2].path, "x");
        assert_eq!(d[2].change, "added");
        assert_eq!(d[2].new_value.as_deref(), Some("[1]"));
    }

    #[test]
    fn tracker_lists_compare_as_sets() {
        let d = compute_diff(Some(&json!({"disable_trackers": ["rutor", "kinozal"]})), Some(&json!({"disable_trackers": ["Kinozal", "rutor"]})), "");
        assert!(d.is_empty());
        let d = compute_diff(Some(&json!({"tsuri": ["a", "b"]})), Some(&json!({"tsuri": ["b", "a"]})), "");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].path, "tsuri");
    }

    #[test]
    fn config_diff_against_defaults() {
        let current = serde_json::to_value(crab_core::config::AppOptions::default()).unwrap();
        let d = compute_config_diff(&current, &json!({"listenport": 9200, "apikey": "k"}), true);
        let paths: Vec<&str> = d.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["listenport", "apikey"]);
        assert_eq!(d[1].new_value.as_deref(), Some("***"));
        assert!(d[1].sensitive);
    }
}
