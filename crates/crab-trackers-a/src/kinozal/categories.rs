//! Kinozal browse section ids → types and title-parse strategy.

use indexmap::IndexMap;
use once_cell::sync::Lazy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KinozalTitleKind {
    Movie,
    SerialRu,
    SerialEn,
    TvShow,
}

#[derive(Clone, Debug)]
pub struct KinozalCategory {
    pub types: &'static [&'static str],
    pub title_kind: KinozalTitleKind,
}

pub static MAP: Lazy<IndexMap<&'static str, KinozalCategory>> = Lazy::new(|| {
    use KinozalTitleKind::*;
    let c = |types: &'static [&'static str], title_kind| KinozalCategory { types, title_kind };
    let mut m = IndexMap::new();
    // Сериалы
    m.insert("45", c(&["serial"], SerialRu));
    m.insert("46", c(&["serial"], SerialEn));
    // Фильмы
    for id in ["8", "6", "15", "17", "35", "39", "13", "14", "24", "11", "9", "47", "12", "10", "7", "16"] {
        m.insert(id, c(&["movie"], Movie));
    }
    // Документальный (films + multi-season docs)
    m.insert("18", c(&["docuserial", "documovie"], Movie));
    // Спорт (broadcasts; title shape is movie-like, type must be sport)
    m.insert("37", c(&["sport"], Movie));
    // ТВ-шоу
    m.insert("49", c(&["tvshow"], TvShow));
    m.insert("50", c(&["tvshow"], TvShow));
    // Мульты
    m.insert("21", c(&["multfilm", "multserial"], SerialEn));
    m.insert("22", c(&["multfilm", "multserial"], SerialRu));
    // Аниме
    m.insert("20", c(&["anime"], SerialEn));
    m
});

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.keys().copied()
}
