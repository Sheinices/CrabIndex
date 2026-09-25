//! Request normalisation before routing.
//!
//! Routing is case-insensitive: every route is registered in lowercase, so the path is
//! lowercased and query parameter *names* are lowercased (values are kept verbatim).
//! A trailing slash is dropped (`/stats/` → `/stats`). Static files are served earlier in
//! the pipeline and never reach this step.

use axum::extract::Request;
use axum::http::Uri;
use axum::middleware::Next;
use axum::response::Response;

/// Lowercase the names of `a=1&B=2` style query parameters.
pub fn normalize_query(q: &str) -> String {
    q.split('&')
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => format!("{}={v}", k.to_ascii_lowercase()),
            None => pair.to_ascii_lowercase(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

pub fn normalize_path(path: &str) -> String {
    let mut p = path.to_ascii_lowercase();
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

pub fn normalize_uri(uri: &Uri) -> Option<Uri> {
    let path = normalize_path(uri.path());
    let query = uri.query().map(normalize_query);
    if path == uri.path() && query.as_deref() == uri.query() {
        return None;
    }
    let pq = match query {
        Some(q) => format!("{path}?{q}"),
        None => path,
    };
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(pq.parse().ok()?);
    Uri::from_parts(parts).ok()
}

pub async fn normalize_request(mut req: Request, next: Next) -> Response {
    if let Some(u) = normalize_uri(req.uri()) {
        *req.uri_mut() = u;
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_path_and_query_names() {
        let u: Uri = "/Cron/Rutor/ParseAllTask/?Page=2&Search=Matrix%20X&flag".parse().unwrap();
        let n = normalize_uri(&u).unwrap();
        assert_eq!(n.path(), "/cron/rutor/parsealltask");
        assert_eq!(n.query(), Some("page=2&search=Matrix%20X&flag"));
    }

    #[test]
    fn untouched_when_already_normal() {
        let u: Uri = "/api/v1.0/torrents?search=ABC".parse().unwrap();
        assert!(normalize_uri(&u).is_none());
        let root: Uri = "/".parse().unwrap();
        assert!(normalize_uri(&root).is_none());
    }
}
