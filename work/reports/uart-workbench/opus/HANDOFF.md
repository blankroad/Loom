# HANDOFF — Opus UART workbench (one proposal) · REV 6.1 (F5 import/fixed-target fix)

- **REV 6.1 source (sha256 in `selfcheck/source-sha256-rev6.1.txt`):** `app.js cbc41b96…`, `styles.css e3a02266…`; `index.html 4c077401…` and vendor unchanged.
- **F5 fix (QA `qa/opus/REV6-scope-review.md`):**
  - **Remapping.** `importData()` builds `idMap` (payload session id → local id). A newly imported session maps to its new id; an existing session with the same alias maps to that session. Each macro's `fixed[]` / legacy `target` is remapped through it.
  - **Unresolved ids are kept, never dropped.** They show as the row chip `.chip.fixed.unresolved` "고정 · 미해결" and the label `고정 · 실행 차단(미해결) → … · 제외 미해결 <id>(저장된 세션 없음)`. The editor lists them as `[data-role=fixed-unresolved]` and says they are removed on save.
  - **Import toast** reports `고정 대상 N개 새 세션에 연결` and `고정 대상 N개 미해결 — 해당 매크로는 실행되지 않음`.
  - **A fixed macro with any unresolved id is blocked as a whole** in `requestRun()` before launch. Nothing is sent, not even to known targets, and the toast reads `실행 차단`. It **never widens to the panel scope**.
  - Known fixed targets that are only closed or disconnected keep the documented exclusions; the other fixed targets still run.
  - Deleting a session keeps it in fixed macros as unresolved (blocked) with a toast, instead of emptying `fixed[]`.
  - The snapshot gains `macros[].unresolvedFixed`.
- **Checks:**
  - The full suite `check.cjs` passed **122/122** (117 + 5 new F5 cases: custom-session remap, same-alias remap, unopened imported target blocked, unresolved kept and reported, unresolved sends nothing). That run was **before** the follow-up whole-macro block correction.
  - After the correction only the affected focused regression `selfcheck/rev61-mixed.cjs` was run: **3/3 PASS** (`rev61-mixed-result.json`). It covers mixed fixed `[aux, unresolved]` → label shows 실행 차단 and **zero TX on every session, aux included**, in both 활성 and 연결된 모두.
  - jsdom only; **no browser**.

# (previous) REV 6 (macro execution scope)

- **REV 6 source (sha256 in `selfcheck/source-sha256-rev6.txt`):**
  - `index.html 4c077401…`, `styles.css ddf7c8a3…`, `app.js db469ee2…`; vendor unchanged.
  - Local jsdom **117/117 PASS** (`selfcheck/selfcheck-result.json`, script `check.cjs e823c89a…`). jsdom only; **no browser verification claimed**. **QA: hard-reload.**
- **Scope control (macro panel, directly above the ▶ list):** `.mscope`.
  - Mode buttons: `[data-act=ms-mode][data-mode=active|all|selected]` (`role=radio`, `aria-checked`), labelled 활성 / 연결된 모두 / 선택.
  - **Live recipients:** `[data-role=ms-recipients]`. Examples: `활성 → mcu`, `연결된 모두 → mcu, aux, console (3) · 제외 debug(읽기 전용)`, `선택 → mcu · 제외 console(닫힘)`.
  - **선택 picker:** `.mscope-pick input[type=checkbox][data-act=ms-sel][value=<sessionId>]` lists the open sessions. Ineligible ones (read-only, closed, lost) are disabled with the reason; selected-but-not-open sessions are listed as excluded.
  - Empty selection: ▶ sends nothing, a toast explains, and focus moves to the first checkbox.
  - Not persisted: reload always starts in **활성**.
