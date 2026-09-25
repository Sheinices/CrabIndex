//! Application configuration: strongly typed mirror of `init.yaml` / `init.conf`
//! (models, loader and hot reload).
//!
//! Field names match the YAML keys (`conf().Rutor.host`, `conf().evercache.enable`).
//!
//! Loading "populates" the defaults: the user document is deep-merged
//! over the defaults (objects merge key-by-key, case-insensitively; scalars/arrays
//! replace; `null` keeps the default), and string scalars are coerced to the
//! numeric/bool type of the default where needed (YAML → JSON quirk of the original).

use arc_swap::ArcSwap;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LoginSettings {
    pub u: Option<String>,
    pub p: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackerSettings {
    pub host: String,
    pub alias: Option<String>,
    pub cookie: Option<String>,
    /// When true and logParsers is enabled, parser writes to Data/log/{tracker}.log
    pub log: bool,
    pub useproxy: bool,
    pub reqMinute: i32,
    /// Rutracker topic fetch attempts under FlareSolverr (<=0 → 1 attempt).
    pub topicFetchAttempts: i32,
    pub login: LoginSettings,
    /// Computed from reqMinute; serialized for the config API, ignored on load.
    #[serde(skip_deserializing)]
    pub parseDelay: i32,
}

impl Default for TrackerSettings {
    fn default() -> Self {
        TrackerSettings::new("", 8)
    }
}

impl TrackerSettings {
    pub fn new(host: &str, req_minute: i32) -> Self {
        let mut t = TrackerSettings {
            host: host.to_string(),
            alias: None,
            cookie: None,
            log: true,
            useproxy: false,
            reqMinute: req_minute,
            topicFetchAttempts: 5,
            login: LoginSettings::default(),
            parseDelay: 0,
        };
        t.parseDelay = t.parse_delay();
        t
    }

    /// Delay between requests in ms derived from `reqMinute`.
    pub fn parse_delay(&self) -> i32 {
        if self.reqMinute == -1 {
            return 10;
        }
        if self.reqMinute >= 60 {
            return 1000;
        }
        if self.reqMinute <= 0 {
            return 60_000;
        }
        (60 / self.reqMinute) * 1000
    }

    /// Request host: alias when set, else host (`rqHost()`).
    pub fn rq_host(&self) -> String {
        match self.alias.as_deref() {
            Some(a) if !a.trim().is_empty() => a.to_string(),
            _ => self.host.clone(),
        }
    }

    /// Rewrite `uri` from host to alias when alias is set (`rqHost(uri)`).
    pub fn rq_host_uri(&self, uri: &str) -> String {
        match self.alias.as_deref() {
            Some(a) if !a.trim().is_empty() => uri.replace(&self.host, a),
            _ => uri.to_string(),
        }
    }

    pub fn cookie_opt(&self) -> Option<&str> {
        self.cookie.as_deref().filter(|c| !c.trim().is_empty())
    }

    pub fn login_u(&self) -> &str {
        self.login.u.as_deref().unwrap_or("")
    }

    pub fn login_p(&self) -> &str {
        self.login.p.as_deref().unwrap_or("")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Evercache {
    pub enable: bool,
    pub validHour: i32,
    pub maxOpenWriteTask: i32,
    pub dropCacheTake: i32,
}

impl Default for Evercache {
    fn default() -> Self {
        Evercache { enable: true, validHour: 1, maxOpenWriteTask: 2000, dropCacheTake: 200 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FlareSolverrSettings {
    pub enable: bool,
    pub url: String,
    pub crawlUrl: String,
    pub maxTimeoutMs: i32,
    pub sessionIdleMinutes: i32,
    pub browserTimeoutRetries: i32,
    pub recycleAfterTimeouts: i32,
    pub guardedHours: i32,
    pub recheckMinutes: i32,
}

impl Default for FlareSolverrSettings {
    fn default() -> Self {
        FlareSolverrSettings {
            enable: true,
            url: "http://127.0.0.1:8191/v1".into(),
            crawlUrl: String::new(),
            maxTimeoutMs: 300_000,
            sessionIdleMinutes: 120,
            browserTimeoutRetries: 1,
            recycleAfterTimeouts: 3,
            guardedHours: 6,
            recheckMinutes: 30,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CfFetchSettings {
    pub enable: bool,
    pub url: String,
    pub impersonate: String,
    pub timeoutSeconds: i32,
    pub maxConcurrent: i32,
    pub clearanceMinutes: i32,
    pub proxy: String,
}

impl Default for CfFetchSettings {
    fn default() -> Self {
        CfFetchSettings {
            enable: true,
            url: "http://127.0.0.1:8192/fetch".into(),
            impersonate: "chrome136".into(),
            timeoutSeconds: 25,
            maxConcurrent: 4,
            clearanceMinutes: 60,
            proxy: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProxySettings {
    pub pattern: Option<String>,
    pub useAuth: bool,
    pub BypassOnLocal: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub list: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    /// v1 fuzzy merge: false | auto | true.
    pub mergeV1: String,
    pub maxV1Pairs: i32,
    pub v1Sort: String,
    pub stripTrailingYear: bool,
    pub stripSeasonEpisode: bool,
    pub skipSeasonEpisodeFilter: bool,
    pub skipCatFilter: bool,
}

impl Default for SearchSettings {
    fn default() -> Self {
        SearchSettings {
            mergeV1: "auto".into(),
            maxV1Pairs: 4,
            v1Sort: "sid".into(),
            stripTrailingYear: true,
            stripSeasonEpisode: true,
            skipSeasonEpisodeFilter: false,
            skipCatFilter: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AllohaSettings {
    pub enable: bool,
    pub baseUrl: String,
    pub token: String,
    pub timeoutSeconds: i32,
    pub cacheHours: i32,
    pub filterByYear: bool,
}

impl Default for AllohaSettings {
    fn default() -> Self {
        AllohaSettings {
            enable: true,
            baseUrl: "https://apbugall.org".into(),
            token: "04941a9a3ca3ac16e2b4327347bbc1".into(),
            timeoutSeconds: 8,
            cacheHours: 24,
            filterByYear: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TorznabSettings {
    pub enable: bool,
    pub enrichTitles: bool,
}

impl Default for TorznabSettings {
    fn default() -> Self {
        TorznabSettings { enable: true, enrichTitles: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingOptions {
    pub defaultLevel: String,
    pub consoleTimestamp: bool,
    pub categories: Option<IndexMap<String, String>>,
    pub tracksConsoleDetail: bool,
    pub cronSkipFastMs: i32,
}

impl Default for LoggingOptions {
    fn default() -> Self {
        LoggingOptions {
            defaultLevel: "Information".into(),
            consoleTimestamp: false,
            categories: None,
            tracksConsoleDetail: false,
            cronSkipFastMs: 100,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TracksIntervalConfig {
    pub task0: i32,
    pub task1: i32,
}

impl Default for TracksIntervalConfig {
    fn default() -> Self {
        TracksIntervalConfig { task0: 180, task1: 60 }
    }
}

/// Admin panel (`admin:` block): URL prefix, entry token and session lifetime.
/// The login password is the root `devkey`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AdminSettings {
    pub enable: bool,
    /// URL prefix, one segment `[a-z0-9_-]{2,32}` (see [`normalize_admin_path`]).
    pub path: String,
    /// Entry token required in `{path}?{token}` (`[A-Za-z0-9]`, generated on first start).
    pub token: String,
    pub sessionHours: i32,
}

impl Default for AdminSettings {
    fn default() -> Self {
        AdminSettings { enable: true, path: DEFAULT_ADMIN_PATH.into(), token: String::new(), sessionHours: 12 }
    }
}

/// WAF rate limit (`waf.rateLimit`): requests per client IP per rolling minute.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WafRateLimit {
    pub enable: bool,
    pub perMinute: i32,
    /// Automatic temporary ban when the limit is exceeded.
    pub banMinutes: i32,
}

impl Default for WafRateLimit {
    fn default() -> Self {
        WafRateLimit { enable: true, perMinute: 300, banMinutes: 15 }
    }
}

/// Web application firewall (`waf:` block). Dynamic lists and bans live in `Data/waf.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WafSettings {
    pub enable: bool,
    /// Keep the in-memory request log and statistics.
    pub logRequests: bool,
    /// Recent requests kept in memory (ring buffer).
    pub historySize: i32,
    pub rateLimit: WafRateLimit,
    /// Scanner bait path prefixes (case-insensitive): a hit bans the IP for `trapBanMinutes`.
    pub trapPaths: Vec<String>,
    pub trapBanMinutes: i32,
    /// Case-insensitive regexes; a matching User-Agent gets 403 and a `rateLimit.banMinutes` ban.
    pub blockUserAgents: Vec<String>,
    /// LAN/loopback clients are never rate-limited or banned.
    pub whitelistLan: bool,
}

impl Default for WafSettings {
    fn default() -> Self {
        WafSettings {
            enable: true,
            logRequests: true,
            historySize: 5000,
            rateLimit: WafRateLimit::default(),
            trapPaths: ["/.env", "/wp-admin", "/wp-login.php", "/.git/", "/phpmyadmin"].iter().map(|s| s.to_string()).collect(),
            trapBanMinutes: 1440,
            blockUserAgents: vec![],
            whitelistLan: true,
        }
    }
}

pub const DEFAULT_ADMIN_PATH: &str = "/admin";

/// Top-level paths the admin panel must not shadow.
pub const ADMIN_RESERVED_PATHS: [&str; 20] = [
    "/", "/api", "/cron", "/dev", "/docs", "/swagger", "/sync", "/stats", "/torznab", "/health", "/version",
    "/lastupdatedb", "/jsondb", "/img", "/assets", "/openapi.yaml", "/opensearch.xml", "/sw.js",
    "/manifest.webmanifest", "/search",
];

/// Validate an admin URL prefix and return it in canonical form (`/name`).
/// Accepts `name`, `/name` or `/name/`; the segment must match `[a-z0-9_-]{2,32}`
/// and must not be one of [`ADMIN_RESERVED_PATHS`].
pub fn normalize_admin_path(raw: &str) -> Result<String, String> {
    let t = raw.trim();
    let seg = t.strip_prefix('/').unwrap_or(t);
    let seg = seg.strip_suffix('/').unwrap_or(seg);
    let canonical = format!("/{seg}");
    if ADMIN_RESERVED_PATHS.iter().any(|r| r.eq_ignore_ascii_case(&canonical)) {
        return Err(format!("admin.path: путь «{canonical}» зарезервирован"));
    }
    let ok_len = (2..=32).contains(&seg.len());
    let ok_chars = seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if !ok_len || !ok_chars {
        return Err("admin.path: один сегмент из [a-z0-9_-], длина 2-32 (например /admin)".into());
    }
    Ok(canonical)
}

/// Entry token format: `[A-Za-z0-9]{12,64}` (generated tokens are 18 characters).
pub fn is_valid_admin_token(token: &str) -> bool {
    (12..=64).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Root config object.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppOptions {
    pub listenip: String,
    pub listenport: u16,
    pub apikey: Option<String>,
    /// Admin panel password; also the access key for /dev/, /cron/ and /jsondb from outside the LAN.
    pub devkey: Option<String>,
    pub mergeduplicates: bool,
    pub mergenumduplicates: bool,
    pub openstats: bool,
    pub opensync: bool,
    pub tracks: bool,
    pub web: bool,
    /// 0 - all, 1 - day/month
    pub tracksmod: i32,
    pub tracksdelay: i32,
    pub trackslog: bool,
    pub tracksatempt: i32,
    pub tracksconcurrency: i32,
    pub tracksffptimeout: i32,
    pub tracksffptimeoutnosid: i32,
    pub tracksreadtimeout: i32,
    pub trackspeerwaittimeout: i32,
    pub tracksffpretry: i32,
    pub tracksminbufferkb: i32,
    pub tracksorphansweepmin: i32,
    pub trackscategory: String,
    #[serde(rename = "tracksinterval")]
    pub TracksInterval: TracksIntervalConfig,
    pub tsuri: Vec<String>,

    pub logFdb: bool,
    pub logFdbRetentionDays: i32,
    pub logFdbMaxSizeMb: i32,
    pub logFdbMaxFiles: i32,
    pub logParsers: bool,

    pub syncapi: Option<String>,
    pub synctrackers: Option<Vec<String>>,
    pub disable_trackers: Vec<String>,
    pub syncsport: bool,
    pub syncspidr: bool,
    pub maxreadfile: i32,
    pub evercache: Evercache,
    pub fdbPathLevels: i32,
    /// minutes
    pub timeStatsUpdate: i32,
    /// minutes
    pub timeSync: i32,
    /// minutes
    pub timeSyncSpidr: i32,
    pub saveCheckpointEveryNBatches: i32,

    pub Rutor: TrackerSettings,
    pub Megapeer: TrackerSettings,
    pub TorrentBy: TrackerSettings,
    pub Kinozal: TrackerSettings,
    pub NNMClub: TrackerSettings,
    pub Bitru: TrackerSettings,
    pub Toloka: TrackerSettings,
    pub Mazepa: TrackerSettings,
    pub Rutracker: TrackerSettings,
    pub Selezen: TrackerSettings,
    pub Lostfilm: TrackerSettings,
    pub Animelayer: TrackerSettings,
    pub Anidub: TrackerSettings,
    pub Anistar: TrackerSettings,
    pub Anibelka: TrackerSettings,
    pub Aniliberty: TrackerSettings,
    pub Anifilm: TrackerSettings,
    pub Leproduction: TrackerSettings,
    pub Viruseproject: TrackerSettings,
    pub Korsars: TrackerSettings,
    pub Ultradox: TrackerSettings,
    pub Knaben: TrackerSettings,
    pub Baibako: TrackerSettings,
    /// RuDub (ex-BaibaKoTV). Host mirrors rotate (rN.rudub.world).
    pub Rudub: TrackerSettings,
    /// SubsPlease - public anime API (1080p magnets only).
    pub SubsPlease: TrackerSettings,

    pub flaresolverr: FlareSolverrSettings,
    pub cffetch: CfFetchSettings,
    pub proxy: ProxySettings,
    pub search: SearchSettings,
    pub alloha: AllohaSettings,
    pub torznab: TorznabSettings,
    pub logging: LoggingOptions,
    pub globalproxy: Option<Vec<ProxySettings>>,
    pub admin: AdminSettings,
    pub waf: WafSettings,
}

impl Default for AppOptions {
    fn default() -> Self {
        let t = TrackerSettings::new;
        let mut categories = IndexMap::new();
        categories.insert("parsers".to_string(), "None".to_string());
        AppOptions {
            listenip: "any".into(),
            listenport: 9117,
            apikey: None,
            devkey: None,
            mergeduplicates: true,
            mergenumduplicates: true,
            openstats: true,
            opensync: true,
            tracks: false,
            web: true,
            tracksmod: 0,
            tracksdelay: 20_000,
            trackslog: true,
            tracksatempt: 20,
            tracksconcurrency: 2,
            tracksffptimeout: 60,
            tracksffptimeoutnosid: 30,
            tracksreadtimeout: 30,
            trackspeerwaittimeout: 30,
            tracksffpretry: 2,
            tracksminbufferkb: 512,
            tracksorphansweepmin: 15,
            trackscategory: "crabindex".into(),
            TracksInterval: TracksIntervalConfig::default(),
            tsuri: vec!["http://127.0.0.1:8090".into()],
            logFdb: true,
            logFdbRetentionDays: 7,
            logFdbMaxSizeMb: 0,
            logFdbMaxFiles: 0,
            logParsers: true,
            syncapi: None,
            synctrackers: None,
            disable_trackers: vec![],
            syncsport: true,
            syncspidr: true,
            maxreadfile: 200,
            evercache: Evercache::default(),
            fdbPathLevels: 2,
            timeStatsUpdate: 90,
            timeSync: 60,
            timeSyncSpidr: 60,
            saveCheckpointEveryNBatches: 5,
            Rutor: t("http://rutor.info", 8),
            Megapeer: t("http://megapeer.vip", 5),
            TorrentBy: t("https://torrent.by", 8),
            Kinozal: t("https://kinozal.guru", 8),
            NNMClub: t("https://nnmclub.to", 8),
            Bitru: t("https://bitru.org", 8),
            Toloka: t("https://toloka.to", 8),
            Mazepa: t("https://mazepa.to", 8),
            Rutracker: t("https://rutracker.org", 8),
            Selezen: t("https://use.selezen.club", 8),
            Lostfilm: t("https://www.lostfilm.tv", 8),
            Animelayer: t("https://animelayer.ru", 8),
            Anidub: t("https://tr.anidub.com", 8),
            Anistar: t("https://anistar.org", 8),
            Anibelka: t("https://anibelka.com", 8),
            Aniliberty: t("https://aniliberty.top", 8),
            Anifilm: t("https://anifilm.pro", 8),
            Leproduction: t("https://www.le-production.online", 8),
            Viruseproject: t("https://viruseproject.tv", 8),
            Korsars: t("https://korsars.pro", 8),
            Ultradox: t("https://ultradox.vip", 8),
            Knaben: t("https://api.knaben.org", 8),
            Baibako: t("http://baibako.tv", 8),
            Rudub: t("https://r4.rudub.world", 8),
            SubsPlease: t("https://subsplease.org", 8),
            flaresolverr: FlareSolverrSettings::default(),
            cffetch: CfFetchSettings::default(),
            proxy: ProxySettings::default(),
            search: SearchSettings::default(),
            alloha: AllohaSettings::default(),
            torznab: TorznabSettings::default(),
            logging: LoggingOptions { categories: Some(categories), ..LoggingOptions::default() },
            globalproxy: None,
            admin: AdminSettings::default(),
            waf: WafSettings::default(),
        }
    }
}

impl AppOptions {
    /// Re-derive computed fields after deserialization.
    fn finalize(mut self) -> Self {
        for t in self.trackers_mut() {
            t.parseDelay = t.parse_delay();
        }
        self
    }

    fn trackers_mut(&mut self) -> [&mut TrackerSettings; 25] {
        [
            &mut self.Rutor,
            &mut self.Megapeer,
            &mut self.TorrentBy,
            &mut self.Kinozal,
            &mut self.NNMClub,
            &mut self.Bitru,
            &mut self.Toloka,
            &mut self.Mazepa,
            &mut self.Rutracker,
            &mut self.Selezen,
            &mut self.Lostfilm,
            &mut self.Animelayer,
            &mut self.Anidub,
            &mut self.Anistar,
            &mut self.Anibelka,
            &mut self.Aniliberty,
            &mut self.Anifilm,
            &mut self.Leproduction,
            &mut self.Viruseproject,
            &mut self.Korsars,
            &mut self.Ultradox,
            &mut self.Knaben,
            &mut self.Baibako,
            &mut self.Rudub,
            &mut self.SubsPlease,
        ]
    }

    /// Tracker settings by slug / field name, case-insensitive (`GetTrackerSettings`).
    pub fn tracker(&self, name: &str) -> Option<&TrackerSettings> {
        let n = name.to_ascii_lowercase();
        Some(match n.as_str() {
            "rutor" => &self.Rutor,
            "megapeer" => &self.Megapeer,
            "torrentby" => &self.TorrentBy,
            "kinozal" => &self.Kinozal,
            "nnmclub" => &self.NNMClub,
            "bitru" => &self.Bitru,
            "toloka" => &self.Toloka,
            "mazepa" => &self.Mazepa,
            "rutracker" => &self.Rutracker,
            "selezen" => &self.Selezen,
            "lostfilm" => &self.Lostfilm,
            "animelayer" => &self.Animelayer,
            "anidub" => &self.Anidub,
            "anistar" => &self.Anistar,
            "anibelka" => &self.Anibelka,
            "aniliberty" => &self.Aniliberty,
            "anifilm" => &self.Anifilm,
            "leproduction" => &self.Leproduction,
            "viruseproject" => &self.Viruseproject,
            "korsars" => &self.Korsars,
            "ultradox" => &self.Ultradox,
            "knaben" => &self.Knaben,
            "baibako" => &self.Baibako,
            "rudub" => &self.Rudub,
            "subsplease" => &self.SubsPlease,
            _ => return None,
        })
    }

    /// Per-tracker parser log switch.
    pub fn tracker_log_enabled(&self, tracker: &str) -> bool {
        if !self.logParsers || tracker.trim().is_empty() {
            return false;
        }
        match self.tracker(tracker) {
            Some(t) => t.log,
            None => self.logParsers,
        }
    }

    /// Admin URL prefix in effect: the configured one when valid, else `/admin`.
    pub fn admin_path(&self) -> String {
        normalize_admin_path(&self.admin.path).unwrap_or_else(|_| DEFAULT_ADMIN_PATH.to_string())
    }

    pub fn is_tracker_disabled(&self, tracker: &str) -> bool {
        self.disable_trackers.iter().any(|t| t.eq_ignore_ascii_case(tracker))
    }
}

/// Every tracker slug known to the app (lowercase).
pub const TRACKER_SLUGS: [&str; 25] = [
    "anibelka", "anidub", "anifilm", "aniliberty", "animelayer", "anistar", "baibako", "bitru", "kinozal", "knaben",
    "korsars", "leproduction", "lostfilm", "mazepa", "megapeer", "nnmclub", "rudub", "rutor", "rutracker", "selezen",
    "subsplease", "toloka", "torrentby", "ultradox", "viruseproject",
];

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

pub const CONFIG_FILE_YAML: &str = "init.yaml";
pub const CONFIG_FILE_JSON: &str = "init.conf";

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ConfigSourceInfo {
    pub path: Option<String>,
    pub format: Option<String>,
    pub exists: bool,
    #[serde(with = "crate::time::net_opt")]
    pub lastModifiedUtc: Option<chrono::DateTime<chrono::Utc>>,
}

/// init.yaml wins over init.conf.
pub fn get_config_source() -> Option<(String, SystemTime)> {
    for p in [CONFIG_FILE_YAML, CONFIG_FILE_JSON] {
        if let Ok(meta) = std::fs::metadata(p) {
            return Some((p.to_string(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
    None
}

pub fn format_for_path(path: &str) -> &'static str {
    let l = path.to_ascii_lowercase();
    if l.ends_with(".yaml") || l.ends_with(".yml") {
        "yaml"
    } else {
        "json"
    }
}

pub fn get_config_source_info() -> ConfigSourceInfo {
    match get_config_source() {
        Some((path, lw)) => ConfigSourceInfo {
            format: Some(format_for_path(&path).to_string()),
            path: Some(path),
            exists: true,
            lastModifiedUtc: Some(chrono::DateTime::<chrono::Utc>::from(lw)),
        },
        None => ConfigSourceInfo::default(),
    }
}

pub fn detect_config_format(content: &str, fallback: &str) -> String {
    let t = content.trim_start();
    if t.is_empty() {
        return fallback.to_string();
    }
    if t.starts_with('{') || t.starts_with('[') {
        return "json".into();
    }
    if t.starts_with("---") || t.contains(':') {
        return "yaml".into();
    }
    fallback.to_string()
}

/// Parse raw text (yaml|json) into a JSON value.
pub fn parse_to_value(content: &str, format: &str) -> Result<Value, String> {
    if format.eq_ignore_ascii_case("yaml") || format.eq_ignore_ascii_case("yml") {
        if content.trim().trim_start_matches("---").trim().is_empty() {
            return Ok(Value::Object(Map::new()));
        }
        let v: Value = serde_yaml::from_str(content).map_err(|e| e.to_string())?;
        Ok(if v.is_null() { Value::Object(Map::new()) } else { v })
    } else {
        serde_json::from_str(&strip_json_comments(content)).map_err(|e| e.to_string())
    }
}

/// Remove `//` and `/* */` comments outside string literals (init.conf allows them).
pub fn strip_json_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_str = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                while let Some(&n) = chars.peek() {
                    if n == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Deep-merge a user JSON document over defaults and deserialize.
pub fn options_from_value(user: Value) -> Result<AppOptions, String> {
    let mut base = serde_json::to_value(AppOptions::default()).map_err(|e| e.to_string())?;
    merge_into(&mut base, user);
    serde_json::from_value::<AppOptions>(base).map(|o| o.finalize()).map_err(|e| e.to_string())
}

pub fn parse_config_content(content: &str, format: &str) -> Result<AppOptions, String> {
    options_from_value(parse_to_value(content, format)?)
}

pub fn load_from_file(path: &str) -> Result<AppOptions, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let text = text.trim_start_matches('\u{feff}');
    parse_config_content(text, format_for_path(path))
}

fn find_key_ci(map: &Map<String, Value>, key: &str) -> Option<String> {
    if map.contains_key(key) {
        return Some(key.to_string());
    }
    map.keys().find(|k| k.eq_ignore_ascii_case(key)).cloned()
}

/// Populate: objects merge (case-insensitive keys), null keeps default,
/// string scalars coerce to numeric/bool defaults, everything else replaces.
pub fn merge_into(base: &mut Value, user: Value) {
    match (base, user) {
        (Value::Object(b), Value::Object(u)) => {
            for (k, v) in u {
                let key = find_key_ci(b, &k).unwrap_or(k);
                match b.get_mut(&key) {
                    Some(bv) => merge_into(bv, v),
                    None => {
                        b.insert(key, v);
                    }
                }
            }
        }
        (_, Value::Null) => {}
        (b @ Value::Number(_), Value::String(s)) => {
            let s = s.trim();
            if let Ok(i) = s.parse::<i64>() {
                *b = Value::from(i);
            } else if let Ok(f) = s.parse::<f64>() {
                *b = Value::from(f as i64);
            }
        }
        (b @ Value::Number(_), Value::Number(n)) => {
            // ints in the model: truncate floats such as "7.0"
            if let Some(i) = n.as_i64() {
                *b = Value::from(i);
            } else if let Some(f) = n.as_f64() {
                *b = Value::from(f as i64);
            }
        }
        (b @ Value::Bool(_), Value::String(s)) => {
            match s.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => *b = Value::Bool(true),
                "false" | "no" | "off" | "0" | "" => *b = Value::Bool(false),
                _ => {}
            }
        }
        (b @ Value::Bool(_), Value::Number(n)) => *b = Value::Bool(n.as_i64().unwrap_or(0) != 0),
        (b @ Value::String(_), Value::Number(n)) => *b = Value::String(n.to_string()),
        (b @ Value::String(_), Value::Bool(x)) => *b = Value::String(x.to_string()),
        (b, u) => *b = u,
    }
}

/// Render a JSON config object as yaml ("---\n" prefixed, nulls omitted) or indented json.
pub fn render_config_value(data: &Value, format: &str) -> String {
    if format.eq_ignore_ascii_case("json") {
        return serde_json::to_string_pretty(data).unwrap_or_else(|_| "{}".into());
    }
    let cleaned = strip_nulls(data.clone());
    let yaml = serde_yaml::to_string(&cleaned).unwrap_or_default();
    format!("---\n{yaml}")
}

fn strip_nulls(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.into_iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k, strip_nulls(v))).collect()),
        Value::Array(a) => Value::Array(a.into_iter().map(strip_nulls).collect()),
        x => x,
    }
}

/// Atomic write via temp file + rename. A symlinked target (Docker: `/app/init.yaml` →
/// config volume) is written through, so the link survives and the volume gets the change.
pub fn write_atomically(path: &str, content: &str) -> std::io::Result<()> {
    let is_link = std::fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    let resolved;
    let path = if is_link {
        resolved = std::fs::canonicalize(path)?.to_string_lossy().to_string();
        resolved.as_str()
    } else {
        path
    };
    if let Some(dir) = Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, content)?;
    // keep the original file mode (e.g. 0600 for a config holding keys)
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path)
}

// ---------------------------------------------------------------------------
// Provider (current config + hot reload)
// ---------------------------------------------------------------------------

struct CacheState {
    path: Option<String>,
    last_write: Option<SystemTime>,
    initialized: bool,
}

static CURRENT: Lazy<ArcSwap<AppOptions>> = Lazy::new(|| {
    let s = ArcSwap::from_pointee(AppOptions::default());
    s
});
static STATE: Lazy<Mutex<CacheState>> =
    Lazy::new(|| Mutex::new(CacheState { path: None, last_write: None, initialized: false }));
type ChangeCb = Box<dyn Fn(&AppOptions) + Send + Sync>;
static CALLBACKS: Lazy<Mutex<Vec<ChangeCb>>> = Lazy::new(|| Mutex::new(Vec::new()));
static INIT: std::sync::Once = std::sync::Once::new();

/// Current configuration snapshot. Loads from disk on first use.
pub fn conf() -> Arc<AppOptions> {
    INIT.call_once(|| refresh_if_changed(None));
    CURRENT.load_full()
}

/// Register a callback fired after a config reload.
pub fn on_change(cb: impl Fn(&AppOptions) + Send + Sync + 'static) {
    CALLBACKS.lock().push(Box::new(cb));
}

fn notify_change(c: &AppOptions) {
    for cb in CALLBACKS.lock().iter() {
        cb(c);
    }
}

/// Reload when init.yaml/init.conf changed (called at startup and by the reload worker).
pub fn refresh_if_changed(force_log_label: Option<&str>) {
    let mut label: Option<String> = force_log_label.map(|s| s.to_string());
    let mut log_path: Option<String> = None;
    let mut changed = false;
    {
        let mut st = STATE.lock();
        let src = get_config_source();
        if !st.initialized {
            st.initialized = true;
            match src {
                None => {
                    CURRENT.store(Arc::new(AppOptions::default()));
                    label.get_or_insert_with(|| "config (default)".into());
                }
                Some((path, lw)) => {
                    match load_from_file(&path) {
                        Ok(c) => CURRENT.store(Arc::new(c)),
                        Err(e) => crate::log::error(crate::log::cat::CONFIG, format!("{path}: {e}")),
                    }
                    st.path = Some(path.clone());
                    st.last_write = Some(lw);
                    if label.is_none() {
                        label = Some("config (start)".into());
                        log_path = Some(path);
                    }
                }
            }
        } else if let Some((path, lw)) = src {
            if st.path.as_deref() != Some(path.as_str()) || st.last_write != Some(lw) {
                let is_reload = st.path.is_some();
                match load_from_file(&path) {
                    Ok(c) => {
                        CURRENT.store(Arc::new(c));
                        changed = true;
                    }
                    Err(e) => crate::log::error(crate::log::cat::CONFIG, format!("{path}: {e}")),
                }
                st.path = Some(path.clone());
                st.last_write = Some(lw);
                if label.is_none() {
                    label = Some(if is_reload { "config (reload)".into() } else { "config (start)".into() });
                    log_path = Some(path);
                }
            }
        }
    }
    let current = CURRENT.load_full();
    crate::log::apply(&current);
    if let Some(l) = label {
        log_safe_config(&l, log_path.as_deref());
    }
    if changed && force_log_label.is_none() {
        notify_change(&current);
    }
}

/// Reload from a just-written file (config save API).
pub fn reload_from_disk(path: &str) -> Result<(), String> {
    let c = load_from_file(path)?;
    {
        let mut st = STATE.lock();
        st.path = Some(path.to_string());
        st.last_write = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        st.initialized = true;
    }
    CURRENT.store(Arc::new(c));
    let current = CURRENT.load_full();
    crate::log::apply(&current);
    log_safe_config("config (saved)", Some(path));
    notify_change(&current);
    Ok(())
}

/// Replace current config in memory (tests / tools).
pub fn set_current(c: AppOptions) {
    INIT.call_once(|| {});
    CURRENT.store(Arc::new(c.finalize()));
}

/// Keys whose values are redacted in logs / config API.
pub const SENSITIVE_KEYS: [&str; 7] = ["apikey", "devkey", "password", "p", "cookie", "token", "username"];

pub fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
}

/// Redact sensitive values in place (non-empty strings → "***").
pub fn redact_sensitive(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, val) in m.iter_mut() {
                if is_sensitive_key(k) {
                    if let Value::String(s) = val {
                        if !s.is_empty() {
                            *val = Value::String("***".into());
                        }
                    }
                } else {
                    redact_sensitive(val);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(redact_sensitive),
        _ => {}
    }
}

/// Current config as JSON (optionally redacted) - `GetConfigData`.
pub fn config_value(redact: bool) -> Value {
    // Reads CURRENT directly: this runs inside conf()'s one-time init (config logging),
    // and calling conf() there would re-enter the Once and deadlock.
    let mut v = serde_json::to_value(&*CURRENT.load_full()).unwrap_or(Value::Object(Map::new()));
    if redact {
        redact_sensitive(&mut v);
    }
    v
}

pub fn safe_config_json() -> String {
    serde_json::to_string_pretty(&config_value(true)).unwrap_or_else(|_| "{}".into())
}

fn log_safe_config(label: &str, source: Option<&str>) {
    let src = source.map(|s| format!(" from {s}")).unwrap_or_default();
    crate::log::info(
        crate::log::cat::CONFIG,
        format!("[{}] {label}{src} applied (sensitive data redacted):", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
    );
    crate::log::info(crate::log::cat::CONFIG, safe_config_json());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_conf_call_does_not_deadlock() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = conf();
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(std::time::Duration::from_secs(10)).is_ok(), "conf() deadlocked");
    }

    #[test]
    fn yaml_populates_over_defaults() {
        let y = "---\nlistenport: \"9200\"\nrutor:\n  log: false\nevercache:\n  enable: false\nlogging:\n  categories:\n    sync: Warning\n";
        let c = parse_config_content(y, "yaml").unwrap();
        assert_eq!(c.listenport, 9200);
        assert_eq!(c.Rutor.host, "http://rutor.info");
        assert!(!c.Rutor.log);
        assert!(!c.evercache.enable);
        assert_eq!(c.evercache.validHour, 1);
        let cats = c.logging.categories.unwrap();
        assert_eq!(cats.get("parsers").map(String::as_str), Some("None"));
        assert_eq!(cats.get("sync").map(String::as_str), Some("Warning"));
        assert_eq!(c.Megapeer.parseDelay, 12000);
    }

    #[cfg(unix)]
    #[test]
    fn write_keeps_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("crab-mode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("init.yaml");
        std::fs::write(&f, "a").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomically(f.to_str().unwrap(), "b").unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_through_symlink_keeps_link() {
        let dir = std::env::temp_dir().join(format!("crab-cfg-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("vol")).unwrap();
        let real = dir.join("vol/init.yaml");
        let link = dir.join("init.yaml");
        std::fs::write(&real, "a").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomically(link.to_str().unwrap(), "b").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "b");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn json_with_comments_parses() {
        let j = "{\n  // port\n  \"listenport\": 9300, /* inline */\n  \"apikey\": \"a//b\"\n}";
        let c = parse_config_content(j, "json").unwrap();
        assert_eq!(c.listenport, 9300);
        assert_eq!(c.apikey.as_deref(), Some("a//b"));
        let e = parse_config_content(include_str!("../../../Data/example.conf"), "json");
        assert!(e.is_ok(), "{e:?}");
    }

    #[test]
    fn admin_path_validation() {
        assert_eq!(normalize_admin_path("/admin").unwrap(), "/admin");
        assert_eq!(normalize_admin_path("panel_1").unwrap(), "/panel_1");
        assert_eq!(normalize_admin_path(" /my-panel/ ").unwrap(), "/my-panel");
        for bad in ["/", "", "/a", "/Admin", "/a/b", "/x y", "/адм", "/api", "/cron", "/STATS", "/docs/", "/sw.js", "/search",
            &format!("/{}", "a".repeat(33))]
        {
            assert!(normalize_admin_path(bad).is_err(), "{bad} accepted");
        }
        for r in ADMIN_RESERVED_PATHS {
            assert!(normalize_admin_path(r).is_err(), "{r} accepted");
        }
        let mut c = AppOptions::default();
        assert_eq!(c.admin_path(), "/admin");
        c.admin.path = "/api".into();
        assert_eq!(c.admin_path(), "/admin");
        c.admin.path = "/secret-door".into();
        assert_eq!(c.admin_path(), "/secret-door");
        assert!(is_valid_admin_token("Z0mt0N7r2hoUM2TuOk"));
        assert!(!is_valid_admin_token("short"));
        assert!(!is_valid_admin_token("Z0mt0N7r2hoUM2TuO&"));
    }

    #[test]
    fn admin_block_parses_and_token_is_redacted() {
        let c = parse_config_content("admin:\n  path: /panel\n  token: abcDEF123456789xyz\n  sessionHours: \"3\"\n", "yaml").unwrap();
        assert!(c.admin.enable);
        assert_eq!(c.admin.path, "/panel");
        assert_eq!(c.admin.sessionHours, 3);
        let mut v = serde_json::to_value(&c).unwrap();
        redact_sensitive(&mut v);
        assert_eq!(v["admin"]["token"], "***");
        assert_eq!(v["admin"]["path"], "/panel");
        assert_eq!(AppOptions::default().admin.sessionHours, 12);
    }

    #[test]
    fn example_yaml_parses() {
        let y = include_str!("../../../Data/example.yaml");
        let c = parse_config_content(y, "yaml").unwrap();
        assert_eq!(c.Rutracker.reqMinute, 30);
        assert_eq!(c.globalproxy.unwrap()[0].pattern.as_deref(), Some("\\.onion"));
        assert_eq!(c.admin.path, "/admin");
        assert!(c.admin.token.is_empty());
        assert_eq!(c.admin.sessionHours, 12);
        assert!(c.waf.enable);
        assert_eq!(c.waf.rateLimit.perMinute, 300);
        assert_eq!(c.waf.trapPaths.len(), 5);
        let j = parse_config_content(include_str!("../../../Data/example.conf"), "json").unwrap();
        assert_eq!(j.admin.path, "/admin");
        assert_eq!(j.waf.trapBanMinutes, 1440);
        assert_eq!(j.waf.historySize, 5000);
    }

    #[test]
    fn waf_block_defaults_and_partial_override() {
        let d = AppOptions::default().waf;
        assert!(d.enable && d.logRequests && d.whitelistLan && d.rateLimit.enable);
        assert_eq!((d.historySize, d.rateLimit.perMinute, d.rateLimit.banMinutes, d.trapBanMinutes), (5000, 300, 15, 1440));
        assert_eq!(d.trapPaths, ["/.env", "/wp-admin", "/wp-login.php", "/.git/", "/phpmyadmin"]);
        assert!(d.blockUserAgents.is_empty());
        let c = parse_config_content("waf:\n  rateLimit:\n    perMinute: \"60\"\n  blockUserAgents: [sqlmap]\n", "yaml").unwrap();
        assert_eq!(c.waf.rateLimit.perMinute, 60);
        assert_eq!(c.waf.rateLimit.banMinutes, 15);
        assert!(c.waf.enable);
        assert_eq!(c.waf.blockUserAgents, ["sqlmap"]);
        assert_eq!(c.waf.trapPaths.len(), 5);
    }
}
