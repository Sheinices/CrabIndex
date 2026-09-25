//! Minimal bencode decoder for .torrent files: magnet, infohash, total size.

use sha1::{Digest, Sha1};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum Bval {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Bval>),
    Dict(BTreeMap<Vec<u8>, Bval>),
}

impl Bval {
    pub fn get(&self, key: &str) -> Option<&Bval> {
        match self {
            Bval::Dict(m) => m.get(key.as_bytes()),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<String> {
        match self {
            Bval::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Bval::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&Vec<Bval>> {
        match self {
            Bval::List(l) => Some(l),
            _ => None,
        }
    }
}

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
    info_span: Option<(usize, usize)>,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn parse(&mut self) -> Option<Bval> {
        let c = *self.data.get(self.pos)?;
        match c {
            b'i' => {
                self.pos += 1;
                let end = self.data[self.pos..].iter().position(|&b| b == b'e')? + self.pos;
                let s = std::str::from_utf8(&self.data[self.pos..end]).ok()?;
                self.pos = end + 1;
                Some(Bval::Int(s.parse().ok()?))
            }
            b'l' => {
                self.pos += 1;
                self.depth += 1;
                let mut v = Vec::new();
                while *self.data.get(self.pos)? != b'e' {
                    v.push(self.parse()?);
                }
                self.pos += 1;
                self.depth -= 1;
                Some(Bval::List(v))
            }
            b'd' => {
                self.pos += 1;
                self.depth += 1;
                let mut m = BTreeMap::new();
                while *self.data.get(self.pos)? != b'e' {
                    let k = match self.parse()? {
                        Bval::Bytes(b) => b,
                        _ => return None,
                    };
                    let start = self.pos;
                    let v = self.parse()?;
                    if self.depth == 1 && k == b"info" {
                        self.info_span = Some((start, self.pos));
                    }
                    m.insert(k, v);
                }
                self.pos += 1;
                self.depth -= 1;
                Some(Bval::Dict(m))
            }
            b'0'..=b'9' => {
                let colon = self.data[self.pos..].iter().position(|&b| b == b':')? + self.pos;
                let len: usize = std::str::from_utf8(&self.data[self.pos..colon]).ok()?.parse().ok()?;
                let start = colon + 1;
                let end = start.checked_add(len)?;
                if end > self.data.len() {
                    return None;
                }
                self.pos = end;
                Some(Bval::Bytes(self.data[start..end].to_vec()))
            }
            _ => None,
        }
    }
}

/// Parsed .torrent essentials.
#[derive(Debug, Clone)]
pub struct TorrentMeta {
    /// Lowercase hex SHA-1 of the raw `info` dictionary.
    pub info_hash: String,
    pub name: Option<String>,
    pub total_size: i64,
    pub trackers: Vec<String>,
}

pub fn decode(data: &[u8]) -> Option<Bval> {
    Parser { data, pos: 0, info_span: None, depth: 0 }.parse()
}

pub fn parse_torrent(data: &[u8]) -> Option<TorrentMeta> {
    let mut p = Parser { data, pos: 0, info_span: None, depth: 0 };
    let root = p.parse()?;
    let (s, e) = p.info_span?;
    let mut h = Sha1::new();
    h.update(&data[s..e]);
    let info_hash = hex::encode(h.finalize());
    let info = root.get("info")?;
    let name = info.get("name.utf-8").or_else(|| info.get("name")).and_then(|v| v.as_str());
    let total_size = if let Some(files) = info.get("files").and_then(|f| f.as_list()) {
        files.iter().filter_map(|f| f.get("length").and_then(|l| l.as_int())).sum()
    } else {
        info.get("length").and_then(|l| l.as_int()).unwrap_or(0)
    };
    let mut trackers = Vec::new();
    if let Some(a) = root.get("announce").and_then(|a| a.as_str()) {
        trackers.push(a);
    }
    if let Some(tiers) = root.get("announce-list").and_then(|l| l.as_list()) {
        for tier in tiers {
            if let Some(list) = tier.as_list() {
                for t in list {
                    if let Some(s) = t.as_str() {
                        if !trackers.contains(&s) {
                            trackers.push(s);
                        }
                    }
                }
            }
        }
    }
    Some(TorrentMeta { info_hash, name, total_size, trackers })
}

/// Magnet with lowercase btih, display name and trackers.
pub fn magnet(torrent: &[u8]) -> Option<String> {
    let m = parse_torrent(torrent)?;
    let mut s = format!("magnet:?xt=urn:btih:{}", m.info_hash);
    if let Some(n) = &m.name {
        s.push_str("&dn=");
        s.push_str(&urlencoding::encode(n));
    }
    for t in &m.trackers {
        s.push_str("&tr=");
        s.push_str(&urlencoding::encode(t));
    }
    Some(s)
}

/// Magnet with infohash (+ dn) only - for passkey-bearing torrents.
pub fn magnet_no_trackers(torrent: &[u8]) -> Option<String> {
    let m = parse_torrent(torrent)?;
    let mut s = format!("magnet:?xt=urn:btih:{}", m.info_hash);
    if let Some(n) = m.name.as_deref().filter(|n| !n.trim().is_empty()) {
        s.push_str("&dn=");
        s.push_str(&urlencoding::encode(n));
    }
    Some(s)
}

/// Total size as "1.46 GB".
pub fn size_name(torrent: &[u8]) -> Option<String> {
    let m = parse_torrent(torrent)?;
    Some(crate::util::format_bytes(m.total_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_torrent() {
        let t = b"d8:announce13:http://tr/ann4:infod6:lengthi2048e4:name5:hello12:piece lengthi16384e6:pieces0:ee";
        let m = parse_torrent(t).unwrap();
        assert_eq!(m.total_size, 2048);
        assert_eq!(m.name.as_deref(), Some("hello"));
        let mg = magnet(t).unwrap();
        assert!(mg.starts_with("magnet:?xt=urn:btih:"));
        assert!(mg.contains("&dn=hello&tr=http%3A%2F%2Ftr%2Fann"));
        assert_eq!(size_name(t).unwrap(), "2.00 KB");
    }
}
