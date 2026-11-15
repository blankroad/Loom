#!/usr/bin/env bash
# Native Loom.app smoke against an ISOLATED LOOM_HOME and an external pty UART fixture.
# Native UI input (typing, Retry click, window close / Cmd-Q) is done by a human or a native
# automation tool (ROOT: cua_repl) between steps; this script launches, drives CLI/fixtures, asserts.
#
#   smoke-native.sh start        [--app PATH] [--cli PATH]   launch app on fresh home, CLI marker → UI
#   smoke-native.sh send TEXT                                CLI sends TEXT to channel dut (UI must show ack:TEXT)
#   smoke-native.sh check-typed TEXT [--tab]                 device received TEXT typed in the native UI terminal
#                                                            (--tab: TEXT was followed by the Tab key → 0x09)
#   smoke-native.sh after-close                              app pid exited; daemon/channel/device still alive
#   smoke-native.sh reopen [--plain]                         relaunch: same daemon reused, viewer back
#                                                            (--plain: no WebDriver/on-top, for native CUA checks)
#   smoke-native.sh error-start                              app with absent explicit daemon bin → error UI, no daemon
#   smoke-native.sh error-fix                                put the binary in place (then click Retry natively)
#   smoke-native.sh error-verify                             Retry started that exact daemon, viewer attached
#   smoke-native.sh hygiene                                  no token/boot fragment in logs/evidence/process args
#   smoke-native.sh ui-mismatch                              reused daemon with a different UI → refusal, daemon kept
#   smoke-native.sh check-download GLOB [TEXT]               REC/export file saved by the WK UI (e2e downloads dir)
#   smoke-native.sh extra-fixtures NONCE                     2nd UART pty (not opened; UI 수동 입력 path), DoIP ECU, UDP server, pcapng
#                                                            stream producer (gated by GO) → $RUN/fixtures.json
#   smoke-native.sh quit                                     AppKit terminate() of the QA app pid (Cmd-Q path)
#   smoke-native.sh status | cleanup
# start --wd PORT: test builds (cargo feature e2e) expose the in-app WebDriver on 127.0.0.1:PORT
# (error-start uses PORT+1); drive it with e2e/wk.mjs.
#
# State: $QA_RUN (default work/reports/tauri-desktop/qa/evidence/native-current) holds state.env and
# step evidence (paths, pids, CLI envelopes — never tokens). Homes live under $QA_HOME_ROOT (default
# $TMPDIR). Only processes recorded in state.env (identified by their LOOM_HOME) are ever signalled.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
RUN="${QA_RUN:-$repo/work/reports/tauri-desktop/qa/evidence/native-current}"
STATE="$RUN/state.env"
mkdir -p "$RUN"
step="${1:-help}"; shift || true

say() { printf '[%s] %s\n' "$step" "$*" | tee -a "$RUN/smoke.log"; }
pass() { say "PASS $*"; echo "PASS|$step|$*" >>"$RUN/results.txt"; }
fail() { say "FAIL $*"; echo "FAIL|$step|$*" >>"$RUN/results.txt"; exit 1; }
save() { printf '%s=%q\n' "$1" "$2" >>"$STATE"; }
load() { [[ -f "$STATE" ]] || fail "no $STATE (run start)"; # shellcheck disable=SC1090
  source "$STATE"; }
