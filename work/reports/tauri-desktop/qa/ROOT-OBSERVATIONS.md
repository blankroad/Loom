# ROOT integration review and native observations

State: WORKING. Native RX/input is not accepted yet.

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
