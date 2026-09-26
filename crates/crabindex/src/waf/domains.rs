//! Domain blocking: the site a browser request comes from (`Origin`, else `Referer`) and
//! rule matching (`example.com` covers `example.com` and every `*.example.com`).

/// Sites that are always refused (`403`, reason `domain`), for every client except loopback -
/// even whitelisted IPs, LAN and domains in the admin whitelist.
///
/// Compiled into the binary on purpose: this list can NOT be removed, overridden or
/// whitelisted through the admin API, `Data/waf.json` or `init.yaml`. The only way to change
/// it is to edit this constant and rebuild. Entries are lowercase and match subdomains too.
pub const BUILTIN_BLOCKED_DOMAINS: &[&str] = &[
    "ndst.pw",
    "diskstation.me",
    "krilzov.it",
    "myds.me",
    "lampa.stream",
    "bylampa.online",
    "abhq.ru",
    "abmsx.tech",
    "akter.black",
    "lampa.click",
    "lampa.land",
    "lampa1.ru",
    "line.pm",
    "nnmtv.pw",
    "tvigl.info",
    "uspeh.sbs",
    "usph.xyz",
    "xabb.ru",
    // FreeDNS shared zone: only this site and its subdomains, never all of mooo.com
    "lampaua.mooo.com",
];

/// Longest domain name.
pub const MAX_DOMAIN_LEN: usize = 253;

/// `host` is `rule` or a subdomain of it (label boundary: `notexample.com` ≠ `example.com`).
pub fn domain_matches(host: &str, rule: &str) -> bool {
    if rule.is_empty() || host.len() < rule.len() {
        return false;
    }
    host == rule || (host.ends_with(rule) && host.as_bytes()[host.len() - rule.len() - 1] == b'.')
}

/// The builtin entry covering `host`, if any.
pub fn builtin_blocked(host: &str) -> Option<&'static str> {
    BUILTIN_BLOCKED_DOMAINS.iter().copied().find(|d| domain_matches(host, d))
}

/// Host part of an `Origin` / `Referer` / `Host` value, normalised: lowercase, no scheme,
/// userinfo, port, path or trailing dot; IPv6 literals without brackets; IDN kept as is.
/// `None` for an empty value or `null` (opaque origin).
pub fn normalize_host(raw: &str) -> Option<String> {
    let s = raw.trim();
    let s = match s.find("://") {
        Some(i) => &s[i + 3..],
        None => s.strip_prefix("//").unwrap_or(s),
    };
    let end = s.find(['/', '?', '#', '\\']).unwrap_or(s.len());
    let mut s = &s[..end];
    if let Some(at) = s.rfind('@') {
        s = &s[at + 1..];
    }
    let host = if let Some(rest) = s.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        s.split(':').next().unwrap_or("")
    };
    let host = host.trim().trim_end_matches('.').to_lowercase();
    if host.is_empty() || host == "null" {
        return None;
    }
    Some(host)
}

/// The domain a request comes from: the `Origin` host, else the `Referer` host.
pub fn request_domain(origin: Option<&str>, referer: Option<&str>) -> Option<String> {
    origin.and_then(normalize_host).or_else(|| referer.and_then(normalize_host))
}

/// Validate and canonicalise an admin domain rule. Accepts a pasted URL (scheme, port and
/// path are stripped) and a leading `*.`; the result is letters, digits, hyphens and dots,
/// with at least one dot and at most 253 characters.
pub fn parse_rule(input: &str) -> Result<String, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("value: укажите домен".into());
    }
    let mut s = normalize_host(raw).unwrap_or_default();
    while let Some(rest) = s.strip_prefix("*.") {
        s = rest.to_string();
    }
    let s = s.trim_start_matches('.').to_string();
    let bad = || format!("value: некорректный домен «{raw}» (пример: example.com)");
    if s.is_empty() || s.chars().count() > MAX_DOMAIN_LEN || !s.contains('.') {
        return Err(bad());
    }
    for label in s.split('.') {
        if label.is_empty() || label.chars().count() > 63 || !label.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err(bad());
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(bad());
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_list_is_lowercase_and_valid() {
        assert_eq!(BUILTIN_BLOCKED_DOMAINS.len(), 19);
        for d in BUILTIN_BLOCKED_DOMAINS {
            assert_eq!(parse_rule(d).as_deref(), Ok(*d));
        }
    }

    #[test]
    fn suffix_matching_on_label_boundary() {
        assert!(domain_matches("ndst.pw", "ndst.pw"));
        assert!(domain_matches("a.b.ndst.pw", "ndst.pw"));
        assert!(!domain_matches("notndst.pw", "ndst.pw"));
        assert!(!domain_matches("ndst.pw.evil.com", "ndst.pw"));
        assert!(!domain_matches("pw", "ndst.pw"));
        assert_eq!(builtin_blocked("app.lampa.stream"), Some("lampa.stream"));
        assert_eq!(builtin_blocked("lampaua.mooo.com"), Some("lampaua.mooo.com"));
        assert_eq!(builtin_blocked("x.lampaua.mooo.com"), Some("lampaua.mooo.com"));
        assert_eq!(builtin_blocked("other.mooo.com"), None);
        assert_eq!(builtin_blocked("mylampa.stream"), None);
        assert_eq!(builtin_blocked("example.com"), None);
    }

    #[test]
    fn host_normalisation() {
        assert_eq!(normalize_host("https://App.NDST.pw:8443").as_deref(), Some("app.ndst.pw"));
        assert_eq!(normalize_host("http://user:pw@example.com./path?q=1#x").as_deref(), Some("example.com"));
        assert_eq!(normalize_host("example.com:9117").as_deref(), Some("example.com"));
        assert_eq!(normalize_host("http://[2001:db8::1]:80/").as_deref(), Some("2001:db8::1"));
        assert_eq!(normalize_host("https://пример.рф").as_deref(), Some("пример.рф"));
        assert_eq!(normalize_host("null"), None);
        assert_eq!(normalize_host("  "), None);
    }

    #[test]
    fn origin_wins_over_referer() {
        assert_eq!(request_domain(Some("https://a.com"), Some("https://b.com/x")).as_deref(), Some("a.com"));
        assert_eq!(request_domain(None, Some("https://b.com/x")).as_deref(), Some("b.com"));
        assert_eq!(request_domain(Some("null"), Some("https://b.com/x")).as_deref(), Some("b.com"));
        assert_eq!(request_domain(None, None), None);
    }

    #[test]
    fn rule_parsing() {
        assert_eq!(parse_rule("Example.COM").as_deref(), Ok("example.com"));
        assert_eq!(parse_rule("*.example.com").as_deref(), Ok("example.com"));
        assert_eq!(parse_rule("https://sub.example.com:8080/path?q").as_deref(), Ok("sub.example.com"));
        assert_eq!(parse_rule("example.com.").as_deref(), Ok("example.com"));
        assert_eq!(parse_rule("пример.рф").as_deref(), Ok("пример.рф"));
        for bad in ["", "localhost", "exa mple.com", "ex_ample.com", "-a.com", "a..com", "*.", format!("{}.com", "a".repeat(260)).as_str()] {
            assert!(parse_rule(bad).is_err(), "{bad}");
        }
    }
}
