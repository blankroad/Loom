# PUSH-SCOPE — commit/push scope review (read-only)

Original scope-review verdict (historical): **SCOPE READY, NOT READY TO PUSH** while w39 was running. Reviewer: foreground Opus 5.5, no subagents. No Git index, source or worktree changes. No tests rerun. Review time: 2026-11-15, after the final freeze.

## Baseline / remote
- HEAD `a8253c13de7a…` on `main`, **ahead 1** of `origin/main` (origin = github.com/blankroad/Loom, no credentials in the URL). Pushing will also publish the existing local commit a8253c1 (Tauri app). ROOT fetches before pushing.
- Dirty tree: 93 tracked files modified and 1,946 untracked files (2,039 paths). Only the groups below are proposed. Everything else stays exactly as it is.
- Freeze check: the 12 per-file hashes in `qa/final/raw-sources-final.sha256` match the current files (`shasum -c` OK). No file under ui/, crates/, desktop/, .github/ or skills/ is newer than the freeze (08:32:53Z), except the gitignored staged sidecar in `desktop/src-tauri/binaries/`. I could not reproduce the aggregate `ui/src tree 1303905e…` hash because its method is not recorded. The per-file hashes and the mtimes are the evidence for no drift.

## Proposed commits (in order; every commit is file-disjoint, so no `add -p` is needed)
The BenchLight UI changes (`preexisting-ui.diff`, 29 files) and the module-apps changes touch the **same files**, for example ShellApp.tsx, consolePanel.ts and ethPanel.ts. The same holds for BenchLight desktop (`preexisting-desktop.diff`, 10 files) and cross-platform desktop. Only the combined tree was validated, so splitting by hunk would create commits nobody has tested. Each group is therefore one whole-file commit.

| # | Suggested message | Files | Depends on | Validation (existing; not rerun) |
|---|---|---|---|---|
| C1 | `fix(eth): export and handler fixes for workbench export` | 3 (list L1) | — | ETH-EXPORT-FIX(-STATUS).md; included in the final release loomd build |
| C2 | `feat(video,logic): build on non-Unix; explicit unsupported video` | 5 (L2) | — | Windows GNU cross check/clippy/link and Unix video/logic tests rc0 (desktop-cross-platform/STATUS.md). **Windows tests were not executed.** |
| C3 | `feat(psu): unbound gauge manifest + connect/disconnect actions` | 2 (L3) | — | RETURN-C: PSU native 15 tests + fmt; actual PSU SCPI protocol PASS |
| C4 | `feat(ui): Bench Light workbench + permanent module apps` | 79 (L4) | C1 (eth export RPC), C3 (psu manifest) | Final: 69 files / 761 UI tests, tsc, full eslint PASS; independent Delta REVIEW-FIX PASS; browser R6 PASS on UI db441bfb |
| C5 | `feat(desktop): Windows/Ubuntu packaging, CI, Bench Light launch` | 35 (L5) | C2 (CI runs `platform_unsupported`), C4 (embedded UI) | Strict Mac release app 302e5c5b / loomd 38219c57 on the final tree; desktop 26+4+23 tests; actionlint rc0; Ubuntu ARM .deb smoke |
| C6 | `docs(reports): module-apps, Bench Light QA, cross-platform evidence` | 90+52+77 (L6a–c), about 9.7 MB | C1–C5 | curated; see Evidence |

You could merge C1–C3 into one backend commit; keeping them separate is cleaner. Each commit builds on the ones before it. C4 without C1 or C3 would only lose runtime features; there is no compile-time coupling with Rust.

Pre-stage drift check: `tr '\n' '\0' < LIST | xargs -0 shasum -a 256 | shasum -a 256` should give L1 `f1fb34e221f7f41d`, L2 `fef071f4b39538bd`, L3 `576a3daaa7384fb0`, L4 `dd95342430f7db82`, L5 `e52ddca8b103f487`, L6a `7a3bef0635626252` (this one changes when STATUS.md, PUSH-SCOPE.md or native evidence is updated), L6b `7e39b35c05389303`, L6c `438cb1c7cc8d1133`.

