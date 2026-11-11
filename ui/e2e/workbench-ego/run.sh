#!/bin/zsh
# UI-workbench Ego acceptance runner (ui-qa). Foreground; owns one harness + one loomd and always
# cleans them up (EXIT/INT/TERM trap, including startup failure).
# usage: ui/e2e/workbench-ego/run.sh <build-label> <scenario>
#   needs a frozen build at $LOOM_WB_CACHE/<label>/{dist/,loomd} (see freeze.sh)
#   evidence -> docs/impl/main/evidence/ui-workbench/<label>/ ; `#boot=` fragments are redacted.
# exit: 0 = every scenario RESULT PASS; 1 = a scenario FAIL / browser error; 2 = usage; 3 = harness startup.
set -u
root="${0:A:h:h:h:h}"; here="${0:A:h}"
cache="${LOOM_WB_CACHE:-$HOME/.cache/loom-ui-workbench}"
build="${1:-}"; sc="${2:-}"
[[ "$build" =~ '^[a-z0-9][a-z0-9-]{0,31}$' ]] || { echo "invalid build label: '$build'" >&2; exit 2; }
[[ "$sc" =~ '^[a-z0-9][a-z0-9-]{0,31}$' && -f "$here/$sc.body.mjs" ]] || { echo "unknown scenario: '$sc'" >&2; exit 2; }
[[ -x "$cache/$build/loomd" && -f "$cache/$build/dist/index.html" ]] || { echo "missing frozen build: '$cache/$build'" >&2; exit 2; }
ev="$root/docs/impl/main/evidence/ui-workbench/$build"; mkdir -p "$ev"
tmp="$(mktemp -d)"; chmod 700 "$tmp"; ctl="$tmp/ctl.json"
stamp="$(date +%H%M%S)"; out="$ev/$sc-$stamp.out"; hlog="$ev/harness-$sc-$stamp.log"
h=""
cleanup() {
  if [[ -n "$h" ]] && kill -0 "$h" 2>/dev/null; then
    kill -TERM "$h" 2>/dev/null
    for _ in {1..150}; do kill -0 "$h" 2>/dev/null || break; sleep 0.1; done
    kill -0 "$h" 2>/dev/null && { echo "harness $h did not exit; SIGKILL" >&2; kill -KILL "$h" 2>/dev/null; }
  fi
  rm -f "$ctl"; rmdir "$tmp" 2>/dev/null
  echo "--- harness log ($hlog)"; cat "$hlog" 2>/dev/null
  echo "--- leftovers:"; pgrep -fl "$cache/$build/loomd|workbench-harness" || echo none
  [[ -e "$ctl" ]] && echo "ctl still present" || echo "ctl removed"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
cd "$root/ui"
node --import tsx e2e/workbench-harness.ts "$ctl" "$cache/$build/dist" "$cache/$build/loomd" > "$hlog" 2>&1 &
h=$!
for _ in {1..100}; do [[ -f "$ctl" ]] && break; kill -0 "$h" 2>/dev/null || break; sleep 0.1; done
[[ -f "$ctl" ]] || { echo "harness startup failed" >&2; exit 3; }
{ printf 'const BUILD=%s, CTL=%s, S=%s;\n' "\"$build\"" "\"$ctl\"" "\"$ev\""; cat "$here/prelude.mjs" "$here/$sc.body.mjs"; } \
  | ego-browser nodejs 2>&1 | sed -E 's/#boot=[^" ]*/#boot=<redacted>/g' | tee "$out"
ego_rc=${pipestatus[2]}
rc=0
[[ $ego_rc -ne 0 ]] && { echo "ego-browser exit $ego_rc" >&2; rc=1; }
grep -q '^RESULT .* FAIL' "$out" && rc=1
grep -q '^RESULT .* PASS' "$out" || rc=1
echo "scenario rc=$rc (evidence: $out)"
exit $rc
