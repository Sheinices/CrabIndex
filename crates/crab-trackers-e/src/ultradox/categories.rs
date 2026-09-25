//! ultradox.vip section paths and their content types (single source of truth).

/// Section path → types, in crawl order.
pub const MAP: [(&str, &[&str]); 6] = [
    ("serial-hd", &["serial"]),
    ("hd", &["movie"]),
    ("rufilm", &["movie"]),
    ("camrip", &["movie"]),
    ("webrips", &["movie"]),
    ("anime", &["anime"]),
];

/// Types for a section path.
pub fn types(section: &str) -> Option<&'static [&'static str]> {
    MAP.iter().find(|(k, _)| *k == section).map(|(_, v)| *v)
}

pub fn contains(section: &str) -> bool {
    types(section).is_some()
}

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.iter().map(|(k, _)| *k)
}
