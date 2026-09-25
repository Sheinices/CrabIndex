//! Kinozal browse page parsing and page classification.
//!
//! Browser-rendered pages (FlareSolverr/Chromium) re-serialize `class='first bg'` / `class=bg`
//! as `class="first bg"` / `class="bg"`, so attribute quotes are optional in every pattern.

use chrono::{DateTime, Duration, Utc};

use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::parsing::tparse;
use crab_core::rx;
use crab_core::{conf, time};

use super::categories::{KinozalTitleKind, MAP};
use crate::common::{g, match_row as m, name_before_brackets, nb, parse_int, utc_datetime, year};

const TRACKER_NAME: &str = "kinozal";

const ROW_SPLIT: &str = r#"<tr class=["']?(?:first )?bg["']?>"#;
/// `/details.php` only - `userdetails.php?id=` contains the substring `details.php?id=`.
const TORRENT_LISTING_HREF: &str = r#"href=["']/details\.php\?id=\d+"#;
const NAM_TORRENT_HREF: &str = r#"<td class=["']?nam["']?>\s*<a href=["']/details\.php\?id=(\d+)["']"#;
const DETAILS_ID_IN_URL: &str = r"/details\.php\?id=(\d+)";
const HTML_TITLE: &str = r"<title>([^<]+)</title>";
/// Digit immediately before `rel="next"` - 1-based last listing page.
const PAGER_DIGIT_BEFORE_NEXT: &str = r#">([0-9]+)</a></li><li><a rel="next""#;
const BROWSE_SELECT: &str = r#"(?s)<select\s+name=["']?(c|d)["']?[^>]*>(.*?)</select>"#;
const BROWSE_OPTION: &str = r"<option([^>]*)>";
const BROWSE_OPTION_VALUE: &str = r#"value=["']?(\d+)"#;
const ARG_YEAR: &str = r"(?:^|[?&])d=(\d{4})(?:&|$)";

/// Length in UTF-16 code units (what the size thresholds below were tuned against).
fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Browse-list date column (header «Залит»): `сегодня/вчера в HH:mm` or `dd.MM.yyyy в HH:mm`.
/// Kinozal shows «Обновлен» when the torrent was re-uploaded, otherwise the upload date.
/// `None` when unparsable.
pub fn parse_listing_update_time(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let raw = raw.filter(|r| !r.trim().is_empty())?;
    let raw = rx::replace(raw.trim(), "[\n\r\t ]+", " ");

    if let Some(c) = rx::captures_i(&raw, "^(сегодня|вчера) в ([0-9]{2}):([0-9]{2})$") {
        let today = time::now().date_naive().and_hms_opt(0, 0, 0)?.and_utc();
        let base = if c.get(1)?.as_str().to_lowercase() == "сегодня" { today } else { today - Duration::days(1) };
        let hour: i64 = c.get(2)?.as_str().parse().ok()?;
        let minute: i64 = c.get(3)?.as_str().parse().ok()?;
        return Some(base + Duration::hours(hour) + Duration::minutes(minute));
    }

    let a = rx::groups(&raw, r"^([0-9]{2})\.([0-9]{2})\.([0-9]{4}) в ([0-9]{2}):([0-9]{2})$");
    if !a[0].is_empty() {
        return utc_datetime(a[3].parse().ok()?, a[2].parse().ok()?, a[1].parse().ok()?, a[4].parse().ok()?, a[5].parse().ok()?);
    }

    tparse::parse_create_time(&raw, "dd.MM.yyyy")
}

