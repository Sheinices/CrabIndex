/// Install default in-memory config once (skips reading init.yaml from disk).
pub fn cfg() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| crab_core::config::set_current(crab_core::config::AppOptions::default()));
}