jget() { python3 -I -c 'import json,sys
d=json.load(sys.stdin)
for k in sys.argv[1].split("."):
    d=d.get(k) if isinstance(d,dict) else None
print("" if d is None else d)' "$1"; }
cli() { LOOM_HOME="$HOME_DIR" LOOM_NO_AUTOSTART=1 LOOM_CLIENT=desktop-qa "$CLI" "$@"; }
djson() { python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$1/run/daemon.json" "$2" 2>/dev/null; }
exe_of() { ps -o comm= -p "$1" 2>/dev/null; }
alive() { kill -0 "$1" 2>/dev/null; }
# app pid = newest process of this bundle's executable whose environment carries our LOOM_HOME
app_exe() { /usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist"; }
# Only the bundle's main executable (never Contents/MacOS/loomd or its workers); exact comm match, no regex on $APP.
find_app() { local h="$1" p want; want="$APP/Contents/MacOS/$(app_exe)"
  for p in $(pgrep -x "$(app_exe)" | sort -rn); do
    [[ "$(ps -o comm= -p "$p" 2>/dev/null)" == "$want" ]] || continue
    ps eww -o command= -p "$p" 2>/dev/null | tr ' ' '\n' | grep -qxF "LOOM_HOME=$h" && { echo "$p"; return; }
  done; }
has_app() { [[ -n "$(find_app "$1")" ]]; }
# positive UI-identity path: the app navigated only after a Match (no unstaged-build bypass, no mismatch)
identity_ok() { local e="$RUN/app.$(basename "$1").stderr"
  ! grep -qE 'unstaged dev build|UI build mismatch' "$e" 2>/dev/null; }
wait_for() { local end=$((SECONDS+$1)); shift; while ((SECONDS<end)); do "$@" && return 0; sleep 0.2; done; return 1; }
bounded() { local secs="$1"; shift; "$@" & local c=$!; ( sleep "$secs"; kill -TERM "$c" 2>/dev/null ) & local w=$!
  wait "$c"; local rc=$?; kill "$w" 2>/dev/null; wait "$w" 2>/dev/null; return $rc; }
viewers() { bounded 5 cli status --json --timeout 4 2>/dev/null | jget data.viewers.visible; }
has_viewer() { local v; v="$(viewers)"; [[ -n "$v" && "$v" -ge 1 ]]; }
no_viewer() { local v; v="$(viewers)"; [[ "$v" == 0 ]]; }
launch() { # $1 home, rest: extra --env ; QA_WD (test builds with feature e2e) enables the in-app WebDriver port
  local h="$1"; shift
  [[ -n "${QA_WD:-}" ]] && set -- "$@" --env "LOOM_DESKTOP_QA_WEBDRIVER_PORT=$QA_WD"
  # e2e builds only: real always-on-top window level so macOS occlusion does not pause WebKit (QA_ON_TOP=1)
  [[ -n "${QA_WD:-}" && "${QA_ON_TOP:-0}" == 1 ]] && set -- "$@" --env "LOOM_DESKTOP_QA_ON_TOP=1"
  # e2e builds only: REC/export downloads land in the isolated home, not the user's ~/Downloads
  [[ -n "${QA_WD:-}" ]] && { mkdir -p "$h/downloads"; set -- "$@" --env "LOOM_DESKTOP_QA_DOWNLOAD_DIR=$h/downloads"; }
  open -n -a "$APP" --env "LOOM_HOME=$h" "$@" --stdout "$RUN/app.$(basename "$h").stdout" --stderr "$RUN/app.$(basename "$h").stderr"
}

case "$step" in
start)
  APP="/tmp/loom-tauri-build/debug/bundle/macos/Loom.app"; CLI="$repo/target/debug/loom"
  while [[ $# -gt 0 ]]; do case "$1" in --app) APP="${2%/}"; shift 2;; --cli) CLI="$2"; shift 2;; --wd) WD_PORT="$2"; shift 2;; *) fail "bad arg $1";; esac; done
  [[ -d "$APP" ]] || fail "no app at $APP"
  APP="$(cd "$APP" && pwd -P)"   # /tmp is a symlink; processes report the physical path
  [[ -f "$STATE" ]] && fail "$STATE exists; run cleanup first"
  [[ -x "$APP/Contents/MacOS/loomd" ]] || fail "$APP has no Contents/MacOS/loomd"
  HOME_DIR="$(mktemp -d "${QA_HOME_ROOT:-${TMPDIR:-/tmp}}/lqa.XXXXXX")"; chmod 700 "$HOME_DIR"
  : >"$STATE"; save APP "$APP"; save CLI "$CLI"; save HOME_DIR "$HOME_DIR"; save WD_PORT "${WD_PORT:-}"
  say "app=$APP cli=$CLI ($(shasum -a 256 "$CLI" | cut -c1-16)) home=$HOME_DIR"
  python3 -I "$here/lib/uart_fixture.py" --state "$HOME_DIR/fixture" >/dev/null 2>&1 &
  save FIX_PID $!
  wait_for 5 test -s "$HOME_DIR/fixture/tty" || fail "fixture did not start"
  TTY="$(cat "$HOME_DIR/fixture/tty")"; save TTY "$TTY"
  [[ -e "$HOME_DIR/run/daemon.json" ]] && fail "home not fresh"
  t0=$(date +%s); QA_WD="${WD_PORT:-}" launch "$HOME_DIR"
  wait_for 20 has_app "$HOME_DIR" || fail "app process for this home not found"
  APP_PID="$(find_app "$HOME_DIR")"; save APP_PID "$APP_PID"
  wait_for 30 test -s "$HOME_DIR/run/daemon.json" || fail "no daemon.json within 30s (see $RUN/app.*.stderr, $HOME_DIR/run/loomd.log)"
  DAEMON_PID="$(djson "$HOME_DIR" pid)"; save DAEMON_PID "$DAEMON_PID"
  [[ "$(exe_of "$DAEMON_PID")" == "$APP/Contents/MacOS/loomd" ]] && pass "absent daemon started from bundled loomd pid=$DAEMON_PID in $(( $(date +%s)-t0 ))s" \
    || fail "daemon exe '$(exe_of "$DAEMON_PID")' is not the bundled loomd"
  wait_for 30 has_viewer && pass "native viewer attached+visible (viewers.visible=$(viewers)) app pid=$APP_PID" || fail "no visible viewer within 30s"
  identity_ok "$HOME_DIR" && pass "UI identity Match on fresh daemon (no unstaged bypass)" || fail "UI identity bypassed or mismatched (see app stderr)"
  cli uart open "$TTY" --as dut --baud 115200 --json >"$RUN/start.uart-open.json" 2>&1 || fail "uart open failed"
  m="qa-cli-$(date +%H%M%S)"; save CLI_MARKER "$m"
  cli uart send dut "$m" --json >"$RUN/start.send.json" 2>&1 || fail "send failed"
  cli uart expect dut "ack:$m" --timeout 10 --json >"$RUN/start.expect.json" 2>&1 && grep -q "$m" "$HOME_DIR/fixture/rx.log" \
    && pass "CLI→device→daemon round trip on $TTY ('$m'; vis=$(jget meta.visibility <"$RUN/start.expect.json"))" || fail "round trip"
  say "ROOT: native window should show Loom UI with UART channel 'dut' containing 'ack:$m' and 'tick N' lines"
  ;;
send)
  load; m="${1:?text}"; cli uart send dut "$m" --json >"$RUN/send.$m.json" 2>&1 || fail "send"
  cli uart expect dut "ack:$m" --timeout 10 --json >"$RUN/send.$m.expect.json" 2>&1 && pass "CLI sent '$m', device acked (UI should show ack:$m)" || fail "no ack"
  ;;
check-typed)
  load; m="${1:?text typed in native UI}"
  wait_for 15 grep -q -- "$m" "$HOME_DIR/fixture/rx.log" && pass "device received native-typed '$m' (WKWebView keyboard → daemon → pty)" || fail "device never received '$m'"
  if [[ "${2:-}" == --tab ]]; then  # Tab must reach the device as 0x09, not move WKWebView focus
    wait_for 5 grep -q -- "$m"$'\t' "$HOME_DIR/fixture/rx.log" && pass "Tab key after '$m' delivered to device as 0x09" || fail "no 0x09 after '$m' (Tab captured by WebView focus?)"
  fi
  cli uart expect dut "ack:$m" --timeout 10 --json >"$RUN/typed.expect.json" 2>&1 && pass "CLI observes device ack:$m on same stream" || fail "CLI did not see ack:$m"
  ;;
after-close)
  load
  say "waiting up to 120s for app pid $APP_PID to exit (close window / Cmd-Q natively)"
  wait_for 120 bash -c "! kill -0 $APP_PID 2>/dev/null" || fail "app pid $APP_PID still running"
  pass "app pid $APP_PID exited after native close"
  alive "$DAEMON_PID" && [[ "$(djson "$HOME_DIR" pid)" == "$DAEMON_PID" ]] && pass "daemon pid $DAEMON_PID alive, discovery unchanged" || fail "daemon died with app"
  alive "$FIX_PID" || fail "fixture died"
  wait_for 15 no_viewer && pass "viewer detached (visible=0)" || say "WARN viewer count still $(viewers)"
  m="qa-after-close-$(date +%H%M%S)"
  cli uart send dut "$m" --json >"$RUN/after-close.send.json" 2>&1 && cli uart expect dut "ack:$m" --timeout 10 --json >"$RUN/after-close.expect.json" 2>&1 \
    && pass "channel dut still open and streaming after app close ('$m')" || fail "channel not usable after close"
  ;;
reopen)
  load; wdp="${WD_PORT:-}"
  # --plain: ordinary window for human/CUA native checks (no in-app WebDriver, no always-on-top)
  [[ "${1:-}" == --plain ]] && { wdp=""; say "plain relaunch (no WebDriver, normal window level)"; }
  QA_WD="$wdp" launch "$HOME_DIR"
  wait_for 20 has_app "$HOME_DIR" || fail "relaunched app not found"
  APP_PID="$(find_app "$HOME_DIR")"; save APP_PID "$APP_PID"
  wait_for 30 has_viewer || fail "no viewer after reopen"
  [[ "$(djson "$HOME_DIR" pid)" == "$DAEMON_PID" ]] && alive "$DAEMON_PID" && pass "reopen reused daemon pid $DAEMON_PID (no second daemon), viewer visible, app pid $APP_PID" || fail "daemon changed"
  identity_ok "$HOME_DIR" && pass "UI identity Match on reused current daemon" || fail "UI identity bypassed or mismatched on reuse"
  [[ "$(for p in $(pgrep -f "^$APP/Contents/MacOS/loomd"); do ps eww -o command= -p "$p" 2>/dev/null; done | grep -v "loomd worker" | grep -c "LOOM_HOME=$HOME_DIR")" -le 1 ]] && pass "single daemon for home" || fail "duplicate daemons"
  ;;