- **One resolution rule for every entry point** (sidebar ▶ `.mac [data-act=run-macro]`, pane header macro menu `[data-act=macros-menu]`, hotkey):
  1. **Fixed override:** `m.fixed` (session ids), shown as the row chip `.chip.fixed` "고정" and the label `고정 → aux`, wins.
  2. **Otherwise the panel scope** `MS`:
     - 활성 = the active tab at launch. A pane-menu launch first makes that pane active, which only matters in 활성.
     - 연결된 모두 = every connected writable tab.
     - 선택 = the chosen sessions.
  - The pane menu header `.menu .mh` shows `실행 대상: <scope → recipients>`. Fixed items show `(고정 → alias)`.
  - The pane menu **never narrows 연결된 모두 / 선택** to its own pane.
- **Independence:**
  - The toolbar control is now labelled **키 입력 대상** (typing only; BC). Macros ignore it.
  - The macro scope never makes typing broadcast.
- **Snapshot per execution:**
  - Recipients are resolved once at launch. Sessions opened mid-run don't join. Ineligible or busy targets are skipped with reasons in the toast.
  - Runs are per-target independent, with `run.gid` for groups. The pane run-strip stop is per target; the row `■ 전체 중지` (`[data-act=stop-macro-all]`) stops this macro on all targets; the toolbar `[data-cmd=stop-all]` stops everything. Disconnect cancels only that target, and reconnect never resumes.
- **Legacy migration:**
  - A stored or imported `target: <sessionId>` becomes `fixed: [sessionId]` with a visible "고정" chip. The retired `confirm` field is dropped.
  - Editor: `input[name=tmode][value=panel|fixed]` plus `input[name=fixed][value=<sessionId>]` (multi). Choosing fixed with none checked is an error.
  - The pinned option is renamed "pane 매크로 메뉴 맨 위에 표시 (★)", so it no longer collides with 고정.
- **Snapshot** adds `macroScope{mode, selected[], recipients[], excluded[]}` and `macros[].fixed[]`/`targetLabel`.
- **Not touched:** recorder taps, timestamp gutter, xterm and Sim behaviour, immediate ▶ (no confirm, missing variables only), ⋯ edit/dup/delete.

# (previous) REV 5.2 (final freeze)

- **REV 5.2 = wording-only change on REV 5.1.**
  - The run-strip stop tooltip and the keys-dialog Esc row now read **터미널 밖에서 Esc**. They say Esc inside a focused terminal goes to the device (xterm consumes it), so the pane 중지 button is the explicit stop.
  - Only `app.js` changed → `bdaf6bd5…`; full manifest in `selfcheck/source-sha256-rev5.2.txt`.
  - `index.html bfda50f8…`, `styles.css 41360ad1…` and vendor are unchanged.
  - **Tests:** the existing **103/103 PASS** ran on REV 5.1 (`app.js eb595125…`), immediately before this label-only edit. After the edit only `node --check app.js` was run; no runtime behaviour changed. **QA: hard-reload.**

- **Model / session:** `claude-opus-5-5`, native Claude Code foreground, no subagents, no fallback. Date 2026-11-13, base `main a7d1be6` (untouched).
- **Owned files (only):** `work/reports/uart-workbench/opus/` → `index.html`, `styles.css`, `app.js`, `vendor/xterm/*`, `SURVEY.md`, `HANDOFF.md`, `CHECKPOINT.md`, `selfcheck/`. No product/plan/STATUS/git/package/settings changes and no installs. Sol creative files were not read.
- **Run:** existing main server → `http://127.0.0.1:4180/uart-workbench/opus/index.html` (no build, no CDN, no network).
- **Previous tested runtime source = REV 5.1 (sha256, full list in `selfcheck/source-sha256-rev5.1.txt`):**
  - `index.html bfda50f8…` (unchanged), `styles.css 41360ad1…`, `app.js eb595125…`
  - Vendor unchanged (`xterm.js 14903579…`, `xterm.css 854a7c0f…`, `LICENSE b569f629…`).
  - Local jsdom **103/103 PASS** (`selfcheck/selfcheck-result.json`). **QA: hard-reload.**
  - Earlier hashes: `selfcheck/source-sha256-rev5.txt`, `selfcheck/source-sha256-rev4.txt`.
