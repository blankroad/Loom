#!/usr/bin/env bash
# Visible-window native suite for an e2e QA build (feature e2e, LOOM_DESKTOP_QA_ON_TOP=1).
# Precondition: smoke-native.sh start --wd <port> already ran (state.env). Requires an UNLOCKED session
# with the display awake — otherwise WebKit correctly reports document hidden and pauses rendering.
#   desktop/scripts/e2e/run-visible-suite.sh [--wait-unlock SECS]
# Exit 0 all PASS; 1 any FAIL; 4 session locked / display asleep (environment, not product).
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"; scripts="$(cd "$here/.." && pwd)"; repo="$(cd "$scripts/../.." && pwd)"
RUN="${QA_RUN:-$repo/work/reports/tauri-desktop/qa/evidence/native-current}"
export QA_RUN="$RUN"   # smoke-native.sh must use the same run directory
export QA_ON_TOP=1
# shellcheck disable=SC1091
source "$RUN/state.env"
smoke() { "$scripts/smoke-native.sh" "$@"; }
wk() { local p="$1"; shift; node "$here/wk.mjs" --wd "$p" --out "$RUN/wk" "$@"; }
screen_ok() { swift "$scripts/lib/screen_state.swift" >/dev/null 2>&1; }
if [[ "${1:-}" == --wait-unlock ]]; then
  end=$((SECONDS+${2:-1800})); until screen_ok; do ((SECONDS<end)) || { echo "suite: session still locked/asleep"; exit 4; }; sleep 5; done
  echo "suite: session unlocked and display awake at $(date '+%H:%M:%S')"; sleep 3
fi
screen_ok || { echo "suite: ENVIRONMENT session locked or display asleep — not running"; exit 4; }
rc=0; t() { "$@" || rc=1; }
t smoke status >/dev/null
t wk "$WD_PORT" ready
t wk "$WD_PORT" rx "ack:$CLI_MARKER"
t wk "$WD_PORT" type qa-typed-1;      t smoke check-typed qa-typed-1
t wk "$WD_PORT" type qa-tab --tab;    t smoke check-typed qa-tab --tab
t smoke send qa-cli-2;                t wk "$WD_PORT" shows ack:qa-cli-2
t smoke quit;                         t smoke after-close
t smoke reopen;                       t wk "$WD_PORT" ready
t wk "$WD_PORT" rx "ack:qa-cli-2"     # late join after relaunch: history + live ticks
t smoke error-start
# shellcheck disable=SC1091
source "$RUN/state.env"
t wk "$ERR_WD" launch-failed;         t smoke error-fix
t wk "$ERR_WD" retry;                 t smoke error-verify
node "$here/wk.mjs" --wd "$ERR_WD" --out "$RUN/wk-err" ready || rc=1
t smoke hygiene
echo "suite: exit $rc"; exit $rc
