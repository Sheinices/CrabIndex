//! Rutor browse ids → types and title-parse strategy.

use indexmap::IndexMap;
use once_cell::sync::Lazy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RutorTitleKind {
    ForeignMovie,
    RuMovie,
    ForeignSerial,
    RuSerial,
    ShowLike,
}

#[derive(Clone, Debug)]
pub struct RutorCategory {
    pub types: &'static [&'static str],
    pub title_kind: RutorTitleKind,
    /// Cat 17: skip rows without " UKR" in the title.
    pub require_ukr_in_title: bool,
}

const fn c(types: &'static [&'static str], title_kind: RutorTitleKind) -> RutorCategory {
    RutorCategory { types, title_kind, require_ukr_in_title: false }
}

/// Browse id → category (insertion order is the crawl order).
pub static MAP: Lazy<IndexMap<&'static str, RutorCategory>> = Lazy::new(|| {
    use RutorTitleKind::*;
    let mut m = IndexMap::new();
    // 1  - Зарубежные фильмы
    m.insert("1", c(&["movie"], ForeignMovie));
    // 5  - Наши фильмы
    m.insert("5", c(&["movie"], RuMovie));
    // 4  - Зарубежные сериалы
    m.insert("4", c(&["serial"], ForeignSerial));
    // 16 - Наши сериалы
    m.insert("16", c(&["serial"], RuSerial));
    // 12 - Научно-популярные фильмы
    m.insert("12", c(&["docuserial", "documovie"], ShowLike));
    // 6  - Телевизор
    m.insert("6", c(&["tvshow"], ShowLike));
    // 7  - Мультипликация
    m.insert("7", c(&["multfilm", "multserial"], ShowLike));
    // 10 - Аниме
    m.insert("10", c(&["anime"], ShowLike));
    // 17 - Иностранные релизы (UKR filter)
    m.insert("17", RutorCategory { types: &["movie"], title_kind: ForeignMovie, require_ukr_in_title: true });
    // 13 - Спорт и Здоровье
    m.insert("13", c(&["sport"], ShowLike));
    // 15 - Юмор
    m.insert("15", c(&["tvshow"], ShowLike));
    m
});

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.keys().copied()
}
