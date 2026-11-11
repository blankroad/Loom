#!/bin/zsh
# Freeze a build for a reproducible Ego run (non-destructive: refuses an existing snapshot).
# usage: freeze.sh <build-label> <dist-dir> <loomd-bin>
set -eu
label="${1:-}"; dist="${2:-}"; loomd="${3:-}"
[[ "$label" =~ '^[a-z0-9][a-z0-9-]{0,31}$' ]] || { echo "invalid build label: '$label'" >&2; exit 2; }
[[ -f "$dist/index.html" && -d "$dist/assets" ]] || { echo "not a built UI dist: '$dist'" >&2; exit 2; }
[[ -f "$loomd" && -x "$loomd" ]] || { echo "loomd not executable: '$loomd'" >&2; exit 2; }
cache="${LOOM_WB_CACHE:-$HOME/.cache/loom-ui-workbench}"
dest="$cache/$label"
[[ -e "$dest" ]] && { echo "snapshot exists, refusing to overwrite: '$dest' (use a new label)" >&2; exit 3; }
mkdir -p "$cache"
mkdir "$dest"
cp -R "$dist" "$dest/dist"
cp "$loomd" "$dest/loomd"
cd "$dest" && shasum -a 256 loomd dist/index.html dist/assets/*.js dist/assets/*.css
