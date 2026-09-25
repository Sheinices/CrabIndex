//! Shared test setup: every test binary runs inside its own temporary working directory
//! (FileDB, masterDb and Data/temp paths are relative to the cwd).

use std::sync::Once;

static INIT: Once = Once::new();

#[allow(dead_code)]
pub fn enter_temp_cwd(name: &str) {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("crab-ops-{name}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::env::set_current_dir(&dir).expect("chdir");
        crab_core::ensure_data_dirs();
        // initialise config before the first conf() call
        crab_core::config::refresh_if_changed(None);
    });
}

#[allow(dead_code)]
pub fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    std::fs::read_to_string(p).expect("fixture")
}
