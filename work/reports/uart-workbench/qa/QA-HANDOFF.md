# UART workbench — QA handoff (Sol accepted; Opus final browser QA BLOCKED)

- QA owner: claude-opus-5-5, native Claude Code foreground session. No subagents, no server started, no prototype source edits, no git.
- Browser: Ego Lite (guide v2.2.0, read to the end). One owned TaskSpace **15 `uart-workbench-qa`** (page p1), reused for Sol then Opus. No user spaces touched. **`task.finish` NOT called.**
- Storage: removed only `loom.uart.sol.v1` (Sol) and `loomUartOpus.v1.*` (Opus) on 127.0.0.1:4180. No profile-wide clears.
- Evidence:
  - `qa/sol/results.json` and `qa/opus/results.json`. Every attempt is kept, with `rerunOf`, `rerunReason`, `superseded`, `deferred`, `diagnosis` and `classification` fields.
  - Screenshots under `qa/*/shots/`.
  - Log: `qa/PROGRESS.md`.
- The mockups are local simulations. No native UART backend or hardware was validated.
- QA watchers: none. The single owned watcher was stopped per MAIN.

## Sol — ACCEPTED (browser-verified)
index 76095ffc / styles 69242e25 / app 33f49dc0. See `qa/sol/SUMMARY.md`: all 50 checks PASS on their latest run. The 4 product failures were fixed and re-verified natively.

## Opus — source stage accepted by MAIN; final browser QA BLOCKED
- **Final frozen REV5.2:** index **bfda50f8** / styles **41360ad1** / app **bdaf6bd5**; vendor xterm.js 14903579, xterm.css 854a7c0f, LICENSE b569f629. QA's hash check matches `opus/selfcheck/source-sha256-rev5.2.txt`.
- **Blocker (QA-side, not a product FAIL):**
  - Ego TaskSpace 15 ownership is `agentDelegatedToUser`.
  - `takeOverTaskSpace(15)` was denied by the Claude Code permission classifier.
  - MAIN has asked the user to return control.
  - QA stays idle until an explicit control-return signal.
- **No browser PASS is claimed for REV4–REV5.2.**

### Browser evidence so far (older hashes; see `qa/opus/STATUS.md`)
- **Phase A** (3470995f/2b761965/cff8a670):
  - PASS: layout, divider drag/clamp, tab pointer drag, views, settings, macro CRUD/stop.
  - FAILs returned to the owner: pane-maximize (fix claimed in REV2) and macro disconnect→reconnect resume (fix claimed in REV3).
  - Superseded by spec change: send-row min-size and form input.
- **R3** (7925c44f/819bf45f/6b4ae726; served hash verified in-browser):
  - PASS: xterm focus, typed login, Tab completion (unique and ambiguous), ↑ history/cursor editing, Ctrl+U/C/L, ANSI, per-tab independence.
  - Multiline paste: latest record **FAIL**. Two causes are classified separately: a real `Sim.key` order race (owner fix: ordered queue) and an over-strict QA check.
- **Static read-only audit** (not browser verification): `qa/opus/REV4-recorder-audit.md`.
  - MAIN accepted the static structure.
  - F1 is fixed in REV5.1/5.2 (app.js:203).

### Pending native checks on REV5.2 after control returns (criteria in `qa/opus/STATUS.md`)
1. **Load and renders:** served-hash check, clean load via another same-origin page, renders at 1440, 768 and 390.
2. **xterm:**
   - multiline paste order (core: arrival order, outputs, TX);
   - selection and copy by mouse drag;
   - switching TERM↔HEX/search and back keeps the buffer and focus;
   - pause/resume catches up, with F1's gap notice on overflow;
   - resize after split, divider drag and tab move.
3. **Macros (M1, M5–M7 + REV6 scope S1–S10 in `qa/opus/STATUS.md`; S supersedes M2–M4/M8 on the macro path):**
   - ▶ runs immediately; the ⋯ menu holds edit/duplicate/delete;
   - Current is the default; All connected shows the broadcast indicator and recipients;
   - excluded and locked targets;
   - Stop stops every target, and sidebar 전체 중지 works;
   - a fixed-target macro keeps its target while a current-target macro follows All;
   - reload resets the mode to Current;
   - TX logging is per recipient;
   - a run does not resume after disconnect→reconnect.
4. **Port dropdown:**
   - the list shows registered ports with in-use badges;
   - a free port opens on exactly that target;
   - a busy port focuses its existing tab with no duplicate;
   - rescan and the empty state.
5. **Layout:** maximize/restore rerun, minimum pane under xterm, session collapse and persistence after reload.
6. **Timestamp gutter and recording:** gutter T1–T2; recording R1–R6 + R2b.
   - Real-file proof: MAIN drives the native `showSaveFilePicker` dialog via computer-use.
   - QA reads the file under `qa/recordings/` while recording, with ≥2 growth reads before Stop.
   - Mock-stream results are never counted.
7. **Wrap-up:**
   - remove QA-created macros and sessions through the UI and restore the initial 2-pane layout (own prefix only);
   - check the root comparison page `/uart-workbench/index.html` links at 1440 and 390;
   - final hash check, then `task.finish` keeping one comparison page.

### REV6 static review (app db469ee2 / styles ddf7c8a3 / index 4c077401 = rev6 manifest) — NOT native
`qa/opus/REV6-scope-review.md`: scope/keyboard independence, preview==resolver, entrypoints, fixed precedence, snapshot, Stop, checkbox state/focus statically OK. **F5 (medium)**: JSON import drops a `fixed` target pointing to a session imported in the same file (new id; filter by old id) → macro silently follows panel scope (possibly All); not covered by selfcheck. Native S1–S10 (+S7b import) pending control return.
- **REV6.1 final (app cbc41b96 / styles e3a02266 / index 4c077401; all 8 manifest hashes verified by MAIN, QA re-hashed the 3 app assets): F5 RESOLVED statically, no definite defect.** Low observation retained: the mixed blocked preview still names a recipient after `실행 차단`. Native S1–S10/S7b and all other pending checks are BLOCKED by Ego 15 user control. No browser PASS, no task.finish.
