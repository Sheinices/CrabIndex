//! `/dev/*` diagnostics, bulk maintenance and data migrations (access restricted by the server).

pub mod diagnostics;
pub mod maintenance;
pub mod migrations;

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use serde_json::Value;
use std::collections::HashMap;

use crate::query::{self, Params};

type Q = Query<HashMap<String, String>>;

/// Run a blocking FileDB job and return its JSON (nulls omitted); 500 if it panicked.
async fn blocking_json(f: impl FnOnce() -> Value + Send + 'static) -> Response {
    match tokio::task::spawn_blocking(f).await {
        Ok(v) => query::json(v).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

macro_rules! simple {
    ($name:ident, $f:path) => {
        async fn $name() -> Response {
            blocking_json($f).await
        }
    };
}

simple!(update_size, maintenance::update_size);
simple!(reset_check_time, maintenance::reset_check_time);
simple!(update_details, maintenance::update_details);
simple!(update_search_name, maintenance::update_search_name);
simple!(fix_knaben_names, migrations::names::fix_knaben_names);
simple!(fix_bitru_names, migrations::names::fix_bitru_names);
simple!(fix_rudub_relased, migrations::names::fix_rudub_relased);
simple!(remove_null_values, migrations::cleanup::remove_null_values);
simple!(fix_empty_search_fields, migrations::cleanup::fix_empty_search_fields);
simple!(migrate_aniliberty_urls, migrations::aniliberty::migrate_urls);
simple!(remove_duplicate_aniliberty, migrations::aniliberty::remove_duplicates);
simple!(fix_animelayer_duplicates, migrations::animelayer::fix_duplicates);
simple!(fix_kinozal_domain_duplicates, migrations::domain_dups::fix_kinozal);
simple!(fix_ultradox_domain_duplicates, migrations::domain_dups::fix_ultradox);

async fn find_corrupt(q: Q) -> Response {
    let n = Params::from_query(q).i32("samplesize", 20);
    blocking_json(move || diagnostics::find_corrupt(n)).await
}

async fn find_duplicate_keys(q: Q) -> Response {
    let p = Params::from_query(q);
    let tracker = p.str("tracker").map(|s| s.to_string());
    let exclude = p.bool("excludenumeric", true);
    blocking_json(move || diagnostics::find_duplicate_keys(tracker.as_deref(), exclude)).await
}

async fn find_empty_search_fields(q: Q) -> Response {
    let n = Params::from_query(q).i32("samplesize", 20);
    blocking_json(move || diagnostics::find_empty_search_fields(n)).await
}

async fn remove_bucket(q: Q) -> Response {
    let p = Params::from_query(q);
    let key = p.str("key").map(|s| s.to_string());
    let mn = p.str("migratename").map(|s| s.to_string());
    let mo = p.str("migrateoriginalname").map(|s| s.to_string());
    blocking_json(move || migrations::cleanup::remove_bucket(key.as_deref(), mn.as_deref(), mo.as_deref())).await
}

pub fn router() -> Router {
    Router::new()
        .route("/dev/findcorrupt", any(find_corrupt))
        .route("/dev/findduplicatekeys", any(find_duplicate_keys))
        .route("/dev/findemptysearchfields", any(find_empty_search_fields))
        .route("/dev/updatesize", any(update_size))
        .route("/dev/resetchecktime", any(reset_check_time))
        .route("/dev/updatedetails", any(update_details))
        .route("/dev/updatesearchname", any(update_search_name))
        .route("/dev/fixknabennames", any(fix_knaben_names))
        .route("/dev/fixbitrunames", any(fix_bitru_names))
        .route("/dev/fixrudubrelased", any(fix_rudub_relased))
        .route("/dev/removenullvalues", any(remove_null_values))
        .route("/dev/removebucket", any(remove_bucket))
        .route("/dev/fixemptysearchfields", any(fix_empty_search_fields))
        .route("/dev/migrateanilibertyurls", any(migrate_aniliberty_urls))
        .route("/dev/removeduplicateaniliberty", any(remove_duplicate_aniliberty))
        .route("/dev/fixanimelayerduplicates", any(fix_animelayer_duplicates))
        .route("/dev/fixkinozaldomainduplicates", any(fix_kinozal_domain_duplicates))
        .route("/dev/fixultradoxdomainduplicates", any(fix_ultradox_domain_duplicates))
}
