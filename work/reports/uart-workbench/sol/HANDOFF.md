# Sol UART handoff

**Status: READY_FOR_REVIEW — QA layout/persistence fixes re-frozen; dedicated Opus recheck pending.**

- Re-frozen after native QA defect corrections: **2026-11-13 13:10:04 UTC (22:10:04 KST)**. Initial freeze was 13:04:21 UTC.
- Native runtime: **GPT-6.1-Sol, high, Fast**. Session **01a1d4f8-a02e-7300-bf2a-b99d05c6f10b**. Runtime verified by MAIN in the current conversation; environment `CODEX_THREAD_ID` matches. Direct foreground implementation; no subagents, workers or model substitution.
- Scope: one interactive UART mockup. No product, old mockup, plan, STATUS, package, configuration or git mutations. No server/browser started. No commit or push.
- Preview: **http://127.0.0.1:4180/uart-workbench/sol/index.html**. MAIN verified HTTP 200; the optional sandbox curl escalation was declined and was not retried. Availability is resolved.
- Owned artifacts: `sol/index.html`, `sol/styles.css`, `sol/app.js`, `sol/SURVEY.md`, `sol/HANDOFF.md`, `sol/CHECKPOINT.md`, `sol/self-check.cjs`, `sol/SELF-CHECK.txt`; shared parent `index.html` only for the authorized neutral two-link gallery after inspecting the frozen peer structure.

## What is ready

Real recursive x/y split tree; pointer-capture divider drag with subtree minimums; separator keyboard resize; pane focus, maximize/restore, close and recoverable final empty pane. Each pane has independent connection tabs. Native drag/drop moves a tab to another pane; Alt+Shift+M moves the active tab to the next pane as an accessible fallback. No broadcast.

Saved mcu (115200 8N1), aux (9600 8N1), console and debug; mcu + aux initially visible. New saved session + open, per-profile settings and independently editable open-connection settings, local send/echo, disconnect/reconnect, TEXT/HEX, search, display pause/resume, clear-view-only, TX/RX counters, boot/telemetry/warning logs. Per-terminal Dark/Light theme, default Dark, independent and persisted.

Macro Login / Read status defaults; user create/edit/duplicate/delete; ordered send/delay steps, step remove/reorder, {{value}} substitution, optional Alt+1…9 assignment. Shortcut selects the macro and focuses the target confirmation; Run is explicit. The exact alias + T-number + endpoint appears in the target chooser. Run snapshots steps and target, reports step progress, and Stop aborts delay and all future sends. Closing/disconnecting the target also aborts. Reload never resumes a run. User state persists in `loom.uart.sol.v1` only; unrelated localStorage is untouched. JSON export is supported.

## Exact Opus QA path and selectors

Use existing browser/server ownership. Start at the URL above. If a clean seed is needed, remove **only** `localStorage.removeItem('loom.uart.sol.v1')` on this origin, then reload; never clear all localStorage. State self-checks do not write to the browser profile.

