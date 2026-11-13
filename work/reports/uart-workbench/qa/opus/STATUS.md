# Opus UART: native QA status (interim, browser paused)

QA: claude-opus-5-5, Ego Lite TaskSpace 15 `uart-workbench-qa` (p1). The raw record of every attempt is in `qa/opus/results.json`.
**Browser blocked (QA-side, not a product FAIL):** space ownership is `agentDelegatedToUser`. `takeOverTaskSpace` was denied by the permission classifier. MAIN has asked the user to return control.

## Phases
| Phase | Hash (index/styles/app) | Use |
|---|---|---|
| A | 3470995f / 2b761965 / cff8a670 | Original freeze. Layout, tabs, macros and views. Evidence only for parts changed later. |
| EARLY | 7925c44f / 819bf45f / 57defddb | First xterm render. Not acceptance. |
| R3 | 7925c44f / 819bf45f / 6b4ae726 (+ xterm.js 14903579) | Current freeze. In-browser SHA-256 verified. |

## R3: done
PASS:
- Served hash, initial 1440 render (2 fitted xterm tabs, no send row).
- Click → xterm focus.
- Typed login (password hidden).
- Tab completion, unique and ambiguous (second Tab lists the candidates).
- Ctrl+U, Ctrl+C (no selection), Ctrl+L.
- ↑ history and cursor editing.
- ANSI colour.
- Per-tab input independence.

QA-expectation corrections (originals kept): trailing space after completion; ambiguous completion needs a second Tab.

**FAIL (open, owner fixing):** `xterm-multiline-paste`. Order race in `Sim.key`: each Enter is handled on an independent 40 + rand(60) ms timer, so a later command's response can overtake an earlier one (MAIN source check). The first QA FAIL was over-strict (8-line tail plus an exact echo prefix). That run's outputs were in order, with TX +12.

## Open product items from phase A (rerun on the next freeze)
- `pane-maximize`: the maximised pane kept its 557px share of 1120 (REV2/3 claims a fix).
- `macro-cancel-on-disconnect-reconnect`: run resumed after reconnect (REV3 claims a fix).

## Superseded / deferred by user spec changes
- `min-size-pane-usable`: Send row removed by spec. Re-judge the minimum pane under xterm.
- `direct-input-target-independent`: replaced by the R3 xterm input checks.

## Remaining after control returns (R3 or next freeze)
- Selection/copy via mouse drag.
- TERM↔HEX/search regression: terminal buffer and focus kept. Pause/resume catches up.
- xterm resize after split, divider drag and tab move.
- Paste order and macro shared send path, including stop and disconnect/reconnect (next freeze).
- Session list collapse/expand and persistence.
- Port dropdown: registry and in-use choices, free → open exact port, busy → focus without a duplicate, rescan/empty.
- Maximize/restore; minimum pane size.
- 768 and 390 renders.
- Cleanup of QA-created macros/sessions via the UI, restore the 2-pane layout.
- Root gallery link check at 1440/390.
- `task.finish` keeping one comparison page.

## Added USER feature (owner implementing): per-session live log recording + RX timestamp gutter. Test plan for the next freeze, after control returns
Settings autosave is not part of this requirement.

**Timestamp display**
- T1: Toggle the per-tab timestamp. Check format and alignment against real VT scroll, line wrap at narrow cols, in-line editing (prompt redraw, ←/→, ^U) and resize after a split or divider drag. The gutter must not corrupt the cursor or the prompt.
- T2: The display toggle must not change raw bytes. TX/RX counters and the bytes sent for a typed command are identical with timestamps on and off. A HEX view of the same RX shows no timestamp bytes.

**Recording**
- R1: Start recording through the app's normal UI. Inspect the mechanism first:
  - `<input type=file>` or a download-based stream: Ego `waitForFileChooser` / `waitForEvent("download")` + `saveAs` give real file evidence.
  - `showSaveFilePicker` (File System Access): this is a browser-owned native prompt, which Ego must not drive. QA hands the step to the user via MAIN and does not fake it.
- R2: Write, flush and stop produce an actual file on disk. The file holds RX and TX lines, each with its own timestamp, separate from the display gutter. QA compares the file contents to what the device sent.
- R3: Incoming data is still saved while the view is paused, cleared, search-filtered, or past the 3000-line display cap.
- R4: Per-target isolation. Recording mcu writes no aux data. Two concurrent recordings go to separate files.
- R5: Cancel, permission denied or a write error leaves no false "Recording" indicator and shows an explicit error state.
- R6: Reload never auto-resumes a recording. The indicator is off after reload.

Evidence is the real file written through the normal UI path. Mocked-stream or jsdom results are not claimed as browser verification.
- R1/R2 native Save dialog coordination (MAIN): after Ego 15 control returns and the code is reviewable, MAIN drives the native showSaveFilePicker via computer-use, if supported. QA owns the page automation (start, stop, generate RX/TX, pause, clear, filter) and inspects the actual file under `work/reports/uart-workbench/qa/recordings/`. Real file saving is never marked PASS from a mock stream.

