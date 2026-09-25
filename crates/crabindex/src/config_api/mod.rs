//! Configuration management API. The handlers are mounted at `/api/v1.0/config/*` but that
//! prefix is reachable only through the admin panel (`{admin.path}/api/config/*`, see
//! `crate::admin`); a direct request gets 404.
//!
//! Responses keep `null` members (the settings UI relies on them).

pub mod diff;
pub mod schema;
pub mod validator;

use axum::body::Bytes;
use axum::extract::RawQuery;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use crab_core::config;
use serde_json::{json, Value};

use validator::{options_to_value, parse_content_to_value, parse_options, validate_config_model, validate_config_object};

pub fn router() -> Router {
    Router::new()
        .route("/api/v1.0/config", get(get_config).post(save))
        .route("/api/v1.0/config/schema", get(schema_handler))
        .route("/api/v1.0/config/validate", post(validate))
        .route("/api/v1.0/config/diff", post(diff_handler))
        .route("/api/v1.0/config/render", post(render))
        .route("/api/v1.0/config/parse", post(parse))
        .route("/api/v1.0/config/format", post(format_handler))
}

fn config_json(payload: Value) -> Response {
    let mut r = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into()).into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    r
}

fn fail(msg: &str) -> Response {
    config_json(json!({ "ok": false, "error": msg }))
}

/// Request body: `{ content?: string, format?: string, data?: object }`.
#[derive(Debug, Default)]
pub struct ConfigSaveRequest {
    pub content: Option<String>,
    pub format: Option<String>,
    pub data: Option<Value>,
}

/// `Ok(None)` for an empty body or a non-object root; `Err` for malformed JSON.
pub fn parse_request_body(text: &str) -> Result<Option<ConfigSaveRequest>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let root: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let Value::Object(obj) = root else { return Ok(None) };
    let content = match obj.get("content") {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    let format = match obj.get("format") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => Some(other.to_string()),
    };
    let data = match obj.get("data") {
        Some(v @ Value::Object(_)) => Some(v.clone()),
        _ => None,
    };
    Ok(Some(ConfigSaveRequest { content, format, data }))
}

fn read_body(bytes: &Bytes) -> Result<Option<ConfigSaveRequest>, Response> {
    let text = String::from_utf8_lossy(bytes);
    parse_request_body(&text).map_err(|e| {
        crab_core::log::error(crab_core::log::cat::CONFIG, format!("config api: invalid request body: {e}"));
        crate::app::internal_error_response()
    })
}

fn resolve_payload(body: &ConfigSaveRequest) -> Result<Value, String> {
    if let Some(d) = &body.data {
        return Ok(d.clone());
    }
    match body.content.as_deref().filter(|c| !c.trim().is_empty()) {
        Some(c) => parse_content_to_value(c, body.format.as_deref()),
        None => Err("Укажите data или content".into()),
    }
}

fn example_path() -> &'static str {
    if std::path::Path::new("Data/example.yaml").exists() {
        "Data/example.yaml"
    } else {
        "Data/example.conf"
    }
}

fn source_info_value() -> Value {
    serde_json::to_value(config::get_config_source_info()).unwrap_or(Value::Null)
}

async fn schema_handler() -> Response {
    config_json(json!({ "ok": true, "schema": schema::get() }))
}

fn query_param(raw: Option<&str>, name: &str) -> Option<String> {
    url::form_urlencoded::parse(raw?.as_bytes()).find(|(k, _)| k == name).map(|(_, v)| v.into_owned())
}

async fn get_config(RawQuery(q): RawQuery) -> Response {
    let info = source_info_value();
    let fmt = query_param(q.as_deref(), "format")
        .or_else(|| info["format"].as_str().map(str::to_string))
        .unwrap_or_else(|| "yaml".into());
    let data = config::config_value(false);
    let content = config::render_config_value(&data, &fmt);
    config_json(json!({
        "ok": true,
        "path": info["path"],
        "format": info["format"],
        "displayFormat": fmt,
        "exists": info["exists"],
        "lastModifiedUtc": info["lastModifiedUtc"],
        "data": data,
        "content": content,
        "schema": schema::get(),
        "examplePath": example_path(),
        "sensitiveFields": schema::SENSITIVE_FIELD_NAMES,
        "note": "Полный конфиг. API доступен только из админ-панели.",
    }))
}

async fn validate(body: Bytes) -> Response {
    let body = match read_body(&body) {
        Ok(Some(b)) => b,
        Ok(None) => return fail("Тело запроса пусто"),
        Err(r) => return r,
    };
    let jo = match resolve_payload(&body) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let r = validate_config_object(&jo);
    config_json(json!({ "ok": r.ok, "error": r.error, "errors": r.errors, "warnings": r.warnings }))
}

