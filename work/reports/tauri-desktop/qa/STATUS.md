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
