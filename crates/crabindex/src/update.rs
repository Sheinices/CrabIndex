//! Self-update from GitHub releases, driven from the admin panel.
//!
//! `check` asks the GitHub API for the latest release (cached, only when the panel asks).
//! `apply` downloads this platform's archive, verifies it against the release `SHA256SUMS`,
//! swaps the binary, `wwwroot/` and the `Data/` templates in place, then requests a graceful
//! shutdown with exit code [`RESTART_EXIT_CODE`] so systemd (`Restart=on-failure`) starts the
//! new version. Only offered on Linux under systemd with a writable install directory;
//! Docker and manual runs get instructions instead.

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crab_core::log::{self, cat};

use crate::version;

pub const REPO: &str = "sheinices/crabindex";
/// Exit code after a successful update; any non-zero code makes systemd restart the unit.
pub const RESTART_EXIT_CODE: i32 = 75;
const CHECK_TTL: Duration = Duration::from_secs(6 * 3600);
const MAX_ARCHIVE_BYTES: usize = 300 * 1024 * 1024;
const DATA_FILES: [&str; 4] = ["example.yaml", "example.conf", "crontab", "run-job.sh"];

static RESTART: AtomicBool = AtomicBool::new(false);
static CACHE: Lazy<Mutex<Option<(Instant, Release)>>> = Lazy::new(|| Mutex::new(None));
static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub tag: String,
    pub version: String,
    pub name: String,
    pub published_at: Option<DateTime<Utc>>,
    pub notes: String,
    pub url: String,
    pub assets: Vec<Asset>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    /// idle | downloading | verifying | installing | restarting | error
    pub stage: String,
    pub message: String,
    pub target: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
}

/// Set once an update is installed; `main` then exits with [`RESTART_EXIT_CODE`].
pub fn restart_requested() -> bool {
    RESTART.load(Ordering::SeqCst)
}

/// Version without the `+sha` build suffix.
pub fn current_version() -> String {
    version::VERSION.split('+').next().unwrap_or(version::VERSION).to_string()
}

/// `1.2.3`, `v1.2.3-next` → (1, 2, 3). Pre-release suffixes are ignored: a `-next` build of
/// the latest tag is newer than the tag itself, so only the numeric part is compared.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Release archive for this build, `None` where self-update is not offered.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("crabindex-linux-x86_64.tar.gz")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("crabindex-linux-arm64.tar.gz")
    } else {
        None
    }
}

/// Install directory when this process can update itself, otherwise the reason (for the UI).
pub fn self_update_support() -> Result<PathBuf, String> {
    if asset_name().is_none() {
        return Err("Автообновление работает только на Linux (x86_64, arm64). Скачайте новую версию со страницы релиза и замените файлы вручную.".into());
    }
    if Path::new("/.dockerenv").exists() || std::env::var_os("CRABINDEX_DOCKER").is_some() {
        return Err("CrabIndex работает в Docker: обновите образ (docker compose pull && docker compose up -d).".into());
    }
    if std::env::var_os("INVOCATION_ID").is_none() {
        return Err("CrabIndex запущен не как служба systemd: после обновления его некому перезапустить. Обновите установщиком: sudo bash install.sh --update".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("не удалось определить путь к программе: {e}"))?;
    let dir = exe.parent().map(Path::to_path_buf).ok_or("не удалось определить каталог программы")?;
    let probe = dir.join(".update-write-test");
    std::fs::write(&probe, b"x").map_err(|_| format!("Нет прав на запись в {}: обновите установщиком (sudo bash install.sh --update).", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(dir)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("CrabIndex/{}", current_version()))
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn parse_release(v: &Value) -> Option<Release> {
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Asset {
                        name: x.get("name")?.as_str()?.to_string(),
                        url: x.get("browser_download_url")?.as_str()?.to_string(),
                        size: x.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        name: v.get("name").and_then(|s| s.as_str()).unwrap_or(&tag).to_string(),
        published_at: v.get("published_at").and_then(|s| s.as_str()).and_then(|s| s.parse().ok()),
        notes: v.get("body").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        url: v.get("html_url").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        tag,
        assets,
        checked_at: Utc::now(),
    })
}

async fn latest_release(force: bool) -> Result<Release, String> {
    if !force {
        if let Some((at, r)) = CACHE.lock().as_ref() {
            if at.elapsed() < CHECK_TTL {
                return Ok(r.clone());
            }
        }
    }
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = client()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("GitHub недоступен: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub ответил {}", resp.status().as_u16()));
    }
    let v: Value = resp.json().await.map_err(|e| format!("неверный ответ GitHub: {e}"))?;
    let r = parse_release(&v).ok_or("в ответе GitHub нет релиза")?;
    *CACHE.lock() = Some((Instant::now(), r.clone()));
    Ok(r)
}