pub fn parse_torrents_from_page(html: &str, cat: &str) -> Vec<TorrentDetails> {
    let mut torrents = Vec::new();
    let Some(meta) = MAP.get(cat) else { return torrents };

    let html = tparse::replace_bad_names(html);
    for row in rx::split_i(&html, ROW_SPLIT).into_iter().skip(1) {
        if row.trim().is_empty() {
            continue;
        }

        let listing_time = m(&row, r#"<td class=["']?sl_p["']?>[0-9]+</td>\s*<td class=["']?s["']?>([^<]+)</td>"#, 1);
        let Some(create_time) = parse_listing_update_time(Some(&listing_time)) else { continue };

        let Some(details_id) = details_id_from_row(&row) else { continue };

        let title = m(&row, r#"class=["']?r[0-9]+["']?>([^<]+)</a>"#, 1);
        let sid = m(&row, r#"<td class=["']?sl_s["']?>([0-9]+)</td>"#, 1);
        let pir = m(&row, r#"<td class=["']?sl_p["']?>([0-9]+)</td>"#, 1);
        let size_name = m(&row, r#"<td class=["']?s["']?>([0-9\.,]+ (МБ|ГБ|ТБ))</td>"#, 1);

        if !nb(&title) || !nb(&sid) || !nb(&pir) || !nb(&size_name) {
            continue;
        }

        let url = details_url(&conf().Kinozal.host, details_id);

        let (mut name, originalname, relased) = match meta.title_kind {
            KinozalTitleKind::Movie => parse_movie_title(&title),
            KinozalTitleKind::SerialRu => parse_serial_ru_title(&title, &row),
            KinozalTitleKind::SerialEn => parse_serial_en_title(&title, &row),
            KinozalTitleKind::TvShow => parse_tv_show_title(&title),
        };
        if !nb(&name) {
            name = name_before_brackets(&title);
        }
        if !nb(&name) {
            continue;
        }

        let mut t = TorrentDetails::new(TRACKER_NAME, meta.types, url, title);
        t.sid = parse_int(&sid);
        t.pir = parse_int(&pir);
        t.sizeName = size_name;
        t.createTime = create_time;
        t.name = name;
        t.originalname = originalname;
        t.relased = relased;
        torrents.push(t);
    }
    torrents
}

type Names = (String, String, i32);

fn parse_movie_title(title: &str) -> Names {
    // Бэд трип (Приколисты в дороге) / Bad Trip / 2020 / ДБ, СТ / WEB-DLRip (AVC)
    // Интерстеллар / Interstellar (IMAX Edition) / 2014 / ДБ / BDRip
    let x = g(title, r"^([^\(/]+) (\([^\)/]+\) )?/ ([^\(/]+) (\([^\)/]+\) )?/ ([0-9]{4})");
    if nb(&x[1]) && nb(&x[3]) && nb(&x[5]) {
        return (x[1].clone(), x[3].clone(), year(&x[5]));
    }
    // Name may contain parentheses and season-like slashes (RU-only titles):
    // Голая правда / 2020 / ЛМ / WEB-DLRip
    if let Some(c) = rx::captures(title, r" / ((?:19|20)[0-9]{2}) / ") {
        if let (Some(whole), Some(y)) = (c.get(0), c.get(1)) {
            return (title[..whole.start()].trim().to_string(), String::new(), year(y.as_str()));
        }
    }
    (String::new(), String::new(), 0)
}

fn parse_serial_ru_title(title: &str, row: &str) -> Names {
    if row.contains("сезон") {
        // Сельский детектив (6 сезон: 1-2 серии из 2) / 2020 / РУ / WEB-DLRip (AVC)
        // Фитнес (Королева фитнеса) (1-4 сезон: 1-80 серии из 80) / 2018-2020 / РУ / WEB-DLRip
        let x = g(title, r"^([^\(/]+) (\([^\)/]+\) )?\([0-9\-]+ сезоны?: [^\)/]+\) ([^/]+ )?/ ([0-9]{4})");
        if nb(&x[1]) && nb(&x[4]) {
            return (x[1].clone(), String::new(), year(&x[4]));
        }
        (String::new(), String::new(), 0)
    } else {
        // Авантюра на двоих (1-8 серии из 8) / 2021 / РУ /  WEBRip (AVC)
        let x = g(title, r"^([^\(/]+) (\([^\)/]+\) )?\([^\)/]+\) ([^/]+ )?/ ([0-9]{4})");
        (x[1].clone(), String::new(), year(&x[4]))
    }
}

fn parse_serial_en_title(title: &str, row: &str) -> Names {
    if row.contains("сезон") {
        // Сокол и Зимний солдат (1 сезон: 1-2 серия из 6) / The Falcon and the Winter Soldier / 2021 / …
        let x = g(title, r"^([^\(/]+) (\([^\)/]+\) )?\([0-9\-]+ сезоны?: [^\)/]+\) ([^/]+ )?/ ([^\(/]+) / ([0-9]{4})");
        if nb(&x[1]) && nb(&x[4]) && nb(&x[5]) {
            return (x[1].clone(), x[4].clone(), year(&x[5]));
        }
        (String::new(), String::new(), 0)
    } else {
        // Дикий ангел (151-270 серии из 270) / Muneca Brava / 1998-1999 / ПМ / DVB
        let x = g(title, r"^([^\(/]+) (\([^\)/]+\) )?\([^\)/]+\) ([^/]+ )?/ ([^\(/]+) / ([0-9]{4})");
        if nb(&x[1]) && nb(&x[4]) && nb(&x[5]) {
            return (x[1].clone(), x[4].clone(), year(&x[5]));
        }
        let x = g(title, r"^([^\(/]+) / ([^\(/]+) / ([0-9]{4})");
        (x[1].clone(), x[2].clone(), year(&x[3]))
    }
}

fn parse_tv_show_title(title: &str) -> Names {
    // Топ Гир (30 сезон: 1-2 выпуски из 10) / Top Gear / 2021 / ЛМ (ColdFilm) / WEBRip
    let x = g(title, r"^([^\(/]+) (\([^\)/]+\) )?/ ([^\(/]+) / ([0-9]{4})");
    if nb(&x[1]) && nb(&x[3]) && nb(&x[4]) {
        return (x[1].clone(), x[3].clone(), year(&x[4]));
    }
    // Супермама (3 сезон: 1-12 выпуски из 40) / 2021 / РУ / IPTV (1080p)
    let x = g(title, r"^([^/\(]+) (\([^\)/]+\) )?/ ([0-9]{4})");
    (x[1].clone(), String::new(), year(&x[3]))
}

/// Info hash from `get_srv_details.php?id=&action=2` (a short fragment with «Инфо хеш»).
/// Browse-sized HTML (stale browser tab) is ignored so no fake magnet is minted.
pub fn parse_info_hash(html: Option<&str>) -> Option<String> {
    let html = html.filter(|h| !h.trim().is_empty())?;
    let labeled = rx::group(html, r"<ul><li>Инфо хеш:\s*([A-Fa-f0-9]{40})</li>", 1);
    if !labeled.is_empty() {
        return Some(labeled);
    }
    if len16(html) > 8000 {
        return None;
    }
    let loose = rx::group(html, "([A-Fa-f0-9]{40})", 1);
    (!loose.is_empty()).then_some(loose)
}

pub fn is_logged_in(html: &str) -> bool {
    !html.is_empty() && html.contains(">Выход</a>")
}

/// Missing body, nginx 503 behind Cloudflare, or a CF interstitial - retry, do not log in again.
pub fn is_transient_browse_failure(html: Option<&str>) -> bool {
    let Some(html) = html.filter(|h| !h.trim().is_empty()) else { return true };
    let lower = html.to_lowercase();
    if lower.contains("just a moment") || lower.contains("один момент") {
        return true;
    }
    len16(html) < 2000 && lower.contains("503 service temporarily unavailable")
}

pub fn is_login_wall(html: Option<&str>) -> bool {
    let Some(h) = html.filter(|h| !h.trim().is_empty()) else { return false };
    if is_transient_browse_failure(html) || is_logged_in(h) {
        return false;
    }
    let lower = h.to_lowercase();
    lower.contains("takelogin.php") || lower.contains("take_login") || lower.contains("name=\"username\"")
}

/// Torrent id from a details URL; `None` for `userdetails.php` profile links.
pub fn try_get_details_id(url: &str) -> Option<i32> {
    if url.is_empty() || url.to_lowercase().contains("userdetails") {
        return None;
    }
    rx::captures_i(url, DETAILS_ID_IN_URL)?.get(1)?.as_str().parse::<i32>().ok().filter(|id| *id > 0)
}

pub fn details_url(host: &str, id: i32) -> String {
    let base = if host.trim().is_empty() { "https://kinozal.guru" } else { host.trim_end_matches('/') };
    format!("{base}/details.php?id={id}")
}

fn details_id_from_row(row: &str) -> Option<i32> {
    if let Some(id) = rx::captures_i(row, NAM_TORRENT_HREF).and_then(|c| c.get(1)?.as_str().parse::<i32>().ok()).filter(|id| *id > 0) {
        return Some(id);
    }
    let href = rx::group_i(row, TORRENT_LISTING_HREF, 0);
    if href.is_empty() {
        return None;
    }
    try_get_details_id(&href)
}

pub fn count_torrent_listing_links(html: Option<&str>) -> usize {
    match html {
        Some(h) if !h.is_empty() => rx::re_i(TORRENT_LISTING_HREF).find_iter(h).filter(|m| m.is_ok()).count(),
        _ => 0,
    }
}

pub fn has_torrent_listing_links(html: Option<&str>) -> bool {
    count_torrent_listing_links(html) > 0
}

fn has_kinozal_title(html: &str) -> bool {
    if html.is_empty() {
        return false;
    }
    let t = rx::group_i(html, HTML_TITLE, 1);
    if !t.is_empty() && t.to_lowercase().contains("кинозал") {
        return true;
    }
    html.contains("Кинозал.GURU") || html.contains("Кинозал.ТВ")
}

/// Real browse table (header present). Rows are not required - empty categories are valid.
pub fn is_valid_browse_page(html: Option<&str>) -> bool {
    matches!(html, Some(h) if !h.trim().is_empty() && h.contains("t_peer") && has_kinozal_title(h))
}

/// Year filter past the last listing: logged-in chrome, no table, «Нет активных раздач».
/// («уточните параметры поиска» also appears on listings over 5000 hits, so it is not used.)
pub fn is_empty_search_result(html: Option<&str>) -> bool {
    if is_transient_browse_failure(html) || is_login_wall(html) {
        return false;
    }
    let Some(h) = html else { return false };
    if h.contains("t_peer") {
        return false;
    }
    if !is_logged_in(h) || !has_kinozal_title(h) {
        return false;
    }
    h.contains("Нет активных раздач")
}

/// Selected option value of browse `select name=c|d`; `None` without such a form.
pub fn try_get_selected_browse_filter(html: &str, name: &str) -> Option<String> {
    if html.is_empty() || name.is_empty() {
        return None;
    }
    for select in rx::all_groups_i(html, BROWSE_SELECT) {
        if !select[1].eq_ignore_ascii_case(name) {
            continue;
        }
        for option in rx::all_groups_i(&select[2], BROWSE_OPTION) {
            let attrs = &option[1];
            if !attrs.to_lowercase().contains("selected") {
                continue;
            }
            let v = rx::group_i(attrs, BROWSE_OPTION_VALUE, 1);
            return (!v.is_empty()).then_some(v);
        }
    }
    None
}

pub fn try_get_requested_year(arg: Option<&str>) -> Option<String> {
    let arg = arg.filter(|a| !a.is_empty())?;
    let y = rx::group_i(arg, ARG_YEAR, 1);
    (!y.is_empty()).then_some(y)
}

/// Leftover browser tab: listing/empty HTML for another category or year. Without form
/// fields it is not a mismatch; selected year 0 (all years) is not a mismatch either.
/// Hourly parse (`arg == None`) checks the category only.
pub fn browse_filters_mismatch(html: Option<&str>, cat: &str, arg: Option<&str>) -> bool {
    let Some(html) = html.filter(|h| !h.trim().is_empty()) else { return false };
    if cat.trim().is_empty() {
        return false;
    }
    if let Some(selected_cat) = try_get_selected_browse_filter(html, "c") {
        if selected_cat != cat {
            return true;
        }
    }
    if let Some(year) = try_get_requested_year(arg) {
        if let Some(selected_year) = try_get_selected_browse_filter(html, "d") {
            if selected_year != "0" && selected_year != year {
                return true;
            }
        }
    }
    false
}

/// The digit before `rel="next"` is the 1-based last listing page, URL `page` is 0-based:
/// pages `0..count`. No pager → one page.
pub fn year_task_page_count_digit(pager_digit_before_next: i32) -> i32 {
    if pager_digit_before_next <= 0 {
        1
    } else {
        pager_digit_before_next
    }
}

pub fn year_task_page_count(html: &str) -> i32 {
    if html.trim().is_empty() {
        return 1;
    }
    match rx::group_i(html, PAGER_DIGIT_BEFORE_NEXT, 1).parse::<i32>() {
        Ok(d) => year_task_page_count_digit(d),
        Err(_) => 1,
    }
}

/// Drop URL pages at or past the page count (old inclusive tails).
pub fn prune_pages_beyond_year_count(tasks: &mut Vec<TaskParse>, page_count: i32) -> i32 {
    if tasks.is_empty() {
        return 0;
    }
    let page_count = page_count.max(1);
    let before = tasks.len();
    tasks.retain(|t| t.page < page_count);
    (before - tasks.len()) as i32
}

/// Length / t_peer / title for stale-page logs (no cookies, no body).
pub fn format_browse_diag(html: Option<&str>) -> String {
    let Some(html) = html.filter(|h| !h.is_empty()) else { return "len=0".into() };
    let t: String = rx::group_i(html, HTML_TITLE, 1).trim().chars().take(80).collect();
    format!("len={} t_peer={} title={t}", len16(html), if html.contains("t_peer") { "True" } else { "False" })
}

/// UpdateTasksParse year-page delay, capped so 25 cats × ~37 years finish inside the wall clock.
pub fn update_tasks_parse_delay_ms(parse_delay: i32) -> i32 {
    parse_delay.clamp(0, 2000)
}

/// Logged-in chrome without a `t_peer` table (typical ~15 KB empty browser tab): retry,
/// do not log in again and do not mark the page done. Empty search is not stale.
pub fn is_stale_listing_html(html: Option<&str>) -> bool {
    if is_transient_browse_failure(html) || is_login_wall(html) || is_empty_search_result(html) {
        return false;
    }
    let Some(h) = html else { return false };
    if h.contains("t_peer") {
        return false;
    }
    is_logged_in(h)
}

/// Empty listing (no torrent hrefs) → done. Parser miss (hrefs but 0 rows) → not done.
/// Rows still needing a magnet → not done until every row is resolved.
pub fn should_mark_page_done(parsed_count: usize, resolved_count: usize, listing_href_count: usize) -> bool {
    if listing_href_count > 0 && parsed_count == 0 {
        return false;
    }
    if parsed_count == 0 {
        return true;
    }
    resolved_count >= parsed_count
}

/// Kinozal re-hashes the .torrent when episodes/voices are added while the listing title
/// often stays the same, so the hash is re-fetched unless title, size and date are unchanged.
pub fn should_skip_hash_fetch(cached: &TorrentDetails, parsed: &TorrentDetails) -> bool {
    if cached.magnet.trim().is_empty() {
        return false;
    }
    if cached.title != parsed.title || cached.sizeName != parsed.sizeName {
        return false;
    }
    parsed.createTime <= cached.createTime
}
