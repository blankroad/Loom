# NATIVE-REQUEST — NO APP LIVE (2026-11-15 ~12:40, bench-final-qa-opus w2R)

- Build #1 ROOT CUA is **done and the lease is returned**: Enter/Tab PASS and Connection→Reconnect PASS on the ordinary dist app
  (ROOT-OBSERVATIONS.md, NATIVE_CHECKS_DONE). That app, its daemon and its fixtures were cleaned up.
- **final2 physical hold: PENDING.** It needs the macOS session unlocked and awake first. After unlock, QA will:
  1. start a private plain final2 ordinary app (`desktop/dist/Loom.app`, exe 0a2a7a52…) with 2 pty sessions (dut, dut2) and the 키 입력 대상 toggle ready;
  2. arm a fresh observer `LOOM_CLIENT=qa-broadcast-observer loom events --since now --domain uart --follow --timeout 120 --max 1000 --json` and confirm its
     first snapshot;
  3. then replace this file with the exact app path, pid, LOOM_HOME, port and active channels, and notify ROOT.
  ROOT checks: physical typing in 현재 세션 (one pty only), then in 연결된 모두 (both ptys); markers are compared in both rx.log files, the observer and the UI.
