// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! rutracker topic titles → `name` / `originalname` / year. Shared by the parser and the
//! FileDB migrations, so rows parsed before a pattern fix can be healed the same way.

use crate::{rx, util};

pub type Names = (Option<String>, Option<String>, i32, bool);

/// Which title pattern family a forum uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Movie,
    Serial,
    NonStandard,
}

/// `(name, originalname, relased, skip_row)` from a rutracker topic title.
pub fn parse(kind: Kind, title: &str) -> Names {
    match kind {
        Kind::Movie => parse_movie_title(title),
        Kind::Serial => parse_serial_title(title),
        Kind::NonStandard => parse_non_standard_title(title),
    }
}

/// Content types whose rutracker forums use the non-standard title family (sport, documentary,
/// TV shows): the parser stores only the name there and `originalname == name` is by design.
const NON_STANDARD_TYPES: [&str; 4] = ["sport", "docuserial", "documovie", "tvshow"];

/// Kind of a stored row when its forum is unknown: non-standard for sport / documentary / TV-show
/// rows (see [`NON_STANDARD_TYPES`]), otherwise [`guess_kind`] from the title. The FileDB
/// migrations and the data check use it so they agree with what the parser writes.
pub fn kind_for_row(types: &[String], title: &str) -> Kind {
    if types.iter().any(|t| NON_STANDARD_TYPES.contains(&t.as_str())) {
        Kind::NonStandard
    } else {
        guess_kind(title)
    }
}

/// Drop a season suffix from a name segment: `Обмани меня Сезон 1` → `Обмани меня`,
/// `Как не стоит жить, сезон 2`, `Гевин и Стейси, 3 сезон`, `... - Season 2`,
/// `Бес в ребро, 1 и 2 сезоны, весь сериал`, `Минисериал + 1 сезон`. A season word without a
/// number is part of the name and stays (`Брачный сезон`, `Open Season`).
pub fn strip_season(seg: &str) -> String {
    // `Сезон 7` / `Season 2` first, so a number that belongs to the name survives
    // (`Глубокий космос 9 Сезон 7` → `Глубокий космос 9`)
    const NAMED_FIRST: &str = r"\s*[,.\-–+:]?\s*(сезон(ы)?|seasons?)\s*:?\s*\d+(\s*-\s*\d+)?\b.*$";
    // `3 сезон`, `1 и 2 сезоны`, `2nd season`, `1-13 серии`, `15 серий`, `S23E10`
    const NUMBER_FIRST: &str = r"\s*[,.\-–+:]?\s*(\d+(\s*(-|,|и|and)\s*\d+)*\s*-?(й|я|st|nd|rd|th)?\s*(сезон(ы|а|ов)?|seasons?|серии|серий|серия|эпизод(ы|ов)?|episodes?)|S\d{1,2}E\d{1,3})\b.*$";
    let mut out = rx::replace_i(seg, NAMED_FIRST, "");
    if out.trim() == seg.trim() {
        out = rx::replace_i(seg, NUMBER_FIRST, "");
    }
    if out.trim() == seg.trim() {
        // nothing stripped: leave the segment as it is (`Концерт ... 2006 г.`)
        return seg.trim().to_string();
    }
    out.trim().trim_end_matches([',', '-', '–', '+', ':', '.']).trim().to_string()
}

/// A whole segment that only names seasons / episodes (`Сезон: 1`, `Серии: 1-10 из 10`, `9, 10 сезоны`).
fn is_marker(seg: &str) -> bool {
    rx::is_match_i(seg, r"^(Сезон(ы)?|Серии|Серия|Season(s)?|Episodes?)\b")
        || rx::is_match_i(seg, r"^[\d\s,\-]+(сезон|season)")
        || rx::is_match(seg, r"^[\d\s,\-/]+$")
}

