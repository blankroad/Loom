# PSU operator upgrade — FUNCTIONS (contracts)

Batch 1 is complete below (**PLAN_READY_BATCH1**). B2–B4 sections are added before their leases (see PLAN.md §2).
Opus owns every signature/type/test; Sonnet writes only the bodies listed in the allowlist. Anything missing or
ambiguous → stop and report to Opus with the exact reason (do not invent a helper, rename, or change a test).

---------------------------------------------------------------------------------------------------

## Batch 1 — PSU-01 limits, PSU-02 chart readout, PSU-05 labels, PSU-06 copy, PSU-07 empty state

### B1.0 Lease, allowlist, acceptance

Lease (full-file, Sonnet only while held): `ui/src/panels/psu/psuModel.ts`, `ui/src/panels/psu/gaugePanel.ts`,
`ui/src/panels/psu/psuConnect.ts`.
Not leased (Opus-owned, read-only for Sonnet): `psu.css` (ROOT: Opus owns CSS/static metadata — B1 rules already added),
`psu.test.ts`, `psuModule.test.ts`, `fixtures.ts`, everything outside `ui/src/panels/psu`.

Body allowlist (Sonnet writes/changes ONLY these bodies; signatures/types/exports already exist and are frozen):

| # | Unit | File | Kind |
|---|---|---|---|
| B1-M1 | `reduceState` | psuModel.ts | change existing body |
| B1-M2 | `limitNum` | psuModel.ts | replace `PENDING` |
| B1-M3 | `limitText` | psuModel.ts | replace `PENDING` |
| B1-M4 | `checkSetpoint` | psuModel.ts | replace `PENDING` |
| B1-M5 | `modeLabel` | psuModel.ts | change existing body |
| B1-M6 | `protLabel` | psuModel.ts | change existing body |
| B1-M7 | `clockLabel` | psuModel.ts | replace `PENDING` |
| B1-M8 | `wallS` | psuModel.ts | replace `PENDING` |
| B1-M9 | `Series.push`, `Series.gap`, `Series.prune`, `Series.laneData` | psuModel.ts | change/replace bodies (remove the `void at;` / `void wallSec;` placeholders) |
| B1-M10 | `readoutAt` | psuModel.ts | replace `PENDING` |
| B1-M11 | `readoutText` | psuModel.ts | replace `PENDING` |
| B1-M12 | `xTickLabels` | psuModel.ts | replace `PENDING` |
| B1-M13 | `yRange` | psuModel.ts | replace `PENDING` |
| B1-M14 | `chartColors` | psuModel.ts | replace `PENDING` |
| B1-M15 | delete the `PENDING` helper once no caller remains | psuModel.ts | cleanup (allowed) |
| B1-C1 | `psuConnectParams` | psuConnect.ts | change existing body |
| B1-C2 | `psuConnectCli` | psuConnect.ts | change existing body |
| B1-C3 | `createPsuConnect` body: placeholders + note text/testid only | psuConnect.ts | change existing body |
| B1-G1…G9 | `createPsuGauge` body units listed in B1.3 | gaugePanel.ts | change existing body |
| ~~B1-S1~~ | CSS rules B1.4 — **done by Opus before the lease** (not Sonnet) | psu.css | — |
| ~~B1-K1~~ | comment fixes B1.5 — **done by Opus before the lease** | psuConnect.ts, gaugePanel.ts | — |

Forbidden in B1: new exports, changed exported types/signatures, new files, `registerPanel` spec changes (actions/queries
are B4), any RPC/param change, package/manifest edits, test edits, new dependencies.

Acceptance (run from `/Users/ctmctm/workspace/Loom/.worktrees/operator-psu/ui`, report exact output):
```
npx vitest run src/panels/psu          # expect 52 passed, 0 failed (baseline before bodies: 20 failed / 32 passed)
npx tsc -b --pretty false              # expect no output, exit 0
npx eslint src/panels/psu              # expect no errors
```
Also run `npx vitest run src/shell/moduleApps.test.tsx src/sdk/capacityLive.test.ts` (consumers of psu.gauge) — must stay green.
No GUI, no daemon, no device IO.

### B1.0a Amendment r2 (2026-11-15, answer to Sonnet blocker) — gaugePanel.ts import block

Imports are Opus-owned, but `gaugePanel.ts` is leased to Sonnet, so Opus does not edit it. Sonnet is **authorized to replace
the `from './psuModel'` import statement of `gaugePanel.ts` with exactly this block** (nothing else in the import section):
```ts
import {
  chartColors,
  checkSetpoint,
  clockLabel,
  EMPTY_MEAS,
  eng,
  fmt,
  gapCauseLabel,
  gapCauseSummary,
  limitNum,
  limitText,
  modeLabel,
  monoS,
  protLabel,
  readMetrics,
  readoutAt,
  readoutText,
  outText,
  reduceState,
  sourceLabel,
  Series,
  wallS,
  xTickLabels,
  yRange,
  WINDOW_S,
  type Lane,
  type Meas,
  type PsuState,
} from './psuModel';
```
`parseSetpoint` is removed (no longer used by the gauge; it stays exported for `checkSetpoint` and tests). If eslint then
reports `clockLabel` unused, drop only that one name. `psuConnect.ts` needs no import change. Go-ahead: B1-C1…C3 and B1-G1…G9.

### B1.1 Data sources (truth table, already in the engine — no engine change in B1)

| Field | sim (`sim.rs state_json` + worker `state_payload`) | scpi-tcp (`scpi.rs state_payload`) | psu.status |
|---|---|---|---|
| `vmax`,`imax` (number, V/A) | always | always | always (status = state payload + extras) |
| `cycling` (bool) | always | always `false` today (B2 makes it truthful) | yes |
| `mode` | `cv`/`cc`/`off` | `off` when output off, else `unknown` | yes |
| `prot` | `ok`/`ovp`/`ocp`/`otp` | always `unknown` | yes |
| record `ts.wall_ns`, `ts.mono_ns` | decimal strings | decimal strings | n/a (no ts) |

### B1.2 psuModel.ts functions

**B1-M1 `reduceState(prev: PsuState, ev: LoomEvent): PsuState`** — keep every existing rule byte-for-byte, add:
- `vmax: posNum(p.vmax) ?? prev.vmax ?? null`, `imax: posNum(p.imax) ?? prev.imax ?? null` where posNum = finite number `> 0`
  (inline expression; do not export a helper). `0`, negative, NaN, strings → keep previous.
- `cycling: typeof p.cycling === 'boolean' ? p.cycling : (prev.cycling ?? null)`.
- Non-`psu.state` kinds still return `prev` unchanged (same object).
Tests: `reduceState keeps limits/cycling…`, `limits from psu.status…` (status payload goes through this function in `refreshStatus`).

**B1-M2 `limitNum(x: number): string`** — `Number.isInteger(Math.round(x * 100 * 1e6) / 1e6) ? x.toFixed(2) : String(x)`.
Meaning: exact at 0.01 (allowing fp noise ≤ 1e-6 of a hundredth) → 2 decimals; otherwise the full JS text so a limit is never shown rounded (rounding could make a rejected value look allowed).
Examples: `16→'16.00'`, `2.5→'2.50'`, `0.1→'0.10'`, `9.005→'9.005'`.

**B1-M3 `limitText(s: PsuState): string`** — `vmax`/`imax` read as `s.vmax ?? null`, `s.imax ?? null`.
Both null → `'미보고'`. Otherwise `` `${v === null ? '—' : limitNum(v)} V / ${i === null ? '—' : limitNum(i)} A` ``.
Examples: `'9.00 V / — A'`, `'9.00 V / 2.50 A'`. Never substitutes `PSU_DEFAULT_LIMITS` (unknown ≠ default).

**B1-M4 `checkSetpoint(raw: string, limit: number | null, unit: 'V' | 'A'): SetpointCheck`**
```
r = parseSetpoint(raw)                                  // existing, unchanged
if !r.ok:  n = Number(raw.trim()); reason = Number.isFinite(n) ? 'negative' : 'nan'
           return { ok:false, reason, message:'0 이상의 숫자를 입력하세요' }
if r.value !== null && limit !== null && r.value > limit:
           return { ok:false, reason:'over', message:`${raw.trim()} ${unit} — 한도 ${limitNum(limit)} ${unit} 초과` }
return r                                                // { ok:true, value }
```
Strict `>` exactly like `Limits::violation` (`v > self.vmax`): equal is allowed. `limit === null` → no limit check (gateway is
the final guard). Message uses the typed text (trimmed), not a reformatted number.

**B1-M5 `modeLabel(mode: string | null): string | null`** — `null`/blank → null; `m = mode.trim()`; case-insensitive
`off` → null (the output word already says OFF); `unknown` → `'모드 미보고'`; `cv`/`cc` → upper-case; anything else → `m`
(device text; rendered via `textContent` only).

**B1-M6 `protLabel(prot: string | null)`** — null or `unknown` → `{ text: '보호 미보고', warn: false }`; others unchanged.

**B1-M7 `clockLabel(wallSec: number, ms = false): string`**
```
msInt = Math.floor(Math.round(wallSec * 1e6) / 1e3)    // µs rounding removes fp noise, then floor to ms
d = new Date(msInt)                                     // LOCAL time (status bar / UART also show local time)
HH:MM:SS from getHours/getMinutes/getSeconds, padStart(2,'0'); with ms append '.' + getMilliseconds padStart(3,'0')
```
Non-finite input is not expected (callers pass `wallS()` results or row values); no special case required.

**B1-M8 `wallS(ev: LoomEvent): number | null`** — `/^\d+$/.test(ev.ts.wall_ns) ? Number(BigInt(ev.ts.wall_ns) / 1_000_000n) / 1000 : null`
(same shape as `monoS`, but null instead of 0: a missing wall time is unknown, not 1970).

**B1-M9 `Series`** (fields `t,v,i,p,w,sv,si` already declared)
- `push(tS, v, i, p = null, at = {})`: append `tS,v,i,p`, `at.wallS ?? null`, `at.setV ?? null`, `at.setI ?? null`; then `prune(tS)`. Remove `void at;`.
- `gap(tS, wallSec = null)`: one row with `v,i,p,sv,si = null`, `w = wallSec`. Must not route through a path that could set setpoints. Remove `void wallSec;`.
- `prune(nowS)`: same window/MAX_POINTS rule as today, splicing **all seven** arrays by the same `k`.
- `laneData(l)`: `v → [this.t, this.v, this.sv]`, `i → [this.t, this.i, this.si]`, `p → [this.t, this.p]` — the same array objects (no copies; uPlot reads them each `setData`).
- `data()`, `lane()` unchanged (existing callers/tests).

**B1-M10 `readoutAt(s: Series, idx: number): ReadoutRow | null`** — `Number.isInteger(idx) && idx >= 0 && idx < s.t.length`
else null. Row = `{ tS: s.t[idx], wallS: s.w[idx] ?? null, v, i, p, setV: s.sv[idx] ?? null, setI: s.si[idx] ?? null, gap: v === null && i === null && p === null }`.

