# Opus UART workbench — checkpoints
> LATEST: REV 6.1 READY — F5: import remaps fixed ids (payload id → new/alias-matched session), unresolved ids kept visibly + whole fixed macro blocked (never widens to panel scope), session delete keeps unresolved. app.js cbc41b96, styles e3a02266; manifest selfcheck/source-sha256-rev6.1.txt. Full suite 122/122 before the mixed-block correction; focused rev61-mixed 3/3 after. No browser.
> REV 6 SOURCE FROZEN (review accepted; SURVEY wording-only correction afterwards: typing-only BC chips vs macro recipient labels/run strips, historical pre-REV3 bullets updated). Source hashes unchanged: selfcheck/source-sha256-rev6.txt.
> LATEST: REV 6 READY (macro execution scope 활성/연결된 모두/선택 in macro panel, separate from 키 입력 대상; one rule: 고정 override > panel scope for sidebar/pane menu/hotkey; per-execution snapshot; legacy target -> visible 고정). Local jsdom 117/117 PASS. Hashes: selfcheck/source-sha256-rev6.txt. SURVEY.md REV6 benchmark table. No browser used. QA: hard-reload.
> EARLIER: REV 5.2 FINAL FREEZE — wording-only on REV 5.1 (Esc hints qualified "터미널 밖에서 Esc"). app.js bdaf6bd5…; manifest selfcheck/source-sha256-rev5.2.txt. Existing 103/103 PASS ran on REV 5.1 immediately before this label-only edit; node --check after. QA: hard-reload.
> PREVIOUS: REV 5.1 FROZEN (review fixes: sidebar ▶ alone + ⋯ edit/dup/delete menu, real target labels, macro-wide 전체 중지, F1 pause-evict gap count, F4 comment). Local 103/103 PASS. Hashes: selfcheck/source-sha256-rev5.1.txt. QA: hard-reload. No further changes until actual QA defects.

## checkpoint 1 (2026-11-13)
Model: claude-opus-5-5, native Claude Code foreground. No subagents. Owned: work/reports/uart-workbench/opus/ (+ shared qa/ later).
Plan (single proposal, "Loom Serial Bench / tree-split"):
1. Scoped official research: MobaXterm (primary: serial session settings, split/terminal tabs, macros), Windows Terminal panes, WezTerm/Tabby. Record exact URLs + observations -> SURVEY.md.
2. Build index.html + styles.css + app.js (native, no CDN): binary split tree, pointer-drag dividers w/ min size, focus/max/restore/close, tab drag between panes + Alt+Shift+Arrow move, saved sessions (mcu 115200 8N1, aux 9600 8N1, console, debug), per-session settings, sim RX logs, TX input/local echo, disconnect/reconnect, TEXT/HEX, search, pause, clear, macro CRUD (steps send/delay/wait, vars, hotkey), run w/ target confirm + stop, localStorage prefix `loomUartOpus.v1.` + JSON import/export.
3. Self-check split/macro state, then HANDOFF.md. Sol creative files not read before freeze.
Status: research starting.

## Checkpoint 2 — first runnable (2026-11-13)
- index.html + styles.css + app.js on disk; `node --check app.js` OK; served 200 at http://127.0.0.1:4180/uart-workbench/opus/index.html
- Steering applied: Bench Light outer shell (F2 Primer light tokens), UART-only shell nav (no planned-service buttons), UART module owns sessions/tabs/splits/macros, per-terminal dark/light theme (default dark).
- Browser QA now owned by dedicated QA worker; Opus does not open a browser. Next: headless logic self-check, SURVEY.md, HANDOFF.md.

## Checkpoint 3 — SOURCE FROZEN 2026-11-13 22:19
Runnable: http://127.0.0.1:4180/uart-workbench/opus/index.html
3470995fa5b69fd6f91716942a8aa50a022cef37f59b50f64eb8a22d0cc89995  index.html
2b761965c62186db21029a255476f7d540517f5f420207563e5151cc1982bb2c  styles.css
cff8a6706efa7ca26f24e5316b9afbaf8133011773fb1e77e5e151071664227f  app.js
Headless jsdom smoke 31/31 PASS (selfcheck/selfcheck-result.json). One real defect found+fixed before freeze: init crash (newTab rendered sidebar before tab placed).

## Checkpoint 4 — HANDOFF.md + SURVEY.md written; source unchanged since freeze. Awaiting dedicated QA defects only.

## Fix 1 — 2026-11-13 22:26 READY for QA re-check
QA CHANGES_REQUESTED min-size-pane-usable (276x150 send overflow). Narrow-pane container query fix (styles.css + send label span in app.js). Local 33/33 PASS. Hashes: selfcheck/source-sha256-fix1.txt

## REV 2 — 2026-11-13 22:30 READY (QA: reload before re-check)
User spec: send row/buttons removed -> inline terminal prompt (Enter/↑↓/Ctrl+U/Ctrl+C, click-to-focus w/o breaking selection); explicit sidebar toggle #tbSide + rail aria-expanded, groups persist. QA bug pane-maximize fixed (CSS: hide non-max .child + dividers). Fix-1 send-row CSS superseded/removed.
Local 44/44 PASS. Hashes: selfcheck/source-sha256-rev2.txt

## REV 3 — 2026-11-13 22:42 READY for native QA (hard-reload)
Real xterm (vendored @xterm/xterm 6.0.0 MIT, unmodified) replaces inline prompt; sim device line discipline (echo/edit/history/Tab/^C^U^L, no-echo password, ANSI/CRLF). Detected-port dropdown (sim registry, uart.ports shape) with in-use focus/move + no duplicate physical port. QA bug fixed: disconnect/lost aborts running macro, reconnect never resumes. Read-only xterm buffer seam in snapshot.
Local jsdom 58/58 PASS. Hashes (incl. vendor + LICENSE): selfcheck/source-sha256-rev3.txt

## REV 3.1 — 2026-11-13 22:48 READY (hard-reload)
QA xterm-multiline-paste: random per-command timers replaced by ordered per-device input queue (fixed 60ms latency); ^C out-of-band; stop/disconnect/macro-stop clear queue. app.js only. Local 61/61 PASS. Hashes: selfcheck/source-sha256-rev3.1.txt

## REV 4 — 2026-11-13 23:17 checkpoint (logging + timestamps), local 84/84 PASS
Hashes: selfcheck/source-sha256-rev4.txt. Next: REV 5 delta (macro run-vs-edit + Current/All-connected target) — small incremental patch.

## REV 5 — 2026-11-13 23:21 FROZEN for native QA (hard-reload)
Delta on REV 4: immediate ▶ / separate ✎ / vars-only dialog; send target Current (default, not persisted) vs All-connected (frozen recipients, exclusions shown, per-target runs with group id, global stop, per-target disconnect cancel); format labels (.log line-buffered vs .jsonl raw chunks). Local 98/98 PASS. Hashes: selfcheck/source-sha256-rev5.txt
