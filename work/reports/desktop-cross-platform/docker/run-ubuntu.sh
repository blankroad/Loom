#!/usr/bin/env bash
# Host wrapper: full Ubuntu 22.04 desktop package build in Docker, isolated from the macOS tree.
#   work/reports/desktop-cross-platform/docker/run-ubuntu.sh [build]    # build image if missing, then full run
#   work/reports/desktop-cross-platform/docker/run-ubuntu.sh smoke      # same built .deb, fresh container: install+start+survival smoke
#   work/reports/desktop-cross-platform/docker/run-ubuntu.sh compare    # snapshot drift vs current tree only
# - image: loom-ci-ubuntu2204:tauri (toolchain only, from an EMPTY build context)
# - repo: bind-mounted READ-ONLY at /source (never written; the container works on a snapshot)
# - cache: named volume loom-cross-ubuntu-root-check-cache (/cache: cargo home, targets, snapshot, npm/pnpm)
# - out: /tmp/loom-ci-ubuntu/out (installers, artifacts.json, per-step logs, steps.tsv)
# Uses its own --rm container name. It never touches other containers, images or volumes.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
img=loom-ci-ubuntu2204:tauri vol=loom-cross-ubuntu-root-check-cache out=/tmp/loom-ci-ubuntu/out
if ! docker image inspect "$img" >/dev/null 2>&1; then
  ctx=$(mktemp -d); cp "$here/Dockerfile.ubuntu2204" "$ctx/Dockerfile"; docker build -t "$img" "$ctx"; rm -rf "$ctx"
fi
mode=${1:-build}   # build | smoke (reinstall the built .deb in a fresh container, no rebuild) | compare (drift only)
[[ $mode == build || $mode == smoke || $mode == compare ]] || { echo "usage: $0 [build|smoke|compare]" >&2; exit 2; }
[[ $mode == build ]] && { rm -rf "$out"; mkdir -p "$out"; }
exec docker run --rm --name "loom-ci-ubuntu-$mode" \
  -v "$repo:/source:ro" -v "$vol:/cache" -v "$out:/out" -v "$here/ubuntu-build.sh:/ubuntu-build.sh:ro" \
  "$img" bash /ubuntu-build.sh "$mode"