- **REV 5.1 (review CHANGES_REQUESTED, only these):**
  1. **Sidebar macro row.** Green ▶ `.mac [data-act=run-macro]` sits alone at the row start and runs immediately; clicking the row body does nothing. Edit, duplicate and delete moved to a separate overflow `.mac [data-act=mac-more]` (⋯, at the row end) → `.menu [data-mi]` items 편집… / 복제 / 삭제. The old row buttons `[data-act=edit-macro|dup-macro|del-macro]` no longer exist.
  2. **Wording.** Retired confirm-era text is gone (실행 (대상 확인), 실행 전 대상 pane 확인, 확인 생략, 활성 pane).
     - ▶ title/aria and the row `[data-role=mac-target]` show the real target: `현재 세션: <alias>`, `ALL → <recipients>`, or `항상 <alias>` for fixed-target macros, which are respected even in All mode.
     - The label follows active-pane changes.
  3. **Sidebar stop.** While a macro runs, its row shows `.mac-stop[data-act=stop-macro-all]` labelled **전체 중지**. It stops every running instance of that macro (all group targets), not just the first.
     - The pane run-strip stop is per target. The toolbar badge `[data-cmd=stop-all]` is global.
  - **F1:** during a long pause, display chunks evicted past the 300 KB pause queue are counted into `displayGapBytes` and the `· 표시 누락 N B` marker. The raw recording is unaffected.
  - **F4:** the stale ~5 s comment now describes the adaptive 1–10 s interval.
  - **F2/F3 (documented limits, no backend work):**
    - the prototype handles decoded JS strings, not raw wire bytes, so RX/TX byte counts are UTF-8 of the decoded text;
    - `.log` TX lines are reconstructed from keystroke bytes (BS/^U applied, CSI dropped), not a wire capture — use `.jsonl` for exact chunks.
- **REV 5 (user: immediate ▶, separate ✎, Current/All-connected send target):**
  - **▶ runs immediately.** The sidebar `.mac [data-act=run-macro]`, pane macro menu items (`[data-act=macros-menu]` → `.menu [data-mi]`) and hotkeys run with no confirm or target popup.
    - Target: the macro's fixed session, otherwise the current send target. Variables: the last saved value, else the default.
    - If the target is unusable (closed, read-only, already running), it does not run and a toast says why.
    - A missing variable value opens a **variables-only** dialog `#dlgRun` (`input[name="var:<n>"][data-missing=true]` focused, `[data-role=go]` 저장 후 실행). It never opens the editor and never guesses.
    - **Edit** (REV 5.1: ⋯ `[data-act=mac-more]` → 편집…) is the only way to open the editor and never runs anything.
    - The old confirm dialog and the editor's "실행 전 대상 확인" option are removed.
  - **Send target (toolbar `.bc`).**
    - `#bcCur[data-cmd=bc-current]` = **현재 세션**, the default. It is not persisted, so every reload starts in Current.
    - `#bcAll[data-cmd=bc-all]` = **연결된 모두**: red when on. The badge `#bcBadge.on` lists `ALL → <recipients>` (`[data-role=bc-recipients]`) and `제외: alias(reason)` (`[data-role=bc-excluded]`: closed / 읽기 전용 debug / 장치 분리).
    - Pane headers show `.chip.bc-in` "ALL" or `.chip.bc-out` "ALL 제외".
    - **The recipient set is frozen when All is switched on.** Ports opened later never join. `↻ 다시 고르기` (`[data-cmd=bc-all]` in the badge) re-snapshots on demand.
    - Disconnect, unplug or close removes only that target. When none are left, the mode returns to Current with a toast.
  - **Typing in All mode.** Bytes from any focused xterm go to every frozen recipient through the normal `transmit()` path, with each target's own EOL. So per-target recording captures the routed TX, and each device echoes into its own terminal. Typing in a non-recipient terminal shows a one-time toast and does not send to that port.
  - **Macros in All mode.**
    - One independent run per eligible recipient, sharing `run.gid`, with the target set locked for that execution.
    - Each target keeps its own steps, waits and result in its pane run strip.
    - Disconnecting one target cancels only that run.
    - **Group/global Stop** `[data-cmd=stop-all]` (badge, shown while any run is active) cancels the remaining unsent steps of every run.
  - **macroStop semantics (accurate):** stop cancels only unsent steps, delays and waits. Bytes already transmitted stay queued at the device in order and still get responses. ^C and disconnect keep their interrupt/flush behaviour.
  - Snapshot adds `broadcast{on, recipients[], excluded[]}` and `tabs[].run.gid`.
