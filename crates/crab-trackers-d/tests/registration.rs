//! Every tracker with a resumable ParseAllTask registers a starter; routes are lowercase.

use crab_core::trackers::parse_all_starters;

#[test]
fn init_registers_parse_all_starters() {
    crab_trackers_d::init();
    let names: Vec<&str> = parse_all_starters().iter().map(|s| s.tracker_name()).collect();
    for slug in crab_trackers_d::PARSE_ALL_TRACKERS {
        assert!(names.contains(slug), "missing ParseAll starter for {slug}");
    }
    for slug in ["anibelka", "korsars"] {
        assert!(crab_trackers_d::PARSE_ALL_TRACKERS.contains(&slug));
    }
}

#[test]
fn router_builds() {
    let _ = crab_trackers_d::router();
}
