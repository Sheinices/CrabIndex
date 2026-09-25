//! Russian/English month-name date parsing used by HTML parsers.

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

use crate::rx;

/// Normalise names that break search keys (ё→е, щ→ш, known titles).
pub fn replace_bad_names(html: &str) -> String {
    html.replace("Ванда/Вижн ", "ВандаВижн ").replace('Ё', "Е").replace('ё', "е").replace('щ', "ш")
}

const MONTHS: [(&str, &str); 36] = [
    (r" янв\.? ", ".01."),
    (r" февр?\.? ", ".02."),
    (r" март?\.? ", ".03."),
    (r" апр\.? ", ".04."),
    (r" май\.? ", ".05."),
    (r" июнь?\.? ", ".06."),
    (r" июль?\.? ", ".07."),
    (r" авг\.? ", ".08."),
    (r" сент?\.? ", ".09."),
    (r" окт\.? ", ".10."),
    (r" нояб?\.? ", ".11."),
    (r" дек\.? ", ".12."),
    (r" январ(ь|я)?\.? ", ".01."),
    (r" феврал(ь|я)?\.? ", ".02."),
    (r" марта?\.? ", ".03."),
    (r" апрел(ь|я)?\.? ", ".04."),
    (r" май?я?\.? ", ".05."),
    (r" июн(ь|я)?\.? ", ".06."),
    (r" июл(ь|я)?\.? ", ".07."),
    (r" августа?\.? ", ".08."),
    (r" сентябр(ь|я)?\.? ", ".09."),
    (r" октябр(ь|я)?\.? ", ".10."),
    (r" ноябр(ь|я)?\.? ", ".11."),
    (r" декабр(ь|я)?\.? ", ".12."),
    (" Jan ", ".01."),
    (" Feb ", ".02."),
    (" Mar ", ".03."),
    (" Apr ", ".04."),
    (" May ", ".05."),
    (" Jun ", ".06."),
    (" Jul ", ".07."),
    (" Aug ", ".08."),
    (" Sep ", ".09."),
    (" Oct ", ".10."),
    (" Nov ", ".11."),
    (" Dec ", ".12."),
];

/// Convert a `dd.MM.yyyy HH:mm`-style date pattern to chrono strftime.
pub fn net_format_to_chrono(format: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        let tok = match (c, n) {
            ('d', 1) | ('d', 2) => "%d".to_string(),
            ('M', 1) | ('M', 2) => "%m".to_string(),
            ('y', 2) => "%y".to_string(),
            ('y', 4) => "%Y".to_string(),
            ('H', 1) | ('H', 2) => "%H".to_string(),
            ('h', 1) | ('h', 2) => "%I".to_string(),
            ('m', 1) | ('m', 2) => "%M".to_string(),
            ('s', 1) | ('s', 2) => "%S".to_string(),
            ('t', 2) => "%p".to_string(),
            ('%', _) => "%%".repeat(n),
            _ => std::iter::repeat(c).take(n).collect(),
        };
        out.push_str(&tok);
        i += n;
    }
    out
}

/// Parse with a `dd.MM.yyyy`-style pattern; `None` on failure.
/// The wall-clock time is stored as UTC as-is (no zone conversion).
pub fn parse_exact(line: &str, net_format: &str) -> Option<DateTime<Utc>> {
    let fmt = net_format_to_chrono(net_format);
    let line = line.trim();
    if let Ok(n) = NaiveDateTime::parse_from_str(line, &fmt) {
        return Some(Utc.from_utc_datetime(&n));
    }
    if let Ok(d) = NaiveDate::parse_from_str(line, &fmt) {
        return Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?));
    }
    None
}

/// Normalise month names, then parse exactly with `format`.
pub fn parse_create_time(line: &str, format: &str) -> Option<DateTime<Utc>> {
    let mut line = line.to_string();
    for (pat, rep) in MONTHS.iter() {
        line = rx::replace_i(&line, pat, rep);
    }
    if rx::is_match(&line, r"^[0-9]\.") {
        line = format!("0{line}");
    }
    parse_exact(&line.to_lowercase(), format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn rutor_style() {
        let d = parse_create_time("05 Янв 24", "dd.MM.yy").unwrap();
        assert_eq!((d.year(), d.month(), d.day()), (2024, 1, 5));
    }

    #[test]
    fn full_month() {
        let d = parse_create_time("5 марта 2023", "dd.MM.yyyy").unwrap();
        assert_eq!((d.year(), d.month(), d.day()), (2023, 3, 5));
    }

    #[test]
    fn with_time() {
        let d = parse_create_time("12.03.2023 14:05", "dd.MM.yyyy HH:mm").unwrap();
        assert_eq!(d.format("%H:%M").to_string(), "14:05");
    }
}