**B1-M11 `readoutText(r: ReadoutRow | null, prefix: '커서' | '최신'): string`**
```
if r === null: return prefix === '최신' ? '측정 없음' : ''
clock = r.wallS !== null ? clockLabel(r.wallS, true) : `t ${r.tS.toFixed(3)} s`
if r.gap: return `${prefix} ${clock} · 측정 손실 (선 끊김)`
q = (x, u) => { const f = eng(x, u); return `${f.num} ${f.unit}` }      // '—' + base unit when null
parts = [`${prefix} ${clock}`, q(r.v,'V'), q(r.i,'A'), q(r.p,'W')]
if r.setV !== null || r.setI !== null: parts.push(`설정 ${fmt(r.setV,'V',2)} / ${fmt(r.setI,'A',2)}`)
return parts.join(' · ')
```
Exact strings in `readoutAt / readoutText` test.

**B1-M12 `xTickLabels(s: Series, splits: readonly number[]): string[]`** (x = mono seconds)
```
n = s.t.length
if n === 0: return splits.map(x => x.toFixed(0))
for each split x:
  k = last index with s.t[k] <= x (binary search; if none → 0)
  j = nearest index to k with s.w[j] !== null, searching k, k-1, …, 0 then k+1, …, n-1
  if j exists: label = clockLabel(s.w[j] + (x - s.t[j]))           // per-point wall + mono offset
  else:        label = `${(x - s.t[n-1]).toFixed(0)} s`              // relative to newest point, e.g. '-5 s'
```
Never use one global wall offset for the whole window (a wall step between samples must stay a step).

**B1-M13 `yRange(min, max, limit): [number, number]`**
```
L = limit !== null && Number.isFinite(limit) && limit > 0 ? limit : 1
if min === null || max === null || !Number.isFinite(min) || !Number.isFinite(max): return [0, L]
if max - min <= 1e-12:
   if Math.abs(max) <= 1e-12: return [0, L]
   pad = Math.max(Math.abs(max) * 0.02, 1e-3); lo = max - pad; hi = max + pad
else:
   pad = (max - min) * 0.1; lo = min - pad; hi = max + pad
if min >= 0 && lo < 0: lo = 0
return [lo, hi]
```
Examples (test): `(0,0,16)→[0,16]`, `(0,0,null)→[0,1]`, `(13.5,13.5,16)→[13.23,13.77]`, `(0.35,0.39,5)→[0.346,0.394]`, `(0,2,5)→[0,2.2]`, `(-1,1,null)→[-1.2,1.2]`, `(0.0004,0.0004,5)→[0,0.0014]`.

