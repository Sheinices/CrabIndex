//! Console logging with per-category levels. The same lines (after the level filter) also go
//! to `Data/log/app.log`, the sync categories to `Data/log/sync.log`, so the admin panel can show
//! them without access to journalctl or `docker logs`. Files rotate once (`app.1.log`) at
//! `logging.fileMaxMb`.

use arc_swap::ArcSwap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::Write;
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
    pub files: bool,
    pub file_max_bytes: u64,
}

pub const LOG_DIR: &str = "Data/log";
pub const APP_LOG: &str = "app.log";
pub const SYNC_LOG: &str = "sync.log";

static FILE_LOCK: Mutex<()> = parking_lot::const_mutex(());

/// Which file a category lands in; parser detail has its own per-tracker files.
pub fn file_for(category: &str) -> Option<&'static str> {
    let c = category.trim().to_lowercase();
    if c == cat::SYNC || c == cat::SYNC_SPIDR || c == "syncspidr" {
        Some(SYNC_LOG)
    } else if c == cat::PARSER {
        None
    } else {
        Some(APP_LOG)
    }
}

/// `app.log` → `app.1.log`.
pub fn rotated_name(name: &str) -> String {
    match name.strip_suffix(".log") {
        Some(stem) => format!("{stem}.1.log"),
        None => format!("{name}.1"),
    }
}

fn level_tag(level: Level) -> &'static str {
    match level {
        Level::Trace => "TRACE",
        Level::Debug => "DEBUG",
        Level::Information => "INFO",
        Level::Warning => "WARN",
        Level::Error => "ERROR",
        Level::Critical => "CRIT",
        Level::None => "NONE",
    }
}

fn append_file(name: &str, line: &str, max_bytes: u64) {
    // Only where the app runs from its data directory (created at startup); tests and CLI
    // runs from a source tree must not leave log files behind.
    if !std::path::Path::new(LOG_DIR).is_dir() {
        return;
    }
    let _g = FILE_LOCK.lock();
    let path = format!("{LOG_DIR}/{name}");
    if max_bytes > 0 {
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() >= max_bytes {
                let _ = std::fs::rename(&path, format!("{LOG_DIR}/{}", rotated_name(name)));
            }
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
        let _ = f.write_all(b"\n");
    }
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
            files: true,
            file_max_bytes: 20 * 1024 * 1024,
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
        files: o.files,
        file_max_bytes: (o.fileMaxMb.max(0) as u64) * 1024 * 1024,
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
    let s = SETTINGS.load();
    let line = if s.console_timestamp && !message.starts_with('[') {
        format!("{category}: [{}] {message}", chrono::Local::now().format("%H:%M:%S"))
    } else {
        format!("{category}: {message}")
    };
    if level >= Level::Warning {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
    if s.files {
        if let Some(file) = file_for(category) {
            let file_line = format!("{} [{}] {category}: {message}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), level_tag(level));
            append_file(file, &file_line, s.file_max_bytes);
        }
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

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn categories_map_to_files() {
        assert_eq!(file_for("sync"), Some(SYNC_LOG));
        assert_eq!(file_for("syncSpidr"), Some(SYNC_LOG));
        assert_eq!(file_for("sync_spidr"), Some(SYNC_LOG));
        assert_eq!(file_for("parser"), None);
        assert_eq!(file_for("host"), Some(APP_LOG));
        assert_eq!(file_for("admin"), Some(APP_LOG));
        assert_eq!(rotated_name("app.log"), "app.1.log");
        assert_eq!(rotated_name("sync.log"), "sync.1.log");
        assert_eq!(level_tag(Level::Warning), "WARN");
    }
}
