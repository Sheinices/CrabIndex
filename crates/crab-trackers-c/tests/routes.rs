#[test]
fn router_builds_without_conflicts() {
    crab_trackers_c::init();
    let _ = crab_trackers_c::router();
}
