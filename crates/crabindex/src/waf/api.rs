//! Admin API under `{admin.path}/api/waf/…` (session and `X-Crab-Admin` are checked by the
//! admin middleware before these handlers run).

use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::net::IpAddr;

use super::net::{loopback_nets, IpNet};
use super::stats::{RequestFilter, MAX_IPS};
use super::store::{iso, ListEntry, ListKind};
use super::{client_ip, save_in_background, WAF};
use crate::admin::json_response;
use crate::config_api::schema::MAX_WAF_HISTORY;

const MAX_BODY: usize = 16 * 1024;
const DEFAULT_LIMIT: usize = 200;
/// Longest accepted ban / rule lifetime (10 years).
const MAX_MINUTES: i64 = 10 * 365 * 24 * 60;
const MAX_COMMENT: usize = 200;

fn query_map(raw: Option<&str>) -> Vec<(String, String)> {
    raw.map(|q| url::form_urlencoded::parse(q.as_bytes()).into_owned().collect()).unwrap_or_default()
}

fn param<'a>(q: &'a [(String, String)], name: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn limit(q: &[(String, String)], max: usize) -> usize {
    param(q, "limit").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(DEFAULT_LIMIT).clamp(1, max)
}

fn ok() -> Response {
    json_response(StatusCode::OK, json!({ "ok": true }))
}

fn bad(msg: impl Into<String>) -> Response {
    json_response(StatusCode::BAD_REQUEST, json!({ "ok": false, "error": msg.into() }))
}

fn not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, json!({ "ok": false, "error": "not found" }))
}

async fn body_json(req: Request) -> Result<Value, Response> {
    let bytes = axum::body::to_bytes(req.into_body(), MAX_BODY).await.map_err(|_| bad("body too large"))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| bad("invalid JSON body"))?;
    if !v.is_object() {
        return Err(bad("invalid JSON body"));
    }
    Ok(v)
}

/// Integer from a JSON number or numeric string; `Ok(None)` when absent / null / empty.
fn int_field(v: &Value, name: &str) -> Result<Option<i64>, String> {
    match &v[name] {
        Value::Null => Ok(None),
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)).map(Some).ok_or(format!("{name}: integer expected")),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) => s.trim().parse::<i64>().map(Some).map_err(|_| format!("{name}: integer expected")),
        _ => Err(format!("{name}: integer expected")),
    }
}

fn str_field<'a>(v: &'a Value, name: &str) -> &'a str {
    v[name].as_str().unwrap_or("").trim()
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Refuse lists/bans that would lock out loopback or the admin themselves.
pub fn self_protection(net: &IpNet, you: Option<IpAddr>) -> Result<(), String> {
    if loopback_nets().iter().any(|l| l.overlaps(net)) {
        return Err(format!("{net} includes loopback"));
    }
    if let Some(y) = you {
        if net.contains(y) {
            return Err(format!("{net} includes your own IP ({y})"));
        }
    }
    Ok(())
}

fn entry_json(e: &ListEntry) -> Value {
    json!({ "value": e.value, "comment": e.comment, "created": iso(&e.created), "expires": e.expires.as_ref().map(iso) })
}

