//! String / hash helpers.

use md5::{Digest, Md5};

/// Empty or whitespace-only.
pub fn is_blank(s: &str) -> bool {
    s.trim().is_empty()
}

/// Search key: lowercase, keep [a-zа-я0-9ё], ё→е, щ→ш. None when empty.
pub fn search_name(val: &str) -> Option<String> {
    if is_blank(val) {
        return None;
    }
    let lower = val.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    for c in lower.chars() {
        let keep = c.is_ascii_lowercase()
            || c.is_ascii_uppercase()
            || c.is_ascii_digit()
            || ('а'..='я').contains(&c)
            || ('А'..='Я').contains(&c)
            || c == 'ё'
            || c == 'Ё';
        if keep {
            out.push(match c {
                'ё' => 'е',
                'щ' => 'ш',
                x => x,
            });
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// `search_name` returning "" instead of None.
pub fn search_name_or_empty(val: &str) -> String {
    search_name(val).unwrap_or_default()
}

/// Lowercase hex MD5 of UTF-8 text.
pub fn md5(text: &str) -> String {
    let mut h = Md5::new();
    h.update(text.as_bytes());
    hex::encode(h.finalize())
}

pub fn md5_bytes(data: &[u8]) -> String {
    let mut h = Md5::new();
    h.update(data);
    hex::encode(h.finalize())
}

/// MD5 of the cleaned lowercase name plus `:kind`.
pub fn name_to_hash(name_or_originalname: &str, kind: &str) -> String {
    let decoded = html_decode(name_or_originalname);
    let cleaned = crate::rx::replace_i(&decoded, "[^а-яA-Z0-9]+", "");
    md5(&format!("{}:{kind}", cleaned.to_lowercase().trim()))
}

/// Decode HTML entities.
pub fn html_decode(s: &str) -> String {
    html_escape::decode_html_entities(s).into_owned()
}

/// Percent-encode a URL component.
pub fn url_encode(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

pub fn url_decode(s: &str) -> String {
    urlencoding::decode(&s.replace('+', " ")).map(|c| c.into_owned()).unwrap_or_else(|_| s.to_string())
}

/// Text before the first `end`.
pub fn find_start_text(data: &str, end: &str) -> Option<String> {
    data.find(end).map(|i| data[..i].to_string())
}

/// Text from the first `start` (optionally up to `end`).
pub fn find_last_text(data: &str, start: &str, end: Option<&str>) -> Option<String> {
    let i = data.find(start)?;
    let res = &data[i..];
    match end {
        None => Some(res.to_string()),
        Some(e) => find_start_text(res, e),
    }
}

/// Substring between `start` and `end` (exclusive), a common parser idiom.
pub fn between<'a>(data: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = data.find(start)? + start.len();
    let rest = &data[i..];
    let j = rest.find(end)?;
    Some(&rest[..j])
}

/// Truncate on a char boundary.
pub fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Lowercase 40-hex BitTorrent v1 infohash from a magnet link.
/// Accepts hex or base32 `urn:btih:`.
pub fn magnet_infohash(magnet: &str) -> Option<String> {
    if is_blank(magnet) {
        return None;
    }
    let lower = magnet.to_ascii_lowercase();
    let idx = lower.find("urn:btih:")?;
    let rest = &magnet[idx + "urn:btih:".len()..];
    let token: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    if token.len() == 40 && token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(token.to_ascii_lowercase());
    }
    if token.len() == 32 {
        let bytes = base32::decode(base32::Alphabet::Rfc4648 { padding: false }, &token.to_ascii_uppercase())?;
        if bytes.len() == 20 {
            return Some(hex::encode(bytes));
        }
    }
    None
}

pub fn is_valid_hex40(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Parse a trimmed integer (0 on failure).
pub fn parse_i32(s: &str) -> i32 {
    s.trim().parse().unwrap_or(0)
}

/// Parse a trimmed integer.
pub fn try_i32(s: &str) -> Option<i32> {
    s.trim().parse().ok()
}

/// Human size like "1.46 GB" from bytes.
pub fn format_bytes(bytes: i64) -> String {
    const SUFFIX: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut b = bytes;
    let mut dbl = bytes as f64;
    let mut i = 0;
    while i < SUFFIX.len() - 1 && b >= 1024 {
        dbl = b as f64 / 1024.0;
        b /= 1024;
        i += 1;
    }
    format!("{:.2} {}", dbl, SUFFIX[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_name_rules() {
        assert_eq!(search_name("Щит Ёжика: 2!").as_deref(), Some("шитежика2"));
        assert_eq!(search_name("  "), None);
        assert_eq!(search_name("!!!"), None);
    }

    #[test]
    fn magnet_hash() {
        assert_eq!(
            magnet_infohash("magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=x").as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef01")
        );
    }

    #[test]
    fn md5_hex() {
        assert_eq!(md5("a"), "0cc175b9c0f1b6a831c399e269772661");
    }
}
