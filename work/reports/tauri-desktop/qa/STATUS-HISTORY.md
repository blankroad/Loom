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