## Evidence selection (portable, no broken links)
- **module-apps (L6a):** add this PUSH-SCOPE.md too; all top-level .md files, REPORT.html and every one of its 46 relative refs (all exist; 4.16 MB), plus `qa/final/sum-*.json`, `raw-sources-final.sha256`, `CASE-COLLISION-NOTE.json`, `can-live/*` and `review-corrections/*`. **Excluded:** `qa/archive`, `qa/lqa{1,2,3}-*` (48 MB fixture logs), raw observer `.out` files, `qa_launch_*.py`, `services.body.mjs.orig`, and `preexisting-ui.diff` (508 KB, redundant once committed). `qa/native/` is excluded until w39 finishes.
- **tauri-desktop (L6b):** the 8 modified tracked files, the untracked top-level/qa `.md` files, `qa/REPORT.html` with its 21 refs (`report-assets/final-b1/*`, all exist), and `qa/root-desktop-regression.txt`. **Excluded:** `REPORT-run4-olddark.html`, run4 jpgs, `evidence/native-*` (about 28 MB), `css-before`, `parity/`, `bench-*/`, `launch.css.olddark.bak`. Some .md files mention excluded evidence paths as text; these are not HTML links.
- **desktop-cross-platform (L6c):** .md/.json/.rc/.log files at the top level, `docker/*`, `evidence/{ubuntu-final,windows-gnu-cross,host-unix-tests}`, `actionlint.log`, `appimage-strict-fail-05-dist.log` and `runtime-logs/*`. **Excluded:** `ubuntu-attempt1` and `ubuntu-attempt3-superseded` (including `dist/`), and `preexisting-desktop.diff`.
- `*.log` is gitignored. About 40 listed log files need `git add -f` (HEAD already force-added `ROOT-TESTS.log`). **ROOT decision:** force-add them or drop the logs.
- Credential pattern scan of every L1–L5 file and the module-apps text: 0 real hits. The only hit is the userinfo-rejection test fixtures in `desktop/src-tauri/src/nav.rs` (`user:***@localhost`, `example.com`), which are expected. The QA scan `sum-credential-scan.json` also shows 0 hits.

