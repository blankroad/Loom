#!/usr/bin/env bash
# Read-only feasibility check for building/validating the Loom desktop app on this Mac.
# Installs nothing, changes no settings. Exit 0 = buildable, 1 = a hard requirement is missing.
#   desktop/scripts/preflight.sh [--json]
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
TOOLS="${LOOM_TAURI_TOOLS:-/tmp/loom-tauri-build/tools}"
TAURI_CLI_VERSION="${TAURI_CLI_VERSION:-2.12.4}"
rows=() hard=0
row() { rows+=("$1|$2|$3"); [[ "$2" == FAIL ]] && hard=1; return 0; }
ver_ge() { [[ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -1)" == "$2" ]]; }

[[ "$(uname -s)" == Darwin ]] && row macos PASS "$(sw_vers -productVersion) $(uname -m)" || row macos FAIL "not macOS"
xcode-select -p >/dev/null 2>&1 && row xcode-clt PASS "$(xcode-select -p)" || row xcode-clt FAIL "xcode-select --install (user action)"
rv="$(rustc --version 2>/dev/null | awk '{print $2}')"
if [[ -n "$rv" ]] && ver_ge "$rv" 1.93.0; then row rustc PASS "$rv ($(rustc --print host-tuple 2>/dev/null))"; else row rustc FAIL "need >=1.93 (have ${rv:-none})"; fi
nv="$(node --version 2>/dev/null | tr -d v)"
if [[ -n "$nv" ]] && ver_ge "$nv" 24.0.0; then row node PASS "$nv"; else row node WARN "ui engines want >=24 (have ${nv:-none}); only needed to rebuild ui/dist or fetch tauri cli"; fi
command -v pnpm >/dev/null && row pnpm PASS "$(pnpm --version)" || row pnpm WARN "only needed to rebuild ui/dist"
command -v python3 >/dev/null && row python3 PASS "$(python3 --version 2>&1)" || row python3 FAIL "scripts need python3"
if [[ -x "$TOOLS/node_modules/.bin/tauri" ]]; then
  row tauri-cli PASS "$("$TOOLS/node_modules/.bin/tauri" --version 2>/dev/null) at $TOOLS (task-local)"
else
  row tauri-cli WARN "not cached; build-app.sh installs @tauri-apps/cli@$TAURI_CLI_VERSION into $TOOLS (task-local, no global)"
fi
[[ -f "$repo/ui/dist/index.html" ]] && row ui-dist PASS "present" || row ui-dist WARN "missing; stage-loomd.sh will pnpm build"
if out="$("$here/stage-loomd.sh" --check 2>&1)"; then row staged-loomd PASS "$out"; else row staged-loomd WARN "$(tr '\n' ';' <<<"$out")"; fi
free_gb="$(df -g /tmp | awk 'NR==2{print $4}')"
[[ "${free_gb:-0}" -ge 10 ]] && row disk-tmp PASS "${free_gb}G free" || row disk-tmp WARN "${free_gb}G free on /tmp"
for t in ffmpeg sigrok-cli; do
  p="$(command -v "$t" 2>/dev/null)"
  [[ -n "$p" ]] && row "ext-$t" INFO "$p (Finder-launched app PATH is minimal; see README)" || row "ext-$t" INFO "absent (video/logic engines unavailable; not bundled)"
done
if lsof -nP -iTCP:47123 -sTCP:LISTEN >/dev/null 2>&1; then row port-47123 INFO "in use (another daemon, e.g. user's ~/.loom); isolated runs use own LOOM_HOME"; else row port-47123 INFO free; fi
ugd="$(pgrep -x loomd 2>/dev/null | wc -l | tr -d ' ')"; row user-daemons INFO "$ugd loomd process(es) running — never touched by these scripts"

if [[ "${1:-}" == --json ]]; then
  printf '%s\n' "${rows[@]}" | python3 -I -c 'import json,sys;print(json.dumps([dict(zip(("check","result","detail"),l.rstrip("\n").split("|",2))) for l in sys.stdin],indent=1))'
else
  printf '%s\n' "${rows[@]}" | awk -F'|' '{printf "%-14s %-5s %s\n",$1,$2,$3}'
fi
exit $hard