error-start)
  load; [[ -n "${ERR_HOME:-}" ]] && fail "error scenario already started"
  ERR_HOME="$(mktemp -d "${QA_HOME_ROOT:-${TMPDIR:-/tmp}}/lqe.XXXXXX")"; chmod 700 "$ERR_HOME"; save ERR_HOME "$ERR_HOME"
  ERR_BIN="$ERR_HOME/bin/loomd"; save ERR_BIN "$ERR_BIN"
  ERR_WD=""; [[ -n "${WD_PORT:-}" ]] && ERR_WD=$((WD_PORT+1)); save ERR_WD "$ERR_WD"
  QA_WD="$ERR_WD" launch "$ERR_HOME" --env "LOOM_DESKTOP_DAEMON_BIN=$ERR_BIN"
  wait_for 20 has_app "$ERR_HOME" || fail "error-scenario app not found"
  ERR_APP_PID="$(find_app "$ERR_HOME")"; save ERR_APP_PID "$ERR_APP_PID"
  sleep 5
  alive "$ERR_APP_PID" && [[ ! -e "$ERR_HOME/run/daemon.json" ]] && pass "missing explicit daemon bin: app alive (pid $ERR_APP_PID), no daemon started, no fallback" || fail "unexpected daemon or app exit"
  say "ROOT: screenshot the error view (expect actionable message, no token/URL), do NOT click Retry yet"
  ;;
