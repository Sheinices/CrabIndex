//! torrent.by section slugs → types and title-parse strategy.

use indexmap::IndexMap;
use once_cell::sync::Lazy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TorrentByTitleKind {
    FilmsForeign,
    FilmsRu,
    SerialForeign,
    SerialRu,
    ShowLike,
    Sport,
}

#[derive(Clone, Debug)]
pub struct TorrentByCategory {
    pub types: &'static [&'static str],
    pub title_kind: TorrentByTitleKind,
}

pub static MAP: Lazy<IndexMap<&'static str, TorrentByCategory>> = Lazy::new(|| {
    use TorrentByTitleKind::*;
    let c = |types: &'static [&'static str], title_kind| TorrentByCategory { types, title_kind };
    let mut m = IndexMap::new();
    // Зарубежные фильмы
    m.insert("films", c(&["movie"], FilmsForeign));
    // Наши фильмы
    m.insert("movies", c(&["movie"], FilmsRu));
    // Зарубежные сериалы
    m.insert("serials", c(&["serial"], SerialForeign));
    // Наши сериалы
    m.insert("series", c(&["serial"], SerialRu));
    // Телевизор / Юмор
    m.insert("tv", c(&["tvshow"], ShowLike));
    m.insert("humor", c(&["tvshow"], ShowLike));
    // Мультфильмы
    m.insert("cartoons", c(&["multfilm", "multserial"], ShowLike));
    // Аниме
    m.insert("anime", c(&["anime"], ShowLike));
    // Спорт
    m.insert("sport", c(&["sport"], Sport));
    m
});

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.keys().copied()
}
