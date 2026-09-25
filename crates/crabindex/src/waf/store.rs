//! Dynamic WAF lists (IP and domain blacklist / whitelist, bans) and their `Data/waf.json` file.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::net::IpAddr;

use super::domains;
use super::net::{unmap, IpNet};

/// ISO-8601 UTC with second precision (`2026-01-02T03:04:05Z`).
pub fn iso(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Drop sub-second precision (what the file keeps).
pub fn whole_secs(t: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp(t.timestamp(), 0).unwrap_or(t)
}

mod iso_serde {
    use super::*;

    pub fn serialize<S: Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&iso(t))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let s = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(s.trim()).map(|t| t.with_timezone(&Utc)).map_err(serde::de::Error::custom)
    }

    pub mod opt {
        use super::*;

        pub fn serialize<S: Serializer>(t: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
            match t {
                Some(t) => s.serialize_str(&iso(t)),
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
            let s = Option::<String>::deserialize(d)?;
            match s.as_deref().map(str::trim) {
                None | Some("") => Ok(None),
                Some(s) => DateTime::parse_from_rfc3339(s).map(|t| Some(t.with_timezone(&Utc))).map_err(serde::de::Error::custom),
            }
        }
    }
}

/// Blacklist / whitelist entry (`value` is an address or CIDR, or a domain, in canonical form).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListEntry {
    pub value: String,
    #[serde(default)]
    pub comment: String,
    #[serde(with = "iso_serde")]
    pub created: DateTime<Utc>,
    #[serde(default, with = "iso_serde::opt")]
    pub expires: Option<DateTime<Utc>>,
}

/// Temporary ban of one address.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BanEntry {
    pub ip: String,
    pub reason: String,
    #[serde(with = "iso_serde")]
    pub created: DateTime<Utc>,
    #[serde(with = "iso_serde")]
    pub expires: DateTime<Utc>,
}

/// On-disk document (every list is optional: older files without the domain lists load).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WafFile {
    pub blacklist: Vec<ListEntry>,
    pub whitelist: Vec<ListEntry>,
    pub bans: Vec<BanEntry>,
    #[serde(rename = "domainBlacklist")]
    pub domain_blacklist: Vec<ListEntry>,
    #[serde(rename = "domainWhitelist")]
    pub domain_whitelist: Vec<ListEntry>,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub entry: ListEntry,
    pub net: IpNet,
}

impl Rule {
    fn active(&self, now: DateTime<Utc>) -> bool {
        self.entry.expires.map(|e| e > now).unwrap_or(true)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Blacklist,
    Whitelist,
}

impl ListKind {
    pub fn parse(s: &str) -> Option<ListKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "blacklist" => Some(ListKind::Blacklist),
            "whitelist" => Some(ListKind::Whitelist),
            _ => None,
        }
    }
}

/// Admin domain lists (`waf/rules` `list` values `domainBlacklist` / `domainWhitelist`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainKind {
    Blacklist,
    Whitelist,
}

impl DomainKind {
    pub fn parse(s: &str) -> Option<DomainKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "domainblacklist" => Some(DomainKind::Blacklist),
            "domainwhitelist" => Some(DomainKind::Whitelist),
            _ => None,
        }
    }
}

fn entry_active(e: &ListEntry, now: DateTime<Utc>) -> bool {
    e.expires.map(|x| x > now).unwrap_or(true)
}

/// In-memory lists (parsed networks, bans keyed by address, canonical domain entries).
#[derive(Debug, Default)]
pub struct Lists {
    pub blacklist: Vec<Rule>,
    pub whitelist: Vec<Rule>,
    pub bans: HashMap<IpAddr, BanEntry>,
    pub domain_blacklist: Vec<ListEntry>,
    pub domain_whitelist: Vec<ListEntry>,
}

/// Canonical, active, deduplicated domain entries. Builtin blocked domains are never kept in
/// the whitelist (they cannot be allowed from the file either) and are redundant in the blacklist.
fn domains_from(entries: Vec<ListEntry>, now: DateTime<Utc>) -> Vec<ListEntry> {
    let mut out: Vec<ListEntry> = Vec::new();
    for mut e in entries {
        let Ok(d) = domains::parse_rule(&e.value) else { continue };
        if domains::builtin_blocked(&d).is_some() || !entry_active(&e, now) || out.iter().any(|x| x.value == d) {
            continue;
        }
        e.value = d;
        out.push(e);
    }
    out
}