1. **Initial / new session:** `.pane` count = 2; `#saved-sessions`, `.session-open[data-open="mcu"]`, `.session-open[data-open="aux"]`. Click `#new-session`, edit `#settings-form [name="name"]` and `[name="port"]`, submit `#settings-form button[type="submit"]`. New profile appears and a tab opens in the focused pane. `[data-profile-settings="mcu"]` edits only the saved profile. `[data-instance-settings]` inside a `.pane` edits only its selected connection. Each opened connection has a unique Txx label.
2. **Nested split:** click a pane's `.log` or `.send-input` to focus it. `#open-profile` chooses the profile used by `#open-tab`, `#split-right` and `#split-down`. Click `#split-down` on the initial mcu pane, then click another pane and `#split-right` for an arbitrary nested layout. Inspect `.split[data-node-id]`, `.split-child`, `.pane[data-pane-id]`. IDs are generated dynamically; do not hardcode them. Active pane has `.focused`.
3. **Actual divider geometry:** each `.splitter[data-split-id]` is **9px** thick and spans its parent split. Its direct parent `.split[data-axis="x"]` is left/right; `data-axis="y"` is top/bottom. Take splitter and adjacent `.split-child` bounding boxes, drag from splitter center at least 70px along its axis, release, and assert an adjacent child's width/height actually changed. There is no preset-grid swap. Desktop leaf minima 280×240; subtree minima sum on the split axis. Every render and pointer resize allocates definite pixel extents recursively, clamped to both child subtree minima. The rendered ratio may differ from a saved preferred ratio when space is tight; the preferred ratio remains available on larger viewports. Root scroll is intentional if the available viewport is too small. `aria-valuenow` reflects ratio percent. Arrow keys while divider focused move by 3%, clamped.
4. **Tab move:** `.terminal-tab[data-tab-id]` is native `draggable=true`; drag onto **a different `.pane`** or its tab strip. Active and inactive tabs both move. Drop preserves the exact `data-tab-id`, theme, settings, output and counter values. Source collapses if its last tab moved. `Alt+Shift+M` performs the same move to the next pane. `[data-zoom]` maximizes/restores; `#restore-all` restores; `[data-close-pane]` closes pane, `[data-close-tab]` closes one connection. Closing the last pane leaves `.empty-pane [data-add-tab]` for recovery.
5. **Input / view:** `.send-input` + `.send-form button[type="submit"]`; enter `status` and send, inspect TX increase + `OK status` output only in that target. `[data-connect]` toggles disconnect/reconnect. `[data-echo]` toggles local echo independently. `[data-mode="hex"]` shows encoded bytes + readable content; `[data-mode="text"]` restores text. `.log-search`, `[data-pause]`, `[data-clear]` affect that connection view only. Clear preserves retained log and counts.
6. **Theme independence:** within one pane set `.terminal-theme` to `light` and dispatch/select normally. Its `.pane[data-theme]` changes; other terminal remains dark. Switch the other terminal, move tabs and reload; theme follows the connection and persists. Only output surface darkens; surrounding chrome stays Bench Light. Verify at 1440, 768, 390.
7. **Macro CRUD:** click `#new-macro`. Fields: `#macro-name`, `#macro-preferred`, `#macro-variable`, `#macro-shortcut`. Steps: `.step-editor[data-step-index="0"] .step-type` / `.step-value`, `#add-step`, `[data-step-up]`, `[data-step-remove]`. Save with `#macro-form button[type="submit"]`. Choose via `[data-macro]`; edit via `[data-edit-macro]` or `#runner-edit`. `#duplicate-macro` opens an unsaved copy; Save persists it. Delete is `#delete-macro` → `#confirm-delete-macro`.
8. **Run / stop proof:** create `send first {{value}}`, `delay 3000`, `send must-not-send`; variable `test`. Save, select exact connection in **`#macro-target`**, click **`#run-macro`**. It locks the target and advances to waiting step. Click **`#stop-macro`** during that delay. Wait >3000ms and verify neither TX count nor target output contains a later `must-not-send`. Run again to completion separately and inspect step/result. Target chooser lists each open connection as `alias · Txx · sim://endpoint` and includes offline status. Disconnect or close the running target as a second cancellation path.
9. **Persistence:** add/edit a user macro, set one terminal Light, change connection baud, alter layout; reload. The user macro, independent terminal theme/settings, saved profiles, tab locations and split ratios remain. No macro auto-runs. `#export-macros` downloads JSON. Separate Sessions/Macros `<details>` collapse; `#toggle-rail` restores the library after `#collapse-rail`.

At 390px the library initially collapses only when there is no explicit user preference; `#toggle-rail` opens it; panes flow vertically. Each modeled split remains nested and draggable along its displayed axis. No other service navigation is present. At 768px subtree minimums may require workspace scrolling; toolbar, runner and library controls remain reachable.

## Verification and limits

Commands (from repository root):

```sh
node --check work/reports/uart-workbench/sol/app.js
node work/reports/uart-workbench/sol/self-check.cjs
```

Both passed. `self-check.cjs` uses the existing `ui/node_modules/jsdom`; no installation/package change. **38 targeted DOM/state assertions** cover nested split, mocked pointer ratio/CSS update, native DnD event handlers, user sessions, input, view controls, settings/theme independence, macro CRUD, target locking, variable substitution, abort-after-delay, completion, disconnect cancellation, persistence, responsive DOM and final-pane recovery. Output: `SELF-CHECK.txt`. Geometry uses an explicit test rectangle, **not a real rendered layout**. No browser screenshot, native pointer drag geometry, responsive visual QA or independent review has been completed by Sol; Opus owns that next stage. Early rendered review is required before final acceptance.

Intentional limits: simulated line-oriented output rather than a full ANSI/VT emulator; TEXT/HEX are output views, direct send is text; no hardware/WebSerial/backend; no wait-for-pattern step (send + delay supported); no JSON import (automatic localStorage persistence + JSON export supported); cap 500 records per connection; no hardware handshake claims. Closing tabs disconnects only those simulated instances. Tiny-screen split orientation renders vertically while preserving desktop axis in state. A reload stops any in-progress macro rather than resuming it. LocalStorage failures show an export fallback label. Continuous simulated telemetry is one page-owned interval; no server/background worker was started.

