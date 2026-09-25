//! NNMClub portal page parsing.

use chrono::{DateTime, Utc};

use crab_core::conf;
use crab_core::models::TorrentDetails;
use crab_core::parsing::tparse;
use crab_core::rx;

use super::categories::{NNMClubTitleKind, MAP};
use crate::common::{g, match_row as m, name_before_brackets, nb, parse_int, year};

const TRACKER_NAME: &str = "nnmclub";

pub fn parse_torrents_from_page(html: &str, cat: &str) -> Vec<TorrentDetails> {
    let Some(meta) = MAP.get(cat) else { return Vec::new() };

    let flat = rx::replace(html, "(\n|\r|\t)", "");
    let container = rx::group(&flat, "<td valign=\"top\" width=\"[0-9]+%\">(.*)<div class=\"paginport nav\">", 1);
    if container.trim().is_empty() {
        return Vec::new();
    }

    let mut torrents = Vec::new();
    let container = tparse::replace_bad_names(&container);
    for row in container.split("<table width=\"100%\" class=\"pline\">") {
        let magnet = rx::group(row, "\"(magnet:[^\"]+)\"", 1);
        if magnet.trim().is_empty() {
            continue;
        }
        let Some(f) = parse_row_fields(row) else { continue };

        let title_lower = f.title.to_lowercase();
        if title_lower.contains("трейлер") {
            continue;
        }
        if meta.skip_pdf_in_title && title_lower.contains("pdf") {
            continue;
        }
        if meta.require_mult_in_row && !row_looks_like_cartoon(row) {
            continue;
        }

        let (mut name, originalname, relased) = parse_title_names(meta.title_kind, &f.title);
        if !nb(&name) {
            name = name_before_brackets(&f.title);
        }
        if !nb(&name) {
            continue;
        }

        let mut t = TorrentDetails::new(TRACKER_NAME, meta.types, f.url, f.title);
        t.sid = parse_int(&f.sid);
        t.pir = parse_int(&f.pir);
        t.sizeName = f.size_name;
        t.magnet = magnet;
        t.createTime = f.create_time;
        t.name = name;
        t.originalname = originalname;
        t.relased = relased;
        torrents.push(t);
    }
    torrents
}

fn row_looks_like_cartoon(row: &str) -> bool {
    let lower = row.to_lowercase();
    lower.contains("мульт") || lower.contains("длительность") || lower.contains("продолжительность")
}

struct RowFields {
    url: String,
    title: String,
    sid: String,
    pir: String,
    size_name: String,
    create_time: DateTime<Utc>,
}

fn parse_row_fields(row: &str) -> Option<RowFields> {
    let create_time =
        tparse::parse_create_time(&m(row, "\\| ([0-9]+ [^ ]+ [0-9]{4} [^<]+)</span> \\| <span class=\"tit\"", 1), "dd.MM.yyyy HH:mm:ss")?;

    let url = m(row, "<a class=\"pgenmed\" href=\"(viewtopic.php[^\"]+)\"", 1);
    let title = m(row, ">([^<]+)</a></h2></td>", 1);
    let sid = m(row, "title=\"Раздаю[щш]их\">&nbsp;([0-9]+)</span>", 1);
    let pir = m(row, "title=\"Качают\">&nbsp;([0-9]+)</span>", 1);
    let size_name = m(row, "<span class=\"pcomm bold\">([^<]+)</span>", 1);

    if !nb(&url) || !nb(&title) || !nb(&sid) || !nb(&pir) || !nb(&size_name) {
        return None;
    }
    let url = format!("{}/forum/{url}", conf().NNMClub.host);
    Some(RowFields { url, title, sid, pir, size_name, create_time })
}

type Names = (String, String, i32);

