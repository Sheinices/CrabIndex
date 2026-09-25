//! Configuration validation and normalisation of user supplied config documents.

use crab_core::config::{self, AppOptions};
use serde::Serialize;
use serde_json::Value;

use super::schema;

#[derive(Clone, Debug, Default, Serialize)]
pub struct ValidationResult {
    pub ok: bool,
    pub error: Option<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// `listenport` is a u16 in the model; out-of-range numbers are mapped to 0 so that the
/// model check reports the friendly range message instead of a type error.
fn clamp_listenport(v: &mut Value) {
    let Value::Object(m) = v else { return };
    let Some(key) = m.keys().find(|k| k.eq_ignore_ascii_case("listenport")).cloned() else { return };
    let n = match &m[&key] {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    if let Some(n) = n {
        if !(0.0..=65535.0).contains(&n) {
            m.insert(key, Value::from(0));
        }
    }
}

/// Deserialize a config document over the defaults.
pub fn parse_options(data: &Value) -> Result<AppOptions, String> {
    let mut v = data.clone();
    clamp_listenport(&mut v);
    config::options_from_value(v)
}

pub fn options_to_value(o: &AppOptions) -> Value {
    serde_json::to_value(o).unwrap_or(Value::Object(Default::default()))
}

pub fn validate_config_model(parsed: &AppOptions) -> ValidationResult {
    let mut r = ValidationResult::default();
    schema::validate_against_schema(parsed, &mut r.errors, &mut r.warnings);
    r.ok = r.errors.is_empty();
    if !r.ok {
        r.error = r.errors.first().cloned();
    }
    r
}

pub fn validate_config_object(data: &Value) -> ValidationResult {
    match parse_options(data) {
        Ok(parsed) => validate_config_model(&parsed),
        Err(e) => ValidationResult { ok: false, error: Some(e.clone()), errors: vec![e], warnings: vec![] },
    }
}

/// Parse raw text (yaml/json) to a full, normalised config document.
pub fn parse_content_to_value(content: &str, format: Option<&str>) -> Result<Value, String> {
    if content.trim().is_empty() {
        return Err("Укажите data или content".into());
    }
    let fmt = match format {
        Some(f) => f.to_string(),
        None => config::detect_config_format(content, "yaml"),
    };
    let fmt = if fmt.eq_ignore_ascii_case("yaml") { "yaml" } else { "json" };
    let raw = config::parse_to_value(content, fmt)?;
    let parsed = parse_options(&raw)?;
    Ok(options_to_value(&parsed))
}

/// Round-trip through the model (fills defaults, drops unknown keys); unchanged on failure.
pub fn normalize_config_value(proposed: &Value) -> Value {
    match parse_options(proposed) {
        Ok(o) => options_to_value(&o),
        Err(_) => proposed.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_partial_document() {
        let r = validate_config_object(&json!({ "listenport": 9118, "rutor": { "reqMinute": 5 } }));
        assert!(r.ok, "{r:?}");
        assert!(r.error.is_none());
    }

    #[test]
    fn out_of_range_port_reports_range_error() {
        let r = validate_config_object(&json!({ "listenport": 70000 }));
        assert!(!r.ok);
        assert_eq!(r.error.as_deref(), Some("listenport: значение должно быть от 1 до 65535"));
        let r = validate_config_object(&json!({ "listenport": "0" }));
        assert_eq!(r.errors, vec!["listenport: значение должно быть от 1 до 65535"]);
    }

    #[test]
    fn type_errors_are_reported() {
        let r = validate_config_object(&json!({ "tsuri": { "a": 1 } }));
        assert!(!r.ok);
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn parse_content_detects_format() {
        let v = parse_content_to_value("listenport: 9200\n", None).unwrap();
        assert_eq!(v["listenport"], 9200);
        assert_eq!(v["Rutor"]["host"], "http://rutor.info");
        let v = parse_content_to_value("{\"apikey\":\"x\"}", None).unwrap();
        assert_eq!(v["apikey"], "x");
        assert!(parse_content_to_value("{bad", Some("json")).is_err());
    }
}
