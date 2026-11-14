# FINDING (real product defect, WK-specific): UART RX rows never render in the native WKWebView

Run: QA test build /tmp/loom-tauri-build/qa/Loom-QA.app (feature e2e), isolated home lqa.fD6PeU, pty fixture
channel `dut` emitting `tick N` every 2 s. 2026-11-15 07:40–07:44 KST.

| Surface (same daemon, same minute) | UART trace `dut` |
|---|---|
| Daemon (`loom uart read dut --tail`) | ticks 45…48 present (live) |
| Chromium (Ego), fresh viewer, `#/p/uart/trace?channel=dut` | 33 rows, `DEV tick 80…85` live — evidence/native-current/ego/browser-trace.png |
| Native WKWebView (Tauri app), deep link `#/p/uart/trace?channel=dut` | "수신 없음", 0 `[data-dir]` rows after 20 s — wk/fail-rx.png |
| Native WKWebView, opened from sidebar 트레이스 button (no deep link) | "수신 없음", 0 rows — wk/06-trace-from-sidebar.png |
| Native WKWebView terminal panel | 2 WebGL canvases, no DOM rows (expected for WebGL renderer); snapshot black — inconclusive by itself |

Same WK window DOES show: 연결됨, LIVE lag 0ms, REC ok, 보는 중 1/1, Activity AGT rows (core events), channel list
`dut 연결됨 gen1`. Daemon reports viewers subscribed=1 visible=1, subscriptions=3.
⇒ Not deep-link params, not daemon/fixture, not WebGL snapshot artifact. Core events reach the WK UI, UART record
rows do not. Suspect area for A: record subscription/decoding path in the module Web Worker
(ui/src/net/index.ts `new Worker(... type:'module', name:'loom-net')`) under WKWebView — e.g. a JS error in the
worker/codec only on WebKit. Rights differ but should not matter: WK viewer shows `자격: approve`, Ego viewer minted
with rights=operate.
Harness bugs separated (not product): zsh no-word-split invocation (void FAILs annotated), plugin not resolving element
refs in execute args (fixed: in-page querySelector focus).

## Update: keyboard → device also FAILs in the same occluded state (07:46)
WebDriver keys `qa-typed-1⏎` and `qa-tab⇥⏎` to the focused `.xterm-helper-textarea` (focus=true before and after keys)
reached NO bytes at the pty device (rx.log holds only the CLI marker). Consistent with A's diagnosis (occluded WK
window → rAF/scheduler never runs). Kept as genuine FAILs (results.txt); rerun after A's backgroundThrottling/policy fix.

## Update 07:47 — rAF/visibility probe (supports A's occlusion diagnosis)
Relaunched QA window pid 33265 (CG window #1640 reported on-screen, app not active): in-page probe via WebDriver
execute/async → `document.visibilityState="hidden"`, `document.hidden=true`, `hasFocus()=false`,
`requestAnimationFrame` did NOT fire within 3 s. After relaunch the sidebar also shows "인터페이스 0 / 연결된 채널 없음"
although the daemon still has `dut` open and streaming (after-close CLI round trip PASS) — presumed same rAF stall;
must be rechecked after the policy fix (else it is a separate late-join defect).