fn parse_title_names(kind: NNMClubTitleKind, title: &str) -> Names {
    match kind {
        NNMClubTitleKind::ForeignCinema
        | NNMClubTitleKind::ForeignSerial
        | NNMClubTitleKind::ShowLike
        | NNMClubTitleKind::Sport => parse_foreign_cinema_title(title),
        NNMClubTitleKind::RuMovie => parse_domestic_movie_title(title),
        NNMClubTitleKind::RuSerial => parse_domestic_serial_title(title),
        NNMClubTitleKind::Anime => parse_anime_title(title),
        NNMClubTitleKind::KidsMult => parse_kids_title(title),
    }
}

/// name=g[n], orig=g[o], year=g[y] when all three are non-blank.
fn try3(title: &str, pattern: &str, n: usize, o: usize, y: usize) -> Option<Names> {
    let x = g(title, pattern);
    (nb(&x[n]) && nb(&x[o]) && nb(&x[y])).then(|| (x[n].clone(), x[o].clone(), year(&x[y])))
}

fn parse_foreign_cinema_title(title: &str) -> Names {
    // Крестная мама (Наркомама) / La Daronne / Mama Weed (2020)
    if let Some(r) = try3(title, r"^([^/\(\|]+) \([^\)]+\) / [^/\(\|]+ / ([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)", 1, 2, 3) {
        return r;
    }
    // Связанный груз / Белые рабыни-девственницы / Bound Cargo / White Slave Virgins (2003) DVDRip
    if let Some(r) = try3(title, r"^([^/\(\|]+) / [^/\(\|]+ / [^/\(\|]+ / ([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)", 1, 2, 3) {
        return r;
    }
    // Академия монстров / Escuela de Miedo / Cranston Academy: Monster Zone (2020)
    if let Some(r) = try3(title, r"^([^/\(\|]+) / [^/\(\|]+ / ([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)", 1, 2, 3) {
        return r;
    }
    // Воображаемая реальность (Долина богов) / Valley of the Gods (2019)
    if let Some(r) = try3(title, r"^([^/\(\|]+) \([^\)]+\) / ([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)", 1, 2, 3) {
        return r;
    }
    // Страна грёз / Dreamland (2019)
    if let Some(r) = try3(title, r"^([^/\(\|]+) / ([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)", 1, 2, 3) {
        return r;
    }
    // Тайны анатомии (Мозг) (2020)
    let x = g(title, r"^([^/\(\|]+) \([^\)]+\) \(([0-9]{4})(-[0-9]{4})?\)");
    if nb(&x[1]) && nb(&x[2]) {
        return (x[1].clone(), String::new(), year(&x[2]));
    }
    // Презумпция виновности (2020)
    let x = g(title, r"^([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)");
    (x[1].clone(), String::new(), year(&x[2]))
}

fn parse_domestic_movie_title(title: &str) -> Names {
    let x = g(title, r"^([^/\(\|]+) \(([0-9]{4})\)");
    (x[1].clone(), String::new(), year(&x[2]))
}

fn parse_domestic_serial_title(title: &str) -> Names {
    // Теория вероятности / Игрок (2020)
    let x = g(title, r"^([^/\(\|]+) / [^/\(\|]+ \(([0-9]{4})(-[0-9]{4})?\)");
    if nb(&x[1]) && nb(&x[2]) {
        return (x[1].clone(), String::new(), year(&x[2]));
    }
    // Тайны следствия (2020)
    let x = g(title, r"^([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)");
    (x[1].clone(), String::new(), year(&x[2]))
}

