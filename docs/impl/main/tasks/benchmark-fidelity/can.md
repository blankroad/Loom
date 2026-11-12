# CAN / CAN-TP fidelity / DONE
Model MUST be Claude Opus 5.5 (verify UI reports this; --model opus resolves 5.5 here, no model fallback). Foreground Herdr dedicated module; shared cwd Loom base26dc5e8. User wants service-by-service parallel work, not one broad worker. No subagents or background CLI, no git index/commit, no global security/config changes. Read MAIN.md in this directory and existing research/benchmark-can.md selectively.

Primary benchmark: PEAK PCAN-View
Start official source: https://www.peak-system.com/products/software/analysis-software/pcan-view/
Focus: Actual Rx and Tx lists, columns, ID-based latest receive mode vs Trace view, cyclic/manual transmit and edit flow, controller/bus status. Do not substitute generic metric cards. Current can.trace chronological data + worker periodic/isotp capabilities; never invent RPC or bus states. CAN-TP raw/reassembled distinct; UDS TP is TesterPresent. Propose implementation stages for data-model changes.

PHASE1 immediately: directly inspect reference visuals, not just marketing feature lists. Use official manuals/screenshots/videos, save any public reference images with source URLs and distinction between screenshot and authored redraw. Do not reproduce logos or claim Loom is the reference app. Actual visual evidence / documented interaction / inference must be labeled separately. Own docs/impl/main/research/fidelity/can.md and work/reports/fidelity/assets/can/ only.
Deliver concise but SPECIFIC fidelity matrix (~8–12 rows): reference area/control, source + screenshot/figure, actual interaction, current Loom location/gap, exact mapping, retained Loom safety/data semantics, intentional deviations/unimplemented backend. Include concrete panel layout plan and acceptance checks, identify shared SDK/global change dependencies. Real hardware and commercial app not run unless actually done. No broad popularity research again. Initial checkpoint ~3–4min. Return path READY_FOR_REVIEW and wait MAIN GO before touching product source.

On product GO only, owner ui/src/panels/can/ files and own scoped CSS/tests (can add panel-local style module, no global shell/SDK edits). Do not weaken tests, mock unimplemented data, or add unsupported controls as working. Main runs aggregate/build/actual Ego and report integration. Preserve all other service edits. No cross-service API edits; request main resolution.

---
## CAN owner handoff — READY_FOR_REVIEW (source FROZEN 2026-11-12, Claude Opus 5.5 `claude-opus-5-5`, foreground, no subagents/git)

Reference matrix: `docs/impl/main/research/fidelity/can.md`; primary visuals + attribution: `work/reports/fidelity/assets/can/` (`SOURCES.md`).
Accepted route value: **`props.view = 'rxtx'`** (CAN service). Absent/other value → Trace (UDS companion, existing integrations).

### Files (only `ui/src/panels/can/`; sha256 prefix at freeze)
| file | state | sha256[:12] |
| --- | --- | --- |
| rxSummary.ts | new — bounded per-(channel, STD/EXT, id) receive summary | 7e8937ffde99 |
| txList.ts | new — manual transmit list + editor→send-line adapter | cf66f735290b |
| rxtxView.ts | new — Receive/Transmit DOM (split lists, editor) | e964ba28c753 |
| canTrace.ts | changed — view tabs/prop, ingest feeds summary, one shared gated send path, throttled rxtx redraw, manifest `view` prop | f31b312e5cb5 |
| can.css | changed — appended panel-scoped `.ct-tab*`, `.crx-*` rules (shell tokens only) | 0c36e7d61c8e |
| rxSummary.test.ts, txList.test.ts, rxtxPanel.test.ts | new tests (25) | 234906ddaecc / 577e9b08f2a3 / 75951c55cb95 |
| traceModel.ts, traceView.ts, sendLine.ts, index.ts, existing tests | **unchanged** | — |

