//! Parses NUM / Lampa-style plain `query` strings into card fields
//! (`title`, `title_original`, `year`) so exact FileDB matching works.
//! Covers Ru-first ("Русское English 1999") and En-first ("English Русское 1999") forms.

use crab_core::{rx, util::is_blank};

use super::params;
use super::request::IndexerSearchRequest;

const RU_EN_YEAR: &str = r"^([^a-zA-Z]+) ([^а-яА-ЯёЁ]+) ((?:19|20)\d{2})$";
const RU_EN: &str = r"^([^a-zA-Z]+) ([^а-яА-ЯёЁ]+)$";
const EN_RU_YEAR: &str = r"^([^а-яА-ЯёЁ]+) ([^a-zA-Z]+) ((?:19|20)\d{2})$";
const EN_RU: &str = r"^([^а-яА-ЯёЁ]+) ([^a-zA-Z]+)$";
const TRAILING_YEAR: &str = r"^(.+?)\s+((?:19|20)\d{2})$";
const CYRILLIC: &str = r"[а-яА-ЯёЁ]";
const LATIN: &str = r"[a-zA-Z]";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub title: Option<String>,
    pub title_original: Option<String>,
    pub year: i32,
    pub matched: bool,
}

fn nb(s: &Option<String>) -> bool {
    s.as_deref().map(|x| !is_blank(x)).unwrap_or(false)
}

fn groups(text: &str, pattern: &str) -> Option<Vec<String>> {
    let c = rx::captures(text, pattern)?;
    Some((0..c.len()).map(|i| c.get(i).map(|m| m.as_str().to_string()).unwrap_or_default()).collect())
}

fn has_latin(v: &str) -> bool {
    !is_blank(v) && rx::is_match(v, LATIN)
}

fn has_cyrillic(v: &str) -> bool {
    !is_blank(v) && rx::is_match(v, CYRILLIC)
}

/// Best-effort parse of free text into title / original title / year.
pub fn parse(query: Option<&str>) -> Parsed {
    let mut result = Parsed::default();
    let Some(query) = query.filter(|q| !is_blank(q)) else {
        return result;
    };
    let q = query.trim();

    let (sru, sen) = params::split_bilingual_query(Some(q));
    if nb(&sru) || nb(&sen) {
        let mut ru = sru;
        let mut en = sen;
        if let Some((stripped, y)) = try_take_trailing_year(ru.as_deref()) {
            result.year = y;
            ru = Some(stripped);
        } else if let Some((stripped, y)) = try_take_trailing_year(en.as_deref()) {
            result.year = y;
            en = Some(stripped);
        }
        result.matched = nb(&ru) || nb(&en);
        result.title = ru;
        result.title_original = en;
        return result;
    }

    if let Some(g) = groups(q, RU_EN_YEAR) {
        if has_latin(&g[2]) {
            result.title = Some(g[1].trim().to_string());
            result.title_original = Some(g[2].trim().to_string());
            result.year = g[3].parse::<i32>().ok().filter(|&y| y > 0).unwrap_or(0);
            result.matched = true;
            return result;
        }
    }

    if let Some(g) = groups(q, EN_RU_YEAR) {
        if has_latin(&g[1]) && has_cyrillic(&g[2]) {
            result.title_original = Some(g[1].trim().to_string());
            result.title = Some(g[2].trim().to_string());
            result.year = g[3].parse::<i32>().ok().filter(|&y| y > 0).unwrap_or(0);
            result.matched = true;
            return result;
        }
    }

    // strip the trailing year first so "Константин 2005" / "Pulp Fiction 1994" work
    let mut body = q.to_string();
    if let Some((stripped, y)) = try_take_trailing_year(Some(q)) {
        result.year = y;
        body = stripped;
    }

    if let Some(g) = groups(&body, RU_EN) {
        if has_latin(&g[2]) {
            result.title = Some(g[1].trim().to_string());
            result.title_original = Some(g[2].trim().to_string());
            result.matched = true;
            return result;
        }
    }

    if let Some(g) = groups(&body, EN_RU) {
        if has_latin(&g[1]) && has_cyrillic(&g[2]) {
            result.title_original = Some(g[1].trim().to_string());
            result.title = Some(g[2].trim().to_string());
            result.matched = true;
            return result;
        }
    }

    if rx::is_match(&body, CYRILLIC) {
        result.title = Some(body);
    } else {
        result.title_original = Some(body);
    }
    result.matched = nb(&result.title) || nb(&result.title_original) || result.year > 0;
    result
}

