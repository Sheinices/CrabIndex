//! Static web UI from `wwwroot/` (only when `web: true`). Files that do not exist fall through
//! to the API pipeline; directories are never listed (`/` is served by the SPA route).
//! A directory with an `index.html` (the `/docs` book) serves that file; without the trailing
//! slash it redirects so relative links resolve.
//!
//! `wwwroot/admin/**` (the admin panel build) is never served here: it is reachable only
//! through the admin gate (`crate::admin`) under the configured admin path.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::Response;
use std::path::{Component, Path, PathBuf};
use tower::ServiceExt;
use tower_http::services::ServeFile;

pub const WWWROOT: &str = "wwwroot";

/// Directory under [`WWWROOT`] holding the admin panel build.
pub const ADMIN_DIR: &str = "admin";

/// True when the (percent-decoded) path points into `wwwroot/admin` or at the configured
/// admin prefix; such paths must not be served as public static files.
pub fn is_blocked_path(url_path: &str, admin_path: &str) -> bool {
    let decoded = match urlencoding::decode(url_path) {
        Ok(d) => d.into_owned(),
        Err(_) => return true,
    };
    let first = decoded.split(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("");
    let admin_seg = admin_path.trim_start_matches('/');
    first.eq_ignore_ascii_case(ADMIN_DIR) || (!admin_seg.is_empty() && first.eq_ignore_ascii_case(admin_seg))
}

/// [`resolve_file`] for public requests: `None` for blocked admin paths.
pub fn resolve_public_file(root: &Path, url_path: &str, admin_path: &str) -> Option<PathBuf> {
    if is_blocked_path(url_path, admin_path) {
        return None;
    }
    resolve_file(root, url_path)
}

/// Map a request path to a file under `root` (rejects traversal and hidden tricks).
pub fn resolve_file(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = urlencoding::decode(url_path).ok()?;
    let rel = decoded.trim_start_matches('/');
    if rel.is_empty() {
        return None;
    }
    let rel_path = Path::new(rel);
    if rel_path.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    let full = root.join(rel_path);
    if full.is_file() {
        return Some(full);
    }
    let index = full.join("index.html");
    (url_path.ends_with('/') && index.is_file()).then_some(index)
}

/// `/docs` → `/docs/` when that directory has an index.html.
fn directory_redirect(root: &Path, url_path: &str) -> Option<String> {
    if url_path.ends_with('/') || url_path == "" {
        return None;
    }
    resolve_file(root, &format!("{url_path}/")).map(|_| format!("{url_path}/"))
}

fn content_type_override(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("yaml") | Some("yml") => Some("application/yaml"),
        Some("webmanifest") => Some("application/manifest+json"),
        _ => None,
    }
}

pub async fn static_files_mw(req: Request, next: Next) -> Response {
    let conf = crate::conf();
    if !conf.web
        || !(req.method() == Method::GET || req.method() == Method::HEAD)
        || req.extensions().get::<crate::security::AdminAuthorized>().is_some()
    {
        return next.run(req).await;
    }
    let admin_path = conf.admin_path();
    drop(conf);
    if is_blocked_path(req.uri().path(), &admin_path) {
        return next.run(req).await;
    }
    let Some(file) = resolve_public_file(Path::new(WWWROOT), req.uri().path(), &admin_path) else {
        if let Some(to) = directory_redirect(Path::new(WWWROOT), req.uri().path()) {
            let to = match req.uri().query() {
                Some(q) => format!("{to}?{q}"),
                None => to,
            };
            return Response::builder()
                .status(axum::http::StatusCode::MOVED_PERMANENTLY)
                .header(header::LOCATION, to)
                .body(Body::empty())
                .unwrap_or_default();
        }
        return next.run(req).await;
    };
    let is_sw = req.uri().path().eq_ignore_ascii_case("/sw.js");
    let ct = content_type_override(&file);
    let resp = match ServeFile::new(&file).oneshot(req).await {
        Ok(r) => r.map(Body::new),
        Err(never) => match never {},
    };
    let mut resp = resp;
    if resp.status().is_success() {
        if let Some(ct) = ct {
            resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
        }
    }
    if is_sw {
        let h = resp.headers_mut();
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache, no-store, must-revalidate"));
        h.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        h.insert(header::EXPIRES, HeaderValue::from_static("0"));
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(resolve_file(root, "/Cargo.toml").is_some());
        assert!(resolve_file(root, "/../crabindex/Cargo.toml").is_none());
        assert!(resolve_file(root, "/%2e%2e/crabindex/Cargo.toml").is_none());
        assert!(resolve_file(root, "/").is_none());
        assert!(resolve_file(root, "/src").is_none());
    }

    #[test]
    fn admin_build_is_not_public() {
        let dir = std::env::temp_dir().join(format!("crab-static-admin-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("admin/assets")).unwrap();
        std::fs::write(dir.join("admin/index.html"), "admin").unwrap();
        std::fs::write(dir.join("admin/assets/app.js"), "js").unwrap();
        std::fs::write(dir.join("index.html"), "public").unwrap();
        assert!(resolve_file(&dir, "/admin/index.html").is_some());
        for p in ["/admin/index.html", "/admin/", "/ADMIN/index.html", "//admin/assets/app.js", "/%61dmin/index.html", "/admin"] {
            assert!(resolve_public_file(&dir, p, "/panel").is_none(), "{p} served");
            assert!(is_blocked_path(p, "/panel"), "{p}");
        }
        assert!(is_blocked_path("/panel/x.js", "/panel"));
        assert!(!is_blocked_path("/administrator", "/panel"));
        assert!(!is_blocked_path("/assets/x.js", "/panel"));
        assert_eq!(resolve_public_file(&dir, "/index.html", "/panel"), Some(dir.join("index.html")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn directory_index_and_redirect() {
        let dir = std::env::temp_dir().join(format!("crab-static-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/index.html"), "x").unwrap();
        assert_eq!(resolve_file(&dir, "/docs/"), Some(dir.join("docs/index.html")));
        assert!(resolve_file(&dir, "/docs").is_none());
        assert_eq!(directory_redirect(&dir, "/docs").as_deref(), Some("/docs/"));
        assert!(directory_redirect(&dir, "/nope").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
