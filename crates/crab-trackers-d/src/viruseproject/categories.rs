//! viruseproject.tv categories, types and `?start` page steps.

pub struct ViruseprojectCategory {
    pub slug: &'static str,
    pub types: &'static [&'static str],
    /// `?start` pagination step.
    pub page_step: i32,
}

/// Categories in crawl order.
pub const MAP: &[ViruseprojectCategory] = &[
    ViruseprojectCategory { slug: "serials", types: &["serial"], page_step: 10 },
    ViruseprojectCategory { slug: "movies", types: &["movie"], page_step: 10 },
    ViruseprojectCategory { slug: "documentary", types: &["docuserial", "documovie"], page_step: 6 },
    ViruseprojectCategory { slug: "cartoons", types: &["multfilm", "multserial"], page_step: 6 },
    ViruseprojectCategory { slug: "reality-show", types: &["tvshow"], page_step: 6 },
];

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.iter().map(|c| c.slug)
}

/// Case-insensitive lookup.
pub fn get(slug: &str) -> Option<&'static ViruseprojectCategory> {
    MAP.iter().find(|c| c.slug.eq_ignore_ascii_case(slug))
}
