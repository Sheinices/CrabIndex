//! Embeds version information into the binary.
//!
//! Exposed as compile-time env vars: `CRABINDEX_VERSION`, `CRABINDEX_GIT_SHA`,
//! `CRABINDEX_GIT_BRANCH`, `CRABINDEX_BUILD_DATE`.
//! Each can be overridden by setting the same variable in the build environment
//! (useful for Docker builds without a `.git` directory).
//!
//! Git data comes from the `git` binary when available, otherwise the `.git` directory is
//! read directly (HEAD, loose refs, packed-refs), so servers without git still get the
//! commit. Without any git data the version is `<crate version>-dev` and the sha is empty.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// `.git` directory of the repository containing this crate (supports `gitdir:` files).
fn find_git_dir() -> Option<PathBuf> {
    let mut dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?);
    loop {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).ok()?;
            let rel = text.trim().strip_prefix("gitdir:")?.trim();
            let p = Path::new(rel);
            return Some(if p.is_absolute() { p.to_path_buf() } else { dir.join(p) });
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Resolve a ref (`refs/heads/main`) to a full sha via loose refs or packed-refs.
fn resolve_ref(git_dir: &Path, name: &str) -> Option<String> {
    if let Some(sha) = read_trim(&git_dir.join(name)) {
        return Some(sha);
    }
    let packed = std::fs::read_to_string(git_dir.join("packed-refs")).ok()?;
    packed.lines().find_map(|l| {
        let (sha, r) = l.split_once(' ')?;
        (r == name).then(|| sha.to_string())
    })
}

/// (full sha, branch) from the `.git` directory.
fn head_from_files(git_dir: &Path) -> Option<(String, String)> {
    let head = read_trim(&git_dir.join("HEAD"))?;
    match head.strip_prefix("ref:") {
        Some(r) => {
            let r = r.trim();
            let sha = resolve_ref(git_dir, r)?;
            let branch = r.strip_prefix("refs/heads/").unwrap_or(r).to_string();
            Some((sha, branch))
        }
        None => Some((head, "HEAD".into())),
    }
}

/// A tag pointing exactly at `sha` (loose refs and packed-refs, incl. peeled annotated tags).
fn tag_at_from_files(git_dir: &Path, sha: &str) -> Option<String> {
    if let Ok(rd) = std::fs::read_dir(git_dir.join("refs/tags")) {
        for e in rd.flatten() {
            if read_trim(&e.path()).as_deref() == Some(sha) {
                return Some(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    let packed = std::fs::read_to_string(git_dir.join("packed-refs")).ok()?;
    let mut last_tag: Option<String> = None;
    for l in packed.lines() {
        if let Some(peeled) = l.strip_prefix('^') {
            if peeled == sha {
                return last_tag;
            }
            continue;
        }
        let Some((s, r)) = l.split_once(' ') else { continue };
        last_tag = r.strip_prefix("refs/tags/").map(|t| t.to_string());
        if s == sha && last_tag.is_some() {
            return last_tag;
        }
    }
    None
}

fn sanitize_branch(b: &str) -> String {
    b.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect()
}

fn compute_version(sha: &str, branch: &str, exact_tag: Option<String>) -> String {
    if let Some(tag) = exact_tag {
        return tag.trim_start_matches('v').to_string();
    }
    let pkg = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let suffix = if sha.is_empty() { String::new() } else { format!("+{sha}") };
    if let Some(latest) = git(&["describe", "--tags", "--abbrev=0"]) {
        return format!("{}-next{suffix}", latest.trim_start_matches('v'));
    }
    if !branch.is_empty() && branch != "HEAD" && branch != "main" && branch != "master" {
        format!("{pkg}-dev.{}{suffix}", sanitize_branch(branch))
    } else {
        format!("{pkg}-dev{suffix}")
    }
}

fn env_override(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={name}");
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let git_dir = git(&["rev-parse", "--absolute-git-dir"]).map(PathBuf::from).or_else(find_git_dir);
    if let Some(dir) = &git_dir {
        for f in ["HEAD", "packed-refs", "refs/tags", "refs/heads"] {
            let p = dir.join(f);
            if p.exists() {
                println!("cargo:rerun-if-changed={}", p.display());
            }
        }
    }

    // full sha + branch: git binary first, then the .git files
    let from_files = git_dir.as_deref().and_then(head_from_files);
    let full_sha = git(&["rev-parse", "HEAD"]).or_else(|| from_files.as_ref().map(|x| x.0.clone()));
    let sha = env_override("CRABINDEX_GIT_SHA")
        .or_else(|| full_sha.as_ref().map(|s| s.chars().take(8).collect()))
        .unwrap_or_default();
    let branch = env_override("CRABINDEX_GIT_BRANCH")
        .or_else(|| git(&["rev-parse", "--abbrev-ref", "HEAD"]))
        .or_else(|| from_files.as_ref().map(|x| x.1.clone()))
        .unwrap_or_default();
    let exact_tag = git(&["describe", "--tags", "--exact-match", "HEAD"]).or_else(|| {
        let dir = git_dir.as_deref()?;
        tag_at_from_files(dir, full_sha.as_deref()?)
    });
    let build_date = env_override("CRABINDEX_BUILD_DATE")
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string());
    let version = env_override("CRABINDEX_VERSION").unwrap_or_else(|| compute_version(&sha, &branch, exact_tag));

    println!("cargo:rustc-env=CRABINDEX_VERSION={version}");
    println!("cargo:rustc-env=CRABINDEX_GIT_SHA={sha}");
    println!("cargo:rustc-env=CRABINDEX_GIT_BRANCH={branch}");
    println!("cargo:rustc-env=CRABINDEX_BUILD_DATE={build_date}");
}