/// Title part before the `[...]` info block with director / studio `(x)` groups removed when a
/// ` / ` segment follows them (`Найтвинг: Сериал / Ночное Крыло (мини-сериал) / Nightwing: ...`);
/// the first `(x)` not followed by ` / ` ends the head. `flat` is the [`collapse_parens`] output.
fn title_head(flat: &str) -> String {
    let end = flat.find('[').unwrap_or(flat.len());
    let mut head = flat[..end].to_string();
    if !flat.contains('[') {
        // no info block: the first `(` ends the head as before
        if let Some(i) = head.find('(') {
            head.truncate(i);
        }
        return head.trim().to_string();
    }
    while let Some(i) = head.find("(x)") {
        let rest = head[i + 3..].trim_start();
        if rest.starts_with('/') {
            head.replace_range(i..i + 3, "");
        } else {
            head.truncate(i);
            break;
        }
    }
    head.trim().to_string()
}

/// Best-effort kind for a title without its forum: serial when it carries a season/episode
/// block, movie otherwise (the non-standard family is never guessed).
pub fn guess_kind(title: &str) -> Kind {
    if rx::is_match_i(title, "(Сезон|Серии)") {
        Kind::Serial
    } else {
        Kind::Movie
    }
}

/// Generic fallback for titles the pattern lists do not match: the head before the first `(`
/// or `[` is split by ` / `; season/episode segments (`Сезон: 10`, `Сезоны 1-4`, `Серии 1-24`,
/// `9, 10 сезоны`) are dropped; the first segment is the name, the last remaining segment with
/// Latin letters is the original; the year is the first four-digit number inside `[...]`.
/// Handles `Terminator(Джеймс Кэмерон)` (no space), `[1984; США; ...]` and `[1994-1995, ...]`.
pub fn parse_generic(title: &str) -> Names {
    let flat = collapse_parens(title);
    // `КЛОН 60-67 серии/ O Clone`, `... /Сезон: 1/Серии: ...`: a slash with a space on one side
    // or next to a season marker is a segment separator too (`1/256`, `AC/DC` stay intact)
    let flat = rx::replace(&flat, r"\s+/\s*|\s*/\s+", " / ");
    let flat = rx::replace_i(&flat, r"/(Сезон|Серии|Season|Episodes)", " / $1");
    let year = rx::group(title, r"\[[^\]]*?((?:19|20)\d{2})", 1).parse().unwrap_or(0);
    let short_end = flat.find(['(', '[']).unwrap_or(flat.len());
    let short = generic_from_head(&flat[..short_end], year);
    if short.1.is_some() || !flat.contains('[') {
        return short;
    }
    let long = generic_from_head(&title_head(&flat), year);
    if long.1.is_some() {
        return long;
    }
    short
}

fn generic_from_head(head: &str, year: i32) -> Names {
    let head = head.trim();
    if head.is_empty() {
        return (None, None, 0, false);
    }
    let segs: Vec<String> = head
        .split(" / ")
        .map(str::trim)
        .filter(|s| !s.is_empty() && !is_marker(s))
        .map(strip_season)
        .filter(|s| !s.is_empty() && !is_marker(s))
        .collect();
    let Some(name) = segs.first().cloned().filter(|n| !util::is_blank(n)) else {
        return (None, None, 0, false);
    };
    let orig = segs.iter().skip(1).rev().find(|s| has_latin(s)).cloned();
    (Some(name), orig, year, false)
}

fn has_latin(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphabetic())
}

