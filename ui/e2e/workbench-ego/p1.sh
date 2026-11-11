#!/bin/zsh
# Canonical P1 real-smoke (5 tests, config unmodified) against the repo ui/dist, which must be
# byte-identical to the frozen snapshot <label>. Never overwrites earlier evidence.
# usage: ui/e2e/workbench-ego/p1.sh <build-label> <tc1_live_smoke-bin>
# exit: Playwright's exit code (0 = 5/5 expected), 2 = usage/precondition.
set -u
root="${0:A:h:h:h:h}"
cache="${LOOM_WB_CACHE:-$HOME/.cache/loom-ui-workbench}"
label="${1:-}"; tc1="${2:-}"
[[ "$label" =~ '^[a-z0-9][a-z0-9-]{0,31}$' && -d "$cache/$label/dist" ]] || { echo "unknown frozen build: '$label'" >&2; exit 2; }
[[ -x "$tc1" ]] || { echo "TC-1 test binary not executable: '$tc1'" >&2; exit 2; }
diff -rq "$root/ui/dist" "$cache/$label/dist" >/dev/null || { echo "repo ui/dist differs from frozen '$label' snapshot" >&2; exit 2; }
ev="$root/docs/impl/main/evidence/ui-workbench/p1-suite-$label"
[[ -e "$ev" ]] && { echo "evidence exists, refusing to overwrite: '$ev'" >&2; exit 2; }
mkdir -p "$ev"
shasum -a 256 "$cache/$label/loomd" "$tc1" "$cache/$label/dist/assets/"*.js > "$ev/inputs.sha256"
cd "$root/ui"
LOOM_REAL_SMOKE=1 LOOMD_BIN="$cache/$label/loomd" LOOM_TC1_TEST_BIN="$tc1" \
  PLAYWRIGHT_JSON_OUTPUT_NAME="$ev/pw-results.json" \
  pnpm exec playwright test -c e2e/real-smoke.config.ts --reporter=list,json --output="$ev/pw-out" > "$ev/pw-run.log" 2>&1
rc=$?
echo "exit=$rc" >> "$ev/pw-run.log"
grep -E '✓|✘|passed|failed|skipped|flaky' "$ev/pw-run.log"
exit $rc
