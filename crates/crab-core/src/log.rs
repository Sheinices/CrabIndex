//! Console logging with per-category levels.

use arc_swap::ArcSwap;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::AppOptions;

pub mod cat {
    pub const TRACKS: &str = "tracks";
    pub const TRACKS_INDEX: &str = "tracks index";
    pub const TRACKS_STATS: &str = "tracks stats";
    pub const TRACKS_EXPORT: &str = "tracks export";
    pub const SYNC: &str = "sync";
    pub const SYNC_SPIDR: &str = "sync_spidr";
    pub const CRON_HTTP: &str = "cron";
    pub const FDB: &str = "fdb";
    pub const STATS: &str = "stats";
    pub const TRACKERS: &str = "trackers";
    pub const CONFIG: &str = "config";
    pub const HOST: &str = "host";
    pub const PARSER: &str = "parser";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace = 0,
    Debug = 1,
    Information = 2,
    Warning = 3,
    Error = 4,
    Critical = 5,
    None = 6,
}

impl Level {
    pub fn parse(value: Option<&str>, fallback: Level) -> Level {
        let Some(v) = value else { return fallback };
        match v.trim().to_ascii_lowercase().as_str() {
            "trace" => Level::Trace,
            "debug" => Level::Debug,
            "information" | "info" => Level::Information,
            "warning" | "warn" => Level::Warning,
            "error" => Level::Error,
            "critical" => Level::Critical,
            "none" => Level::None,
            _ => fallback,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogSettings {
    pub console_timestamp: bool,
    pub tracks_console_detail: bool,
    pub cron_skip_fast_ms: i32,
    pub default_level: Level,
    pub category_levels: Option<HashMap<String, Level>>,
}

impl Default for LogSettings {
    fn default() -> Self {
        let mut m = HashMap::new();
        m.insert(normalize_category_key("parsers"), Level::None);
        LogSettings {
            console_timestamp: false,
            tracks_console_detail: false,
            cron_skip_fast_ms: 100,
            default_level: Level::Information,
            category_levels: Some(m),
        }
    }
}

static SETTINGS: Lazy<ArcSwap<LogSettings>> = Lazy::new(|| ArcSwap::from_pointee(LogSettings::default()));

pub fn settings() -> Arc<LogSettings> {
    SETTINGS.load_full()
}

fn normalize_category_key(key: &str) -> String {
    if key.eq_ignore_ascii_case("syncSpidr") {
        return cat::SYNC_SPIDR.to_string();
    }
    if key.eq_ignore_ascii_case("parsers") {
        return cat::PARSER.to_string();
    }
    key.trim().to_lowercase()
}

/// Apply the `logging:` block from config.
pub fn apply(conf: &AppOptions) {
    let o = &conf.logging;
    let category_levels = o.categories.as_ref().and_then(|cats| {
        if cats.is_empty() {
            return None;
        }
        let mut map = HashMap::new();
        for (k, v) in cats {
            if k.trim().is_empty() {
                continue;
            }
            let lvl = if v.eq_ignore_ascii_case("None") { Level::None } else { Level::parse(Some(v), Level::Information) };
            map.insert(normalize_category_key(k), lvl);
        }
        Some(map)
    });
    SETTINGS.store(Arc::new(LogSettings {
        console_timestamp: o.consoleTimestamp,
        tracks_console_detail: o.tracksConsoleDetail,
        cron_skip_fast_ms: o.cronSkipFastMs,
        default_level: Level::parse(Some(&o.defaultLevel), Level::Information),
        category_levels,
    }));
}

pub fn is_enabled(category: &str, level: Level) -> bool {
    let s = SETTINGS.load();
    if let Some(map) = &s.category_levels {
        if let Some(min) = map.get(&normalize_category_key(category)) {
            return level >= *min && *min != Level::None;
        }
    }
    level >= s.default_level
}

pub fn write(category: &str, level: Level, message: impl AsRef<str>) {
    if !is_enabled(category, level) {
        return;
    }
    let message = message.as_ref();
    let line = if SETTINGS.load().console_timestamp && !message.starts_with('[') {
        format!("{category}: [{}] {message}", chrono::Local::now().format("%H:%M:%S"))
    } else {
        format!("{category}: {message}")
    };
    if level >= Level::Warning {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}

pub fn debug(category: &str, message: impl AsRef<str>) {
    write(category, Level::Debug, message)
}
pub fn info(category: &str, message: impl AsRef<str>) {
    write(category, Level::Information, message)
}
pub fn warn(category: &str, message: impl AsRef<str>) {
    write(category, Level::Warning, message)
}
pub fn error(category: &str, message: impl AsRef<str>) {
    write(category, Level::Error, message)
}

/// Classify tracks pipeline messages when level not specified.
pub fn classify_tracks_message(message: &str) -> Level {
    if message.is_empty() {
        return Level::Information;
    }
    if message.contains("без результата")
        || message.contains("Нет данных")
        || message.contains("Ошибка")
        || message.to_lowercase().contains("не удалось")
    {
        return Level::Warning;
    }
    const DEBUG_MARKERS: [&str; 9] = [
        "Начало анализа",
        "успешно добавлен",
        "успешно удален",
        "не требуется",
        "уже существует",
        "API успешно",
        "Сохранение данных",
        "Обнаружены аудио",
        "Пауза",
    ];
    if DEBUG_MARKERS.iter().any(|m| message.contains(m)) {
        return Level::Debug;
    }
    Level::Information
}
