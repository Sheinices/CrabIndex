//! API key / DEV key extraction and validation.
//!
//! * apikey: `?apikey=` (raw query), `X-Api-Key` header, or `Authorization: Bearer …`.
//! * devkey: `X-Dev-Key` header or `?devkey=` (raw query).

use axum::http::HeaderMap;
use once_cell::sync::Lazy;
use regex::Regex;

static APIKEY_QUERY: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\?|&)apikey=([^&]+)").expect("regex"));
static DEVKEY_QUERY: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\?|&)devkey=([^&]+)").expect("regex"));

/// Constant-time string comparison (length leak only).
pub fn secure_equals(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let (a, b) = (a.as_bytes(), b.as_bytes());
            if a.len() != b.len() {
                return false;
            }
            a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
        }
        _ => false,
    }
}

/// Percent-decode a query value (`+` is kept as-is); returns the raw value on failure.
pub fn decode_query_value(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    match urlencoding::decode(raw) {
        Ok(s) => s.into_owned(),
        Err(_) => raw.to_string(),
    }
}

/// Query string with a leading `?` (empty when there is no query).
fn query_with_mark(raw_query: Option<&str>) -> String {
    match raw_query {
        Some(q) => format!("?{q}"),
        None => String::new(),
    }
}

fn first_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Key supplied by the client (query wins over headers).
pub fn api_key_from_request(headers: &HeaderMap, raw_query: Option<&str>) -> Option<String> {
    let q = query_with_mark(raw_query);
    if let Some(c) = APIKEY_QUERY.captures(&q) {
        return Some(decode_query_value(&c[2]));
    }
    if let Some(h) = first_header(headers, "x-api-key").filter(|h| !h.is_empty()) {
        return Some(h.trim().to_string());
    }
    if let Some(auth) = first_header(headers, "authorization") {
        if auth.len() >= 7 && auth[..7].eq_ignore_ascii_case("Bearer ") {
            return Some(auth[7..].trim().to_string());
        }
    }
    None
}

/// True when the request carries the configured DEV key (or no key is configured).
pub fn dev_key_matches(headers: &HeaderMap, raw_query: Option<&str>, configured: Option<&str>) -> bool {
    let Some(configured) = configured.filter(|c| !c.is_empty()) else {
        return true;
    };
    if let Some(h) = first_header(headers, "x-dev-key").filter(|h| !h.is_empty()) {
        return secure_equals(Some(h), Some(configured));
    }
    let q = query_with_mark(raw_query);
    if let Some(c) = DEVKEY_QUERY.captures(&q) {
        return secure_equals(Some(&decode_query_value(&c[2])), Some(configured));
    }
    false
}

/// apikey validator bound to a configured key.
pub struct ApiKeyValidator<'a> {
    pub configured: Option<&'a str>,
}

impl ApiKeyValidator<'_> {
    pub fn is_configured(&self) -> bool {
        self.configured.map(|k| !k.is_empty()).unwrap_or(false)
    }

    pub fn validate(&self, headers: &HeaderMap, raw_query: Option<&str>) -> bool {
        if !self.is_configured() {
            return true;
        }
        match api_key_from_request(headers, raw_query) {
            Some(p) if !p.is_empty() => secure_equals(Some(&p), self.configured),
            _ => false,
        }
    }
}

/// devkey validator bound to a configured key.
pub struct DevKeyValidator<'a> {
    pub configured: Option<&'a str>,
}

impl DevKeyValidator<'_> {
    pub fn is_configured(&self) -> bool {
        self.configured.map(|k| !k.is_empty()).unwrap_or(false)
    }

    pub fn validate(&self, headers: &HeaderMap, raw_query: Option<&str>) -> bool {
        dev_key_matches(headers, raw_query, self.configured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::network::tests::headers;

    #[test]
    fn api_key_sources() {
        let h = headers(&[]);
        assert_eq!(api_key_from_request(&h, Some("a=1&apikey=k%20y")), Some("k y".into()));
        assert_eq!(api_key_from_request(&h, Some("apikey=abc")), Some("abc".into()));
        assert_eq!(api_key_from_request(&h, Some("xapikey=abc")), None);
        let h = headers(&[("X-Api-Key", " hdr ")]);
        assert_eq!(api_key_from_request(&h, None), Some("hdr".into()));
        let h = headers(&[("Authorization", "bearer tok")]);
        assert_eq!(api_key_from_request(&h, None), Some("tok".into()));
    }

    #[test]
    fn api_key_validator() {
        let v = ApiKeyValidator { configured: Some("secret") };
        assert!(v.validate(&headers(&[]), Some("apikey=secret")));
        assert!(!v.validate(&headers(&[]), Some("apikey=wrong")));
        assert!(!v.validate(&headers(&[]), None));
        let open = ApiKeyValidator { configured: Some("") };
        assert!(!open.is_configured());
        assert!(open.validate(&headers(&[]), None));
    }

    #[test]
    fn dev_key_header_wins_over_query() {
        let h = headers(&[("X-Dev-Key", "wrong")]);
        assert!(!dev_key_matches(&h, Some("devkey=ok"), Some("ok")));
        assert!(dev_key_matches(&headers(&[]), Some("devkey=ok"), Some("ok")));
        assert!(dev_key_matches(&headers(&[]), None, None));
        assert!(!dev_key_matches(&headers(&[]), None, Some("ok")));
    }
}
