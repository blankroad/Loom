# RESUME-FINAL2 — handoff from bench-final-qa-opus (w2R), 2026-11-15 ~12:45 KST

> ROOT update — Windows/Ubuntu build extension: runtime, host, platform config and packaging sources have changed since final2. The original `desktop/dist/Loom.app` and original Mac staging were deliberately preserved, so their fingerprints still describe that older artifact, not the current source. Before new native acceptance, rebuild/restage an ordinary Mac app and a matching e2e app under the desktop lock, record fresh fingerprints, then run the remaining flows below on that new build. Unchanged UI digest remains 643e5952…; do not rerun the unchanged 686 UI tests merely for this extension. See `../../desktop-cross-platform/STATUS.md`. Screen unlock requirement and space 16 ownership remain unchanged.

**Read this first.** The final combined build is done and verified. Only native/visual acceptance on it remains, blocked because the Mac is locked.
Not DONE, no commit (ROOT alone). Rules unchanged: no subagents, no background LLMs, no Git/index.
Use a fresh visible Herdr workspace for the native Opus 5.5 QA worker; pass `--disallowedTools Agent,Task` (supported CLI syntax verified). ROOT remains orchestration/review/CUA/Git only.

## Do NOT redo
- **No rebuild and no restage** (unless the source changes). The final2 build is current.
- Do not re-run tests that already passed:
  - full UI 686/686 + schema/tsc/lint (evidence/final2/ui-checks.log);
  - `git diff --check` clean;
  - owner tests (ETH-EXPORT Rust 56 / UI 100, STARTUP 17+29, MACRO 59, DOCK 116);
  - desktop guard tests 16+4+real_loomd 1;
  - ROOT 40.
- Do not re-run the build-#1 functional flows that are unaffected by the patches (see REPORT.html §2):
  - UART scope counts, REC .log/.jsonl, config export/import;
  - ETH stream identity, filter, 채널 추가, UDP send;
  - UDS connect/DID/NRC/Raw/reconnect;
  - ROOT CUA Enter/Tab, Connection→Reconnect.

## final2 fingerprint (evidence/final2/fingerprints.txt)
UI dist 643e595265ae92e7… · staged loomd 7efffad83fa7ef19… (bundled re-sign 3a4fa400983ec29b…) ·
`desktop/dist/Loom.app` exe 0a2a7a52… (verify-dist PASS) · e2e `/tmp/loom-tauri-build/qa/Loom-QA.app` exe 0265f8aa… (verify-bundle PASS) ·
loom CLI target/debug/loom 514dd915…. Build #1 (8215041d/8ed2ef33) is the separate pre-patch prefix. Never mix the two.

## Preconditions
1. `swift desktop/scripts/lib/screen_state.swift` must print `locked=false displayAsleep=false`. ROOT confirms the user unlock. Do not poll and do not bypass.
2. `pgrep -fl "lqa\.|lqe\.|lqm\.|Loom-QA|uart_fixture|doip_fixture|udp_server|pcapng_producer"` → none. There are currently **no own processes**.
3. All desktop cargo commands use `CARGO_TARGET_DIR=/tmp/loom-tauri-build` under `.cargo-desktop.lock`. desktop/target must stay absent.

## Remaining actual flows on final2 (in order; new run dir = evidence/native-current)
1. `QA_ON_TOP=1 desktop/scripts/smoke-native.sh start --app /tmp/loom-tauri-build/qa/Loom-QA.app --wd 47941` → `extra-fixtures <nonce>`.
2. Startup:
   - positive: `quit` → `reopen` (fresh process, dut is the only live service) → `wk.mjs geom` must show `#/s/uart` with rail/mtbar;
   - negative: explicit `#/l/m1-default` stays.
3. UART (fresh markers):
   - open dut2 via 새 세션 → `select … __manual` → portman = TTY2;
   - split right; macro send+wait **succeeds** with one final strip;
   - hidden-tab dut2 is **included** in 연결된 모두 / 선택;
   - deep link `#/p/uart/terminal?channel=dut` → back to `#/s/uart` **keeps the split**;
   - hidden-tab REC: hide via session-list "활성 pane에 열기", because a WebDriver click on dv-tab does not activate the tab.
