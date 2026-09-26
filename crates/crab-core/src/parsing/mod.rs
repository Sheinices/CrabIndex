//! Parser helpers: date parsing, .torrent decoding, per-tracker logs.
pub mod bencode;
pub mod parser_log;
pub mod tparse;

pub use parser_log as plog;

use crate::rx;

/// True when a release title carries a season marker in brackets: `[S01]`, `[S01-03]`,
/// `[S01E05]`, `[01x01-08 из 08]`, `[2 сезон]`. Used by trackers whose "movie" categories also
/// list series (rutor cat 17 "Иностранные релизы") and by the matching data migration.
/// Bracketed forms only, so a film titled "Сезон охоты" is not mistaken for a series.
pub fn has_season_marker(title: &str) -> bool {
    rx::is_match_i(title, r"\[(?:s\d{1,2}|\d{1,2}x\d{1,2}|[^\]]*сезон)")
}

#[cfg(test)]
mod season_marker_tests {
    use super::has_season_marker;

    #[test]
    fn detects_bracketed_markers() {
        for t in [
            "Ведьмак / Відьмак / The Witcher [S01] (2019) WEBRip 1080p | UKR",
            "Дом Дракона / House of the Dragon [S01-03] (2022-2026) WEB-DLRip-AVC",
            "Слово пацана. Кровь на асфальте [01x01-08 из 08] (2023) WEB-DL 1080p",
            "Шоу [2 сезон] (2024) WEB-DL",
            "Serial [s02e05] (2020)",
        ] {
            assert!(has_season_marker(t), "{t}");
        }
    }

    #[test]
    fn ignores_films_and_loose_words() {
        for t in [
            "Сезон охоты / Open Season (2006) BDRip 1080p | UKR",
            "Опасное небо / Top Gunner (2020) WEB-DL 1080p | P",
            "Фильм [2160p] (2021) [HDR]",
        ] {
            assert!(!has_season_marker(t), "{t}");
        }
    }
}
