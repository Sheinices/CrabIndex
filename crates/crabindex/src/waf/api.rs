//! Admin API under `{admin.path}/api/waf/…` (session and `X-Crab-Admin` are checked by the
//! admin middleware before these handlers run).

use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::net::IpAddr;

use super::bots::{self, Category};
use super::domains::{self, BUILTIN_BLOCKED_DOMAINS};
use super::net::{loopback_nets, IpNet};
use super::stats::{RequestFilter, MAX_IPS};
use super::store::{iso, BotList, DomainKind, ListEntry, ListKind};
use super::{client_ip, save_in_background, WAF};
use crate::admin::json_response;
use crate::config_api::schema::MAX_WAF_HISTORY;

const MAX_BODY: usize = 16 * 1024;
const DEFAULT_LIMIT: usize = 200;
/// Longest accepted ban / rule lifetime (10 years).
const MAX_MINUTES: i64 = 10 * 365 * 24 * 60;
const MAX_COMMENT: usize = 200;
const LIST_EXPECTED: &str = "list: blacklist, whitelist, domainBlacklist or domainWhitelist expected";
const BOT_LIST_EXPECTED: &str = "list: botBlocked or botAllowed expected";
/// Rows of `GET waf/bots`.
const MAX_BOT_ROWS: usize = 500;

/// `list` of `waf/rules`: an IP list or an admin domain list.
enum AnyList {
    Ip(ListKind),
    Domain(DomainKind),
}

fn parse_list(s: &str) -> Option<AnyList> {
    ListKind::parse(s).map(AnyList::Ip).or_else(|| DomainKind::parse(s).map(AnyList::Domain))
}