**B1-M14 `chartColors(read): ChartColors`** — `v = (t, fb) => read(t).trim() || fb`;
`axis = v('--muted','#a9b8c9')`, `grid = v('--line','#293849')`, `tick = v('--line','#405267')`, `key = `${axis}|${grid}|${tick}``.
(The fallbacks are today's hard-coded dark palette, so a page without tokens renders exactly as before.)

### B1.3 gaugePanel.ts — `createPsuGauge` body units (DOM contract)

Reuse the existing local `el`/`tid` helpers, `ctx.scheduler`, `Series`, existing DOM nodes. Do not add exports, panel spec
fields, RPCs, timers or listeners beyond those named here. All device/text values go through `textContent`.

**B1-G1 Unbound layout (PSU-07).** When `bound === null`:
- create `placeholder = tid(el('div', 'psu-placeholder muted', '연결하면 실측·설정·추세가 여기에 표시됩니다'), 'psu-placeholder')` and append it to `root` right after `connect.root` (order: connect, head, placeholder, vals, ctl, trend, foot);
- set `hidden = true` once on: `outWord`, `modeChip`, `protChip`, `acts` (contains stopBadge/emergency/outBtn/r3/disconnect), `vals`, `ctl`, `trend`, `foot`;
- `makePlots` returns immediately (no uPlot instances, no canvas probe).
When bound: no placeholder element at all; nothing above is hidden by this rule. `draw()` must not un-hide them for unbound
(it sets `modeChip.hidden` — guard: `modeChip.hidden = bound === null || mode === null`). Existing disabled states stay
(emergency `disabled = bound === null`, etc.) — hidden is in addition, not instead.

**B1-G2 Limits row (PSU-01).** In the setpoints section after the `전류 제한` row:
`limRow = tid(el('div','psu-setrow psu-limits'),'psu-limits')` containing `el('span','psu-setlabel','한도')` and
`limVal = el('span','psu-setval')`; `limRow.title = '프로필 한도 (연결 시 낮춘 값 포함) — 넘는 설정은 장치로 보내지 않습니다'`.
In `draw()`: `limVal.textContent = limitText(state)`.
Input titles in `draw()`: `vIn.title = state.vmax ? `한도 ${limitNum(state.vmax)} V 이하` : '한도 미보고 — 데몬이 최종 확인'`,
same for `iIn` with `imax`/`A`.

**B1-G3 `updatePending` (PSU-01).** Replace `parseSetpoint` with
`v = checkSetpoint(vIn.value, state.vmax ?? null, 'V')`, `i = checkSetpoint(iIn.value, state.imax ?? null, 'A')`.
`aria-invalid` per field = `String(!x.ok)`. If either fails: `pending.textContent = (first failing of v, i).message`,
`pending.className = 'psu-pending err'`, return. Else the existing dirty logic unchanged (uses `.value`).

**B1-G4 `setBtn` click (PSU-01).** Same guards; use the two `checkSetpoint` results instead of `parseSetpoint`;
any `!ok` or both values null → `ctx.ui.toast('설정값을 확인하세요')`, no RPC. Params unchanged (`{channel, v?, i?}`).
After success the existing clear + `updatePending()` stays.

**B1-G5 Mode/prot chips (PSU-05).** In `draw()`: `modeChip.setAttribute('aria-label', `동작 모드: ${mode}`)` when shown;
`protChip.setAttribute('aria-label', `보호 상태: ${prot.text}`)` always.

**B1-G6 Series feed (PSU-02).** In the `draw()` loop: measurement → `series.push(monoS(ev), m.v, m.i, m.p, { wallS: wallS(ev), setV: state.setV, setI: state.setI })`
(state is already reduced for this record, so setpoints are the last *reported* before the sample); gap → `series.gap(monoS(ev), wallS(ev))`.

**B1-G7 Readout (PSU-02).** `readout = tid(el('span','psu-readout'),'psu-readout')` inserted in `trendHead` after the
note and before `toggles`. Panel-local `let cursorIdx: number | null = null`.
In `draw()`: `readout.textContent = cursorIdx !== null ? readoutText(readoutAt(series, cursorIdx), '커서') || readoutText(readoutAt(series, series.t.length - 1), '최신') : readoutText(readoutAt(series, series.t.length - 1), '최신')`.
(An out-of-range cursor after pruning falls back to the latest row.) No `aria-live` (updates up to 10 Hz).

**B1-G8 Plots (PSU-02)** — `makePlots(w)` per lane, keeping the existing canvas probe, try/catch, sync key, LANE_H, width rule:
- colours: `c = chartColors((t) => getComputedStyle(root).getPropertyValue(t))`; axes `stroke: c.axis`, `grid: { stroke: c.grid }`, `ticks: { stroke: c.tick }`; remember `colorKey = c.key` and `plotW = w` (panel-local lets).
- x axis: `values: (_u, splits) => xTickLabels(series, splits)`; `scales.x.time` stays `false`.
- y scale: `scales: { x: { time: false }, y: { range: (_u, mn, mx) => yRange(mn, mx, laneLimit(l.key)) } }` where `laneLimit` is an inline closure: `v → state.vmax ?? null`, `i → state.imax ?? null`, `p → (vmax && imax) ? vmax * imax : null` (reads current `state` at call time).
- series: `[{ label: 't' }, { label: l.unit, stroke: l.stroke, spanGaps: false }, ...(l.key === 'p' ? [] : [{ label: '설정', stroke: l.stroke, width: 1, dash: [4, 4], spanGaps: false }])]`; data `series.laneData(l.key)`.
- cursor: keep `cursor: { sync: { key: syncKey } }`; add `hooks: { setCursor: [(u) => { const k = u.cursor.idx; cursorIdx = typeof k === 'number' ? k : null; ctx.scheduler.invalidate(); }] }`.
- `legend: { show: false }` stays (the readout replaces it).
In `draw()`: `plots.get(l.key)?.setData(series.laneData(l.key))` (was `lane`). Theme change: in `draw()` when `plots.size > 0`,
compute `chartColors(...)`; if `key !== colorKey` → destroy all plots, `plots.clear()`, `makePlots(plotW)`.
`resize(w)` also updates `plotW`. jsdom has no canvas → `plotErr` path as today (tests rely on it).

**B1-G9 Nothing else.** Header/meter/STOP/emergency/output/disconnect/status-poll logic unchanged.

### B1.4 psu.css (Opus, already applied — class names Sonnet must use)
```css
.psu-tile [hidden] { display: none !important; }            /* author display rules must not un-hide (PSU-07) */
.psu-tile .psu-placeholder { padding: 12px 10px; font-size: 12px; }
.psu-tile .psu-limits .psu-setval { color: var(--muted); }
.psu-tile .psu-readout { font: 11px ui-monospace, monospace; font-variant-numeric: tabular-nums; color: var(--fg); overflow-wrap: anywhere; }
```

### B1.5 psuConnect.ts

**B1-C1 `psuConnectParams`** — after the existing `> 0` check: `vmax.v > PSU_DEFAULT_LIMITS.vmax` →
`{ ok:false, error:'전압 한도는 기본 16 V 이하로만 낮출 수 있습니다' }`; then `imax.v > PSU_DEFAULT_LIMITS.imax` →
`'전류 한도는 기본 5 A 이하로만 낮출 수 있습니다'` (build the numbers from the constant). Equal to default is OK. Params unchanged.

**B1-C2 `psuConnectCli`** — `loom psu connect <ch>[ --scpi-tcp <addr>][ --vmax <String(vmax)>][ --imax <String(imax)>]`
from the validated params (exactly what would be sent); invalid draft → unchanged `'loom psu connect <별칭>'`.
Examples: `'loom psu connect a --vmax 9'`, `'loom psu connect ext --scpi-tcp 127.0.0.1:5025 --vmax 12.5 --imax 2'`.
(CLI flags exist: `cli.rs` `num("vmax")`, `num("imax")`.)

**B1-C3 `createPsuConnect`** — `vmaxIn.placeholder = `기본 ${PSU_DEFAULT_LIMITS.vmax}``, `imaxIn.placeholder = `기본 ${PSU_DEFAULT_LIMITS.imax}``;
`note` gets `tid(…,'psu-connect-note')` and text:
`'지원: sim, scpi-tcp(루프백 소프트웨어 엔드포인트, VOLT/CURR/OUTP/MEAS만). VISA·USB·GPIB·LAN 계측기는 미구현. 별칭마다 별도 PSU 작업자 (최대 4대) — 같은 별칭에 두 번째 전원은 연결되지 않습니다. 다출력 계측기의 출력 번호는 미지원.'`

**B1-K1 comments (Opus, already applied)** — `psuConnect.ts` header line "One supply per PSU worker: a second connect with another alias is refused…" →
"One supply per PSU worker *process*; the daemon runs one worker per alias (PerChannel, ≤ MAX_CHANNELS = 4). A second supply on
the same alias is refused (E_PORT_BUSY)." `gaugePanel.ts` header: "Not offered … logging/export" stays until B4; add
"limits hint (PSU-01), wall-clock axis + readout (PSU-02)" to the layout comment.

### B1.6 Tests (Opus-authored, frozen) → behaviour covered

| Test (file › name) | Covers |
|---|---|
| psu.test › PSU-01 active limits ×4 | G2, G3, G4, M1 (state + status), unknown limit = no client check |
| psu.test › PSU-02 trend readout ×2 | G6, G7, M8–M11, loss row |
| psu.test › PSU-05 mode / protection labels | G1 guard, G5, M5, M6 |
| psu.test › model batch 1 ×10 | M1–M14 exact I/O incl. boundaries (equal limit, fp ms, wall step, flat/zero ranges, token fallback) |
| psu.test › unknown stays dash (updated) | `보호 미보고` |
| psuModule.test › full tile + connect section (updated) | G1 hidden set, placeholder text/order, no RPC/no sub unchanged |
| psuModule.test › PSU-07 bound tile | no placeholder when bound, emergency visible |
| psuModule.test › PSU-01/PSU-06 connect form | C1 (form), C3 placeholders + note |
| psuModule.test › PSU-01 CLI preview | C1, C2, constant |
| all pre-existing PSU tests | regressions (emergency, `!`, typing never sends, gaps, STOP badge, provenance, disconnect) |

uPlot internals (setCursor hook, axis callbacks) cannot run in jsdom; their pure parts are M10–M14 (tested) and the
wiring is reviewed by Opus and checked by ROOT GUI (light/dark, hover readout) after integration.

### B1.7 Edge cases to respect
- Limits arrive late (status poll after first frame): `updatePending` runs every `draw()`, so a typed over-limit value becomes invalid as soon as limits are known.
- `cursorIdx` is index-based; pruning can shift it by a few rows for one frame — accepted (readout is a reading aid; values shown are always a real row).
- No interpolation anywhere: setpoint lines break at gaps (`sv/si = null` in gap rows).
- Theme rebuild must not leak: destroy before re-create; `dispose()` already destroys `plots`.

---------------------------------------------------------------------------------------------------

## Batch 2 — R2 all-off routing (safety defect) + PSU-04 engine power cycle

### B2.0 Lease, allowlist, acceptance

Lease (Sonnet, bodies only): `crates/loom-psu/src/gateway/handlers.rs`, `crates/loom-psu/src/worker.rs`, `crates/loom-psu/src/scpi.rs`.
Opus-owned, already done before the lease (do not edit): every type/field/variant/import/signature (`Act::Cycle(u64)`,
`Shared.cycling`, `PendingOn`, `Io.pending`, `PendingCycle.off_at_ns`, new fn signatures below, their `todo!()` placeholders to be
replaced), doc comments, `gateway.rs` help text, tests (`tests/psu.rs`, `tests/scpi.rs`, `tests/support/mod.rs`),
`crates/loom-gateway/tests/r1.rs` (R2 tests). No new imports are needed: if one is, stop and ask (imports are Opus).

| # | Unit | File | Kind |
|---|---|---|---|
| B2-H1 | `write_to` | handlers.rs | replace `todo!` |
| B2-H2 | `targets_param` | handlers.rs | replace `todo!` |
| B2-H3 | `Forward::call`, `Connect::call`, `Disconnect::call` | handlers.rs | change body: worker via `ctx.worker_for(WORKER_KIND, &ch)` |
| B2-H4 | `Set::call`, `Cycle::call` | handlers.rs | change body: `write_to(ctx, &ch, …)` |
| B2-H5 | `Out::call` | handlers.rs | change body (targets / routing / partial) |
| B2-H6 | delete private `write` once unused | handlers.rs | cleanup allowed |
| B2-W1 | `PsuWorker::foreign_ch` | worker.rs | replace `todo!` |
| B2-W2 | `PsuWorker::on_call`, `PsuWorker::on_write` | worker.rs | add the foreign-ch guard (first statement after the connect/disconnect dispatch) |
| B2-W3 | `Device::finish_cycle` | worker.rs | STOP check + `off_ms_actual` |
| B2-W4 | `on_write` sim `"cycle"` arm | worker.rs | supersede a pending cycle |
| B2-W5 | `PsuWorker::write_scpi` `"cycle"` arm | worker.rs | build `Act::Cycle` (remove NotImplemented refusal) |
| B2-S1 | `Io::state_payload`, `ScpiDevice::state_payload` | scpi.rs | `"cycling": s.cycling` |
| B2-S2 | `Io::write` | scpi.rs | Cycle routing + supersede; replace the two `todo!` arms |
| B2-S3 | `Io::cycle_off` | scpi.rs | replace `todo!` |
| B2-S4 | `Io::supersede_cycle` | scpi.rs | replace `todo!` |
| B2-S5 | `Io::drop_cycle` | scpi.rs | replace `todo!` |
| B2-S6 | `Io::finish_cycle` | scpi.rs | replace `todo!` |
| B2-S7 | `Io::run` | scpi.rs | change body (pending wait/due/exit) |

Forbidden: method schemas, risk table, panel manifest, CLI, model constants, tests, new pub items, new deps.

Acceptance (from `/Users/ctmctm/workspace/Loom/.worktrees/operator-psu`, env `CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2`):
```
cargo test -p loom-psu --test psu      # baseline now: 15 passed / 9 failed  → expect 24 passed
cargo test -p loom-psu --test scpi     # baseline now:  8 passed / 8 failed  → expect 16 passed
cargo test -p loom-psu --lib           # expect all pass
cargo test -p loom-gateway --test r1 psu_   # baseline 2 failed → expect 2 passed (real daemon, 2 PerChannel workers)
cargo clippy -p loom-psu --all-targets -- -D warnings   # expect clean (skeleton dead-code warnings disappear)
```
No device IO, no GUI. These tests use in-process fakes / a loopback in-test server only.

### B2.1 Gateway routing (R2 defect — evidence in STATUS.md)

Rule: after `resolve_channel` (or a defaulted `ch`), every worker access uses `ctx.worker_for(WORKER_KIND, &ch)`. The daemon
routes a channel-less `ctx.worker()` handle to *any running instance* (`pool.get(None)`), which is how a `pb` write landed on `pa`.

**B2-H1 `async fn write_to(ctx: &CallCtx, ch: &str, rid: &str, op: Value) -> Result<Value, DomainError>`**
`ctx.worker_for(WORKER_KIND, ch)?.write(rid, op, ctx.deadline()).await`.

**B2-H2 `pub fn targets_param(p: &Value) -> Result<Option<Vec<String>>, DomainError>`**
`p.get("targets")` absent or `null` → `Ok(None)`. Array where every item is a non-empty string and length ≥ 1 → `Ok(Some(items))`
(order kept, duplicates removed keeping first). Anything else (string, number, empty array, empty/non-string item) →
`Err(usage("targets must be a non-empty list of supply names or \"*\""))`.

**B2-H3** `Forward::call`: `ctx.worker_for(WORKER_KIND, &ch)?.call(self.0, args, ctx.deadline()).await`.
`Connect::call`: after defaulting `args["ch"]`, `let ch = param_ch(&args).unwrap_or("bench").to_owned();` then
`ctx.worker_for(WORKER_KIND, &ch)?.call("psu.connect", args, …)`. `Disconnect::call`: `worker_for(…, &ch)`.

**B2-H4** `Set::call`, `Cycle::call`: replace `write(ctx, &rid, op)` with `write_to(ctx, &ch, &rid, op)`. Nothing else changes.

**B2-H5 `Out::call`** (params `{ch?, channel?, state?, targets?, dry_run?}`)
```
targets = targets_param(&params)?
state = match params.state { Some("on"|"off") → it; None if targets.is_some() → "off"; _ → usage("out needs state on|off") }
if targets.is_some():
    if state == "on"            → usage("targets is for switching outputs off only")
    if param_ch(&params).is_some() → usage("give ch or targets, not both")
chans, multi =
    targets = Some(t): (if t contains "*" { book.names() } else { t }), true
    else ch given:       vec![ch], false
    else state=="off" && book non-empty: book.names(), true      (existing behaviour)
    else:                vec![resolve_channel(...)?], false
if chans.is_empty()  → return Ok(json!({"results": [], "note": "no supply connected"}))
dry_run → unchanged preview of the first channel
!multi   → return write_to(ctx, &chans[0], &rid, out_op(&chans[0], &state)).await       (result shape unchanged)
multi    → for ch in chans (in order): r = write_to(ctx, ch, &format!("{rid}/{ch}"), out_op(ch, &state)).await
             results.push(Ok → {"ch", "ok": true, "result": v} | Err(e) → {"ch", "ok": false, "error": e.to_string(), "code": e.code})
           every ok → Ok(json!({"results": results}))
           else → Err(DomainError::new(ErrorCode::Partial,
                     format!("{failed}/{total} supplies not confirmed off: {names}")).with_detail(json!({"results": results})))
```
`names` = comma-joined failed channels. A partial all-off is never an overall success (the safe-state runner reports `failed`
with that reason). Note: the multi path is used for `state:"off"` only (targets or channel-less), so `"on"` never fans out.
Tests: psu.rs `b2_handlers_route…`, `b2_emergency_targets…`, `b2_out_targets_parameter_rules…`, `b2_emergency_with_no_supply…`,
`b2_partial_all_off…`; r1.rs `psu_emergency_off_reaches_every_per_channel_worker`, `psu_channelless_out_off_reaches_every_per_channel_worker`.

### B2.2 Worker channel guard

**B2-W1 `fn foreign_ch(&self, args: &Value) -> Option<WorkerError>`** — `asked = param_ch(args)?` (`ch` or `channel`);
`own = self.ext.ch` else `self.dev.ch`; no device → `None` (existing NotOpen paths answer); `asked != own` →
`Some(err(ErrorCode::NotOpen, format!("this worker drives {own}, not {asked}")))`.

**B2-W2** `on_call`: after the `psu.connect`(scpi) and `psu.disconnect` early returns, and before any other dispatch:
`if call.method != "psu.connect" && let Some(e) = self.foreign_ch(&call.args) { return Err(e) }` (sim `psu.connect` keeps its
PortBusy rule). `on_write`: first statement: `if let Some(e) = self.foreign_ch(&w.op) { return outcome(WriteState::Failed,
json!({"sent": false}), Some(e)) }` — no write row (the row would be on the wrong alias), nothing sent, device untouched.

### B2.3 Sim cycle (worker.rs)

**B2-W3 `Device::finish_cycle(&mut self, cx, at_ns)`** — after `take()`:
- if `cx.latch().is_on() && c.origin != Origin::User`: do **not** call `sim.output(true)`; `write_row(…, WriteState::Failed,
  json!({"action":"cycle","phase":"on","sent":false,"error":{"code":ErrorCode::Stopped,"message":"STOP latched during the off phase; ON not sent, output left OFF"}}))`; then `state_if_changed` (cycling → false); return.
- else as today, and the terminal row additionally carries `"off_ms_actual": (at_ns - c.off_at_ns) / 1_000_000`.

**B2-W4** sim `"cycle"` arm: before storing the new `PendingCycle`, if `d.cycle.take()` is `Some(old)` → `write_row(old cause,
WriteState::EffectUnverified, {"action":"cycle","phase":"on","sent":false,"superseded_by": w.request_id})` (same shape as the
existing `out` supersede). `off_at_ns: now` is already wired.

### B2.4 SCPI cycle (scpi.rs, IO thread) — COMMON edge case "once OFF, never accidentally ON"

**B2-W5** `write_scpi` `"cycle"`: `off_ms = w.op.off_ms as u64`; `None | 0 | > MAX_CYCLE_OFF_MS` →
`refuse(ErrorCode::Usage, format!("off_ms must be 1..={MAX_CYCLE_OFF_MS}"))`; else `Act::Cycle(off_ms)` and continue like the
other acts (same deadline / queue path).

**B2-S1** both `state_payload`s: `"cycling": s.cycling` (Io: from the locked `Shared`).

**B2-S2 `Io::write`** — keep the STOP and link checks first (they apply to Cycle too: agent cycle under STOP → Failed
Stopped `sent:false`, nothing sent). Then:
- `Act::Out(_)` or `Act::Cycle(_)`: `self.supersede_cycle(cause)` (only after both checks passed, i.e. this write will be sent).
- `Act::Cycle(off_ms)` → `return self.cycle_off(off_ms, cause, until)`. Replace the two `todo!` arms with `unreachable!`-free
  code (e.g. compute `cmds`/`sent_payload` only for Set/Out after the early Cycle return; restructuring inside the body is fine).
- `Act::Set` never touches a pending cycle.

**B2-S3 `fn cycle_off(&mut self, off_ms: u64, cause: &Cause, until: Instant) -> WriteOutcome`**
```
if now >= until → Failed Timeout {"sent": false}             (same message style as the unit loop)
send "OUTP OFF": Err(e) → fail_io(&e); NotSent at first unit → Failed Disconnected sent:false; else unknown(cause,"cycle",&e,&[])
row(cause, Sent, {"action":"cycle","off_ms":off_ms,"phase":"off"})
SYST:ERR?: Err → fail_io + unknown(...,["OUTP OFF"]); code≠0 → state_after_readback(); Failed Denied "supply refused \"OUTP OFF\": c,msg"
           with {"device_error":…}; unparsable → Broken → fail_io + unknown
OUTP?:     Err → fail_io + unknown; b = parse_bool; if Some(b) { sh().out = Some(b) }
if b != Some(false):
    row(cause, EffectUnverified, {"action":"cycle","phase":"off","readback":{"out":b},"on_sent":false,
                                  "note":"output not confirmed OFF; ON not sent"})
    state_if_changed(cause)
    return WriteOutcome{EffectUnverified, detail {"phase":"off","readback":{"out":b},"on_sent":false}, error None}
t = Instant::now(); sh().cycling = true
pending = Some(PendingOn{ on_at: t + off_ms ms, off_confirmed_at: t, off_ms, cause: cause.clone() })
state_if_changed(cause)
return WriteOutcome{Sent, detail {"phase":"off","off_ms":off_ms,"readback":{"out":false},
                    "on_at_mono_ns": U64(em.clock().mono_ns() + off_ms*1e6)}, error None}
```
(The IO thread sets `link.cap = Some(until)` around `write`, which bounds these queries like any write.)

**B2-S4 `fn supersede_cycle(&mut self, by: &Cause)`** — `if let Some(p) = self.pending.take()`: `sh().cycling = false`;
`row(&p.cause, EffectUnverified, {"action":"cycle","phase":"on","sent":false,"superseded_by": by.request_id})`;
`state_if_changed(by)`. No-op when nothing pending.

**B2-S5 `fn drop_cycle(&mut self, code: ErrorCode, message: &str)`** — `if let Some(p) = self.pending.take()`:
`sh().cycling = false`; `row(&p.cause, Failed, {"action":"cycle","phase":"on","sent":false,"error":{"code":code,"message":message}})`;
`state_if_changed(&p.cause)`.

**B2-S6 `fn finish_cycle(&mut self)`** (called only when `pending.on_at <= now`)
```
if closing                                   → drop_cycle(Disconnected, "disconnected during the off phase; ON not sent, output left OFF"); return
if latch.is_on() && pending.cause.origin != User → drop_cycle(Stopped, "STOP latched during the off phase; ON not sent, output left OFF"); return
if sh().link != Up                           → drop_cycle(Disconnected, "link lost during the off phase; ON not sent; output state unknown"); return
p = pending.take(); sh().cycling = false
link.cap = Some(Instant::now() + WRITE_BUDGET)
on_sent = Instant::now(); send "OUTP ON": Err → fail_io; unknown(&p.cause,"cycle",&e,&[]) ; (cap reset) return
SYST:ERR? / OUTP? exactly as cycle_off (device error → Failed Denied with device_error; query error → unknown), using p.cause
st = if b == Some(true) { EffectiveVerified } else { EffectUnverified }
row(&p.cause, st, {"action":"cycle","phase":"on","readback":{"out":b},"off_ms":p.off_ms,
                   "off_ms_actual": (on_sent - p.off_confirmed_at).as_millis() as u64, "lines":["OUTP ON"]})
link.cap = None; state_if_changed(&p.cause)
```
Every path leaves `cycling == false` and emits exactly one terminal row for the cycle request.

**B2-S7 `Io::run`** — loop body order:
1. `closing` → break.
2. `up = link == Up`; `wait = if up { next - now } else { 200 ms }`; if `pending` → `wait = min(wait, on_at - now)` (saturating).
3. `recv_timeout(wait)`: `Write` → as today (queued-1, cap, write, cap None, reply); `Timeout` → nothing yet; `Disconnected` → break.
4. if `pending.is_some_and(|p| p.on_at <= Instant::now())` → `finish_cycle()`.
5. if link is Up and `Instant::now() >= next` → `poll(); next += period; if next <= now { next = now + period }`.
After the loop, **before** answering still-queued commands: `drop_cycle(Disconnected, "disconnected during the off phase; ON not
sent, output left OFF")` (no-op when nothing pending). Polling cadence otherwise unchanged (polls keep running during OFF).
Tests: scpi.rs `b2_*` (7) + updated `unsupported_operations_are_refused_not_simulated` (off_ms usage).

### B2.5 Notes
- `cycling` feeds the existing gateway interlock `psu.cycling` (ChannelBook reads `state.cycling`) — now truthful for scpi-tcp.
- Hooks/risk unchanged: `psu.out` without `state` but with `targets` is classified as a power cut (`is_power_cut`: state ≠ "on").
- The emergency write runs with user origin (viewer) → never latched by STOP, interlocks warn only (A3) — unchanged.

---------------------------------------------------------------------------------------------------

## Batch 3 — PSU-03 engine: `psu.history` (journal-backed stats + paged rows), anchor index, CLI

### B3.0 Lease, allowlist, acceptance

Lease (Sonnet, bodies only): `crates/loom-psu/src/gateway/common.rs`, `crates/loom-psu/src/gateway/history.rs`,
`crates/loom-psu/src/gateway/evidence.rs`, `crates/loom-psu/src/cli.rs`.
Opus-owned, already in place (do not edit): all types (`Anchor`, `Seek`, `TimeIndex`, `IndexCh`, `ScanPage`, `HistStat`, `Stop`,
`PageFold`, `Window`, `History`), signatures, imports, model constants (`HISTORY_*` in model.rs), the method schema +
registration + reducer wiring + help example in `gateway.rs`, the clap `history` subcommand definition in `cli.rs`
(`command()`), the `#[allow(dead_code)]` on `anchor_of` (Opus removes it at review), tests.

| # | Unit | File |
|---|---|---|
| B3-I1 | `TimeIndex::note` | common.rs |
| B3-I2 | `TimeIndex::seek` | common.rs |
| B3-I3 | `TimeIndex::coverage` | common.rs |
| B3-P1 | `scan_page` | common.rs |
| B3-R1 | `anchor_of` | evidence.rs |
| B3-R2 | `PsuReducer::apply` — `KIND_MEAS` arm: first statement `self.index.note(&ch, anchor_of(ev));` (before the metrics check) | evidence.rs |
| B3-H1…H3 | `HistStat::add`, `merge`, `json` | history.rs |
| B3-W1 | `history_window` | history.rs |
| B3-F1 | `history_page` | history.rs |
| B3-Q1 | `history` (async handler body) | history.rs |
| B3-C1 | `PsuCli::to_call` — new `Some(("history", s))` arm | cli.rs |

Acceptance (repo root; `CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2`):
```
cargo test -p loom-psu --test psu      # baseline now 24 passed / 9 failed (all b3_*) → expect 33 passed
cargo test -p loom-psu --test scpi     # stays 16 passed
cargo test -p loom-psu --lib
cargo clippy -p loom-psu --all-targets -- -D warnings   # clean except the deliberate allow on anchor_of
cargo test -p loom-gateway --test r1   # stays 8 passed (registration change must not break the daemon)
```

### B3.1 Why this design (Opus decisions, for review context)
- Daemon journal scans accept `seq:` (durable recording when older than the hub ring), `now`, `mark:`, relative `-Nms` (ring only);
  **no wall-clock cursor**. The ring is 4 MiB per channel (≈25 min of `psu.meas`), so a relative cursor over hours would only
  report `ring_evicted`. Hence a domain-owned time→seq anchor index fed by the existing reducer (it already sees every `psu.meas`).
- One bounded page per call (existing 32 MiB budget); stats are mergeable so the UI/agent merge pages exactly (`HistStat::merge`).
- Window is wall-clock (COMMON: wall display from `ts.wall_ns`); records are matched by their own `ts.wall_ns`.

### B3.2 Index (common.rs)

**B3-I1 `TimeIndex::note(&self, ch: &str, a: Anchor)`** (hub thread: O(1) amortized, never blocks on IO)
```
lock (poisoned → return)
if !map.contains_key(ch) && map.len() >= MAX_BOOK_CHANNELS → return            (bounded; unknown extra supply ignored)
e = map.entry(ch)
match e.anchors.back():
  None                                   → push_back(a)
  Some(l) if a.wall_ns < l.wall_ns       → anchors.clear(); dropped = true; push_back(a)     (wall clock stepped back)
  Some(l) if a.epoch != l.epoch || a.wall_ns - l.wall_ns >= HISTORY_ANCHOR_EVERY_NS → push_back(a)
  _                                      → (not due)
if anchors.len() > HISTORY_ANCHORS_MAX   → pop_front(); dropped = true
```
**B3-I2 `seek(&self, ch, from_wall_ns) -> Seek`** (r3: compared at the API's millisecond resolution) — let
`key(a) = a.wall_ns / 1_000_000`, `f = from_wall_ns / 1_000_000`. No entry / empty → `Empty`; `key(front) > f` → `BeforeIndex(front)`;
else the last anchor with `key(a) <= f` (binary search, `partition_point`) → `At(that)`. Reason: window bounds are integer ms, so a
window starting at the first sample's own millisecond is fully covered; the fold still filters samples by exact `wall_ns >= from`.
**B3-I3 `coverage(&self, ch) -> (Option<Anchor>, bool)`** — `(front, dropped)`; unknown ch → `(None, false)`.

**B3-P1 `async fn scan_page(ctx, ch, from: Cursor, budget: u64) -> Result<ScanPage, DomainError>`** — same `JournalQuery` as
`scan` (same filter: channel `ch`, `SCAN_KINDS`, `max_events: SCAN_EVENTS_PER_CALL`, `until: None`), same per-call byte limit
`SCAN_BYTES_PER_CALL.min(budget - used).max(1)`. Per call: `used += scanned_bytes`, `gaps.extend(r.gaps)`, `events.extend(r.events)`.
`r.resume_from = Some(c)` and `used < budget` → continue from `c`; `Some(c)` otherwise → return with `resume: Some(c)`;
`None` → return with `resume: None`. Keep the journal's cursor as is (it carries the epoch). `scan` stays unchanged.

### B3.3 Reducer (evidence.rs)
**B3-R1 `anchor_of(ev) -> Anchor`** = `{ wall_ns: ev.ts.wall_ns.get(), epoch: ev.epoch.get(), seq: ev.seq.get() }`.
**B3-R2** as in the table (every `psu.meas`, even one without metrics, is a valid stream position).

### B3.4 history.rs

**B3-H1 `add(x)`**: `n == 0` → `min = max = x`; else min/max; `n += 1; sum += x; last = Some(x)`.
**B3-H2 `merge(o)`** (o = later page): `o.n == 0` → return; `self.n == 0` → `*self = *o`; else `min = min.min(o.min)`,
`max = max.max(o.max)`, `n += o.n`, `sum += o.sum`, `last = o.last`.
**B3-H3 `json()`**: `n == 0` → `Value::Null`; else `{"n","min","max","sum","mean": sum / n as f64,"last"}`.

**B3-W1 `history_window(p, now_wall_ns) -> Result<Window, DomainError>`**
```
int(k): absent|null → None; as_u64 → Some; else → usage(format!("{k} must be a non-negative integer (ms)"))
(from_ms, last_ms, to_ms) = int("from_ms")?, int("last_ms")?, int("to_ms")?
exactly one of from_ms / last_ms                       else usage("give exactly one of from_ms, last_ms")
last_ms == 0 || last_ms > HISTORY_MAX_SPAN_MS          → usage(format!("last_ms must be 1..={HISTORY_MAX_SPAN_MS}"))
to   = to_ms.map(|t| t.saturating_mul(1_000_000)).unwrap_or(now_wall_ns)      (r2: an explicit end is NOT clamped — a
       future end just reads up to the newest journaled sample; `now` is used only when to_ms is absent / for last_ms)
from = from_ms * 1e6 (saturating)  |  to.saturating_sub(last_ms * 1e6)
from >= to                                             → usage("empty window: from must be before to (to is clamped to now)")
to - from > HISTORY_MAX_SPAN_MS * 1e6                  → usage(format!("window longer than {HISTORY_MAX_SPAN_MS} ms"))
Ok(Window{from_wall_ns: from, to_wall_ns: to})
```
**B3-F1 `history_page(events, w, max_rows) -> PageFold`** (events in stream order; `stop` starts as `Exhausted`)
```
for e in events:
  KIND_MEAS: v = metric(voltage), i = metric(current); either missing → continue
     wall = e.ts.wall_ns
     wall >= w.to_wall_ns                     → stop = ReachedEnd; break      (only samples end a window; gap/state ts may run ahead)
     wall <  w.from_wall_ns                   → continue
     max_rows > 0 && rows.len() == max_rows   → stop = RowCap{epoch: e.epoch, seq: e.seq}; break   (sample NOT consumed)
     p = metric(power) else v*i ; v/i/p.add ; samples += 1 ; first.get_or_insert((seq, wall)) ; last = Some((seq, wall))
     max_rows > 0 → rows.push({"seq": seq, "wall_ms": wall / 1_000_000, "v": v, "i": i, "p": p})
  "source.gap" | "record.gap": from <= wall < to → losses += 1
  other kinds: ignore
```
Invariant: stats, samples, first/last, rows and losses cover exactly the consumed prefix (so pages merge without overlap).

**B3-Q1 `history(ctx, book, index, p)`**
```
ch   = resolve_channel(p, book)?
now  = SystemTime::now().duration_since(UNIX_EPOCH) → ns as u64 (error → usage("system clock before 1970"))
w    = history_window(p, now)?
rows = p.rows: absent|null → 0; as_u64 ≤ HISTORY_MAX_ROWS → it; else usage(format!("rows must be 0..={HISTORY_MAX_ROWS}"))
seek = index.seek(&ch, w.from_wall_ns);  before_index = !matches!(seek, Seek::At(_))
start = match cursor_param(p.get("cursor"))? {
          Some(c @ Cursor::Seq{..}) → c
          Some(_) → usage("cursor must be the next_cursor of a previous page (seq cursor)")
          None → match seek { At(a) | BeforeIndex(a) → Cursor::Seq{epoch: Some(a.epoch), seq: a.seq},
                              Empty → Cursor::Relative{amount: (now - w.from_wall_ns) / 1_000_000 + 1, unit: TimeUnit::Millis} } }
page = scan_page(ctx, &ch, start, SCAN_BUDGET_BYTES).await?
f    = history_page(&page.events, w, rows as usize)
next = ReachedEnd → None | RowCap{epoch, seq} → Some(Cursor::Seq{epoch: Some(epoch), seq}) | Exhausted → page.resume
(oldest, dropped) = index.coverage(&ch)
complete = next.is_none() && page.gaps.is_empty() && !before_index
Ok(json!({
  "ch": ch, "range": {"from_ms": w.from/1e6, "to_ms": w.to/1e6}, "now_ms": now/1e6,
  "samples": f.samples, "losses": f.losses,
  "stats": {"voltage": f.v.json(), "current": f.i.json(), "power": f.p.json()},
  "first": f.first.map(|(s, t)| {"seq": s, "wall_ms": t/1e6}) (null when none), "last": same,
  "next_cursor": next.map(|c| c.to_string()) (null when none),
  "before_index": before_index, "index": {"oldest_ms": oldest.map(|a| a.wall_ns/1e6), "dropped": dropped},
  "journal_gaps": page.gaps, "scanned_bytes": page.scanned_bytes, "complete": complete
}))  + "rows": f.rows   only when rows > 0
```
Semantics to keep (UI/agent rely on them): `complete` is per page and true only for a final, gap-free page whose start was
resolved by the index; a reader of several pages reports complete only if the last page has `next_cursor: null`, no page had
`journal_gaps`, and `before_index` is false. `before_index: true` means "data before `index.oldest_ms` is not included".
Device losses (`losses`) are real missing samples, not journal incompleteness.

### B3.4a Amendment r2 (answer to worker gap, 2026-11-15)
`now` = `SystemTime` (the clock the real daemon's records carry). The test fixture's SimClock wall runs ahead of real time, so
the contract no longer clamps an explicit `to_ms` to `now` (W1 above) and the result reports `"now_ms"` (Q1 above) so a reader
can state "end is after the query time; data up to the newest sample". Tests updated by Opus (`b3_history_window_rules`,
`b3_history_start_older_than_the_index…` now passes `to_ms`, whole-window test asserts `now_ms`). No new accessor/type.

### B3.4b Amendment r3 (2026-11-15)
B3-I2 compares at ms resolution (above). Only the `TimeIndex::seek` body changes; tests unchanged
(`b3_time_index_spacing_cap_and_clock_step` uses ms-aligned anchors and still holds).

### B3.5 CLI (cli.rs)
**B3-C1** arm `Some(("history", s))`:
```
p = json!({"ch": get_name(s)?})
if let Some(d) = s.get_one::<Duration>("last"): m = ms(*d); m == 0 || m > HISTORY_MAX_SPAN_MS →
      Err(UsageError::new("--last must be between 1ms and 24h")); p["last_ms"] = m
from-ms → p["from_ms"], to-ms → p["to_ms"], rows → p["rows"], cursor (String) → p["cursor"]     (only when given; no nulls)
Ok(RpcCall::new("psu.history", p))
```
Exact params asserted by `b3_cli_history_maps_to_one_page`.

### B3.6 Tests (Opus, frozen)
`tests/psu.rs`: `b3_history_whole_window_matches_the_journal`, `b3_history_pages_by_rows_without_loss_or_duplicate`,
`b3_history_start_older_than_the_index_is_reported_not_complete`, `b3_history_is_a_registered_read_with_schema`,
`b3_history_window_rules`, `b3_hist_stat_merge_equals_a_single_pass`, `b3_time_index_spacing_cap_and_clock_step`,
`b3_history_page_fold_stops_at_row_cap_and_window_end`, `b3_cli_history_maps_to_one_page`; updated `registration_is_complete`.

---------------------------------------------------------------------------------------------------

## Batch 4 — PSU-03 UI (history section, CSV) + PSU-04 UI (cycle control) + B4-X1 message fix

### B4.0 Lease, allowlist, acceptance

Lease (Sonnet, bodies only): `ui/src/panels/psu/psuHistory.ts`, `ui/src/panels/psu/gaugePanel.ts`, `crates/loom-psu/src/gateway/history.rs`.
Opus-owned, already in place: psuHistory.ts types/constants/exported signatures + private `download` signature (and its
`void download;` line — remove that line only once `download` is used), gaugePanel.ts imports (`createPsuHistory`), the
`registerPanel` spec (`cycle` action, `psu.history` query), the gateway `panel_manifest` (same), psu.css (`.psu-cycle`,
`.psu-history`, `.psu-hist-*`), all tests. No new imports are needed (psuHistory.ts may use `eng` only if you ask me to import
it — not required: stats cells use the local format below).

| # | Unit | File |
|---|---|---|
| B4-M1…M10 | `readHistoryPage`, `mergeHistStat`, `addPage`, `isComplete`, `dateClock`, `coverageText`, `historyParams`, `runHistory`, `csvText`, `download` | psuHistory.ts |
| B4-D1 | `createPsuHistory` | psuHistory.ts |
| B4-G10…G13 | `createPsuGauge` body units below | gaugePanel.ts |
| B4-X1 | `history_window`: message `"empty window: from must be before to (to defaults to now)"` (text only) | history.rs |

Acceptance (from `ui/`): `npx vitest run src/panels/psu` (baseline 51 passed / 16 failed → expect 67 passed);
`npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo` and the node counterpart
(`tsconfig.node.json`, `../work/typecheck/node.tsbuildinfo`) → exit 0; `npx eslint src/panels/psu` → clean;
`npx vitest run src/shell/moduleApps.test.tsx src/sdk/capacityLive.test.ts` → green. From repo root:
`cargo test -p loom-psu --test psu` (33 passed) and `cargo clippy -p loom-psu --all-targets -- -D warnings` (after X1).

### B4.1 psuHistory.ts pure functions

**B4-M1 `readHistoryPage(raw)`** — `o = raw?.data` when `raw` is an object with an object `data`, else `raw`. Return null unless:
`range.from_ms`, `range.to_ms` finite numbers; `samples`, `losses` non-negative integers; `next_cursor` string or null;
`before_index` boolean; `journal_gaps` array; each of `stats.voltage|current|power` is null or `{n: integer ≥ 1, min, max, sum
finite numbers, last: number|null}` (ignore `mean`); `first`/`last` null or `{seq, wall_ms}` numbers; `rows` absent or an array of
`{seq, wall_ms, v, i, p}` all finite numbers (any bad row → null). Optional: `now_ms` number else null; `index.oldest_ms` number
else null. Output field mapping as in the test (`wall_ms`→`wallMs`, `journal_gaps.length`→`journalGaps`, missing rows → `[]`).

**B4-M2 `mergeHistStat(a, b)`** (b later) — both null → null; one null → a copy of the other; else
`{n: a.n+b.n, min: min, max: max, sum: a.sum+b.sum, last: b.last ?? a.last}`. Never mutates inputs.

**B4-M3 `addPage(agg, page)`** — `agg === null` → `{fromMs, toMs, nowMs, pages: 1, samples, losses, v, i, p, first, last,
beforeIndex, indexOldestMs, journalGaps, done: page.nextCursor === null, cut: null}` from the page. Else: window/now/index/
beforeIndex/first kept from `agg` (`first = agg.first ?? page.first`); `pages+1`; samples/losses/journalGaps summed; stats via
`mergeHistStat`; `last = page.last ?? agg.last`; `done = page.nextCursor === null`; `cut` kept.

**B4-M4 `isComplete(a)`** = `a.done && a.cut === null && a.journalGaps === 0 && !a.beforeIndex`.

**B4-M5 `dateClock(ms)`** — local `MM-DD HH:MM:SS` (`getMonth()+1`, padStart 2).

**B4-M6 `coverageText(a)`** — exact (tests):
```
range  = `요청 ${dateClock(a.fromMs)} – ${dateClock(a.toMs)} · 실제 ${a.first ? dateClock(a.first.wallMs) : '—'} – ${a.last ? dateClock(a.last.wallMs) : '—'} · 샘플 ${a.samples} · 측정 손실 ${a.losses}`
status = isComplete(a) ? '완전 (요청 구간 전체 조회)' : '부분: ' + reasons.join(', ')
reasons (in order, each only when true):
   a.beforeIndex      → `색인 시작 ${a.indexOldestMs !== null ? dateClock(a.indexOldestMs) : '없음'} 이전 미포함`
   a.journalGaps > 0  → `저널 손실 ${a.journalGaps}건`
   a.cut === 'aborted' → '중지됨' | 'page_cap' → `페이지 상한 ${HISTORY_MAX_PAGES} 도달` | 'row_cap' → `CSV 행 상한 ${CSV_MAX_ROWS} 도달`
   !a.done && a.cut === null → '미완료'
text   = `${range} · ${status}` + (a.nowMs !== null && a.toMs > a.nowMs ? ` · 끝 시각이 조회 시각 이후 — ${dateClock(a.nowMs)}까지만 존재` : '')
```
**B4-M7 `historyParams(ch, q, rows, cursor)`** — `{ch}` + (`'lastMs' in q` ? `{last_ms}` : `{from_ms, to_ms}`) + (`rows > 0` ? `{rows}`)
+ (`cursor !== null` ? `{cursor}`). No undefined/null keys.

**B4-M8 `runHistory(query, ch, q, opts)`**
```
agg = null; rows = []; cursor = null
for (n = 0; ; n++):
  if opts.signal.aborted: if agg: agg = {...agg, cut: 'aborted'}; break
  if n >= opts.maxPages: agg = {...agg!, cut: 'page_cap'}; break          (maxPages ≥ 1 assumed)
  qn  = n === 0 ? q : { fromMs: agg!.fromMs, toMs: agg!.toMs }            (pin the window: a preset must not slide)
  raw = await query(historyParams(ch, qn, opts.rows, cursor), opts.signal)
  if opts.signal.aborted: if agg: agg = {...agg, cut: 'aborted'}; break   (the page that arrived after abort is discarded)
  page = readHistoryPage(raw); null → throw new Error('psu.history: unexpected result shape')
  agg = addPage(agg, page); rows.push(...page.rows); opts.onPage?.(agg)
  if opts.rows > 0 && rows.length >= opts.maxRows:
       if rows.length > opts.maxRows || page.nextCursor !== null: rows.length = opts.maxRows; agg = {...agg, cut: 'row_cap'}
       break
  if page.nextCursor === null: break
  cursor = page.nextCursor
return { agg, rows }
```
Query errors propagate (caller shows them). No timers, no retries.

**B4-M9 `csvText(ch, a, rows)`** — `q(s)` = CSV quote when `s` contains `"`, `,`, `\n` (wrap in `"`, double inner quotes).
Line 1: `# loom psu history,ch=${ch},from=${iso(a.fromMs)},to=${iso(a.toMs)},samples=${a.samples},rows=${rows.length},losses=${a.losses},complete=${isComplete(a)},` + `q('coverage=' + coverageText(a))`;
line 2 `seq,wall_ms,wall_iso,v,i,p`; one line per row `${seq},${wallMs},${iso(wallMs)},${v},${i},${p}` (`String(n)` numbers,
`iso = new Date(ms).toISOString()`); joined with `\n` and ending with `\n`.

**B4-M10 `download(name, text)`** — same as the UDS console helper (`ui/src/panels/uds/consolePanel.ts:136`): no
`URL.createObjectURL` → false; Blob(`text/csv`), `<a href download=name>` click, revoke after 1 s via `setTimeout`, try/catch → false.

### B4.2 createPsuHistory (B4-D1) — DOM contract (all text via textContent)
```
root    section.psu-history [data-testid=psu-history] aria-label='측정 기록 조회 (저널)'
head    div.psu-hist-head:
          span.psu-hist-title '기록 조회 (저널)'
          per HISTORY_PRESETS: button [data-testid=psu-hist-preset-<id>] text=label, title=`최근 ${label} — 저널에 기록된 psu.meas 통계`
          label '시작' + input[type=datetime-local][data-testid=psu-hist-from]; label '끝' + input[type=datetime-local][data-testid=psu-hist-to]
          button [data-testid=psu-hist-range] '구간 조회'
          button [data-testid=psu-hist-csv] 'CSV' title=`마지막 조회 구간의 측정 행을 CSV로 (최대 ${CSV_MAX_ROWS}행, 브라우저 다운로드)`
          button [data-testid=psu-hist-stop] '중지' hidden while idle
status  div.psu-hist-status [data-testid=psu-hist-status] role=status; initial '구간을 고르면 저널에 기록된 측정으로 통계를 냅니다'
cover   div.psu-hist-coverage [data-testid=psu-hist-coverage]; class 'partial' toggled = !isComplete(agg)
stats   table.psu-hist-stats [data-testid=psu-hist-stats] hidden until a result; thead '', '최소','최대','평균','마지막';
        tbody rows tr[data-testid=psu-hist-row-v|i|p] cells: label ('전압'|'전류'|'전력'), min, max, mean (sum/n), last;
        cell text = `${x.toFixed(3)} ${unit}` (unit V|A|W), '—' for null stat / null last
state   running: AbortController|null; lastWindow: {fromMs,toMs}|null; disposed: boolean
run(q, mode 'stats'|'csv'):
   if running → return; running = new AbortController; presets/range/csv disabled; stop shown
   status = mode==='csv' ? 'CSV 행 읽는 중…' : '조회 중…'
   r = await runHistory((p, s) => ctx.query('psu.history', p, s), ch, q, {rows: csv ? HISTORY_PAGE_ROWS : 0,
         maxPages: HISTORY_MAX_PAGES, maxRows: CSV_MAX_ROWS, signal, onPage: (a) => status = `${csv ? 'CSV 행 읽는 중' : '조회 중'} — ${a.pages}쪽 · 샘플 ${a.samples}`})
   error → (unless disposed) status = `조회 실패: ${e instanceof Error ? e.message : String(e)}`; stats/coverage untouched
   r.agg === null → status '중지됨 — 결과 없음'
   else: lastWindow = {fromMs: agg.fromMs, toMs: agg.toMs}; coverage + stats rendered from agg
        stats mode → status = agg.cut === 'aborted' ? '중지됨 — 부분 결과' : `${agg.pages}쪽 조회 완료`
        csv mode   → ok = download(`psu-${ch}-${agg.fromMs}.csv`, csvText(ch, agg, r.rows))
                     ok → toast + status `CSV 다운로드 요청됨 — 브라우저 다운로드 위치를 확인하세요 (${r.rows.length}행${isComplete(agg) ? '' : ', 부분'})`
                     !ok → toast + status '내보내기 실패: 이 환경에서 다운로드를 만들 수 없음'
   finally (unless disposed): running = null; buttons enabled; csv.disabled = lastWindow === null; stop hidden
preset click → run({lastMs: p.ms}, 'stats')
range click  → f = new Date(from.value).getTime(), t = new Date(to.value).getTime(); !finite || f >= t || t - f > 86_400_000 →
               status '시작/끝 시각을 확인하세요 (시작 < 끝, 최대 24시간)', no RPC; else run({fromMs: f, toMs: t}, 'stats')
csv click    → lastWindow && run(lastWindow, 'csv')          (exact same window as the stats the user saw)
stop click   → running?.abort()
dispose()    → disposed = true; running?.abort()
```
Read-only: no rights check (a query). Never a timer, never auto-refresh.

### B4.3 gaugePanel.ts body units
**B4-G10 history mount** — bound only: `history = createPsuHistory(ctx, bound)`; `root.append` order becomes
`connect?, head, placeholder?, vals, ctl, trend, history?, foot`; `dispose()` calls `history?.dispose()` before `root.remove()`.
**B4-G11 trend note** — text `'실측 추세 · 60 s 화면 창 (긴 구간은 아래 기록 조회)'`.
**B4-G12 cycle control** — in `ctl`, after `pending`: `cyc = tid(el('span','psu-cycle'),'psu-cycle-wrap')` containing
`el('span','psu-setlabel','OFF ms')`, `cycMs = tid(input,'psu-cycle-ms')` (`inputMode 'numeric'`, `value '1000'`, `autocomplete off`,
`aria-label 'OFF 시간 (ms)'`), `cycBtn = tid(button '전원 사이클','psu-cycle')` (`type=button`), `el('span','risk risk-R3','R3')`.
`validMs()` = `/^\d+$/.test(v.trim())` and `1 ≤ n ≤ 60000` → number, else null. On `input`: `aria-invalid = String(validMs() === null)`
then `ctx.scheduler.invalidate()`. In `draw()`: `cycMs.disabled = !op`; `cycBtn.disabled = !op || !confirmed || state.out !== true ||
state.cycling === true || validMs() === null`; text `state.cycling === true ? '사이클 중…' : '전원 사이클'`; title:
cycling → 'OFF 구간 — ON 예정 (STOP·다른 출력 조작·링크 손실 시 ON을 보내지 않음)'; `state.out !== true` → '출력 ON 상태에서만 사이클';
else `출력 OFF → ${validMs() ?? '?'} ms → ON (장치 작업자가 수행, R3)`. Also set `aria-invalid` in draw (keeps it right after typing).
Click: same guard as `disabled` (re-evaluated), then `void act('cycle', { channel: bound, off_ms: n })`. Enter in the field never
sends (existing root keydown ignores INPUT; add nothing).
**B4-G13 nothing else** — emergency/output/set/status/limits/readout logic unchanged.

### B4.4 Tests (Opus, frozen)
psu.test.ts: `PSU-04 power cycle control` ×3, `psu history model (B4)` ×9, `PSU-03 history section in the gauge` ×4,
updated `60 s observation lanes…` (note text) and `offers no unsupported control…` (inputs list). Rust: manifest assertion in
`tests/psu.rs` (already green: static metadata).

## Batch 5 — FIRST3 runtime corrections (ROOT-authorized 2026-11-16; evidence RUNTIME.md, ROOT-UI-OFF-FAILURE.md)

Base: the imported FIRST3 snapshot of this worktree (diff sha256 b506d9d1…374b43). The return is incremental against it.
No shared/core file changes: everything below is in loom-psu or the PSU panel. `crates/loom-gateway/tests/r1.rs` gets
Opus test additions only, as in R2.

### B5.0 Lease, allowlist, acceptance

**5a (authorized: R5-OFF, F1, F2, F4)** Sonnet, bodies only:
| # | Unit | File |
|---|---|---|
| B5-G1 | `Psu::register` body: register `psu.off` + its dry-run preview | `crates/loom-psu/src/gateway.rs` |
| B5-H1 | `impl Handler for Off` → `call` body | `crates/loom-psu/src/gateway/handlers.rs` |
| B5-K1, K2 | `is_power_cut` body; `PsuHooks::classify` new match arm | `crates/loom-psu/src/gateway/hooks.rs` |
| B5-M1, M2 | `Limits::requested`, `config_mismatch` bodies | `crates/loom-psu/src/model.rs` |
| B5-W1, W2 | `PsuWorker::connect` (sim) and `connect_scpi` bodies | `crates/loom-psu/src/worker.rs` |
| B5-S1…S3 | `ScpiDevice::limits` body; both `state_payload` bodies; three message strings | `crates/loom-psu/src/scpi.rs` |
| B5-U1 | `createPsuGauge` body: the `emergencyOff` closure | `ui/src/panels/psu/gaugePanel.ts` |
| B5-C3 | `scan` body: the resume value when no event was returned (F3) | `crates/loom-psu/src/gateway/common.rs` |
**5b (F5 — awaiting ROOT authorization; NOT in the 5a lease):** B5-C1 `scan_page` body and B5-C2 (the `E_SCAN_LIMIT` tolerance inside `scan`) in `crates/loom-psu/src/gateway/common.rs`.

Opus-owned, already in place:
- skeletons: the `Off` struct and `impl`; the `Limits::requested`, `config_mismatch` and `ScpiDevice::limits` signatures, each with a `todo!("B5-…")` body you replace;
- the gateway.rs import of `Off`;
- static metadata: the `panel_manifest` emergencyOff `rpc` → `psu.off`, the UI `registerPanel` spec (same), the `risk_table` row for `psu.off`, and the `help()` note;
- comments; all tests; SKILL.md (F3).

Do not touch imports, types, signatures, tests, CSS or docs. If a unit needs any of those, stop and write a gap in B5-WORKER.md.

Acceptance (repo root, `CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2`):
- `cargo test -p loom-psu --test psu`, `--test scpi`, `--lib`: all green except the 5b tests named `b5_f5_*`, which stay red until 5b (2 tests).
- `cargo clippy -p loom-psu --all-targets -- -D warnings`: clean, except `todo!` in the 5b units.
- `cargo test -p loom-gateway --test r1 psu_`: green.
- From `ui/`: `npx vitest run --maxWorkers=2 src/panels/psu` green. Targeted runs only: the full UI suite is ROOT-only at the
  final freeze (ROOT resource rule, COMMON); never weaken a timeout or assertion.
- Isolated `tsc` exits 0: `npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo` and the node counterpart.
- `npx eslint src/panels/psu` clean.

### B5.1 R5-OFF — fixed-OFF endpoint `psu.off` (watch viewer emergency OFF; ROOT-UI-OFF-FAILURE.md)

Why: the gauge `!` (manifest `right: watch`) called `psu.out`, whose role set is `RoleSet::WRITE` (viewer needs operate). The daemon
refuses it before the handler (`daemon.rs` role guard): "this role may not call this command". The fix does **not** widen
`psu.out`. Instead, a new domain write command that can only switch one supply OFF:
- `psu.off{ch, dry_run?}`, `write_class: Request`, **roles `RoleSet::READ`** (agent, script, viewer ≥ watch).
- It goes through the normal write pipeline (`write_request`, recovery=false). That gives:
  - audit/write rows, with origin `user` for a viewer;
  - policy deny patterns still apply;
  - STOP latch: agent/script refused (`E_STOPPED`), user passes (existing core rule `origin != User`);
  - interlocks via the hooks (K1/K2): agent/script refused (`E_LOCKED`), the user is warned;
  - the generation check stays agent-only.
- It can never switch anything ON: there is no `state` param, and any other key is refused.
- Risk row (Opus): `psu.off` R3 "output OFF (fixed-OFF endpoint; any viewer incl. watch)".

**B5-G1 `Psu::register`** — in the `commands` vec, after the `psu.out` entry, add a `spec("psu.off", true, Arc::new(Off(hooks.clone())),
obj(json!({"ch": ch, "dry_run": dry}), &["ch"]), DEFAULT_WAIT)`, with `roles` set to `RoleSet::READ` on that value before it goes
into the vec (e.g. `CommandSpec { roles: RoleSet::READ, ..spec(…) }`). Add `"psu.off"` to the `dry_run_preview` loop list
(READ). Command order is asserted: `…, "psu.out", "psu.off", "psu.cycle", …`; `write_commands` `["psu.connect", "psu.set", "psu.out",
"psu.off", "psu.cycle"]`.

**B5-H1 `Off::call(ctx, params)`**, in this order:
1. If `params` is not an object → `usage("psu.off takes {ch}")`.
2. Any key other than `ch`, `channel`, `dry_run` → `usage("psu.off only switches one supply OFF: give ch (state/targets are not accepted)")`.
   Nothing is sent.
3. `let ch = param_ch(&params).ok_or_else(|| usage("psu.off needs ch"))?` (no implicit channel, no all-supplies).
4. If `dry(&params)` → `Ok(preview(ctx, &self.0, "psu.off", ch, &params, out_op(ch, "off")))`.
5. `let rid = ctx.request_id().unwrap_or("psu-off").to_owned();` then `write_to(ctx, ch, &rid, out_op(ch, "off")).await`
   (same op and rows as `psu.out{ch,state:"off"}`: `action: "out", state: "off"`).

**B5-K1 `is_power_cut(method, args)`** — true for `psu.cycle`, for `psu.off`, and for `psu.out` unless `state == "on"`.
**B5-K2 `PsuHooks::classify`** — new arm before the `m =>` fallback: `"psu.off" => (Risk::R3, "output OFF (fixed-OFF endpoint)".to_owned(), None)`.
The interlock text appended for power cuts then applies unchanged.

**B5-U1 gaugePanel.ts `emergencyOff`** — `if (bound !== null) void act('emergencyOff', { channel: bound });` (no `state`).
`act` resolves the manifest action, which now names `rpc: 'psu.off'` (Opus).

### B5.2 F1 + F2 — connect config (RUNTIME.md F1/F2)

**B5-M1 `Limits::requested(v: &Value) -> Result<Limits, String>`** — the limits a connect asks for. `from_value` stays as is (state payloads).
- For `k` in `vmax` (unit `V`, default `DEFAULT_VMAX`) and `imax` (`A`, `DEFAULT_IMAX`):
  - absent or `null` → the default;
  - not a number, not finite, or ≤ 0 → `Err(format!("{k} must be a positive number"))`;
  - `> default` → `Err(format!("{k} {x} {unit} is above the profile limit {def} {unit}; only loom.toml raises it"))`;
  - else `x`.
- Numbers are formatted with `{}` (f64 Display: `20`, `16`, `12.5`).

**B5-M2 `config_mismatch(have: &Limits, args: &Value, new_only: &[&str]) -> Option<String>`** — why a connect to an
already-connected alias asks for a different config; `None` = same.
- Only keys the caller **gave** count; an omitted key is no requirement.
- Order: `vmax`, `imax` (number given and `!= have.<k>` → `format!("{k} {req} differs from the connected {have} {unit}")`),
  then each key in `new_only` that is present and non-null → `format!("{k} applies only to a new connection")`.
- First difference wins.

**B5-W1 sim `connect`** and **B5-W2 `connect_scpi`** — the very first step (before any existing check, any IO, any generation
change): `let limits = Limits::requested(args).map_err(|m| err(ErrorCode::Usage, m))?;`.
- **F2:** above-profile or invalid is `E_USAGE`, exit 2. Nothing is connected and nothing is sent.
- **Sim:** use `limits` where `Limits::from_value(args)` was. In the same-alias branch (`d.ch == ch`):
  - `config_mismatch(&d.sim.limits, args, &["seed", "load"])` = `Some(why)` → `Err(err(ErrorCode::PortBusy, format!("{ch} is already connected: {why}; disconnect first")))`;
  - else `Ok` of `d.status()` with `"already_connected": true` inserted.
- **SCPI:** pass `limits` to `scpi::connect` (was `Limits::from_value(args)`). In the `link Up && address == addr` branch:
  - `config_mismatch(&e.limits(), args, &[])` = `Some(why)` → the same `PortBusy` error;
  - else `Ok` of `e.status(now)` with `"already_connected": true`.
- A fresh connect (no existing device) does **not** carry `already_connected`.

**B5-S1 `ScpiDevice::limits(&self) -> Limits`** — returns the field.

### B5.3 F4 — truthful current vs last-verified output (RUNTIME.md F4)

**B5-S2** both `state_payload` bodies in scpi.rs (`Io`, ~l.491, and `ScpiDevice`, ~l.1383):
- `"out_known": s.out.is_some() && s.link == LinkState::Up` — no current reading without a live link;
- new key `"out_last_verified": s.out` (bool | null), the last value read back;
- `out` and `mode` unchanged.

**B5-S3** messages (exact):
- the two disconnect messages (`finish_cycle` closing branch and after the `Io::run` loop):
  `"disconnected during the off phase; ON not sent; last verified output OFF (link closed, current state not observed)"`;
- `finish_cycle` link branch: `"link lost during the off phase; ON not sent; last verified output OFF, current state unknown"`.

UI is unchanged: `psuModel` already shows `out_known:false` as unknown, never OFF.

### B5.4 F3 — wait after a write (Opus: SKILL.md + help note; tests) + B5-C3

The default `wait_for` look-back (`since` = now − (hold + 1 s)) is a contract other callers rely on (evidence windows), so it stays.
After a write, agents pass `--since now`. A new cursor field on write results was considered and rejected: `now` at wait time is
exact once the write returned.

**Defect found while verifying (real daemon, 2026-11-16, `runtime/96-wait-since-now.json`):**
- `wait-for upqa-psu-a --since now --curr-lt 1 --hold 600ms --timeout 5s` → `E_TIMEOUT`, **0 samples**.
- The same wait with the default look-back, or with `--since seq:<hw>`, matched.
- Cause: `wait_for` advances `from` only when `scan` returns `Some(next)`, and `scan` returns `None` when no event came back. So
  `Cursor::Now` is re-resolved to the newest seq on every poll, and a sample published between polls is never inside the range.

**B5-C3 `scan` (common.rs)** — keep `through = max(through, r.scanned_through)` over the loop. In both return arms the resume
value is `out.last().map(|e| e.seq.get() + 1)`, else `(through > 0).then_some(through + 1)`. The daemon ring sets
`scanned_through` to the upper bound (high-water mark) on a complete read even with no matching event, so `now` is pinned once.
`watch` ignores this value; `wait_for` then continues from a fixed seq.

### B5.5 F5 — durable-path paging (5b, awaiting authorization)

The real daemon reader returns `Err(DomainError{code: E_SCAN_LIMIT, detail: {resume_from: "<cursor>", partial: [PublishedRecord…],
scanned_bytes: "<n>"}})` when its byte budget is reached (`loom-gateway/src/daemon/core_rpc.rs` `journal_read_bounded`).

**B5-C1 `scan_page`** — on `Err(e)` with `e.code == ErrorCode::ScanLimit` and both of these hold:
- `e.detail["resume_from"]` is a string that parses as `Cursor`;
- `e.detail["partial"]` deserializes as `Vec<PublishedRecord>`.

Then: extend `page.events` with `partial`, add `scanned_bytes` (string or number, else 0) to `used`/`page.scanned_bytes`, set
`page.resume = Some(cursor)` and return `Ok(page)`. Any other error, or either condition failing, → return `Err(e)` unchanged.

**B5-C2 `scan`** — the same tolerance; the loop continues from the cursor while budget remains, as with `resume_from`.

### B5.6 Tests (Opus, frozen)

| Test | Proves |
|---|---|
| psu.rs `registration_is_complete` (updated) | `psu.off` registered (order, write class, roles READ, preview READ); manifest emergencyOff → `psu.off`, right watch |
| psu.rs `b5_off_is_fixed_off_and_alias_scoped` | `psu.off{ch}` as user: only that supply OFF, other stays ON; op = `out_op(ch,"off")` |
| psu.rs `b5_off_refuses_any_on_or_extra_param` | `state`, `targets`, extra key, missing ch → E_USAGE, nothing sent |
| psu.rs `b5_off_under_stop_user_passes_agent_refused` | STOP: viewer OFF ok, agent `E_STOPPED` |
| psu.rs `b5_off_hooks_interlock_and_risk` | classify R3; interlock refuses agent `psu.off`, warns user |
| psu.rs `b5_f1_reconnect_same_alias` | changed vmax/imax → E_PORT_BUSY, limits kept; same/omitted → `already_connected`; seed on reconnect → E_PORT_BUSY |
| psu.rs `b5_f2_limit_above_profile_is_usage` (and updated `over_limit_is_refused_not_clamped`) | vmax 20 / imax 6 / vmax 0 / "x" → E_USAGE, no device |
| psu.rs `b5_f3_wait_since_now_ignores_samples_before_the_write` | default look-back matches pre-write samples; `since: "now"` does not |
| psu.rs `b5_f3_scan_from_now_returns_a_resume_seq` | B5-C3: `scan(Now)` with no event → `Some(hw + 1)`; later samples are seen from it |
| psu.rs `b5_f5_history_pages_through_journal_scan_limit` (5b) | E_SCAN_LIMIT shape → pages to null cursor, stats equal to an unlimited read |
| psu.rs `b5_f5_wait_for_over_a_scan_limited_journal` (5b) | `wait_for` with an old `since` still matches |
| scpi.rs `b5_f1_reconnect_changed_limits_is_refused_without_a_second_connection` | one accept, limits unchanged |
| scpi.rs `b5_f2_above_profile_is_refused_before_any_tcp_connect` | zero accepts |
| scpi.rs `b5_f4_link_lost_reports_last_verified_not_current` | `out_known:false`, `out_last_verified`, message text |
| scpi.rs `b2_disconnect…`/`b2_link_lost…` (message asserts added) | exact B5-S3 strings |
| r1.rs `psu_watch_viewer_fixed_off_is_alias_scoped` | real daemon role guard: **watch** viewer `psu.off{ch:pa}` ok, pa OFF, pb ON; watch `psu.out` on/off still `E_DENIED` |
| psu.test.ts emergency OFF tests (updated) | `psu.off {channel}` only |

### B5.7 Amendment r2 — 5a ACCEPTED; 5b AUTHORIZED (ROOT 2026-11-16)

5a review (Opus re-run): psu 41/2 (only `b5_f5_*` red), scpi 19, lib 2, clippy clean, r1 `psu_` 3, vitest `--maxWorkers=2` 67,
tsc app/node 0, eslint 0. The diff matches B5-G1…C3 exactly.

ROOT conditions for 5b: preserve partial records, the resume cursor and the byte budget from the real `E_SCAN_LIMIT`; never claim
complete on an interrupted page; malformed details propagate unchanged; guard against a non-advancing resume (no loop).

**5b lease (Sonnet, bodies only):**
| # | Unit | File |
|---|---|---|
| B5-C1 | `scan_page` body | `crates/loom-psu/src/gateway/common.rs` |
| B5-C2 | `scan` body (adds the tolerance; keep B5-C3) | `crates/loom-psu/src/gateway/common.rs` |
| B5-U2 | `createPsuGauge` body: the bound `emergency.title` text | `ui/src/panels/psu/gaugePanel.ts` |

**Tolerance rule (C1 and C2 identical)** — on `Err(e)` from `ctx.journal().scan(..)`:
1. Usable only if all of these hold:
   - `e.code == ErrorCode::ScanLimit`;
   - `e.detail["resume_from"]` is a string that parses as `Cursor` → `cur`;
   - `e.detail["partial"]` deserializes as `Vec<PublishedRecord>` → `partial`.
   Otherwise `return Err(e)` **unchanged** (same code, message and detail).
2. **Non-advancing guard:** `partial.is_empty() && cur == from` (the `from` of this call) → `return Err(e)` unchanged.
   A budget hit that made no progress is reported, never retried.
3. `bytes` = `e.detail["scanned_bytes"]`, as a decimal string or a number, else 0. Then `used = used.saturating_add(bytes)`.
4. **C1 `scan_page`:** extend `page.events` with `partial`, set `page.scanned_bytes = used`, set `page.resume = Some(cur)`, and return
   `Ok(page)`. The page ends here: `history` then reports `next_cursor` and `complete: false`. No history.rs change.
5. **C2 `scan`:** extend `out` with `partial` (set `through` from the last partial seq if any), then continue exactly like
   `resume_from = Some(cur)`:
   - `used < budget` → `from = cur`, loop again;
   - else return `(out, resume, true)`, with resume per B5-C3, falling back to `cur`'s seq when `out` is empty and `cur` is a `Seq`.

**B5-U2** bound title, exact: `` `${bound} 채널만 확인 없이 출력 OFF 요청 (보기 자격 가능 · OFF 전용 · 인터록은 경고만) — 정책이 막으면 거부될 수 있습니다` ``.
It no longer promises an unconditional OFF. The unbound title is unchanged.

**Tests (Opus, frozen):**
| Test | Proves |
|---|---|
| `b5_f5_history_pages_through_journal_scan_limit` (+ asserts) | every interrupted page `complete: false` with `next_cursor`; merged samples/n equal an unlimited read; `scanned_bytes` > 0 |
| `b5_f5_wait_for_over_a_scan_limited_journal` | `wait_for` from an old `since` matches through budget hits |
| `b5_f5_malformed_scan_limit_propagates_unchanged` | detail without `resume_from` → history and wait_for return that exact error |
| `b5_f5_non_advancing_scan_limit_is_an_error_not_a_loop` | resume == from, no partial → `E_SCAN_LIMIT` returned promptly |
| psu.test.ts emergency OFF test (+ title assert) | B5-U2 text |
FakeJournal test modes: `limit_err_shape` = `""` (daemon shape), `"malformed"` (detail `{partial, scanned_bytes}` only),
`"stuck"` (resume_from = the query's `from`, empty partial).

Acceptance: as B5.0, with **all** `b5_*` green.

## Batch 6 — history provenance (ROOT finding after the B5 runtime proof; lease granted 2026-11-16)

Why: GUI CSV rows carried `seq,wall_ms,wall_iso,v,i,p` only. `seq` restarts per stream epoch, and this owned journal already holds two
epochs (1794774986, 1794778200), so an exported row could not point back to its canonical journal record. COMMON requires historic
provenance. B5 is already imported: this batch is incremental on top of it.
Scope: `crates/loom-psu/src/gateway/history.rs`, `ui/src/panels/psu/psuHistory.ts`, and their tests. CSS (Opus, static) is ROOT's
second finding. No index persistence, no shared schema change, `psu.history` params unchanged.

### B6.0 Lease (Sonnet, bodies only), acceptance
| # | Unit | File |
|---|---|---|
| B6-H1 | `history_page` body: the raw row object | `crates/loom-psu/src/gateway/history.rs` |
| B6-M1 | `readHistoryPage` body: the row loop | `ui/src/panels/psu/psuHistory.ts` |
| B6-M2 | `csvText` body: header and row line | `ui/src/panels/psu/psuHistory.ts` |

Opus, already in place:
- the `HistRow` type (new fields);
- tests (`psu.rs` `b6_*`, `psu.test.ts` updated and new);
- `psu.css` (datetime-local width; no unit test, ROOT re-checks with screenshots).

Do not edit imports, types, signatures, tests, CSS or docs.

Acceptance (`CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2`):
- `cargo test -p loom-psu --test psu` all green;
- `cargo clippy -p loom-psu --all-targets -- -D warnings` clean;
- `cargo fmt -p loom-psu --check` clean;
- from `ui/`: `npx vitest run --maxWorkers=2 src/panels/psu` green; isolated app/node `tsc` 0; `npx eslint src/panels/psu` 0.

### B6.1 Engine — B6-H1 (history_page row)
Each raw row is built from **that** record `e`. Existing keys stay unchanged; three are added:
```
{"seq": seq, "wall_ms": wall / 1_000_000, "v": v, "i": i, "p": p,
 "ref": Cursor::Seq { epoch: Some(e.epoch.get()), seq }.to_string(),   // canonical "e<epoch>:seq:<seq>"
 "generation": e.generation,                                         // Option<u32> → number | null
 "clock_id": e.ts.clock_id}                                          // that record's clock
```
- Never take these from current state, the book or the page.
- Statistics, `first`/`last` and paging are unchanged.

### B6.2 UI — psuHistory.ts
**Type (Opus):** `HistRow` gains `ref: string | null`, `generation: number | null`, `clockId: string | null`.

**B6-M1 `readHistoryPage` row loop** — the existing required checks (`seq, wall_ms, v, i, p` finite, else the page is `null`) stay.
The new fields are optional and never reject a page (older daemons send none):
- `ref` = `r.ref` if it is a string matching `/^e\d+:seq:\d+$/`, else `null`;
- `generation` = `r.generation` if it is an integer ≥ 0, else `null`;
- `clockId` = `r.clock_id` if it is a non-empty string, else `null`.
- Push `{ seq, wallMs, v, i, p, ref, generation, clockId }`.

**B6-M2 `csvText`** — the metadata line is unchanged.
- Header: `seq,wall_ms,wall_iso,v,i,p,ref,generation,clock_id`.
- Row: the existing six values, then `,${q(r.ref ?? '')},${r.generation ?? ''},${q(r.clockId ?? '')}`. A missing value is an empty cell, never a guess.

### B6.3 Tests (Opus, frozen)
| Test | Proves |
|---|---|
| psu.rs `b6_history_rows_carry_the_records_own_provenance` | every row's `ref`/`generation`/`clock_id` equal its journal record's (by seq) |
| psu.rs `b6_same_seq_in_two_epochs_has_distinct_refs` | `history_page` over two records with equal seq, different epoch/clock → distinct `ref`, own `clock_id` |
| psu.test.ts readHistoryPage (updated) | new fields parsed; absent (old daemon) → `null`; wrong type / non-canonical ref → `null`, page kept |
| psu.test.ts csvText (updated) | 9-column header; provenance values; old rows → empty cells |
| psu.test.ts CSV download (updated) | header line in the downloaded blob |

### B6.4 CSS (Opus, ROOT finding 2)
`.psu-tile .psu-field input { width: 6.5em }` also caught the history datetime-local inputs, which then showed only year/month.
New rules, history head only (setpoint widths unchanged):
- `.psu-tile .psu-hist-head .psu-field` → `flex-wrap: wrap; max-width: 100%; min-width: 0`;
- `.psu-tile .psu-hist-head .psu-field input[type='datetime-local']` → `width: 16em; max-width: 100%; min-width: 0`.

This shows the full date and time, and wraps in narrow panes without horizontal overflow.
