//! anibelka.com forum sections under «Скачать аниме».

pub struct AnibelkaCategory {
    pub id: &'static str,
    pub name: &'static str,
    pub types: &'static [&'static str],
}

const ANIME: &[&str] = &["anime"];

/// Sections in crawl order.
pub const MAP: &[AnibelkaCategory] = &[
    AnibelkaCategory { id: "32", name: "Универсальные", types: ANIME },
    AnibelkaCategory { id: "33", name: "С озвучкой", types: ANIME },
    AnibelkaCategory { id: "34", name: "С субтитрами", types: ANIME },
    AnibelkaCategory { id: "36", name: "Полнометражки", types: ANIME },
    AnibelkaCategory { id: "37", name: "PSP", types: ANIME },
];

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.iter().map(|c| c.id)
}

pub fn get(id: &str) -> Option<&'static AnibelkaCategory> {
    MAP.iter().find(|c| c.id == id)
}
