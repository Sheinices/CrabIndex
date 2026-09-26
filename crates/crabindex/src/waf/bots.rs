//! Bot detection: a compiled-in catalog of User-Agent signatures grouped by category, a
//! generic "other bots" heuristic, the per-bot statistics and admin rule validation.
//!
//! Signatures are case-insensitive substrings checked in catalog order (the first hit wins, so
//! more specific entries come first). Legit sync clients (CrabIndex, Jackett, Prowlarr, media
//! apps) are deliberately not in the catalog, and nothing is blocked unless the admin enables a
//! category or adds a rule (see `botBlockCategories` / `botBlocked` in `Data/waf.json`).

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use super::store::iso;

/// Bot category (`id` is what the API and `Data/waf.json` use).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Search,
    Ai,
    Seo,
    Social,
    Monitoring,
    Scanners,
    Libraries,
    Empty,
    Other,
}

const CATEGORIES: usize = 9;

impl Category {
    pub const ALL: [Category; CATEGORIES] = [
        Category::Search,
        Category::Ai,
        Category::Seo,
        Category::Social,
        Category::Monitoring,
        Category::Scanners,
        Category::Libraries,
        Category::Empty,
        Category::Other,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Category::Search => "search",
            Category::Ai => "ai",
            Category::Seo => "seo",
            Category::Social => "social",
            Category::Monitoring => "monitoring",
            Category::Scanners => "scanners",
            Category::Libraries => "libraries",
            Category::Empty => "empty",
            Category::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Search => "Поисковые роботы",
            Category::Ai => "AI-краулеры",
            Category::Seo => "SEO-сервисы",
            Category::Social => "Соцсети и мессенджеры",
            Category::Monitoring => "Мониторинг доступности",
            Category::Scanners => "Сканеры уязвимостей",
            Category::Libraries => "HTTP-библиотеки и утилиты",
            Category::Empty => "Пустой User-Agent",
            Category::Other => "Прочие боты",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Category::Search => "Индексируют сайты для поисковых систем (Google, Яндекс, Bing и другие)",
            Category::Ai => "Собирают данные для обучения и ответов нейросетей",
            Category::Seo => "Анализ ссылок и позиций сайтов (Ahrefs, Semrush и другие)",
            Category::Social => "Строят превью ссылок в соцсетях и мессенджерах",
            Category::Monitoring => "Проверяют, что сервер отвечает (UptimeRobot, Pingdom и другие)",
            Category::Scanners => "Массовые сканеры интернета и поиск уязвимостей",
            Category::Libraries => "Скрипты и утилиты без собственного имени (curl, python-requests, Go и другие)",
            Category::Empty => "Запросы без заголовка User-Agent",
            Category::Other => "User-Agent со словами bot, crawler, spider, slurp, которых нет в каталоге",
        }
    }

    pub fn parse(s: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.id().eq_ignore_ascii_case(s.trim()))
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// `(category, display name, lowercase substring)` in match order: specific entries first.
pub const CATALOG: &[(Category, &str, &str)] = &[
    // search
    (Category::Search, "Googlebot", "googlebot"),
    (Category::Search, "bingbot", "bingbot"),
    (Category::Search, "YandexImages", "yandeximages"),
    (Category::Search, "YandexBot", "yandexbot"),
    (Category::Search, "Baiduspider", "baiduspider"),
    (Category::Search, "DuckDuckBot", "duckduckbot"),
    (Category::Search, "Applebot", "applebot"),
    (Category::Search, "Sogou", "sogou"),
    (Category::Search, "Exabot", "exabot"),
    (Category::Search, "SeznamBot", "seznambot"),
    (Category::Search, "PetalBot", "petalbot"),
    (Category::Search, "Yahoo Slurp", "yahoo! slurp"),
    // ai
    (Category::Ai, "GPTBot", "gptbot"),
    (Category::Ai, "ChatGPT-User", "chatgpt-user"),
    (Category::Ai, "OAI-SearchBot", "oai-searchbot"),
    (Category::Ai, "ClaudeBot", "claudebot"),
    (Category::Ai, "Claude-Web", "claude-web"),
    (Category::Ai, "anthropic-ai", "anthropic-ai"),
    (Category::Ai, "CCBot", "ccbot"),
    (Category::Ai, "Bytespider", "bytespider"),
    (Category::Ai, "PerplexityBot", "perplexitybot"),
    (Category::Ai, "Google-Extended", "google-extended"),
    (Category::Ai, "Amazonbot", "amazonbot"),
    (Category::Ai, "cohere-ai", "cohere-ai"),
    (Category::Ai, "Diffbot", "diffbot"),
    (Category::Ai, "ImagesiftBot", "imagesiftbot"),
    (Category::Ai, "meta-externalagent", "meta-externalagent"),
    (Category::Ai, "FacebookBot", "facebookbot"),
    // seo
    (Category::Seo, "AhrefsBot", "ahrefsbot"),
    (Category::Seo, "SemrushBot", "semrushbot"),
    (Category::Seo, "MJ12bot", "mj12bot"),
    (Category::Seo, "DotBot", "dotbot"),
    (Category::Seo, "BLEXBot", "blexbot"),
    (Category::Seo, "DataForSeoBot", "dataforseobot"),
    (Category::Seo, "serpstatbot", "serpstatbot"),
    (Category::Seo, "Barkrowler", "barkrowler"),
    (Category::Seo, "SeekportBot", "seekportbot"),
    (Category::Seo, "MegaIndex", "megaindex"),
    (Category::Seo, "Screaming Frog", "screaming frog"),
    (Category::Seo, "rogerbot", "rogerbot"),
    // social
    (Category::Social, "facebookexternalhit", "facebookexternalhit"),
    (Category::Social, "TelegramBot", "telegrambot"),
    (Category::Social, "Twitterbot", "twitterbot"),
    (Category::Social, "WhatsApp", "whatsapp"),
    (Category::Social, "Discordbot", "discordbot"),
    (Category::Social, "Slackbot", "slackbot"),
    (Category::Social, "LinkedInBot", "linkedinbot"),
    (Category::Social, "vkShare", "vkshare"),
    (Category::Social, "SkypeUriPreview", "skypeuripreview"),
    // monitoring
    (Category::Monitoring, "UptimeRobot", "uptimerobot"),
    (Category::Monitoring, "Pingdom", "pingdom"),
    (Category::Monitoring, "StatusCake", "statuscake"),
    (Category::Monitoring, "Better Uptime", "better uptime"),
    (Category::Monitoring, "Better Uptime", "betteruptime"),
    (Category::Monitoring, "Site24x7", "site24x7"),
    (Category::Monitoring, "Datadog", "datadog"),
    (Category::Monitoring, "HetrixTools", "hetrixtools"),
    // libraries that could be mistaken for scanners below
    (Category::Libraries, "python-httpx", "python-httpx"),
    // scanners
    (Category::Scanners, "zgrab", "zgrab"),
    (Category::Scanners, "masscan", "masscan"),
    (Category::Scanners, "Nmap", "nmap"),
    (Category::Scanners, "Nuclei", "nuclei"),
    (Category::Scanners, "sqlmap", "sqlmap"),
    (Category::Scanners, "Nikto", "nikto"),
    (Category::Scanners, "CensysInspect", "censysinspect"),
    (Category::Scanners, "Censys", "censys"),
    (Category::Scanners, "Expanse", "expanse"),
    (Category::Scanners, "Palo Alto Networks", "palo alto networks"),
    (Category::Scanners, "InternetMeasurement", "internetmeasurement"),
    (Category::Scanners, "ModatScanner", "modatscanner"),
    (Category::Scanners, "l9explore", "l9explore"),
    (Category::Scanners, "httpx", "httpx"),
    (Category::Scanners, "fasthttp", "fasthttp"),
    (Category::Scanners, "WPScan", "wpscan"),
    // libraries and command-line tools
    (Category::Libraries, "python-requests", "python-requests"),
    (Category::Libraries, "python-urllib", "python-urllib"),
    (Category::Libraries, "aiohttp", "aiohttp"),
    (Category::Libraries, "Go-http-client", "go-http-client"),
    (Category::Libraries, "curl", "curl/"),
    (Category::Libraries, "Wget", "wget/"),
    (Category::Libraries, "okhttp", "okhttp"),
    (Category::Libraries, "Apache-HttpClient", "apache-httpclient"),
    (Category::Libraries, "Java", "java/"),
    (Category::Libraries, "node-fetch", "node-fetch"),
    (Category::Libraries, "axios", "axios/"),
    (Category::Libraries, "libwww-perl", "libwww-perl"),
    (Category::Libraries, "PHP", "php/"),
    (Category::Libraries, "Scrapy", "scrapy"),
    (Category::Libraries, "HeadlessChrome", "headlesschrome"),
];

