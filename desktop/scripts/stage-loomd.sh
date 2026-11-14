#!/usr/bin/env bash
# Stage the exact loomd (with the current UI embedded) as the Tauri externalBin sidecar.
#
#   desktop/scripts/stage-loomd.sh --from <existing-loomd>      # dev: stage a prebuilt binary, no cargo
#   desktop/scripts/stage-loomd.sh [--profile release|debug] [--force-ui]   # build path
#   desktop/scripts/stage-loomd.sh --check
#
# After staging, verify-loomd.sh runs automatically (STAGE_SKIP_VERIFY=1 to skip) and a stale
# embedded UI fails the stage.
#
# --from: no ui build, no cargo. The binary is copied with provenance (source path, mtime, sha256)
#   and labeled "prebuilt"; its embedded UI identity is NOT assumed — verify-loomd.sh proves it by
#   serving /ui from an isolated LOOM_HOME and comparing bytes with ui/dist.
#
# 1. Rebuild ui/dist only when an input is newer than ui/dist/index.html (or --force-ui).
# 2. `cargo build --locked -p loom-gateway --bin loomd` in the MAIN workspace (its own target dir,
#    never mutates Cargo.lock). loom-ui-assets embeds ui/dist at compile time (rust-embed), so when
#    the dist digest changed since the last stage, only that crate is cleaned to force re-embedding.
# 3. Copy to desktop/src-tauri/binaries/loomd-<host-triple> (Tauri `bundle.externalBin:
#    ["binaries/loomd"]` → `<App>.app/Contents/MacOS/loomd`) and write loomd.stamp.json next to it.
#
# --check: only report whether staging is current (exit 0) or stale (exit 3); builds nothing.
# Workers need no extra binaries: loomd re-executes itself as `loomd worker <kind>`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
profile=release force_ui=0 check=0 from=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile) profile="$2"; shift 2 ;;
    --force-ui) force_ui=1; shift ;;
    --check) check=1; shift ;;
    --from) from="$2"; shift 2 ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "stage-loomd: unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ "$profile" == release || "$profile" == debug ]] || { echo "stage-loomd: profile must be release|debug" >&2; exit 2; }

triple="$(rustc --print host-tuple 2>/dev/null || rustc -Vv | awk '/^host:/{print $2}')"
out_dir="$repo/desktop/src-tauri/binaries"
staged="$out_dir/loomd-$triple"
stamp="$out_dir/loomd.stamp.json"
ui="$repo/ui"
dist_index="$ui/dist/index.html"

ui_inputs() { # files whose change requires `pnpm build`
  find "$ui/src" "$ui/public" "$ui/index.html" "$ui/package.json" "$ui/pnpm-lock.yaml" \
       "$ui/vite.config.ts" "$ui/tsconfig.json" "$ui/tsconfig.app.json" "$ui/tsconfig.node.json" \
       -type f 2>/dev/null
}
ui_stale() {
  [[ -f "$dist_index" ]] || return 0
  [[ -n "$(ui_inputs | xargs -I{} find {} -newer "$dist_index" 2>/dev/null | head -1)" ]]
}
dist_digest() { # order-stable digest of every embedded file (path + content)
  (cd "$ui/dist" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256) | shasum -a 256 | cut -c1-64
}
stamp_field() { python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1])).get(sys.argv[2],""))' "$stamp" "$1" 2>/dev/null || true; }

