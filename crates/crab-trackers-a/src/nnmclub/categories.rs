//! NNMClub portal ids (`/forum/portal.php?c={id}`) → types and title-parse strategy.

use indexmap::IndexMap;
use once_cell::sync::Lazy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NNMClubTitleKind {
    ForeignCinema,
    ForeignSerial,
    RuMovie,
    RuSerial,
    Anime,
    KidsMult,
    ShowLike,
    Sport,
}

#[derive(Clone, Debug)]
pub struct NNMClubCategory {
    pub types: &'static [&'static str],
    pub title_kind: NNMClubTitleKind,
    /// Cat 7: only parse rows that look like cartoons (мульт / duration).
    pub require_mult_in_row: bool,
    /// Cat 7: skip PDF book releases.
    pub skip_pdf_in_title: bool,
}

pub static MAP: Lazy<IndexMap<&'static str, NNMClubCategory>> = Lazy::new(|| {
    use NNMClubTitleKind::*;
    let c = |types: &'static [&'static str], title_kind| NNMClubCategory { types, title_kind, require_mult_in_row: false, skip_pdf_in_title: false };
    let mut m = IndexMap::new();
    // 10 - Новинки кино
    m.insert("10", c(&["movie"], ForeignCinema));
    // 13 - Наше кино
    m.insert("13", c(&["movie"], RuMovie));
    // 6  - Зарубежное кино
    m.insert("6", c(&["movie"], ForeignCinema));
    // 11 - HD, UHD и 3D Кино
    m.insert("11", c(&["movie"], ForeignCinema));
    // 4  - Наши сериалы
    m.insert("4", c(&["serial"], RuSerial));
    // 3  - Зарубежные сериалы
    m.insert("3", c(&["serial"], ForeignSerial));
    // 22 - Док. TV-бренды
    m.insert("22", c(&["docuserial", "documovie"], ShowLike));
    // 23 - Док. и телепередачи
    m.insert("23", c(&["docuserial", "documovie"], ShowLike));
    // 1  - Аниме и Манга
    m.insert("1", c(&["anime"], Anime));
    // 7  - Детям и родителям
    m.insert(
        "7",
        NNMClubCategory { types: &["multfilm", "multserial"], title_kind: KidsMult, require_mult_in_row: true, skip_pdf_in_title: true },
    );
    // 24 - Спорт и активный отдых
    m.insert("24", c(&["sport"], Sport));
    // 21 - Театр, МузВидео, Разное
    m.insert("21", c(&["tvshow"], ShowLike));
    // 27 - Юмор и сатира
    m.insert("27", c(&["tvshow"], ShowLike));
    m
});

/// Portal sections that are not video (music, books, software, games, …) plus the
/// temporary WC 2026 event section 28.
pub const NON_VIDEO_IDS: [&str; 14] = ["2", "5", "8", "9", "12", "14", "15", "16", "18", "19", "20", "25", "26", "28"];

pub fn ids() -> impl Iterator<Item = &'static str> {
    MAP.keys().copied()
}