/// `GET {admin}/api/update[?force=1]`
pub async fn check(force: bool) -> Value {
    let current = current_version();
    let support = self_update_support();
    let state = STATE.lock().clone();
    match latest_release(force).await {
        Ok(r) => {
            let available = is_newer(&r.version, &current);
            let asset = asset_name().and_then(|n| r.assets.iter().find(|a| a.name == n)).cloned();
            json!({
                "current": current,
                "latest": r,
                "available": available,
                "canSelfUpdate": support.is_ok() && asset.is_some(),
                "reason": support.err().or_else(|| asset.is_none().then(|| "в релизе нет архива для этой платформы".to_string())),
                "asset": asset,
                "state": state,
            })
        }
        Err(e) => json!({ "current": current, "latest": null, "available": false, "error": e, "canSelfUpdate": false, "state": state }),
    }
}

fn set_state(stage: &str, message: impl Into<String>) {
    let mut s = STATE.lock();
    s.stage = stage.to_string();
    s.message = message.into();
    if stage == "downloading" {
        s.started_at = Some(Utc::now());
    }
}

/// `POST {admin}/api/update/apply` - starts the update in the background.
pub async fn apply() -> Result<Value, String> {
    let dir = self_update_support()?;
    let release = latest_release(true).await?;
    let current = current_version();
    if !is_newer(&release.version, &current) {
        return Err(format!("Установлена актуальная версия {current}"));
    }
    let name = asset_name().ok_or("нет архива для этой платформы")?;
    let asset = release.assets.iter().find(|a| a.name == name).cloned().ok_or("в релизе нет архива для этой платформы")?;
    let sums = release.assets.iter().find(|a| a.name == "SHA256SUMS").cloned().ok_or("в релизе нет SHA256SUMS, обновление без проверки не выполняется")?;
    {
        let mut s = STATE.lock();
        if matches!(s.stage.as_str(), "downloading" | "verifying" | "installing" | "restarting") {
            return Err("Обновление уже выполняется".into());
        }
        s.target = Some(release.version.clone());
    }
    set_state("downloading", format!("Загрузка {}", asset.name));
    log::warn(cat::HOST, format!("update: {current} -> {} started from the admin panel", release.version));
    let version = release.version.clone();
    tokio::spawn(async move {
        match run(dir, asset, sums, &version).await {
            Ok(()) => {
                set_state("restarting", format!("Версия {version} установлена, перезапуск"));
                log::warn(cat::HOST, format!("update: {version} installed, restarting"));
                tokio::time::sleep(Duration::from_millis(1500)).await;
                RESTART.store(true, Ordering::SeqCst);
                crab_core::trackers::app_stopping().cancel();
            }
            Err(e) => {
                log::error(cat::HOST, format!("update failed: {e}"));
                set_state("error", e);
            }
        }
    });
    Ok(json!({ "ok": true, "target": release.version }))
}

