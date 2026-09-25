//! anifilm.pro category slugs, types and page limits.

pub struct AnifilmCategory {
    pub slug: &'static str,
    pub types: &'static [&'static str],
    pub full_max: i32,
    pub quick_max: i32,
}

/// Categories in crawl order.
pub const MAP: &[AnifilmCategory] = &[
    AnifilmCategory { slug: "serials", types: &["anime"], full_max: 70, quick_max: 2 },
    AnifilmCategory { slug: "ova", types: &["anime"], full_max: 32, quick_max: 2 },
    AnifilmCategory { slug: "ona", types: &["anime"], full_max: 2, quick_max: 2 },
    AnifilmCategory { slug: "movies", types: &["anime"], full_max: 17, quick_max: 2 },
    AnifilmCategory { slug: "dorams", types: &["serial"], full_max: 10, quick_max: 2 },
    AnifilmCategory { slug: "special", types: &["anime"], full_max: 5, quick_max: 2 },
    AnifilmCategory { slug: "hentai", types: &["anime"], full_max: 5, quick_max: 2 },
    AnifilmCategory { slug: "short-serials", types: &["anime"], full_max: 5, quick_max: 2 },
];

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.iter().map(|c| c.slug)
}

pub fn get(slug: &str) -> Option<&'static AnifilmCategory> {
    MAP.iter().find(|c| c.slug == slug)
}

pub fn max_pages(cat: &AnifilmCategory, fullparse: bool) -> i32 {
    if fullparse {
        cat.full_max
    } else {
        cat.quick_max
    }
}
