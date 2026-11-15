# tauri-qa-opus (w2K) STATUS

State: **BLOCKED_ENV (screen locked)** — QA e2e window ready (on-top, fresh UI); visible suite auto-runs on unlock (≤30 min)
Owns: desktop/scripts/**, desktop/README.md, work/reports/tauri-desktop/qa/**

## Packaging contract proposal (for tauri-app-opus / ROOT)
- `desktop/scripts/stage-loomd.sh` builds ui/dist only when stale, then `cargo build --release -p loom-gateway --bin loomd`
  in the MAIN workspace (its own target dir, not /tmp/loom-tauri-build), and stages the binary as Tauri externalBin:
  `desktop/src-tauri/binaries/loomd-<host-triple>` (only that generated file; gitignore suggested) + `loomd.stamp.json`
  (sha256, ui dist hash, git baseline). App config: `bundle.externalBin: ["binaries/loomd"]` → `Contents/MacOS/loomd` in .app.
- Single loomd binary suffices: workers re-exec `loomd worker <kind>` (pool.rs current_exe fallback). UI is embedded at
  compile time by loom-ui-assets (rust-embed of ui/dist) → dist must be built BEFORE cargo build of loomd.
- External runtime tools NOT bundled: ffmpeg (video), sigrok-cli (logic). Documented as unverified/optional.

## FINDING Q1 (2026-11-15) — existing target/debug/loomd embeds a STALE UI
verify-loomd.sh: /ui/index.html served by target/debug/loomd (sha256 7f3ad381…) references index-DVbGndTA.js;
ui/dist has index-COcBrDfS.js (4/6 files mismatch). Cause: rust-embed does not track files added to ui/dist.
Fix in stage-loomd.sh: always `cargo clean -p loom-ui-assets` (cheap; only ui-assets + gateway recompile) and
auto-verify byte identity after staging. **Do not bundle a loomd that has not passed verify-loomd.sh.**
Staged file: desktop/src-tauri/binaries/loomd-aarch64-apple-darwin + loomd.stamp.json (app owner: please gitignore
`desktop/src-tauri/binaries/loomd-*` and stamp, and set bundle.externalBin ["binaries/loomd"]).

## Coordination (QA → app owner/ROOT), 2026-11-15
- QA now running `desktop/scripts/build-app.sh --debug` (tauri-cli 2.12.4 task-local at /tmp/loom-tauri-build/tools,
  CARGO_TARGET_DIR=/tmp/loom-tauri-build, mkdir lock /tmp/loom-tauri-build/.cargo-desktop.lock — app owner please
  take the same lock for cargo runs). Output: /tmp/loom-tauri-build/debug/bundle/macos/Loom.app (dev-labelled).
- QA native runs: own LOOM_HOME under the QA scratchpad (never /tmp/loom-app-iso); the app is quit ONLY by QA's own pid
  (NSRunningApplication.terminate == Cmd-Q). Never quit by name/bundle id; never pkill.

## Progress (2026-11-15)
- Scripts: preflight.sh, stage-loomd.sh, verify-loomd.sh, build-app.sh, verify-bundle.sh, smoke-native.sh, lib/uart_fixture.py.
- Built /tmp/loom-tauri-build/debug/bundle/macos/Loom.app via build-app.sh --debug (34 s; tauri-cli 2.12.4 task-local):
  verify-bundle PASS (bundle id, main exe arm64, bundled loomd sha == stamp 98dd890e…, embedded UI 6/6 identical, no
  credentials/home data); codesign WARN (local ad-hoc: "code has no resources but signature indicates they must be present").
- smoke-native start PASS: absent daemon cold-started from bundled loomd (1 s), native viewer visible, CLI↔pty round trip.
- WebDriver evaluation: official docs (v2.tauri.app/develop/tests/webdriver) — macOS only via @wdio/tauri-service embedded
  provider, which requires linking `tauri-plugin-wdio-webdriver` into the app (an in-app WebDriver server). Not added:
  product-surface/security cost; native proof uses CUA (ROOT) + daemon-side viewer/rx assertions. Could be a
  debug-only cargo feature later (app owner).
- smoke-native.sh: fixed ROOT-reported one-shot `$(find_app …)` inside wait_for (has_app helper; start/reopen/error-start).
  The live session (pid 10201) predates the fix but its assertions all ran on the real pid.
- hygiene PASS on live session (daemon.json 0600; no token/#boot= in app stdout/stderr, loomd.log, evidence, ps args).
- Window diagnosis: CG window #1587 on-screen for pid 10201 (see NATIVE-REQUEST.md). CUA lookup issue, not a missing window.
- Independent source review: SOURCE-REVIEW.md (no blockers; setsid parity, reconnect-after-daemon-restart, codesign seal notes).
- desktop/README.md written (build/run/env vars/Finder PATH + [video] ffmpeg / [logic] sigrok_cli/validation).
- Waiting: ROOT native steps 1–5 (NATIVE-REQUEST.md). QA monitor watches ROOT-OBSERVATIONS.md, device RX bytes, app exit.

## Update (CUA blocked → WK WebDriver path), 2026-11-15 ~07:50
- ROOT CUA: getApp(path|bundle id) cgWindowNotFound, Dock activation timeout → tool-binding blocker (window exists:
  CG #1587). Run1 evidence kept: evidence/native-run1-cua-blocked/ (start PASS, hygiene PASS; no UI input done).
- Review P1–P5 (host/REVIEW-PACKAGING.md) integrated: P1 find_app = exact main-executable comm + exact LOOM_HOME env
  token; P2 build-app forwards `-- --locked`, missing/changed desktop/Cargo.lock = exit 1; P3 stage-loomd main-workspace
  cargo runs with `env -u CARGO_TARGET_DIR` and build-app exports the desktop target only after staging; P4 wait_for uses
  a $SECONDS deadline + `bounded` per-probe watchdog (status probe 5 s, CLI --timeout 4); P5 token patterns via
  `grep -f <(…)` (never argv) for files and ps.
- App owner wired feature `e2e` (tauri-plugin-wdio-webdriver =1.5.1, debug-only, compile_error in release, server only
  with LOOM_DESKTOP_QA_WEBDRIVER_PORT). QA runner: desktop/scripts/e2e/wk.mjs (zero-dependency W3C client; never reads
  URL/title/source/cookies). QA build in progress: build-app.sh --debug --features e2e --out /tmp/loom-tauri-build/qa/Loom-QA.app.
- desktop/dist/Loom.app (app owner's release artifact) verified by verify-dist.sh: strict codesign PASS (adhoc, runtime),
  no WebDriver plugin, loomd identity LC_UUID 2BCDE92C… + __TEXT,__text/__const hashes == staged pre-sign binary,
  embedded UI 6/6 identical. Manifest: evidence/dist-manifest.json (pre-sign 98dd890e…, bundled 34c2c1d8…).
  NOTE: the bundled loomd is the verified DEBUG-profile daemon (release app shell + debug loomd). A release loomd needs
  `stage-loomd.sh --profile release` (cold release build of the main workspace) — not done, ROOT decision.
- Reconnect menu (Connection → Reconnect, Cmd-Shift-R) is native-menu only → not reachable by WK WebDriver; will be
  reported UNVERIFIED unless ROOT CUA can bind. Relaunch/reuse + error→Retry are covered by WK tests.

## Update ~07:50 — fresh UI + on-top knob; environment blocker = LOCKED SCREEN
- Restaged loomd after A's ui/src presence fix: ui/dist digest 6fdd549d…, staged loomd 49803a53…, embedded UI 6/6 identical
  (stage 9 s). QA e2e build /tmp/loom-tauri-build/qa/Loom-QA.app re-built: bundled b96de8e8… == ad-hoc re-sign of
  staged; signature valid; WebDriver INFO (test build). build-app-e2e-2.log.
- Run3 (evidence/native-current): QA app pid with LOOM_DESKTOP_QA_ON_TOP=1 → CG window layer 5 (floating), env verified.
  WK still `visibilityState=hidden`, `hasFocus=false` because **console session is LOCKED and display ASLEEP**
  (CGSSessionScreenIsLocked=1, CGDisplayIsAsleep, loginwindow layer 2004 covering 1920×1080). WebKit is correct;
  QA will not wake/unlock the Mac. Daemon now reports `viewers visible:0` and agent envelopes `visibility=not_visible`
  → A's presence fix verified natively (before the fix the same situation reported visible:1).
- Daemon/CLI/device parts of run3 PASS (absent daemon start 1 s from bundled loomd; channel dut; CLI round trip).
- desktop/scripts/e2e/run-visible-suite.sh armed with --wait-unlock 1800: as soon as the session is unlocked and the
  display awake it runs ready → rx → keys → Tab → CLI→UI → quit/after-close → reopen/late-join rx → error/Retry → hygiene,
  logging to evidence/native-current/suite.log + wk/*.png (real WKWebView snapshots).
- **NEED (user/ROOT): unlock the Mac / wake the display** (no other action needed; the QA window floats on top).
- Earlier run2 (evidence/native-run2-occluded) PASS without visibility dependence: quit (AppKit terminate, Cmd-Q path)
  → app exit, daemon pid alive, channel streaming; relaunch reused same daemon; error view (actionable, no credentials)
  → Retry (WK click) → exact explicit daemon bin started → UI ready. RX/keys there = INVALID visibility env (occluded).

## Archived STATUS.md (checkpoint 07:54, before run4)
# tauri-qa-opus (w2K) STATUS — concise (history: STATUS-HISTORY.md)

**State: PENDING_NATIVE (checkpointed) — screen locked/asleep, user has not responded. Not READY/DONE.**
Waiting for user to unlock/wake (ROOT asked). Native visible tests run only after `lib/screen_state.swift` = unlocked+awake.

## Checkpoint (live session removed)
QA app pid 41482, daemon pid 41514 and fixture pid 41452 were stopped with `smoke-native.sh cleanup` (own home only). The
home was removed and the state archived at evidence/native-run3-locked/. No QA waiters or processes remain, and ports
47941-47942 are free. The Ego space was finished (no QA spaces remain). The user/report spaces 0, 10, 11 and 12 were
untouched. Kept: /tmp/loom-tauri-build/qa/Loom-QA.app. Resume: RESUME.md.

## Verified
| Area | Result | Evidence |
|---|---|---|
| Staging | loomd staged with embedded UI byte-identical to ui/dist (6/6). Caught an earlier STALE embedded UI in target/debug/loomd and fixed it. | stage-loomd/verify-loomd |
| QA e2e bundle | Bundled loomd = staged (deterministic ad-hoc re-sign); signature valid; WebDriver present (INFO, test build) | build-app-e2e-2.log |
| desktop/dist/Loom.app | Strict codesign PASS; no WebDriver; loomd LC_UUID 62C5982E… and __text/__const = staged pre-sign; embedded UI 6/6 (fresh UI, digest 6fdd549d) | evidence/dist-manifest.json |
| Absent-daemon start | Bundled loomd started by the app in about 1 s | run2, run3 |
| Quit persistence | AppKit terminate (Cmd-Q path): app exits; daemon, channel and device keep streaming | run2 |
| Relaunch | Same daemon reused; single daemon | run2 |
| Error/Retry | Actionable error with no credentials; WK Retry starts the exact explicit daemon bin; UI loads | run2 |
| Presence fix (A, ui/src) | Natively: hidden window → daemon `visible:0`, agent envelope `visibility=not_visible` | run3 |
| Hygiene | No token or #boot= in app stdout/stderr, loomd.log, evidence, ps args (token pattern never in argv) | run1–2 |
| Browser parity | Chromium (Ego) on the same daemon renders live UART RX | run2/ego |

## Pending / not verified
- PENDING (needs unlocked+awake): WK UART RX render, keyboard + Tab → device, CLI→UI live, relaunch late-join render,
  final native screenshots. Earlier RX/key FAILs are labelled INVALID visibility env (occluded/locked), not product.
- UNVERIFIED: Connection → Reconnect (native menu only; WebDriver cannot click native menus; CUA cannot bind the window).
- UNVERIFIED: CUA/AX native input: cgWindowNotFound, a tool-binding limitation; the window exists per CGWindowList.
- Not done: release-profile loomd (the dist ships the verified debug-profile daemon; ROOT decision); CAN/UDS/video smoke.

## Handoff notes
- /tmp/loom-tauri-build/debug/bundle/macos/Loom.app and debug/loom-desktop currently contain the e2e build. Rebuild with
  `build-app.sh --debug` (no features) before using it as a dev app.
- Scripts: desktop/scripts/{preflight,stage-loomd,verify-loomd,build-app,verify-bundle,verify-dist,smoke-native}.sh,
  e2e/{wk.mjs,run-visible-suite.sh}, lib/{uart_fixture.py,winlist.swift,quit_app.swift,screen_state.swift}; desktop/README.md.
- Reviews: SOURCE-REVIEW.md (QA); host/REVIEW-PACKAGING.md P1–P5 all integrated.

## Final fixes (ROOT review), 2026-11-15
- run-visible-suite.sh: repo root = scripts/../.. (was desktop/); `export QA_RUN="$RUN"` before smoke calls.
- README: sequential error-start/error-fix/error-verify; concrete wk.mjs examples; Run → desktop/dist/Loom.app +
  Connection → Reconnect; signing scope = ad-hoc only, Developer ID/notarization excluded, redistribution unverified;
  verify-bundle documents post-sign identity (deterministic re-sign) instead of raw hash == stamp.
- desktop/.gitignore (ownership transferred to QA): + target/, __pycache__/, *.pyc. Removed the __pycache__ created by
  QA's py_compile check.
- bash -n: all 8 shell scripts OK; node --check wk.mjs OK; py_compile uart_fixture.py OK.
- ROOT independent: combined Rust tests 34 PASS; verify-dist PASS with 49803a53 daemon + fresh UI 6/6 (ROOT-TESTS.log).
- Native visible suite still PENDING the user unlock. The live QA session stays (pids above) until ROOT checkpoints:
  then `desktop/scripts/smoke-native.sh cleanup`, and later a fresh `start --wd` + run-visible-suite.sh.


---
## Archived 2026-11-15 by bench-final-qa-opus (w2R): STATUS.md as of role handoff

# tauri-qa-opus (w2K) STATUS — concise (history: STATUS-HISTORY.md)

**State: ROLE_HANDOFF — see INTEGRATION-HANDOFF.md (UI identity guard + download glue + light launch done, unit tests PASS; final build/parity pending UI READY). No QA processes; own desktop/target removed.**

## Run4 (2026-11-15 10:55–10:58 KST, unlocked+awake, no source changes since a8253c1)
Preconditions passed: screen_state, stage-loomd --check, and verify-bundle (Loom-QA.app: loomd b96de8e8… = ad-hoc
re-sign of staged 49803a53…, UI 6/6). Evidence: evidence/native-current/.
- PASS: cold start (bundled loomd 77778, 2 s); WK live RX (`ack:qa-cli-105543`, tick 13→14); CLI→UI `ack:qa-cli-2`;
  quit → daemon, channel and device survive; reopen reuses the daemon; late-join RX after relaunch (tick 39→40);
  missing-bin error → Retry → exact bin; hygiene.
- PASS (ROOT CUA, native OS keys): `root-native-1⏎` and `root-tab⇥⏎` reached the pty (`\t` = 0x09) and the CLI saw both acks.
- FAIL (test driver, not product): WebDriver send-keys delivered only Enter/Tab/Enter (`\r\r\t\r`) and no printable
  text to xterm in WK. Native typing supersedes it for acceptance.
- Note: the reopened app 78725 and the error app 78964 both exited by 10:57 without any QA quit (cause not investigated,
  per ROOT). Live now: plain window pid 79705 (no WebDriver, layer 0), home lqa.a6y51Y, daemon 77778, port 52212.
  Details are in NATIVE-REQUEST.md.
- Pending: ROOT native Connection → Reconnect; final cleanup after NATIVE_CHECKS_DONE.
- Script change: smoke-native.sh `reopen --plain` (normal window for CUA). bash -n OK.

## Earlier checkpoint (pre-run4)
## Verified
| Area | Result | Evidence |
|---|---|---|
| Staging | loomd staged with embedded UI byte-identical to ui/dist (6/6). Caught an earlier STALE embedded UI in target/debug/loomd and fixed it. | stage-loomd/verify-loomd |
| QA e2e bundle | Bundled loomd = staged (deterministic ad-hoc re-sign); signature valid; WebDriver present (INFO, test build) | build-app-e2e-2.log |
| desktop/dist/Loom.app | Strict codesign PASS; no WebDriver; loomd LC_UUID 62C5982E… and __text/__const = staged pre-sign; embedded UI 6/6 (fresh UI, digest 6fdd549d) | evidence/dist-manifest.json |
| Absent-daemon start | Bundled loomd started by the app in about 1 s | run2, run3 |
| Quit persistence | AppKit terminate (Cmd-Q path): app exits; daemon, channel and device keep streaming | run2 |
| Relaunch | Same daemon reused; single daemon | run2 |
| Error/Retry | Actionable error with no credentials; WK Retry starts the exact explicit daemon bin; UI loads | run2 |
| Presence fix (A, ui/src) | Natively: hidden window → daemon `visible:0`, agent envelope `visibility=not_visible` | run3 |
| Hygiene | No token or #boot= in app stdout/stderr, loomd.log, evidence, ps args (token pattern never in argv) | run1–2 |
| Browser parity | Chromium (Ego) on the same daemon renders live UART RX | run2/ego |

## Pending / not verified
- PENDING (needs unlocked+awake): WK UART RX render, keyboard + Tab → device, CLI→UI live, relaunch late-join render,
  final native screenshots. Earlier RX/key FAILs are labelled INVALID visibility env (occluded/locked), not product.
- UNVERIFIED: Connection → Reconnect (native menu only; WebDriver cannot click native menus; CUA cannot bind the window).
- UNVERIFIED: CUA/AX native input: cgWindowNotFound, a tool-binding limitation; the window exists per CGWindowList.
- Not done: release-profile loomd (the dist ships the verified debug-profile daemon; ROOT decision); CAN/UDS/video smoke.

## Handoff notes
- /tmp/loom-tauri-build/debug/bundle/macos/Loom.app and debug/loom-desktop currently contain the e2e build. Rebuild with
  `build-app.sh --debug` (no features) before using it as a dev app.
- Scripts: desktop/scripts/{preflight,stage-loomd,verify-loomd,build-app,verify-bundle,verify-dist,smoke-native}.sh,
  e2e/{wk.mjs,run-visible-suite.sh}, lib/{uart_fixture.py,winlist.swift,quit_app.swift,screen_state.swift}; desktop/README.md.
- Reviews: SOURCE-REVIEW.md (QA); host/REVIEW-PACKAGING.md P1–P5 all integrated.

## Final fixes (ROOT review), 2026-11-15
- run-visible-suite.sh: repo root = scripts/../.. (was desktop/); `export QA_RUN="$RUN"` before smoke calls.
- README: sequential error-start/error-fix/error-verify; concrete wk.mjs examples; Run → desktop/dist/Loom.app +
  Connection → Reconnect; signing scope = ad-hoc only, Developer ID/notarization excluded, redistribution unverified;
  verify-bundle documents post-sign identity (deterministic re-sign) instead of raw hash == stamp.
- desktop/.gitignore (ownership transferred to QA): + target/, __pycache__/, *.pyc. Removed the __pycache__ created by
  QA's py_compile check.
- bash -n: all 8 shell scripts OK; node --check wk.mjs OK; py_compile uart_fixture.py OK.
- ROOT independent: combined Rust tests 34 PASS; verify-dist PASS with 49803a53 daemon + fresh UI 6/6 (ROOT-TESTS.log).
- Native visible suite still PENDING the user unlock. The live QA session stays (pids above) until ROOT checkpoints:
  then `desktop/scripts/smoke-native.sh cleanup`, and later a fresh `start --wd` + run-visible-suite.sh.


## Archived: NATIVE-REQUEST.md run4 (apps/daemon of run4 no longer running)

# NATIVE-REQUEST — run4 — STAGE 2: plain window LIVE, held for ROOT (10:58 KST)

**pid 77745 is gone.** The suite quit it at 10:56 (`smoke quit`, AppKit terminate) before ROOT's hold message arrived.
The suite then reopened the app as 78725 and started the error-scenario app 78964. Both exited by themselves by 10:57
(not quit by QA scripts; cause not investigated, per ROOT). QA then relaunched one plain window:

## Live QA app (hold — QA makes no UI/route changes until ROOT writes NATIVE_CHECKS_DONE)
- App: `/private/tmp/loom-tauri-build/qa/Loom-QA.app/Contents/MacOS/loom-desktop`, bundle id `local.loom.desktop`
- App pid **79705**: window #1704, 1440×900 at (240,52), **layer 0 (normal)**, **no WebDriver** (`reopen --plain`)
- LOOM_HOME `/var/folders/ln/bc49b46j5w932sv2klk5zdjh0000gn/T/lqa.a6y51Y`
- Daemon pid **77778** (bundled loomd, reused; single daemon), port **52212**
- Fixture pid 77717, `/dev/ttys003`, channel `dut`
- Leftover own error-scenario daemon 79036 (home lqe.PiNbQy, port 52315, no app): stopped at cleanup

## Suite results (evidence/native-current/results.txt, suite.log, wk/*.png)
| Check | Result |
|---|---|
| Cold start from bundled loomd (2 s); visible viewer | PASS |
| WK renders live RX: `ack:qa-cli-105543`, tick 13→14; after relaunch `ack:qa-cli-2`, tick 39→40 | PASS (visibility=visible, rAF≈61–63/s) |
| CLI→UI shared stream `ack:qa-cli-2` | PASS |
| AppKit quit → daemon, channel and device survive; viewer 0; reopen reuses daemon 77778 | PASS |
| Missing daemon binary → Failed view + Retry → exact bin started → viewer attached | PASS |
| Credential hygiene (logs, evidence, argv); daemon.json mode 600 | PASS |
| **WK DOM input** (WebDriver send-keys `qa-typed-1⏎`, `qa-tab⇥⏎`) | **FAIL (driver)**: the fixture got only `\r\r\t\r`, so control keys reached the device but printable text did not. Limitation of the WebDriver plugin's key synthesis |
| **Native OS keys (ROOT CUA)** `root-native-1⏎`, `root-tab⇥⏎` | **PASS**: device rx `root-native-1\r`, `root-tab\t\r` (0x09); CLI sees both acks |

ROOT assertion commands (read-only on the app):
`desktop/scripts/smoke-native.sh check-typed <text> [--tab]` · `smoke-native.sh send <text>` · `smoke-native.sh status`
Pending ROOT: native Connection → Reconnect (menu/shortcut). Write `NATIVE_CHECKS_DONE` in ROOT-OBSERVATIONS.md when finished.
QA is not polling (no clock loops). After NATIVE_CHECKS_DONE, resume QA. Cleanup is these two commands (only this run's
app, daemons 77778/79036, fixture and homes are touched; evidence is kept):
`desktop/scripts/smoke-native.sh cleanup && mv work/reports/tauri-desktop/qa/evidence/native-current work/reports/tauri-desktop/qa/evidence/native-run4-visible`
Report: qa/REPORT.html (actual screenshots in report-assets/).