4. ETH:
   - `loom eth capture start ext --ch ext --backend stream --connect <pcapng_stream>`, arm the observer, `touch GO`;
   - **2 UI exports → 2 distinct files**, with the real path shown in the UI;
   - a send on an errored channel → the **failed-TX row shows its reason** (fresh nonce).
5. UDS:
   - configure DoIP (fixtures.json port, 0E80/1001) → 연결;
   - the final capture has 연결 설정 and 주소 상세 **collapsed**, a **selected result row** (응답 순서 / 해석 / 바이트), and the empty 응답 순서 geometry (li.empty spans the pane).
6. Ego **space 16** (created by ROOT; p1 = REPORT.html draft). Reuse it and **never create another space**.
   - ROOT rendered and visually reviewed the static draft (`report-draft-preview.png`; all21 local links resolve), then called `handOff()`. After the user confirms unlock/continue, use `takeOverTaskSpace(16)` per Ego guide; retain p1 as the report and use returned/live labels for added reference/product pages.
   - Canonical pages are served at `http://127.0.0.1:64785/{uart-workbench/opus,ethernet-workbench/opus,uds-read-did/app}/index.html`, a server not owned by QA.
   - The product page uses a watch fixture: `python3 -I desktop/scripts/e2e/watch_url.py <QA LOOM_HOME> <0600 file> [--next route]`; the Ego script reads and deletes the file.
   - Run the same `ASSET_DIGESTS` probe (desktop/scripts/e2e/probes.mjs) as `wk.mjs digests`.
   - Emulate, measure and screenshot in ONE call: 1440×900 for canonical-vs-product, 1440×868 (WK inner) for browser-vs-native.
7. ROOT CUA hold, plain final2 **ordinary** app with dut + dut2:
   - FIRST arm `LOOM_CLIENT=qa-broadcast-observer loom events --since now --domain uart --follow --timeout 120 --max 1000 --json > <run>/broadcast-observer.jsonl` (background exec, no renew) and confirm its first snapshot;
   - THEN write NATIVE-REQUEST.md (pid/path/home/port/channels) and notify ROOT (`herdr --session loom agent prompt w8:p1 …`, identity-validated agent surface);
   - check 현재 vs 연결된 모두 markers in both rx.log files + the observer + the UI; hold until NATIVE_CHECKS_DONE.
8. `smoke-native.sh ui-mismatch` + `wk.mjs --wd 47943 launch-failed`; error-start/fix/Retry.
9. **Global STOP last.** Run a macro across dut+dut2, press STOP: no more bytes. Never release it.
10. hygiene → update REPORT.html (final2 sections replace the PENDING rows) → cleanup of own processes only → READY_FOR_REVIEW to ROOT.

## Tooling notes (desktop/scripts/e2e/wk.mjs)
- Commands:
  - `query <css>`: tag, class, text, aria, title, data-role|act, and option `val`;
  - `click <css> [@N]`; `fill <css> <text>` (fails honestly if keys are dropped); `select <css> <value>` (DOM change event);
  - `keys <css> '\n'` (no navigation); `text <css>` (≤400 chars); `digests`.
- `wk type` navigates to the dut deep link, so do not use it for in-place tests.
- Avoid guessed selectors. Two blind clicks in the b1 run each caused an extra action (recorded in results.txt).
- ROOT must verify the requested ordinary-app PID/path is live before a new CUA `getApp` binding. Previous CUA variables refer to closed build-#1 processes; `getApp` auto-launches a missing app and must not accidentally start the default user home.

## Protected / not ours
- User Ego spaces 0/10/11/12, the user's default LOOM_HOME and daemons, the core worktree loomd processes, and the canonical server on :64785. MAINw8.
- REPORT.html stays **DRAFT** with every final2 native item **PENDING** until the flows above actually pass.