### Implemented (exact)
1. View tabs `수신/송신 | 트레이스` inside the panel; initial view from `props.view` (`'rxtx'|'trace'`, default `'trace'`). Trace DOM stays first so `[data-testid=can-trace] [role=grid]` is still the trace grid.
2. Receive list (PCAN Receive analogue): one row per channel+format+id, columns `CAN-ID │ 유형(STD/EXT FD BRS ESI RTR) │ Len(DLC tooltip) │ Data(latest) │ 주기 ms │ 횟수 │ 마지막 s`; sorted STD→EXT, by id; changed bytes highlighted. Device rx only (own tx echoes / request-tagged frames excluded). Fed from the same `can.frame` ingest as the ring, independent of ring eviction and render pause.
3. Window vs gap: window line shows frames aggregated, ids n/1024, untracked frames beyond bound, gap marks (known lost count + unknown marks). Rows existing at a gap show count as `≥N` (`data-lower-bound=true`); cycle time blanked across a gap. `집계 초기화` resets summary only.
4. Receive row select (click/↑↓) → Enter/dblclick opens Trace with the ID filter set; trace ID filter also narrows the Receive list (display only).
5. Transmit list (PCAN Transmit analogue): `CAN-ID │ 유형 │ Len │ Data │ 주기(수동) │ 횟수 │ 트리거(수동) │ 메모`, ≤32 entries, panel memory only. Select click/↑↓, Space or `보내기` = send once, Enter/dblclick/`편집` = edit, Delete/`삭제`.
6. Editor (New Transmit Message analogue): ID hex, Length select (classic 0–8 / FD list to 64), per-byte hex cells with index, Extended / CAN FD / BRS (FD only), Comment, OK/취소, Esc. Editor builds send-line text → **the existing `parseSendLine` is the only validator**; draft drives the existing debounced `can.send{dry_run:true}` risk preview.
7. One gated send path for send line and list: `ctx.act('send', params)` (ActionGate, operate right, STALLED lock). Count increments only on accepted send with request_id; failures shown per row.

### Intentional deviations / not implemented (honest)
- Cyclic transmit: **deferred** — Cycle Time field rendered **disabled** (value 0) + `can-tx-periodic-deferred` note; no `can.periodic.*` calls, no local timers.
- RTR checkbox rendered disabled (structured `can.send` has no rtr). Trigger column always `수동`.
- No bus status, bit rate, bus load %, overruns, error counters, Error Generator, Connect dialog (no verified source). `can.status` not used (not in manifest queries).
- Count is "this panel's window", not a bus total. Trace column order unchanged (raw trace/FD/RTR/gap rows untouched).
- Pre-existing, not changed: `updateStatus` rewrites the `can-risk` text to `R1 <ENV> 자동` every frame, so a dry-run result is visible only until the next frame — MAIN may want to review (shared semantics, not altered here).

### Browser selectors (data-testid)
Panel: `can-trace` (`data-view="rxtx|trace"`), tabs `can-view-tabs`, `can-view-rxtx`, `can-view-trace` (`aria-selected`).
Receive: `can-rxtx`, `can-rx`, `can-rx-window` (`data-frames|gaps|untracked`), `can-rx-reset`, rows `can-rx-row` (`data-key|id|ext|count|flags|data|lower-bound|cycle`, `aria-selected`), grid `[aria-label="CAN 수신 요약"]`.
Transmit: `can-tx`, `can-tx-new|edit|delete|send`, `can-tx-msg`, `can-tx-periodic-deferred`, rows `can-tx-row` (`data-uid|id|count`, `aria-selected`), grid `[aria-label="CAN 송신 목록"]`.
Editor: `can-tx-editor`, `can-tx-id`, `can-tx-len`, `can-tx-bytes`, `can-tx-byte` (`data-index`), `can-tx-ext|fd|brs|rtr|cycle|comment`, `can-tx-error` (shows `→ <canonical>` or reason), `can-tx-ok`, `can-tx-cancel`.
Unchanged canonical: `can-send-input`, `can-send-msg`, `can-risk`, `can-pause`, `can-live`, `can-status`, `can-agent-read`, `can-new-rows`, `.ct-row[data-*]`.
DEV probe: `__loomCan[panelId].summary|txList` added.

### Checks run (2026-11-12)
- `npx vitest run src/panels/can` → 6 files, 72 tests pass (47 existing unchanged + 25 new).
- `npx vitest run` (ui, whole) → 50 files, 553 pass.
- `npx eslint src/panels/can` → clean. `tsx scripts/gen-schema.ts --check` → ok.
- `tsc -b` → only error is `src/panels/uart/uartFidelity.test.ts:26` (UART owner's file, not CAN).
- Not run: browser/Ego, e2e (MAIN-owned), real hardware, PCAN-View.

### Suggested MAIN Ego acceptance
CAN service (`view=rxtx`): sim `0x310` → one `can-rx-row[data-id="0x310"]` with growing `data-count`, `data-cycle` ≈ trace Δt; Enter on it → `data-view=trace` with filter `0x310`. `+ 새 메시지` → fill → OK → Space → USR tx row in Trace + `can-tx-row[data-count="1"]`. Watch-only rights → `can-tx-send` disabled. UDS companion CAN panel opens `data-view=trace`.

MAIN accepted frozen handoff. Post-handoff main removed per-frame hardcoded R1 risk overwrite and extended existing test to check persistence across h.frame(); focused 18 PASS. Full worker suite 553 PASS; final main schema/lint/type/build PASS. MAIN owns remaining native/Ego. Normal CLI exit agent_not_found verified, own wT closed.
