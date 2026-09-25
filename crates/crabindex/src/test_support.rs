//! Shared test configuration: every pipeline test in this binary uses one global config,
//! so it is set exactly once, before any request.

use crab_core::config::{set_current, AppOptions};
use std::sync::Once;

pub const TOKEN: &str = "Z0mt0N7r2hoUM2TuOk";
pub const DEVKEY: &str = "test-devkey-0123456789";
/// `waf.blockUserAgents` of the test config.
pub const BLOCKED_UA: [&str; 2] = ["sqlmap", "nikto"];

pub fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let mut c = AppOptions::default();
        c.devkey = Some(DEVKEY.into());
        c.admin.token = TOKEN.into();
        c.admin.path = "/admin".into();
        c.admin.sessionHours = 2;
        c.waf.blockUserAgents = BLOCKED_UA.iter().map(|s| s.to_string()).collect();
        set_current(c);
    });
}