/// A domain rule accepted for `kind`: builtin blocked domains can be neither allowed nor
/// (redundantly) blocked from the admin API.
pub fn domain_rule(kind: DomainKind, value: &str) -> Result<String, String> {
    let d = domains::parse_rule(value)?;
    if let Some(b) = domains::builtin_blocked(&d) {
        return Err(match kind {
            DomainKind::Whitelist if b == d => format!("{d} заблокирован встроенным списком и не может быть разрешён"),
            DomainKind::Whitelist => format!("{d} (поддомен {b}) заблокирован встроенным списком и не может быть разрешён"),
            DomainKind::Blacklist => format!("{d} уже заблокирован встроенным списком"),
        });
    }
    Ok(d)
}

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
                origin: param(&q, "origin").map(|s| s.trim().to_string()),
                host: param(&q, "host").map(|s| s.trim().to_string()),
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
                    "domainBlacklist": f.domain_blacklist.iter().map(entry_json).collect::<Vec<_>>(),
                    "domainWhitelist": f.domain_whitelist.iter().map(entry_json).collect::<Vec<_>>(),
                    "builtinDomains": BUILTIN_BLOCKED_DOMAINS,
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
            let removed = match param(&q, "list").and_then(parse_list) {
                None => return bad(LIST_EXPECTED),
                Some(AnyList::Ip(kind)) => {
                    let Some(net) = param(&q, "value").and_then(IpNet::parse) else { return bad("value: invalid IP address or CIDR") };
                    WAF.remove_rule(kind, net)
                }
                Some(AnyList::Domain(kind)) => {
                    let d = match domains::parse_rule(param(&q, "value").unwrap_or("")) {
                        Ok(d) => d,
                        Err(e) => return bad(e),
                    };
                    if BUILTIN_BLOCKED_DOMAINS.contains(&d.as_str()) {
                        return bad(format!("{d} входит во встроенный список и не может быть удалён"));
                    }
                    WAF.remove_domain(kind, &d)
                }
            };
            if !removed {
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
        (Method::GET, "bots") => json_response(StatusCode::OK, bots_overview(now)),
        (Method::POST, "bots/category") => {
            let v = match body_json(req).await {
                Ok(v) => v,
                Err(r) => return r,
            };
            let Some(c) = Category::parse(str_field(&v, "id")) else {
                return bad(format!("id: {} expected", Category::ALL.map(Category::id).join(", ")));
            };
            let Some(block) = v["block"].as_bool() else { return bad("block: true or false expected") };
            if WAF.set_bot_category(c, block) {
                save_in_background();
            }
            ok()
        }
        (Method::POST, "bots/rules") => {
            let v = match body_json(req).await {
                Ok(v) => v,
                Err(r) => return r,
            };
            match add_bot_rule(&v, now) {
                Ok(()) => {
                    save_in_background();
                    ok()
                }
                Err(e) => bad(e),
            }
        }
        (Method::DELETE, "bots/rules") => {
            let Some(kind) = param(&q, "list").and_then(BotList::parse) else { return bad(BOT_LIST_EXPECTED) };
            let value = param(&q, "value").unwrap_or("").trim();
            if value.is_empty() {
                return bad("value: required");
            }
            if !WAF.remove_bot(kind, value) {
                return not_found();
            }
            save_in_background();
            ok()
        }
        (Method::POST, "bots/robots") => {
            let v = match body_json(req).await {
                Ok(v) => v,
                Err(r) => return r,
            };
            let Some(disallow) = v["disallow"].as_bool() else { return bad("disallow: true or false expected") };
            if WAF.set_robots_disallow(disallow) {
                save_in_background();
            }
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
    let kind = match parse_list(str_field(v, "list")).ok_or(LIST_EXPECTED)? {
        AnyList::Ip(kind) => kind,
        AnyList::Domain(kind) => {
            let d = domain_rule(kind, str_field(v, "value"))?;
            let expires = expiry(v, "expiresMinutes", now, false)?;
            WAF.upsert_domain(kind, d, clip(str_field(v, "comment"), MAX_COMMENT), expires, now);
            return Ok(());
        }
    };
    let net = IpNet::parse(str_field(v, "value")).ok_or("value: invalid IP address or CIDR")?;
    let expires = expiry(v, "expiresMinutes", now, false)?;
    if kind == ListKind::Blacklist {
        self_protection(&net, you)?;
    }
    WAF.upsert_rule(kind, net, clip(str_field(v, "comment"), MAX_COMMENT), expires, now);
    Ok(())
}

pub fn add_bot_rule(v: &Value, now: DateTime<Utc>) -> Result<(), String> {
    let kind = BotList::parse(str_field(v, "list")).ok_or(BOT_LIST_EXPECTED)?;
    let value = bots::parse_rule(str_field(v, "value"))?;
    let expires = expiry(v, "expiresMinutes", now, false)?;
    WAF.upsert_bot(kind, value, clip(str_field(v, "comment"), MAX_COMMENT), expires, now);
    Ok(())
}

/// `GET waf/bots` payload: category cards, seen bots, rules and the builtin catalog.
fn bots_overview(now: DateTime<Utc>) -> Value {
    let rows = WAF.stats.bots.aggregates();
    let totals = WAF.stats.bots.category_totals();
    let rules = WAF.snapshot(now);
    let blocked_cats: Vec<Category> = rules.bot_block_categories.iter().filter_map(|c| Category::parse(c)).collect();
    let categories: Vec<Value> = Category::ALL
        .iter()
        .enumerate()
        .map(|(i, c)| {
            json!({
                "id": c.id(),
                "label": c.label(),
                "description": c.description(),
                "requests": totals[i].0,
                "blocked": totals[i].1,
                "blockedCategory": blocked_cats.contains(c),
                "botCount": rows.iter().filter(|r| r.category == *c).count(),
            })
        })
        .collect();
    let bots_json: Vec<Value> = rows
        .iter()
        .take(MAX_BOT_ROWS)
        .map(|a| {
            let hit = bots::BotHit { category: a.category, name: a.name.clone() };
            let mut v = bots::bot_json(a);
            v["status"] = json!(WAF.bot_status(&hit, &a.samples, now));
            v
        })
        .collect();
    let mut catalog = serde_json::Map::new();
    for c in Category::ALL {
        let names = bots::catalog_names(c);
        if !names.is_empty() {
            catalog.insert(c.id().into(), json!(names));
        }
    }
    json!({
        "categories": categories,
        "bots": bots_json,
        "rules": {
            "botBlockCategories": rules.bot_block_categories,
            "botBlocked": rules.bot_blocked.iter().map(entry_json).collect::<Vec<_>>(),
            "botAllowed": rules.bot_allowed.iter().map(entry_json).collect::<Vec<_>>(),
            "robotsDisallow": rules.robots_disallow,
        },
        "catalog": catalog,
    })
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