if [[ $check -eq 1 ]]; then
  reasons=()
  ui_stale && reasons+=("ui/dist older than ui sources")
  [[ -x "$staged" ]] || reasons+=("missing $staged")
  if [[ -x "$staged" && -f "$stamp" ]]; then
    [[ "$(shasum -a 256 "$staged" | cut -c1-64)" == "$(stamp_field loomd_sha256)" ]] || reasons+=("staged binary != stamp")
    [[ -f "$dist_index" && "$(dist_digest)" == "$(stamp_field ui_dist_digest)" ]] || reasons+=("ui/dist changed since stage")
    [[ "$(stamp_field embedded_ui_verified)" == True ]] || reasons+=("embedded UI not verified (run verify-loomd.sh)")
  else
    reasons+=("missing stamp")
  fi
  if [[ ${#reasons[@]} -eq 0 ]]; then echo "stage-loomd: current ($staged)"; exit 0; fi
  printf 'stage-loomd: stale: %s\n' "${reasons[@]}"; exit 3
fi

if [[ -n "$from" ]]; then
  [[ -x "$from" && -f "$from" ]] || { echo "stage-loomd: --from $from is not an executable file" >&2; exit 2; }
  built="$(cd "$(dirname "$from")" && pwd)/$(basename "$from")"
  profile="prebuilt"
  [[ -f "$dist_index" ]] || { echo "stage-loomd: ui/dist missing; cannot record UI identity" >&2; exit 1; }
  ui_stale && echo "stage-loomd: WARNING ui sources newer than ui/dist (the prebuilt binary may embed an older UI)" >&2
  digest="$(dist_digest)"
else
if [[ $force_ui -eq 1 ]] || ui_stale; then
  echo "stage-loomd: building ui/dist (pnpm build)"
  [[ -d "$ui/node_modules" ]] || pnpm --dir "$ui" install --frozen-lockfile
  pnpm --dir "$ui" build
else
  echo "stage-loomd: ui/dist is current; not rebuilding"
fi
[[ -f "$dist_index" ]] || { echo "stage-loomd: ui/dist/index.html missing after build" >&2; exit 1; }
digest="$(dist_digest)"

cargo_profile=(); [[ "$profile" == release ]] && cargo_profile=(--release)
# rust-embed does not notice files added/removed in ui/dist (hashed asset names), so a cached
# loom-ui-assets can silently embed an OLD UI (observed 2026-11-15 with target/debug/loomd).
# Cleaning only that crate is cheap (it + loom-gateway recompile; nothing else).
echo "stage-loomd: cleaning loom-ui-assets ($profile) to force re-embedding ui/dist $digest"
(cd "$repo" && env -u CARGO_TARGET_DIR cargo clean --quiet -p loom-ui-assets ${cargo_profile[@]+"${cargo_profile[@]}"})
echo "stage-loomd: cargo build --locked ${cargo_profile[*]-} -p loom-gateway --bin loomd"
(cd "$repo" && env -u CARGO_TARGET_DIR cargo build --locked ${cargo_profile[@]+"${cargo_profile[@]}"} -p loom-gateway --bin loomd)

target_dir="$(cd "$repo" && env -u CARGO_TARGET_DIR cargo metadata --format-version 1 --no-deps --locked | python3 -I -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')"
built="$target_dir/$profile/loomd"
[[ -x "$built" ]] || { echo "stage-loomd: $built not built" >&2; exit 1; }
fi

mkdir -p "$out_dir"
tmp="$staged.tmp.$$"
cp "$built" "$tmp" && chmod 0755 "$tmp" && mv -f "$tmp" "$staged"
sha="$(shasum -a 256 "$staged" | cut -c1-64)"
version="$("$staged" --version 2>/dev/null | head -1 || true)"
base="$(git -C "$repo" rev-parse --short HEAD 2>/dev/null || echo unknown)"
src_mtime="$(stat -f %Sm -t %Y-%m-%dT%H:%M:%S "$built")"
ui_state=current; ui_stale && ui_state=stale_vs_sources
python3 -I - "$stamp" "$sha" "$digest" "$profile" "$triple" "$version" "$base" "$built" "$src_mtime" "$ui_state" <<'PY'
import json, sys, datetime
path, sha, digest, profile, triple, version, base, src, mtime, ui_state = sys.argv[1:]
json.dump({"loomd_sha256": sha, "ui_dist_digest": digest, "profile": profile, "target": triple,
           "loomd_version": version, "baseline": base, "source_path": src, "source_mtime": mtime,
           "ui_dist_vs_sources": ui_state, "embedded_ui_verified": False,
           "staged_at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")},
          open(path, "w"), indent=2)
PY
echo "stage-loomd: staged $staged (sha256 $sha, ui $digest)"
if [[ "${STAGE_SKIP_VERIFY:-0}" != 1 ]]; then
  "$here/verify-loomd.sh" "$staged" || { echo "stage-loomd: staged binary FAILED embedded-UI identity check" >&2; exit 1; }
fi