fn parse_anime_title(title: &str) -> Names {
    // Anime titles put the original first: name=g[2], original=g[1].
    let swap = |r: Names| (r.1, r.0, r.2);
    // Black Clover (2017) | Чёрный клевер (часть 2) [2017(-2021)?,
    if let Some(r) = try3(title, r"^([^/\[\(]+) \([0-9]{4}\) \| ([^/\[\(]+) \([^\)]+\) \[([0-9]{4})(-[0-9]{4})?,", 1, 2, 3) {
        return swap(r);
    }
    // Black Clover (2017) | Чёрный клевер [2017(-2021)?,
    if let Some(r) = try3(title, r"^([^/\[\(]+) \([0-9]{4}\) \| ([^/\[\(]+) \[([0-9]{4})(-[0-9]{4})?,", 1, 2, 3) {
        return swap(r);
    }
    // Tunshi Xingkong | Swallowed Star | Пожиратель звёзд | Поглощая звезду [ТВ-1] [2020(-2021)?,
    if let Some(r) = try3(title, r"^([^/\[\(]+) \| [^/\[\(]+ \| [^/\[\(]+ \| ([^/\[\(]+) (\[(ТВ|TV)-[0-9]+\] )?\[([0-9]{4})(-[0-9]{4})?,", 1, 2, 5) {
        return swap(r);
    }
    // Uzaki-chan wa Asobitai! | Uzaki-chan Wants to Hang Out! | Узаки хочет тусоваться! (Удзаки хочет погулять!) [ТВ-1] [2020(-2021)?,
    if let Some(r) = try3(title, r"^([^/\[\(]+) \| [^/\[\(]+ \| ([^/\[\(]+) \([^\)]+\) (\[(ТВ|TV)-[0-9]+\] )?\[([0-9]{4})(-[0-9]{4})?,", 1, 2, 5) {
        return swap(r);
    }
    // Kanojo, Okarishimasu | Rent-A-Girlfriend | Девушка на час [ТВ-1] [2020(-2021)?,
    if let Some(r) = try3(title, r"^([^/\[\(]+) \| [^/\[\(]+ \| ([^/\[\(]+) (\[(ТВ|TV)-[0-9]+\] )?\[([0-9]{4})(-[0-9]{4})?,", 1, 2, 5) {
        return swap(r);
    }
    // Hortensia Saga | Сага о гортензии [2021(-2021)?,
    if let Some(r) = try3(title, r"^([^/\[\(]+) \| ([^/\[\(]+) (\[(ТВ|TV)-[0-9]+\] )?\[([0-9]{4})(-[0-9]{4})?,", 1, 2, 5) {
        return swap(r);
    }
    // Shingeki no Kyojin: The Final Season / Attack on Titan Final Season / Атака титанов. Последний сезон [TV-4] [2020(-2021)?,
    if let Some(r) = try3(title, r"^([^/\[\(]+) / [^/\[\(]+ / ([^/\[\(]+) (\[(ТВ|TV)-[0-9]+\] )?\[([0-9]{4})(-[0-9]{4})?,", 1, 2, 5) {
        return swap(r);
    }
    // Shingeki no Kyojin: The Final Season / Атака титанов. Последний сезон [TV-4] [2020(-2021)?,
    if let Some(r) = try3(title, r"^([^/\[\(]+) / ([^/\[\(]+) (\[(ТВ|TV)-[0-9]+\] )?\[([0-9]{4})(-[0-9]{4})?,", 1, 2, 5) {
        return swap(r);
    }
    (String::new(), String::new(), 0)
}

fn parse_kids_title(title: &str) -> Names {
    // Академия монстров / Escuela de Miedo / Cranston Academy: Monster Zone (2020)
    let x = g(title, r"^([^/\(\|]+) / [^/\(\|]+ / ([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)");
    if nb(&x[1]) && nb(&x[2]) {
        return (x[1].clone(), x[2].clone(), year(&x[3]));
    }
    // Трансформеры: Война за Кибертрон / Transformers: War For Cybertron (2020)
    let x = g(title, r"^([^/\(\|]+) / ([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)");
    if nb(&x[1]) && nb(&x[2]) {
        return (x[1].clone(), x[2].clone(), year(&x[3]));
    }
    // Спина к спине (2020-2021)
    let x = g(title, r"^([^/\(\|]+) \(([0-9]{4})(-[0-9]{4})?\)");
    (x[1].clone(), String::new(), year(&x[2]))
}