fn rules_from(entries: Vec<ListEntry>, now: DateTime<Utc>) -> Vec<Rule> {
    let mut out: Vec<Rule> = Vec::new();
    for mut e in entries {
        let Some(net) = IpNet::parse(&e.value) else { continue };
        e.value = net.to_string();
        let r = Rule { entry: e, net };
        if r.active(now) && !out.iter().any(|x| x.net == net) {
            out.push(r);
        }
    }
    out
}

impl Lists {
    /// Build from the file document, dropping invalid and expired entries.
    pub fn from_file(f: WafFile, now: DateTime<Utc>) -> Lists {
        let mut bans = HashMap::new();
        for mut b in f.bans {
            let Ok(ip) = b.ip.trim().parse::<IpAddr>().map(unmap) else { continue };
            if b.expires > now {
                b.ip = ip.to_string();
                bans.insert(ip, b);
            }
        }
        Lists {
            blacklist: rules_from(f.blacklist, now),
            whitelist: rules_from(f.whitelist, now),
            bans,
            domain_blacklist: domains_from(f.domain_blacklist, now),
            domain_whitelist: domains_from(f.domain_whitelist, now),
        }
    }

    pub fn to_file(&self) -> WafFile {
        let mut bans: Vec<BanEntry> = self.bans.values().cloned().collect();
        bans.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.ip.cmp(&b.ip)));
        WafFile {
            blacklist: self.blacklist.iter().map(|r| r.entry.clone()).collect(),
            whitelist: self.whitelist.iter().map(|r| r.entry.clone()).collect(),
            bans,
            domain_blacklist: self.domain_blacklist.clone(),
            domain_whitelist: self.domain_whitelist.clone(),
        }
    }

    pub fn list(&self, kind: ListKind) -> &Vec<Rule> {
        match kind {
            ListKind::Blacklist => &self.blacklist,
            ListKind::Whitelist => &self.whitelist,
        }
    }

    pub fn list_mut(&mut self, kind: ListKind) -> &mut Vec<Rule> {
        match kind {
            ListKind::Blacklist => &mut self.blacklist,
            ListKind::Whitelist => &mut self.whitelist,
        }
    }

    pub fn in_list(&self, kind: ListKind, ip: IpAddr, now: DateTime<Utc>) -> bool {
        self.list(kind).iter().any(|r| r.active(now) && r.net.contains(ip))
    }

    pub fn domains(&self, kind: DomainKind) -> &Vec<ListEntry> {
        match kind {
            DomainKind::Blacklist => &self.domain_blacklist,
            DomainKind::Whitelist => &self.domain_whitelist,
        }
    }

    fn domains_mut(&mut self, kind: DomainKind) -> &mut Vec<ListEntry> {
        match kind {
            DomainKind::Blacklist => &mut self.domain_blacklist,
            DomainKind::Whitelist => &mut self.domain_whitelist,
        }
    }

    /// `host` (normalised) is covered by an active entry of the admin domain list.
    pub fn domain_in(&self, kind: DomainKind, host: &str, now: DateTime<Utc>) -> bool {
        self.domains(kind).iter().any(|e| entry_active(e, now) && domains::domain_matches(host, &e.value))
    }

    /// Add or replace (same domain) a canonical domain entry.
    pub fn upsert_domain(&mut self, kind: DomainKind, domain: String, comment: String, expires: Option<DateTime<Utc>>, now: DateTime<Utc>) {
        let entry = ListEntry { value: domain, comment, created: whole_secs(now), expires: expires.map(whole_secs) };
        let list = self.domains_mut(kind);
        match list.iter_mut().find(|e| e.value == entry.value) {
            Some(e) => *e = entry,
            None => list.push(entry),
        }
    }

    pub fn remove_domain(&mut self, kind: DomainKind, domain: &str) -> bool {
        let list = self.domains_mut(kind);
        let before = list.len();
        list.retain(|e| e.value != domain);
        list.len() != before
    }

    /// Active ban of `ip` (expiry), if any.
    pub fn ban_of(&self, ip: IpAddr, now: DateTime<Utc>) -> Option<&BanEntry> {
        self.bans.get(&unmap(ip)).filter(|b| b.expires > now)
    }

    /// Add or replace (same network) an entry.
    pub fn upsert(&mut self, kind: ListKind, net: IpNet, comment: String, expires: Option<DateTime<Utc>>, now: DateTime<Utc>) {
        let entry = ListEntry { value: net.to_string(), comment, created: whole_secs(now), expires: expires.map(whole_secs) };
        let list = self.list_mut(kind);
        match list.iter_mut().find(|r| r.net == net) {
            Some(r) => r.entry = entry,
            None => list.push(Rule { entry, net }),
        }
    }

    pub fn remove(&mut self, kind: ListKind, net: IpNet) -> bool {
        let list = self.list_mut(kind);
        let before = list.len();
        list.retain(|r| r.net != net);
        list.len() != before
    }

    /// Ban `ip` until `expires` (an existing longer ban is kept). Returns true when changed.
    pub fn ban(&mut self, ip: IpAddr, reason: &str, expires: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        let ip = unmap(ip);
        let (expires, now) = (whole_secs(expires), whole_secs(now));
        if let Some(b) = self.bans.get(&ip) {
            if b.expires >= expires {
                return false;
            }
        }
        self.bans.insert(ip, BanEntry { ip: ip.to_string(), reason: reason.to_string(), created: now, expires });
        true
    }

    /// Replace a ban unconditionally (manual bans may shorten an existing one).
    pub fn set_ban(&mut self, ip: IpAddr, reason: &str, expires: DateTime<Utc>, now: DateTime<Utc>) {
        let ip = unmap(ip);
        let (expires, now) = (whole_secs(expires), whole_secs(now));
        self.bans.insert(ip, BanEntry { ip: ip.to_string(), reason: reason.to_string(), created: now, expires });
    }

    pub fn unban(&mut self, ip: IpAddr) -> bool {
        self.bans.remove(&unmap(ip)).is_some()
    }

    /// Any entry or ban past its expiry?
    pub fn has_expired(&self, now: DateTime<Utc>) -> bool {
        self.blacklist.iter().chain(self.whitelist.iter()).any(|r| !r.active(now))
            || self.bans.values().any(|b| b.expires <= now)
            || self.domain_blacklist.iter().chain(self.domain_whitelist.iter()).any(|e| !entry_active(e, now))
    }

    /// Drop expired entries and bans; true when anything was removed.
    pub fn prune(&mut self, now: DateTime<Utc>) -> bool {
        let count = |l: &Lists| l.blacklist.len() + l.whitelist.len() + l.bans.len() + l.domain_blacklist.len() + l.domain_whitelist.len();
        let before = count(self);
        self.blacklist.retain(|r| r.active(now));
        self.whitelist.retain(|r| r.active(now));
        self.bans.retain(|_, b| b.expires > now);
        self.domain_blacklist.retain(|e| entry_active(e, now));
        self.domain_whitelist.retain(|e| entry_active(e, now));
        before != count(self)
    }
}

