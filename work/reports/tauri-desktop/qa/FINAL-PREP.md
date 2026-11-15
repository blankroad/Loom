> **Status 2026-11-15 ~12:40:** the gate opened and closed out. Build #1 (prefix) native evidence is done; the final combined build **final2** is built and verified
> (evidence/final2). Native acceptance on final2 is PENDING because the session is locked. The current state and next steps are in STATUS.md, READY.json and REPORT.html.
> The gate table and §B below are the original prep runbook (historical).

# FINAL-PREP — final integration QA runbook (bench-final-qa-opus, w2R), 2026-11-15

This file is preparation only. **No production build, re-embed, stage or bundle has been run.** No Ego space was touched,
since A (bench-uart-opus) holds the space-14 lease. Desktop cargo tests were not rerun: ROOT's 40 tests cover the unchanged Rust source.

## Gate (all four are required before step B1)
| Dependency | Source of truth | State at prep |
|---|---|---|
| ETH (bench-diag) | bench-diag/STATUS.md | **READY** (45 tests, Ego live proof, cleanup done; w2P closed) |
| UART + shell (A, w2N) | bench-uart/STATUS.md | File says READY (targeted 210/210, REC UTF-8/bounded). **Gate needs ROOT's relay of A READY + an explicit space-14 handoff (not received)** |
| UDS (bench-uds, w2Q) | bench-uds/STATUS.md | **READY_FOR_REVIEW** (ROOT source+boundary review done; 51 targeted, tsc/lint clean; full 665/666 = shell `defaultLayout` timeout under load, alone 7/7) |
| ROOT source-review go-ahead | ROOT message / ROOT-REVIEW.md | pending |
Ego: ask ROOT/A for the **space 14 lease** (p1 UART canonical, p2 UDS canonical, p3 product) only after the gate. Reuse it and do not create spaces.

## Desktop source review (this role, read-only re-review of the uncommitted desktop diff)
- UI identity guard (`main.rs` connect_and_open → `ui_identity::verdict`):
  - It runs after the origin check and before `launch.port` is set, so it reruns on Retry and on Reconnect.
  - On Mismatch it never stops the daemon.
  - The message is credential-free and the home is POSIX-quoted. OK.
