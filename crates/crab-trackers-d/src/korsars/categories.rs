//! korsars.pro forum ids and types (hand-picked movie/serial/cartoon sections).

pub const MOVIE_IDS: &[&str] = &["282", "31", "33", "125", "146", "270"];

pub const SERIAL_IDS: &[&str] = &["287", "286", "267", "303", "288", "39", "40", "300", "41", "121", "144", "271"];

pub const CARTOON_IDS: &[&str] = &["43", "44", "277", "46", "272", "273"];

const MOVIE: &[&str] = &["movie"];
const SERIAL: &[&str] = &["serial"];
/// Cartoon forums mix films and series - emit both types.
const CARTOON: &[&str] = &["multfilm", "multserial"];

/// All forum ids in crawl order (movies, serials, cartoons).
pub fn ids() -> impl Iterator<Item = &'static str> {
    MOVIE_IDS.iter().chain(SERIAL_IDS.iter()).chain(CARTOON_IDS.iter()).copied()
}

/// Number of distinct forum ids.
pub fn count() -> usize {
    let mut v: Vec<&str> = ids().collect();
    v.sort();
    v.dedup();
    v.len()
}

/// Types of a mapped forum, if any.
pub fn get(cat: &str) -> Option<&'static [&'static str]> {
    // Later groups win on duplicate ids.
    if CARTOON_IDS.contains(&cat) {
        Some(CARTOON)
    } else if SERIAL_IDS.contains(&cat) {
        Some(SERIAL)
    } else if MOVIE_IDS.contains(&cat) {
        Some(MOVIE)
    } else {
        None
    }
}

/// Types for a forum id (`["movie"]` for unknown/blank ids).
pub fn types_for(cat: &str) -> &'static [&'static str] {
    if cat.trim().is_empty() {
        return MOVIE;
    }
    get(cat).unwrap_or(MOVIE)
}
