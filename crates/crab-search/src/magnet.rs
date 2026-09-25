//! Magnet link parsing and the text encoders used by the search output formats.

/// Parsed magnet link (strict: errors on anything malformed).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MagnetLink {
    pub v1: Option<String>,
    pub v2: Option<String>,
    pub name: Option<String>,
    pub announce_urls: Vec<String>,
}

impl MagnetLink {
    /// Lowercase hex of the v1 infohash, or of the v2 hash when only v2 is present.
    pub fn v1_or_v2_hex(&self) -> String {
        self.v1.clone().or_else(|| self.v2.clone()).unwrap_or_default()
    }

    /// Strict parse. Returns `None` whenever the link would be rejected as malformed.
    pub fn parse(uri: &str) -> Option<MagnetLink> {
        let s = uri.trim();
        if s.len() < 7 || !s[..7].eq_ignore_ascii_case("magnet:") {
            return None;
        }
        let rest = &s[7..];
        let qi = rest.find('?')?;
        let mut query = &rest[qi + 1..];
        if let Some(h) = query.find('#') {
            query = &query[..h];
        }

        let mut link = MagnetLink::default();
        for param in query.split('&') {
            let kv: Vec<&str> = param.split('=').collect();
            if kv.len() != 2 {
                continue;
            }
            let (key, val) = (kv[0], kv[1]);
            let prefix = key.get(0..2)?;
            match prefix {
                "xt" => {
                    let kind = val.get(0..9)?;
                    let hash = &val[9..];
                    match kind {
                        "urn:sha1:" | "urn:btih:" => {
                            if link.v1.is_some() {
                                return None;
                            }
                            link.v1 = Some(match hash.len() {
                                32 => {
                                    let bytes = base32::decode(
                                        base32::Alphabet::Rfc4648 { padding: false },
                                        &hash.to_ascii_uppercase(),
                                    )?;
                                    if bytes.len() != 20 {
                                        return None;
                                    }
                                    hex_lower(&bytes)
                                }
                                40 => {
                                    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                                        return None;
                                    }
                                    hash.to_ascii_lowercase()
                                }
                                _ => return None,
                            });
                        }
                        "urn:btmh:" => {
                            if link.v2.is_some() {
                                return None;
                            }
                            // multihash: 0x12 (sha256) 0x20 (32 bytes) + digest
                            if hash.len() != 68
                                || !hash[..4].eq_ignore_ascii_case("1220")
                                || !hash.chars().all(|c| c.is_ascii_hexdigit())
                            {
                                return None;
                            }
                            link.v2 = Some(hash[4..].to_ascii_lowercase());
                        }
                        _ => {}
                    }
                }
                "tr" => link.announce_urls.push(url_decode_plus(val)),
                "dn" => link.name = Some(url_decode_plus(val)),
                "xl" => {
                    val.parse::<i64>().ok()?;
                }
                _ => {}
            }
        }

        if link.v1.is_none() && link.v2.is_none() {
            return None;
        }
        Some(link)
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Percent-decoding with `+` as space; invalid escapes are kept literally.
pub fn url_decode_plus(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|x| u8::from_str_radix(x, 16).ok());
                match h {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Form-style URL encoding: UTF-8, space → `+`, lowercase hex escapes,
/// unreserved set `A-Za-z0-9-_.!*()`.
pub fn url_encode_form(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'*' | b'(' | b')' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02x}")),
        }
    }
    out
}

/// HTML/XML text encoding: `< > " ' &` plus Latin-1 supplement and
/// astral-plane characters as numeric references.
pub fn html_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '&' => out.push_str("&amp;"),
            c if (c as u32) >= 160 && (c as u32) < 256 => out.push_str(&format!("&#{};", c as u32)),
            c if (c as u32) > 0xFFFF => out.push_str(&format!("&#{};", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_and_base32() {
        let m = MagnetLink::parse("magnet:?xt=urn:btih:0123456789ABCDEF0123456789abcdef01234567&dn=Some+Name&tr=udp%3A%2F%2Ft.example%3A80").unwrap();
        assert_eq!(m.v1_or_v2_hex(), "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(m.name.as_deref(), Some("Some Name"));
        assert_eq!(m.announce_urls, vec!["udp://t.example:80"]);

        let b32 = MagnetLink::parse("magnet:?xt=urn:btih:AERUKZ4JVPG66AJDIVSYATLHTXX6AERU").unwrap();
        assert_eq!(b32.v1_or_v2_hex().len(), 40);
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(MagnetLink::parse("").is_none());
        assert!(MagnetLink::parse("http://x").is_none());
        assert!(MagnetLink::parse("magnet:?dn=x").is_none());
        assert!(MagnetLink::parse("magnet:?xt=urn:btih:zz").is_none());
        assert!(MagnetLink::parse("magnet:?xt=abc").is_none());
    }

    #[test]
    fn encoders() {
        assert_eq!(url_encode_form("udp://a b/"), "udp%3a%2f%2fa+b%2f");
        assert_eq!(html_encode("a&b<\"'>"), "a&amp;b&lt;&quot;&#39;&gt;");
        assert_eq!(html_encode("é Ж"), "&#233; Ж");
    }
}
