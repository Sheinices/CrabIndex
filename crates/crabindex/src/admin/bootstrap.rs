//! First-start bootstrap: generate `admin.token` / `devkey` when empty and persist them in the
//! config file (YAML stays YAML, JSON stays JSON; `init.yaml` is created when there is none).
//! Also renders the entry URL printed at startup and by `crabindex admin`.

use crab_core::config::{self, AppOptions};
use crab_core::log;
use serde_json::{Map, Value};
use std::net::{IpAddr, UdpSocket};

use super::crypto::random_alnum;

/// Env var consulted when the admin path is generated.
pub const ADMIN_PATH_ENV: &str = "CRABINDEX_ADMIN_PATH";
pub const TOKEN_LEN: usize = 18;
pub const DEVKEY_LEN: usize = 32;

/// What the bootstrap generated (for logging).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Generated {
    pub path: String,
    pub token: Option<String>,
    pub devkey: Option<String>,
}

fn key_ci(m: &Map<String, Value>, key: &str) -> Option<String> {
    m.keys().find(|k| k.eq_ignore_ascii_case(key)).cloned()
}

fn str_at<'a>(m: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    key_ci(m, key).and_then(|k| m.get(&k)).and_then(Value::as_str)
}

/// Fill missing admin values in a raw config document. Returns `None` when nothing is missing.
pub fn apply_to_document(doc: &mut Value, env_path: Option<&str>) -> Option<Generated> {
    if !doc.is_object() {
        *doc = Value::Object(Map::new());
    }
    let root = doc.as_object_mut()?;
    let admin_key = key_ci(root, "admin").unwrap_or_else(|| "admin".into());
    if !root.get(&admin_key).map(Value::is_object).unwrap_or(false) {
        root.insert(admin_key.clone(), Value::Object(Map::new()));
    }
    let need_devkey = str_at(root, "devkey").map(|s| s.trim().is_empty()).unwrap_or(true);
    let admin = root.get_mut(&admin_key)?.as_object_mut()?;
    let need_token = str_at(admin, "token").map(|s| s.trim().is_empty()).unwrap_or(true);
    if !need_devkey && !need_token {
        return None;
    }

    let mut out = Generated::default();
    let configured = str_at(admin, "path").and_then(|p| config::normalize_admin_path(p).ok());
    let from_env = env_path.and_then(|p| match config::normalize_admin_path(p) {
        Ok(p) => Some(p),
        Err(e) => {
            log::warn("admin", format!("{ADMIN_PATH_ENV}: {e}"));
            None
        }
    });
    out.path = from_env.or(configured).unwrap_or_else(|| config::DEFAULT_ADMIN_PATH.to_string());
    let path_key = key_ci(admin, "path").unwrap_or_else(|| "path".into());
    admin.insert(path_key, Value::String(out.path.clone()));
    if need_token {
        let t = random_alnum(TOKEN_LEN);
        let k = key_ci(admin, "token").unwrap_or_else(|| "token".into());
        admin.insert(k, Value::String(t.clone()));
        out.token = Some(t);
    }
    if need_devkey {
        let d = random_alnum(DEVKEY_LEN);
        let k = key_ci(root, "devkey").unwrap_or_else(|| "devkey".into());
        root.insert(k, Value::String(d.clone()));
        out.devkey = Some(d);
    }
    Some(out)
}

/// Bootstrap one config file (`path` may not exist yet; its extension picks the format).
pub fn bootstrap_file(path: &str, env_path: Option<&str>) -> Result<Option<Generated>, String> {
    let format = config::format_for_path(path);
    let mut doc = match std::fs::read_to_string(path) {
        Ok(text) => config::parse_to_value(text.trim_start_matches('\u{feff}'), format)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(e) => return Err(e.to_string()),
    };
    let Some(generated) = apply_to_document(&mut doc, env_path) else { return Ok(None) };
    // Refuse to write a document the loader would reject.
    config::options_from_value(doc.clone())?;
    config::write_atomically(path, &config::render_config_value(&doc, format)).map_err(|e| e.to_string())?;
    Ok(Some(generated))
}

/// Startup hook: generate missing values into the active config file (or `init.yaml`),
/// reload it and log the entry URL + devkey once.
pub fn run_startup() {
    let c = crab_core::conf();
    let has_devkey = c.devkey.as_deref().map(|d| !d.trim().is_empty()).unwrap_or(false);
    if !c.admin.token.trim().is_empty() && has_devkey {
        if config::normalize_admin_path(&c.admin.path).is_err() {
            log::warn("admin", format!("admin.path '{}' is invalid, using {}", c.admin.path, c.admin_path()));
        }
        return;
    }
    let path = config::get_config_source().map(|(p, _)| p).unwrap_or_else(|| config::CONFIG_FILE_YAML.to_string());
    let env = std::env::var(ADMIN_PATH_ENV).ok().filter(|v| !v.trim().is_empty());
    match bootstrap_file(&path, env.as_deref()) {
        Ok(Some(_)) => {
            if let Err(e) = config::reload_from_disk(&path) {
                log::error("admin", format!("{path}: reload after bootstrap failed: {e}"));
                return;
            }
            let c = crab_core::conf();
            log::warn("admin", format!("generated admin credentials in {path}"));
            for line in credential_lines(&c) {
                log::warn("admin", line);
            }
        }
        Ok(None) => {}
        Err(e) => log::error("admin", format!("{path}: cannot write admin credentials: {e}")),
    }
}