/// Name recorded for requests without a User-Agent.
pub const EMPTY_NAME: &str = "(пустой)";
/// Name for a heuristic hit without a usable product token.
const UNKNOWN_NAME: &str = "unknown-bot";
const HEURISTIC: [&str; 4] = ["bot", "crawl", "spider", "slurp"];
/// Tokens that contain a heuristic word but are not bots (device models and the like).
const NOT_BOTS: [&str; 2] = ["cubot", "robotics"];
const MAX_NAME_LEN: usize = 64;

/// A classified User-Agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BotHit {
    pub category: Category,
    pub name: String,
}

/// Catalog names of `c` (deduplicated, catalog order).
pub fn catalog_names(c: Category) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (cat, name, _) in CATALOG {
        if *cat == c && !out.contains(name) {
            out.push(name);
        }
    }
    out
}

fn heuristic_word(s: &str) -> bool {
    HEURISTIC.iter().any(|w| s.contains(w))
}

/// Product name of the first token that looks like a bot (`FooBot/1.2` → `FooBot`).
fn heuristic_name(ua: &str) -> Option<String> {
    let lower = ua.to_ascii_lowercase();
    if !heuristic_word(&lower) {
        return None;
    }
    for token in ua.split(|c: char| c.is_whitespace() || ";(),+[]\"'".contains(c)) {
        let t = token.to_ascii_lowercase();
        if t.contains("://") || t.starts_with("http") || t.starts_with("www.") || t.contains('@') {
            continue;
        }
        let name = token.split('/').next().unwrap_or("").trim_matches(|c: char| !c.is_alphanumeric());
        let n = name.to_ascii_lowercase();
        if name.len() >= 3 && heuristic_word(&n) && !NOT_BOTS.iter().any(|x| n.contains(x)) {
            return Some(name.chars().take(MAX_NAME_LEN).collect());
        }
    }
    // The word only appears in a URL or an excluded token.
    let rest = NOT_BOTS.iter().fold(lower, |s, x| s.replace(x, ""));
    heuristic_word(&rest).then(|| UNKNOWN_NAME.to_string())
}

