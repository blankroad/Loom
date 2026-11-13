# Sol UART — native Ego Lite QA summary

QA: claude-opus-5-5 (native Claude Code), Ego Lite TaskSpace 15 `uart-workbench-qa`, page p1. URL http://127.0.0.1:4180/uart-workbench/sol/index.html. Only `loom.uart.sol.v1` was ever removed.
Raw records: `qa/sol/results.json` (every attempt kept, with sourceHash/viewport/expected/observed/screenshot; reruns carry `rerunOf`/`rerunReason`).

**Verdict: ACCEPTED at current hash** — index `76095ffc` / styles `69242e25` / app `33f49dc0`. 50 unique checks; latest result of each = PASS; 0 open product failures.

## Phase hashes
- Phase A (frozen 1): index 76095ffc / styles 276c1930 / app 13430227 — interaction checks on one loaded page.
- Phase B (re-frozen fix): index 76095ffc / styles 69242e25 / app 33f49dc0 — single reload, only affected layout/resize/rail/persistence + the deferred cancel check rerun.

## Product failures → resolved
| Check | 1st FAIL (phase A) | Resolution (phase B, new hash) |
|---|---|---|
| narrow-autocollapse-not-persisted | 390 load wrote `railCollapsed:true`; 1440 reload sidebar 0px (04-FAIL-…png) | PASS: `railPreference:null`, 1440 sidebar 232px (B04) |
| leaf-min-width-280 @768 | panes 342/218 (05-FAIL-…png) | PASS: 280/280, overflow in `#tree-scroll` (B02) |
| nested leaf minima @1440 | split-right aux → 241/197 (10-…png) | PASS at true 1440x900: 522/522/352/280 × ≥240, doc 1440×900, runner visible (B13) |
| split-down-fits-available-height | tree 624→1258, doc 944→1577, runner off-screen (11-FAIL-…png) | PASS: tree 538 constant, panes 289/240 (B10; measured 1309x858) |
| workspace-height-stable-idle | tree 1900→2020 in 12 s (14-FAIL-…png) | PASS: 13 s idle extents constant, `.log` scrollHeight +60 (measured 1309x858) |
| macro-cancel-on-disconnect | attempts 1–2 inconclusive (late click; Run off-viewport due to height bug) | PASS: disconnect 1064 ms into 1500 ms delay, no `sensors read`, runner STOPPED, target shown `(offline)` (B20) |

## QA-harness corrections (no product change; 1st record preserved)
keyboard 3% (clamp expectation), Alt+Shift+M (focus was in input — intentional skip), macro-run lock (placeholder target picked / echo+reply counted twice), macro-run-complete (15 B arithmetic), macro-duplicate (Duplicate/Delete live inside edit dialog), y drag (bottom already at 240 min).

## Phase A PASS (not rerun; unaffected by layout fix)
Direct input target-only, per-terminal theme, native tab DnD (active + inactive; `page.dragAndDrop` real input drag; tab id/theme/TX/output preserved; source collapses), Alt+Shift+M, maximize/restore, close tab/pane, last-pane recovery + add-tab, macro create/edit/duplicate/delete, run lock + Stop during 3000 ms delay → no `must-not-send` after 5 s, complete 3/3, Alt+3 selects without run, connect toggle, local echo, HEX/TEXT, search, pause/resume, clear-view-only, new session, profile vs instance settings.

## Phase B regression PASS
x/y native pointer drag (9px, spans parent, aria updates, no doc growth), x/y clamps incl. subtree 569, persistence reload (pane ids, tab locations, themes, T03 57600, ratios 58/45/56, user macro, no auto-run), export JSON (`uart-macros.json`), sections collapse, explicit rail preference survives reload, per-terminal theme at 768 and 390 (only aux light, chrome Bench Light, no hscroll).

## Known limits
- Simulation only; no native UART backend/hardware verified (as the mockup states).
- Ego emulated viewport resets per process; from phase B each script re-pins and records measured size. Split-down/idle reruns measured at 1309x858; true-1440 geometry confirmed by B13 after reallocation.
- Alt+Shift+M / Alt+3 tested with CDP key events (`e.key` "M"); a physical macOS Option key may report a composed `e.key` (handler uses `e.key`, not `e.code`) — unverified, low risk.
