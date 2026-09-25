//! Networking: HTTP client with proxy rotation and Cloudflare fallback.
pub mod cf;
pub mod http;

pub use http::{cp1251, download, get, get_json, post, post_json, raw_client, BaseResponse, PostBody, Req, USER_AGENT};