/// Classify a User-Agent: catalog signature, empty UA, or the generic heuristic.
pub fn classify(ua: &str) -> Option<BotHit> {
    let ua = ua.trim();
    if ua.is_empty() {
        return Some(BotHit { category: Category::Empty, name: EMPTY_NAME.into() });
    }
    let lower = ua.to_ascii_lowercase();
    if let Some((c, name, _)) = CATALOG.iter().find(|(_, _, sig)| lower.contains(sig)) {
        return Some(BotHit { category: *c, name: (*name).into() });
    }
    heuristic_name(ua).map(|name| BotHit { category: Category::Other, name })
}

/// Distinct User-Agents kept in the classification cache (cleared when full).
const CACHE_MAX: usize = 4096;
static CACHE: once_cell::sync::Lazy<DashMap<String, Option<BotHit>>> = once_cell::sync::Lazy::new(DashMap::new);

/// [`classify`] memoised by the (already truncated) User-Agent string.
pub fn classify_cached(ua: &str) -> Option<BotHit> {
    if let Some(v) = CACHE.get(ua) {
        return v.clone();
    }
    let hit = classify(ua);
    if CACHE.len() >= CACHE_MAX {
        CACHE.clear();
    }
    CACHE.insert(ua.to_string(), hit.clone());
    hit
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

pub const MIN_RULE_LEN: usize = 3;
pub const MAX_RULE_LEN: usize = 128;

/// Validate a `botBlocked` / `botAllowed` value: a catalog name or a UA substring of
/// 3-128 characters without control characters (kept as typed, matched case-insensitively).
pub fn parse_rule(input: &str) -> Result<String, String> {
    let s = input.trim();
    let n = s.chars().count();
    if n < MIN_RULE_LEN {
        return Err(format!("value: не короче {MIN_RULE_LEN} символов (имя бота или часть User-Agent)"));
    }
    if n > MAX_RULE_LEN {
        return Err(format!("value: не длиннее {MAX_RULE_LEN} символов"));
    }
    if s.chars().any(char::is_control) {
        return Err("value: недопустимые символы".into());
    }
    Ok(s.to_string())
}

/// A rule value matches a request: the classified bot name equals it, or the UA contains it
/// (both case-insensitive). `ua_lower` is the lowercased User-Agent.
pub fn rule_matches(value: &str, hit: Option<&BotHit>, ua_lower: &str) -> bool {
    let v = value.to_lowercase();
    !v.is_empty() && (hit.is_some_and(|h| h.name.to_lowercase() == v) || ua_lower.contains(&v))
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Tracked bots (least recently seen are evicted beyond this).
pub const MAX_BOTS: usize = 2_000;
const EVICT_SLACK: usize = 200;
/// Distinct IPs counted per bot (the count saturates here).
pub const MAX_BOT_IPS: usize = 1_000;
const MAX_BOT_PATHS: usize = 100;
const TOP_PATHS: usize = 5;
const MAX_SAMPLES: usize = 5;

#[derive(Clone, Debug)]
pub struct BotAgg {
    pub category: Category,
    pub name: String,
    pub requests: u64,
    pub blocked: u64,
    pub last_seen: DateTime<Utc>,
    pub ips: HashSet<String>,
    pub paths: HashMap<String, u64>,
    pub samples: Vec<String>,
}

impl BotAgg {
    pub fn top_paths(&self) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = self.paths.iter().map(|(p, n)| (p.clone(), *n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v.truncate(TOP_PATHS);
        v
    }
}

pub struct BotStats {
    bots: DashMap<(Category, String), BotAgg>,
    categories: Mutex<[(u64, u64); CATEGORIES]>,
    evicting: AtomicBool,
}

impl Default for BotStats {
    fn default() -> Self {
        BotStats { bots: DashMap::new(), categories: Mutex::new([(0, 0); CATEGORIES]), evicting: AtomicBool::new(false) }
    }
}

impl BotStats {
    pub fn reset(&self) {
        self.bots.clear();
        *self.categories.lock() = [(0, 0); CATEGORIES];
    }

    pub fn record(&self, hit: &BotHit, ip: &str, path: &str, ua: &str, blocked: bool, time: DateTime<Utc>) {
        {
            let mut c = self.categories.lock();
            let slot = &mut c[hit.category.index()];
            slot.0 += 1;
            slot.1 += blocked as u64;
        }
        {
            let mut a = self.bots.entry((hit.category, hit.name.clone())).or_insert_with(|| BotAgg {
                category: hit.category,
                name: hit.name.clone(),
                requests: 0,
                blocked: 0,
                last_seen: time,
                ips: HashSet::new(),
                paths: HashMap::new(),
                samples: Vec::new(),
            });
            a.requests += 1;
            a.blocked += blocked as u64;
            a.last_seen = a.last_seen.max(time);
            if a.ips.len() < MAX_BOT_IPS && !a.ips.contains(ip) {
                a.ips.insert(ip.to_string());
            }
            if let Some(n) = a.paths.get_mut(path) {
                *n += 1;
            } else if a.paths.len() < MAX_BOT_PATHS {
                a.paths.insert(path.to_string(), 1);
            }
            if !ua.is_empty() && a.samples.len() < MAX_SAMPLES && !a.samples.iter().any(|s| s == ua) {
                a.samples.push(ua.to_string());
            }
        }
        if self.bots.len() > MAX_BOTS + EVICT_SLACK {
            self.evict();
        }
    }

    fn evict(&self) {
        if self.evicting.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut v: Vec<(DateTime<Utc>, (Category, String))> = self.bots.iter().map(|r| (r.value().last_seen, r.key().clone())).collect();
        v.sort();
        let excess = v.len().saturating_sub(MAX_BOTS);
        for (_, k) in v.into_iter().take(excess) {
            self.bots.remove(&k);
        }
        self.evicting.store(false, Ordering::Release);
    }

    /// Per-category `(requests, blocked)` since the last reset.
    pub fn category_totals(&self) -> [(u64, u64); CATEGORIES] {
        *self.categories.lock()
    }

    /// All tracked bots, most requests first.
    pub fn aggregates(&self) -> Vec<BotAgg> {
        let mut v: Vec<BotAgg> = self.bots.iter().map(|r| r.value().clone()).collect();
        v.sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| b.last_seen.cmp(&a.last_seen)).then_with(|| a.name.cmp(&b.name)));
        v
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.bots.len()
    }
}

/// JSON row of one bot for `GET waf/bots` (without `status`).
pub fn bot_json(a: &BotAgg) -> Value {
    json!({
        "name": a.name,
        "category": a.category.id(),
        "requests": a.requests,
        "blocked": a.blocked,
        "lastSeen": iso(&a.last_seen),
        "ips": a.ips.len(),
        "topPaths": a.top_paths().into_iter().map(|(p, n)| json!({ "path": p, "requests": n })).collect::<Vec<_>>(),
        "samples": a.samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(ua: &str) -> Option<(&'static str, String)> {
        classify(ua).map(|h| (h.category.id(), h.name))
    }

    #[test]
    fn catalog_hits() {
        let cases = [
            ("Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)", "search", "Googlebot"),
            ("Mozilla/5.0 (compatible; YandexBot/3.0; +http://yandex.com/bots)", "search", "YandexBot"),
            ("Mozilla/5.0 (compatible; YandexImages/3.0; +http://yandex.com/bots)", "search", "YandexImages"),
            ("Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; GPTBot/1.2; +https://openai.com/gptbot)", "ai", "GPTBot"),
            ("Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; ClaudeBot/1.0; +claudebot@anthropic.com)", "ai", "ClaudeBot"),
            ("Mozilla/5.0 (compatible; AhrefsBot/7.0; +http://ahrefs.com/robot/)", "seo", "AhrefsBot"),
            ("Screaming Frog SEO Spider/19.0", "seo", "Screaming Frog"),
            ("TelegramBot (like TwitterBot)", "social", "TelegramBot"),
            ("facebookexternalhit/1.1 (+http://www.facebook.com/externalhit_uatext.php)", "social", "facebookexternalhit"),
            ("Mozilla/5.0+(compatible; UptimeRobot/2.0; http://www.uptimerobot.com/)", "monitoring", "UptimeRobot"),
            ("Better Uptime Bot Mozilla/5.0", "monitoring", "Better Uptime"),
            ("Mozilla/5.0 (compatible; CensysInspect/1.1; +https://about.censys.io/)", "scanners", "CensysInspect"),
            ("Expanse, a Palo Alto Networks company, searches across the global IPv4 space", "scanners", "Expanse"),
            ("Mozilla/5.0 zgrab/0.x", "scanners", "zgrab"),
            ("sqlmap/1.8.9#stable (https://sqlmap.org)", "scanners", "sqlmap"),
            ("python-httpx/0.27.0", "libraries", "python-httpx"),
            ("python-requests/2.31.0", "libraries", "python-requests"),
            ("curl/8.9.1", "libraries", "curl"),
            ("Go-http-client/1.1", "libraries", "Go-http-client"),
            ("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/120.0 Safari/537.36", "libraries", "HeadlessChrome"),
        ];
        for (ua, c, n) in cases {
            assert_eq!(cat(ua), Some((c, n.to_string())), "{ua}");
        }
    }

    #[test]
    fn heuristic_and_empty() {
        assert_eq!(cat(""), Some(("empty", EMPTY_NAME.to_string())));
        assert_eq!(cat("   "), Some(("empty", EMPTY_NAME.to_string())));
        assert_eq!(cat("Mozilla/5.0 (compatible; FooBot/1.2; +https://foo.example/bot)"), Some(("other", "FooBot".to_string())));
        assert_eq!(cat("my-crawler/0.1"), Some(("other", "my-crawler".to_string())));
        assert_eq!(cat("Mozilla/5.0 (compatible; +https://example.com/spider.html)"), Some(("other", UNKNOWN_NAME.to_string())));
        assert_eq!(cat("Mozilla/5.0 (Linux; Android 10; CUBOT_X30) AppleWebKit/537.36 Chrome/120.0 Mobile Safari/537.36"), None);
    }

    #[test]
    fn legit_clients_are_not_bots() {
        for ua in [
            "Jackett",
            "Jackett/0.22.1880",
            "Prowlarr/1.0",
            "Prowlarr/1.24.3.4754 (ubuntu 24.04)",
            "CrabIndex/1.0.3",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_6) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0 Safari/537.36",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/111.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Linux; Android 12; SHIELD Android TV) AppleWebKit/537.36 Lampa/2.3.1",
            "Kodi/21.0 (Windows NT 10.0.22631; Win64; x64) App_Bitness/64 Version/21.0-(21.0.0)-Git:20240406",
        ] {
            assert_eq!(classify(ua), None, "{ua}");
        }
    }

    #[test]
    fn cached_and_catalog_names() {
        assert_eq!(classify_cached("AhrefsBot/7.0"), classify("AhrefsBot/7.0"));
        assert_eq!(classify_cached("AhrefsBot/7.0").unwrap().name, "AhrefsBot");
        assert_eq!(catalog_names(Category::Monitoring).iter().filter(|n| **n == "Better Uptime").count(), 1);
        for c in Category::ALL {
            assert_eq!(Category::parse(c.id()), Some(c));
        }
        assert!(catalog_names(Category::Empty).is_empty());
        for (_, name, sig) in CATALOG {
            assert_eq!(*sig, sig.to_lowercase(), "{name}");
        }
    }

    #[test]
    fn rules() {
        assert!(parse_rule("ab").is_err());
        assert!(parse_rule(&"x".repeat(129)).is_err());
        assert!(parse_rule("a\u{7}bc").is_err());
        assert_eq!(parse_rule("  MyScraper ").as_deref(), Ok("MyScraper"));
        let hit = classify("Mozilla/5.0 (compatible; AhrefsBot/7.0)");
        let ua = "mozilla/5.0 (compatible; ahrefsbot/7.0)";
        assert!(rule_matches("ahrefsbot", hit.as_ref(), ua));
        assert!(rule_matches("compatible; Ahrefs", hit.as_ref(), ua));
        assert!(!rule_matches("SemrushBot", hit.as_ref(), ua));
        let empty = classify("");
        assert!(rule_matches(EMPTY_NAME, empty.as_ref(), ""));
    }

    #[test]
    fn stats_are_bounded() {
        let s = BotStats::default();
        let now = Utc::now();
        let hit = classify("AhrefsBot/7.0").unwrap();
        for i in 0..(MAX_BOT_IPS + 10) {
            s.record(&hit, &format!("ip{i}"), &format!("/p{}", i % 200), "AhrefsBot/7.0", i % 2 == 0, now);
        }
        let a = &s.aggregates()[0];
        assert_eq!(a.requests, (MAX_BOT_IPS + 10) as u64);
        assert_eq!(a.ips.len(), MAX_BOT_IPS);
        assert_eq!(a.paths.len(), MAX_BOT_PATHS);
        assert_eq!(a.top_paths().len(), TOP_PATHS);
        assert_eq!(a.samples, vec!["AhrefsBot/7.0".to_string()]);
        assert_eq!(s.category_totals()[Category::Seo.index()].1, ((MAX_BOT_IPS + 10) / 2) as u64);
        for i in 0..(MAX_BOTS + EVICT_SLACK + 5) {
            let h = BotHit { category: Category::Other, name: format!("b{i}") };
            s.record(&h, "1.1.1.1", "/", "", false, now + chrono::Duration::milliseconds(i as i64));
        }
        assert!(s.len() <= MAX_BOTS + EVICT_SLACK);
        s.reset();
        assert_eq!(s.len(), 0);
        assert_eq!(s.category_totals()[Category::Seo.index()], (0, 0));
    }
}
