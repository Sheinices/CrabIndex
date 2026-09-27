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

/// Best-effort kind for a title without its forum: serial when it carries a season/episode
/// block, movie otherwise (the non-standard family is never guessed).
pub fn guess_kind(title: &str) -> Kind {
    if rx::is_match_i(title, "(Сезон|Серии)") {
        Kind::Serial
    } else {
        Kind::Movie
    }
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
    let bad = |s: &Option<String>| rx::is_match_i(s.as_deref().unwrap_or(""), "(Сезон|Серии)");
    if bad(&res.0) || bad(&res.1) {
        return (None, None, 0, false);
    }
    (res.0, res.1, res.2, false)
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