/// Replace every top-level `( ... )` group (nested parentheses included) by `(x)` so the title
/// patterns can treat the director/studio block as one token:
/// `Матрица / The Matrix (Братья Вачовски (Энди, Ларри) / The Wachowski Brothers (Andy, Larry)) [1999, ...]`.
pub fn collapse_parens(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut depth = 0usize;
    for ch in title.chars() {
        match ch {
            '(' => {
                if depth == 0 {
                    out.push_str("(x)");
                }
                depth += 1;
            }
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

fn nb(s: &str) -> bool {
    !util::is_blank(s)
}

/// Try `pattern`; when groups `n`, `o` (0 = none) and `y` are non-blank return them.
fn try_names(title: &str, pattern: &str, n: usize, o: usize, y: usize) -> Option<(String, Option<String>, i32)> {
    let g = rx::groups(title, pattern);
    let get = |i: usize| g.get(i).cloned().unwrap_or_default();
    let (name, orig, year) = (get(n), if o > 0 { get(o) } else { String::new() }, get(y));
    if nb(&name) && (o == 0 || nb(&orig)) && nb(&year) {
        Some((name, if o > 0 { Some(orig) } else { None }, year.parse().unwrap_or(0)))
    } else {
        None
    }
}

fn parse_movie_title(title: &str) -> Names {
    let title = &collapse_parens(title);
    let patterns: [(&str, usize, usize, usize); 3] = [
        // Ниже нуля / Bajocero / Below Zero (Йуис Килес / Lluís Quílez) [2021, Испания, ...]
        (r"^([^/\(\[]+) / [^/\(\[]+ / ([^/\(\[]+) \([^\)]+\) \[([0-9]+), ", 1, 2, 3),
        // Белый тигр / The White Tiger (Рамин Бахрани / Ramin Bahrani) [2021, Индия, ...]
        (r"^([^/\(\[]+) / ([^/\(\[]+) \([^\)]+\) \[([0-9]+), ", 1, 2, 3),
        // Дневной дозор (Тимур Бекмамбетов) [2006, Россия, ...]
        (r"^([^/\(\[]+) \([^\)]+\) \[([0-9]+), ", 1, 0, 2),
    ];
    let mut res = (None, None, 0);
    for (p, n, o, y) in patterns {
        if let Some((name, orig, year)) = try_names(title, p, n, o, y) {
            res = (Some(name), orig, year);
            break;
        }
    }
    if res.0.is_none() {
        let (n, o, y, skip) = parse_generic(title);
        res = (n, o, y);
        if skip {
            return (None, None, 0, true);
        }
    }
    let name = res.0.map(|n| n.replace("в 3Д", "").trim().to_string());
    let orig = res.1.map(|o| o.replace(" in 3D", "").replace(" 3D", "").trim().to_string());
    (name, orig, res.2, false)
}

fn parse_serial_title(title: &str) -> Names {
    if !rx::is_match_i(title, "(Сезон|Серии)") {
        return (None, None, 0, false);
    }
    let title = &collapse_parens(title);
    let patterns: Vec<(&str, usize, usize, usize)> = if title.contains("Сезон:") {
        vec![
            // Голяк / Без гроша / Без денег / Brassic / Сезон: 4 / Серии: 1-8 из 8 (...) [2022, ...]
            (r"^([^/\(\[]+) / [^/\(\[]+ / [^/\(\[]+ / ([^/\(\[]+) / Сезон: [^/]+ / [^\(\[]+ \([^\)]+\) \[([0-9]+)(,|-)", 1, 2, 3),
            // Уравнитель / Великий уравнитель / The Equalizer / Сезон: 1 / Серии: 1-3 из 4 (...) [2021, ...]
            (r"^([^/\(\[]+) / [^/\(\[]+ / ([^/\(\[]+) / Сезон: [^/]+ / [^\(\[]+ \([^\)]+\) \[([0-9]+)(,|-)", 1, 2, 3),
            // 911 служба спасения / 9-1-1 / Сезон: 4 / Серии: 1-6 из 9 (...) [2021, ...]
            (r"^([^/\(\[]+) / ([^/\(\[]+) / Сезон: [^/]+ / [^\(\[]+ \([^\)]+\) \[([0-9]+)(,|-)", 1, 2, 3),
            // Петербургский роман / Сезон: 1 / Серии: 1-8 из 8 (Александр Муратов) [2018, ...]
            (r"^([^/\(\[]+) / Сезон: [^/]+ / [^\(\[]+ \([^\)]+\) \[([0-9]+)(,|-)", 1, 0, 2),
        ]
    } else {
        vec![
            // Уравнитель / Великий уравнитель / The Equalizer / Серии: 1-3 из 4 (...) [2021, ...]
            (r"^([^/\(\[]+) / [^/\(\[]+ / ([^/\(\[]+) / [^\(\[]+ \([^\)]+\) \[([0-9]+)(,|-)", 1, 2, 3),
            // 911 служба спасения / 9-1-1 / Серии: 1-6 из 9 (...) [2021, ...]
            (r"^([^/\(\[]+) / ([^/\(\[]+) / [^\(\[]+ \([^\)]+\) \[([0-9]+)(,|-)", 1, 2, 3),
            // Петербургский роман / Серии: 1-8 из 8 (Александр Муратов) [2018, ...]
            (r"^([^/\(\[]+) / [^\(\[]+ \([^\)]+\) \[([0-9]+)(,|-)", 1, 0, 2),
        ]
    };
    let mut res: (Option<String>, Option<String>, i32) = (None, None, 0);
    for (p, n, o, y) in patterns {
        if let Some((name, orig, year)) = try_names(title, p, n, o, y) {
            res = (Some(name), orig, year);
            break;
        }
    }
    // a season word inside a real name (`Брачный сезон`) is fine; a numbered season suffix is cut
    let clean = |s: Option<String>| s.map(|v| strip_season(&v)).filter(|v| !v.is_empty());
    let bad = |s: &Option<String>| s.as_deref().map(is_marker).unwrap_or(false) || s.as_deref().map(|v| rx::is_match_i(v, r"(Сезон|Серии)\s*:")).unwrap_or(false);
    let (name, orig) = (clean(res.0), clean(res.1));
    // No usable original from the patterns (none captured, or the captured one was only a season
    // marker): the generic split may still find one, e.g. `Обмани меня Сезон 1 / Lie To Me Season 1`.
    let latin_orig = orig.as_deref().map(has_latin).unwrap_or(false);
    if name.is_none() || orig.is_none() || bad(&name) || bad(&orig) || !latin_orig {
        let (n, o, y, _) = parse_generic(title);
        let generic_ok = n.is_some() && !bad(&n) && !bad(&o);
        if generic_ok && (o.is_some() || name.is_none() || bad(&name)) {
            return (n, o, if y > 0 { y } else { res.2 }, false);
        }
        if name.is_none() || bad(&name) {
            return (None, None, 0, false);
        }
        // keep a non-Latin original from the patterns when nothing better exists
        return (name, orig.filter(|o| !is_marker(o)), res.2, false);
    }
    (name, orig, res.2, false)
}

fn parse_non_standard_title(title: &str) -> Names {
    let name = rx::group(title, r"^([^/\(\[]+) ", 1);
    let relased = rx::group(title, r" \[([0-9]{4})(,|-) ", 1).parse().unwrap_or(0);
    let skip = rx::is_match_i(&name, "(Сезон|Серии)");
    (Some(name), None, relased, skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_director_block_is_collapsed() {
        let t = "Матрица / The Matrix (Братья Вачовски (Энди Вачовски, Ларри Вачовски) / The Wachowski Brothers (Andy Wachowski, Larry Wachowski)) [1999, США, фантастика, боевик, VHSRip -> DVD] [Fullscreen] Dub";
        assert_eq!(collapse_parens("a (b (c) d) e"), "a (x) e");
        assert_eq!(parse(Kind::Movie, t), (Some("Матрица".into()), Some("The Matrix".into()), 1999, false));
        assert_eq!(guess_kind(t), Kind::Movie);
    }

    #[test]
    fn generic_fallback_covers_real_rutracker_titles() {
        let cases: [(&str, Kind, &str, Option<&str>, i32); 9] = [
            ("Друзья / Friends / Сезон: 10 (David Crane, Marta Kauffman) [2004, США, Комедийный сериал, DVDRip, ENG+RUS]", Kind::Serial, "Друзья", Some("Friends"), 2004),
            ("Друзья / Friends / Сезон 1 / Серии 1-24 из 24 (Дэвид Крэйн / David Crane, Марта Кауффман / Marta Kauffman) [1994-1995, США, мелодрама]", Kind::Serial, "Друзья", Some("Friends"), 1994),
            ("Друзья / Friends / Сезон: 1-5 / Серии: 121 (121) (Гари Хэлворсон) [1994-1999, США, Комедия, HDTVRip]", Kind::Serial, "Друзья", Some("Friends"), 1994),
            ("Друзья / Friends (9, 10 сезоны / 9, 10 seasons) (David Crane, Marta Kauffman) [1994-2004, США, комедия, DVDRip-AVC]", Kind::Movie, "Друзья", Some("Friends"), 1994),
            ("Друзья / Friends / Сезон: 10 [2004]", Kind::Serial, "Друзья", Some("Friends"), 2004),
            ("Шерлок / Sherlock / Сезоны: 1-4 / Серии: 1-13 из 13 (Пол МакГиган) [2010-2017, Великобритания, триллер]", Kind::Serial, "Шерлок", Some("Sherlock"), 2010),
            ("Шерлок / Шерлок: Нерассказанные истории / Sherlock: Untold Stories [11/11] + SP [2019, детектив, WEBRip] [720p] DVO", Kind::Movie, "Шерлок", Some("Sherlock: Untold Stories"), 2019),
            ("Терминатор / Terminator(Джеймс Кэмерон / James Cameron) [1984, США, Боевик, BDRip 1080p] MVO", Kind::Movie, "Терминатор", Some("Terminator"), 1984),
            ("Терминатор / The Terminator (Джеймс Кэмерон / James Cameron) [1984; Великобритания, США; фантастика; DVB] Dub", Kind::Movie, "Терминатор", Some("The Terminator"), 1984),
        ];
        for (title, kind, name, orig, year) in cases {
            let (n, o, y, skip) = parse(kind, title);
            assert_eq!((n.as_deref(), o.as_deref(), y, skip), (Some(name), orig, year, false), "{title}");
        }
        // a plain Russian-only title keeps no original
        assert_eq!(parse_generic("Просто фильм [2020, Россия]"), (Some("Просто фильм".into()), None, 2020, false));
    }

    #[test]
    fn season_words_inside_names() {
        let cases: [(&str, &str, Option<&str>); 9] = [
            ("Брачный сезон / Mating Season / Сезон: 1 / Серии: 1-10 из 10 (Энрике Жардим / Henrique Jardim) [2026, США, мультсериал для взрослых, WEB-DL 1080p]", "Брачный сезон", Some("Mating Season")),
            ("Обмани меня Сезон 1 / Lie To Me Season 1 (Сэмюэл Баум / Samuel Baum) [2009, США, драма, DVD9] Диск 3", "Обмани меня", Some("Lie To Me")),
            ("Как не стоит жить, сезон 2 / How Not to Live Your Life (Dan Clark) [2009, Великобритания, комедия] (1001cinema)", "Как не стоит жить", Some("How Not to Live Your Life")),
            ("Гевин и Стейси, 3 сезон / Gavin and Stacey (Кристин Гернон) [2009, Великобритания, романтическо-откровенная комедия, DVDRip] (1001cinema)", "Гевин и Стейси", Some("Gavin and Stacey")),
            ("Звездные Войны: Войны Клонов - Сезон 2 / Star Wars: The Clone Wars - Season 2 [2009, США, Сингапур, Фантастика, Анимация, HDTV 720p (Sky HD)] E01-E22", "Звездные Войны: Войны Клонов", Some("Star Wars: The Clone Wars")),
            ("Бес в ребро, 1 и 2 сезоны, весь сериал - 15 серий / Manchild, season 1-2 (Одри Кук и Дэвид Эванс) [2002-2003, Великобритания, комедия, SATRip]", "Бес в ребро", Some("Manchild")),
            ("Звездный крейсер Галактика. Минисериал + 1 сезон / Battlestar Galactica Miniseries / Сезон: 1 / Серии: 1-13 из 13 (Майкл Раймер, Рональд Д. Мур) [2003-2005, США, фантастика, BDRip]", "Звездный крейсер Галактика. Минисериал", Some("Battlestar Galactica Miniseries")),
            ("Найтвинг: Сериал / Ночное Крыло (мини-сериал) / Nightwing: The Series / Сезон: 1 / Серии: 1-5 из 5 (Адам Зилински) [2014, США, Боевик, WEBRip] MVO", "Найтвинг: Сериал", Some("Nightwing: The Series")),
            ("Бэтмен: мультсериал / Batman: The Animated Series /Сезон: 1/Серии: 2-10,30,38-39 (65)/Сезон: 2/Серии: 9, 14 (20) (Алан Бернет / Alan Burnett) [1992-1995, США, мультсериал, DVDRip]", "Бэтмен: мультсериал", Some("Batman: The Animated Series")),
        ];
        for (title, name, orig) in cases {
            let kind = guess_kind(title);
            let (n, o, _, skip) = parse(kind, title);
            assert_eq!((n.as_deref(), o.as_deref(), skip), (Some(name), orig, false), "{title}");
        }
        let more: [(&str, &str, Option<&str>); 6] = [
            ("Звездный путь. Глубокий космос 9 Сезон 7 (Эпизоды 1-26 (полный)) / Star Trek - Deep Space Nine (Рик Берман, Майкл Пиллер) [1995, США, фантастика, DVDRip]", "Звездный путь. Глубокий космос 9", Some("Star Trek - Deep Space Nine")),
            ("Обмани меня. Сезон 2 (серии 1-22 из 22) / Lie to Me (Кларк Джонсон) [2009, США, комедия, драма, HDTVRip, WEB-DLRip] Первый канал + Eng", "Обмани меня", Some("Lie to Me")),
            ("Шейлок 2 / Новые приключения Шайло / Шайло 2: Сезон охоты на Шайло / Shiloh 2: Shiloh Season (Сэнди Танг /Sandy Tung) [1999, США, Великобритания, семейный, DVDRip]", "Шейлок 2", Some("Shiloh 2: Shiloh Season")),
            ("Бессмертные: Война миров / Immortel (ad vitam) / Immortal (Энки Билал / Enki Bilal) [2004, Франция, Великобритания, Италия, фантастика, драма, DVDRip]", "Бессмертные: Война миров", Some("Immortel")),
            ("КЛОН 60-67 серии/ O Clone (Жайме Монжардим) [2001, бразилия, сериал, SATRip]добавленные серии", "КЛОН", Some("O Clone")),
            ("Южный парк Сезон 23 Эпизод 10 Рождественское шоу / South Park S23E10 - Christmas Snow.mkv / Сезон: 23 / Серии: 10 из 1 (Трей Паркер, Эрик Сточ, Мэтт Стоун) [2019, США, Мультфильм, комедия, WEB-DL 1080p]", "Южный парк", Some("South Park")),
        ];
        for (title, name, orig) in more {
            let (n, o, _, _) = parse(guess_kind(title), title);
            assert_eq!((n.as_deref(), o.as_deref()), (Some(name), orig), "{title}");
        }
        // a season word with no number is part of the name
        assert_eq!(strip_season("Сезон охоты"), "Сезон охоты");
        assert_eq!(strip_season("Open Season"), "Open Season");
        assert_eq!(strip_season("Lie To Me Season 1"), "Lie To Me");
        assert_eq!(strip_season("Концерт Erreway в Мадриде 2006 г."), "Концерт Erreway в Мадриде 2006 г.");
        assert_eq!(strip_season("Никто, кроме тебя. 55 серия из 60"), "Никто, кроме тебя");
    }

    #[test]
    fn row_kind_follows_content_type() {
        let sport = vec!["sport".to_string()];
        assert_eq!(kind_for_row(&sport, "Единая лига ВТБ 2024-2025 / Плей-офф / Финал / Матч! ТВ HD [09.06.2025, Баскетбол, HD/720p]"), Kind::NonStandard);
        let serial = vec!["serial".to_string()];
        assert_eq!(kind_for_row(&serial, "Друзья / Friends / Сезон: 10 [2004]"), Kind::Serial);
        assert_eq!(kind_for_row(&[], "Белый тигр / The White Tiger (Рамин Бахрани) [2021, Индия]"), Kind::Movie);
    }

    #[test]
    fn movie_and_serial_patterns() {
        assert_eq!(
            parse(Kind::Movie, "Белый тигр / The White Tiger (Рамин Бахрани / Ramin Bahrani) [2021, Индия, драма, WEB-DL 1080p]"),
            (Some("Белый тигр".into()), Some("The White Tiger".into()), 2021, false)
        );
        assert_eq!(parse(Kind::Movie, "Дневной дозор (Тимур Бекмамбетов) [2006, Россия, фэнтези, BDRip]"), (Some("Дневной дозор".into()), None, 2006, false));
        let s = "Уравнитель / Великий уравнитель / The Equalizer / Сезон: 1 / Серии: 1-3 из 4 (Лиз Фридлендер (Liz Friedlander)) [2021, США, боевик, WEB-DL 1080p]";
        assert_eq!(parse(Kind::Serial, s), (Some("Уравнитель".into()), Some("The Equalizer".into()), 2021, false));
        assert_eq!(guess_kind(s), Kind::Serial);
        assert_eq!(parse(Kind::Serial, "Просто фильм (Кто-то) [2020, Россия]"), (None, None, 0, false));
    }
}
