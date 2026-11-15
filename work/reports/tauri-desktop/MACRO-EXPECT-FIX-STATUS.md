# MACRO-EXPECT-FIX — STATUS: READY (2026-11-15)

Foreground Opus 5.5 worker, direct implementation (no subagents, no build/browser/Git).
Scope: FINDING-MACRO-EXPECT.md (main fix + stale strip) and FINDING-UART-SCOPE-LAYOUT.md **item 1 only**, which ROOT handed over mid-run. Item 2 (split layout) belongs to the shell worker and was not touched.

## 1. Macro wait used kind `text` (every wait failed on the real engine)
- Fix: `ui/src/workbench/uart/workbench.ts` uart.expect → `patterns: [{ kind: 'literal', pattern }]`.
- Contract checked against Rust: `loom-uart-worker` `PatternKind` = `literal|regex|prompt` (snake_case, `channel/query.rs:271`); `worker.rs expect()` reads `patterns`, `timeout_ms`, `from` (`"now"` → `PosSpec::Now`, `query.rs:42`); CLI `patterns_json` sends `{"kind":"literal"}` (`crates/loom-uart/src/cli.rs:377`).
- Unchanged: the verdict comes only from `matched`, plus STOP/abort/generation re-check, pre-register before send, `dropExpect` cleanup and no retry.
- The old unit test asserted the wrong `kind:'text'`. It now asserts `'literal'`.

## 2. Stale "실행 중 · 1/2 ■ 중지" row (rows accumulated)
- Cause: `terminalPanel.ts renderRun` called `runStrip.append(top, steps)` on every key change without clearing, so each state (running → failed …) stacked a new row. The workbench state itself was correct: runningCount was 0.
- Fix: `runStrip.replaceChildren(top, steps)`. The strip shows exactly the channel's latest run, and the error stays visible.
- Pre-registration rejection: a registration error normally arrives after the send, so the run fails at the wait step once (error text kept, later steps `skip`, no further send or expect). No behavior change.

## 3. Hidden-tab CONNECTED session excluded as "상태 확인 중" (SCOPE-LAYOUT item 1)
- Cause: `RenderScheduler` never draws hidden panels (`renderScheduler.ts:195`), and the panel applied the snapshot/backlog `uart.state`/`uart.config` only inside `draw()`→`consume()`. A tab that was mounted or remounted in the background kept `port.status='?'`, and `blocked()`/`canWrite()` read that stale value.
- Fix (`terminalPanel.ts`): a model-only `syncPort()` with its own cursors applies the snapshot port and every state/config row through the existing `applyPort` seq guard. It runs:
  - on every mirror append (calling `uartBench.bump()` when the status changes),
  - lazily inside `blocked()`/`canWrite()`,
  - at the start of `consume()`.

  The control path also bumps the bench on a status change. Nothing is forced visible and `'?'` is never treated as open. Real closed, read-only, STOP and generation still refuse, and a hidden close still stops the run through `applyPort`.

## Tests (focused, passing)
- `npx vitest run src/workbench/uart src/panels/uart`: 6 files, 59 tests passed.
  - workbench.test.ts: new tests for the exact request shape (`toEqual` on all params, kind ∈ literal|regex|prompt), and for an engine rejection that fails the run once (err kept, steps ok/fail/skip, runningCount 0, no later send, one expect).
  - uartFidelity.test.ts, run strip: two rejected runs each leave exactly one failed row (no 실행 중, no stop button, error shown, 2 expects / 2 inputs total); STOP mid-wait leaves only the stopped row.
  - uartFidelity.test.ts, hidden tab (events delivered with no frame): a background CONNECTED tab is included in 연결된 모두/선택 and receives the macro send; unknown, a later closed row (an older row does not re-open it) and watch rights are refused; when the tab is shown it draws the same state.
- Mutation checks:
  - Reverting to `append`: the strip tests fail (3 rows / 2 rows).
  - Making `syncPort` draw-only: both hidden-tab tests fail.
- `npm run typecheck` (tsc -b): 0 errors.
- Scoped eslint on the 4 files: clean.

## Files and fingerprint (sha256)
```
c5858231e4cf8d337602d8dbd39b3de9ff35ffa0afce71dc763c17cb78781f58  ui/src/workbench/uart/workbench.ts
eea0b8d374c807d2eb10dd37a537b7c2b66618061514645fc41a383f692608be  ui/src/workbench/uart/workbench.test.ts
270b56a155a55d3a36f61aa7c4bc55c0af90127eb188871a771eb14bfdf80769  ui/src/panels/uart/terminalPanel.ts
1557bde4b309b62ca8ecc09dc70f2867c9c3f1fbfc2f42583dd9de0421843525  ui/src/panels/uart/uartFidelity.test.ts
```

## For finalQA (after the combined rebuild)
- Rerun macro `mping` send→wait "ack:mping" (활성/모두/선택): expect 완료, one row per pane, rx.log +1 per recipient.
- Rerun the STOP-mid-run check.
- Retest hidden-tab dut2: it must be in 연결된 모두 with no 상태 확인 중 exclusion.
