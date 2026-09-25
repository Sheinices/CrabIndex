//! Access policies, path → policy registry and the route catalog used for self-checks.

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessPolicy {
    /// Whitelisted paths - no apikey/devkey (health, sync, static shell, swagger).
    Public,
    /// Search and API paths - apikey enforced when configured.
    ApiKeyWhenConfigured,
    /// /api/v1.0/config - admin panel only (requests rewritten by `crate::admin`); 404 otherwise.
    ConfigApi,
    /// /dev/, /cron/, /jsondb - LAN or devkey (same-host proxy alone is not enough).
    DevAdmin,
}

static PATH_WHITELIST: Lazy<Regex> = Lazy::new(|| Regex::new(r"^/(api/v1\.0/conf|sync/)").expect("regex"));

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn eq_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub fn resolve_policy(path: &str) -> AccessPolicy {
    if is_dev_only_path(path) {
        return AccessPolicy::DevAdmin;
    }
    if is_config_api_path(path) {
        return AccessPolicy::ConfigApi;
    }
    if is_path_whitelisted(path) {
        return AccessPolicy::Public;
    }
    AccessPolicy::ApiKeyWhenConfigured
}

pub fn is_config_api_path(path: &str) -> bool {
    !path.is_empty() && starts_with_ci(path, "/api/v1.0/config")
}

pub fn is_dev_only_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    starts_with_ci(path, "/cron/") || eq_ci(path, "/jsondb") || starts_with_ci(path, "/jsondb/") || starts_with_ci(path, "/dev/")
}

pub fn is_restricted_admin_path(path: &str) -> bool {
    is_dev_only_path(path) || is_config_api_path(path)
}

fn is_public_web_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    eq_ci(path, "/opensearch.xml")
        || eq_ci(path, "/manifest.webmanifest")
        || eq_ci(path, "/manifest.json")
        || eq_ci(path, "/sw.js")
        || starts_with_ci(path, "/workbox-")
        || starts_with_ci(path, "/assets/")
        || starts_with_ci(path, "/img/")
        || starts_with_ci(path, "/fonts/")
}

pub fn is_path_whitelisted(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    const EXACT: [&str; 9] = [
        "/",
        "/stats",
        "/stats/",
        "/health",
        "/health/background-jobs",
        "/version",
        "/lastupdatedb",
        "/openapi.yaml",
        "/opensearch.xml",
    ];
    EXACT.iter().any(|p| eq_ci(path, p))
        || is_public_web_path(path)
        || starts_with_ci(path, "/swagger")
        || PATH_WHITELIST.is_match(path)
}

/// Known HTTP routes and the policy they are expected to resolve to.
pub struct RouteEntry {
    pub path: &'static str,
    pub policy: AccessPolicy,
    pub owner: &'static str,
}

const fn r(path: &'static str, policy: AccessPolicy, owner: &'static str) -> RouteEntry {
    RouteEntry { path, policy, owner }
}

use AccessPolicy::*;