- `host::daemon_ui_index`: a public GET with no Authorization header, and the bearer request bytes are unchanged. It is bounded to 5 s. OK.
- Download glue:
  - wry 0.57.2 `download_policy` picks `~/Downloads/<name>` and de-duplicates with " (n)" before calling our handler. OK.
  - The blob origin check (localhost:<port>) matches the page origin, so A's `recorder.ts`/`UartWorkbench.tsx` `<a download href=blob:>` is allowed.
  - Test-only note: the e2e redirect to `$HOME_DIR/downloads/<name>` is not de-duplicated, so a repeated download with the same
    name in one run may fail in WK. Mitigation: use a fresh home per run and distinct REC names (A's names include the date and alias).
- Release builds fail closed on an unstaged identity. Debug builds warn and continue.
  - Gap closed now: `smoke-native.sh start` and `reopen` now **assert `identity_ok`**, meaning no "unstaged dev build" or
    "UI build mismatch" line in app stderr. A debug QA bundle can no longer pass the positive path by bypass.
- Not exercised yet: `on_download` natively, ui-mismatch natively, and the positive Match natively. All need the final build.

## Prep changes (desktop-owned, syntax-checked; no build)
- `desktop/scripts/e2e/probes.mjs` (new): `ASSET_DIGESTS`, the SHA-256 of every loaded same-origin `/ui/` asset. **The same string runs in
  WK (wk.mjs `digests`) and in Ego (`page.evaluate`)**. A stub run in Node checked the hash correctness and that foreign/non-/ui entries are dropped.
- `wk.mjs`: added `click <css>` (WebDriver element click), `fill <css> <text>` (fails honestly if the driver drops printable keys,
  the known run4 limitation), `text <css>` and `digests`. `node --check` OK.
- `smoke-native.sh`:
  - `extra-fixtures <nonce>` starts the 2nd UART pty (left **unopened**, for the UI 수동 입력 path, see §B7), the DoIP ECU fixture, the UDP server and
    the pcapng stream producer (gated by GO). Results go to `$RUN/fixtures.json` (no secrets).
  - `cleanup` signals only these recorded PIDs and copies the fixture logs (doip.jsonl, udp-server.jsonl, producer*.json*, rx.log ×2).
  - `identity_ok` was added (see above).
  - Dry run in a scratch home: all 4 fixtures came up and cleanup left 0 leftovers. `bash -n` OK.

## B. Final run (after the gate), in order
1. Preconditions: `swift desktop/scripts/lib/screen_state.swift` (unlocked/awake); there must be no own QA processes; `ls desktop/target` must be absent.
2. Once, as **one clean full-UI run with no concurrent load**: no other vitest/vite/cargo running (check `pgrep -fl 'vitest|vite|cargo|rustc'`
   and record it). Then `pnpm --dir ui typecheck && pnpm --dir ui lint && pnpm --dir ui test`. This settles the `shell/dock/defaultLayout.test.tsx`
   load timeout seen by UDS (665/666). If it still times out alone-loaded, report it to A via ROOT, with the timing. Findings go to the file owners through ROOT, never fixed here.
3. `desktop/scripts/stage-loomd.sh --profile debug --force-ui` → `stage-loomd.sh --check` (includes loomd.ui-index.html == ui/dist/index.html).
   Record the ui/dist digest, the loomd sha and the stamp.
4. `desktop/scripts/build-app.sh --release` → `(cd desktop && npm run dist)` → `verify-dist.sh desktop/dist/Loom.app` (ordinary, no WebDriver).
   E2E copy: `build-app.sh --debug --features e2e --out /tmp/loom-tauri-build/qa/Loom-QA.app` + `QA_ALLOW_WEBDRIVER=1 verify-bundle.sh`.
   All cargo commands use `CARGO_TARGET_DIR=/tmp/loom-tauri-build` under `.cargo-desktop.lock`; never create desktop/target.
   Rerun the desktop cargo tests only if the desktop source changed after ROOT's 40-test run (today only scripts changed).
5. `QA_ON_TOP=1 smoke-native.sh start --app /tmp/loom-tauri-build/qa/Loom-QA.app --wd 47941`, then `extra-fixtures <nonce>`.
6. Visual: **fresh entry requires an already-open UART channel** (A: bootstrap fallback lands on `#/s/uart` only when UART is the live service).
   `start` opens the app before `dut` exists, so measure fresh entry after `quit` → `reopen` (new process, no hash) → `wk.mjs geom` (no nav).
   Then: `goto #/p/uart/{trace,terminal}?channel=dut` → geom, explicit integrated layout `#/l/m1-default` (기본 작업 공간; UART+ETH+UDS channels live) → geom.
   ETH/UDS routes must be `.mod-bare` (no outer rail/side/toolbar), and the ETH pause-banner overflow (B→A) must be rechecked.
   Ego p3: same daemon/route, emulate 1440×900, then measure and screenshot **in one call**. Run `ASSET_DIGESTS` in both and diff with `wk-asset-digests.json`.
   Compare with `parity/canonical-*.png` and canonical-controls.json, and **open and visually assess every PNG** (blank-panel lesson).
7. UART functional (CLI observer snapshot first, then fresh markers):
   - Port dropdown (ROOT review): open the session dialog and **record the real `uart.ports` dropdown as-is** in evidence
     (option list text, plus a CLI `loom uart ports --json` taken at the same time). macOS ptys may not appear in OS enumeration.
     That is recorded as an observation and **not** judged as a module defect, and no fake ports are injected.
     The fixture pty (tty2) is then opened through the canonical **`수동 입력…`** option (`select` value `__manual` →
     `input[data-role=portman]`) as a saved session. If a pty does appear in the list, also note it, but still use 수동 입력 for the fixture.
     Also check the "현재 감지 안 됨 (저장된 값)" option on reopen of the saved session.
   - Macros Active/All/Selected, keyboard broadcast separate, green ▶ separate from edit, STOP/abort mid-macro.
   - Split/tabs/resize, REC .log/.jsonl + config export/import → `check-download`.
   - A's operate handoff (a)–(g):
     - (a) 새 세션 → 저장 후 열기, open right/down
     - (b) physical Enter/Tab, plus the EOL CR/LF/CRLF mapping (ROOT CUA)
     - (c) 키 입력 대상 연결된 모두 across dut+dut2
     - (d) macro ▶ 활성/연결된 모두/선택 with send→wait. **STOP mid-run is done LAST** (ROOT order), see §B13
     - (e) REC buffer → 내보내기 in WK (desktop on_download)
     - (f) pane close/maximize/reset layout
     - (g) 포트 닫기/다시 연결 overlay
     Device rx.log + CLI observer are the ground truth for every send.
   - Reopen and retained layout.
   - REC acceptance per ROOT review:
     - multibyte UTF-8 split across chunk boundaries must be readable, and unterminated lines are bounded;
     - the WK download is a **portable buffer export**, not a durable live file save. The UI label and the report must say so;
       the daemon journal persists independently;
     - verify the downloaded bytes (marker + UTF-8 sample sent by the fixture via CLI) in WK and Ego.
8. ETH:
   - `loom eth capture start <label> --backend stream --connect <pcapng_stream>`, then `touch GO`.
   - UI: active card, a distinct 채널 추가, filter, detail/hex, socket UDP raw/text → udp-server.jsonl hex.
   - Observer/export (`eth.export` pcapng is a daemon job, so verify the file through the CLI) against producer-manifest sha256.
   - ROOT-accepted ETH scope, **disclosed in the report as bounded differences, not as feature equivalence**:
     - per-channel `eth.packets{channel}` lists with real cards for navigation (no synthetic merged `전체` feed);
     - no persistent TCP session state (each send uses its own connection);
     - no synthetic burst stimulus;
     - export is the daemon pcapng job.
     QA still fails any fake-enabled control or visual redesign.
9. UDS: UI edits host/port/logical addresses from fixtures.json (tester 0x0E80, ECU 0x1001) and connects from the UI:
   - DID `22 F190` (VIN), the NRC `22 FFFF` → 7F 22 31, an unsupported service → 7F xx 11, and a wrong tester address → routing refusal.
   - Fixture disconnect file → link-down honesty. Cross-check the CLI observer against doip.jsonl.
   - Presets must not imply a connection, and CAN-TP/EthDiag stay disabled.
10. Native negatives:
    - `smoke-native.sh ui-mismatch` + `wk.mjs --wd 47943 launch-failed` (Korean message, daemon kept alive).
    - error-start/fix/Retry (light launch page). quit/after-close/reopen (identity Match on reuse).
11. ROOT CUA: `smoke-native.sh reopen --plain` → write NATIVE-REQUEST.md (app path/pid/home/port) → hold until NATIVE_CHECKS_DONE:
    - native Enter/Tab: `check-typed root-native-N [--tab]`
    - Connection → Reconnect after this run's own daemon restart. Never auto-launch the default home.
12. `hygiene`, then REPORT.html (canonical/product and browser/native pairs, fingerprints, limits), then cleanup of own processes/tabs only,
    then READY_FOR_REVIEW.
13. **Global STOP test LAST (ROOT order)**:
    - Run it only after every other write check (UART/ETH/UDS), ROOT CUA and Reconnect, or in a separate disposable simulator run.
    - STOP is never self-released, and there is no reset/release/restart to continue other write checks after it.
    - Check: a macro running across dut+dut2, then STOP (UI button) → no further bytes in rx.log, strip "STOP으로 중단", new runs refused.
    - The user's real home, daemons and policy are preserved.
