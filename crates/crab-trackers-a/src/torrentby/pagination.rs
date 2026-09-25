//! torrent.by listing pager: 0-based `?page=`, chips in `circle_page` spans,
//! the last chip is often `...` (not the true last page).

use std::future::Future;

use tokio_util::sync::CancellationToken;

use crab_core::models::TaskParse;
use crab_core::rx;
use crab_core::trackers::{self, Cancelled};

pub const MAX_ELLIPSIS_HOPS: i32 = 40;

/// Old pager regex; does not match current HTML (kept for regression tests).
pub const LEGACY_MAX_PAGE_REGEX: &str = "href=\"\\?page=([0-9]+)\">[0-9]+</a>([\\t ]+)?</center></td>";

const PAGER_BLOCK_RE: &str = r"(?s)Страницы:(.*?)</center>";
const PAGE_HREF_RE: &str = r#"href="\?page=([0-9]+)""#;
const CIRCLE_CHIP_RE: &str = r#"<span class="circle_page"[^>]*>([^<]*)</span>"#;
const CURRENT_CHIP_RE: &str = r#"<span class="circle_page"[^>]*background[^>]*>([0-9]+)</span>"#;
const PAGE_LINK_RE: &str = r#"<a[^>]*href="\?page=([0-9]+)"[^>]*>\s*<span class="circle_page"[^>]*>([^<]*)</span>"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TorrentByPager {
    pub current_display: i32,
    pub max_page_index: i32,
    pub has_trailing_ellipsis: bool,
    pub ellipsis_jump_page: Option<i32>,
}

impl TorrentByPager {
    fn empty() -> Self {
        TorrentByPager { current_display: 1, max_page_index: 0, has_trailing_ellipsis: false, ellipsis_jump_page: None }
    }
}

pub fn parse_pager(html: &str) -> TorrentByPager {
    if html.trim().is_empty() {
        return TorrentByPager::empty();
    }
    let block = rx::group_i(html, PAGER_BLOCK_RE, 1);
    if block.trim().is_empty() {
        return TorrentByPager::empty();
    }

    let mut max_href = 0;
    let mut any_href = false;
    for m in rx::all_groups_i(&block, PAGE_HREF_RE) {
        let Ok(n) = m[1].parse::<i32>() else { continue };
        any_href = true;
        if n > max_href {
            max_href = n;
        }
    }

    let mut current_display = 1;
    let current = rx::groups_i(&block, CURRENT_CHIP_RE);
    if let Ok(cur) = current[1].parse::<i32>() {
        current_display = cur;
    } else if let Some(first) = rx::captures_i(&block, CIRCLE_CHIP_RE) {
        if let Some(n) = first.get(1).and_then(|x| x.as_str().trim().parse::<i32>().ok()) {
            current_display = n;
        }
    }

    let chips = rx::all_groups_i(&block, CIRCLE_CHIP_RE);
    let last_chip = chips.last().map(|c| c[1].trim().to_string()).unwrap_or_default();
    let ellipsis = last_chip == "..." || last_chip == "…";

    let mut jump = None;
    if ellipsis {
        let last_link = rx::all_groups_i(&block, PAGE_LINK_RE).into_iter().last();
        if let Some(j) = last_link.and_then(|l| l[1].parse::<i32>().ok()) {
            jump = Some(j);
        } else if any_href {
            jump = Some(max_href);
        }
    }

    let current_index = (current_display - 1).max(0);
    let max_page_index = current_index.max(if any_href { max_href } else { 0 });

    TorrentByPager { current_display, max_page_index, has_trailing_ellipsis: ellipsis, ellipsis_jump_page: jump }
}

/// Drop map slots past the discovered 0-based last index (inclusive `page <= max_page`).
pub fn prune_pages_beyond_max(tasks: &mut Vec<TaskParse>, max_page: i32) -> i32 {
    crate::common::prune_pages_beyond_max(tasks, max_page)
}

/// Pager chips can run past empty pages: binary search for the last page with rows.
/// Probe result: `Some(true)` = rows, `Some(false)` = empty listing, `None` = failed fetch
/// (keep `claimed_last`, never prune on a bad GET).
pub async fn shrink_to_last_non_empty<F, Fut>(claimed_last: i32, mut page_has_rows: F, ct: &CancellationToken) -> Result<i32, Cancelled>
where
    F: FnMut(i32) -> Fut,
    Fut: Future<Output = Option<bool>>,
{
    if claimed_last <= 0 {
        return Ok(claimed_last.max(0));
    }
    let (mut lo, mut hi) = (0, claimed_last);
    while lo < hi {
        trackers::check(ct)?;
        let mid = lo + (hi - lo + 1) / 2;
        match page_has_rows(mid).await {
            None => return Ok(claimed_last),
            Some(true) => lo = mid,
            Some(false) => hi = mid - 1,
        }
    }
    Ok(lo)
}
