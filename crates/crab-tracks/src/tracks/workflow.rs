//! Full analysis of one magnet: pick a TorrServer, add the torrent, wait for metadata/peers/buffer,
//! probe `/ffp`, save the result to `Data/tracks` and update the FileDB attempt counter.

use crab_core::conf;
use crab_core::models::FfprobeModel;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use super::db::{self, DATABASE};
use super::logging::{log_to_file, tlog};
use super::models;
use super::paths::{self, normalize_infohash};
use super::remote::{self, FfpError};
use super::{index, selector};

/// Analyze tracks for a magnet (skips bad types, busy hashes and missing config).
pub async fn add(magnet: &str, current_attempt: i32, types: Option<&[String]>, torrent_key: Option<&str>, typetask: i32, sid: i32) {
    let tt = Some(typetask);
    if crab_core::util::is_blank(magnet) {
        tlog("Ошибка: magnet-ссылка не может быть пустой", tt);
        return;
    }
    if let Some(t) = types {
        if db::the_bad(t) {
            tlog(format!("Пропуск добавления треков: недопустимый тип контента [{}]", t.join(", ")), tt);
            return;
        }
    }
    let c = conf();
    if c.tsuri.is_empty() {
        tlog("Ошибка: не настроены tsuri серверы", tt);
        return;
    }
    if c.trackscategory.is_empty() {
        tlog("Ошибка: не настроена trackscategory", tt);
        return;
    }
    let Some(infohash) = db::infohash_from_magnet(magnet) else {
        tlog("Ошибка парсинга magnet-ссылки: Invalid magnet link", tt);
        return;
    };

    let Some(_lock) = remote::try_acquire_hash_lock(&infohash) else {
        tlog(format!("Торрент {infohash} уже анализируется - пропуск."), tt);
        return;
    };

    remote::register_in_flight(&infohash);
    add_core(magnet, current_attempt, torrent_key, typetask, sid, &infohash).await;
    remote::unregister_in_flight(&infohash);
}

/// Cancels `token` after `dur` unless dropped first.
struct Deadline(tokio::task::JoinHandle<()>);

impl Deadline {
    fn new(token: CancellationToken, dur: Duration) -> Self {
        Deadline(tokio::spawn(async move {
            tokio::time::sleep(dur).await;
            token.cancel();
        }))
    }
}

impl Drop for Deadline {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Default)]
struct Run {
    res: Option<FfprobeModel>,
    success: bool,
    skip_result_update: bool,
    owned_in_category: bool,
    error_message: Option<String>,
    api_status_code: i32,
}

enum RunErr {
    Cancelled,
    Json(String),
    Other(String),
}