/// `sub` is the path after `waf/` (`overview`, `rules`, …).
pub async fn handle(req: Request, sub: &str) -> Response {
    let method = req.method().clone();
    let q = query_map(req.uri().query());
    let c = crate::conf();
    let now = Utc::now();
    match (method, sub.trim_end_matches('/')) {
        (Method::GET, "overview") => {
            let window = match param(&q, "window").map(|w| w.trim().to_ascii_lowercase()) {
                Some(w) if w == "24h" || w == "1d" || w == "1440m" => 1440,
                _ => 60,
            };
            let mut v = WAF.stats.overview(window, now);
            if let Value::Object(m) = &mut v {
                m.insert("enabled".into(), json!(c.waf.enable));
                m.insert("logRequests".into(), json!(c.waf.logRequests));
            }
            json_response(StatusCode::OK, v)
        }
        (Method::GET, "requests") => {
            let f = RequestFilter {
                ip: param(&q, "ip").map(|s| s.trim().to_string()),
                path: param(&q, "path").map(|s| s.trim().to_string()),
                status: param(&q, "status").map(|s| s.trim().to_string()),
                blocked: param(&q, "blocked").map(|s| s.trim().to_string()),
                limit: limit(&q, MAX_WAF_HISTORY as usize),
            };
            json_response(StatusCode::OK, Value::Array(WAF.stats.requests(&f)))
        }
        (Method::GET, "ips") => json_response(StatusCode::OK, Value::Array(ips(&c.waf, &q, now))),
        (Method::GET, "rules") => {
            let f = WAF.snapshot(now);
            json_response(
                StatusCode::OK,
                json!({
                    "blacklist": f.blacklist.iter().map(entry_json).collect::<Vec<_>>(),
                    "whitelist": f.whitelist.iter().map(entry_json).collect::<Vec<_>>(),
                    "bans": f.bans.iter().map(|b| json!({ "ip": b.ip, "reason": b.reason, "created": iso(&b.created), "expires": iso(&b.expires) })).collect::<Vec<_>>(),
                    "config": serde_json::to_value(&c.waf).unwrap_or(Value::Null),
                    "you": client_ip(&req).map(|i| i.to_string()),
                }),
            )
        }
        (Method::POST, "rules") => {
            let you = client_ip(&req);
            let v = match body_json(req).await {
                Ok(v) => v,
                Err(r) => return r,
            };
            match add_rule(&v, you, now) {
                Ok(()) => {
                    save_in_background();
                    ok()
                }
                Err(e) => bad(e),
            }
        }
        (Method::DELETE, "rules") => {
            let Some(kind) = param(&q, "list").and_then(ListKind::parse) else { return bad("list: blacklist or whitelist expected") };
            let Some(net) = param(&q, "value").and_then(IpNet::parse) else { return bad("value: invalid IP address or CIDR") };
            if !WAF.remove_rule(kind, net) {
                return not_found();
            }
            save_in_background();
            ok()
        }
        (Method::POST, "ban") => {
            let you = client_ip(&req);
            let v = match body_json(req).await {
                Ok(v) => v,
                Err(r) => return r,
            };
            match add_ban(&v, you, now) {
                Ok(()) => {
                    save_in_background();
                    ok()
                }
                Err(e) => bad(e),
            }
        }
        (Method::DELETE, "ban") => {
            let Some(ip) = param(&q, "ip").and_then(|s| s.trim().parse::<IpAddr>().ok()) else { return bad("ip: invalid IP address") };
            if !WAF.unban(ip) {
                return not_found();
            }
            save_in_background();
            ok()
        }
        (Method::POST, "reset") => {
            WAF.stats.reset();
            ok()
        }
        _ => json_response(StatusCode::NOT_FOUND, json!({ "error": "not found" })),
    }
}

fn expiry(v: &Value, name: &str, now: DateTime<Utc>, required: bool) -> Result<Option<DateTime<Utc>>, String> {
    match int_field(v, name)? {
        None | Some(0) if !required => Ok(None),
        None => Err(format!("{name}: required")),
        Some(m) if m < 1 || m > MAX_MINUTES => Err(format!("{name}: 1-{MAX_MINUTES} expected")),
        Some(m) => Ok(Some(now + Duration::minutes(m))),
    }
}

pub fn add_rule(v: &Value, you: Option<IpAddr>, now: DateTime<Utc>) -> Result<(), String> {
    let kind = ListKind::parse(str_field(v, "list")).ok_or("list: blacklist or whitelist expected")?;
    let net = IpNet::parse(str_field(v, "value")).ok_or("value: invalid IP address or CIDR")?;
    let expires = expiry(v, "expiresMinutes", now, false)?;
    if kind == ListKind::Blacklist {
        self_protection(&net, you)?;
    }
    WAF.upsert_rule(kind, net, clip(str_field(v, "comment"), MAX_COMMENT), expires, now);
    Ok(())
}

pub fn add_ban(v: &Value, you: Option<IpAddr>, now: DateTime<Utc>) -> Result<(), String> {
    let net = IpNet::parse(str_field(v, "ip")).filter(IpNet::is_host).ok_or("ip: invalid IP address")?;
    let expires = expiry(v, "minutes", now, true)?.ok_or("minutes: required")?;
    self_protection(&net, you)?;
    let reason = match str_field(v, "reason") {
        "" => "manual".to_string(),
        r => clip(r, 64),
    };
    WAF.ban(net.addr(), &reason, expires, now);
    Ok(())
}

fn ips(cfg: &crab_core::config::WafSettings, q: &[(String, String)], now: DateTime<Utc>) -> Vec<Value> {
    let mut rows = WAF.stats.ip_aggregates();
    match param(q, "sort").map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("blocked") => rows.sort_by(|a, b| b.1.blocked.cmp(&a.1.blocked).then_with(|| b.1.requests.cmp(&a.1.requests))),
        Some("lastseen") => rows.sort_by(|a, b| b.1.last_seen.cmp(&a.1.last_seen)),
        _ => rows.sort_by(|a, b| b.1.requests.cmp(&a.1.requests).then_with(|| b.1.last_seen.cmp(&a.1.last_seen))),
    }
    rows.truncate(limit(q, MAX_IPS));
    rows.into_iter()
        .map(|(ip, a)| {
            let (state, ban) = match ip.parse::<IpAddr>() {
                Ok(addr) => WAF.state_of(cfg, addr, now),
                Err(_) => ("normal", None),
            };
            json!({
                "ip": ip,
                "requests": a.requests,
                "blocked": a.blocked,
                "errors": a.errors,
                "firstSeen": iso(&a.first_seen),
                "lastSeen": iso(&a.last_seen),
                "lastPath": a.last_path,
                "ua": a.ua,
                "state": state,
                "banExpires": ban.as_ref().map(iso),
            })
        })
        .collect()
}
