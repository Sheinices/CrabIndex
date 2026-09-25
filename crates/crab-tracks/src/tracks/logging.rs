//! Tracks pipeline logging: console (category `tracks`) + optional `Data/log/tracks.log`.

use crab_core::conf;
use crab_core::log::{self, cat, Level};
use std::io::Write;

/// Log a tracks message; the level is inferred from the text when `level` is `None`.
pub fn log(message: &str, typetask: Option<i32>, level: Option<Level>) {
    let lvl = level.unwrap_or_else(|| log::classify_tracks_message(message));
    let detail = log::settings().tracks_console_detail;
    let to_file = conf().trackslog;

    if lvl == Level::Debug && !detail {
        if to_file {
            log_to_file(message, typetask);
        }
        return;
    }

    if !detail && lvl == Level::Warning && !message.contains("без результата") {
        if to_file {
            log_to_file(message, typetask);
        }
        return;
    }

    let time_now = chrono::Local::now().format("%H:%M:%S");
    let task = typetask.map(|t| format!(" [task:{t}]")).unwrap_or_default();
    log::write(cat::TRACKS, lvl, format!("[{time_now}]{task} {message}"));

    if to_file {
        log_to_file(message, typetask);
    }
}

/// Shorthand: `log(message, typetask, None)`.
pub fn tlog(message: impl AsRef<str>, typetask: Option<i32>) {
    log(message.as_ref(), typetask, None);
}

/// Append to `Data/log/tracks.log` (3 attempts on I/O errors).
pub fn log_to_file(message: &str, typetask: Option<i32>) {
    let dir = "Data/log";
    let file = format!("{dir}/tracks.log");
    if let Err(e) = std::fs::create_dir_all(dir) {
        let time_now = chrono::Local::now().format("%H:%M:%S");
        log::error(cat::TRACKS, format!("[{time_now}] Ошибка записи в лог файл: {e}"));
        return;
    }
    let time_now = chrono::Local::now().format("%H:%M:%S");
    let task = typetask.map(|t| format!(" [task:{t}]")).unwrap_or_default();
    let line = format!("tracks: [{time_now}]{task} {message}\n");

    let mut last_err = None;
    for i in 0..3 {
        match std::fs::OpenOptions::new().create(true).append(true).open(&file).and_then(|mut f| f.write_all(line.as_bytes())) {
            Ok(()) => return,
            Err(e) => {
                last_err = Some(e);
                if i < 2 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }
    if let Some(e) = last_err {
        let time_now = chrono::Local::now().format("%H:%M:%S");
        log::error(cat::TRACKS, format!("[{time_now}] Ошибка записи в лог файл: {e}"));
    }
}