- **REV 4 (user: receive-time gutter + real-time UART recording):**
  - **Gutter.** `.term.xslot[data-ts=on|off] > .tsgutter[data-tab] > .tsr` shows one host receive time `HH:mm:ss.SSS` per xterm row, left of the VT rows.
    - Each row's time is the arrival of its first byte. It is recorded by an xterm `registerMarker`, so it survives scrollback trimming.
    - Wrapped continuation rows show `↳`. Bytes that arrive during pause keep their arrival time.
    - Nothing is injected into VT bytes or input, so cursor, Tab and selection are unaffected.
    - Toggle: header `[data-act=ts-toggle][aria-pressed]` (label 시각, default on via migration `mig3`). The gutter changes the host width, and the ResizeObserver refits cols.
  - **Recording.**
    - **Controls:** header pill `[data-act=rec][data-rec-state=idle|picking|recording|stopping|closed|error]` opens `#dlgRec`, which offers:
      - format `input[name=format][value=log|jsonl]`;
      - `input[name=rx]`, `input[name=tx]`;
      - `input[name=fileTs]` (.log only; disabled for JSONL, which always has `t`/`ms`);
      - start `[data-act=rec-file]` (user-chosen file via `showSaveFilePicker`, suggested name `alias-YYYYMMDD-HHmmss.log|.jsonl`);
      - start `[data-act=rec-buffer]` (explicitly *not a file*, 5 MiB memory, export via `[data-act=rec-export]`);
      - `[data-act=rec-stop]`.
    - **Status:** `[data-role=recstat]` and live `[data-role=recbytes]` show RX/TX seen, **committed (durable)**, writing, pending and last-saved time. `[data-role=rectail]` shows the last 20 formatted lines. `[data-role=err]` shows errors.
    - **Source:** raw RX/TX are tapped in `feed()`/`transmit()` before display, pause, filter, clear and the 3000-line cap. Each target is isolated.
    - **Formats:**
      - `.log` (UI label: 읽기 쉬운 텍스트 · 줄 단위): header, `YYYY-MM-DD HH:mm:ss.SSS RX|TX  line` (cursor-emulated lines, ANSI stripped, TX keystrokes folded), `--` notes, footer. **Unterminated partial lines (e.g. a prompt without a newline) are held until the next newline or Stop; Stop writes them with `⟨줄바꿈 없음⟩`.** Not byte-identical to the wire;
      - `.jsonl` (UI label: 원문 · 조각 그대로 즉시): `start` / `data {t, ms, dir, len, data}` / `note` / `stop` records. Each raw RX/TX chunk is recorded immediately and exactly (ANSI and control bytes kept). Choose this for prompts and binary.
    - **Durability (Chrome FSA commits on `close()`):**
      - writes are serialized on one queue;
      - a checkpoint (close → `createWritable({keepExistingData:true})` → seek) runs every **1 s**, plus 1 s per 4 MiB, max 10 s;
      - Stop flushes, writes the footer and closes;
      - the pill shows `● 저장 <committed>` for files and `● 버퍼 <bytes>` for the buffer.
    - **Bounded:** accepted-but-unwritten file backlog is capped at 8 MiB. Past that, or on any write error, the state becomes `error` with the message. Nothing is ever dropped silently.
    - **Start and stop rules:**
      - Picker cancel gives `idle` plus a message, and nothing is recorded.
      - Start always needs a click; recording never auto-resumes after reload. Only preferences are stored, in `loomUartOpus.v1.recPrefs`.
      - Disconnect or unplug stops and closes with the reason (포트 닫힘/장치 분리로 기록 종료); reconnect does not resume.
      - Close tab, pane or reset stops and closes asynchronously, with a toast.
      - Reload during recording triggers the browser's leave prompt.
    - **Overwrite:** never silent. The file is the user's pick, and the browser's own save dialog confirms replacing an existing file.
  - **Display-gap guard:** if `xterm.write` throws (overflow), the bytes are already recorded and logged. The screen shows `· 표시 누락 N B` and the snapshot gains `displayGapBytes`.
  - **macroStop correction:** stop cancels only unsent steps, delays and waits. Bytes already sent, including later manual input queued at the device, still run in order. ^C and disconnect keep their interrupt/flush semantics.
  - **Snapshot** adds `tsGutter`, `gutter[]` (≤20), `recState`, `rec{state, sink, format, name, dirs, fileTs, rxBytes, txBytes, queued, written, committed, pending, lastSavedAt, errors, reason, tail[]}` and `displayGapBytes`.
  - **Browser limits (honest):**
    - File recording needs Chromium FSA (`showSaveFilePicker`) on a secure context; `127.0.0.1` qualifies.
    - Between checkpoints, up to about 1 s (or the larger interval for big files) is not yet in the file. A crash or forced close can lose that window.
    - Each checkpoint makes Chrome copy the existing file into a swap file, which is why the interval scales with size.
    - The file picker cannot be driven headlessly, so local tests use a fake handle that models commit-on-close.
    - A realistic just-over-3000-line feed test was not added. Independence from the cap follows from the tap point, and jsdom shows it with pause plus clear.