async fn download(url: &str) -> Result<Vec<u8>, String> {
    let resp = client().get(url).send().await.map_err(|e| format!("загрузка {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("загрузка {url}: HTTP {}", resp.status().as_u16()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("загрузка {url}: {e}"))?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("архив слишком большой".into());
    }
    Ok(bytes.to_vec())
}

/// Expected hash for `name` from a `sha256sum` listing (`<hex>  <name>` or `<hex> *<name>`).
pub fn expected_sha256(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hash = it.next()?;
        let file = it.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

async fn run(dir: PathBuf, asset: Asset, sums: Asset, version: &str) -> Result<(), String> {
    let archive = download(&asset.url).await?;
    set_state("verifying", "Проверка контрольной суммы");
    let sums_text = String::from_utf8_lossy(&download(&sums.url).await?).to_string();
    let expected = expected_sha256(&sums_text, &asset.name).ok_or("в SHA256SUMS нет строки для архива")?;
    let actual = hex::encode(Sha256::digest(&archive));
    if actual != expected {
        return Err(format!("контрольная сумма не совпала (ожидалась {expected}, получена {actual}), обновление отменено"));
    }
    set_state("installing", format!("Установка версии {version}"));
    let dir2 = dir.clone();
    tokio::task::spawn_blocking(move || install(&dir2, &archive)).await.map_err(|e| format!("установка: {e}"))??;
    reinstall_crontab(&dir).await;
    Ok(())
}

/// Unpack into `dir/.update-staging` and swap files in. Renames only (the install dir is
/// writable; some files in it may be root-owned but can still be replaced by rename).
fn install(dir: &Path, archive: &[u8]) -> Result<(), String> {
    let staging = dir.join(".update-staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("каталог распаковки: {e}"))?;
    let result = (|| {
        let gz = flate2::read::GzDecoder::new(archive);
        let mut tar = tar::Archive::new(gz);
        for entry in tar.entries().map_err(|e| format!("архив: {e}"))? {
            let mut entry = entry.map_err(|e| format!("архив: {e}"))?;
            // unpack_in refuses absolute paths and `..` components
            entry.unpack_in(&staging).map_err(|e| format!("распаковка: {e}"))?;
        }
        let root = bundle_root(&staging).ok_or("в архиве нет файла crabindex")?;
        let bin = root.join("crabindex");
        let mut head = [0u8; 4];
        std::fs::File::open(&bin).and_then(|mut f| f.read_exact(&mut head)).map_err(|e| format!("новый бинарник: {e}"))?;
        if head != *b"\x7fELF" {
            return Err("новый бинарник не является программой Linux".to_string());
        }
        replace_file(&bin, &dir.join("crabindex"), 0o755)?;
        let www = root.join("wwwroot");
        if www.is_dir() {
            let prev = dir.join("wwwroot.prev");
            let _ = std::fs::remove_dir_all(&prev);
            let cur = dir.join("wwwroot");
            if cur.exists() {
                std::fs::rename(&cur, &prev).map_err(|e| format!("wwwroot: {e}"))?;
            }
            std::fs::rename(&www, &cur).map_err(|e| format!("wwwroot: {e}"))?;
            let _ = std::fs::remove_dir_all(&prev);
        }
        for f in DATA_FILES {
            let src = root.join("Data").join(f);
            if src.is_file() {
                replace_file(&src, &dir.join("Data").join(f), if f.ends_with(".sh") { 0o755 } else { 0o644 })?;
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn bundle_root(staging: &Path) -> Option<PathBuf> {
    if staging.join("crabindex").is_file() {
        return Some(staging.to_path_buf());
    }
    std::fs::read_dir(staging).ok()?.flatten().map(|e| e.path()).find(|p| p.join("crabindex").is_file())
}

fn replace_file(src: &Path, dst: &Path, mode: u32) -> Result<(), String> {
    let tmp = dst.with_extension("update-new");
    std::fs::copy(src, &tmp).map_err(|e| format!("{}: {e}", dst.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = mode;
    std::fs::rename(&tmp, dst).map_err(|e| format!("{}: {e}", dst.display()))
}

/// The new `Data/crontab` may add jobs: install it for the service user, with the actual port
/// and install directory (same rewrite as the installer). Failures are only logged.
async fn reinstall_crontab(dir: &Path) {
    let Ok(tab) = std::fs::read_to_string(dir.join("Data/crontab")) else { return };
    let port = crate::conf().listenport;
    let mut text = tab;
    if port != 9117 {
        text = text.replace("127.0.0.1:9117", &format!("127.0.0.1:{port}"));
    }
    let d = dir.display().to_string();
    if d != "/opt/crabindex" {
        text = text.replace("/opt/crabindex/", &format!("{d}/"));
    }
    let child = tokio::process::Command::new("crontab").arg("-").stdin(std::process::Stdio::piped()).spawn();
    let Ok(mut child) = child else {
        log::warn(cat::HOST, "update: crontab not available, jobs not reinstalled");
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(text.as_bytes()).await;
    }
    match child.wait().await {
        Ok(s) if s.success() => log::info(cat::HOST, "update: crontab reinstalled"),
        _ => log::warn(cat::HOST, "update: crontab reinstall failed (keep the old one)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v1.0.10"), Some((1, 0, 10)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("1.0.3-next+abc"), Some((1, 0, 3)));
        assert!(is_newer("v1.0.10", "1.0.9"));
        assert!(is_newer("1.1.0", "1.0.3-next"));
        assert!(!is_newer("1.0.3", "1.0.3-next"));
        assert!(!is_newer("1.0.2", "1.0.3"));
        assert!(!is_newer("garbage", "1.0.0"));
    }

    #[test]
    fn sha256sums_lookup() {
        let s = format!("{}  crabindex-linux-x86_64.tar.gz\n{} *crabindex-linux-arm64.tar.gz\n", "a".repeat(64), "B".repeat(64));
        assert_eq!(expected_sha256(&s, "crabindex-linux-x86_64.tar.gz"), Some("a".repeat(64)));
        assert_eq!(expected_sha256(&s, "crabindex-linux-arm64.tar.gz"), Some("b".repeat(64)));
        assert_eq!(expected_sha256(&s, "crabindex-macos-arm64.tar.gz"), None);
    }

    #[test]
    fn parses_github_release() {
        let v = json!({
            "tag_name": "v1.0.4", "name": "v1.0.4", "published_at": "2026-09-26T10:00:00Z", "body": "notes",
            "html_url": "https://github.com/sheinices/crabindex/releases/tag/v1.0.4",
            "assets": [{ "name": "SHA256SUMS", "browser_download_url": "https://x/SHA256SUMS", "size": 10 }]
        });
        let r = parse_release(&v).unwrap();
        assert_eq!(r.version, "1.0.4");
        assert_eq!(r.assets[0].name, "SHA256SUMS");
        assert!(r.published_at.is_some());
    }

    #[test]
    fn install_swaps_files_from_a_bundle() {
        let tmp = std::env::temp_dir().join(format!("crab-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("Data")).unwrap();
        std::fs::create_dir_all(tmp.join("wwwroot")).unwrap();
        std::fs::write(tmp.join("crabindex"), b"old").unwrap();
        std::fs::write(tmp.join("wwwroot/index.html"), b"old").unwrap();

        let mut buf = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            let mut add = |path: &str, data: &[u8]| {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                b.append_data(&mut h, path, data).unwrap();
            };
            add("crabindex-1.0.4-linux-x86_64/crabindex", b"\x7fELFnew");
            add("crabindex-1.0.4-linux-x86_64/wwwroot/index.html", b"new");
            add("crabindex-1.0.4-linux-x86_64/Data/crontab", b"# new");
            b.into_inner().unwrap().finish().unwrap();
        }
        install(&tmp, &buf).unwrap();
        assert_eq!(std::fs::read(tmp.join("crabindex")).unwrap(), b"\x7fELFnew");
        assert_eq!(std::fs::read(tmp.join("wwwroot/index.html")).unwrap(), b"new");
        assert_eq!(std::fs::read(tmp.join("Data/crontab")).unwrap(), b"# new");
        assert!(!tmp.join(".update-staging").exists());
        assert!(!tmp.join("wwwroot.prev").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_rejects_non_elf() {
        let tmp = std::env::temp_dir().join(format!("crab-update-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("Data")).unwrap();
        std::fs::write(tmp.join("crabindex"), b"old").unwrap();
        let mut buf = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu();
            h.set_size(3);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, "x/crabindex", &b"bad"[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        assert!(install(&tmp, &buf).is_err());
        assert_eq!(std::fs::read(tmp.join("crabindex")).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