error-fix)
  load; mkdir -p "$(dirname "$ERR_BIN")"; cp "$APP/Contents/MacOS/loomd" "$ERR_BIN"; chmod 755 "$ERR_BIN"
  pass "placed $ERR_BIN; ROOT: click Retry in the error window now"
  ;;
error-verify)
  load
  wait_for 60 test -s "$ERR_HOME/run/daemon.json" || fail "Retry did not start daemon within 60s"
  p="$(djson "$ERR_HOME" pid)"; save ERR_DAEMON_PID "$p"
  [[ "$(exe_of "$p")" == "$ERR_BIN" ]] && pass "Retry started exactly LOOM_DESKTOP_DAEMON_BIN (pid $p)" || fail "daemon exe $(exe_of "$p")"
  HOME_DIR="$ERR_HOME" wait_for 30 has_viewer && pass "viewer attached after Retry" || fail "no viewer after Retry"
  ;;
hygiene)
  load; bad=0
  for h in "$HOME_DIR" ${ERR_HOME:+"$ERR_HOME"}; do
    [[ -s "$h/run/daemon.json" ]] || continue
    tok="$(djson "$h" token)"; [[ -n "$tok" ]] || continue
    for f in "$RUN"/* "$h/run/loomd.log"; do
      [[ -f "$f" ]] || continue
      if grep -qF -f <(printf '%s\n' "$tok") -- "$f"; then say "LEAK token in $(basename "$f")"; bad=1; fi
      if grep -qE '#boot=|[?&]code=' "$f"; then say "LEAK boot fragment/code in $(basename "$f")"; bad=1; fi
    done
    ps -axww -o pid=,args= | grep -qF -f <(printf '%s\n' "$tok") && { say "LEAK token in process args"; bad=1; }
    unset tok
  done
  perms="$(stat -f %Lp "$HOME_DIR/run/daemon.json" 2>/dev/null)"; say "daemon.json mode $perms"
  [[ $bad -eq 0 ]] && pass "no token/boot fragment in app stdout/stderr, loomd.log, evidence, process args" || fail "credential leak (details above, values not printed)"
  ;;
quit)  # Cmd-Q-equivalent: AppKit terminate() of OUR app pid only (no UI input, no System Events)
  load; alive "$APP_PID" || fail "app pid $APP_PID not running"
  swift "$here/lib/quit_app.swift" "$APP_PID" | tee -a "$RUN/smoke.log"
  pass "requested AppKit quit of app pid $APP_PID"
  ;;
ui-mismatch)  # reused daemon serving a DIFFERENT UI build → app must refuse (Korean error), daemon untouched
  load; [[ -n "${UIM_HOME:-}" ]] && fail "ui-mismatch already ran"
  UIM_HOME="$(mktemp -d "${QA_HOME_ROOT:-${TMPDIR:-/tmp}}/lqm.XXXXXX")"; chmod 700 "$UIM_HOME"; save UIM_HOME "$UIM_HOME"
  mkdir -p "$UIM_HOME/oldui"; printf '<!doctype html><title>old</title>\n' >"$UIM_HOME/oldui/index.html"
  (LOOM_HOME="$UIM_HOME" nohup "$APP/Contents/MacOS/loomd" --ui-dir "$UIM_HOME/oldui" >"$UIM_HOME/loomd.out" 2>&1 &)
  wait_for 20 test -s "$UIM_HOME/run/daemon.json" || fail "old-UI daemon did not start"
  UIM_DAEMON_PID="$(djson "$UIM_HOME" pid)"; save UIM_DAEMON_PID "$UIM_DAEMON_PID"
  QA_WD="${WD_PORT:+$((WD_PORT+2))}" launch "$UIM_HOME"
  wait_for 20 has_app "$UIM_HOME" || fail "ui-mismatch app not found"
  UIM_APP_PID="$(find_app "$UIM_HOME")"; save UIM_APP_PID "$UIM_APP_PID"
  wait_for 20 grep -q "UI build mismatch" "$RUN/app.$(basename "$UIM_HOME").stderr" \
    && pass "app refused reused daemon pid $UIM_DAEMON_PID with different UI (no navigation)" || fail "no UI mismatch refusal"
  sleep 2; alive "$UIM_DAEMON_PID" && [[ "$(djson "$UIM_HOME" pid)" == "$UIM_DAEMON_PID" ]] \
    && pass "user daemon left running (not auto-stopped)" || fail "daemon was stopped/replaced"
  say "WK: wk.mjs --wd $((WD_PORT+2)) launch-failed → Korean message; screenshot"
  ;;
check-download)  # a REC/export file saved by the WK UI (e2e: $HOME_DIR/downloads); arg = file-name glob, then expected text
  load; g="${1:?name glob}"; want="${2:-}"
  wait_for 30 bash -c "compgen -G '$HOME_DIR/downloads/$g' >/dev/null" || fail "no download matching '$g' in QA downloads dir"
  f="$(ls -t "$HOME_DIR"/downloads/$g | head -1)"; cp "$f" "$RUN/"; sz=$(stat -f %z "$f")
  [[ -z "$want" ]] || grep -qF -- "$want" "$f" || fail "download $(basename "$f") ($sz B) lacks '$want'"
  pass "WK download saved $(basename "$f") ($sz B)${want:+ containing '$want'}; copy in evidence"
  ;;
extra-fixtures)  # external simulators only; every device/client I/O goes through Loom (UI or CLI) afterwards
  load; [[ -n "${DOIP_PID:-}" ]] && fail "extra fixtures already started"
  nonce="${1:?alnum nonce}"; [[ "$nonce" =~ ^[A-Za-z0-9]{1,32}$ ]] || fail "nonce must be alnum <= 32"
  ethdir="$repo/work/reports/external-simulator-validation/ethernet"
  python3 -I "$here/lib/uart_fixture.py" --state "$HOME_DIR/fixture2" --banner-every 3 >/dev/null 2>&1 &
  save FIX2_PID $!
  python3 -I "$repo/work/reports/validation-report/tools/doip_fixture.py" "$HOME_DIR/doip" >/dev/null 2>&1 &
  save DOIP_PID $!
  python3 -I "$ethdir/udp_server.py" "$HOME_DIR/eth-udp" 0 1800 >/dev/null 2>&1 &
  save UDP_PID $!
  python3 -I "$ethdir/pcapng_producer.py" "$HOME_DIR/eth-stream" "$nonce" 1800 >/dev/null 2>&1 &
  save PCAP_PID $!
  wait_for 5 test -s "$HOME_DIR/fixture2/tty" || fail "2nd uart fixture did not start"
  wait_for 5 test -s "$HOME_DIR/doip/ready.json" || fail "doip fixture did not start"
  wait_for 5 test -s "$HOME_DIR/eth-udp/udp-server-ready.json" || fail "udp server did not start"
  wait_for 5 test -s "$HOME_DIR/eth-stream/producer-ready.json" || fail "pcapng producer did not start"
  TTY2="$(cat "$HOME_DIR/fixture2/tty")"; save TTY2 "$TTY2"; save ETH_NONCE "$nonce"
  python3 -I - "$HOME_DIR" "$TTY2" >"$RUN/fixtures.json" <<'PY' || fail "fixtures.json"
import json, sys; h = sys.argv[1]; r = lambda p: json.load(open(f"{h}/{p}"))
d = r("doip/ready.json"); u = r("eth-udp/udp-server-ready.json"); s = r("eth-stream/producer-ready.json")
print(json.dumps({"uart2_tty": sys.argv[2], "doip": {"host": "127.0.0.1", "port": d["port"], "ecu": d["ecu"], "tester": d["tester"], "pid": d["pid"]},
  "udp_server": u["bind"], "pcapng_stream": s["bind"], "stream_gate": f"{h}/eth-stream/GO", "stream_close": f"{h}/eth-stream/CLOSE"}, indent=1))
PY
  pass "extra fixtures up: uart2 $TTY2 (not opened: record the real uart.ports dropdown as-is, then open via 수동 입력…), $(jget doip.port <"$RUN/fixtures.json" | sed 's/^/doip :/'), udp $(jget udp_server <"$RUN/fixtures.json"), stream $(jget pcapng_stream <"$RUN/fixtures.json") (gated: touch GO)"
  ;;
status)
  load; for v in APP_PID DAEMON_PID FIX_PID FIX2_PID DOIP_PID UDP_PID PCAP_PID ERR_APP_PID ERR_DAEMON_PID; do p="${!v:-}"; [[ -n "$p" ]] && echo "$v=$p $(alive "$p" && echo alive || echo gone)"; done
  echo "HOME_DIR=$HOME_DIR TTY=$TTY"; [[ -f "$RUN/results.txt" ]] && cat "$RUN/results.txt"
  ;;
cleanup)
  load
  for v in APP_PID ERR_APP_PID UIM_APP_PID; do p="${!v:-}"; [[ -n "$p" ]] && alive "$p" && { say "terminating own app pid $p (SIGTERM)"; kill -TERM "$p"; wait_for 10 bash -c "! kill -0 $p 2>/dev/null"; }; done
  for h in "$HOME_DIR" ${ERR_HOME:+"$ERR_HOME"} ${UIM_HOME:+"$UIM_HOME"}; do
    [[ -s "$h/run/daemon.json" ]] && LOOM_HOME="$h" "$CLI" daemon stop >>"$RUN/smoke.log" 2>&1
  done
  for v in DAEMON_PID ERR_DAEMON_PID UIM_DAEMON_PID; do p="${!v:-}"; [[ -n "$p" ]] && wait_for 10 bash -c "! kill -0 $p 2>/dev/null" || true; done
  for v in FIX_PID FIX2_PID DOIP_PID UDP_PID PCAP_PID; do p="${!v:-}"
    [[ -n "$p" ]] && alive "$p" && kill -TERM "$p" && wait_for 5 bash -c "! kill -0 $p 2>/dev/null"; done
  [[ -d "$HOME_DIR/eth-stream" ]] && cp "$HOME_DIR"/eth-stream/producer*.json* "$RUN/" 2>/dev/null
  for f in doip/doip.jsonl eth-udp/udp-server.jsonl fixture/rx.log fixture2/rx.log; do
    [[ -f "$HOME_DIR/$f" ]] && cp "$HOME_DIR/$f" "$RUN/fixture-$(echo "$f" | tr / -)"; done
  left="$(pgrep -f -- "$HOME_DIR|${ERR_HOME:-/nonexistent-qa}|${UIM_HOME:-/nonexistent-qa}" || true)"
  [[ -z "$left" ]] && say "no processes left for QA homes" || say "WARN leftover pids for QA homes: $left"
  rm -rf "$HOME_DIR" ${ERR_HOME:+"$ERR_HOME"} ${UIM_HOME:+"$UIM_HOME"}
  mv "$STATE" "$RUN/state.env.done"; say "cleaned homes; evidence kept in $RUN"
  ;;
*) sed -n '2,22p' "$0" ;;
esac
