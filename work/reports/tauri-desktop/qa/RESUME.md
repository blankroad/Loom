# QA native resume (state PENDING_NATIVE, checkpoint 2026-11-15)

Kept for fast resume: `/tmp/loom-tauri-build/qa/Loom-QA.app` (e2e TEST build with on-top knob; bundles the staged loomd
49803a53… / UI digest 6fdd549d…; never distribute). Archived runs: evidence/native-run1-cua-blocked,
native-run2-occluded, native-run3-locked (state.env.done, results.txt, wk/*.png). All QA homes removed; no QA processes.

## Preconditions (each must pass; otherwise do NOT test)
```sh
cd /Users/ctmctm/workspace/Loom
swift desktop/scripts/lib/screen_state.swift          # must print locked=false displayAsleep=false (exit 0)
desktop/scripts/stage-loomd.sh --check                # staged loomd current + embedded UI verified
QA_ALLOW_WEBDRIVER=1 desktop/scripts/verify-bundle.sh /tmp/loom-tauri-build/qa/Loom-QA.app
```
If ui/src changed since the checkpoint: `desktop/scripts/stage-loomd.sh --profile debug` and then
`desktop/scripts/build-app.sh --debug --features e2e --out /tmp/loom-tauri-build/qa/Loom-QA.app`.

## Fresh run
```sh
QA_ON_TOP=1 QA_HOME_ROOT="${TMPDIR:-/tmp}" desktop/scripts/smoke-native.sh start --app /tmp/loom-tauri-build/qa/Loom-QA.app --wd 47941
desktop/scripts/e2e/run-visible-suite.sh             # exit 0 = all PASS, 1 = FAIL, 4 = screen locked (no test)
desktop/scripts/smoke-native.sh status
desktop/scripts/smoke-native.sh cleanup              # stops ONLY this run's app/daemon/fixture; keeps evidence
```
Evidence: work/reports/tauri-desktop/qa/evidence/native-current/{results.txt,smoke.log,suite.log,wk/*.png}.
Archive it afterwards (`mv evidence/native-current evidence/native-run4-visible`).

## Process checks (before and after)
```sh
pgrep -fl "Loom-QA.app|uart_fixture|lqa\.|lqe\." || echo none
lsof -nP -iTCP:47941-47942 -sTCP:LISTEN || echo "ports free"
```
Never signal other loomd/app instances (the user's ~/.loom daemons, the app owner's /tmp/loom-app-iso*).