pub static ROUTES: &[RouteEntry] = &[
    // Public - SPA shells & health
    r("/", Public, "home"),
    r("/stats", Public, "home"),
    r("/opensearch.xml", Public, "home"),
    r("/health", Public, "health"),
    r("/health/background-jobs", Public, "health"),
    r("/version", Public, "health"),
    r("/lastupdatedb", Public, "health"),
    r("/api/v1.0/conf", Public, "health"),
    r("/openapi.yaml", Public, "openapi"),
    r("/swagger", Public, "swagger"),
    r("/swagger/index.html", Public, "swagger"),
    // Public - sync whitelist (opensync checked by the sync handlers)
    r("/sync/conf", Public, "sync"),
    r("/sync/fdb", Public, "sync"),
    r("/sync/fdb/torrents", Public, "sync"),
    r("/sync/torrents", Public, "sync"),
    // Config API (admin panel only; the admin prefix itself is dynamic and not listed)
    r("/api/v1.0/config", ConfigApi, "config"),
    r("/api/v1.0/config/schema", ConfigApi, "config"),
    r("/api/v1.0/config/validate", ConfigApi, "config"),
    r("/api/v1.0/config/diff", ConfigApi, "config"),
    r("/api/v1.0/config/render", ConfigApi, "config"),
    r("/api/v1.0/config/parse", ConfigApi, "config"),
    r("/api/v1.0/config/format", ConfigApi, "config"),
    // Dev admin
    r("/dev/updateSize", DevAdmin, "dev maintenance"),
    r("/dev/FindCorrupt", DevAdmin, "dev diagnostics"),
    r("/dev/TracksStats", DevAdmin, "dev tracks"),
    r("/dev/FixKnabenNames", DevAdmin, "dev migrations"),
    r("/dev/FixRudubRelased", DevAdmin, "dev migrations"),
    r("/jsondb/save", DevAdmin, "db"),
    r("/cron/maintenance/Check", DevAdmin, "maintenance"),
    r("/cron/maintenance/Status", DevAdmin, "maintenance"),
    r("/cron/maintenance/ResumeParseAll", DevAdmin, "maintenance"),
    r("/cron/maintenance/ParseAllStatus", DevAdmin, "maintenance"),
    // Search - apikey when configured
    r("/api/v1.0/torrents", ApiKeyWhenConfigured, "torrents"),
    r("/api/v1.0/trackers", ApiKeyWhenConfigured, "torrents"),
    r("/api/v1.0/qualitys", ApiKeyWhenConfigured, "torrents"),
    r("/api/v2.0/indexers/all/results", ApiKeyWhenConfigured, "jackett"),
    r("/torznab/api", ApiKeyWhenConfigured, "torznab"),
    r("/api/v2.0/indexers", ApiKeyWhenConfigured, "torznab"),
    r("/api/v1/indexer", ApiKeyWhenConfigured, "torznab"),
    r("/api/v1/search", ApiKeyWhenConfigured, "torznab"),
    // Stats JSON - apikey + openstats in the handlers
    r("/stats/torrents", ApiKeyWhenConfigured, "stats"),
    r("/stats/tracks", ApiKeyWhenConfigured, "stats"),
    r("/stats/meta", ApiKeyWhenConfigured, "stats"),
];

/// Registry mismatches (empty = OK).
pub fn verify_registry() -> Vec<String> {
    ROUTES
        .iter()
        .filter_map(|e| {
            let actual = resolve_policy(e.path);
            (actual != e.policy).then(|| format!("{}: expected {:?}, registry {:?} ({})", e.path, e.policy, actual, e.owner))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_matches_registry() {
        assert!(verify_registry().is_empty(), "{:?}", verify_registry());
    }

    #[test]
    fn policies() {
        assert_eq!(resolve_policy("/CRON/rutor/parse"), AccessPolicy::DevAdmin);
        assert_eq!(resolve_policy("/jsondb"), AccessPolicy::DevAdmin);
        assert_eq!(resolve_policy("/jsondbx"), AccessPolicy::ApiKeyWhenConfigured);
        assert_eq!(resolve_policy("/Api/v1.0/Config/schema"), AccessPolicy::ConfigApi);
        assert_eq!(resolve_policy("/api/v1.0/conf"), AccessPolicy::Public);
        assert_eq!(resolve_policy("/sync/fdb/torrents"), AccessPolicy::Public);
        assert_eq!(resolve_policy("/assets/app.js"), AccessPolicy::Public);
        assert_eq!(resolve_policy("/swagger/v1/swagger.json"), AccessPolicy::Public);
        assert_eq!(resolve_policy("/api/v2.0/indexers/all/results"), AccessPolicy::ApiKeyWhenConfigured);
        assert_eq!(resolve_policy("/jobs"), AccessPolicy::ApiKeyWhenConfigured);
        assert_eq!(resolve_policy("/settings"), AccessPolicy::ApiKeyWhenConfigured);
        assert_eq!(resolve_policy("/stats"), AccessPolicy::Public);
        assert!(is_restricted_admin_path("/dev/x"));
        assert!(is_restricted_admin_path("/api/v1.0/config/save"));
        assert!(!is_restricted_admin_path("/health"));
    }
}