/// Read the lists file; a missing file is an empty document.
pub fn load(path: &std::path::Path) -> Result<WafFile, String> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(WafFile::default()),
        Ok(s) => serde_json::from_str(s.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(WafFile::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn save(path: &std::path::Path, f: &WafFile) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(f).map_err(std::io::Error::other)?;
    crab_core::config::write_atomically(&path.to_string_lossy(), &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn file_round_trip_drops_invalid_and_expired() {
        let now = Utc::now();
        let path = std::env::temp_dir().join(format!("crab-waf-store-{}.json", std::process::id()));
        let mut l = Lists::default();
        l.upsert(ListKind::Blacklist, IpNet::parse("203.0.113.7").unwrap(), "bot".into(), None, now);
        l.upsert(ListKind::Whitelist, IpNet::parse("198.51.100.9/24").unwrap(), "office".into(), Some(now + Duration::hours(1)), now);
        l.ban(ip("192.0.2.10"), "rate", now + Duration::minutes(15), now);
        save(&path, &l.to_file()).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["blacklist"][0]["value"], "203.0.113.7");
        assert!(v["blacklist"][0]["expires"].is_null());
        assert_eq!(v["whitelist"][0]["value"], "198.51.100.0/24");
        assert_eq!(v["bans"][0]["reason"], "rate");
        assert!(v["bans"][0]["expires"].as_str().unwrap().ends_with('Z'));

        let back = Lists::from_file(load(&path).unwrap(), now);
        assert!(back.in_list(ListKind::Blacklist, ip("203.0.113.7"), now));
        assert!(back.in_list(ListKind::Whitelist, ip("198.51.100.200"), now));
        assert!(back.ban_of(ip("192.0.2.10"), now).is_some());
        assert_eq!(back.to_file().blacklist[0].comment, "bot");

        // expired / invalid entries vanish on load
        let later = now + Duration::hours(2);
        let back = Lists::from_file(load(&path).unwrap(), later);
        assert!(back.whitelist.is_empty());
        assert!(back.bans.is_empty());
        assert_eq!(back.blacklist.len(), 1);
        std::fs::write(&path, r#"{"blacklist":[{"value":"nope","created":"2024-01-01T00:00:00Z"}],"bans":[{"ip":"x","reason":"rate","created":"2024-01-01T00:00:00Z","expires":"2999-01-01T00:00:00Z"}]}"#).unwrap();
        let back = Lists::from_file(load(&path).unwrap(), now);
        assert!(back.blacklist.is_empty() && back.bans.is_empty());
        let _ = std::fs::remove_file(&path);
        assert!(load(&path).unwrap().blacklist.is_empty());
    }

    #[test]
    fn domain_lists_round_trip_and_old_files_load() {
        let now = Utc::now();
        let path = std::env::temp_dir().join(format!("crab-waf-store-domains-{}.json", std::process::id()));
        // a file written before the domain lists existed
        std::fs::write(&path, r#"{"blacklist":[{"value":"203.0.113.7","created":"2024-01-01T00:00:00Z"}],"whitelist":[],"bans":[]}"#).unwrap();
        let f = load(&path).unwrap();
        assert!(f.domain_blacklist.is_empty() && f.domain_whitelist.is_empty());
        let mut l = Lists::from_file(f, now);
        assert_eq!(l.blacklist.len(), 1);

        l.upsert_domain(DomainKind::Blacklist, "evil.example".into(), "spam".into(), None, now);
        l.upsert_domain(DomainKind::Whitelist, "friend.example".into(), String::new(), Some(now + Duration::hours(1)), now);
        save(&path, &l.to_file()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["domainBlacklist"][0]["value"], "evil.example");
        assert_eq!(v["domainBlacklist"][0]["comment"], "spam");
        assert!(v["domainWhitelist"][0]["expires"].is_string());

        let back = Lists::from_file(load(&path).unwrap(), now);
        assert!(back.domain_in(DomainKind::Blacklist, "a.evil.example", now));
        assert!(!back.domain_in(DomainKind::Blacklist, "notevil.example", now));
        assert!(back.domain_in(DomainKind::Whitelist, "friend.example", now));
        assert!(!back.domain_in(DomainKind::Whitelist, "friend.example", now + Duration::hours(2)));

        // hand-edited file: invalid, duplicate and builtin entries are dropped, pasted URLs canonicalised
        std::fs::write(
            &path,
            r#"{"domainWhitelist":[{"value":"ndst.pw","created":"2024-01-01T00:00:00Z"},{"value":"x.myds.me","created":"2024-01-01T00:00:00Z"},{"value":"https://OK.example/x","created":"2024-01-01T00:00:00Z"},{"value":"ok.example","created":"2024-01-01T00:00:00Z"}],
               "domainBlacklist":[{"value":"no_dot","created":"2024-01-01T00:00:00Z"},{"value":"*.bad.example","created":"2024-01-01T00:00:00Z"}]}"#,
        )
        .unwrap();
        let back = Lists::from_file(load(&path).unwrap(), now);
        assert_eq!(back.domain_whitelist.iter().map(|e| e.value.as_str()).collect::<Vec<_>>(), ["ok.example"]);
        assert_eq!(back.domain_blacklist.iter().map(|e| e.value.as_str()).collect::<Vec<_>>(), ["bad.example"]);
        assert!(back.blacklist.is_empty());

        let mut back = back;
        assert!(back.remove_domain(DomainKind::Whitelist, "ok.example"));
        assert!(!back.remove_domain(DomainKind::Whitelist, "ok.example"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ban_keeps_longer_and_prunes() {
        let now = Utc::now();
        let mut l = Lists::default();
        assert!(l.ban(ip("::ffff:192.0.2.1"), "trap", now + Duration::minutes(60), now));
        assert!(!l.ban(ip("192.0.2.1"), "rate", now + Duration::minutes(5), now));
        assert_eq!(l.ban_of(ip("192.0.2.1"), now).unwrap().reason, "trap");
        assert!(l.ban_of(ip("192.0.2.1"), now + Duration::minutes(61)).is_none());
        assert!(!l.prune(now));
        assert!(l.prune(now + Duration::minutes(61)));
        assert!(l.bans.is_empty());
    }
}
