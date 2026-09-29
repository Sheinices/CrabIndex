#!/usr/bin/env bash
# Bump the workspace version and keep Cargo.lock in step, so `cargo ... --locked` in CI and the
# release build do not fail on a stale lock file.
#
#   scripts/bump-version.sh 1.2.3        # set version, refresh Cargo.lock, verify --locked
#   scripts/bump-version.sh 1.2.3 --tag  # ...and create the git tag v1.2.3 (after you commit)
#
# Only the workspace members are touched in Cargo.lock (`cargo update --workspace`), third-party
# dependencies stay pinned; no network access is needed.
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:-}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: $0 X.Y.Z [--tag]" >&2
  exit 2
fi

current=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
if [[ "$current" == "$version" ]]; then
  echo "Cargo.toml already at $version"
else
  # first `version = "..."` in Cargo.toml is [workspace.package]
  sed -i.bak "0,/^version = \"$current\"/s//version = \"$version\"/" Cargo.toml && rm -f Cargo.toml.bak
  echo "Cargo.toml: $current -> $version"
fi

# refresh the workspace members' entries in Cargo.lock without touching dependencies
cargo update --workspace --quiet
# prove the lock file is consistent the way CI checks it
cargo metadata --locked --format-version 1 >/dev/null
echo "Cargo.lock: in step with $version"

if [[ "${2:-}" == "--tag" ]]; then
  if [[ -n "$(git status --porcelain Cargo.toml Cargo.lock)" ]]; then
    echo "commit Cargo.toml and Cargo.lock first, then tag" >&2
    exit 1
  fi
  git tag "v$version"
  echo "tag v$version created: git push origin main v$version"
else
  echo "next: git add Cargo.toml Cargo.lock && git commit -m \"v$version\" && git tag v$version && git push origin main v$version"
fi
