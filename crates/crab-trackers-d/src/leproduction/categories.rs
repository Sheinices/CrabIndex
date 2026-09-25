//! le-production.online category slugs and types.

pub struct LeproductionCategory {
    pub slug: &'static str,
    pub types: &'static [&'static str],
}

/// Categories in crawl order.
pub const MAP: &[LeproductionCategory] = &[
    LeproductionCategory { slug: "anime", types: &["anime"] },
    LeproductionCategory { slug: "dorama", types: &["serial"] },
    LeproductionCategory { slug: "film", types: &["movie"] },
    LeproductionCategory { slug: "serial", types: &["serial"] },
    LeproductionCategory { slug: "fulcartoon", types: &["multfilm"] },
    LeproductionCategory { slug: "cartoon", types: &["multserial"] },
];

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.iter().map(|c| c.slug)
}

/// Case-insensitive lookup.
pub fn get(slug: &str) -> Option<&'static LeproductionCategory> {
    MAP.iter().find(|c| c.slug.eq_ignore_ascii_case(slug))
}