async fn add_core(magnet: &str, current_attempt: i32, torrent_key: Option<&str>, typetask: i32, sid: i32, infohash: &str) {
    let tt = Some(typetask);
    tlog(format!("Начало анализа треков для {infohash}."), tt);

    let expected_category = conf().trackscategory.clone();
    let mut run = Run::default();

    let pick_token = CancellationToken::new();
    let tsuri = {
        let _d = Deadline::new(pick_token.clone(), Duration::from_secs(60));
        remote::select_best_server(&pick_token).await.ok().flatten()
    };

    match &tsuri {
        None => tlog("Все серверы недоступны.", tt),
        Some(tsuri) => {
            let _slot = remote::acquire_analyze_slot().await;
            let overall = remote::analyze_overall_timeout(sid);
            let token = CancellationToken::new();
            let deadline = Deadline::new(token.clone(), overall);

            let outcome = analyze(tsuri, magnet, infohash, &expected_category, sid, typetask, &token, &mut run).await;
            drop(deadline);
            match outcome {
                Ok(()) => {}
                Err(RunErr::Cancelled) => {
                    let msg = format!("Анализ для инфохаша {infohash} отменен по таймауту (overall {:.0}s)", overall.as_secs_f64());
                    tlog(&msg, tt);
                    run.error_message = Some(msg);
                    run.api_status_code = 408;
                }
                Err(RunErr::Json(e)) => {
                    let msg = format!("Ошибка обработки JSON ответа: {e}");
                    tlog(&msg, tt);
                    run.error_message = Some(msg);
                }
                Err(RunErr::Other(e)) => {
                    let msg = format!("Критическая ошибка при анализе треков: {e}");
                    tlog(&msg, tt);
                    run.error_message = Some(msg);
                }
            }
            remote::cleanup_torrent(tsuri, infohash, tt, run.owned_in_category).await;
        }
    }

    let c = conf();
    if tsuri.is_none() || run.api_status_code == 503 {
        let backoff = c.tracksdelay.max(10_000);
        tlog(format!("Backoff {backoff}ms (TorrServer down/timeout)."), tt);
        tokio::time::sleep(Duration::from_millis(backoff as u64)).await;
        return;
    }
    if run.skip_result_update {
        return;
    }

    let code = run.api_status_code;
    let success = run.success;
    update_analysis_results(magnet, torrent_key, infohash, current_attempt, success, run.res, typetask, code, run.error_message).await;

    if !success && (code == 400 || code == 408 || code == 504 || code >= 500) {
        let backoff = c.tracksdelay.max(5_000);
        tlog(format!("Backoff {backoff}ms after API {code} for {infohash}"), tt);
        tokio::time::sleep(Duration::from_millis(backoff as u64)).await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn analyze(
    tsuri: &str,
    magnet: &str,
    infohash: &str,
    expected_category: &str,
    sid: i32,
    typetask: i32,
    token: &CancellationToken,
    run: &mut Run,
) -> Result<(), RunErr> {
    let tt = Some(typetask);
    let add = remote::add_torrent_to_server(tsuri, magnet, infohash, expected_category, token, tt).await;
    // Only the pick → server-reflects-it window needs the in-flight count.
    remote::release_in_flight(tsuri);
    let add = add.map_err(|_| RunErr::Cancelled)?;

    if add.server_error {
        run.owned_in_category = add.add_attempted || add.exists_in_correct_category;
        let msg = "TorrServer недоступен или таймаут add/list".to_string();
        run.api_status_code = 503;
        run.skip_result_update = true;
        tlog(format!("{msg}."), tt);
        run.error_message = Some(msg);
        return Ok(());
    }

    let should_analyze = add.added || add.exists_in_correct_category;
    run.owned_in_category = should_analyze || add.add_attempted;
    if !should_analyze {
        let msg = format!("Торрент не в категории '{expected_category}'");
        tlog(format!("{msg}. Анализ отменен."), tt);
        run.error_message = Some(msg);
        run.skip_result_update = true;
        return Ok(());
    }

    if add.exists_in_correct_category {
        tlog(format!("Торрент {infohash} уже существует на сервере в категории '{expected_category}'. Начинаем анализ..."), tt);
    } else {
        tlog(format!("Торрент {infohash} успешно добавлен в категорию '{expected_category}'. Начинаем анализ..."), tt);
    }

    remote::wait_torrent_ready(tsuri, infohash, token, tt, None).await.map_err(|_| RunErr::Cancelled)?;
    let can_probe = remote::wait_download_progress(tsuri, infohash, token, tt, None).await.map_err(|_| RunErr::Cancelled)?;
    if !can_probe {
        let msg = "нет данных от сидов - /ffp пропущен".to_string();
        run.api_status_code = 408;
        tlog(format!("{msg} для {infohash}"), tt);
        run.error_message = Some(msg);
        return Ok(());
    }

    let peer_info = remote::get_torrent_from_server(tsuri, infohash, token, tt).await.map_err(|_| RunErr::Cancelled)?;
    remote::wait_media_buffer(tsuri, infohash, token, tt, None).await.map_err(|_| RunErr::Cancelled)?;

    let ffp_timeout = remote::ffp_timeout(sid, peer_info.as_ref());
    let max_files = 1 + remote::ffp_retry_extra();
    let file_stats = peer_info.as_ref().and_then(|p| p.file_stats.as_deref());
    let file_ids = selector::select_file_ids(file_stats, max_files);

    if let (Some(fs), Some(first_id)) = (file_stats, file_ids.first()) {
        if let Some(first) = fs.iter().find(|f| f.id == *first_id) {
            tlog(
                format!("Выбран file id={} path={} length={}", first.id, first.path.as_deref().unwrap_or(""), first.length),
                tt,
            );
        }
    }

    let (res, code, err) = match remote::probe_ffp_with_retries(tsuri, infohash, &file_ids, ffp_timeout, token, tt).await {
        Ok(v) => v,
        Err(FfpError::Cancelled) | Err(FfpError::Timeout) => return Err(RunErr::Cancelled),
        Err(FfpError::Json(e)) => return Err(RunErr::Json(e)),
        Err(FfpError::Http(e)) => return Err(RunErr::Other(e)),
    };
    run.api_status_code = code;
    let stream_count = res.as_ref().and_then(|r| r.streams.as_ref()).map(Vec::len).unwrap_or(0);
    run.res = res;

    if stream_count > 0 {
        run.success = true;
        tlog(format!("API успешно вернул {stream_count} треков"), tt);
    } else {
        match err.as_deref() {
            None | Some("") => {
                let msg = "Нет данных о треках".to_string();
                tlog(format!("{msg} для инфохаша {infohash} (код: {code})"), tt);
                run.error_message = Some(msg);
            }
            Some("no probeable media file") => {
                let msg = "нет подходящего media-файла для /ffp".to_string();
                tlog(format!("{msg} для {infohash}"), tt);
                run.error_message = Some(msg);
            }
            Some(e) => {
                tlog(format!("{e} для инфохаша {infohash} (код: {code})"), tt);
                run.error_message = Some(e.to_string());
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_analysis_results(
    magnet: &str,
    torrent_key: Option<&str>,
    infohash: &str,
    current_attempt: i32,
    success: bool,
    result: Option<FfprobeModel>,
    typetask: i32,
    api_status_code: i32,
    error_message: Option<String>,
) {
    let tt = Some(typetask);
    let mut key = torrent_key.filter(|k| !k.is_empty()).map(str::to_string);

    if success {
        if let Some(r) = result.filter(|r| r.streams.as_ref().map(|s| !s.is_empty()).unwrap_or(false)) {
            save_track_results(r, infohash, tt);
        }
        tlog(format!("Анализ треков для {infohash} успешно завершен!"), tt);
        if current_attempt == 0 {
            return;
        }
        if key.is_none() {
            key = find_torrent_key_by_magnet_async(magnet).await;
        }
        let Some(key) = key else {
            tlog(format!("Не удалось найти torrentKey для {infohash}. Сброс ffprobe_tryingdata невозможен."), tt);
            return;
        };
        update_ffprobe_info(key, magnet.to_string(), 0).await;
        return;
    }

    if key.is_none() {
        key = find_torrent_key_by_magnet_async(magnet).await;
    }
    let Some(key) = key else {
        tlog(format!("Не удалось найти torrentKey для {infohash}. Обновление ffprobe_tryingdata невозможно."), tt);
        return;
    };

    let new_attempt = db::next_failure_attempt(current_attempt);
    if new_attempt != current_attempt {
        update_ffprobe_info(key, magnet.to_string(), new_attempt).await;
    }
    db::log_analysis_failure(typetask, infohash, api_status_code, (conf().tracksatempt - new_attempt).max(0), error_message.as_deref());
}

async fn update_ffprobe_info(key: String, magnet: String, attempt: i32) {
    let _ = tokio::task::spawn_blocking(move || crab_core::fdb::update_torrent_ffprobe_info(&key, &magnet, attempt, None)).await;
}

async fn find_torrent_key_by_magnet_async(magnet: &str) -> Option<String> {
    let m = magnet.to_string();
    tokio::task::spawn_blocking(move || find_torrent_key_by_magnet(&m)).await.ok().flatten()
}

/// FileDB bucket that holds a torrent with the same infohash (full scan).
pub fn find_torrent_key_by_magnet(magnet: &str) -> Option<String> {
    let infohash = db::infohash_from_magnet(magnet)?;
    for (key, _) in crab_core::fdb::master_db_snapshot() {
        let shard = crab_core::fdb::open_read(&key, false, false);
        let found = shard
            .values()
            .any(|t| !t.magnet.is_empty() && db::infohash_from_magnet(&t.magnet).as_deref() == Some(infohash.as_str()));
        if found {
            return Some(key);
        }
    }
    None
}

/// Store a successful probe in memory and in `Data/tracks` (removing legacy/uppercase copies).
pub fn save_track_results(result: FfprobeModel, infohash: &str, typetask: Option<i32>) {
    let Some(streams) = result.streams.as_ref().filter(|s| !s.is_empty()) else { return };
    let infohash = normalize_infohash(infohash);
    let audio = streams.iter().filter(|s| s.codec_type.as_deref() == Some("audio")).count();
    let video = streams.iter().filter(|s| s.codec_type.as_deref() == Some("video")).count();
    tlog(format!("Сохранение данных треков для {infohash}. Аудио: {audio}, видео: {video}"), typetask);

    let mut audio_languages: Vec<String> = Vec::new();
    for s in streams.iter().filter(|s| s.codec_type.as_deref() == Some("audio")) {
        if let Some(l) = s.tags.as_ref().and_then(|t| t.language.as_ref()) {
            if !audio_languages.contains(l) {
                audio_languages.push(l.clone());
            }
        }
    }

    let result = Arc::new(result);
    DATABASE.insert(infohash.clone(), result.clone());

    let Some(path) = paths::path_db(&infohash, true) else { return };
    if let Err(e) = models::write_track_file(Path::new(&path), &result) {
        tlog(format!("Ошибка при сохранении данных в файл: {e}"), typetask);
        log_to_file(&format!("StackTrace: {e:?}"), typetask);
        return;
    }
    index::register_track_hash(&infohash);

    if let Some(legacy) = paths::resolve_legacy_track_path(&infohash, paths::TRACKS_DIR) {
        if !legacy.eq_ignore_ascii_case(&path) {
            let _ = std::fs::remove_file(legacy);
        }
    }
    if let Some(upper) = paths::uppercase_layout_path(paths::TRACKS_DIR, &infohash, true) {
        if Path::new(&upper).exists() && !upper.eq_ignore_ascii_case(&path) {
            let _ = std::fs::remove_file(upper);
        }
    }

    if !audio_languages.is_empty() {
        tlog(format!("Обнаружены аудио дорожки на языках: {}", audio_languages.join(", ")), typetask);
    }
}