## Leave untouched (unrelated or older; preserve)
- `docs/impl/{can,core,eth,main,uart,uds,ui}/STATUS.md`, `docs/impl/uds/ORCHESTRATOR-HANDOFF.md` (modified, Nov 11–12 P1/CORE-R1 history) and all untracked `docs/impl/**`, including `docs/impl/main/evidence/ui-workbench/*benchmark-fidelity*` (43 MB) and `docs/impl/main/reports/*.patch`.
- `spikes/eth/`, `work/mockups/`, `work/plans/`, `work/reports/{fidelity,ui-style-survey,validation-report(96 MB),external-simulator-validation,luna-skill-validation,saleae-live,video-capture,workbench-agent-tabs/reuse}`, the untracked `work/reports/logic-live/*` probe dumps (.bin), and top-level `work/reports/*.{html,png,py,json,md}` (Nov 12–14).
- `.pnpm-store/` (pnpm index DB; **not ignored**). Do not stage it. ROOT may add it to `.gitignore` later.
- Ignored and never to be staged: `desktop/src-tauri/binaries/`, `desktop/dist/` (Loom.app, linux-arm64 .deb), node_modules, target.
- **ROOT decision (optional):** `.agents/skills/loom-logic` and `.claude/skills/loom-logic` are 2 symlinks to `../../skills/loom-logic` that c30cf3a missed (the other skills' symlinks are tracked). They are not part of this release; a separate `chore(skills)` commit is possible.

## Not validated / limits (state these truthfully in commit or PR text)
- Native visible Mac Connect is **PENDING** (w39 is running). The browser/protocol checks passed on db441bfb. Do not push until ROOT accepts the native result, then add the curated native summary to L6a.
- Windows: cross-compile, clippy and test linking only. No Windows test run, MSVC/NSIS installer or app launch. The `desktop.yml` native runners have never run.
- Ubuntu x64: configured only. The Ubuntu ARM `.deb` (sha 9b28d36c…) was built **before** the module-apps UI and the PSU change, so it does not reflect C3/C4. AppImage is unsupported and is rejected.
- No physical hardware, no lossless UI recording, and no native performance claims. Simulator and fixture coverage only.

## Exact file lists

### L1 (3 files)
```
crates/loom-eth/src/gateway/export.rs
crates/loom-eth/src/gateway/handlers.rs
crates/loom-eth/tests/gateway.rs
```

### L2 (5 files)
```
crates/loom-logic/tests/engine.rs
crates/loom-video/src/media.rs
crates/loom-video/src/worker.rs
crates/loom-video/tests/fixture_e2e.rs
crates/loom-video/tests/platform_unsupported.rs
```

### L3 (2 files)
```
crates/loom-psu/src/gateway.rs
crates/loom-psu/tests/psu.rs
```

### L4 (79 files)
```
ui/e2e/workbench-ego/services.body.mjs
ui/src/net/api.ts
ui/src/net/client.test.ts
ui/src/net/client.ts
ui/src/net/core.ts
ui/src/net/mux.test.ts
ui/src/net/mux.ts
ui/src/net/protocol.ts
ui/src/panels/can/can.css
ui/src/panels/can/canConnect.ts
ui/src/panels/can/canModule.test.ts
ui/src/panels/can/canTrace.test.ts
ui/src/panels/can/canTrace.ts
ui/src/panels/can/fixtures/qaCanMarker.native.json
ui/src/panels/can/traceModel.ts
ui/src/panels/eth/dialogs.ts
ui/src/panels/eth/eth.css
ui/src/panels/eth/eth.test.ts
ui/src/panels/eth/ethModel.ts
ui/src/panels/eth/ethPanel.ts
ui/src/panels/eth/ethUnbound.test.ts
ui/src/panels/eth/filterBar.ts
ui/src/panels/eth/ifaceConfigs.ts
ui/src/panels/eth/socketDrawer.ts
ui/src/panels/logic/logic.css
ui/src/panels/logic/logicModule.test.ts
ui/src/panels/logic/logicPanel.ts
ui/src/panels/psu/gaugePanel.ts
ui/src/panels/psu/psu.css
ui/src/panels/psu/psuConnect.ts
ui/src/panels/psu/psuModule.test.ts
ui/src/panels/uart/terminalPanel.ts
ui/src/panels/uart/uart.css
ui/src/panels/uart/uartFidelity.test.ts
ui/src/panels/uds/consolePanel.ts
ui/src/panels/uds/uds.css
ui/src/panels/uds/uds.test.ts
ui/src/panels/uds/udsModel.ts
ui/src/panels/uds/udsPrefs.ts
ui/src/panels/uds/udsStateHydrate.test.ts
ui/src/panels/uds/udsTools.ts
ui/src/panels/uds/udsUnbound.test.ts
ui/src/panels/uds/udsWorkbench.test.ts
ui/src/panels/video/video.css
ui/src/panels/video/videoModule.test.ts
ui/src/panels/video/videoPanel.ts
ui/src/sdk/capacityLive.test.ts
ui/src/sdk/context.ts
ui/src/sdk/sdk.test.ts
ui/src/sdk/testing/harness.ts
ui/src/shell/bootstrap.ts
ui/src/shell/dock/activeWorkspace.ts
ui/src/shell/dock/defaultLayout.test.tsx
ui/src/shell/dock/dockHost.ts
ui/src/shell/dock/DockLayout.tsx
ui/src/shell/explorer.css
ui/src/shell/interfaceConnect.css
ui/src/shell/InterfaceConnect.test.tsx
ui/src/shell/InterfaceConnect.tsx
ui/src/shell/moduleApps.test.tsx
ui/src/shell/routes.ts
ui/src/shell/ServiceNav.tsx
ui/src/shell/services.ts
ui/src/shell/serviceSeparation.test.tsx
ui/src/shell/shell.css
ui/src/shell/ShellApp.tsx
ui/src/shell/TopBar.tsx
ui/src/shell/workbenchShell.test.tsx
ui/src/store/testing/fakeNet.ts
ui/src/workbench/eth/bench.css
ui/src/workbench/eth/bench.ts
ui/src/workbench/uart/bench.css
ui/src/workbench/uart/Icon.tsx
ui/src/workbench/uart/icons.ts
ui/src/workbench/uart/recorder.test.ts
ui/src/workbench/uart/recorder.ts
ui/src/workbench/uart/UartWorkbench.tsx
ui/src/workbench/uart/workbench.test.ts
ui/src/workbench/uart/workbench.ts
```

### L5 (35 files)
```
.github/workflows/desktop.yml
desktop/Cargo.lock
desktop/frontend/launch.css
desktop/host/src/http.rs
desktop/host/src/lib.rs
desktop/host/tests/lifecycle.rs
desktop/host/tests/real_loomd.rs
desktop/package-lock.json
desktop/package.json
desktop/README.md
desktop/scripts/e2e/probes.mjs
desktop/scripts/e2e/watch_url.py
desktop/scripts/e2e/wk.mjs
desktop/scripts/package.mjs
desktop/scripts/portable/bundle.mjs
desktop/scripts/portable/lib.mjs
desktop/scripts/portable/stage.mjs
desktop/scripts/portable/test/bundle.test.mjs
desktop/scripts/portable/test/config.test.mjs
desktop/scripts/portable/test/fake-loomd.mjs
desktop/scripts/portable/test/helpers.mjs
desktop/scripts/portable/test/lib.test.mjs
desktop/scripts/portable/test/stage.test.mjs
desktop/scripts/smoke-native.sh
desktop/scripts/stage-loomd.sh
desktop/src-tauri/build.rs
desktop/src-tauri/Cargo.toml
desktop/src-tauri/icons/icon.ico
desktop/src-tauri/src/appimage.rs
desktop/src-tauri/src/config.rs
desktop/src-tauri/src/main.rs
desktop/src-tauri/src/nav.rs
desktop/src-tauri/src/ui_identity.rs
desktop/src-tauri/tauri.linux.conf.json
desktop/src-tauri/tauri.windows.conf.json
```

### L6a (90 files)
```
work/reports/module-apps/can-live/after.sha256
work/reports/module-apps/can-live/before.sha256
work/reports/module-apps/can-live/diff.patch
work/reports/module-apps/can-live/ui-lint.log
work/reports/module-apps/can-live/ui-test.log
work/reports/module-apps/can-live/ui-tsc.log
work/reports/module-apps/CONTRACT.md
work/reports/module-apps/NATIVE-NEXT.md
work/reports/module-apps/QA-RETURN.md
work/reports/module-apps/QA.md
work/reports/module-apps/qa/browser/can-1440.png
work/reports/module-apps/qa/browser/can-900.png
work/reports/module-apps/qa/browser/eth-1440.png
work/reports/module-apps/qa/browser/eth-900.png
work/reports/module-apps/qa/browser/eth-marker.png
work/reports/module-apps/qa/browser/logic-1440.png
work/reports/module-apps/qa/browser/logic-900.png
work/reports/module-apps/qa/browser/logic-waveform.png
work/reports/module-apps/qa/browser/psu-1440.png
work/reports/module-apps/qa/browser/psu-900.png
work/reports/module-apps/qa/browser/psu-isolated-live.png
work/reports/module-apps/qa/browser/uart-1440.png
work/reports/module-apps/qa/browser/uart-900.png
work/reports/module-apps/qa/browser/uart-ack-confirmed.png
work/reports/module-apps/qa/browser/uds-1440.png
work/reports/module-apps/qa/browser/uds-900.png
work/reports/module-apps/qa/browser/video-1440.png
work/reports/module-apps/qa/browser/video-900.png
work/reports/module-apps/qa/browser/video-frame-match.json
work/reports/module-apps/qa/browser/video-frozen.png
work/reports/module-apps/qa/browser/zero-channel-1440.json
work/reports/module-apps/qa/browser/zero-channel-900.json
work/reports/module-apps/qa/CASE-COLLISION-NOTE.json
work/reports/module-apps/qa/ETH-MARKER.json
work/reports/module-apps/qa/final/browser/catchup-after.json
work/reports/module-apps/qa/final/browser/catchup-after.png
work/reports/module-apps/qa/final/browser/integrated-eight-proof.png
work/reports/module-apps/qa/final/raw-sources-final.sha256
work/reports/module-apps/qa/final/sum-build-final.json
work/reports/module-apps/qa/final/sum-catchup.json
work/reports/module-apps/qa/final/sum-cleanup.json
work/reports/module-apps/qa/final/sum-credential-scan.json
work/reports/module-apps/qa/final/sum-gates-final.json
work/reports/module-apps/qa/final/sum-prepare.json
work/reports/module-apps/qa/final/sum-publish-stage.json
work/reports/module-apps/qa/final/sum-published-running.json
work/reports/module-apps/qa/final/sum-published.json
work/reports/module-apps/qa/final/sum-r6.json
work/reports/module-apps/qa/final/sum-user-served-ui.json
work/reports/module-apps/qa/final/sum-user-stop.json
work/reports/module-apps/qa/fix/browser/can-reply.png
work/reports/module-apps/qa/fix/browser/psu-after-tour.png
work/reports/module-apps/qa/fix/browser/tour-zero.json
work/reports/module-apps/qa/fix/browser/uart-two-acks.png
work/reports/module-apps/qa/fix/browser/uds-before-did.png
work/reports/module-apps/qa/fix/browser/uds-vin.png
work/reports/module-apps/qa/fix/BUILD.json
work/reports/module-apps/qa/UART-MARKER.json
work/reports/module-apps/qa/UDS-DID.json
work/reports/module-apps/REPORT.html
work/reports/module-apps/RESUME-NATIVE.md
work/reports/module-apps/RETURN-A.md
work/reports/module-apps/RETURN-B.md
work/reports/module-apps/RETURN-C.md
work/reports/module-apps/RETURN-CAN-LIVE.md
work/reports/module-apps/RETURN-REVIEW-CORRECTIONS.md
work/reports/module-apps/RETURN-UDS-STATE.md
work/reports/module-apps/review-corrections/after.sha256
work/reports/module-apps/review-corrections/before.sha256
work/reports/module-apps/review-corrections/diff.patch
work/reports/module-apps/review-corrections/f1-negative.log
work/reports/module-apps/review-corrections/f2-negative.log
work/reports/module-apps/review-corrections/f3-negative.log
work/reports/module-apps/review-corrections/f4-context-negative.log
work/reports/module-apps/review-corrections/f4-negative.log
work/reports/module-apps/review-corrections/LOGS.sha256
work/reports/module-apps/review-corrections/ui-lint.log
work/reports/module-apps/review-corrections/ui-test.log
work/reports/module-apps/review-corrections/ui-tsc.log
work/reports/module-apps/REVIEW-FIX.md
work/reports/module-apps/STATUS.md
work/reports/module-apps/TASK-a.md
work/reports/module-apps/TASK-b.md
work/reports/module-apps/TASK-c.md
work/reports/module-apps/TASK-CAN-LIVE.md
work/reports/module-apps/TASK-NATIVE-ACCEPTANCE.md
work/reports/module-apps/TASK-QA.md
work/reports/module-apps/TASK-REVIEW-CORRECTIONS.md
work/reports/module-apps/TASK-REVIEW-FIX.md
work/reports/module-apps/TASK-UDS-STATE.md
```

### L6b (52 files)
```
work/reports/tauri-desktop/BENCH-LIGHT-TASK.md
work/reports/tauri-desktop/DOCK-ROUTE-FIX-STATUS.md
work/reports/tauri-desktop/DOCK-ROUTE-FIX.md
work/reports/tauri-desktop/ETH-EXPORT-FIX-STATUS.md
work/reports/tauri-desktop/ETH-EXPORT-FIX.md
work/reports/tauri-desktop/FINAL-PARITY-QA-TASK.md
work/reports/tauri-desktop/MACRO-EXPECT-FIX-STATUS.md
work/reports/tauri-desktop/MACRO-EXPECT-FIX.md
work/reports/tauri-desktop/qa/evidence/dist-manifest.json
work/reports/tauri-desktop/qa/evidence/dist-manifest.json.bundle.json
work/reports/tauri-desktop/qa/FINAL-PREP.md
work/reports/tauri-desktop/qa/FINDING-ENTRY-ROUTE.md
work/reports/tauri-desktop/qa/FINDING-GUARD-308.md
work/reports/tauri-desktop/qa/FINDING-MACRO-EXPECT.md
work/reports/tauri-desktop/qa/FINDING-TSC-ETH.md
work/reports/tauri-desktop/qa/FINDING-UART-SCOPE-LAYOUT.md
work/reports/tauri-desktop/qa/INTEGRATION-HANDOFF.md
work/reports/tauri-desktop/qa/NATIVE-REQUEST.md
work/reports/tauri-desktop/qa/PARITY-PLAN.md
work/reports/tauri-desktop/qa/READY.json
work/reports/tauri-desktop/qa/report-assets/final-b1/00-canonical-eth.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/00-canonical-uart.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/00-canonical-uds.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/01-ordinary-reopen.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/02-ordinary-after-reconnect.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/03-guard-308-refusal.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/10-uart-session-dialog.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/11-uart-dut2-opened.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/12-uart-macro-dialog.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/13-uart-macro-wait-defect.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/14-uart-stale-strips.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/15-uart-hidden-tab-excluded.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/16-uart-rec-dialog.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/20-eth-stream-live.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/21-eth-detail-hex.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/22-eth-filter.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/23-eth-channel-added.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/24-eth-socket-udp.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/30-uds-config.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/31-uds-f190-confirmed.jpg
work/reports/tauri-desktop/qa/report-assets/final-b1/32-uds-nrc.jpg
work/reports/tauri-desktop/qa/REPORT.html
work/reports/tauri-desktop/qa/RESUME-FINAL2.md
work/reports/tauri-desktop/qa/RESUME-TASK.md
work/reports/tauri-desktop/qa/root-desktop-regression.txt
work/reports/tauri-desktop/qa/ROOT-OBSERVATIONS.md
work/reports/tauri-desktop/qa/ROOT-REVIEW.md
work/reports/tauri-desktop/qa/STATUS-HISTORY.md
work/reports/tauri-desktop/qa/STATUS.md
work/reports/tauri-desktop/RUN-STATE.json
work/reports/tauri-desktop/STARTUP-ROUTE-FIX.md
work/reports/tauri-desktop/UDS-PARITY-TASK.md
```

### L6c (77 files)
```
work/reports/desktop-cross-platform/CI.md
work/reports/desktop-cross-platform/docker/Dockerfile.ubuntu2204
work/reports/desktop-cross-platform/docker/run-ubuntu.sh
work/reports/desktop-cross-platform/docker/ubuntu-build.sh
work/reports/desktop-cross-platform/evidence/actionlint.log
work/reports/desktop-cross-platform/evidence/appimage-strict-fail-05-dist.log
work/reports/desktop-cross-platform/evidence/host-unix-tests/h-clippy.log
work/reports/desktop-cross-platform/evidence/host-unix-tests/h-test.log
work/reports/desktop-cross-platform/evidence/host-unix-tests/loom-ci-host-target-evidence.txt
work/reports/desktop-cross-platform/evidence/ubuntu-final/00-snapshot.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/01-provenance.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/02-npm-ci.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/03-pnpm-install.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/04-packaging-tests.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/05-dist.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/06-check.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/07-desktop-tests.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/08-manifest-deb-only.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/10-deb-contents.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/11-deb-install.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/12-deb-verify-loomd.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/12b-installed-identity.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/13-deb-launch-smoke.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/15-artifacts.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/99-snapshot-drift.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/attempt2-provenance-fail.run.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/compare-after-doc-edits.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/smoke-08-manifest-deb-only.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/smoke-10-deb-contents.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/smoke-11-deb-install.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/smoke-12-deb-verify-loomd.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/smoke-12b-installed-identity.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/smoke-13-deb-launch-smoke.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/smoke-15-artifacts.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/smoke-99-snapshot-drift.log
work/reports/desktop-cross-platform/evidence/ubuntu-final/snapshot-drift-after-doc-edits.diff
work/reports/desktop-cross-platform/evidence/ubuntu-final/snapshot-drift.diff
work/reports/desktop-cross-platform/evidence/ubuntu-final/snapshot.sha256
work/reports/desktop-cross-platform/evidence/ubuntu-final/source-now.sha256
work/reports/desktop-cross-platform/evidence/ubuntu-final/steps-smoke.tsv
work/reports/desktop-cross-platform/evidence/ubuntu-final/steps.tsv
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/a2-loomd-check.log
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/b2-workspace-check.log
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/before-fix-loomd-check.log
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/before-fix-workspace-check.log
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/c-clippy-video-logic.log
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/c2-clippy-video-logic.log
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/d2-loomd-build.log
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/e2-test-norun.log
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/evidence-after-fix.txt
work/reports/desktop-cross-platform/evidence/windows-gnu-cross/evidence-after-fix2.txt
work/reports/desktop-cross-platform/INTERFACE.md
work/reports/desktop-cross-platform/linux-root-check.log
work/reports/desktop-cross-platform/mac-current-build.log
work/reports/desktop-cross-platform/mac-current-build.rc
work/reports/desktop-cross-platform/mac-current-manifest.json
work/reports/desktop-cross-platform/mac-current-manifest.json.bundle.json
work/reports/desktop-cross-platform/mac-current-verify.log
work/reports/desktop-cross-platform/mac-current-verify.rc
work/reports/desktop-cross-platform/MAC-LAUNCH.md
work/reports/desktop-cross-platform/PACKAGING.md
work/reports/desktop-cross-platform/REQUEST-backend-windows.md
work/reports/desktop-cross-platform/REQUEST-packaging-appimage.md
work/reports/desktop-cross-platform/REVIEW.md
work/reports/desktop-cross-platform/runtime-logs/mac-clippy.log
work/reports/desktop-cross-platform/runtime-logs/mac-desktop-test.log
work/reports/desktop-cross-platform/runtime-logs/mac-host-test.log
work/reports/desktop-cross-platform/runtime-logs/mac-release-check.log
work/reports/desktop-cross-platform/runtime-logs/runtime-vs-HEAD.diff
work/reports/desktop-cross-platform/runtime-logs/win-gnu-check.log
work/reports/desktop-cross-platform/runtime-logs/win-gnu-clippy.log
work/reports/desktop-cross-platform/runtime-logs/win-gnu-test-build.log
work/reports/desktop-cross-platform/RUNTIME.md
work/reports/desktop-cross-platform/STATUS.md
work/reports/desktop-cross-platform/TASK-ci.md
work/reports/desktop-cross-platform/TASK-packaging.md
work/reports/desktop-cross-platform/TASK-runtime.md
```


## ROOT final acceptance / publication decision

2026-11-15: NATIVE-RETURN.md PASS independently reviewed and accepted. Actual seven-module native Connect/Start and fresh same-daemon receive, selected3paneIntegrated and layout persistence complete. App/daemon kept, all QA fixtures/observers stopped; QA local sessions/profile/views removed. Worker normal exit verified agent_not_found and w39 closed; scope reviewer w3A similarly closed. Native source unchanged, L1–L5 aggregate hashes match above. User explicitly authorized commit+push. ROOT selects C1–C6 full-file commits, including curated ignored validation logs via explicit force-add, all native evidence allowlist plus report-rendered.png. No optional skill symlinks or unrelated docs/impl changes. Platform limits and historical macro/REC backlog remain as stated. Final remote SHA is verified through Git, not embedded in this document.

Publication addendum: C5 newly staged test lib.test.mjs had one extra EOF blank line not covered by the earlier tracked-only diff check. Foreground Opus w3B removed that byte; focused9tests PASS, PUBLISH-FORMAT.md records hashes. Runtime/UI source and final bundle unchanged. Production commits pass staged whitespace checks. Evidence artifacts retain original CR/whitespace in logs, patch contexts and AX captures; these are intentionally not normalized. Documentation/HTML/JSON whitespace is checked separately. Worker normal exit verified and w3B closed.
