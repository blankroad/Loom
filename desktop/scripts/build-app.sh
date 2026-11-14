#!/usr/bin/env bash
# Build the Loom.app bundle with the verified staged loomd.
#
#   desktop/scripts/build-app.sh [--debug|--release] [--stage-from <loomd> | --stage-profile debug|release]
#
# 1. Staging: by default REQUIRE `stage-loomd.sh --check` to pass (verified embedded UI).
#    --stage-from / --stage-profile run stage-loomd.sh first (it re-verifies).
# 2. Tauri CLI pinned @tauri-apps/cli@$TAURI_CLI_VERSION: desktop/node_modules (desktop/package.json) when
#    it matches, else a task-local prefix ($LOOM_TAURI_TOOLS, default /tmp/loom-tauri-build/tools); never global.
# 3. `tauri build [--debug] --bundles app --ci` with CARGO_TARGET_DIR=/tmp/loom-tauri-build, under a
#    mkdir lock (/tmp/loom-tauri-build/.cargo-desktop.lock) so desktop-workspace cargo runs never overlap.
#    cargo runs with `-- --locked`; a missing or changed desktop/Cargo.lock is a hard failure.
#    --features F --out PATH: test-only builds (e.g. qa-webdriver) are copied out of bundle/macos.
# 4. verify-bundle.sh on the produced .app. Prints the .app path last.
# Default label is --debug (dev build: debug Rust, devtools per app config); --release for an optimized bundle.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
TOOLS="${LOOM_TAURI_TOOLS:-/tmp/loom-tauri-build/tools}"
TAURI_CLI_VERSION="${TAURI_CLI_VERSION:-2.12.4}"
DESKTOP_TARGET="${LOOM_DESKTOP_TARGET_DIR:-/tmp/loom-tauri-build}"
mode=debug stage_from="" stage_profile="" features="" out=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug) mode=debug; shift ;;
    --release) mode=release; shift ;;
    --stage-from) stage_from="$2"; shift 2 ;;
    --stage-profile) stage_profile="$2"; shift 2 ;;
    --features) features="$2"; shift 2 ;;   # e.g. qa-webdriver (test-only; requires --out)
    --out) out="$2"; shift 2 ;;             # copy the bundle here (QA builds must not stay in bundle/macos)
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "build-app: unknown argument $1" >&2; exit 2 ;;
  esac
done

if [[ -n "$stage_from" ]]; then "$here/stage-loomd.sh" --from "$stage_from"
elif [[ -n "$stage_profile" ]]; then "$here/stage-loomd.sh" --profile "$stage_profile"
fi
"$here/stage-loomd.sh" --check || { echo "build-app: staged loomd is not current+verified; run stage-loomd.sh" >&2; exit 1; }

tauri="$repo/desktop/node_modules/.bin/tauri"   # app owner's pinned devDependency, if installed
[[ -x "$tauri" && "$("$tauri" --version 2>/dev/null)" == "tauri-cli $TAURI_CLI_VERSION" ]] || tauri="$TOOLS/node_modules/.bin/tauri"
if [[ ! -x "$tauri" || "$("$tauri" --version 2>/dev/null)" != "tauri-cli $TAURI_CLI_VERSION" ]]; then
  echo "build-app: installing @tauri-apps/cli@$TAURI_CLI_VERSION into $TOOLS (task-local)"
  mkdir -p "$TOOLS"
  npm install --prefix "$TOOLS" --no-save --no-audit --no-fund --loglevel=error "@tauri-apps/cli@$TAURI_CLI_VERSION"
fi
echo "build-app: $("$tauri" --version)"

export CARGO_TARGET_DIR="$DESKTOP_TARGET"   # only after staging (main workspace keeps repo/target)
lock="$CARGO_TARGET_DIR/.cargo-desktop.lock"
mkdir -p "$CARGO_TARGET_DIR"
got=0
for _ in $(seq 600); do if mkdir "$lock" 2>/dev/null; then got=1; break; fi; sleep 1; done
[[ $got -eq 1 ]] || { echo "build-app: lock $lock held >10 min by $(cat "$lock/owner" 2>/dev/null || echo unknown)" >&2; exit 1; }
echo "build-app pid $$ $(date -u +%FT%TZ)" >"$lock/owner"
trap 'rm -rf "$lock"' EXIT

lockfile="$repo/desktop/Cargo.lock"
before="$( [[ -f "$lockfile" ]] && shasum -a 256 "$lockfile" | cut -c1-64 || echo none)"
args=(build --bundles app --ci); [[ "$mode" == debug ]] && args+=(--debug)
[[ -n "$features" ]] && args+=(--features "$features")
[[ -f "$lockfile" ]] || { echo "build-app: desktop/Cargo.lock missing (app owner creates it); refusing to resolve" >&2; exit 1; }
args+=(-- --locked)
echo "build-app: tauri ${args[*]} (CARGO_TARGET_DIR=$CARGO_TARGET_DIR)"
(cd "$repo/desktop/src-tauri" && "$tauri" "${args[@]}")
after="$( [[ -f "$lockfile" ]] && shasum -a 256 "$lockfile" | cut -c1-64 || echo none)"
[[ "$before" == "$after" ]] || { echo "build-app: FAIL desktop/Cargo.lock changed during build" >&2; exit 1; }

app="$CARGO_TARGET_DIR/$mode/bundle/macos/Loom.app"
[[ -d "$app" ]] || app="$(ls -d "$CARGO_TARGET_DIR/$mode"/bundle/macos/*.app 2>/dev/null | head -1)"
[[ -d "$app" ]] || { echo "build-app: no .app produced under $CARGO_TARGET_DIR/$mode/bundle/macos" >&2; exit 1; }
if [[ -n "$features" && -z "$out" ]]; then echo "build-app: --features needs --out (keep test builds separate)" >&2; exit 2; fi
if [[ -n "$out" ]]; then
  rm -rf "$out"; mkdir -p "$(dirname "$out")"; ditto "$app" "$out"; app="$out"
fi
if [[ "$features" == *e2e* || "$features" == *qa-webdriver* ]]; then QA_ALLOW_WEBDRIVER=1 "$here/verify-bundle.sh" "$app"; else "$here/verify-bundle.sh" "$app"; fi
echo "$app"
