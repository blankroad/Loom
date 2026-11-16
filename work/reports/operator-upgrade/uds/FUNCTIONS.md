# UDS operator upgrade — FUNCTIONS (function-body contracts)

Owner of every signature, type and test: Opus. Owner of every body listed in an allowlist: Sonnet.
If a contract is unclear, a needed helper is missing, or a test seems wrong: stop and report the exact
function and reason to Opus (Herdr). Do not add, rename or re-shape exported types and signatures. Do not edit tests,
manifests or `registerPanel` options. Do not edit functions outside the allowlist.

---------------------------------------------------------------------------------------------------
# BATCH 1 — U4 transaction layout · U3 DTC status bits · U7 profile-less recovery (UI only)

**Status: PLAN_READY_BATCH1**

## B1.0 Lease, allowlist, acceptance

**LEASE GRANTED to `upgrade-uds-sonnet` (2026-11-15):** exact FUNCTION BODIES only (incl. callbacks) in
`ui/src/panels/uds/consolePanel.ts`, `ui/src/panels/uds/udsModel.ts`, `ui/src/panels/uds/udsPrefs.ts`.
Opus makes no edits to these three files until the lease is returned.
Opus-authored and done before the grant, NOT in the worker scope (do not touch):
- `uds.css` (B1-C rules applied).
- All import lists (B1-I).
- Static metadata and DOM construction: column order/headers/widths, `ibtn exp` class, `DTC_FILTERS`,
  `let dtcFilter`, `dtcFilterSeg`/`dtcCount`/`dtcFbar` construction and `dtcSec.append`, the `mkProfileBtn`
  construction and its `addEventListener('click', onMakeProfile)`.
- New exported types/signatures and the named callback stubs.
Tests are Opus-owned and read-only for Sonnet: `udsBatch1.test.ts` (new) and `uds.test.ts` (DTC test updated to U3).

Body allowlist (nothing else may change):

| ID | File | Function / code site |
|---|---|---|
| B1-F1 | udsModel.ts | `dtcStatusFlags` |
| B1-F2 | udsModel.ts | `dtcAvailabilityMask` |
| B1-F3 | udsModel.ts | `filterDtcs` |
| B1-F4 | udsModel.ts | `dtcStatusTitle` |
| B1-F5 | udsModel.ts | `latestDtcs` (existing; fill the new fields) |
| B1-F6 | udsModel.ts | `hexRowWidth` |
| B1-F7 | udsModel.ts | `laFromAddr` |
| B1-F8 | udsModel.ts | `linkDownMessage` |
| B1-F9 | udsPrefs.ts | `profileDraftForTarget` |
| B1-P1a | consolePanel.ts | `cell:` lambdas of the `t` and `ecu` columns only |
| B1-P1c | consolePanel.ts | `renderDetail` — memo key + the hex block only |
| B1-P2a | consolePanel.ts | `renderDtc` body |
| B1-P2b | consolePanel.ts | `onDtcFilter(v)` body (stub) |
| B1-P3b | consolePanel.ts | `openCfg` body |
| B1-P3c | consolePanel.ts | `renderUi`: the `!linkAllowsWrite()` message branch, the `connectBtn.title` `!pr` branch, one new `mkProfileBtn.hidden = …` statement |
| B1-P3d | consolePanel.ts | `onMakeProfile()` body (stub) |

Every `throw new Error('UDS-B1-… not implemented (Sonnet)')` stub (9 in udsModel/udsPrefs, 2 in consolePanel) must be
replaced, including the `void x;` placeholder lines. None may remain.
Post-scaffold state (Opus, measured): `vitest run src/panels/uds` → 64 pass, 26 fail (all body-dependent).
`tsc -b` → the single expected error in `latestDtcs` (B1-F5).

Acceptance (run from `/Users/ctmctm/workspace/Loom/.worktrees/operator-uds/ui`, no installs). Per COMMON: never use
shared-cache `tsc -b`; once, from the worktree root, run `mkdir -p work/typecheck`:
```
./node_modules/.bin/vitest run --maxWorkers=2 src/panels/uds          # expect 0 failures (baseline before B1: 61 pass; B1 adds 29 + 1 updated)
./node_modules/.bin/tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo --pretty false    # no output
./node_modules/.bin/tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo --pretty false  # no output
./node_modules/.bin/eslint src/panels/uds             # expect no errors
```

**B1 ACCEPTED (Opus review, 2026-11-15).** The worker returned BLOCKED_ON_LEAD because my scaffold missed
`import { laFromAddr } from './udsModel'` in udsPrefs.ts. I added it after the lease was returned.
Diff reviewed against every B1 contract: no deviation, no out-of-allowlist edit, 0 stubs left.
Results (isolated): vitest 90/90 pass (5 files). tsc app exit 0, node exit 0. eslint exit 0.
sha256: consolePanel.ts 2639d232…8bb6, udsModel.ts 0251e7bd…1598, udsPrefs.ts cec9e82d…8ec2, uds.css c82f0080…9f0c,
udsBatch1.test.ts b95c2878…663f, uds.test.ts fd29d493…dfd5.
Pending ROOT GUI only: U4 alignment / labels and U7-2 capture.
Failing-first evidence (Opus): before the static scaffold, 30 failed (29 in udsBatch1.test.ts + the updated DTC test)
and 60 passed. After the static scaffold, 26 failed and 64 passed.

General UI rules: build DOM with the existing `el()/button()/segGroup()` helpers; set text via `textContent`
(the helpers already do). No `innerHTML` with row/target data. No new timers. Keep existing `data-testid`s.

---------------------------------------------------------------------------------------------------
## B1-F1 `dtcStatusFlags(status: number, availability: number | null): DtcFlag[]`

- Input: `status` integer 0..255 (callers only pass a parsed byte). `availability` integer 0..255 or null (unknown).
- Output: exactly 8 `DtcFlag`, bit order 0→7. `abbr`/`name` copied from `DTC_STATUS_BITS[bit]`.
  `set = ((status >> bit) & 1) === 1`. `supported = availability === null ? null : ((availability >> bit) & 1) === 1`.
- Pure; no allocation beyond the array.
- Examples: `(0x09, 0xFF)` → set bits {0,3}, all supported true. `(0x2F, null)` → set {0,1,2,3,5}, supported all null.
  `(0x80, 0x09)` → bit 7 `{set:true, supported:false}`.

## B1-F2 `dtcAvailabilityMask(r: RowLike): number | null`

- `hex = respHexOf(r)` (existing helper). Return `parseInt(hex.slice(4,6),16)` iff `hex !== null`, `hex.length >= 6`,
  `hex.slice(0,2) === '59'` and `(parseInt(hex.slice(2,4),16) & 0x7F) === 0x02`. Otherwise null.
  The row state is not checked here (latestDtcs already only picks final non-failed rows).
- Examples: `5902ff…` → 255. `5902` → null. `7f1931` → null. `59010901` → null.

## B1-F3 `filterDtcs(entries: readonly DtcEntry[], filter: DtcFilter): DtcEntry[]`

- `'all'` → a NEW array with the same entries in the same order.
- `'confirmed'` → bit 3, `'testFailed'` → bit 0, `'pending'` → bit 2: keep entries with `statusByte !== null` and that
  bit set, order preserved. An entry with `statusByte === null` never matches a bit filter.
- No sorting; response order is the evidence order.

## B1-F4 `dtcStatusTitle(status: number | null, availability: number | null): string`

- `status === null` → exactly `'status 미보고'`.
- Else lines joined with `'\n'`:
  1. `status 0x%02X = 0b%08b` (upper-case hex, 8 binary digits), e.g. `status 0x09 = 0b00001001`.
  2. For each flag of `dtcStatusFlags(status, availability)`:
     `bit${bit} ${abbr} ${name}: ${set ? 1 : 0}` + (`supported === false` ? ` (ECU 미지원)` : ``).
  3. Last line: `availability mask 0x%02X` or `availability mask 미상` when null.
- Exact example (tested): `dtcStatusTitle(0x09, 0xF9)` → see udsBatch1.test.ts B1-F4.

## B1-F5 `latestDtcs(rows, target): DtcView | null` (existing — extend)

- Same row selection as today (scan from newest; target match; `isFinal`, not `failed`, `Array.isArray(payload.dtcs)`).
- For each `payload.dtcs` item `o`:
  - `dtc = s(o.dtc) ?? '?'` (unchanged). `st = hexByte(o.status)`. `status = st ? '0x'+st : '—'` (unchanged legacy text).
  - `statusByte = st !== null && /^[0-9A-F]{2}$/.test(st) ? parseInt(st, 16) : null`. (Numbers 0..255 and strings
    like `'0x08'` parse; `'zz'` → status `'0xZZ'`, statusByte null; missing → `'—'`, null.)
  - `flags = statusByte === null ? [] : dtcStatusFlags(statusByte, null).filter(f => f.set).map(f => f.abbr)`.
- `availabilityMask = dtcAvailabilityMask(r)` of the chosen row.
- Example: fixture `dtc_list` → `{availabilityMask: 255, entries: [{dtc:'923400',status:'0x09',statusByte:9,flags:['TF','CDTC']}, {dtc:'C10000',status:'0x2F',statusByte:47,flags:['TF','TFTOC','PDTC','CDTC','TFSLC']}]}`.

## B1-F6 `hexRowWidth(paneWidthPx: number): 8 | 16`

