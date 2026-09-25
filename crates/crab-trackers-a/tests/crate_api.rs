#[test]
fn init_registers_parse_all_starters() {
    crab_trackers_a::init();
    crab_trackers_a::init();
    let names: Vec<_> = crab_core::trackers::parse_all_starters().iter().map(|s| s.tracker_name()).collect();
    for slug in ["kinozal", "megapeer", "nnmclub", "rutor", "torrentby"] {
        assert_eq!(names.iter().filter(|n| **n == slug).count(), 1, "starter {slug}");
        assert_eq!(crab_core::trackers::cycle::cycle_path_for_tracker(slug), format!("Data/temp/{slug}_parseAllCycle.json"));
        assert_eq!(crab_core::trackers::cycle::task_parse_path_for_tracker(slug), format!("Data/temp/{slug}_taskParse.json"));
    }
}

#[test]
fn router_builds() {
    let _ = crab_trackers_a::router();
}
