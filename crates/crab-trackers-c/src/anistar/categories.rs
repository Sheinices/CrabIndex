//! Anistar section paths and the types assigned to their releases (iteration order is crawl order).

pub struct AnistarCategory {
    pub id: &'static str,
    pub types: &'static [&'static str],
}

pub const MAP: &[AnistarCategory] = &[
    AnistarCategory { id: "anime", types: &["anime"] },
    AnistarCategory { id: "hentai", types: &["anime"] },
    AnistarCategory { id: "dorams", types: &["serial"] },
];

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.iter().map(|c| c.id)
}
