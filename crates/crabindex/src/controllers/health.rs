//! Health, version and identity endpoints.

use axum::extract::RawQuery;
use axum::http::HeaderMap;
use axum::routing::any;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use crab_core::trackers::{self, JobSnapshot};
use serde_json::{json, Map, Value};

use crate::security::keys::{api_key_from_request, secure_equals};
use crate::version;

pub fn router() -> Router {
    Router::new()
        .route("/health", any(health))
        .route("/health/background-jobs", any(background_jobs))
        .route("/version", any(version_info))
        .route("/lastupdatedb", any(last_update_db))
        .route("/api/v1.0/conf", any(conf))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "OK" }))
}

/// Round-trip ("o") timestamp: `2025-01-02T03:04:05.1234567Z`.
pub fn format_roundtrip(dt: &DateTime<Utc>) -> String {
    format!("{}.{:07}Z", dt.format("%Y-%m-%dT%H:%M:%S"), dt.timestamp_subsec_nanos() / 100)
}

pub fn job_json(j: &JobSnapshot, now: DateTime<Utc>) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(j.key));
    m.insert("tracker".into(), json!(j.tracker));
    m.insert("job".into(), json!(j.job_label));
    m.insert("startedAtUtc".into(), json!(format_roundtrip(&j.started_at_utc)));
    m.insert("elapsedSeconds".into(), json!((now - j.started_at_utc).num_seconds().max(0)));
    m.insert("pagesCompleted".into(), json!(j.pages_completed));
    m.insert("pagesTotal".into(), json!(j.pages_total));
    if let Some(p) = trackers::percent(j.pages_completed, j.pages_total) {
        m.insert("percent".into(), json!(p));
    }
    if let Some(c) = &j.current_category {
        m.insert("currentCategory".into(), json!(c));
    }
    if let Some(p) = j.current_page {
        m.insert("currentPage".into(), json!(p));
    }
    m.insert("summary".into(), json!(trackers::format_summary(j)));
    Value::Object(m)
}

async fn background_jobs() -> Json<Value> {
    let now = Utc::now();
    let jobs: Vec<Value> = trackers::get_active_jobs().iter().map(|j| job_json(j, now)).collect();
    Json(json!({ "jobs": jobs }))
}

async fn version_info() -> Json<Value> {
    Json(json!({
        "version": version::VERSION,
        "gitSha": version::GIT_SHA,
        "gitBranch": version::GIT_BRANCH,
        "buildDate": version::BUILD_DATE,
    }))
}

async fn last_update_db() -> Json<Value> {
    let last = tokio::task::spawn_blocking(|| {
        crab_core::fdb::MASTER_DB.iter().map(|e| e.value().updateTime).max()
    })
    .await
    .ok()
    .flatten();
    let s = match last {
        Some(dt) => dt.format("%d.%m.%Y %H:%M").to_string(),
        None => "01.01.2000 01:01".to_string(),
    };
    Json(json!({ "lastupdatedb": s }))
}

fn query_param(raw: Option<&str>, name: &str) -> Option<String> {
    let raw = raw?;
    url::form_urlencoded::parse(raw.as_bytes()).find(|(k, _)| k == name).map(|(_, v)| v.into_owned())
}

/// Identity probe: tells clients this is an indexer of this family, whether an apikey is
/// configured and whether the supplied one is valid.
pub fn conf_payload(query_apikey: Option<&str>, headers: &HeaderMap, raw_query: Option<&str>, configured: Option<&str>) -> Value {
    let provided = match query_apikey.filter(|k| !k.trim().is_empty()) {
        Some(k) => Some(k.trim().to_string()),
        None => api_key_from_request(headers, raw_query),
    };
    let configured = configured.filter(|k| !k.trim().is_empty());
    let is_configured = configured.is_some();
    json!({
        "jacred": true,
        "configured": is_configured,
        "apikey": !is_configured || secure_equals(provided.as_deref(), configured),
        "version": version::VERSION,
    })
}

async fn conf(headers: HeaderMap, RawQuery(q): RawQuery) -> Json<Value> {
    let c = crate::conf();
    let qk = query_param(q.as_deref(), "apikey");
    Json(conf_payload(qk.as_deref(), &headers, q.as_deref(), c.apikey.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invoke(apikey: Option<&str>, configured: Option<&str>) -> Value {
        conf_payload(apikey, &HeaderMap::new(), None, configured)
    }

    #[test]
    fn conf_without_server_apikey_returns_open_access() {
        let j = invoke(None, None);
        assert_eq!(j["jacred"], true);
        assert_eq!(j["configured"], false);
        assert_eq!(j["apikey"], true);
        assert_eq!(j["version"], version::VERSION);
    }

    #[test]
    fn conf_with_matching_apikey_returns_valid() {
        let j = invoke(Some("secret-key"), Some("secret-key"));
        assert_eq!(j["configured"], true);
        assert_eq!(j["apikey"], true);
        assert_eq!(j["version"], version::VERSION);
    }

    #[test]
    fn conf_with_missing_apikey_when_configured_returns_invalid() {
        let j = invoke(None, Some("secret-key"));
        assert_eq!(j["jacred"], true);
        assert_eq!(j["configured"], true);
        assert_eq!(j["apikey"], false);
    }

    #[test]
    fn conf_with_wrong_apikey_when_configured_returns_invalid() {
        let j = invoke(Some("wrong"), Some("secret-key"));
        assert_eq!(j["configured"], true);
        assert_eq!(j["apikey"], false);
    }

    #[test]
    fn roundtrip_format() {
        let dt = DateTime::parse_from_rfc3339("2025-01-02T03:04:05.123456789Z").unwrap().with_timezone(&Utc);
        assert_eq!(format_roundtrip(&dt), "2025-01-02T03:04:05.1234567Z");
    }

    #[test]
    fn job_json_omits_nulls() {
        let j = JobSnapshot {
            key: "rutor:parseall".into(),
            tracker: "rutor".into(),
            job_label: "ParseAll".into(),
            started_at_utc: Utc::now(),
            pages_completed: 0,
            pages_total: 0,
            last_activity_utc: None,
            current_category: None,
            current_page: None,
        };
        let v = job_json(&j, Utc::now());
        assert!(v.get("percent").is_none());
        assert!(v.get("currentPage").is_none());
        assert_eq!(v["summary"], "running");
    }
}
