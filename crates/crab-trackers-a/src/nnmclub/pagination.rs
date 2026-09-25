//! The NNMClub portal only serves a limited page window (~500). Older offsets redirect to
//! the FAQ topic https://nnmclub.to/forum/viewtopic.php?t=1626984

use crab_core::models::TaskParse;

/// Max zero-based portal pages kept in the task map (pages 0..MAX_PORTAL_PAGES-1).
pub const MAX_PORTAL_PAGES: i32 = 500;

/// FAQ topic that explains the portal page limit.
pub const PORTAL_LIMIT_FAQ_TOPIC_ID: &str = "1626984";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageParseStatus {
    /// HTTP/HTML failure - retry later.
    TransientError,
    /// Redirected to the portal-limit FAQ - settle for today.
    PortalLimitFaq,
    /// Valid portal listing with zero torrents - settle for today.
    EmptyPortal,
    /// Parsed at least one torrent.
    OkWithTorrents,
}

/// The pager digit before «След.» is the 1-based last page; tasks cover `0..=digit`,
/// clamped to [`MAX_PORTAL_PAGES`].
pub fn clamp_task_page_count(maxpages_from_pager: i32) -> i32 {
    let inclusive = maxpages_from_pager.max(0) as i64 + 1;
    inclusive.min(MAX_PORTAL_PAGES as i64) as i32
}

pub fn is_portal_limit_faq(html: &str) -> bool {
    if html.is_empty() {
        return false;
    }
    if html.contains(&format!("t={PORTAL_LIMIT_FAQ_TOPIC_ID}")) || html.contains(&format!("viewtopic.php?t={PORTAL_LIMIT_FAQ_TOPIC_ID}")) {
        return true;
    }
    let lower = html.to_lowercase();
    if lower.contains("только 500 страниц") {
        return true;
    }
    lower.contains("500 страниц") && lower.contains("<title>")
}

pub fn looks_like_portal_listing(html: &str) -> bool {
    !html.is_empty() && html.contains("paginport")
}

/// Remove tasks with `page >= MAX_PORTAL_PAGES`; returns the removed count.
pub fn prune_tasks_beyond_portal_limit(tasks: &mut Vec<TaskParse>) -> i32 {
    let before = tasks.len();
    tasks.retain(|t| t.page < MAX_PORTAL_PAGES);
    (before - tasks.len()) as i32
}

pub fn classify_page(html: Option<&str>, torrent_count: usize) -> PageParseStatus {
    let Some(html) = html.filter(|h| h.contains("NNM-Club</title>")) else {
        return PageParseStatus::TransientError;
    };
    if is_portal_limit_faq(html) {
        return PageParseStatus::PortalLimitFaq;
    }
    if torrent_count > 0 {
        return PageParseStatus::OkWithTorrents;
    }
    if looks_like_portal_listing(html) {
        return PageParseStatus::EmptyPortal;
    }
    PageParseStatus::TransientError
}

pub fn should_settle_task(status: PageParseStatus) -> bool {
    matches!(status, PageParseStatus::OkWithTorrents | PageParseStatus::PortalLimitFaq | PageParseStatus::EmptyPortal)
}