## Added USER acceptance (Opus): immediate macro run + Current / All-connected target mode. Native checks after control return + new freeze
Broadcast to All connected is explicitly authorized by the user, so these checks need no extra approval.
- M1 Arrow vs edit: the green ▶ on a macro runs it at once (no editor, no `#dlgRun` confirm). The separate edit button opens the editor only. Assert no TX or run for the edit button.
- M2 Mode default = Current. Fresh and after reload, the mode is Current. A macro arrow run and terminal typing go only to the active tab. All other tabs' TX is unchanged.
- M3 All connected, ≥2 connected targets:
  - A conspicuous broadcast indicator lists the recipient aliases.
  - Both terminal input and the macro ▶ go to every recipient exactly once. Check each recipient's TX delta and its output/echo once.
  - Per-target progress, wait and result are shown.
- M4 Filtering:
  - Disconnected (overlay) and read-only (debug) tabs are excluded and not listed as recipients.
  - The target set is locked when a run starts. A port opened or reconnected during the run does not join it.
  - A target that disconnects mid-run is aborted for that target only, with nothing sent after reconnect (REV3 rule).
- M5 Stop during a delay stops all remaining steps on every recipient. After waiting longer than the delay, no target gets a follow-up send.
- M6 Reload resets the mode to Current and never auto-runs.
- M7 Recording/logging metadata: the actual routed TX is attributed per recipient. Each recipient's log or record holds its own TX lines only, and no line is attributed to a target that did not receive it.
- R2b Live on-disk growth (from the Chrome File System Access doc: `createWritable()` writes reach disk only on `close()`; the owner was told periodic commits are needed): while a recording is running, QA reads the chosen file under `qa/recordings/` from the shell at least twice before Stop. Its size and content must grow with new RX/TX lines. A queued-bytes counter, app state, or a final file that only appears at Stop does not satisfy R2b. Mock-state results are never used for a browser disk claim.
- M8 (added from MAIN's REV5 UX review, owner fixing before the final freeze):
  - The edit button sits right next to Play (▶), in the same place.
  - No stale labels for the confirm/active pane.
  - Sidebar Stop stops the whole recipient group, not just the first recipient.
  - A fixed-target macro ("항상 <alias>") keeps its single target even in All mode.
  - A current-target macro uses the All scope when All is on, and this is visibly indicated before and during the run.
  - Native checks: run each kind in Current and in All, and assert recipients, TX deltas and the visible scope label.
- F1 re-check (narrow fix): pause long enough to overflow the display buffer, or use a lowered local test cap if the owner exposes one. After resume, assert that the `표시 누락` notice shows and `displayGapBytes` > 0 while the recording is complete.

## REV6 macro execution scope: native QA acceptance (S1–S10)
Source: top section of `opus/REQUEST.md`. Baseline REV5.2. These criteria supersede M2–M4 and M8 for the macro path. Run only after control returns and REV6 is frozen.
- **S1 Scope control placement and accessibility.** Next to the macro ▶ actions there is one control with Active / All connected / Selected sessions. Toggles/checkboxes have accessible names and checked state. ▶ runs at once with no generic confirm; the only prompt allowed is missing-variable entry. Edit/duplicate/delete live only in the ⋯ menu.
- **S2 Visible recipients.** Before a run, the control shows the actual recipient aliases and count for the current scope. During the run, each recipient has a badge with its own progress/wait/result. Assert that the labelled set == the set whose TX changed.
- **S3 Active (default).** Fresh load and every reload start in Active. ▶ goes only to the tab that is active at launch. Switching the active tab during the run does not move the run.
- **S4 All connected.** ▶ reaches every eligible tab exactly once, each with its own EOL; check the TX delta and output per tab. Disconnected and read-only (debug) tabs are excluded and listed as excluded.
- **S5 Selected sessions.**
  - Selecting one or several eligible tabs sends to exactly that subset; non-selected tabs see no TX change.
  - An empty selection disables ▶ or shows a clear message, and sends nothing.
  - A selected tab that later disconnects is clearly marked and excluded.
  - Ineligible tabs cannot be selected, or are shown disabled.
- **S6 Macro scope vs keyboard independence.** With the macro scope on All or Selected, typing in a terminal goes only to the focused tab; check other tabs' TX unchanged. The toolbar keyboard broadcast stays a separate, distinct control, and turning one on does not change the other.
- **S7 Legacy fixed-target precedence, no silent override.**
  - Take a macro saved with a fixed target ("항상 <alias>", from REV5.2 storage) and check the documented migration/precedence.
  - Either it is migrated into a visible per-macro selection, or it is shown as an explicitly labelled override.
  - The recipients shown before ▶ == the recipients actually used. A visible Active/All choice is never ignored without a label.
- **S8 Per-run snapshot.** Start All or Selected with a delay step. Open or connect a new eligible session during the run: it is not added. Changing the scope control mid-run does not alter the running recipient set.
- **S9 Stop.**
  - Group Stop stops all remaining steps on every recipient.
  - Per-pane Stop stops only that recipient; the others continue.
  - After waiting longer than the delay, no follow-up TX reaches any stopped recipient.
- **S10 Disconnect / reconnect / reload / preserved behaviour.**
  - A recipient that disconnects mid-run is aborted for that tab only, and reconnecting it does not resume the run.
  - Reload resets the scope to Active and never auto-runs.
  - Recorder TX is attributed per recipient (M7).
  - The timestamp gutter and terminal input are unchanged.
Evidence per check: per-tab `tx`/`macroSends`/`run` snapshots plus a screenshot of the scope control and badges. Each check asserts an explicit boolean.
