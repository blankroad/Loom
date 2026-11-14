# NEED_ROOT_NATIVE — QA native CUA request (window stable since 07:29)

Target = QA instance ONLY:
- Bundle: `/private/tmp/loom-tauri-build/debug/bundle/macos/Loom.app` (bundle id `local.loom.desktop`, DEBUG/dev-labelled
  build of baseline c7112c6 + desktop/, verified by verify-bundle.sh)
- App pid **10201** — select its window by pid. (App owner's instance = `/tmp/loom-tauri-build/debug/loom-desktop`,
  home /tmp/loom-app-iso — do NOT touch.)
- LOOM_HOME `/private/tmp/claude-501/-Users-ctmctm-workspace-Loom/75228f2f-6a9a-46c8-ba97-2b52093e7c01/scratchpad/lqa.hpwaC3`; daemon pid 10221 = bundled `Contents/MacOS/loomd`, cold-started by the app.
- External device: pty fixture /dev/ttys006 (pid 10172), opened as UART channel **`dut`**. It echoes input, answers each
  line with `ack:<line>` + `fx> ` prompt, and prints `tick N` every 2 s.

Already verified by QA (CLI, no UI input): daemon started from bundle; viewers.visible=1 (WKWebView attached and
visible); CLI→device→daemon round trip `qa-cli-072905`.

## Minimum native steps (ROOT action → QA assertion command)
1. **RX in native terminal**: screenshot window of pid 10201. Open the UART `dut` panel/terminal if not visible.
   Expect real Loom UI (dark shell), no launch error, no URL/token visible; live `tick N` lines increasing and
   `ack:qa-cli-072905`. Take two screenshots ~5 s apart (tick advances = live RX).
2. **Outbound keys + Tab**: click into the `dut` terminal; type `qa-typed-1`, press **Return**; then type `qa-tab`, press
   **Tab**, then **Return**. Screenshot (terminal should show `ack:qa-typed-1`, and the tab echoed).
   QA runs: `desktop/scripts/smoke-native.sh check-typed qa-typed-1` and `smoke-native.sh check-typed qa-tab --tab`
   (fixture rx.log must contain 0x09 after qa-tab, i.e. Tab not swallowed by WebView focus).
3. **CLI→UI shared stream**: write "STEP3 GO" in ROOT-OBSERVATIONS.md or just screenshot after QA runs
   `smoke-native.sh send qa-cli-2` (QA will run it on request); UI must show `ack:qa-cli-2`.
4. **Cmd-Q persistence**: press **Cmd-Q** on pid 10201 (also note whether the red close button alone quits).
   QA runs: `smoke-native.sh after-close` (app pid gone; daemon 10221 + channel dut + device alive; viewers 0).
5. **Relaunch**: QA runs `smoke-native.sh reopen` (same home; asserts daemon 10221 reused). ROOT screenshot new window:
   `dut` still streaming ticks, no error, history visible.
Optional 6. **Error/Retry**: QA `error-start` → ROOT screenshot error view → QA `error-fix` → ROOT clicks Retry → QA `error-verify`.

Please record screenshot paths + observations (pass/fail, anything unexpected) in
`work/reports/tauri-desktop/qa/ROOT-OBSERVATIONS.md` after each step, or tell QA. QA will run assertions on request.
Credential rule: screenshots must not show the boot URL; if a URL with `#boot=` is visible anywhere, that is a FAIL to report.

## Diagnosis of ROOT CUA `cgWindowNotFound (-10005)` (QA, read-only, evidence/native-current/window-diagnosis.txt)
- Only ONE loom-desktop process exists: pid 10201 (QA, bundle /private/tmp/loom-tauri-build/debug/bundle/macos/Loom.app).
  App owner's direct instance is NOT running → no same-bundle-id ambiguity right now.
- CGWindowListCopyWindowInfo: pid 10201 owns **window #1587, on-screen, layer 0, alpha 1, 1440×900 at (240,50)**
  (= configured inner size) + 4 menu-bar-size helper windows (off-screen). So the native window EXISTS on screen.
- NSRunningApplication(10201): bundle local.loom.desktop, activationPolicy regular(0), finishedLaunching, **active=false**,
  hidden=false. lsappinfo: type Foreground, launched via LaunchServices (`open -n`).
- Daemon side: viewers subscribed=1 visible=1 (WKWebView page is loaded and reports document visible).
- Likely CUA causes: (a) path given as `/tmp/...` while LaunchServices registers `/private/tmp/...` (symlink);
  (b) CUA matching on the frontmost/active app — Loom is not active; (c) `open -n` instance not registered under the
  bundle-id lookup CUA uses. Suggest: getApp by pid 10201 or by `/private/tmp/loom-tauri-build/debug/bundle/macos/Loom.app`,
  or activate Loom via CUA first (clicking window #1587 region or Dock icon), then getApp again.
- QA will not relaunch while ROOT retries, to keep pid/window stable. Re-run diagnosis anytime:
  `swift desktop/scripts/lib/winlist.swift 10201`.
