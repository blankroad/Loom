# ROOT integration review and native observations

State: WORKING — new Bench Light native acceptance pending. Historical checkpoints below are retained, not current visual acceptance.

## Bench Light correction / current run

- ROOT rejected the old dark product as the wrong visual baseline. Agreed UART REV6.1, Ethernet and UDS workbenches were ported into the actual shared UI by native foreground Opus 5.5 owners. ROOT reviewed source, boundary fixes and actual Ego screenshots.
- Old run4 after the screen was unlocked: ROOT CUA actual OS Enter (`root-native-1`) and Tab (`root-tab` + 0x09) reached the external PTY and the same CLI receive journal. This is valid functional evidence for that earlier UI, not acceptance of the new visual build.
- New UI checks: 666 tests passed, schema/lint/typecheck passed (type-only ETH array fix), app bundles and signatures verified. Real-daemon UI identity defect (canonical localhost Host header) was fixed and tested against an actual isolated daemon. Other details in ROOT-REVIEW.md and finalQA evidence.
- Waiting for stable private app NATIVE-REQUEST before new CUA checks. Never bind a nonrunning app path (CUA auto-launches and could use the default home). ROOT owns the native input lease while acting; QA holds its app automation until return.

### New ordinary release native checks (2026-11-15 ~11:49 KST)

- Verified live process 45431 executable is `desktop/dist/Loom.app/Contents/MacOS/loom-desktop`, private daemon 45073, port54389 and external PTY /dev/ttys000 before CUA binding. Actual ordinary signed release, no WebDriver.
- CUA initial AX: app reopened at `#/l/m1-default` with live dut. ROOT clicked UART service to obtain the agreed workbench. The initial/fresh automatic route still needs actual QA measurement; clicking to UART is not proof of automatic startup routing.
- CUA clicked actual Terminal input, typed `root-native-bl1`, physical Return. `smoke-native.sh check-typed root-native-bl1`: **PASS** external device bytes and CLI `ack:root-native-bl1` on same stream.
- CUA typed `root-tab-bl1`, physical Tab, Return. `check-typed root-tab-bl1 --tab`: **PASS**, exact `0x09` at device plus CLI ack.
- ROOT visually inspected CUA's actual app screenshot: Bench Light outer shell, sessions rail/sidebar and toolbar, dark terminal with host timestamp gutter, live ticks and both acks. No blank panel. This capture includes native title bar; browser/native CSS geometry comparisons remain QA's separate evidence.
- QA stopped only private daemon45073 via ordinary `loom daemon stop`, preserving ordinary app45431. ROOT CUA fetched fresh native AX, clicked Connection menu then Reconnect. Same app navigated to `localhost:54546/ui/#/l/m1-default`, AX reports connected, interfaces0, RECok, visible1/1, approve, light Welcome. The missing dut is expected: the old daemon owned its connection; QA reopens it. This is actual native menu input, not DOM synthesis or app relaunch. QA verifies new PID, binary and identity Match.
- **NATIVE_CHECKS_DONE** — ROOT returns the app input lease to finalQA. New ordinary release physical Enter, physical Tab0x09 and native menu Reconnect UI recovery passed. No production/default user daemon was touched. Final module flows, startup route measurement and final comparative screenshots/report still belong to QA.

## Historical native checkpoint (superseded by later runs)

- Reviewed host lifecycle source: same-home lock coordination, explicit binary, detached daemon, bounded probes, credential redaction, no Drop termination.
- Reviewed app source and independent host reviews. Required native Reconnect after daemon restart and top-level external-navigation restriction; app owner implemented both. Packaging review findings sent to QA (PID identity, lockfile enforcement, separated targets, deadlines, token hygiene).
- CUA initialized on user's macOS. `getApp` for QA debug bundle absolute path and bundle id returned `cgWindowNotFound (-10005)`; signed release bundle also failed. Display-name lookup returned Invalid app; Dock activation path timed out. No native OS keystrokes or clicks were performed by ROOT. These are tool binding failures, not proof the app lacks a window.
- QA's diagnostic evidence identifies a valid on-screen 1440x900 window and visible viewer; native WK WebDriver was added as a debug-only, explicitly enabled test feature. Normal release rejects that feature and excludes the server.
- ROOT visually inspected actual WK snapshots `native-current/wk/01-ready.png` and `fail-rx.png`: Loom shell/auth/channel metadata render, but terminal is blank and trace says 수신 없음. This is a real failed UI observation, not accepted as PASS.
- QA separately reproduced working Chromium RX on the same daemon. App owner is investigating WK occlusion/rAF throttling. Source changes and native rerun remain required.
- The test runner explicitly labels its input as DOM events in real WKWebView, not physical OS keystrokes. Keep this distinction in the final evidence.

No device access outside Loom by ROOT; all integration device work belongs to QA's isolated external PTY fixture.

## Checkpoint after implementation review

- Root independently ran the combined desktop Cargo workspace against the actual current loomd: **34 tests passed**, log `../ROOT-TESTS.log`. Opus app owner ran UI suite: **624 tests passed**, typecheck/lint clean.
- Root independently ran `desktop/scripts/verify-dist.sh desktop/dist/Loom.app`: strict ad-hoc codesign, no WebDriver plugin, post-sign daemon code-section identity and all 6 embedded UI files passed.
- Final diagnosis supersedes the provisional empty-RX interpretation above: the macOS console is **locked**, main display **asleep**. Actual WebKit visibility is hidden, rAF is stopped; renderer acceptance cannot be performed in this state. No fake visibility or OS unlock workaround was used.
- A real defect was fixed: a page initially loaded hidden falsely reported visible until a later visibilitychange. `services.ts` now syncs initial visibility at bind, with regression tests. Native daemon now correctly reports `visible:0` and agent `visibility:not_visible` in the locked session.
- **Native visible RX, renderer Enter/Tab and final screenshot acceptance remain pending.** Test scaffold and a standalone debug-only QA app are preserved. See `RESUME.md` for a fresh run after the user unlocks/wakes the display.
- All QA fixture processes/waiters and QA Ego space were cleaned. ROOT's CUA lookup had auto-launched the normal app (pid19191) and its new default-home daemon (pid19201); ROOT also stopped those after checking executable path, start provenance and discovery PID. Home contents and other user daemons were preserved.
- This checkpoint is not a DONE claim. Physical OS keystrokes/native menu acceptance is distinct from DOM events in real WKWebView.
