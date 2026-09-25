//! HTTP application: route table and middleware pipeline.
//!
//! Pipeline, outermost first:
//! 1. panic → `500 {"error":"internal server error"}`
//! 2. CORS (any origin, credentials, any header/method)
//! 3. response compression (gzip, br)
//! 4. network capture (TCP peer; `X-Forwarded-For`/`-Proto` trusted only from loopback)
//! 5. WAF (lists, bans, User-Agent, trap paths, rate limit; request log)
//! 6. `/openapi.yaml`, `/swagger/v1/swagger.json`, Swagger UI
//! 7. security headers
//! 8. admin panel gate, session and API path rewriting (under `admin.path`)
//! 9. static files from `wwwroot/` (when `web: true`; never `wwwroot/admin/**`)
//! 10. authorization (apikey/devkey policies, `/cron/` request log)
//! 11. normalisation (lowercase path and query names) → router

use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::from_fn;
use axum::response::{IntoResponse, Response};
use axum::Router;
use std::any::Any;
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

use crate::{admin, config_api, controllers, normalize, openapi, security, static_files, waf};

pub fn internal_error_response() -> Response {
    let mut r = (StatusCode::INTERNAL_SERVER_ERROR, "{\"error\":\"internal server error\"}").into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    r
}

fn panic_response(err: Box<dyn Any + Send + 'static>) -> Response<Body> {
    let msg = err
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown panic".into());
    crab_core::log::error(crab_core::log::cat::HOST, format!("[fatal] request handler panicked: {msg}"));
    internal_error_response()
}

/// Routes of the server itself plus every feature crate.
pub fn api_router() -> Router {
    Router::new()
        .merge(controllers::home::router())
        .merge(controllers::health::router())
        .merge(config_api::router())
        .merge(crab_cloudflare::router())
        .merge(crab_trackers_a::router())
        .merge(crab_trackers_b::router())
        .merge(crab_trackers_c::router())
        .merge(crab_trackers_d::router())
        .merge(crab_trackers_e::router())
        .merge(crab_search::router())
        .merge(crab_tracks::router())
        .merge(crab_ops::router())
        .fallback(|| async { StatusCode::NOT_FOUND })
}

/// Wrap a router with the full middleware pipeline.
pub fn build_app(routes: Router) -> Router {
    // Path rewriting must happen before routing, so it wraps the router as a service.
    let inner = ServiceBuilder::new().layer(from_fn(normalize::normalize_request)).service(routes);
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::mirror_request())
        .allow_headers(AllowHeaders::mirror_request())
        .allow_methods(AllowMethods::mirror_request())
        .allow_credentials(true);

    Router::new()
        .fallback_service(inner)
        .layer(from_fn(security::authorization))
        .layer(from_fn(static_files::static_files_mw))
        .layer(from_fn(admin::admin_mw))
        .layer(from_fn(security::security_headers_mw))
        .layer(from_fn(openapi::openapi_mw))
        .layer(from_fn(waf::waf_mw))
        .layer(from_fn(security::capture_network))
        .layer(CompressionLayer::new())
        .layer(cors)
        .layer(CatchPanicLayer::custom(panic_response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{ConnectInfo, RawQuery};
    use axum::http::{Method, Request};
    use axum::routing::get;
    use std::net::SocketAddr;
    use tower::ServiceExt;

    async fn boom() -> &'static str {
        panic!("boom")
    }

    fn test_routes() -> Router {
        Router::new()
            .route("/cron/rutor/parsealltask", get(|RawQuery(q): RawQuery| async move { q.unwrap_or_default() }))
            .route("/api/v1.0/torrents", get(|| async { "ok" }))
            .route("/boom", get(boom))
            .merge(controllers::health::router())
            .fallback(|| async { StatusCode::NOT_FOUND })
    }

    async fn call(uri: &str, peer: &str, headers: &[(&str, &str)]) -> Response {
        let app = build_app(test_routes());
        let mut req = Request::builder().method(Method::GET).uri(uri);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let mut req = req.body(Body::empty()).unwrap();
        req.extensions_mut().insert(ConnectInfo::<SocketAddr>(format!("{peer}:5555").parse().unwrap()));
        app.oneshot(req).await.unwrap()
    }

    async fn body(r: Response) -> String {
        String::from_utf8(axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn case_insensitive_routing_and_query_names() {
        let r = call("/Cron/Rutor/ParseAllTask?Page=2&Name=AbC", "127.0.0.1", &[]).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers().get("access-control-allow-private-network").unwrap(), "true");
        assert_eq!(body(r).await, "page=2&name=AbC");
    }

    #[tokio::test]
    async fn dev_paths_denied_from_public_peer() {
        let r = call("/cron/rutor/parsealltask", "203.0.113.9", &[]).await;
        assert!(r.status() == StatusCode::FORBIDDEN || r.status() == StatusCode::UNAUTHORIZED);
        assert!(r.headers().get("access-control-allow-private-network").is_some());
    }

    #[tokio::test]
    async fn spoofed_xff_from_lan_peer_is_ignored_but_requires_devkey() {
        let r = call("/cron/rutor/parsealltask", "192.168.1.10", &[("X-Forwarded-For", "127.0.0.1")]).await;
        assert!(r.status().is_client_error());
    }

    #[tokio::test]
    async fn health_has_security_headers_and_cors() {
        let r = call("/HEALTH", "10.0.0.2", &[("Origin", "http://example.com")]).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers().get("x-frame-options").unwrap(), "SAMEORIGIN");
        assert!(r.headers().get("content-security-policy").is_some());
        assert_eq!(r.headers().get("access-control-allow-origin").unwrap(), "http://example.com");
        assert_eq!(r.headers().get("access-control-allow-credentials").unwrap(), "true");
        assert_eq!(body(r).await, r#"{"status":"OK"}"#);
    }

    #[tokio::test]
    async fn trailing_slash_and_unknown_route() {
        assert_eq!(call("/health/", "10.0.0.2", &[]).await.status(), StatusCode::OK);
        assert_eq!(call("/nope", "10.0.0.2", &[]).await.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn panic_becomes_json_500() {
        let r = call("/boom", "10.0.0.2", &[]).await;
        assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body(r).await, r#"{"error":"internal server error"}"#);
    }

    #[tokio::test]
    async fn swagger_ui_without_csp() {
        let r = call("/swagger", "10.0.0.2", &[]).await;
        assert_eq!(r.status(), StatusCode::MOVED_PERMANENTLY);
        let r = call("/swagger/index.html", "10.0.0.2", &[]).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers().get("content-security-policy").is_none());
        assert!(body(r).await.contains("cdn.jsdelivr.net/npm/swagger-ui-dist"));
    }
}
