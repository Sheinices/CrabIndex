//! OpenAPI spec (`/openapi.yaml`, `/swagger/v1/swagger.json`) and Swagger UI (`/swagger`).

use axum::extract::Request;
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use crab_core::log;
use std::path::{Path, PathBuf};

/// Candidate locations of the spec, first existing wins:
/// `wwwroot/openapi.yaml` under the working dir and next to the executable,
/// then `web/public/openapi.yaml` (source tree).
pub fn yaml_path() -> PathBuf {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        if !roots.contains(&dir) {
            roots.push(dir);
        }
    }
    let mut candidates: Vec<PathBuf> = roots.iter().map(|r| r.join("wwwroot").join("openapi.yaml")).collect();
    candidates.extend(roots.iter().map(|r| r.join("web").join("public").join("openapi.yaml")));
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("wwwroot").join("openapi.yaml"))
}

/// Convert a YAML document into compact JSON.
pub fn yaml_to_json(yaml: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_yaml::from_str(yaml).map_err(|e| e.to_string())?;
    let v = if v.is_null() { serde_json::Value::Object(Default::default()) } else { v };
    serde_json::to_string(&v).map_err(|e| e.to_string())
}

/// API version published in the spec: the build version (git tag, e.g. `1.0.0`) without the
/// `+commit` build metadata, so it always matches the running binary.
pub fn api_version() -> &'static str {
    crate::version::VERSION.split('+').next().unwrap_or(crate::version::VERSION)
}

/// Replace `info.version` in the spec text with [`api_version`].
pub fn stamp_version(yaml: &str) -> String {
    let mut in_info = false;
    let mut done = false;
    let mut out = String::with_capacity(yaml.len());
    for line in yaml.split_inclusive('\n') {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            in_info = line.trim_end() == "info:";
        }
        if in_info && !done && line.starts_with("  version:") {
            out.push_str(&format!("  version: {}\n", api_version()));
            done = true;
            continue;
        }
        out.push_str(line);
    }
    out
}

fn read_spec() -> Result<String, String> {
    let path = yaml_path();
    if !path.is_file() {
        return Err(format!("OpenAPI file not found: {}", path.display()));
    }
    let yaml = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(stamp_version(&yaml))
}

pub fn openapi_json() -> Result<String, String> {
    yaml_to_json(&read_spec()?)
}

const SWAGGER_UI_VERSION: &str = "5";

pub fn swagger_ui_html() -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>CrabIndex API</title>
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@{v}/swagger-ui.css">
  <style>html{{box-sizing:border-box;overflow-y:scroll}}*,*:before,*:after{{box-sizing:inherit}}body{{margin:0;background:#fafafa}}</style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@{v}/swagger-ui-bundle.js" crossorigin></script>
  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@{v}/swagger-ui-standalone-preset.js" crossorigin></script>
  <script>
    window.onload = function () {{
      window.ui = SwaggerUIBundle({{
        urls: [
          {{ url: "/openapi.yaml", name: "CrabIndex API (YAML)" }},
          {{ url: "/swagger/v1/swagger.json", name: "CrabIndex API (JSON)" }}
        ],
        dom_id: "#swagger-ui",
        deepLinking: true,
        presets: [SwaggerUIBundle.presets.apis, SwaggerUIStandalonePreset],
        plugins: [SwaggerUIBundle.plugins.DownloadUrl],
        layout: "StandaloneLayout"
      }});
    }};
  </script>
</body>
</html>
"##,
        v = SWAGGER_UI_VERSION
    )
}

fn with_content_type(body: impl IntoResponse, ct: &'static str) -> Response {
    let mut r = body.into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    r
}

/// Serves the spec and Swagger UI ahead of static files, security headers and auth.
pub async fn openapi_mw(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_ascii_lowercase();
    match path.as_str() {
        "/openapi.yaml" => {
            if let Ok(yaml) = read_spec() {
                return with_content_type(yaml, "application/yaml; charset=utf-8");
            }
        }
        "/swagger/v1/swagger.json" => {
            return match openapi_json() {
                Ok(json) => with_content_type(json, "application/json; charset=utf-8"),
                Err(e) => {
                    log::warn("swagger", format!("openapi.yaml → json failed ({e})"));
                    let body = serde_json::json!({ "error": e }).to_string();
                    let mut r = with_content_type(body, "application/json; charset=utf-8");
                    *r.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
                    r
                }
            };
        }
        "/swagger" | "/swagger/" => {
            return (StatusCode::MOVED_PERMANENTLY, [(header::LOCATION, "/swagger/index.html")]).into_response()
        }
        "/swagger/index.html" => return with_content_type(swagger_ui_html(), "text/html; charset=utf-8"),
        _ => {}
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_version_is_stamped() {
        let y = "openapi: 3.0.3\ninfo:\n  title: T\n  version: 9.9.9\ncomponents:\n  schemas:\n    X:\n      properties:\n        version:\n          type: string\n";
        let out = stamp_version(y);
        assert!(out.contains(&format!("  version: {}\n", api_version())));
        assert!(!out.contains("9.9.9"));
        assert!(out.contains("        version:\n"));
    }

    #[test]
    fn yaml_converts_to_json() {
        let j = yaml_to_json("openapi: 3.0.3\ninfo:\n  title: X\npaths: {}\n").unwrap();
        assert_eq!(j, r#"{"openapi":"3.0.3","info":{"title":"X"},"paths":{}}"#);
        assert!(yaml_to_json("a: [").is_err());
    }

    #[test]
    fn repo_spec_parses() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public/openapi.yaml");
        if let Ok(y) = std::fs::read_to_string(p) {
            assert!(yaml_to_json(&y).unwrap().starts_with('{'));
        }
    }
}
