//! SPA shell routes and the OpenSearch description.

use axum::extract::Request;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;

use crate::security::request_network;

pub fn router() -> Router {
    Router::new()
        .route("/", any(spa))
        .route("/stats", any(spa))
        .route("/opensearch.xml", any(opensearch))
}

/// With `web: false` the public site is off: its shell routes answer like unknown routes.
fn web_disabled() -> Option<Response> {
    (!crate::conf().web).then(|| axum::http::StatusCode::NOT_FOUND.into_response())
}

/// `wwwroot/index.html` with no-store caching headers (public web UI; settings and jobs
/// live in the admin panel, see `crate::admin`).
async fn spa() -> Response {
    if let Some(r) = web_disabled() {
        return r;
    }
    match tokio::fs::read("wwwroot/index.html").await {
        Ok(bytes) => {
            let mut r = bytes.into_response();
            let h = r.headers_mut();
            h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache, no-store, must-revalidate"));
            h.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
            h.insert(header::EXPIRES, HeaderValue::from_static("0"));
            r
        }
        Err(e) => {
            crab_core::log::error(crab_core::log::cat::HOST, format!("wwwroot/index.html: {e}"));
            crate::app::internal_error_response()
        }
    }
}

/// XML escaping of `& < > " '`.
pub fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c => o.push(c),
        }
    }
    o
}

pub fn opensearch_xml(base_url: &str) -> String {
    let search_template = format!("{base_url}/?q={{searchTerms}}");
    let icon_url = format!("{base_url}/img/icon-32.png");
    [
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>".to_string(),
        "<OpenSearchDescription xmlns=\"http://a9.com/-/spec/opensearch/1.1/\">".to_string(),
        "  <ShortName>CrabIndex</ShortName>".to_string(),
        "  <Description>Поиск торрентов CrabIndex</Description>".to_string(),
        "  <InputEncoding>UTF-8</InputEncoding>".to_string(),
        format!("  <Image width=\"32\" height=\"32\" type=\"image/png\">{}</Image>", xml_escape(&icon_url)),
        format!("  <Url type=\"text/html\" method=\"get\" template=\"{}\"/>", xml_escape(&search_template)),
        "</OpenSearchDescription>".to_string(),
    ]
    .join("\n")
}

async fn opensearch(req: Request) -> Response {
    if let Some(r) = web_disabled() {
        return r;
    }
    let net = request_network(&req);
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(str::to_string)
        .or_else(|| req.uri().authority().map(|a| a.to_string()))
        .unwrap_or_default();
    let xml = opensearch_xml(&format!("{}://{}", net.scheme, host));
    let mut r = xml.into_response();
    r.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/opensearchdescription+xml; charset=utf-8"));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opensearch_escapes_template() {
        let x = opensearch_xml("http://h:9117");
        assert!(x.contains("<Image width=\"32\" height=\"32\" type=\"image/png\">http://h:9117/img/icon-32.png</Image>"));
        assert!(x.contains("template=\"http://h:9117/?q={searchTerms}\"/>"));
        assert_eq!(xml_escape("a&b<'\">"), "a&amp;b&lt;&apos;&quot;&gt;");
    }
}