/// When no explicit card titles were provided, promote the free-text query into card
/// fields and enable card mode. Skips external ids (tt/kp/tmdb) and requests that
/// already carry a title.
pub fn apply_to_request(req: &mut IndexerSearchRequest) -> bool {
    if nb(&req.title) || nb(&req.title_original) {
        return false;
    }
    if !nb(&req.query) {
        return false;
    }
    // an external id is not a card title; keep it for the id-resolve path
    if params::is_imdb_or_kp_query(req.query.as_deref()) {
        return false;
    }

    let parsed = parse(req.query.as_deref());
    if !parsed.matched {
        return false;
    }
    if nb(&parsed.title) {
        req.title = parsed.title.clone();
    }
    if nb(&parsed.title_original) {
        req.title_original = parsed.title_original.clone();
    }
    if parsed.year > 0 && req.year <= 0 {
        req.year = parsed.year;
    }

    req.card_mode = params::is_card_metadata_search(
        req.title.as_deref(),
        req.title_original.as_deref(),
        if req.is_serial >= 0 { Some(req.is_serial) } else { None },
        &req.categories,
        req.genres.as_deref(),
    );
    req.card_mode
}

fn try_take_trailing_year(value: Option<&str>) -> Option<(String, i32)> {
    let value = value.filter(|v| !is_blank(v))?;
    let g = groups(value.trim(), TRAILING_YEAR)?;
    let year = g[2].parse::<i32>().ok()?;
    if !(1900..=2100).contains(&year) {
        return None;
    }
    let stripped = g[1].trim().to_string();
    if is_blank(&stripped) {
        return None;
    }
    Some((stripped, year))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexers::prowlarr_query;

    #[test]
    fn parse_ru_en_year_extracts_title_original_and_year() {
        let p = parse(Some("Криминальное чтиво Pulp Fiction 1994"));
        assert!(p.matched);
        assert_eq!(p.title.as_deref(), Some("Криминальное чтиво"));
        assert_eq!(p.title_original.as_deref(), Some("Pulp Fiction"));
        assert_eq!(p.year, 1994);
    }

    #[test]
    fn parse_en_ru_year() {
        let p = parse(Some("Pulp Fiction Криминальное чтиво 1994"));
        assert!(p.matched);
        assert_eq!(p.title.as_deref(), Some("Криминальное чтиво"));
        assert_eq!(p.title_original.as_deref(), Some("Pulp Fiction"));
        assert_eq!(p.year, 1994);
    }

    #[test]
    fn parse_en_ru() {
        let p = parse(Some("Pulp Fiction Криминальное чтиво"));
        assert!(p.matched);
        assert_eq!(p.title.as_deref(), Some("Криминальное чтиво"));
        assert_eq!(p.title_original.as_deref(), Some("Pulp Fiction"));
        assert_eq!(p.year, 0);
    }

    #[test]
    fn parse_original_trailing_year_sets_title_original_and_year() {
        let p = parse(Some("Pulp Fiction 1994"));
        assert!(p.matched);
        assert!(p.title.as_deref().unwrap_or("").is_empty());
        assert_eq!(p.title_original.as_deref(), Some("Pulp Fiction"));
        assert_eq!(p.year, 1994);
    }

    #[test]
    fn parse_ru_en_extracts_titles() {
        let p = parse(Some("Константин Constantine"));
        assert!(p.matched);
        assert_eq!(p.title.as_deref(), Some("Константин"));
        assert_eq!(p.title_original.as_deref(), Some("Constantine"));
        assert_eq!(p.year, 0);
    }

    #[test]
    fn parse_ru_en_year_constantine() {
        let p = parse(Some("Константин Constantine 2005"));
        assert!(p.matched);
        assert_eq!(p.title.as_deref(), Some("Константин"));
        assert_eq!(p.title_original.as_deref(), Some("Constantine"));
        assert_eq!(p.year, 2005);
    }

    #[test]
    fn parse_cyrillic_trailing_year_sets_title_and_year() {
        let p = parse(Some("Константин 2005"));
        assert!(p.matched);
        assert_eq!(p.title.as_deref(), Some("Константин"));
        assert!(p.title_original.as_deref().unwrap_or("").is_empty());
        assert_eq!(p.year, 2005);
    }

    #[test]
    fn parse_cyrillic_only_sets_title() {
        let p = parse(Some("Криминальное чтиво"));
        assert!(p.matched);
        assert_eq!(p.title.as_deref(), Some("Криминальное чтиво"));
        assert_eq!(p.year, 0);
    }

    #[test]
    fn apply_to_request_rqnum_promotes_query_to_card_mode() {
        let mut req = IndexerSearchRequest {
            query: Some("Криминальное чтиво Pulp Fiction 1994".into()),
            rq_num: true,
            is_serial: -1,
            ..Default::default()
        };
        assert!(apply_to_request(&mut req));
        assert!(req.card_mode);
        assert_eq!(req.title.as_deref(), Some("Криминальное чтиво"));
        assert_eq!(req.title_original.as_deref(), Some("Pulp Fiction"));
        assert_eq!(req.year, 1994);
    }

    #[test]
    fn apply_to_request_without_rqnum_promotes_when_titles_absent() {
        let mut req = IndexerSearchRequest {
            query: Some("Pulp Fiction Криминальное чтиво 1994".into()),
            rq_num: false,
            is_serial: 1,
            ..Default::default()
        };
        assert!(apply_to_request(&mut req));
        assert!(req.card_mode);
        assert_eq!(req.title.as_deref(), Some("Криминальное чтиво"));
        assert_eq!(req.title_original.as_deref(), Some("Pulp Fiction"));
        assert_eq!(req.year, 1994);
    }

    #[test]
    fn apply_to_request_cyrillic_year_enables_card_mode() {
        let mut req = IndexerSearchRequest { query: Some("Константин 2005".into()), rq_num: true, ..Default::default() };
        assert!(apply_to_request(&mut req));
        assert!(req.card_mode);
        assert_eq!(req.title.as_deref(), Some("Константин"));
        assert_eq!(req.year, 2005);
    }

    #[test]
    fn apply_to_request_skips_when_title_already_set() {
        let mut req = IndexerSearchRequest {
            query: Some("Криминальное чтиво Pulp Fiction 1994".into()),
            title: Some("Already".into()),
            rq_num: true,
            ..Default::default()
        };
        assert!(!apply_to_request(&mut req));
        assert_eq!(req.title.as_deref(), Some("Already"));
    }

    #[test]
    fn apply_to_request_skips_external_ids() {
        let mut req = IndexerSearchRequest { query: Some("tt0137523".into()), ..Default::default() };
        assert!(!apply_to_request(&mut req));
        assert!(!req.card_mode);
    }

    #[test]
    fn prowlarr_plain_ru_en_year_still_enriches() {
        let p = prowlarr_query::parse(Some("Криминальное чтиво Pulp Fiction 1994"), Some("search"));
        assert_eq!(p.title.as_deref(), Some("Криминальное чтиво"));
        assert_eq!(p.title_original.as_deref(), Some("Pulp Fiction"));
        assert_eq!(p.year, Some(1994));
    }

    #[test]
    fn prowlarr_plain_en_ru_year_still_enriches() {
        let p = prowlarr_query::parse(Some("Pulp Fiction Криминальное чтиво 1994"), Some("search"));
        assert_eq!(p.title.as_deref(), Some("Криминальное чтиво"));
        assert_eq!(p.title_original.as_deref(), Some("Pulp Fiction"));
        assert_eq!(p.year, Some(1994));
    }
}