- **Dependency (vendored, unmodified):** `@xterm/xterm` **6.0.0**, MIT. Copied from the repo's existing `ui/node_modules/@xterm/xterm/{lib/xterm.js, css/xterm.css, LICENSE}` into `opus/vendor/xterm/` (the copy's hash equals the source's). Provenance is in `vendor/xterm/SOURCE.txt`. No fit addon exists locally, so cols/rows are computed in `fitXterm()` from xterm's render cell size, with a DOM-measurement fallback.
- **Browser QA:** NOT done by Opus. The dedicated QA worker owns `qa/` and Ego Lite. I make no browser-verification claim. The user notes that headless checks are insufficient for the real terminal: **native Tab, ANSI, arrows, control keys and focus need QA on this hash.**
- **Local check:** jsdom (not a browser; xterm actually opens in it) **61/61 PASS** (REV 3.1) — `selfcheck/check.cjs`, `selfcheck/selfcheck-result.json`. Real keys can't be pressed there, so terminal-input checks drive the **same transmit path** with macro sends containing `\t`, `ESC[A`, `ESC[D`, `^U` and `^C`. Rerun: `node selfcheck/check.cjs <opus dir> <out dir>`. Two test bugs (wrong sequence, `\b` regex) were found and fixed; they were not app bugs.

