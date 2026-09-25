//! IP address and CIDR parsing / matching (IPv4 and IPv6).
//!
//! IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) are treated as the IPv4 address everywhere.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// `::ffff:a.b.c.d` → `a.b.c.d`; everything else unchanged.
pub fn unmap(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 => v4,
    }
}

/// A network (`addr/prefix`, host bits cleared); a single address has the full prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IpNet {
    addr: IpAddr,
    prefix: u8,
}

fn mask32(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix as u32)
    }
}

fn mask128(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix as u32)
    }
}

impl IpNet {
    /// Parses `1.2.3.4`, `1.2.3.0/24`, `2001:db8::1`, `2001:db8::/32`, `::ffff:1.2.3.0/120`.
    pub fn parse(s: &str) -> Option<IpNet> {
        let s = s.trim();
        let (a, p) = match s.split_once('/') {
            Some((a, p)) => (a.trim(), Some(p.trim())),
            None => (s, None),
        };
        let raw: IpAddr = a.parse().ok()?;
        let addr = unmap(raw);
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match p {
            None => max,
            Some(p) => {
                if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let mut n: u8 = p.parse().ok()?;
                if raw.is_ipv6() && addr.is_ipv4() {
                    // prefix given against the 128-bit mapped form
                    n = n.checked_sub(96)?;
                }
                if n > max {
                    return None;
                }
                n
            }
        };
        Some(IpNet::new(addr, prefix))
    }

    pub fn new(addr: IpAddr, prefix: u8) -> IpNet {
        let addr = unmap(addr);
        match addr {
            IpAddr::V4(v4) => {
                let p = prefix.min(32);
                IpNet { addr: IpAddr::V4(Ipv4Addr::from(u32::from(v4) & mask32(p))), prefix: p }
            }
            IpAddr::V6(v6) => {
                let p = prefix.min(128);
                IpNet { addr: IpAddr::V6(Ipv6Addr::from(u128::from(v6) & mask128(p))), prefix: p }
            }
        }
    }

    pub fn host(ip: IpAddr) -> IpNet {
        IpNet::new(ip, 128)
    }

    pub fn addr(&self) -> IpAddr {
        self.addr
    }

    pub fn is_host(&self) -> bool {
        self.prefix == if self.addr.is_ipv4() { 32 } else { 128 }
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, unmap(ip)) {
            (IpAddr::V4(n), IpAddr::V4(i)) => u32::from(n) == u32::from(i) & mask32(self.prefix),
            (IpAddr::V6(n), IpAddr::V6(i)) => u128::from(n) == u128::from(i) & mask128(self.prefix),
            _ => false,
        }
    }

    pub fn contains_net(&self, other: &IpNet) -> bool {
        self.prefix <= other.prefix && self.contains(other.addr)
    }

    pub fn overlaps(&self, other: &IpNet) -> bool {
        self.contains_net(other) || other.contains_net(self)
    }
}

/// Canonical text: `1.2.3.4` for a single address, `1.2.3.0/24` for a network.
impl fmt::Display for IpNet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_host() {
            write!(f, "{}", self.addr)
        } else {
            write!(f, "{}/{}", self.addr, self.prefix)
        }
    }
}

/// `127.0.0.0/8` and `::1`.
pub fn loopback_nets() -> [IpNet; 2] {
    [IpNet::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)), 8), IpNet::host(IpAddr::V6(Ipv6Addr::LOCALHOST))]
}

pub fn is_loopback(ip: IpAddr) -> bool {
    loopback_nets().iter().any(|n| n.contains(ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn parse_and_canonical_form() {
        assert_eq!(IpNet::parse("203.0.113.7").unwrap().to_string(), "203.0.113.7");
        assert_eq!(IpNet::parse(" 198.51.100.77/24 ").unwrap().to_string(), "198.51.100.0/24");
        assert_eq!(IpNet::parse("10.0.0.1/32").unwrap().to_string(), "10.0.0.1");
        assert_eq!(IpNet::parse("0.0.0.0/0").unwrap().to_string(), "0.0.0.0/0");
        assert_eq!(IpNet::parse("2001:DB8::1").unwrap().to_string(), "2001:db8::1");
        assert_eq!(IpNet::parse("2001:db8:abcd::5/48").unwrap().to_string(), "2001:db8:abcd::/48");
        assert_eq!(IpNet::parse("::ffff:192.0.2.9").unwrap().to_string(), "192.0.2.9");
        assert_eq!(IpNet::parse("::ffff:192.0.2.0/120").unwrap().to_string(), "192.0.2.0/24");
        for bad in ["", "x", "1.2.3", "1.2.3.4/33", "1.2.3.4/", "1.2.3.4/-1", "1.2.3.4/+8", "::1/129", "1.2.3.4/8/8", "[::1]"] {
            assert!(IpNet::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn cidr_matching_v4() {
        let n = IpNet::parse("198.51.100.0/24").unwrap();
        assert!(n.contains(ip("198.51.100.0")));
        assert!(n.contains(ip("198.51.100.255")));
        assert!(n.contains(ip("::ffff:198.51.100.9")));
        assert!(!n.contains(ip("198.51.101.1")));
        assert!(!n.contains(ip("2001:db8::1")));
        let n = IpNet::parse("10.0.0.0/8").unwrap();
        assert!(n.contains(ip("10.255.1.2")));
        assert!(!n.contains(ip("11.0.0.1")));
        assert!(IpNet::parse("0.0.0.0/0").unwrap().contains(ip("8.8.8.8")));
        let h = IpNet::parse("203.0.113.7").unwrap();
        assert!(h.contains(ip("203.0.113.7")) && !h.contains(ip("203.0.113.8")));
    }

    #[test]
    fn cidr_matching_v6() {
        let n = IpNet::parse("2001:db8::/32").unwrap();
        assert!(n.contains(ip("2001:db8:ffff::1")));
        assert!(!n.contains(ip("2001:db9::1")));
        assert!(!n.contains(ip("192.0.2.1")));
        let n = IpNet::parse("fe80::/10").unwrap();
        assert!(n.contains(ip("febf::1")));
        assert!(!n.contains(ip("fec0::1")));
        assert!(IpNet::parse("::/0").unwrap().contains(ip("::1")));
        assert!(!IpNet::parse("::/0").unwrap().contains(ip("1.2.3.4")));
    }

    #[test]
    fn overlaps_and_loopback() {
        let big = IpNet::parse("0.0.0.0/0").unwrap();
        let lo = IpNet::parse("127.0.0.1").unwrap();
        assert!(big.overlaps(&lo) && lo.overlaps(&big));
        assert!(!IpNet::parse("128.0.0.0/1").unwrap().overlaps(&loopback_nets()[0]));
        assert!(IpNet::parse("::/0").unwrap().overlaps(&loopback_nets()[1]));
        assert!(is_loopback(ip("127.3.4.5")) && is_loopback(ip("::1")) && is_loopback(ip("::ffff:127.0.0.1")));
        assert!(!is_loopback(ip("10.0.0.1")));
    }
}