Shared shell/component boundary: `.app-header` is the F2 Bench Light standalone shell adapter (Loom identity, UART navigation, local connection summary). `.app-body` is the UART region; module owns sessions, pane tree, tabs, input/settings/themes/macros. Production transport integration and future services remain in the unchanged existing plans. No second app brand/header is nested in the module. Read-only F2 reference was used per explicit user authorization; no old file changed.

Frozen SHA-256:

```text
index.html  76095ffc381c6e3ef3be190d29709cda5016e847eb20b712c23eb39498625f31
styles.css  69242e25b53201bcf8f3125721548bf183688489c19d101d0bfa3cbec1636e6c
app.js      33f49dc0c3bec36ec93f9245813a562ba61f57e26d24d3206b04e22033a8b59d
```

No cosmetic edits after freeze. Available for narrowly scoped native QA defects; any source fix must update affected checks/hashes and this handoff. Next owner: MAIN coordination → existing Opus browser owner for both proposals' rendered interaction QA.

## Evidence-only completion after freeze

- Native `view_image` inspected shared official `moba-terminal.png` and `moba-multiexec.png`; SURVEY now separates direct visual observations from documented facts and design inference. No app source changed.
- Authorized parent gallery created at `work/reports/uart-workbench/index.html`, preview `http://127.0.0.1:4180/uart-workbench/`. Two neutral links, shared Bench Light decision, concise internal-layout distinction. Peer HTML structure was read only after own source freeze; no peer creative files were edited. Peer interaction completion is not claimed.
- Dedicated `uart-qa-opus` now owns sole Ego operations and `qa/`; HANDOFF delivered for early rendered review. Sol remains available for actual QA defects only.


## Native QA corrections — 2026-11-13 13:10:04 UTC

Source-stage **CHANGES_REQUESTED → READY_FOR_REVIEW**, not final acceptance. Evidence: `../qa/sol/results.json` and reported screenshots from the dedicated QA owner. The old loaded page remains valid for continued old-hash interaction QA; reload is required for this correction set.

Only `app.js`, `styles.css`, affected `self-check.cjs` and own evidence docs changed. `index.html` and gallery are unchanged. No styling redesign, browser start, network retry, product, package, git or old-plan changes.

1. **390 → 1440 library preference leak:** responsive default is derived at render time; it no longer mutates persisted `railCollapsed`. New `railPreference` is `null` until an explicit user expand/collapse sets it to a boolean. Explicit expand on mobile and collapse on desktop persist across reloads and viewport changes. Old storage cannot distinguish automatic and user-triggered collapse (both used the same bit); migration resets only that ambiguous unmarked rail preference, preserving sessions, macros, layout, logs and terminal settings. New explicit preferences remain authoritative.
2. **768 / nested child minima:** root minimum alone did not protect each child against a saved .61/.39 split. Layout now computes a definite width and height for every subtree, clamps both child allocations to their recursive minima, and sets pixel flex bases at every level. At a 542px-wide 768 workspace the minimum tree is 569px, with 280px / 280px leaves and a 9px divider; overflow belongs to `#tree-scroll`. For the reported nested four-pane arrangement, minimum width is 858px and height is 489px. No leaf is deliberately allocated below 280×240 on desktop.
3. **Split-down / idle workspace height growth:** `.app` has a definite `100dvh` desktop height; flex ancestors and `#tree-scroll` have `min-height:0`; non-log controls keep their row heights. `#tree-scroll` consumes the remaining available main height, with internal overflow. The tree and every split receive definite pixel heights; records append only inside `.log`, which retains its internal scrolling. Main rendering sizes the tree after runner/chrome content is present; a page-owned `ResizeObserver` updates allocation if the available workspace changes. Mobile keeps the intended vertically scrolling page.

Syntax + **38 targeted DOM/state checks passed**, including new 390/1440 defaults and explicit choices, legacy migration, reported 768 allocation, nested four-pane allocation, subtree overflow, and append-without-extent-mutation. These checks validate state/allocation and DOM styles, **not actual browser geometry**. Recheck with native bounding boxes at 768 and nested 1440, then idle >12s with telemetry and verify desktop `#tree-scroll` height is stable while `.log` scrollHeight grows. Verify split-down stays within the available viewport except the explicit workspace scroll needed for subtree minimums. Existing pointer x/y drag passed under the old source; recheck against new allocation to confirm no regression.

**READY source hashes:** `index 76095ffc` / `styles 69242e25` / `app 33f49dc0`. Dedicated QA should record the new hashes before continuing geometry acceptance. All interaction selectors remain unchanged.