## REV 3 changes (only these)
1. **User spec: real terminal (xterm) replaces the styled input.**
   - **One terminal per tab.** Each tab owns one persistent `Terminal` and its `.xhost` element. The pane renders an empty slot `.term.xslot[data-view=xterm][data-tab]`, and the same host is re-attached into it after every re-render, split, tab move, maximize or reload. Instances are never recreated per render.
   - **Resize:** a ResizeObserver plus `fitXterm()` follow divider drag, maximize and window resize.
   - **Input path:** `Terminal.onData` → `keyInput()` (maps Enter `\r` to the tab's EOL) → `transmit()`. Macro sends use the same `transmit()`.
   - **Device side (sim peer):** the simulator acts as the device's own line discipline. It handles echo, ←/→, Home/End (also ^A/^E), Delete, Backspace, ↑/↓ history, **Tab completion** (mcu after login, Linux console after login, aux AT commands; a unique match completes, several matches list the candidates) and ^C/^U/^L. Passwords are not echoed. Async log lines print over an active prompt, then the prompt and typed buffer are redrawn (Zephyr/NuttX-style). Output is raw VT with CRLF and ANSI colour (E red, W yellow, OK green, telemetry dim, `ls` dirs blue).
   - **Keys the workbench keeps while a terminal is focused:** returned `false` from `attachCustomKeyEventHandler`, so xterm ignores them and the workbench handles them. These are the macro hotkeys and the reserved layout keys (Alt+Shift+=/-/Enter/W, Alt+Arrow, Alt+Shift+Arrow, Alt+1/2/0, Alt+F). Ctrl+C **with a selection** copies; without one it sends `^C`. Ctrl+Shift+C copies. Ctrl+V uses the browser paste, which xterm turns into `onData`. Esc inside the terminal goes to the device.
   - **Views:** HEX (`[data-mode=hex]`) and search use the existing log view `.term[data-view=log] .tlines`, built from the device stream by `logAssemble()`, a small single-line cursor emulation. Closing search or choosing TERM re-attaches the same xterm.
   - **Echo default:** local echo is now off by default because the sim devices echo. A one-time migration key `loomUartOpus.v1.mig2` turns it off in stored sessions.
2. **User spec: detected-port dropdown.**
   - The session dialog's free-text port becomes `select[data-role=portsel]`, filled from `uartPortsSim()`, a **simulated** registry with exactly the `uart.ports` result shape `{ports:[{path,vid,pid,serial,manufacturer,product,interface,location}], truncated}` from `crates/loom-uart-worker/src/worker.rs`. The future adapter is to replace this one function with `net.call('uart.ports')`.
   - Each option shows: path — manufacturer, product (VID:PID), plus one badge: `사용 가능` / `저장됨: alias` / `사용 중: alias (pane N)` / `이 세션이 열고 있음`.
   - Ports whose tab is in `lost`/`waiting` drop out of the list (unplugged).
   - `[data-act=rescan]` refreshes the list (toast with the count). The empty state is a disabled option.
   - `수동 입력…` (`__manual`) reveals a separate `input[data-role=portman]`.
   - A new session defaults to the first free detected port.
   - **In-use port:** `[data-role=portinfo]` offers `[data-act=port-focus]` (go to that tab) and `[data-act=port-move]` (move it to the active pane). For a port that is saved but not open, it offers `[data-act=port-open-saved]`.
   - Save + open on a port another tab uses is **refused** (`[data-role=err]`). `placeSession()` also refuses a second profile on the same physical port; it focuses the existing tab and shows a toast. The snapshot gains `detectedPorts[]`.
3. **QA bug: macro resumed after disconnect/reconnect.** `disconnect()` (user close or simulated unplug) now aborts the running macro at once: it sets `run.stopped` plus a reason and calls `run.abort()`, cancelling the pending delay or wait. Status becomes `stopped` (`포트 닫힘으로 중단` / `장치 분리로 중단`). Reconnecting never resumes it. QA's repro (aux: send `version` → delay 3000 → send `must-not-send`, disconnect at about 1.1 s, reconnect at about 2 s) gives `macroSends = ["version"]` locally.
4. **QA seam (read-only):** `snapshot().tabs[].xterm` adds `cursorY`, `selection` (≤500 chars), `viewportY`, `baseY`, `visible[]` (≤20 viewport lines) and `tail[]` (≤20 lines up to the cursor), all via the public `buffer.active` API. `deviceLine {prompt, buffer (*** while password), cursor}` is also added.

## Structure (component boundary)
- `.shell` = common **Bench Light outer shell**. `header.shell-hdr[data-shell=header]` holds the brand, the UART-only `.svcnav`, the `SIM` chip and `#shellConn`. `footer.shell-status[data-shell=status]` holds `#stTarget`, `#stExtra` and one quiet prototype note.
- `main#modUart.mod-uart[data-module=uart]` = **UART module**: rail, `.mtbar`, `#side`, `#work > #treeRoot`. Future modules replace this element, not the shell. Port discovery is isolated in `uartPortsSim()` (adapter seam).

## Selectors
| Thing | Selector |
|---|---|
| Pane / split / divider | `.pane[data-leaf][data-pane-no="N"]` (`.active`, `.maxed`, `#treeRoot.has-max`); `.split.row\|.col[data-split]`; `.divider[role=separator][data-split]` (`aria-valuenow`, `data-mina/minb`) |
| Pane actions | `[data-act=split-right\|split-down\|max\|close-pane][data-leaf]`; toolbar `[data-cmd=split-right\|split-down\|maximize\|close-pane\|new-session\|new-macro\|export\|import\|reset-layout\|keys\|toggle-side]` |
| Tabs | `.tab[data-tab]` (`.on`), `.t-label`, `.t-port`, close `.tab .x[data-act=close-tab]`; add `.tab-add[data-act=add-tab]` → `.menu [data-mi]`; empty pane `.picker .pitem[data-act=pick][data-sid]` |
| Pane header | `.phead[data-tab]`: `.ph-alias`, `.ph-port`, `.ph-frame`, `.ph-state.<conn>`, `[data-role=ctr]`; tools `[data-act=mode][data-mode=text(TERM)\|hex]`, `[data-act=pause\|clear\|search\|theme\|disconnect\|connect\|settings\|more\|macros-menu]`, `select.eol-sel[data-act=eol]` |
| **Real terminal** | `.term-wrap[data-tab][data-theme]` → `.term.xslot[data-view=xterm][data-tab] > .xhost[data-tab] > .xterm` (xterm's own DOM; focus target `.xterm-helper-textarea`). Log/HEX/search view: `.term[data-view=log] .tlines[role=log]` |
| Search | `.search.open input[data-act=search-q]`, `[data-role=count]`, `input[data-act=search-filter]` |
| Connection overlay | `.conn-overlay[data-conn]` → `[data-act=connect]`, `[data-act=cancel-retry]` |
| Sidebar | toggle `#tbSide[aria-expanded]`, rail `#railSessions\|#railMacros[aria-expanded]`; sessions `.sess[data-sid]` + `[data-act=open-here\|open-right\|open-down\|edit-session]`, groups `.group-h[data-act=toggle-group][data-group][aria-expanded]`; macros `.mac[data-mid]` + `[data-act=run-macro\|edit-macro\|dup-macro\|del-macro\|stop-macro]` |
| **Session dialog / ports** | `#dlgSession[open]`: `select[data-role=portsel][name=portSel]` (option `__manual` → `input[data-role=portman]`), `[data-act=rescan]`, `[data-role=portinfo]` → `[data-act=port-focus\|port-move][data-tab]`, `[data-act=port-open-saved][data-sid]`; `[name=alias\|group\|profile\|baud\|dataBits\|parity\|stopBits\|flow\|eol\|color\|echo\|ts\|autoReconnect]`; submit `button[value=save\|open]`; error `[data-role=err]` |
| Macro editor / run | `#dlgMacro` (as before: `.step-row`, `[data-f]`, `[data-add]`, `[data-sa]`, `[data-va]`, `[data-act=cap-hotkey]`); `#dlgRun` → `[data-role=target]`, `[data-role=go]`; run strip `.runstrip[data-status]`, `.step.run\|ok\|fail\|skip`, `.btn-stop[data-act=stop-macro]` |
| Import/export, drag | `#dlgConfirm textarea[data-role=json]`; `.drag-ghost`, `#dropOv.show`/`#dropLabel`, `#insertMark` |

**Read-only hook:** `window.loomUartOpus.snapshot()`. It returns the layout tree with rects, the active and maximized pane, and these per-tab fields: `{alias, pane, conn, mode, theme, rx, tx, lines, run{status, step, steps}, macroSends[], xterm{cols, rows, attached, opened, focused, bg, cursorX, cursorY, selection, viewportY, baseY, visible[], tail[]}, deviceLine{prompt, buffer, cursor}}`. It also returns `macros`, `sessions`, `detectedPorts[]` and `storageKeys`. Use it for assertions only.

## Interaction contract (native QA on this hash)
1. **Initial:** `mcu` | `aux`, ratio 0.56. Dark terminals stream boot logs, then `loom-mcu login:` (a prompt without a newline, cursor after it).
2. **Real terminal:** click the mcu terminal (xterm focus), type `root` Enter, type the password (**not echoed**) Enter, and expect `mcu> `. Then:
   - `sta`<Tab> → `status ` (completed), then Enter gives the green `OK`.
   - <Tab> on an empty line lists the commands.
   - ↑ recalls the previous line; ←/→/Home/End/Delete/Backspace edit mid-line.
   - Ctrl+U clears the line, Ctrl+C prints `^C` and a fresh prompt, Ctrl+L clears the screen and redraws the prompt.
   - Paste inserts text.
   - Telemetry lines arriving while you type are printed above and your partial line is redrawn.
   - Select text with the mouse, then Ctrl+C or Ctrl+Shift+C copies (no `^C` sent).
   - Each terminal keeps its own focus and line; aux completes `AT+ST`<Tab> → `AT+STAT?`; `debug` ignores input (read-only stream).
3. **Persistence of terminal across layout:** split, drag a tab to another pane or edge, maximize/restore, switch tabs. Expect the same scrollback and cursor, and cols/rows updated (snapshot `xterm.cols/rows` change with the pane size).
4. **Split/resize/move/max/close, settings, theme, search/HEX/pause/clear, persistence:** same as before. HEX and search show the log view; returning to TERM shows the same terminal. The theme toggle changes only that terminal (`xterm.bg` `#0f1216` ↔ `#ffffff`).
5. **Ports:** `[data-cmd=new-session]` opens a dropdown of detected ports; `/dev/ttyUSB0` shows `사용 중: mcu`, and a free port (`/dev/ttyUSB3`) is the default.
   - Pick ttyUSB0 → focus/move buttons appear, and "저장 후 활성 pane에 열기" is refused.
   - Pick ttyUSB3 → it opens a new tab, and the dropdown or `detectedPorts` then shows it in use.
   - Simulated unplug of a port (`⋯` → 장치 분리 시뮬레이션) removes it from the list after a refresh, until reconnect.
6. **Macros:** run, stop and CRUD as before (Login F2, Read status F3, the pane header macro menu, the sidebar).
   - *Stop test:* stop mid-delay → no further sends.
   - *Disconnect test:* disconnect or unplug mid-run → status `stopped` with the reason, and reconnecting never resumes it.

## Responsive / known limits
- Responsive rules are unchanged from REV 2 (≤900 sidebar overlay, ≤640 stacked panes). In panes ≤420 px the header drops `.ph-port` and its tools scroll.
- **Simulated peer, real emulator.** The xterm emulator is real; the device on the other end is a deterministic local simulator. This is not a real UART engine: no Web Serial, no host shell, no SSH, no hardware, no eval.
- Tab completion and editing are the simulated device's behaviour, not xterm's.
- The TX log is reconstructed from bytes sent (BS/^U/^C applied), so HEX shows line-level TX plus EOL, not each key.
- The cols/rows fit reads xterm's private render cell size, with a DOM-measurement fallback.
- Reserved workbench keys (macro hotkeys, Alt layout keys) never reach the device.
- Drag uses pointer events. If the driver can't produce real pointer moves, use Alt+Shift+Arrow or the tab menu.