- `!Number.isFinite(w) || w <= 0` → 16 (unmeasured: jsdom, hidden pane — keeps today's layout).
- `w < 520` → 8. Else 16. (520 px fits offset + 16×hex + ASCII at the 12 px mono font.)

## B1-F7 `laFromAddr(addr: string | null): string | null`

- null/empty → null. Match the TRIMMED string against `/(?:^|\s)0x([0-9a-f]{1,4})$/i`. On match →
  `'0x' + group.toUpperCase().padStart(4, '0')`. Else null. Never guesses from CAN ids or other shapes.
- Examples: `'DoIP 0x1001'`→`'0x1001'`, `'DoIP 0xe80'`→`'0x0E80'`, `'0x0001'`→`'0x0001'`, `'CAN 7E0/7E8'`→null,
  `'DoIP 0x12345'`→null, `'DoIP 0x1001 extra'`→null.

## B1-F8 `linkDownMessage(link: string, hasProfile: boolean): string`

- `hasProfile` → `` `링크 ${link} — 읽기 전용 (기록은 유지). [연결]로 다시 연결` `` (byte-identical to today's text).
- else → `` `링크 ${link} — 읽기 전용 (기록은 유지). 이 대상의 프로필 없음 — [이 대상으로 프로필 만들기] 후 [연결]` ``.

## B1-F9 `profileDraftForTarget(ecu: string, reported: ReportedRoute | undefined, id: string): ConnPreset` (udsPrefs.ts)

- Returns `{ id, ecu, label: `${ecu} (daemon 보고에서)`, sim: reported?.backend === 'sim', host: '', port: 13400,
  la: laFromAddr(reported?.addr ?? null) ?? '', testerLa: '0x0E80', activation: 0, timeoutMs: 5000 }`.
- Import `laFromAddr` from `./udsModel` (no cycle: udsModel imports nothing from udsPrefs).
- Never reads `prefs`; never copies another preset. `la: ''` is deliberate: `presetIssues` flags it and the user must enter it.

---------------------------------------------------------------------------------------------------
## B1-P1 U4 transaction table (consolePanel.ts + uds.css)

### B1-P1a `columns` — order/headers/widths ALREADY APPLIED by Opus; Sonnet changes only the `t` and `ecu` cell lambdas as in the last column
| # | key | header | width | cell change |
|---|---|---|---|---|
| 1 | no | `#` | 44px | — |
| 2 | t | `시각` | **108px** | row: `cell(fmtClock(firstWallMs), 'c-t', firstWallMs ? new Date(firstWallMs).toISOString() : undefined)` |
| 3 | ecu | `ECU` | **72px** | row: `cell(rowTarget(row), 'c-ecu', rowTarget(row) \|\| undefined)` (title always the full name) |
| 4 | rt | `경로` | 132px | — (B4 changes the source) |
| 5 | did | `DID · SID` | 112px | — |
| 6 | req | `요청` | 84px | — |
| 7 | st | `상태` | 136px | — |
| 8 | p | `78` | 30px | — |
| 9 | el | `경과` | 64px | — |
| 10 | rsp | `최종 응답` | `minmax(170px, 1fr)` | — (moved last) |

Effect: header and rows get the same `gridTemplateColumns` string (VirtualTable builds it from `width`).
Status and elapsed stay left of the only flexible track.

### B1-P1b export buttons — DONE by Opus (`'ibtn exp'` class). Not in worker scope.

### B1-P1c `renderDetail` hex block
- At the top of `renderDetail`, compute `const cols = hexRowWidth(hexEl.clientWidth);` and append `|${cols}` to BOTH memo
  key variants (`…` and `'none'` → `none|${cols}`). A width change re-renders: `resize()` already invalidates the scheduler.
- Replace the literal `16` (loop step and slice length) in the hex dump with `cols`. Offsets stay `o.toString(16).padStart(4,'0')`.

### B1-C uds.css — APPLIED by Opus (record only; not in worker scope)
1. Line `.uds-wb .hist { --tcols: … }` → `.uds-wb .hist { --tcols: 44px 108px 72px 132px 112px 84px 136px 30px 64px minmax(170px, 1fr); }`
2. Replace `min-width: max-content;` in `.uds-wb .hist .vt-head, .uds-wb .hist .vt-row { … }` with `min-width: 952px;`
   (= sum of the fixed tracks 782 + 170. Every row and the header are equally wide, so the tracks align.)
3. Add `min-width: 0;` to the `.uds-wb .hist .vt-th, .uds-wb .hist .vt-td { … }` rule.
4. Add after the `.pane-h .hcount` rule:
   `.uds-wb .pane-h .ibtn.exp { width: auto; min-width: 0; padding: 0 6px; gap: 3px; }`
   `.uds-wb .pane-h .ibtn.exp small { white-space: nowrap; line-height: 1; }`
5. DTC grid (replace the `.uds-ds-dtcgrid` rule; add the rest):
   `.uds-wb .uds-ds-dtcgrid { grid-template-columns: minmax(7em, auto) 5em minmax(0, 1fr); border: 1px solid var(--wb-line2); border-radius: var(--wb-radius); background: var(--wb-bg); }`
   `.uds-wb .dtc-bits { display: flex; flex-wrap: wrap; gap: 3px; min-width: 0; }`
   `.uds-wb .dtc-bit { font: 600 10px var(--wb-mono); padding: 0 4px; border-radius: 3px; background: var(--wb-sf2); color: var(--wb-text); text-decoration: none; }`
   `.uds-wb .dtc-bit.b0 { color: var(--wb-err); } .uds-wb .dtc-bit.b3 { color: var(--wb-warn); } .uds-wb .dtc-bit.b2 { color: var(--u-pend); }`
   `.uds-wb .dtc-bit.na { border: 1px dashed var(--wb-line); }`
   `.uds-wb .dtc-fbar { display: flex; align-items: center; gap: 8px; margin: 4px 0; }`

ROOT GUI (not jsdom): VIN row + short NRC row align, status/elapsed visible without scrolling, `JSON`/`CSV` labels separate.

---------------------------------------------------------------------------------------------------
## B1-P2 U3 DTC pane (consolePanel.ts)

Already in place (Opus): `let dtcFilter: DtcFilter = 'all'`, `DTC_FILTERS`, `dtcFilterSeg` (class `sm dtc-filter`,
callback `onDtcFilter`), `dtcCount` (`.hcount.dtc-count`), `dtcFbar`, and `dtcSec.append(h3, dtcActions, dtcFbar, dtcBody)`.

`onDtcFilter(v: DtcFilter): void` (B1-P2b): `dtcFilter = v; dtcKey = ''; ctx.scheduler.invalidate();`.
Nothing else, no persistence. (`dtcKey` is declared later in the closure. It is only touched when clicked, after init.)

`renderDtc` (B1-P2a; memo key: `${rowsVersion}|${cur}|${editable()}|${dtcFilter}`):
1. `dtcReadBtn.disabled = dtcClearBtn.disabled = !editable();` and `dtcFilterSeg.set(dtcFilter);` (unchanged rule + seg state).
2. `dv = latestDtcs(rows.list(), cur)`. Header cells: `['DTC (hex)', '상태', '상태 비트 (ISO 14229-1)']`.
3. `!dv` → empty line as today; `dtcCount.textContent = ''`.
   `dv.entries.length === 0` → empty line as today; `dtcCount.textContent = '표시 0/0'`.
4. Else `shown = filterDtcs(dv.entries, dtcFilter)`; `dtcCount.textContent = `표시 ${shown.length}/${dv.entries.length}``.
   If `shown.length === 0` → `el('div','uds-ds-empty', `필터 ${label}: 해당 DTC 없음 (전체 ${dv.entries.length})`)`
   (`label` = the DTC_FILTERS label).
   For each entry `d` of `shown`, one row `div.uds-ds-row[role=row][data-dtc=d.dtc]` with exactly 3 children:
   - `el('span','mono', d.dtc)`
   - `el('span','mono', d.status)` with `dataset.cell = 'status'` and `title = dtcStatusTitle(d.statusByte, dv.availabilityMask)`
   - `span.dtc-bits` containing, for each flag `f` of `dtcStatusFlags(d.statusByte, dv.availabilityMask)` with `f.set`
     (bit order): `el('abbr', `dtc-bit b${f.bit}${f.supported === false ? ' na' : ''}`, f.abbr)` with
     `title = f.name + (f.supported === false ? ' — availability mask에 없음' : '')`.
     If `d.statusByte === null` → the span text is `'?'` (class `na`). If no bit is set → the span text is `'—'`.
5. Source note: `'출처 '` + link button (unchanged reveal) + `` ` · DTC 코드 원시값 (재매핑 없음) · 상태 비트 ISO 14229-1 · ${dv.availabilityMask === null ? 'availability mask 미상' : `availability mask 0x${hex2(mask)}`}` ``
   (`hex2` = upper-case 2-digit; inline `toString(16).toUpperCase().padStart(2,'0')`). No-dv text: `'DTC 코드·상태 원시값 + ISO 14229-1 상태 비트'`.
6. `dtcBody.replaceChildren(grid, src)` as today.

---------------------------------------------------------------------------------------------------
## B1-P3 U7 profile-less target (consolePanel.ts)

### B1-P3a make-profile button — construction DONE by Opus (`[data-cmd="make-profile"]`, hidden, click → `onMakeProfile`)

### B1-P3d `onMakeProfile(): void` (Sonnet body)
Exactly `openCfg();`. No argument, so the draft comes from the B1-P3b fallback for the selected target.

### B1-P3b `openCfg(presetId?: string)` (new body; same signature)
```
// 1. re-selecting the unsaved draft in the 편집 대상 select keeps the draft (no silent loss of edits)
if (presetId && draft && presetId === draft.id && !prefs.presets.some(p => p.id === presetId)) { cfgOpen = true; buildCfg(); uiKey = ''; ctx.scheduler.invalidate(); return; }
const t = target();
const explicit = presetId ? (prefs.presets.find(x => x.id === presetId) ?? null) : null;
const p = explicit ?? presetOf(t) ?? (t === null ? (prefs.presets[0] ?? null) : null);   // NEVER presets[0] when a target is selected
if (p) { draft = { ...p }; draftBase = JSON.stringify(p); }
else if (t !== null) {
  const tv = targets.get(t);
  draft = profileDraftForTarget(t, tv ? { backend: tv.backend, addr: tv.addr } : undefined, newId('p'));
  draftBase = '';
} else {
  draft = { id: newId('p'), ecu: 'ecu', label: '새 프로필', sim: false, host: '', port: 13400, la: '0x1001', testerLa: '0x0E80', activation: 0, timeoutMs: 5000 };
  draftBase = '';
}
cfgOpen = true; buildCfg(); uiKey = ''; ctx.scheduler.invalidate();
```
Side effects: none on `prefs`/localStorage (saving stays the existing [저장] path). The `del` handler's `openCfg()` now
falls back to a target draft too (intended).

### B1-P3c `renderUi`
- The `!linkAllowsWrite()` branch: `text = linkDownMessage(link, presetOf(cur) !== null);` (`cls = 'warn'` unchanged).
- `mkProfileBtn.hidden = !(cur !== null && presetOf(cur) === null && targets.has(cur));` (anywhere in renderUi
  after `cur`/`tv` are known; visible regardless of link state, since it only edits locally).
- `connectBtn.title` `!pr` branch → `'이 대상의 프로필이 없음 — [이 대상으로 프로필 만들기] 후 [연결]'`.

Known limit (documented in PLAN §5): until B2's connect-replace, `uds.connect` on a dead Down target returns
`existing:true` and does not reconnect. B1 only fixes the guidance and the profile overwrite risk.

---------------------------------------------------------------------------------------------------
> **Test CPU coordination (ROOT, 2026-11-16; COMMON):** every future targeted Vitest run in this module uses
> `--maxWorkers=2` (acceptance commands below amended). The full UI suite is ROOT's final integrated gate only. No
> assertion or timeout is weakened. Rust stays `CARGO_BUILD_JOBS=2`. Already-passed checks are not rerun for the flag.

# BATCH 2 — U8 row provenance · U6 disconnect/forget/replace · U1 TesterPresent job (engine)

**Status: ACCEPTED** (one test precondition fixed by Opus: `uds.session extended` before the 0x2E in `u8_rows_carry_the_route_of_their_generation`; the sim refuses 0x2E in the default session)

## B2.0 Lease, allowlist, acceptance

Lease (function bodies only) in `crates/loom-uds/src/`: `payload.rs`, `session.rs`, `tester_present.rs`, `lanes.rs`,
`manager.rs`, `module.rs`, `handlers.rs`, `cli.rs`. Opus makes no edits there until the lease is returned.
Opus-authored, read-only for the worker:
- Every type, field, const, signature and `params::*` schema; command registration; the link-state pointer enum; help text.
- CLI subcommand definitions; `lib.rs`; the new `attach(.., generation_base)` parameter.
- Tests: `tests/b2_units.rs` (14), `tests/b2_e2e.rs` (11), `tests/b2_cli.rs` (3). All existing tests stay unchanged and must still pass.

Body allowlist (each `unimplemented!("UDS-B2-…")` stub must be replaced; nothing else may change):

| ID | Where | What |
|---|---|---|
| B2-F1 | payload.rs `RowPayload::with_route` | stub |
| B2-S1..S7 | session.rs `with_generation_base`, `link_closed`, `tp_start`, `tp_stop`, `tp_owner`/`tp_interval`/`last_activity`, `route_stamp`, `mark_forgotten` | stubs |
| B2-S8 | session.rs `state_payload` | fill `suppress`, `owner`, `tp_stop_cause`, `forgotten` (Opus put `None`/`false` placeholders) |
| B2-S9 | session.rs `link_down`, `reset_to_assumed_default` | the one `self.tester_present = None;` line in each |
| B2-T1..T5 | tester_present.rs `TpConfig::pdu`, `parse_tp_params`, `tp_decide`, `tp_exchange_outcome`, `TpCancel::{cancel,is_cancelled,cancelled}` | stubs (remove the `let _ = …` placeholder lines) |
| B2-L1/L2 | lanes.rs `try_acquire`, `is_busy` | stubs |
| B2-M1..M7 | manager.rs `PresendState::remove`, `Target::{stop_tp, stop_tp_job, tp_on_lease_change, is_closing, is_replaceable, retire, tp_exchange}`, `tp_loop` | stubs |
| B2-M8 | manager.rs `attach` | use `generation_base` (remove the `let _ = generation_base;` line); store the watcher handle |
| B2-M9..M11 | manager.rs `UdsManager::{disconnect, forget, tester_present}` | stubs |
| B2-M12 | manager.rs `finish_row` | turn into a free `fn finish_row(pdu: &[u8], res: &TxnResult, row: &mut RowPayload)` (same body minus `let _ = target;`) and update its call site |
| B2-M13 | manager.rs `watch_link` | closing check + TP stop |
| B2-M14 | manager.rs `UdsManager::request` | closing check, route stamp, `tp_on_lease_change()` after the two lease writes |
| B2-M15 | manager.rs `lease_takeover`, `stop_latch` | one call each |
| B2-M16 | manager.rs `connect`, `connect_link` | the "existing target" branch (replace / reconnecting) |
| B2-R1 | module.rs `StateReducer::apply` | forgotten removal |
| B2-H1..H3 | handlers.rs `disconnect`, `forget`, `tester_present` | closure bodies |
| B2-C1/C2 | cli.rs `to_call` (new arms `disconnect`, `forget`, `tp`), `render_data` (new arms `uds.disconnect`, `uds.forget`, `uds.tester_present`) | match arms |

Acceptance (worktree root):
```
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-uds --no-fail-fast            # all binaries green (baseline: 31 existing pass; 28 new B2 fail)
cargo clippy -p loom-uds --tests -- -D warnings  # clean (no unimplemented!/todo!/unwrap in product code)
cargo test -p loom-uds --test fixtures           # unchanged fixtures (new fields are skip-if-none)
```
No new dependency. No change to `crates/loom-uds/Cargo.toml` in B2.

Cross-cutting rules: short std `Mutex` locks are never held across `.await`. All publishing goes through the existing
`Target::publish(ctx, …)` / `publish_bg`. Origins are rendered with `origin_str(o)` =
`serde_json::to_value(o)` as a string (`"agent"`, `"user"`, `"script"`), written inline where needed.

---------------------------------------------------------------------------------------------------
## Payload / session (pure)

**B2-F1 `RowPayload::with_route(self, route) -> Self`**: `generation = Some(route.generation)`, `backend = route.backend.clone()`,
`addr = route.addr.clone()`. `purpose` is untouched.

**B2-S1 `with_generation_base(self, base)`**: `self.generation = base`. Then `link_up` gives `base+1` with cause
`"reconnected"` when base > 0 (existing rule: cause is `connected` only for generation 0).

**B2-S2 `link_closed(&mut self, cause)`**:
1. `link = Closed`, `link_cause = Some(cause)`.
2. session and security confidence → `Stale`.
3. `lease = None`.
4. `self.tp_stop("link closed")` (result ignored).
5. Generation unchanged.

**B2-S3 `tp_start(interval, suppress, owner)`**: `tester_present = Some(interval)`, `tp_ok = 0`, `tp_suppress = Some(suppress)`,
`tp_owner = Some(owner)`, `tp_stop_cause = None`.

**B2-S4 `tp_stop(cause) -> bool`**:
- `tester_present.is_none()` → return `false` and change NOTHING (the cause is kept).
- Else `tester_present = None`, `tp_suppress = None`, `tp_owner = None`, `tp_stop_cause = Some(cause)`, return `true`.

**B2-S5**: `tp_owner()` → `self.tp_owner.as_ref()`, `tp_interval()` → `self.tester_present`, `last_activity()` → `self.last_activity`.

**B2-S6 `route_stamp()`**: `RouteStamp { generation, backend: backend.clone(), addr: addr.clone() }`.

**B2-S7 `mark_forgotten()`**: `forgotten = true`. If link is not Closed: `link = Closed`, `link_cause = Some("forgotten")`.

**B2-S8 `state_payload`**: `TesterPresentPayload.suppress = self.tp_suppress`, `.owner = self.tp_owner.map(LeasePayload{..})`
(same mapping as `lease`), `tp_stop_cause = self.tp_stop_cause.clone()`, `forgotten = self.forgotten`.
Legacy `set_tester_present` (fixtures) leaves suppress/owner `None`, so the fixtures stay byte-identical.

**B2-S9**:
- `link_down(cause)`: replace `self.tester_present = None;` with `self.tp_stop(format!("link down ({cause})"))`. Build the
  string before `cause` is moved into `link_cause`, and keep everything else.
- `reset_to_assumed_default`: replace the same line with `self.tp_stop("session reset to default");` (ECU reset / suppressed reset).

## TesterPresent pure (tester_present.rs)

**B2-T1 `pdu()`**: `vec![0x3E, if suppress {0x80} else {0x00}]`.

**B2-T2 `parse_tp_params(p)`**:
- `on` not a bool → `Err("tester_present needs on: true|false")`. `on:false` → `Ok(Off)` (everything else ignored).
- `on:true`: `interval_ms` must be a JSON integer in `[TP_MIN_INTERVAL_MS, TP_MAX_INTERVAL_MS]`. Missing →
  `Err("tester_present on needs interval_ms (500–30000 ms, below the ECU's S3)")`. Wrong type or range →
  `Err(format!("interval_ms must be an integer 500–30000 ms, got {v}"))`.
- `suppress`: absent/null → true; bool → value; else `Err("suppress must be true|false")`.

**B2-T3 `tp_decide(now, last, interval, busy)`**:
- `due = last.map_or(now, |t| t.saturating_add(interval))`.
- `now < due` → `Wait(due - now)`. Else `busy` → `Busy`. Else `Send`.

**B2-T4 `tp_exchange_outcome(res)`**, by `res.status`:
- `Positive | OkSuppressed` → `Ok(())`.
- `Negative{nrc}` → `"NRC 0x%02X"` + (`" {name}"` if `nrc_name(nrc)`).
- `Timeout{..}` → `"timeout"`. `LinkLost` → `"link lost"`. `GenerationChanged` → `"generation changed"`.
- `TransportNack{code}` → `"transport NACK 0x%02X"`. `Rejected(r)` → `format!("not sent: {r:?}")`.
  `Aborted{kind}` → `format!("aborted ({kind:?})")`.

**B2-T5 `TpCancel`**:
- `cancel`: `flag.store(true, SeqCst); notify.notify_waiters();`
- `is_cancelled`: `flag.load(SeqCst)`.
- `cancelled`: `loop { let n = self.notify.notified(); tokio::pin!(n); n.as_mut().enable(); if self.is_cancelled() { return } n.await; }`
  This cannot miss a wake-up between the check and the await.

## Lanes

**B2-L1 `try_acquire`**: under the queue lock: if `busy || !waiters.is_empty()` → `None`. Else `busy = true` and
`Some(Permit{gate:self})`. lane/origin/request_id are unused (documented for symmetry).

**B2-L2 `is_busy`**: `lock(&self.q).busy`.

## Manager (manager.rs)

**B2-M1 `PresendState::remove(target)`**: `lock(&self.targets).remove(target);`.

**B2-M2 `Target::stop_tp(cause) -> bool`** (sync):
1. `job = lock(&self.tp).take()`. If None → return false.
2. `job.cancel.cancel()`. Drop the `JoinHandle` (the task ends by itself after at most one in-flight exchange).
3. `changed = lock(&self.session).tp_stop(cause)`. If changed → `self.publish_bg(self.state_draft())`.
4. Return true.

**B2-M2b `stop_tp_job(cancel, cause) -> bool`**: as M2, but only when the slot holds a job with `Arc::ptr_eq(&job.cancel, cancel)`.
Otherwise return false and touch nothing.

**B2-M3 `tp_on_lease_change()`**: `owner = slot job's owner.client_id` (None → return). If the session lease is
`Some(l)` and `l.client_id != owner` → `self.stop_tp(&format!("lease taken by {}", l.client_id))`.

**B2-M4**:
- `is_closing()` = `closing.load(SeqCst)`.
- `is_replaceable()` = `is_closing() || link == Closed || (link == Down && watcher finished)`, where "watcher finished"
  means `lock(&self.watch).as_ref().is_none_or(JoinHandle::is_finished)`.

**B2-M5 `retire()`** (idempotent):
1. `closing.store(true, SeqCst)`.
2. `self.stop_tp("target closed")`.
3. Take and `abort()` the watcher handle.
4. `let _permit = self.lanes.acquire(Lane::Control, Origin::Script, "retire").await` (an `Err` still proceeds).
5. `let mut gate = self.gate.lock().await`. Drop `ctl_handle.take()` and `gate.handle.take()`. If `gate.task.take()` is Some →
   `let _ = tokio::time::timeout(DISCONNECT_DRAIN_TIMEOUT, task).await;`
6. `drop(gate.link.take())`. That closes the DoIP TCP; the sim entity lives until the Target is dropped.

**B2-M6 `tp_exchange(ctx, txn, row) -> TxnResult`**: the same gate/runner mechanics as `UdsManager::request` lines
"let mut gate … drain after", with these differences:
- Events: `Sent{mono}` → `row.write_state = Sent; row.t_send_mono_ns = Some(U64(mono))`. `Pending{count,..}` →
  `row.pending_count = count`. `Late{..}` → `self.publish_late(ctx, …)`. `Released` → clear `timed_out` + presend
  quarantine. Never publish tx/pending/rx for keep-alive.
- No handle → `TxnResult::rejected(&txn, RejectReason::RunnerStopped, &MonoClock::new(self.clock.at, self.clock.base_ns), 0)`.
- Submit under `lock(&self.stopped)`: if latched → do not submit; return `TxnResult::aborted(&probe, AbortKind::Stop, &clock, 0)`.
- The receiver loop is the same `tokio::select!{ biased; … }` as `request`.

**B2-M7 `tp_loop(target, ctx, cfg, owner, generation, cancel)`**. Counters `n: u64 = 0`, `fails: u32 = 0`. Loop:
1. `if cancel.is_cancelled() { return }`.
2. Snapshot under one session lock: `gen`, `link`, `tp_owner client`, `last_activity`.
   - `gen != generation` → `stop_tp_job(&cancel, "generation changed")`, return.
   - `link != Up` → `stop_tp_job(&cancel, "link down")`, return.
   - `tp_owner` missing or a different client → `stop_tp_job(&cancel, "owner changed")`, return.
3. `match tp_decide(target.now(), last, cfg.interval, target.lanes.is_busy())`:
   - `Wait(d)` → `select!{ sleep(d), cancel.cancelled() → return }`, continue.
   - `Busy` → same with `TP_BUSY_RECHECK`.
   - `Send` → go on.
4. `let Some(permit) = target.lanes.try_acquire(Lane::Agent, owner.owner_origin, "tp")` else → treat as `Busy`.
   `if cancel.is_cancelled() { return }`.
   `n += 1; rid = format!("tp-{}-{}-{}", target.name, generation, n)`. `tp_in_flight.store(true)`.
5. Build the row:
   - `pdu = cfg.pdu()`; `(timing, stamp)` from one session lock.
   - `row = RowPayload::request(&name, &pdu, false, WriteState::Requested, timing).with_route(&stamp)`.
   - `row.purpose = Some(PURPOSE_TESTER_PRESENT.into())`.
6. Send:
   - `txn = TxnRequest::new(pdu.clone(), Access::Read, cfg.interval.min(TP_EXCHANGE_TIMEOUT)).with_priority(Priority::Agent).with_origin(owner.owner_origin).with_request_id(rid.clone())`.
   - `res = target.tp_exchange(&ctx, txn, &mut row).await`.
   - `tp_in_flight.store(false)`; `drop(permit)`.
7. `finish_row(&pdu, &res, &mut row)`. Same quarantine bookkeeping as `request` (timeout, or sent + LinkLost/GenerationChanged
   → push `(0x3E, rid)` and insert SID 0x3E into presend quarantine). Then
   `target.publish(&ctx, events::row(KIND_RESULT, &rid, stamp.generation, target.env, &row))`.
8. Outcome:
   - `Ok` → `fails = 0; session.on_tester_present_ok(target.now())`.
   - `Err(e)` → `session.on_sent(target.now())`.
     - If `res.status == Aborted{kind: Stop}` → `stop_tp_job(&cancel, "STOP")`, return.
     - Else `fails += 1`; if `fails >= TP_MAX_CONSECUTIVE_FAILURES` →
       `stop_tp_job(&cancel, &format!("{fails} consecutive failures: {e}"))`, return.

**B2-M8 `attach`**: `open_link(connector, u64::from(generation_base) + 1)`,
`TargetSession::new(name).with_generation_base(generation_base)`, then the existing flow. After inserting the
target: `*lock(&target.watch) = Some(tokio::spawn(watch_link(target.clone(), link_events)))` (replaces the bare spawn).

**B2-M9 `disconnect(ctx, name, abort)`**:
1. `t = self.target(name)?` (missing → existing `E_NOT_OPEN`).
2. Lease: if the session lease is held by `Origin::User` and `ctx.origin() != User` → `E_HELD_BY_USER` "target is held by the user".
3. If `t.is_closing()` or link == Closed → `Ok({"target","state":t.state(),"already_closed":true})`.
4. `waiting = (a,u,x,c)` sum of `t.lanes.depth()`. `busy = waiting > 0 || (t.lanes.is_busy() && !t.tp_in_flight)`.
   If `busy && !abort` → `E_BUSY` "target has requests in progress; pass abort:true to abort them and close",
   detail `{"target", "waiting": waiting, "in_flight": t.lanes.is_busy()}`. Nothing changes.
5. If abort:
   - `cw = t.lanes.cancel(AbortKind::User, |_,_| true, false)`.
   - `s = t.ctl().map(|h| h.abort(AbortScope::All))`.
   - `aborted = {"cancelled_waiting": cw + s.cancelled_waiting, "in_flight": s.in_flight}`.
   - Else `aborted = {"cancelled_waiting":0,"in_flight":false}`.
6. `t.retire().await`.
7. `lock(session).link_closed(format!("disconnected by {}", origin_str(ctx.origin())))`. Then `t.sync_presend()` and
   `t.publish(ctx, t.state_draft())`.
8. Return `{"target", "state", "aborted", "others_since_last": self.others_since_last(ctx.client_id())}`.

**B2-M10 `forget(ctx, name)`** (async):
1. `target(name)?`.
2. Same lease rule as M9.
3. `!t.is_replaceable()` → `E_BUSY` "target {name} is not closed (uds.disconnect first)".
4. `t.retire().await`. Remove from `targets` only if the map still holds `Arc::ptr_eq` of `t`. `self.presend.remove(name)`.
5. `g = t.generation()`. `lock(session).mark_forgotten()`. `t.publish(ctx, t.state_draft())`.
6. Return `{"target": name, "forgotten": true, "last_generation": g}`.

**B2-M11 `tester_present(ctx, name, req)`**:
1. `t = self.resolve(name)?`.
2. `Off` → `was = t.stop_tp(&format!("stopped by {}", origin_str))`. Return `{"target","state","was_active":was}`.
   Anyone may stop it (reduce direction).
3. `On(cfg)`, checked in this order:
   - `self.stopped()` → `E_STOPPED` "STOP is latched: TesterPresent not started".
   - `t.is_closing()` or link != Up → `E_NOT_OPEN` "target {name} link is not up".
   - User lease and caller not User → `E_HELD_BY_USER`.
4. Start:
   - `owner = Lease{owner_origin: ctx.origin(), actor_id: client, client_id: client}`.
   - `t.stop_tp("replaced by a new TesterPresent job")`.
   - `generation = t.generation()`; `cancel = Arc::new(TpCancel::default())`.
   - `lock(session).tp_start(cfg.interval, cfg.suppress, owner.clone())`.
   - `task = tokio::spawn(tp_loop(t.clone(), ctx.clone(), cfg, owner.clone(), generation, cancel.clone()))`.
   - `*lock(&t.tp) = Some(TpJob{..})`.
   - `t.publish(ctx, t.state_draft())`.
   - Return `{"target","state"}`.

**B2-M12** free `finish_row` (mechanical; body unchanged).

**B2-M13 `watch_link`**:
- After `target.on_link_down(&reason)`: `target.stop_tp("link down");`. The session already recorded the precise cause,
  so this only clears the slot and normally publishes nothing.
- Then `if target.is_closing() || !target.auto_reconnect { return; }`.
- After a successful `reconnect()`, `if target.is_closing() { return; }`.

**B2-M14 `request`**:
1. Right after `let target = self.target(&spec.target)?;`: `if target.is_closing() { return Err(NotOpen,
   format!("UDS target {:?} is closed (uds.connect opens it again)", name)) }`. No row is published.
2. Replace `let timing = lock(&target.session).timing();` with one lock that yields `(timing, stamp)`, then
   `RowPayload::request(..).with_route(&stamp)`.
3. Call `target.tp_on_lease_change()` after the `if changed {…}` block (user path) and after the agent `set_lease` publish.

**B2-M15**:
- `lease_takeover`: `target.tp_on_lease_change()` after `set_lease`.
- `stop_latch`: inside the target loop, before the lane cancel: `t.stop_tp("STOP");`. It stays sync and bounded.

**B2-M16 `connect` / `connect_link`, existing target `t`**:
- `t.is_replaceable()`:
  - `prev = t.generation()`; `t.retire().await`; remove the map entry (ptr_eq).
  - Continue as a new connect with `attach(.., prev)`.
  - Add `"replaced": true, "previous_generation": prev` to the result.
- Else return `{"target","state","existing":true,"reconnecting": link == Down}` (`reconnecting` is new; the rest is unchanged).

**B2-R1 `StateReducer::apply`**: after the `target` extraction:
`if ev.payload.get("forgotten") == Some(&Value::Bool(true)) { self.targets.remove(target); return; }`.

**B2-H1** `disconnect`: `target` (str) required → else usage "disconnect needs target". `abort` = bool default false.
→ `m.disconnect(ctx, target, abort).await`.
**B2-H2** `forget`: `target` required → else usage "forget needs target". → `m.forget(ctx, target).await`.
**B2-H3** `tester_present`: `req = parse_tp_params(&p).map_err(usage)?` → `m.tester_present(ctx, target_arg(&p), req)`.

**B2-C1 `to_call`**:
- `disconnect`: `p = params(s)`; `--abort` → `"abort": true`.
  `RpcCall::new("uds.disconnect", Value::Object(p))` with `wait = Some(Duration::from_secs(40).min(MAX_WAIT))`.
- `forget`: `RpcCall::new("uds.forget", params)`.
- `tp`: `"on": state=="on"`; `--interval-ms` → `interval_ms`; `--no-suppress` → `"suppress": false`.
  `on` without interval → `UsageError::new("tp on needs --interval-ms (500–30000)")`.

**B2-C2 `render_data`** (exact text in tests/b2_cli.rs):
- `uds.disconnect`:
  - Line 1: `"{target}  link {state.link.state}"` + `" (already closed)"` if already_closed, else `" ({cause})"` if a cause exists.
  - If `aborted.cancelled_waiting > 0 || aborted.in_flight`, line 2: `"  aborted: {n} waiting"` +
    `", in-flight request aborted (outcome unknown)"` if in_flight.
- `uds.forget`: `"{target}  forgotten (last generation {g})"`.
- `uds.tester_present`:
  - TP object → `"{target}  TesterPresent on every {interval_ms} ms (3E 80|3E 00)"`.
  - Else `"{target}  TesterPresent off"` + `" ({tp_stop_cause})"` when present.

---------------------------------------------------------------------------------------------------
# BATCH 3 — U5 inline diagnostic DB · U2 security unlock with a bounded key provider (engine)

Status: **ACCEPTED** (B3-A + B3-B; see B3.0).

Opus scaffold already done (outside the B2 lease):
- `src/diagdb.rs`, `src/keyprovider.rs`; `policy.rs` `KeyProviderConfig` + `PolicySection.key_providers`; `lib.rs` mods.
- `Cargo.toml`: tokio `process` feature (workspace tokio; no new crate; Cargo.lock unchanged).
- Test fixtures `tests/fixtures/keyprovider/{xor,env,slow,big,fail,junk}.sh` (0755) and `tests/b3_units.rs` (9 tests;
  the process tests are `cfg(unix)`. ROOT note: on Windows hosts the provider is an .exe/.cmd; no shell is used).

## B3-A module contracts

**B3-D1 `parse_inline_db(v) -> Result<LoadedDb, String>`**:
1. `bytes = serde_json::to_vec(v)` (infallible for `Value`; on error use the message).
2. `bytes.len() > MAX_INLINE_DB_BYTES` → `Err(format!("diagnostic DB is {} bytes; the inline limit is {MAX_INLINE_DB_BYTES}", len))`.
   This is checked BEFORE `load_db`.
3. `load_db(&bytes, Format::Json)`. On Err(e) → `format!("invalid loom_diag DB: {e}")` truncated to `MAX_DB_ERROR_LEN`
   chars (char boundary safe).

**B3-D2 `db_info`**: `DbInfo{hash: db.hash_hex(), ecu: db.db.ecu.clone(), dids: db.db.dids.len() as u32}`
(use `u32::try_from(..).unwrap_or(u32::MAX)`).

**B3-D3 `db_warnings`**:
- In file order: for each def with `standard_did(def.id) = Some(std)` → `format!("DID {:04X} overrides the ISO table entry {}", def.id, std.name)`.
- Then, if any def is `writable`, exactly once:
  `"writable flags are informational: write risk is decided from the bytes and never lowered by a DB"`.

**B3-D4 `decode_dids_with(lookup, pdu, resp)`**: the body of today's `manager::decode_dids` with `lookup` in place of
`StandardDids` in BOTH places (`parse_read_did_response(pdu, resp, lookup)` and `lookup.did_def(r.did)`). `None` when
the parse fails. B3-B makes `manager::decode_dids` delegate to it.

**B3-K1 `ProviderError::{code,message}`**: exact table in tests/b3_units.rs `b3_k5_snippet_and_k1_codes`.
- Codes: Timeout → `Timeout`; OutputTooLarge → `Limit`; everything else → `Config`.
- `LevelNotAllowed` lists levels as two-digit upper hex joined by `", "`.
- `Exit`: `code N` or `a signal`, plus `": {stderr}"` only when stderr is non-empty.

**B3-K2 `validate_provider(cfg)`**: first failure wins, `BadConfig(msg)` where msg contains the shown word:
- name not `^[A-Za-z0-9_.-]{1,64}$` → "name …"
- command not `std::path::Path::new(c).is_absolute()` (the HOST OS rule: Unix `/…`; Windows `C:\…`, `C:/…`, `\\server\share\…`;
  NEVER a `starts_with('/')` test), or > `MAX_PROVIDER_COMMAND_LEN`, or contains NUL → "command must be an absolute path …"
- command's extension (`Path::extension`, ASCII case-insensitive) is `bat` or `cmd` →
  "command must be an executable, not a batch script (.bat/.cmd run through cmd.exe)". Checked on every OS (deterministic
  config validation); it keeps the "no shell" rule on Windows, where `CreateProcess` would start cmd.exe implicitly.
- args > `MAX_PROVIDER_ARGS`, or any arg > `MAX_PROVIDER_ARG_LEN` / with NUL → "args …"
- `timeout_ms` Some(0) or > `MAX_PROVIDER_TIMEOUT_MS` → "timeout_ms …"
- levels empty → "levels must not be empty"
- any level even, 0, or > 0x7D → "levels must be odd requestSeed sub-functions 0x01–0x7D"

**B3-K3 `provider_from_config(config, name, level)`**:
1. `config.get("key_providers")`: absent/null → `NotConfigured(name)`; not an array → `BadConfig("key_providers must be an array")`.
2. Each element → `serde_json::from_value::<KeyProviderConfig>` (deny_unknown_fields). Error →
   `BadConfig(format!("key_providers[{i}]: {e}"))`. Every element is checked, not only the matching one.
3. Find by `name` → `NotConfigured(name)` if missing.
4. `validate_provider`. Then `!levels.contains(&level)` → `LevelNotAllowed{level, allowed: levels.clone()}`.

**B3-K4 `parse_key_output(stdout)`**:
1. `len > MAX_PROVIDER_OUTPUT_BYTES` → `OutputTooLarge`.
2. Not UTF-8 → `BadKey("not UTF-8")`.
3. `t = s.trim()`. Empty → `BadKey("empty output")`. Any whitespace inside t → `BadKey("expected one hex line")`.
4. Odd length, non-hex, or `t.len() > MAX_KEY_HEX_CHARS` → `BadKey("key must be 2–1024 hex digits")`. Else decode.

**B3-K5 `sanitize_snippet(bytes)`**: `String::from_utf8_lossy`, `trim_end()`, map every `char::is_control` to `'?'`,
keep the first `MAX_ERROR_SNIPPET` chars.

**B3-K6 `run_provider(cfg, level, seed)`** (bounded; no retry; no shell):
1. `validate_provider(cfg)?`; `level ∉ cfg.levels` → `LevelNotAllowed`.
2. `ms = cfg.timeout_ms.unwrap_or(DEFAULT_PROVIDER_TIMEOUT_MS)`.
3. Spawn:
   - `tokio::process::Command::new(&cfg.command).args(&cfg.args).env_clear().env("LOOM_UDS_LEVEL", format!("{level:02X}"))`
   - Windows only (`#[cfg(windows)]` block, the one platform branch): if `std::env::var_os("SystemRoot")` is Some, pass that
     one variable through (Windows system DLLs such as Winsock fail to initialise without it). Nothing else is inherited on
     any OS.
   - `.stdin(piped).stdout(piped).stderr(piped).kill_on_drop(true).spawn()`.
   - Error → `Spawn(sanitize_snippet(e.to_string().as_bytes()))`.
4. Inside ONE `tokio::time::timeout(ms)` around steps 4–5:
   - `tokio::join!` of: write `hex::encode_upper(seed) + "\n"` to stdin and drop it (write errors ignored: provider exited
     early); read stdout via `take(MAX_PROVIDER_OUTPUT_BYTES as u64 + 1)` to end; read stderr via
     `take(MAX_PROVIDER_OUTPUT_BYTES as u64)` to end.
   - stdout longer than the cap → `child.start_kill()` and return `OutputTooLarge` at once (do NOT wait for the exit:
     a provider that keeps writing would otherwise turn into a timeout).
   - Then `child.wait()`.
5. Outcomes:
   - Timeout → `child.start_kill()` and return `Timeout{ms}`.
   - Non-success exit → `Exit{code: status.code(), stderr: sanitize_snippet(&stderr)}`.
   - Else → `parse_key_output(&stdout)`.
6. Dropping the future kills the child (`kill_on_drop`). Never logs or publishes the key or provider output.
7. Portability: only std/tokio process APIs (no `/bin/sh`, no `std::os::unix`, no signals by name). The kill is the
   platform's (`SIGKILL` / `TerminateProcess`) for the DIRECT child only; descendants a provider starts are not tracked
   on any OS (documented in SKILL.md: the provider must not leave children running).

## B3-B wiring (scaffolded; B3 = B3-A + B3-B, one lease)

**Status: ACCEPTED** (Opus removed the 2 imports made dead by B3-M2: `parse_read_did_response`, `DecodedDid`).

### B3.0 Lease, allowlist, acceptance

Lease (function bodies only, `crates/loom-uds/src/`): `diagdb.rs`, `keyprovider.rs`, `text.rs`, `hooks.rs`, `manager.rs`,
`handlers.rs`, `cli.rs`. Opus-owned (read-only): every type/field/const/signature/import, `params::*` schemas, the
registration, `risk.rs` (`typed(0x27 Even, "uds.security_unlock")` is done), CLI subcommand definitions, `payload.rs`
(`RowPayload.db_hash`, `StatePayload.db`), `session.rs` (`db: None` placeholder stays: the manager fills it), `policy.rs`,
`lib.rs`, `Cargo.toml`, all tests and fixtures.

Spec tests:
- `tests/b3_units.rs` (11): 10 platform-independent, 2 `cfg(unix)` (sh fixtures), 1 `cfg(windows)`.
- `tests/b3_e2e.rs` (8): 6 platform-independent, 2 `cfg(unix)`.
- `tests/b3_cli.rs` (4): all platform-independent.

| ID | Where | What |
|---|---|---|
| B3-D1..D4 | diagdb.rs | B3-A above |
| B3-D5 | diagdb.rs `AttachedDb::new` | `AttachedDb { info: db_info(&db), db }` |
| B3-K1..K6 | keyprovider.rs | B3-A above (incl. the platform rules in K2/K6) |
| B3-X1 | text.rs `encode_typed` | new arm `"uds.security_unlock"` |
| B3-X2 | hooks.rs `approval_binding` | provider digest/display for that method |
| B3-M1 | manager.rs `Target::db_snapshot` | `lock(&self.db).clone()` |
| B3-M2 | manager.rs `finish_row` (+ `decode_dids`) | decode with `lookup`; delete the now-unused `manager::decode_dids` |
| B3-M3 | manager.rs `Target::state` | fill `db` (`state_draft` already goes through `state()`) |
| B3-M4 | manager.rs `UdsManager::request` | DB snapshot → `db_hash` stamp + `finish_row(.., lookup)` |
| B3-M5..M7 | manager.rs `UdsManager::{security_unlock, db_set, db_clear}` | stubs |
| B3-H1..H3 | handlers.rs `security_unlock`, `db_set`, `db_clear` | closure bodies |
| B3-C1..C3 | cli.rs `to_call` arms `unlock`, `db`; `render_data` arms; `read_db_file` | arms + stub |

Acceptance (worktree root):
```
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-uds --no-fail-fast            # all green (baseline: 59 existing pass; 22 B3 fail on stubs)
cargo clippy -p loom-uds --tests -- -D warnings  # clean (scaffold's unused-import warnings disappear with the bodies)
cargo test -p loom-uds --test fixtures           # unchanged (new fields are skip-if-none)
```
No new dependency. No `Cargo.toml` change. Product code uses only std/tokio process APIs; the only platform branch is K6's
`#[cfg(windows)]` SystemRoot pass-through. No `std::os::unix`, no `/bin/sh`.

### Risk / encode / approval

**B3-X1 `encode_typed("uds.security_unlock", args)`**:
- `level = args["level"]` must be a JSON integer. Absent or not an integer → `Err("security_unlock needs level")`.
- Not odd, or not in `0x01..=0x7D` → `Err("level must be an odd requestSeed sub-function 0x01–0x7D")`.
- Else `Ok(vec![0x27, level + 1])`. This is the R3 sendKey PDU that classification and approval see. `provider` is not
  checked here.

**B3-X2 `approval_binding`**: only when `req.method == "uds.security_unlock"`:
- `digest_fields.insert("provider", json!(args["provider"].as_str()))`.
- `display["provider"]` = the same value.
- Other methods are unchanged.

### Manager

**B3-M1** `db_snapshot()`: `lock(&self.db).clone()`.

**B3-M2** `finish_row(pdu, res, row, lookup)`:
- Remove the placeholder line and use `crate::diagdb::decode_dids_with(lookup, pdu, resp)` (imported as
  `decode_dids_with`) where it called `decode_dids(pdu, resp)`.
- Delete `fn decode_dids` (dead code).
- `tp_loop` keeps `&StandardDids`: a TesterPresent response has no DIDs, and no `db_hash` is stamped on TP rows.

**B3-M3** `Target::state()`: `let mut p = lock(&self.session).state_payload(); p.db = self.db_snapshot().map(|d| d.info.clone()); p`
(take the session lock and the db lock one after the other, never nested). `state_draft` is unchanged: it calls `state()`.

**B3-M4** `request`: take the snapshot once, in the same place as `(timing, stamp)`:
- `let db = target.db_snapshot();`
- `row.db_hash = db.as_ref().map(|d| d.info.hash.clone())`, set right after `.with_route(&stamp)`.
- `finish_row(&spec.pdu, &res, &mut row, db.as_ref().map_or(&StandardDids as &dyn DidLookup, |d| &d.db))`.
The same snapshot decodes and stamps the row, so a concurrent `db.set` never splits them.

**B3-M5 `security_unlock(ctx, name, level, provider, timeout)`** (no retry anywhere):
1. `t = self.resolve(name)?`.
2. `cfg = provider_from_config(&ctx.config(), provider, level)`. On `Err(e)` → `DomainError::new(e.code(), e.message())`.
   No row is created and no byte is sent.
3. Request ids:
   - `rid = ctx.request_id().map(str::to_owned).unwrap_or_else(|| self.next_request_id())`.
   - `seed_rid = format!("{rid}.seed")`.
4. Seed:
   - `out = self.request(ctx, ReqSpec{target: t.name().into(), pdu: vec![0x27, level], timeout, request_id: Some(seed_rid.clone()), recovery: false}).await?`.
   - If `out.result` is not `TxnStatus::Positive` → return `out.into_result()`. If that is `Ok` (cannot happen for odd
     levels), return `E_INTERNAL "unexpected seed response"` instead.
   - `resp = out.result.response`. It must be `[0x67, level, seed @ ..]` with a non-empty seed. Otherwise →
     `E_INTERNAL "unexpected seed response"` with detail `{"seed_request_id"}`.
5. If every seed byte is 0 → `Ok({"target": t.name(), "level": level, "unlocked": true, "already_unlocked": true,
   "seed_request_id", "others_since_last": self.others_since_last(ctx.client_id())})`. No provider runs and no key is
   sent. The gateway then closes the call's own row as `confirmed`.
6. If `stoppable(ctx.origin()) && self.stopped()` → `E_STOPPED "STOP is latched: key provider not started, no key sent"`
   with detail `{"seed_request_id"}`.
7. `key = run_provider(&cfg, level, seed).await`. On `Err(e)` → `DomainError::new(e.code(), e.message())` with detail
   `{"seed_request_id"}`. The key and the provider output are never logged.
8. Key = the call's own request. Build the PDU as `[0x27, level + 1] ++ key`, then
   `out = self.request(ctx, ReqSpec{.., pdu, timeout, request_id: if ctx.request_id().is_some() { None } else { Some(rid) }, ..}).await?`.
   `None` means the gateway's own row for the call carries the R3 sendKey.
   `request()` applies STOP, the lease and quarantine as usual.
9. `unlocked = matches!(out.result status, Positive)`. Then `let mut d = out.into_result()?;` (an NRC such as 0x35/0x36 is
   returned as is; never retried) and add `"target", "level", "unlocked", "already_unlocked": false, "seed_request_id"`.
   Return `Ok(d)`.

**B3-M6 `db_set(ctx, name, db)`**:
1. `t = self.target(name)?`.
2. Lease: user-held and caller not User → `E_HELD_BY_USER "target is held by the user"`.
3. `t.is_closing()` or link `Closed` → `E_NOT_OPEN format!("target {name} is closed (uds.connect opens it again)")`.
4. `loaded = parse_inline_db(db).map_err(|m| E_USAGE m)?` (message as is).
5. `warnings = db_warnings(&loaded)`, `att = Arc::new(AttachedDb::new(loaded))`, `info = att.info.clone()`.
6. `replaced = lock(&t.db).replace(att).is_some()`.
7. `t.publish(ctx, t.state_draft())`.
8. Return `{"target", "db": info, "warnings", "replaced", "state": t.state()}`.

**B3-M7 `db_clear(ctx, name)`**:
1. `target(name)?`.
2. Same lease rule as M6 (a closed target may be cleared).
3. `cleared = lock(&t.db).take().is_some()`.
4. If cleared, publish the state.
5. Return `{"target", "cleared", "state"}`.

### Handlers

**B3-H1** `security_unlock`:
- `level`: a JSON integer in `0..=255`, else usage "security_unlock needs level". Then
  `encode_typed("uds.security_unlock", &p).map_err(usage)?` checks odd/range.
- `provider`: a non-empty string, else usage "security_unlock needs provider".
- `m.security_unlock(ctx, target_arg(&p), level, provider, timeout_arg(&p)).await`.

**B3-H2** `db_set`:
- `target` (str) is required → else usage "db.set needs target".
- `db` must be present and non-null → else usage "db.set needs db".
- `m.db_set(ctx, target, db)`.

**B3-H3** `db_clear`: `target` is required → else usage "db.clear needs target". Then `m.db_clear(ctx, target)`.

### CLI

**B3-C1 `to_call`**:
- `unlock`:
  - `p.level = <u8>` and `p.provider`.
  - `check("uds.security_unlock", &p)?`, so an even level or one > 0x7D is a usage error with the encoder's message.
  - `RpcCall::new("uds.security_unlock", p)` with `wait = Some(Duration::from_secs(45).min(MAX_WAIT))`.
- `db` → `set`:
  - `p = params(l)` plus `"db"`: the parsed `Value` from `--file` (the value parser already read it).
  - Gives `uds.db.set`.
- `db` → `clear`: `uds.db.clear`.
- Any other `db` subcommand → `UsageError::new("expected `db set` or `db clear`")`.

**B3-C2 `render_data`** (exact text in tests/b3_cli.rs):
- `uds.security_unlock`:
  - If `d["row"]` is an object → `render_row(d)`.
  - Then if `already_unlocked`: `"{target}  security level {LL} already unlocked (zero seed; no key sent)"`.
  - Else: `"{target}  security level {LL} unlocked (seed {seed_request_id})"`.
  - `LL` = `{:02X}`.
- `uds.db.set`:
  - `"{target}  DB {hash} attached ({ecu or -}, {n} DIDs{; replaced the previous DB})"`.
  - Then one line `"  warning: {w}"` per warning.
- `uds.db.clear`: `"{target}  DB cleared (ISO DID table only)"` or `"{target}  no DB attached"`.

**B3-C3 `read_db_file(path)`**:
1. `std::fs::metadata(path)`. Error → `Err(format!("cannot read {path}: {e}"))`.
2. `len > MAX_DB_FILE_BYTES` → `Err(format!("{path} is {len} bytes; the DB limit is {MAX_DB_FILE_BYTES}"))`.
3. Read through `File::open(..).take(MAX_DB_FILE_BYTES as u64 + 1)` (the file may grow). Over the limit → same message.
4. `serde_json::from_slice` → `Err(format!("{path}: not JSON (line {}, column {})", e.line(), e.column()))`. Never echo
   the content.

### Docs (Opus)

`skills/loom-uds/SKILL.md` gets TP / disconnect / forget / DB / unlock. Platform claims are in the STATUS report.

---------------------------------------------------------------------------------------------------
# BATCH 4 — UI for U8/U6/U1/U5/U2

Status: **ACCEPTED** (was READY_B4: pure + DOM scaffolded; `udsBatch4.test.ts` 15 + `udsBatch4Dom.test.ts` 14 failing on stubs, plus the updated `udsWorkbench` connect test red until B4-D7;
89 other existing pass; isolated tsc clean). Assigned after B3 acceptance (one worker, one lease at a time).

## B4 pure contracts (udsModel.ts / udsPrefs.ts)

**B4-F1 `rowRoute(p)`**:
- `p.generation` is an integer ≥ 0 → `known: true`, `text: `${s(p.backend) ?? '?'}${s(p.addr) ? ' ' + addr : ''} · g${gen}``,
  `title: `요청 시점에 daemon이 기록한 경로 (generation ${gen})``.
- Anything else → `{text: '?', title: ROUTE_UNRECORDED_TITLE, known: false, generation: null}`. Never looks at current target state.

**B4-F2 `isForgotten(payload)`**: `obj(payload).forgotten === true`.

**B4-F6 `reduceTargetState`** (replace the placeholders):
- `generation = n(p.generation) ?? prev?.generation ?? null`.
- A canonical state is one that has the `tester_present` key. When it does:
  - `tpIntervalMs = tpOn ? n(tp.interval_ms) : null`.
  - `tpSuppress = tpOn && typeof tp.suppress === 'boolean' ? tp.suppress : null`.
  - `tpOwner = tpOn ? s(obj(tp.owner).owner_origin) : null`.
  - `tpStopCause = s(p.tp_stop_cause)` (absent → null).
  - DB: `'db' in p && p.db` object → `dbHash = s(db.hash)`, `dbEcu = s(db.ecu)`, `dbDids = n(db.dids)`. Otherwise all three null.
- Without the key (legacy/partial): all of these keep `prev` values (or null).

**B4-F3a `tpOkCount(t, folded)`**: `t.tpSuppress !== null ? Math.max(t.tpOk, folded) : t.tpOk + folded`.

**B4-F3 `tpCellText(t, folded)`**:
- `t.tpOn` and `tpIntervalMs !== null` → `` `on ${(ms/1000).toFixed(1)}s ✓×${tpOkCount}` ``. `t.tpOn` otherwise → `` `on ✓×${tpOkCount}` ``.
- Off with a stop cause → `` `off (${cause})` `` + (`folded ? ` ×${folded}` : ''`). Off with folded > 0 → `` `off ×${folded}` ``. Else `'off'`.

**B4-F3b** (bodies of existing `targetCells` / `targetLine`): `testerPresent: tpCellText(t, tpFolded)`. In targetLine
the TP part becomes `t.tpOn ? ` TP ✓×${tpOkCount(t, tpFolded)}` : tpFolded ? ` TP ×${tpFolded}` : ' TP off'`. Correction at B4 review: one legacy assertion
(`uds.test.ts` "one row per target…", fixture `state.json` extended, which reports `interval_ms: 2000`) predates U1. Opus
updated it to `'on 2.0s ✓×1'`: U1 requires showing the reported interval. The worker's BLOCKED_ON_LEAD was correct.

**B4-F4 `seedFromRow(r)`**:
- Only when `isFinal(r.state) && r.state !== 'failed'`. `req = reqHexOf(r)` must match `^27([0-9A-F]{2})$` with an odd level in 0x01–0x7D.
- `resp = respHexOf(r)` must start with `67` + the same level and have ≥ 1 seed byte.
- Result: `{level, seedHex: resp.slice(4), zero: /^(00)+$/.test(seedHex)}`. Else null.

**B4-F5 `keyTemplate(level)`**: `` `27 ${hh(level + 1)} <key>` `` (upper-case 2-digit hex, & 0xFF).

**B4-P1 `tpIntervalOf(p)`**: `p && Number.isInteger(p.tpIntervalMs) && 500 ≤ v ≤ 30000 ? v : DEFAULT_TP_INTERVAL_MS`.

**B4-P2 `profileNeedsReconnect(p, {backend, addr, link})`**, first match wins:
1. `link !== 'up'` → false.
2. `p.sim !== (backend === 'sim')` → true.
3. `p.sim` → false.
4. `la = laFromAddr(addr)`; null → false (host/port are not reported, so no claim).
5. Otherwise `parseLa(p.la) !== parseInt(la.slice(2), 16)` (an unparsable `p.la` counts as different).

**B4-P3 `checkDbText(text)`**, first failure wins:
1. `bytes = new TextEncoder().encode(text).length`. `> MAX_DB_TEXT_BYTES` → `` `DB가 너무 큼 (${bytes} B, 최대 ${MAX_DB_TEXT_BYTES} B)` ``.
2. JSON.parse error → `` `JSON 구문 오류: ${message}` ``.
3. Not a plain object (array/null/primitive) → `'loom_diag DB 객체가 아님'`.
4. `loom_diag !== 1` → `'loom_diag 1 형식이 아님'`.
5. `dids` present and not an array → `'dids: 배열이 아님'`.
6. Otherwise `{db, err: null, bytes}`. Failures return `db: null`.

**B4-P4 `sanitizePrefs`** (body, udsPrefs.ts), applied to each kept preset as a copy:
- `tpIntervalMs`: kept only if it is an integer in 500–30000.
- `keyProvider`: kept only if it matches `^[A-Za-z0-9_.-]{1,64}$`.
- `unlockLevel`: kept only if it is an odd integer in 1–0x7D.
- An invalid value removes the key. It is never defaulted.
`diagDbs`:
- If it is an array, keep the entries with `id` matching the existing `ID` regex, `name` a trimmed non-empty string of at
  most 80 characters, and `json` a string of at most `MAX_DB_TEXT_BYTES` UTF-8 bytes (`TextEncoder`).
- Then drop repeated `id`s (first one wins) and keep at most `MAX_SAVED_DBS`.
- The `diagDbs` key is present ONLY when at least one entry survives, so `DEFAULT_PREFS` equality tests stay green.

## B4 DOM (consolePanel.ts) — Opus scaffold done; Sonnet bodies

Opus-owned, already in place:
- Imports.
- Elements: `discBtn`/`discLabel`, `forgetBtn`, `reconnNote`, `tpBtn`, `tpIv`, `tpInfo`, `secLevel`, `secProv`,
  `unlockBtn`/`unlockLabel`, `secMsg`, `dbInfo`, `dbText`, `dbFile`, `dbPick`, `dbSaved`, `dbApply`, `dbClearBtn`,
  `dbMsg`, `seedBar`/`seedCode`/`seedCopy`/`seedTpl`.
- State: `pendingOps`, `discArm`, `unlockArm`, `dbCheck`.
- Event wiring to the named stub functions.
- The manifest actions `disconnect`, `forget`, `tp_on`, `tp_off` (reduce), `unlock`, `db_set`, `db_clear`.
- CSS.
- Tests: `udsBatch4Dom.test.ts` (14) + `udsBatch4.test.ts` (15).
- Updated test: `udsWorkbench.test.ts` "connect …". Its old assertion "no 연결 해제 anywhere" was the requirement U6
  replaces. It now asserts the [연결 해제] control exists and that connecting never calls `uds.disconnect`.

Common to every handler:
- `t = target()`.
- An op key is `` `${op}:${t}` ``. If `pendingOps.has(key)` → return. Add it before `ctx.act`, then delete it, set
  `uiKey = ''` and `ctx.scheduler.invalidate()` in `finally`.
- `msg(e) = e instanceof Error ? e.message : String(e)`. `code(e) = (e as {code?: unknown})?.code`.
- Use `ctx.act` directly. The `act` helper swallows errors.
- `hh(n) = n.toString(16).toUpperCase().padStart(2, '0')`.

**B4-D1 `onDisconnect`**:
1. Guard: `t && targets.has(t) && canOperate()`, and not pending.
2. `abort = discArm?.target === t && performance.now() < discArm.until`; then `discArm = null`.
3. `ctx.act('disconnect', abort ? {target: t, abort: true} : {target: t})`. Success → nothing else; the `uds.state`
   shows `closed`.
4. Error:
   - `code(e) === 'E_BUSY' && !abort` → `discArm = {target: t, until: performance.now() + SECOND_ENTER_MS}` and toast
     `` `${t}: 진행 중 요청 있음 — [연결 해제]를 ${SECOND_ENTER_MS / 1000}초 안에 다시 누르면 요청을 중단하고 해제 (쓰기는 결과 불확정이 될 수 있음)` ``.
   - Else toast `` `uds.disconnect 실패: ${msg(e)}` ``.

**B4-D2 `onForget`**:
1. Guard: `t && canOperate()`, and not pending.
2. `ctx.act('forget', {target: t})`.
3. Error → toast `` `uds.forget 실패: ${msg(e)}` ``.

**B4-D3 `onTpToggle`**:
1. `tv = targets.get(t)`. Guard: `t && tv`, and not pending.
2. If `tv.tpOn` → `ctx.act('tp_off', {target: t, on: false})` (reduce direction: no `canOperate` check).
3. Else:
   - Require `canOperate() && tv.link === 'up'`.
   - `iv = Number(tpIv.value)`. If it is not an integer in 500–30000 → toast
     `'TesterPresent 주기: 500–30000 ms 정수 (ECU S3보다 짧게)'` and return.
   - `ctx.act('tp_on', {target: t, on: true, interval_ms: iv})`.
   - On success, if `presetOf(t)` exists and its `tpIntervalMs !== iv` → `setPrefs` with that preset replaced by
     `{...p, tpIntervalMs: iv}`.
4. Error → toast `` `uds.tester_present 실패: ${msg(e)}` ``.

**B4-D4 `onUnlock`**:
1. `tv = targets.get(t)`. Guard: `t && tv && canOperate() && tv.link === 'up'`, and not pending.
2. Level: `lv = secLevel.value.trim().replace(/^0x/i, '')`. It must match `^[0-9A-Fa-f]{1,2}$`, and
   `level = parseInt(lv, 16)` must be odd and in 1–0x7D. Otherwise `secMsg` gets class `err` and text
   `'레벨: 홀수 hex 01–7D (requestSeed)'`. Set `unlockArm = null` and return.
3. Provider: `prov = secProv.value.trim()` must match `^[A-Za-z0-9_.-]{1,64}$`. Otherwise `err` with
   `'키 제공자 이름: 영문·숫자·_.- 1–64자 (loom.toml [[uds.key_providers]])'`. Set `unlockArm = null` and return.
4. Arming: `key = `${t}|${level}|${prov}``. If not (`unlockArm?.key === key && performance.now() < unlockArm.until`):
   - Set `unlockArm = {key, until: now + SECOND_ENTER_MS}`.
   - Give `secMsg` class `warn` and text
     `` `R3 보안 해제: ${t} 레벨 ${hh(level)} · 제공자 ${prov} — ${SECOND_ENTER_MS / 1000}초 안에 다시 누르면 실행 (실패 시 재시도 없음)` ``.
   - `uiKey = ''`, invalidate, return.
5. Send: `unlockArm = null`. Clear `secMsg` (className base, empty text).
   `h = await ctx.act('unlock', {target: t, level, provider: prov})`.
   - `d = h.result` as an object.
   - `secMsg` gets no state class and text
     `d.already_unlocked === true ? `레벨 ${hh(level)} 이미 해제됨 (seed 0 — 키 전송 없음)` : `레벨 ${hh(level)} 해제됨``.
   - If `presetOf(t)` exists and its (`keyProvider`, `unlockLevel`) differ → `setPrefs` with `{...p, keyProvider: prov, unlockLevel: level}`.
6. Error → `secMsg` class `err`, text `` `해제 실패: ${msg(e)} — 자동 재시도 없음` ``.
`secMsg` class names: base `'sv-note sec-msg'` plus an optional ` err` / ` warn`.

**B4-D5a `onDbInput`**:
- `dbCheck = dbText.value.trim() ? checkDbText(dbText.value) : null`.
- `dbMsg`:
  - `null` → empty, base class.
  - Error → class `err`, text = `dbCheck.err` exactly.
  - Ok → `` `${dbCheck.bytes} B · DID ${Array.isArray(db.dids) ? db.dids.length : 0}개 — [대상에 적용]하면 daemon이 검증` ``.
- `uiKey = ''`, invalidate.
- Base class: `'sv-note db-msg'`.

**B4-D5b `onDbFile`**:
1. `f = dbFile.files?.[0]`. None → return.
2. `f.size > MAX_DB_TEXT_BYTES` → `dbMsg` err `` `파일이 너무 큼 (${f.size} B, 최대 ${MAX_DB_TEXT_BYTES} B)` ``.
3. Else `f.text()`:
   - Success → `dbText.value = text; onDbInput()`.
   - Failure → err `'파일을 읽을 수 없음'`.
4. Always `dbFile.value = ''`. Nothing is sent.

**B4-D5c `onDbSaved`**: `s = prefs.diagDbs?.find(x => x.id === dbSaved.value)`. If found → `dbText.value = s.json; onDbInput()`.
Then `dbSaved.value = ''`. Nothing is sent.

**B4-D5d `onDbApply`**:
1. `tv = targets.get(t)`. Guard: `t && tv && canOperate() && tv.link !== 'closed'`, and not pending.
2. If `dbCheck?.db` is missing → `dbMsg` err `dbCheck?.err ?? 'DB JSON을 입력하세요'` and return.
3. `h = await ctx.act('db_set', {target: t, db: dbCheck.db})`. `d = h.result`:
   `hash = String(d.db.hash)`, `ecu = typeof d.db.ecu === 'string' ? d.db.ecu : null`, `dids = d.db.dids`,
   `warnings = string[]`.
4. `dbMsg` (no class): `` `${t}: DB ${hash} 적용 (${ecu ?? '-'}, DID ${dids}개)` `` plus
   `` ` · ${warnings.join(' · ')}` `` when there are warnings.
5. Remember the DB: `entry = {id: `db-${hash}`, name: ecu ?? hash, json: dbText.value}`.
   `list = [entry, ...(prefs.diagDbs ?? []).filter(x => x.id !== entry.id)].slice(0, MAX_SAVED_DBS)`.
   `setPrefs({...prefs, diagDbs: list})`.
6. Error → err `` `적용 실패: ${msg(e)}` ``.

**B4-D5e `onDbClear`**:
1. Guard: `t && canOperate()`, and not pending.
2. `h = await ctx.act('db_clear', {target: t})`.
3. `dbMsg` = `` `${t}: ${h.result?.cleared === true ? 'DB 해제 — ISO DID 표만 사용' : '적용된 DB 없음'}` ``.
4. Error → err `` `해제 실패: ${msg(e)}` ``.

**B4-D6** Seed of the selected row: `r = rowById(table.selected)`, `sd = r ? seedFromRow(r) : null`. Nothing happens
without `sd`.
- `onSeedCopy`:
  - `navigator.clipboard?.writeText` missing → toast `'클립보드 사용 불가'`.
  - Else `writeText(sd.seedHex)`. Success → toast `'seed 복사됨'`; failure → toast `'클립보드 사용 불가'`.
- `onSeedTemplate`: `setMode('raw'); modeSeg.set('raw'); rawInput.value = keyTemplate(sd.level); persistComposer(); reqChanged(); rawInput.focus();`.
  Do NOT call `syncFromHex`: the template is not hex, and the composer stays invalid until the key is typed. Never sends.

**B4-D7 `renderUi`** additions:
- Add to the `key` array: `[...pendingOps]`, `discArm`, `unlockArm`, `prefs.diagDbs?.length ?? 0`.
  `tv`/`pr` are already there.
- `link === 'closed'`: `linkSt.className = 'link-st closed'`, text `` `해제됨${tv?.linkCause ? ` (${tv.linkCause})` : ''}` ``.
  Put this branch before the generic "끊김" else.
- `connectBtn.title` when `link === 'up'`: `'연결됨 — 새 설정을 적용하려면 [연결 해제] 후 [연결]'`. The "RPC가 없어" text
  goes away, and so does the stale `// no disconnect RPC exists` comment.
- `discBtn`:
  - `hidden = !(tv && ['up', 'down', 'connecting'].includes(link))`.
  - `disabled = !canOperate() || pendingOps.has(`disconnect:${cur}`)`.
  - `armed = discArm?.target === cur && performance.now() < discArm.until`; `classList.toggle('armed', armed)`.
  - `discLabel.textContent = armed ? '중단 후 해제' : '연결 해제'`.
  - `title = armed ? '다시 누르면 진행 중 요청을 중단하고 연결 해제 (uds.disconnect abort)' : 'uds.disconnect — 의도적으로 링크를 닫음 (자동 재연결 없음, UDS 바이트 전송 없음)'`.
- `forgetBtn`:
  - `hidden = !(tv && (link === 'closed' || link === 'down'))`.
  - `disabled = !canOperate() || pendingOps.has(`forget:${cur}`)`.
- `reconnNote`:
  - `hidden = !(pr && tv && profileNeedsReconnect(pr, {backend: tv.backend, addr: tv.addr, link: tv.link}))`.
  - `title = '프로필이 현재 연결과 다름 — [연결 해제] 후 [연결]로 새 설정 적용'`.
- `tpBtn`:
  - `textContent = tv?.tpOn ? 'TP 중지' : 'TP 시작'`.
  - `disabled = !tv || pendingOps.has(`tp:${cur}`) || (!tv.tpOn && !(canOperate() && link === 'up'))`.
  - Use the op key `tp` for both on and off.
- `tpIv`:
  - Unless it is `document.activeElement`: `value = String(tv?.tpOn && tv.tpIntervalMs !== null ? tv.tpIntervalMs : tpIntervalOf(pr))`.
  - `disabled = !!tv?.tpOn || !canOperate()`.
- `tpInfo.textContent`:
  - No `tv` → empty.
  - Else `tpCellText(tv, tpBy.get(tv.target) ?? 0)`, plus `` ` · ${tv.tpOwner}` `` when `tpOn && tv.tpOwner`, plus
    `` ` · 기본 주기 ${DEFAULT_TP_INTERVAL_MS} ms (프로필 미설정)` `` when `!tv.tpOn && !(pr && pr.tpIntervalMs)`.
- `secLevel`/`secProv`:
  - When empty and not focused, prefill from `pr?.unlockLevel` (`hh`) / `pr?.keyProvider`.
  - `disabled = !canOperate()`.
- `unlockBtn`:
  - `disabled = !canOperate() || !tv || link !== 'up' || pendingOps.has(`unlock:${cur}`)`.
  - `armed = unlockArm !== null && unlockArm.key.startsWith(`${cur}|`) && now < until` → class `armed`.
  - `unlockLabel.textContent = armed ? 'R3 실행 확인' : '자동 해제'`.
- `dbInfo`:
  - `tv?.dbHash` → `replaceChildren('적용됨 ', el('b', undefined, tv.dbHash), ` · ${tv.dbEcu ?? '-'} · DID ${tv.dbDids ?? '?'}개`)`.
  - Else text `'적용된 DB 없음 — ISO DID 표만 사용'`.
- `dbSaved` options are rebuilt only when `dataset.k` (the ids/names joined) changes:
  `option('저장된 DB…', '')` first, then one per `prefs.diagDbs` (`` `${name}` ``, value `id`).
- `dbApply.disabled = !canOperate() || !tv || link === 'closed' || !dbCheck?.db || pendingOps.has(`db_set:${cur}`)`.
- `dbClearBtn.disabled = !canOperate() || !tv?.dbHash || pendingOps.has(`db_clear:${cur}`)`.

**B4-D8 `applyState`**: right after the seq guard (before `noteTarget`):
- If `isForgotten(ev.payload)`:
  - `targets.delete(name); targetSeq.set(name, seq);`
  - `if (selectedTarget === name) selectedTarget = null;`
  - `uiKey = ''; linksKey = ''; return true;`.
- A later state with a higher seq adds the target again through the existing path.

**B4-D9 U8** (bodies in createUdsConsole):
- History column `rt`: `rr = rowRoute(it.row.payload)` → `cell(rr.text, rr.known ? 'c-rt' : 'c-rt na', rr.title)`. The
  gap row is unchanged.
- `renderDetail` seqHead: `` `#${no ?? '?'} · ${rowTarget(r)} · ${rowRoute(r.payload).text}` ``.
- Confirmed-value provenance (`renderVals`): `rowRoute(rowById(v.requestId)?.payload ?? {}).text` in place of the
  routeAt lookup.
- `exportRows`:
  - Replace `route_first_seen` with `generation: rr.generation`, `backend: rr.known ? (string payload.backend or null) : null`,
    `addr: rr.known ? (string payload.addr or null) : null`, `db_hash: string payload.db_hash or null`,
    `purpose: string payload.purpose or null`. These sit at the same position, in this order.
  - The JSON envelope `version: 2`. The CSV empty-header fallback lists the same keys in the same order.
- Delete `routeAt` (declaration and the `routeAt.set` in `draw`), `routeText` and `ROUTE_UNKNOWN`. They are dead after
  the above. This is the only allowed deletion.
- `renderDetail`, after `out.push(appliedNote(r))`: if `sd = seedFromRow(r)`, set
  `seedCode.textContent = spacedHex(sd.seedHex) + (sd.zero ? ' (0 — 이미 해제됨)' : '')`, set `seedTpl.disabled = sd.zero`,
  and `out.push(seedBar)`.

### B4 lease & acceptance
Lease: function bodies, including callbacks, in `ui/src/panels/uds/{udsModel,udsPrefs,consolePanel}.ts`. B4-F1..F6, F3a,
F3b, P1..P4, D1..D9 only. No CSS, imports, DOM construction, manifest or test edits.

Acceptance, run from `ui/` (isolated cache, never `tsc -b`):
```
./node_modules/.bin/vitest run --maxWorkers=2 src/panels/uds           # all green (baseline: 89 pass; 30 fail on stubs = 15 pure + 14 DOM + 1 updated workbench test)
./node_modules/.bin/tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
./node_modules/.bin/tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
./node_modules/.bin/eslint src/panels/uds
```
