//! Megapeer browse page parsing and the rate-limited browse fetch.

use std::sync::atomic::{AtomicUsize, Ordering};

use once_cell::sync::Lazy;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crab_core::conf;
use crab_core::models::{TaskParse, TorrentDetails};
use crab_core::net::{self, Req};
use crab_core::parsing::{bencode, parser_log, tparse};
use crab_core::rx;
use crab_core::trackers::{self, Cancelled};

use crate::common::{self, g, match_row_nbsp as m, name_before_brackets, nb, parse_int, year};

const BROWSE_PAGE_VALID_MARKER: &str = "id=\"logo\"";
pub const MAX_TASK_PAGES: i32 = 10;
const TOTAL_COUNT_RE: &str = r">Всего: ([0-9]+)";
const PARSE_DELAY_CYCLE_MS: [u64; 3] = [30_000, 60_000, 90_000];

static PARSE_DELAY_INDEX: AtomicUsize = AtomicUsize::new(0);
static BROWSE_LOCK: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(1));

/// Megapeer row: the listing has no magnet, only a download id for the .torrent.
#[derive(Clone, Debug, Default)]
pub struct MegapeerDetails {
    pub t: TorrentDetails,
    pub download_id: String,
}

impl AsRef<TorrentDetails> for MegapeerDetails {
    fn as_ref(&self) -> &TorrentDetails {
        &self.t
    }
}

impl AsMut<TorrentDetails> for MegapeerDetails {
    fn as_mut(&mut self) -> &mut TorrentDetails {
        &mut self.t
    }
}

impl std::ops::Deref for MegapeerDetails {
    type Target = TorrentDetails;
    fn deref(&self) -> &TorrentDetails {
        &self.t
    }
}

/// Real browse chrome, not a CF interstitial / empty fetch.
pub fn looks_like_browse_page(html: &str) -> bool {
    !html.is_empty() && html.contains(BROWSE_PAGE_VALID_MARKER)
}

/// 0-based last browse index from «Всего: N» / 50, capped at [`MAX_TASK_PAGES`].
pub fn last_page_from_html(html: &str) -> i32 {
    if html.trim().is_empty() {
        return 0;
    }
    let Some(total) = rx::captures(html, TOTAL_COUNT_RE).and_then(|c| c.get(1).and_then(|x| x.as_str().parse::<i32>().ok())) else {
        return 0;
    };
    if total < 0 {
        return 0;
    }
    (total / 50).min(MAX_TASK_PAGES)
}

/// Drop map slots past the live 0-based last index (inclusive, capped).
pub fn prune_pages_beyond_max(tasks: &mut Vec<TaskParse>, max_page: i32) -> i32 {
    common::prune_pages_beyond_max(tasks, max_page)
}

fn next_parse_delay_ms() -> u64 {
    let i = PARSE_DELAY_INDEX.fetch_add(1, Ordering::SeqCst);
    PARSE_DELAY_CYCLE_MS[i % PARSE_DELAY_CYCLE_MS.len()]
}

/// Fetch a browse page with the 30/60/90s delay cycle and up to 3 attempts.
/// Only one browse runs at a time; a concurrent call returns `Ok(None)` immediately.
pub async fn get_megapeer_browse_page(url: &str, cat: &str, ct: &CancellationToken) -> Result<Option<String>, Cancelled> {
    let Ok(_permit) = BROWSE_LOCK.try_acquire() else {
        parser_log::write("megapeer", "GetMegapeerBrowsePage skipped: browse already in progress");
        return Ok(None);
    };
    let c = conf();
    let req = Req::new()
        .cp1251()
        .useproxy(c.Megapeer.useproxy)
        .header("dnt", "1")
        .header("pragma", "no-cache")
        .referer(format!("{}/cat/{cat}", c.Megapeer.rq_host()))
        .header("sec-fetch-dest", "document")
        .header("sec-fetch-mode", "navigate")
        .header("sec-fetch-site", "same-origin")
        .header("sec-fetch-user", "?1")
        .header("upgrade-insecure-requests", "1")
        .cancel(ct);

    const MAX_RETRIES: i32 = 3;
    for attempt in 1..=MAX_RETRIES {
        trackers::check(ct)?;
        trackers::sleep(next_parse_delay_ms(), ct).await?;

        let (content, response) = net::http::base_get(url, &req).await;
        trackers::check(ct)?;
        if let Some(content) = content.filter(|s| s.contains(BROWSE_PAGE_VALID_MARKER)) {
            return Ok(Some(content));
        }
        if attempt < MAX_RETRIES {
            parser_log::write(
                "megapeer",
                format!(
                    "Rate limit or invalid page (status={}), retry {attempt}/{MAX_RETRIES} after next cycle delay (30/60/90s)",
                    response.status
                ),
            );
            continue;
        }
        return Ok(None);
    }
    Ok(None)
}

/// Fetch + parse one browse page; rows without a cached identical title get a magnet from
/// their .torrent download. `Ok(false)` when the page could not be fetched.
pub async fn parse_page(cat: String, page: i32, ct: CancellationToken) -> Result<bool, Cancelled> {
    let url = format!("{}/browse.php?cat={cat}&page={page}", conf().Megapeer.rq_host());
    let html = get_megapeer_browse_page(&url, &cat, &ct).await?;
    let Some(html) = html.filter(|h| looks_like_browse_page(h)) else { return Ok(false) };

    let torrents = parse_torrents_from_page(&html, &cat);
    common::add_or_update_async(torrents, |mut t, cached| async move {
        if let Some(c) = cached {
            if c.title == t.t.title {
                return Some(t);
            }
        }
        let host = conf().Megapeer.host.clone();
        let data = net::download(&format!("{host}/download/{}", t.download_id), &Req::new().referer(host.clone()).timeout(30)).await?;
        let magnet = bencode::magnet(&data)?;
        if magnet.trim().is_empty() {
            return None;
        }
        t.t.magnet = magnet;
        Some(t)
    })
    .await;
    Ok(true)
}

