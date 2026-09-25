//! Headless FileDB integrity CLI: `crabindex maintain [--mode=report|safe|full] [--sample-size=20] [--include-numeric-xx]`.
//!
//! Exit codes: 0 ok (or `--help`), 1 report finished with errors, 2 bad arguments, 130 cancelled (Ctrl+C).

use crab_core::log::{self, cat};
use tokio_util::sync::CancellationToken;

use crate::maintenance;

#[derive(Debug, PartialEq, Eq)]
pub struct MaintainArgs {
    pub mode: String,
    pub sample_size: i32,
    pub exclude_numeric_xx: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseOutcome {
    Run(MaintainArgs),
    Help,
    Error(String),
}

/// Parse options after the leading `maintain` verb (`args[0]`).
pub fn parse_args(args: &[String]) -> ParseOutcome {
    let mut out = MaintainArgs { mode: "report".into(), sample_size: 20, exclude_numeric_xx: true };
    for a in args.iter().skip(1) {
        let lower = a.to_lowercase();
        if lower == "--help" || lower == "-h" {
            return ParseOutcome::Help;
        }
        if lower.starts_with("--mode=") {
            let mode = a["--mode=".len()..].trim().to_string();
            if !matches!(mode.as_str(), "report" | "safe" | "full") {
                return ParseOutcome::Error(format!("Unknown mode '{mode}'. Use report, safe, or full."));
            }
            out.mode = mode;
            continue;
        }
        if lower.starts_with("--sample-size=") {
            match a["--sample-size=".len()..].trim().parse::<i32>() {
                Ok(n) if n >= 1 => out.sample_size = n,
                _ => return ParseOutcome::Error("Invalid --sample-size (positive integer expected).".into()),
            }
            continue;
        }
        if lower == "--include-numeric-xx" {
            out.exclude_numeric_xx = false;
            continue;
        }
        return ParseOutcome::Error(format!("Unknown argument: {a}"));
    }
    ParseOutcome::Run(out)
}

const RULE: &str = "═══════════════════════════════════════════════════════════";

fn print_banner() {
    println!("{RULE}");
    println!("  CrabIndex maintain - offline FDB integrity");
    println!("{RULE}");
    println!("  Version:     {}", env!("CARGO_PKG_VERSION"));
    println!("  Git SHA:     {}", option_env!("GIT_SHA").unwrap_or("unknown"));
    println!("{RULE}");
    println!();
}

fn print_usage() {
    println!();
    println!("Usage:");
    println!("  crabindex maintain [--mode=report|safe|full] [--sample-size=20] [--include-numeric-xx]");
    println!();
    println!("  Run from the install directory (where Data/ lives).");
    println!("  Default mode=report (read-only). Report: Data/temp/maintenance-last.json");
}

pub fn run(args: &[String]) -> i32 {
    print_banner();
    let a = match parse_args(args) {
        ParseOutcome::Run(a) => a,
        ParseOutcome::Help => {
            print_usage();
            return 0;
        }
        ParseOutcome::Error(e) => {
            eprintln!("{e}");
            print_usage();
            return 2;
        }
    };

    crab_core::ensure_data_dirs();
    // load config so the FileDB path layout (fdbPathLevels) is right; initialising via
    // refresh_if_changed first avoids re-entering conf() while its first load logs the config
    crab_core::config::refresh_if_changed(None);
    let _ = crab_core::conf();

    let ct = CancellationToken::new();
    {
        let ct = ct.clone();
        std::thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            rt.block_on(async {
                if tokio::signal::ctrl_c().await.is_ok() {
                    println!();
                    println!("[maintain] Ctrl+C - cancelling…");
                    ct.cancel();
                }
            });
        });
    }

    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    println!("[maintain] cwd={cwd}");
    println!("[maintain] mode={} sampleSize={} excludeNumericXx={}", a.mode, a.sample_size, if a.exclude_numeric_xx { "True" } else { "False" });
    println!("[maintain] Tip: stop the server before safe/full so only this process touches Data/.");
    println!();

    match maintenance::run(&a.mode, a.sample_size, a.exclude_numeric_xx, &ct, true) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(_) => {
            log::warn(cat::FDB, "maintain CLI cancelled");
            130
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults() {
        assert_eq!(
            parse_args(&args(&["maintain"])),
            ParseOutcome::Run(MaintainArgs { mode: "report".into(), sample_size: 20, exclude_numeric_xx: true })
        );
    }

    #[test]
    fn all_options() {
        assert_eq!(
            parse_args(&args(&["maintain", "--MODE=full", "--sample-size=5", "--include-numeric-xx"])),
            ParseOutcome::Run(MaintainArgs { mode: "full".into(), sample_size: 5, exclude_numeric_xx: false })
        );
    }

    #[test]
    fn errors_and_help() {
        assert_eq!(parse_args(&args(&["maintain", "-h"])), ParseOutcome::Help);
        assert_eq!(
            parse_args(&args(&["maintain", "--mode=wipe"])),
            ParseOutcome::Error("Unknown mode 'wipe'. Use report, safe, or full.".into())
        );
        assert!(matches!(parse_args(&args(&["maintain", "--sample-size=0"])), ParseOutcome::Error(_)));
        assert_eq!(parse_args(&args(&["maintain", "--x"])), ParseOutcome::Error("Unknown argument: --x".into()));
    }
}
