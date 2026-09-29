// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! `/dev/*` diagnostics, bulk maintenance and data migrations (access restricted by the server).

pub mod diagnostics;
pub mod maintenance;
pub mod datacheck;
pub mod fixall;
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

/// A migration that changes rows: after it finishes, the data-quality report is refreshed in
/// the background so the panel shows before / after.
macro_rules! fixing {
    ($name:ident, $f:path) => {
        async fn $name() -> Response {
            let r = blocking_json($f).await;
            datacheck::spawn_after_fix(stringify!($name));
            r
        }
    };
}

fixing!(update_size, maintenance::update_size);
simple!(check_data, datacheck::run);
simple!(check_data_status, datacheck::status);
simple!(fix_all, fixall::start);
simple!(fix_all_status, fixall::status);
fixing!(fix_zero_sizes, maintenance::fix_zero_sizes);
simple!(reset_check_time, maintenance::reset_check_time);
fixing!(update_details, maintenance::update_details);
fixing!(update_search_name, maintenance::update_search_name);
fixing!(fix_knaben_names, migrations::names::fix_knaben_names);
fixing!(fix_bitru_names, migrations::names::fix_bitru_names);
fixing!(fix_rudub_relased, migrations::names::fix_rudub_relased);
fixing!(remove_null_values, migrations::cleanup::remove_null_values);
fixing!(fix_empty_search_fields, migrations::cleanup::fix_empty_search_fields);
fixing!(migrate_aniliberty_urls, migrations::aniliberty::migrate_urls);
fixing!(remove_duplicate_aniliberty, migrations::aniliberty::remove_duplicates);
fixing!(fix_animelayer_duplicates, migrations::animelayer::fix_duplicates);
fixing!(fix_kinozal_domain_duplicates, migrations::domain_dups::fix_kinozal);
fixing!(fix_rutracker_domain_duplicates, migrations::domain_dups::fix_rutracker);
fixing!(fix_selezen_domain_duplicates, migrations::domain_dups::fix_selezen);
fixing!(fix_ultradox_domain_duplicates, migrations::domain_dups::fix_ultradox);
fixing!(fix_serial_types, migrations::serial_types::fix_serial_types);
fixing!(fix_rutracker_names, migrations::rutracker_names::fix_rutracker_names);
fixing!(fix_slug_duplicates, migrations::slug_dups::fix_slug_duplicates);

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
        .route("/dev/fixzerosizes", any(fix_zero_sizes))
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
        .route("/dev/fixrutrackerdomainduplicates", any(fix_rutracker_domain_duplicates))
        .route("/dev/fixselezendomainduplicates", any(fix_selezen_domain_duplicates))
        .route("/dev/fixultradoxdomainduplicates", any(fix_ultradox_domain_duplicates))
        .route("/dev/fixserialtypes", any(fix_serial_types))
        .route("/dev/fixrutrackernames", any(fix_rutracker_names))
        .route("/dev/fixslugduplicates", any(fix_slug_duplicates))
        .route("/dev/checkdata", any(check_data))
        .route("/dev/checkdatastatus", any(check_data_status))
        .route("/dev/fixall", any(fix_all))
        .route("/dev/fixallstatus", any(fix_all_status))
}