pub fn parse_torrents_from_page(html: &str, cat: &str) -> Vec<MegapeerDetails> {
    let mut torrents = Vec::new();

    for row in html.split("class=\"table_fon\"").skip(1) {
        let Some(create_time) = tparse::parse_create_time(&m(row, "<td>([0-9]+ [^ ]+ [0-9]+)</td>\\s*<td[^>]*>", 1), "dd.MM.yy") else {
            continue;
        };

        let url = m(row, "href=\"/(torrent/[0-9]+)", 1);
        let mut title = m(row, "class=\"url\"[^>]*>([^<]+)</a>", 1);
        if !nb(&title) {
            title = m(row, "class=\"url\">([^<]+)</a></td>", 1);
        }
        let size_name = m(row, "<td align=\"right\">([^<\n\r]+)", 1).trim().to_string();
        if !nb(&title) {
            continue;
        }

        let mut sid = m(row, "alt=\"S\"><font [^>]+>([0-9]+)</font>", 1);
        if !nb(&sid) {
            sid = m(row, "alt=\"S\"[^>]*>\\s*([0-9]+)", 1);
        }
        let mut pir = m(row, "alt=\"L\"><font [^>]+>([0-9]+)</font>", 1);
        if !nb(&pir) {
            pir = m(row, "alt=\"L\"[^>]*>\\s*([0-9]+)", 1);
        }

        let url = format!("{}/{}", conf().Megapeer.host, url);
        let (mut name, originalname, relased) = parse_title(cat, &title);
        if !nb(&name) {
            name = name_before_brackets(&title);
        }
        if !nb(&name) {
            continue;
        }

        let types: &[&str] = match cat {
            "80" | "79" => &["movie"],
            "6" | "5" => &["serial"],
            "55" => &["docuserial", "documovie"],
            "57" => &["tvshow"],
            "76" => &["multfilm", "multserial"],
            _ => &[],
        };

        let download_id = m(row, "href=\"/?download/([0-9]+)\"", 1);
        if !nb(&download_id) {
            continue;
        }

        let mut t = TorrentDetails::new("megapeer", types, url, title);
        t.sid = parse_int(&sid);
        t.pir = parse_int(&pir);
        t.sizeName = size_name;
        t.createTime = create_time;
        t.name = name;
        t.originalname = originalname;
        t.relased = relased;
        torrents.push(MegapeerDetails { t, download_id });
    }
    torrents
}

fn three_ok(x: &[String], a: usize, b: usize, c: usize) -> bool {
    nb(&x[a]) && nb(&x[b]) && nb(&x[c])
}

fn parse_title(cat: &str, title: &str) -> (String, String, i32) {
    match cat {
        "80" => {
            let x = g(title, r"^([^/]+) / ([^/]+) / ([^/\(]+) \(([0-9]{4})\)");
            if three_ok(&x, 1, 2, 3) {
                return (x[1].clone(), x[3].clone(), year(&x[4]));
            }
            let x = g(title, r"^([^/\(]+) / ([^/\(]+) \(([0-9]{4})\)");
            (x[1].clone(), x[2].clone(), year(&x[3]))
        }
        "79" => {
            let x = g(title, r"^([^/\(]+) \(([0-9]{4})\)");
            (x[1].clone(), String::new(), year(&x[2]))
        }
        "6" => {
            let x = g(title, r"^([^/]+) / [^/]+ / [^/]+ / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
            if three_ok(&x, 1, 2, 3) {
                return (x[1].clone(), x[2].clone(), year(&x[3]));
            }
            let x = g(title, r"^([^/]+) / [^/]+ / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
            if three_ok(&x, 1, 2, 3) {
                return (x[1].clone(), x[2].clone(), year(&x[3]));
            }
            let x = g(title, r"^([^/]+) / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
            (x[1].clone(), x[2].clone(), year(&x[3]))
        }
        "5" => {
            let x = g(title, r"^([^/]+) \[[^\]]+\] \(([0-9]{4})(\)|-)");
            (x[1].clone(), String::new(), year(&x[2]))
        }
        "55" | "57" | "76" => {
            let brackets = title.contains('[') && title.contains(']');
            if title.contains(" / ") {
                if brackets {
                    let x = g(title, r"^([^/]+) / ([^/]+) / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
                    if three_ok(&x, 1, 2, 3) {
                        return (x[1].clone(), x[3].clone(), year(&x[4]));
                    }
                    let x = g(title, r"^([^/]+) / ([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
                    (x[1].clone(), x[2].clone(), year(&x[3]))
                } else {
                    let x = g(title, r"^([^/]+) / ([^/]+) / ([^/\(]+) \(([0-9]{4})\)");
                    if three_ok(&x, 1, 2, 3) {
                        return (x[1].clone(), x[3].clone(), year(&x[4]));
                    }
                    let x = g(title, r"^([^/\(]+) / ([^/\(]+) \(([0-9]{4})\)");
                    (x[1].clone(), x[2].clone(), year(&x[3]))
                }
            } else if brackets {
                let x = g(title, r"^([^/\[]+) \[[^\]]+\] +\(([0-9]{4})(\)|-)");
                (x[1].clone(), String::new(), year(&x[2]))
            } else {
                let x = g(title, r"^([^/\(]+) \(([0-9]{4})\)");
                (x[1].clone(), String::new(), year(&x[2]))
            }
        }
        _ => (String::new(), String::new(), 0),
    }
}
