use crab_core::trackers::{self, cycle};

#[test]
fn init_registers_rutracker_and_toloka_starters() {
    crab_trackers_b::init();
    let names: Vec<&str> = trackers::parse_all_starters().iter().map(|s| s.tracker_name()).collect();
    assert!(names.contains(&"rutracker"));
    assert!(names.contains(&"toloka"));
    for other in ["mazepa", "selezen", "bitru"] {
        assert!(!names.contains(&other), "{other} must not be a ParseAll starter");
    }
}

#[test]
fn starters_have_matching_cycle_and_task_parse_paths() {
    for slug in ["rutracker", "toloka"] {
        assert_eq!(cycle::cycle_path_for_tracker(slug), format!("Data/temp/{slug}_parseAllCycle.json"));
        assert_eq!(cycle::task_parse_path_for_tracker(slug), format!("Data/temp/{slug}_taskParse.json"));
    }
}

#[test]
fn router_builds() {
    let _ = crab_trackers_b::router();
}