async fn diff_handler(body: Bytes) -> Response {
    let body = match read_body(&body) {
        Ok(Some(b)) => b,
        Ok(None) => return fail("Тело запроса пусто"),
        Err(r) => return r,
    };
    let proposed = match resolve_payload(&body) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let validation = validate_config_object(&proposed);
    let diffs = diff::compute_config_diff(&config::config_value(false), &proposed, false);
    config_json(json!({
        "ok": true,
        "diffs": diffs,
        "changeCount": diffs.len(),
        "validation": {
            "ok": validation.ok,
            "error": validation.error,
            "errors": validation.errors,
            "warnings": validation.warnings,
        }
    }))
}

async fn render(body: Bytes) -> Response {
    let body = match read_body(&body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let Some(data) = body.as_ref().and_then(|b| b.data.clone()) else {
        return fail("Укажите data");
    };
    let fmt = body.and_then(|b| b.format).unwrap_or_else(|| "yaml".into());
    config_json(json!({ "ok": true, "content": config::render_config_value(&data, &fmt), "format": fmt }))
}

async fn parse(body: Bytes) -> Response {
    let body = match read_body(&body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let Some(body) = body.filter(|b| b.content.as_deref().map(|c| !c.trim().is_empty()).unwrap_or(false)) else {
        return fail("Укажите content");
    };
    match parse_content_to_value(body.content.as_deref().unwrap_or(""), body.format.as_deref()) {
        Ok(v) => config_json(json!({ "ok": true, "data": v })),
        Err(e) => fail(&e),
    }
}

/// Parse + validate + normalise; returns the normalised document.
fn prepare(jo: &Value) -> Result<Value, String> {
    let parsed = parse_options(jo)?;
    let v = validate_config_model(&parsed);
    if !v.ok {
        return Err(v.error.unwrap_or_else(|| "Ошибка валидации".into()));
    }
    Ok(options_to_value(&parsed))
}

async fn format_handler(body: Bytes) -> Response {
    let body = match read_body(&body) {
        Ok(Some(b)) => b,
        Ok(None) => return fail("Тело запроса пусто"),
        Err(r) => return r,
    };
    let jo = match resolve_payload(&body) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let fmt = body.format.clone().unwrap_or_else(|| "yaml".into());
    match prepare(&jo) {
        Ok(data) => {
            let content = config::render_config_value(&data, &fmt);
            config_json(json!({ "ok": true, "data": data, "content": content, "format": fmt }))
        }
        Err(e) => fail(&e),
    }
}

/// Validate and write the config file atomically, then reload it.
pub fn save_config_object(data: &Value, format: Option<&str>) -> Result<config::ConfigSourceInfo, String> {
    let jo = prepare(data)?;
    let info = config::get_config_source_info();
    let output_format = format.map(str::to_string).or_else(|| info.format.clone()).unwrap_or_else(|| "yaml".into());
    let target = info.path.clone().unwrap_or_else(|| {
        if output_format == "json" { config::CONFIG_FILE_JSON } else { config::CONFIG_FILE_YAML }.to_string()
    });
    let serialized = config::render_config_value(&jo, &output_format);
    config::write_atomically(&target, &serialized).map_err(|e| e.to_string())?;
    config::reload_from_disk(&target)?;
    Ok(config::get_config_source_info())
}

async fn save(body: Bytes) -> Response {
    let body = match read_body(&body) {
        Ok(Some(b)) => b,
        Ok(None) => return fail("Тело запроса пусто"),
        Err(r) => return r,
    };
    let jo = match resolve_payload(&body) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let format = body.format.clone();
    let result = tokio::task::spawn_blocking(move || save_config_object(&jo, format.as_deref())).await;
    match result {
        Ok(Ok(info)) => {
            let info = serde_json::to_value(info).unwrap_or(Value::Null);
            config_json(json!({
                "ok": true,
                "path": info["path"],
                "format": info["format"],
                "lastModifiedUtc": info["lastModifiedUtc"],
                "message": "Конфигурация сохранена. Изменения применятся автоматически.",
            }))
        }
        Ok(Err(e)) => fail(&e),
        Err(e) => fail(&e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_parsing() {
        assert!(parse_request_body("").unwrap().is_none());
        assert!(parse_request_body("[1]").unwrap().is_none());
        assert!(parse_request_body("{bad").is_err());
        let b = parse_request_body(r#"{"content":"a: 1","format":"yaml","data":[1]}"#).unwrap().unwrap();
        assert_eq!(b.content.as_deref(), Some("a: 1"));
        assert_eq!(b.format.as_deref(), Some("yaml"));
        assert!(b.data.is_none());
    }

    #[test]
    fn resolve_requires_data_or_content() {
        let b = ConfigSaveRequest::default();
        assert_eq!(resolve_payload(&b).unwrap_err(), "Укажите data или content");
    }

    #[test]
    fn prepare_rejects_invalid() {
        assert_eq!(prepare(&json!({"tracksmod": 5})).unwrap_err(), "tracksmod: допустимы только 0 или 1");
        assert_eq!(prepare(&json!({"tracksmod": 1})).unwrap()["tracksmod"], 1);
    }
}