/// Address shown in the entry URL: the configured listen IP, else the primary local IP
/// (`<host>` inside a container, where that IP is not reachable from outside).
pub fn display_host(listenip: &str) -> String {
    let ip = listenip.trim();
    let specific = ip.parse::<IpAddr>().ok().filter(|a| !a.is_unspecified());
    let host = match specific {
        Some(a) => a,
        None if std::path::Path::new("/.dockerenv").exists() => return "<host>".into(),
        None => primary_ip().unwrap_or(IpAddr::from([127, 0, 0, 1])),
    };
    match host {
        IpAddr::V6(v6) => format!("[{v6}]"),
        v4 => v4.to_string(),
    }
}

/// Local address of the default route (no packets are sent).
fn primary_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
}

pub fn entry_url(c: &AppOptions) -> String {
    format!("http://{}:{}{}?{}", display_host(&c.listenip), c.listenport, c.admin_path(), c.admin.token)
}

/// `http://<host>:9117/admin?<token>` and `devkey: …` (without the `admin: ` log prefix).
pub fn credential_lines(c: &AppOptions) -> [String; 2] {
    [entry_url(c), format!("devkey: {}", c.devkey.as_deref().unwrap_or(""))]
}

/// `crabindex admin`: print the entry URL and devkey from the config file and exit.
pub fn print_cli() -> i32 {
    let c = match config::get_config_source() {
        Some((p, _)) => match config::load_from_file(&p) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{p}: {e}");
                return 1;
            }
        },
        None => AppOptions::default(),
    };
    if c.admin.token.trim().is_empty() || c.devkey.as_deref().map(|d| d.trim().is_empty()).unwrap_or(true) {
        eprintln!("admin: credentials are not generated yet - start the server once (or run scripts/install.sh)");
        return 1;
    }
    if !c.admin.enable {
        eprintln!("admin: the admin panel is disabled (admin.enable: false)");
    }
    for line in credential_lines(&c) {
        println!("admin: {line}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("crab-admin-boot-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn yaml_config_is_filled_and_stays_yaml() {
        let d = temp_dir("yaml");
        let p = d.join("init.yaml");
        std::fs::write(&p, "---\n# comment\nlistenport: 9200\napikey: abc\ndevkey: \"\"\nrutor:\n  log: false\n").unwrap();
        let p = p.to_str().unwrap();
        let g = bootstrap_file(p, None).unwrap().unwrap();
        assert_eq!(g.path, "/admin");
        assert_eq!(g.token.as_ref().unwrap().len(), TOKEN_LEN);
        assert_eq!(g.devkey.as_ref().unwrap().len(), DEVKEY_LEN);
        let text = std::fs::read_to_string(p).unwrap();
        assert!(text.starts_with("---\n"), "{text}");
        let c = config::load_from_file(p).unwrap();
        assert_eq!(c.listenport, 9200);
        assert_eq!(c.apikey.as_deref(), Some("abc"));
        assert!(!c.Rutor.log);
        assert_eq!(c.admin.token, g.token.unwrap());
        assert_eq!(c.devkey, g.devkey);
        assert_eq!(c.admin.path, "/admin");
        // second run: nothing to do, file untouched
        assert!(bootstrap_file(p, Some("/other")).unwrap().is_none());
        assert_eq!(std::fs::read_to_string(p).unwrap(), text);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn json_config_is_filled_and_stays_json() {
        let d = temp_dir("json");
        let p = d.join("init.conf");
        std::fs::write(&p, "{\n  // comment\n  \"listenport\": 9300,\n  \"devkey\": \"keep-me\",\n  \"Admin\": { \"path\": \"/panel\", \"sessionHours\": 2 }\n}").unwrap();
        let p = p.to_str().unwrap();
        let g = bootstrap_file(p, None).unwrap().unwrap();
        assert_eq!(g.path, "/panel");
        assert!(g.devkey.is_none());
        let text = std::fs::read_to_string(p).unwrap();
        let v: Value = serde_json::from_str(&text).expect("plain JSON");
        assert_eq!(v["devkey"], "keep-me");
        assert_eq!(v["Admin"]["sessionHours"], 2);
        assert_eq!(v["Admin"]["token"], g.token.clone().unwrap());
        let c = config::load_from_file(p).unwrap();
        assert_eq!(c.admin.path, "/panel");
        assert_eq!(c.admin.sessionHours, 2);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn missing_file_is_created_with_env_path() {
        let d = temp_dir("new");
        let p = d.join("init.yaml");
        let p = p.to_str().unwrap();
        let g = bootstrap_file(p, Some("my-door")).unwrap().unwrap();
        assert_eq!(g.path, "/my-door");
        let c = config::load_from_file(p).unwrap();
        assert_eq!(c.admin_path(), "/my-door");
        assert!(config::is_valid_admin_token(&c.admin.token));
        assert_eq!(c.listenport, 9117);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn invalid_env_path_and_broken_file() {
        let mut doc = serde_json::json!({ "admin": { "path": "/api", "token": "" }, "devkey": "k" });
        let g = apply_to_document(&mut doc, Some("/Bad Path")).unwrap();
        assert_eq!(g.path, "/admin");
        assert_eq!(doc["admin"]["path"], "/admin");
        let d = temp_dir("broken");
        let p = d.join("init.yaml");
        std::fs::write(&p, "listenport: [unclosed\n").unwrap();
        assert!(bootstrap_file(p.to_str().unwrap(), None).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "listenport: [unclosed\n");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn entry_url_format() {
        let mut c = AppOptions::default();
        c.listenip = "10.1.2.3".into();
        c.admin.token = "Z0mt0N7r2hoUM2TuOk".into();
        c.admin.path = "/panel".into();
        c.devkey = Some("dk".into());
        assert_eq!(credential_lines(&c), ["http://10.1.2.3:9117/panel?Z0mt0N7r2hoUM2TuOk".to_string(), "devkey: dk".to_string()]);
        assert_eq!(display_host("::1"), "[::1]");
        assert!(!display_host("any").is_empty());
    }
}
