//! Build/version information embedded by `build.rs`.

pub const VERSION: &str = env!("CRABINDEX_VERSION");
pub const GIT_SHA: &str = env!("CRABINDEX_GIT_SHA");
pub const GIT_BRANCH: &str = env!("CRABINDEX_GIT_BRANCH");
pub const BUILD_DATE: &str = env!("CRABINDEX_BUILD_DATE");

pub fn print_banner() {
    let line = "═══════════════════════════════════════════════════════════";
    println!("{line}");
    println!("  CrabIndex - Torrent Aggregator & File Database");
    println!("{line}");
    println!("  Version:     {VERSION}");
    println!("  Git SHA:     {GIT_SHA}");
    println!("  Git Branch:  {GIT_BRANCH}");
    println!("  Build Date:  {BUILD_DATE}");
    println!("{line}");
    println!();
}
