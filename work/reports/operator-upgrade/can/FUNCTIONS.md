# CAN — FUNCTIONS (function contracts)

Status: **B1 = ACCEPTED (Opus review, §B1-REVIEW)** · **B2a = ACCEPTED** · **B2b = ACCEPTED** · **B2c = ACCEPTED (B2 done)** · **B4a = ACCEPTED** · **B4b = ACCEPTED (B4 done)** · **B3a = ACCEPTED** · **B3b = ACCEPTED (B3 done)** · **B5a = ACCEPTED** · **B5b = ACCEPTED** — ALL BATCHES ACCEPTED, READY_TO_INTEGRATE (STATUS.md) · B5 schema fix done by Opus (§B5-SCHEMA). B1 history: contract was frozen at scaffold sha256 08b5df54…ac41. Worker returns `/Users/ctmctm/workspace/Loom/.worktrees/operator-can/work/reports/operator-upgrade/can/B1-WORKER.md` (changed files, final hash with the same command, exact outputs of the 4 acceptance commands, any lease-return reasons) (CAN-1 time column, CAN-3 persisted TX list). B2–B5 will be added below; they are not leased yet.
Root for all paths: `/Users/ctmctm/workspace/Loom/.worktrees/operator-can/ui/src/panels/can/` (abbreviated `can/`).

> **Resource rule (ROOT, 2026-11-16, applies to every batch from B3b on):** every targeted Vitest command in this file runs with `--maxWorkers=2` (e.g. `npx vitest run src/panels/can --maxWorkers=2`). The full UI suite is ROOT-only at the final integrated freeze. Cargo keeps `CARGO_BUILD_JOBS=2`. No assertion/timeout weakening; already-passed checks are not re-run just to add the flag. The one earlier load-sensitive CAN flake (B4b review) is consistent with this host-load finding.

## 0. Rules for the B1 worker (Sonnet)

- Write only the function and callback bodies in the allowlist (§B1-ALLOW). Opus already wrote the top-level scaffold: imports, types, interface fields, class field declarations, constants, signatures and CSS. Do **not** change any of those, any test file, or `can.css`.
- Stubs are `throw new Error('UNIMPLEMENTED(B1): …')` with `void […]` placeholder lines. Replace the whole stub body. Leave no `UNIMPLEMENTED` and no `void [...]` placeholder lines.
- If you need a missing import/type/constant/helper, or a contract is ambiguous: stop, return the lease to Opus with the exact reason. Do not invent.
- Text from devices or imports → `textContent` / `.value` only, never `innerHTML`.
- No timers, intervals or polling. No new dependencies. No Git. No GUI or device IO.
- Acceptance (run in `/Users/ctmctm/workspace/Loom/.worktrees/operator-can/ui`). Typecheck is ISOLATED: node_modules is shared across worktrees, so never use the default `tsc -b` cache (COMMON.md validation concern).
  1. `npx vitest run src/panels/can` → 139 tests, 0 failed (the 84 pre-existing ones stay green).
  2. `mkdir -p ../work/typecheck && npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo` → 0 errors.
  3. `npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo` → 0 errors.
  4. `npx eslint src/panels/can` → 0 problems.
  Scaffold state (Opus measured, before bodies): vitest 55 failed / 84 passed; app tsc 33 errors (missing fields, definite assignment, fillRow arity, missing hook lambdas — all resolved by the bodies); node tsc 0; eslint 16 unused-symbol errors (resolved when bodies use the scaffolded imports/params). Report exact outputs.

Scaffold hash (Opus, final pre-lease, includes §B1-SCAFFOLD-FIX): `(git diff -- ui/src/panels/can; for f in $(git ls-files --others --exclude-standard ui/src/panels/can | sort); do echo "== $f"; cat "$f"; done) | shasum -a 256` → `08b5df54bc0fae48021f7da3c85915d24a086e18677ec71cda40c2283eb1ac41`.

---

## B1-ALLOW — exclusive file lease and allowlisted bodies

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `kv.ts` | `defaultKV` |
| `timeFmt.ts` | `wallMsOf`, `ClockDict.index`, `ClockDict.idOf`, `ClockDict.size` (storage fields `ids: string[]` and `map: Map<string, number>` are already declared by Opus — use them, add no fields), `timeCell`, `refLabel`, `readTimeMode`, `writeTimeMode` |
| `txStore.ts` | `cansendText`, `txDoc`, `txDocText`, `parseTxDoc`, `storeKey`, `loadTxList`, `saveTxList` |
| `traceModel.ts` | `bytesForRows` (body), `CanTraceModel.constructor` (allocate `wall`, `clk`), `push`, `appendRow`, `applyFrame` (merge path only), `applyGapEvent`, `addGap`, `wallAt`, `clockAt`, `clockIdOf`, `timeAt` |
| `traceView.ts` | `TraceView.constructor` (time header + click listener), `render` (header text + pass `idx`), `fillRow`, `select`, `get selected`, `prevShownFrame`, `onClick`, `dispose` |
| `rxSummary.ts` | `RxSummary.apply` |
| `txList.ts` | `TxList.upsert`, `TxList.remove`, `TxList.load` |
| `rxtxView.ts` | `RxTxView.constructor` (new DOM), `renderRx` (col 6 title only), `commitEditor`, the existing delete click handler and Delete-key branch (one added call each), `setPersist`, `openExport`, `openImport`, `closeIo`, `applyImport`, `sayIo` |
| `canTrace.ts` | `createCanTrace` body only: the closures listed in §B1-PANEL (`onTimeMode`, `onSetRef`, `onResetRef`, `timeRef`, `persistLoad`, `persistTick`), the DOM additions, the TraceView/RxTxView option lambdas, and calls in the frame callback / `updateStatus` / `dispose` |

Structural edit owner: Opus (scaffold already in place). The lease covers exactly these 9 files. Tests (`timeFmt.test.ts`, `txStore.test.ts`, `traceTime.test.ts`, `canTime.test.ts`, `txPersist.test.ts`) are Opus-owned and read-only for Sonnet.

---

## B1-KV — `kv.ts`

`defaultKV(): KV | null`
- Returns `globalThis.localStorage` if `typeof localStorage !== 'undefined'`, else `null`. Wrap the access in try/catch: a throw (SecurityError) → `null`.
- No other side effects. Precedent: `ui/src/store/tabSettings.ts defaultStorage()`.

---

## B1-TIME — `timeFmt.ts` (pure)

**`wallMsOf(ev: Pick<LoomEvent,'ts'>): number`**
- `s = ev.ts?.wall_ns`. If `typeof s !== 'string'` or `!/^\d{1,20}$/.test(s)` → `0`.
- If every digit is `0` → `0` (unknown, not 1970).
- `s.length <= 6` → `Number(s) / 1e6`. Else `Number(s.slice(0, -6)) + Number(s.slice(-6)) / 1e6`. This is the same split as `monoMs`, keeping sub-ms precision.
- Examples:
  - `'1794742414837452625'` → ≈1794742414837.4526
  - `'1500000'` → 1.5
  - `'999'` → 0.000999
  - `undefined`, `''`, `'0'`, `'000'`, `'-1'`, `'12a'`, `'1e18'`, a 21-digit string, `' 1'` → 0.

**`ClockDict`** — bounded interning of `ts.clock_id`.
- `index(id)`:
  - `null`/`undefined`/`''` → `NO_CLOCK`.
  - A known id → its index.
  - A new id while `size < CLOCK_IDS_MAX` → next index (0-based, first seen = 0).
  - A new id when full → `NO_CLOCK`, and the id is not stored.
- `idOf(i)` → the id, or `null` for `NO_CLOCK` or an unknown index.
- `size` → number of ids stored (≤64).

**`timeCell(mode, row, ref, prev, prevAcrossGap): TimeCell`** — exact text.

Helpers:
- `FULL = \`wall ${row.wallMs > 0 ? new Date(row.wallMs).toISOString() : '없음'} · mono ${(row.monoMs / 1000).toFixed(6)} s · clock ${row.clockId ?? '?'} · seq ${row.seq}\``
- `signed(x, d) = (x < 0 ? '-' : '+') + Math.abs(x).toFixed(d)`
- `xclock(a, b) = a === NO_CLOCK || b === NO_CLOCK || a !== b`

Cells by mode:

- **`wall`**
  - `row.wallMs > 0` → `{ text: fmtWall(row.wallMs), cls: '', title: FULL }` (`fmtWall` already imported from `'../../sdk'` by the scaffold).
  - otherwise → `{ text: '--:--:--.---', cls: 'ct-t-unknown', title: \`벽시계 없음 (ts.wall_ns 없음) — 단조 시각으로 추정하지 않음 · ${FULL}\` }`.
- **`rel`**
  - `ref === null` → `{ text: '—', cls: 'ct-t-unknown', title: \`기준점 없음 · ${FULL}\` }`.
  - `xclock(row.clock, ref.clock)` → `{ text: '≠clock', cls: 'ct-t-xclock', title: \`시계가 다름 (행 clock ${row.clockId ?? '?'}) — 기준점과 다른 시계라 상대 시각을 계산하지 않음 · ${FULL}\` }`.
  - otherwise → `{ text: signed((row.monoMs - ref.monoMs) / 1000, 6), cls: '', title: \`${refLabel(ref)} · ${FULL}\` }`.
- **`delta`**
  - `prev === null` → `{ text: '—', cls: 'ct-t-unknown', title: \`직전 표시 행 없음 · ${FULL}\` }`.
  - `xclock(row.clock, prev.clock)` → `{ text: '≠clock', cls: 'ct-t-xclock', title: \`시계가 다름 (직전 행 seq ${prev.seq}) — Δ를 계산하지 않음 · ${FULL}\` }`.
  - otherwise → `{ text: signed(row.monoMs - prev.monoMs, 3), cls: prevAcrossGap ? 'ct-t-gap' : '', title: \`직전 표시 행(seq ${prev.seq})과의 간격 (필터 적용 후)${prevAcrossGap ? ' · 사이에 생략·손실 행 있음' : ''} · ${FULL}\` }`.

Examples (from `timeFmt.test.ts`):
- rel with ref 9953877.626791 and row +1234.568 ms → `+1.234568`; row −10 ms → `-0.010000`.
- delta 1000 → 1100.0126 → `+100.013`.

**`refLabel(ref)`**
- `null` → `'기준 없음'`
- `kind 'start'` → `` `기준: 시작 seq ${seq}` ``
- `kind 'row'` → `` `기준: seq ${seq} (선택 행)` ``

**`readTimeMode(kv)`**
- try `v = kv?.getItem(TIME_MODE_KEY)`. If `v` is in `TIME_MODES` → `v`. Otherwise, or if it throws → `'wall'`.

**`writeTimeMode(kv, mode)`**
- try `kv?.setItem(TIME_MODE_KEY, mode)`; swallow any throw.

---

## B1-TXSTORE — `txStore.ts` (pure + KV)

**`cansendText(p)`**
- Id text:
  - `idHex = p.id.toString(16).toUpperCase()`
  - `id = p.ext ? idHex.padStart(8, '0') : idHex.padStart(3, '0')`
  - `D = p.data.toUpperCase()`
- Classic → `${id}#${D}`. FD → `${id}##${p.brs ? 1 : 0}${D}`.
- These parse identically in `loom-can-frame::parse_frame` (>3 digits = extended; `##flag`) and in `parseSendLine` (8 digits = extended).
- Examples:
  - `0x300#01 02` → `300#0102`
  - `0x7#` → `007#`
  - `x7#` → `00000007#`
  - `x7FF#` → `000007FF#`
  - `fd 0x123#` → `123##0`
  - `fd 18DAF110#11` → `18DAF110##011`

**`txDoc(channel, entries, savedWallMs): TxDoc`**
- `{ format: TX_DOC_FORMAT, version: TX_DOC_VERSION, channel, saved_at: new Date(savedWallMs).toISOString(), entries: entries.map(e => ({ frame: cansendText(e.params), comment: e.comment, cycle_ms: e.cycleMs })) }`
- Key order exactly as written. No count, request id or error.

**`txDocText(doc)`** → `JSON.stringify(doc, null, 2)`.

**`parseTxDoc(input, channel): TxParse`** — validation order:

1. If `typeof input === 'string'`:
   - `input.length > TX_DOC_MAX_CHARS` → `{ok:false, error:'가져오기 텍스트가 너무 큽니다 (≤ 65,536자)'}`
   - Otherwise `JSON.parse`; on a throw → `{ok:false, error:\`JSON 형식 오류: ${message}\`}`.
   - The parsed value continues as `v`. A non-string input is used as `v` directly.
2. If `Array.isArray(v)` → `list = v`, `sourceChannel = null`.
3. Else if `v` is a non-null object:
   - `v.format !== TX_DOC_FORMAT` → `{ok:false, error:'알 수 없는 문서 형식 — format "loom.can.txlist" 필요'}`
   - `v.version !== 1` → `{ok:false, error:\`지원하지 않는 버전 ${String(v.version)} (지원: 1)\`}`
   - `!Array.isArray(v.entries)` → `{ok:false, error:'entries 배열이 없습니다'}`
   - Otherwise `list = v.entries`, `sourceChannel = typeof v.channel === 'string' ? v.channel : null`.
4. Else → `{ok:false, error:'가져오기: JSON 문서 또는 문자열 배열이 필요합니다'}`.
5. For each `list[i]`:
   - A string → `frame = it`, `comment = ''`, `cycle = 0`.
   - An object with a string `frame` → `frame`; `comment = typeof c === 'string' ? c.slice(0, COMMENT_MAX) : ''`; `cycle = cycle_ms ?? 0` (also `0` when `null`).
   - Anything else → skip `{index:i, reason:'frame 없음 (cansend 문자열 필요)'}`.
   - Then:
     - `r = parseSendLine(frame, channel)`; if `!r.ok` → skip `{index:i, reason: r.error}`.
     - If `!Number.isInteger(cycle) || cycle < 0 || cycle > CYCLE_MAX_MS` → skip `{index:i, reason:'cycle_ms: 0–60000 정수'}`.
     - Otherwise push `{params: r.params, text: r.canonical, comment, cycleMs: cycle}` while `items.length < TX_LIST_MAX`; beyond that `dropped++`.
6. → `{ok:true, items, skipped, dropped, sourceChannel}`.

Imports `parseSendLine`, `COMMENT_MAX`, `TX_LIST_MAX` are already in the scaffold.

**`storeKey(channel)`** → `TX_STORE_PREFIX + channel`.

**`loadTxList(kv, channel)`**
- `kv === null` → `{ok:true, items:[], skipped:[], dropped:0, sourceChannel:null}`.
- try `raw = kv.getItem(storeKey(channel))`; on a throw → `{ok:false, error:\`브라우저 저장소 읽기 실패: ${message}\`}`.
- `raw === null` → same ok-empty. Otherwise `parseTxDoc(raw, channel)`.

**`saveTxList(kv, channel, entries, savedWallMs)`**
- `kv === null` → `{ok:false, error:'브라우저 저장소 없음 — 이 세션에서만 유지'}`.
- try:
  - `entries.length === 0` → `kv.removeItem(key)`
  - else `kv.setItem(key, JSON.stringify(txDoc(channel, entries, savedWallMs)))` (compact).
  - → `{ok:true}`.
- On a throw → `{ok:false, error:\`저장 실패: ${message}\`}`.

---

## B1-MODEL — `traceModel.ts`

- **`bytesForRows`**: add `+ 8 + 2` to `perRow` (wall f64 + clock u16). Test: `bytesForRows(2000) - bytesForRows(1000) === 59_000`.
- **constructor**: `this.wall = new Float64Array(c); this.clk = new Uint16Array(c).fill(NO_CLOCK);`
- **`push(kind, seq, mono, flags, src = null)`**:
  - After the existing column resets, add:
    - `this.wall[i] = src ? wallMsOf(src) : 0`
    - `this.clk[i] = src ? this.clocks.index(src.ts?.clock_id) : NO_CLOCK`
  - Then, if `kind !== K_GAP && this.firstFrameRef === null` → `this.firstFrameRef = { monoMs: mono, clock: this.clk[i], seq, kind: 'start' }`.
- **`appendRow`**: the `this.push(kind, seq, monoMs(ev), fl)` call gains `ev` as its 5th argument. Nothing else changes.
- **`applyFrame`** merge branch (`fi === 0 && info.txRow >= this.oldestAbs`): right after `this.mono[i] = monoMs(ev);` add:
  - `this.wall[i] = wallMsOf(ev)`
  - `this.clk[i] = this.clocks.index(ev.ts?.clock_id)`

  Do not touch `firstFrameRef` here.
- **`applyGapEvent`**: the `this.addGap({...}, numOf(ev.seq) ?? 0, monoMs(ev))` call gains `ev` as its 4th argument.
- **`addGap(g, seq, mono, src = null)`** → `this.push(K_GAP, seq, mono, 0, src)`. The rest is unchanged. `omitRun` is unchanged (no src).
- **Readers**:
  - `wallAt(abs)` → `this.wall[abs % this.capacity]`
  - `clockAt(abs)` → `this.clk[abs % this.capacity]`
  - `clockIdOf(i)` → `this.clocks.idOf(i)`
  - `timeAt(abs)` → `{ monoMs: this.monoAt(abs), wallMs: this.wallAt(abs), clock: this.clockAt(abs), clockId: this.clockIdOf(this.clockAt(abs)), seq: this.seqAt(abs) }` — exactly these keys.

---

## B1-VIEW — `traceView.ts`

- **constructor**: while building `HEAD` cells, keep the span for column index 1:
  - `this.tHead = s`
  - `s.dataset.testid = 'can-time-head'`
  - Add `this.scroll.addEventListener('click', this.onClick)`.
- **render()**:
  - Before the pool loop:
    - `const mode = this.o.timeMode?.() ?? 'wall'`
    - `setText(this.tHead, TIME_HEAD[mode])`
    - `this.tHead.title` = per mode:
      - wall → `'벽시계 (ts.wall_ns, 로컬 표시) — 툴팁에 UTC·mono·clock'`
      - rel → `` `${refLabel(this.o.timeRef?.() ?? null)} — 같은 clock_id 행만` `` (`refLabel` already imported)
      - delta → `'직전 표시 행과의 간격 ms (필터 적용 후, 같은 clock_id만)'`
  - Call `this.fillRow(p, abs, nowMs, idx)`.
- **fillRow(p, abs, nowMs, idx)**:
  - For every row: `attrs['data-abs'] = String(abs)`.
  - For non-gap rows:
    - `const mode = this.o.timeMode?.() ?? 'wall'`
    - `const pv = mode === 'delta' ? this.prevShownFrame(idx) : null`
    - `const cell = timeCell(mode, m.timeAt(abs), this.o.timeRef?.() ?? null, pv ? m.timeAt(pv.abs) : null, pv?.acrossGap ?? false)`
    - `vals[1] = cell.text; cls[1] = cell.cls; titles[1] = cell.title`. This replaces the `(m.monoAt(abs)/1000).toFixed(3)` line.
    - `attrs['aria-selected'] = String(abs === this.selAbs)`.
  - Gap rows get no `aria-selected`. Everything else is unchanged.
- **`prevShownFrame(idx)`**:
  - Start with `crossed = false`.
  - For `k = idx-1` down to `Math.max(0, idx - DELTA_LOOKBACK)`:
    - `a = m.dispAt(k)`; if `m.kindAt(a) === K_GAP` → `crossed = true`, continue.
    - Otherwise return `{abs: a, acrossGap: crossed}`.
  - Nothing found → `null`.
  - Uses display order, so the filter applies and pause is respected.
- **`select(abs)`**:
  - If `!Number.isInteger(abs) || !m.has(abs) || m.kindAt(abs) === K_GAP` → return with no change.
  - Otherwise `this.selAbs = abs; this.o.onNeedRender()`.
- **`get selected`** → `this.selAbs >= 0 && m.has(this.selAbs) ? this.selAbs : -1`.
- **`onClick(e)`**:
  - `row = (e.target as HTMLElement).closest<HTMLElement>('.ct-row')`
  - If `!row || row.dataset.gap !== undefined` → return.
  - Otherwise `this.select(Number(row.dataset.abs))`.
- **dispose**: also `this.scroll.removeEventListener('click', this.onClick)`.

---

## B1-RXSUM — `rxSummary.ts`

`apply`: new-row literal gets `lastWallMs: 0`. Each aggregated frame sets `r.lastWallMs = wallMsOf(ev)` next to `r.lastMonoMs = t`. Nothing else changes.

---

## B1-TXLIST — `txList.ts`

- **`upsert`**:
  - Replace path: keep the existing `cycleMs`, `contentVersion++`.
  - New entry: `cycleMs: 0`, `contentVersion++`.
  - The "full" error path does not bump.
- **`remove`**: when an entry is removed → `contentVersion++`. A missing uid → no bump.
- **`load(items, mode)`**:
  - `replace` → `this.items = []` (uids keep counting from `nextUid`; they are never reused).
  - Then for each item:
    - while `items.length < TX_LIST_MAX` push `{uid: nextUid++, params: it.params, text: it.text, comment: it.comment.slice(0, COMMENT_MAX), cycleMs: it.cycleMs, count: 0, lastRequestId: null, lastError: null}`, `added++`;
    - otherwise `dropped++`.
  - `selected`:
    - replace → first entry's uid or `null`;
    - append → unchanged if still present, else the first added or `null`.
  - If `mode === 'replace' || added > 0` → `version++; contentVersion++`.
  - Return `{added, dropped}`.
- `select`, `move`, `noteSent` and `noteFailed` never touch `contentVersion`.

---

## B1-RXTX — `rxtxView.ts`

**constructor**: create these elements with the existing `el`/`tid` helpers. All are `type='button'` where applicable.
- `this.exportBtn = tid(el('button', undefined, 'JSON 내보내기'), 'can-tx-export')`, title `'송신 목록을 JSON 문서로 보기·복사 (파일 다운로드 아님)'`.
- `this.importBtn = tid(el('button', undefined, 'JSON 가져오기'), 'can-tx-import')`, title `'붙여 넣은 JSON(문서 또는 ["300#01", …])을 검증해 추가/교체'`.
- `this.persistEl = tid(el('span', 'crx-persist'), 'can-tx-persist')`, `aria-live=polite`, `dataset.state='off'`.
- `txTools.append(newBtn, editBtn, delBtn, sendBtn, exportBtn, importBtn, defer, txMsg, persistEl)`. This replaces the current append order; `defer` stays until B3.
- `this.io = tid(el('div', 'crx-io'), 'can-tx-io')`:
  - `hidden = true`, `aria-label '송신 목록 JSON'`.
  - Children:
    - `this.ioText = tid(el('textarea', 'crx-io-text'), 'can-tx-io-text')` (spellcheck false)
    - a row `el('div', 'crx-io-row')` holding:
      - `ioCopy` (`'복사'`, `can-tx-io-copy`)
      - `ioAppend` (`'목록에 추가'`, `can-tx-io-append`)
      - `ioReplace` (`'목록 바꾸기'`, `can-tx-io-replace`)
      - `ioClose` (`'닫기'`, `can-tx-io-close`)
      - `this.ioMsg = tid(el('span', 'crx-io-msg'), 'can-tx-io-msg')` (aria-live polite)
- `txMain.append(txTools, txHead, txBody, editor, io)`.
- Listeners:
  - export → `openExport()`; import → `openImport()`; close → `closeIo()`
  - append → `applyImport('append')`; replace → `applyImport('replace')`
  - copy → `this.hooks.copyText(this.ioText.value).then(() => this.sayIo('JSON을 클립보드에 복사함', false), () => this.sayIo('복사 실패 — 상자에서 직접 선택해 복사하세요', true))`
  - `io` keydown: Escape → `preventDefault`, `closeIo()`; **always** `stopPropagation()` (same as the editor).

**renderRx** col 6: `c[6].title = \`마지막 수신: 집계 창 시작 기준 · 벽시계 ${r.lastWallMs > 0 ? \`${fmtWall(r.lastWallMs)} (${new Date(r.lastWallMs).toISOString()})\` : '없음'} · seq ${r.lastSeq}\``. The text is unchanged (`rel.toFixed(4)`). The header constant was already changed by Opus.

**Existing handlers**: after `this.tx.remove(...)` in the delete button handler and in the Delete-key branch, and after a successful upsert in `commitEditor` (before `closeEditor`), call `this.hooks.listChanged()`.

**`setPersist(text, state)`**:
- `setText(persistEl, text)`, `persistEl.title = text`, `persistEl.dataset.state = state`, `persistEl.classList.toggle('err', state === 'err')`.

**`openExport()`**:
- `ioText.value = txDocText(txDoc(this.hooks.channel, this.tx.entries, this.hooks.wallNow()))`
- `ioText.readOnly = true`, `io.dataset.mode = 'export'`
- `ioCopy.hidden = false`, `ioAppend.hidden = ioReplace.hidden = true`
- `sayIo('', false)`, `io.hidden = false`, `ioText.focus()`, `ioText.select()`.

**`openImport()`**:
- `ioText.value = ''`, `ioText.readOnly = false`
- `ioText.placeholder = '{"format":"loom.can.txlist",…} 또는 ["300#01", …]'`
- `io.dataset.mode = 'import'`
- `ioCopy.hidden = true`, `ioAppend.hidden = ioReplace.hidden = false`
- `setText(ioReplace, \`목록 바꾸기 (현재 ${this.tx.entries.length}개 삭제)\`)`
- `sayIo('', false)`, `io.hidden = false`, `ioText.focus()`.

**`closeIo()`**:
- `io.hidden = true`, `ioText.value = ''`, `sayIo('', false)`, `this.txBody.focus()`.

**`applyImport(mode)`**:
1. `r = parseTxDoc(this.ioText.value, this.hooks.channel)`. If `!r.ok` → `sayIo(\`가져오기 실패: ${r.error}\`, true)` and return (the list is untouched).
2. `res = this.tx.load(r.items, mode)`.
3. Build the message:
   - start: `msg = mode === 'replace' ? \`목록을 ${res.added}개로 바꿈\` : \`${res.added}개 추가\``
   - if `r.skipped.length`: `+= \` · ${r.skipped.length}개 건너뜀 (#${r.skipped[0].index + 1}: ${r.skipped[0].reason})\``
   - `d = r.dropped + res.dropped`; if `d`: `+= \` · 상한 ${TX_LIST_MAX}개 초과 ${d}개 제외\``
   - if `r.sourceChannel && r.sourceChannel !== this.hooks.channel`: `+= \` · 원래 채널 ${r.sourceChannel}\``
4. `sayIo(msg, false); this.hooks.listChanged(); this.render()`. The box stays open.

**`sayIo(text, err)`**: `setText(ioMsg, text)`, `ioMsg.classList.toggle('err', err)`.

---

## B1-PANEL — `canTrace.ts` (`createCanTrace` body)

Order and exact additions:

1. Before `const view = new TraceView(...)`:
   - `const kv = defaultKV();`
   - `let timeMode: TimeMode = readTimeMode(kv);`
   - `let userRef: TimeRef | null = null;`
   - `const timeRef = (): TimeRef | null => userRef ?? model.firstFrameRef;`
   - `const wallNow = opts.wallNow ?? (() => Date.now());`
2. Top-bar DOM:
   - `timeSel = el('select')`:
     - `dataset.testid='can-time-mode'`, `aria-label '시각 표시 방식'`
     - one `option` per `TIME_MODES` (value = mode, text = `TIME_LABEL[mode]`)
     - `value = timeMode`
   - `timeBox = el('span', 'ct-timebox')` (`testid 'can-time-refbox'`) containing:
     - `refBtn = el('button', undefined, '기준점 = 선택 행')` (`testid 'can-time-ref'`, title `'트레이스에서 클릭한 행을 상대 시각 0으로'`)
     - `refReset = el('button', undefined, '시작 기준')` (`testid 'can-time-ref-reset'`)
     - `refLbl = el('span', 'ct-time-ref-label')` (`testid 'can-time-ref-label'`)
   - All buttons are `type='button'`.
   - Change the append to: `top.append(chLabel, envBadge, tabs, live, pauseBtn, timeSel, timeBox, idFilter, dirSel, orgWrap, agentInfo)`.
3. TraceView options: add `timeMode: () => timeMode, timeRef`.
4. Closures (exact signatures):
   - `const onTimeMode = (v: string): void` — if `(TIME_MODES as readonly string[]).includes(v)` → `timeMode = v as TimeMode; writeTimeMode(kv, timeMode); ctx.scheduler.invalidate()`.
   - `const onSetRef = (): void` — `abs = view.selected`; `if (abs < 0) return`; `t = model.timeAt(abs)`; `userRef = { monoMs: t.monoMs, clock: t.clock, seq: t.seq, kind: 'row' }`; `ctx.scheduler.invalidate()`.
   - `const onResetRef = (): void` — `userRef = null; ctx.scheduler.invalidate()`.
   - Listeners: `timeSel` change → `onTimeMode(timeSel.value)`; `refBtn` click → `onSetRef()`; `refReset` click → `onResetRef()`.
5. RxTxView hooks:
   - `listChanged: () => ctx.scheduler.invalidate()`
   - `wallNow`
   - `copyText: (t) => (typeof navigator !== 'undefined' && navigator.clipboard?.writeText ? navigator.clipboard.writeText(t) : Promise.reject(new Error('clipboard 없음')))`
6. Persistence, after `rxtx` is created:
   - `let savedVersion = -1;`
   - **`const persistLoad = (): void`**:
     - `!bound` → `rxtx.setPersist('채널 미선택 — 송신 목록을 저장하지 않음', 'off')`
     - else `r = loadTxList(kv, channel)`:
       - `r.ok`:
         - `if (r.items.length) txList.load(r.items, 'replace')`
         - text = `r.items.length ? \`이 브라우저에 저장된 목록 ${r.items.length}개 불러옴 (${channel})\` : \`이 브라우저에 저장 (${channel}) — 저장된 목록 없음\``
         - plus `\` · 건너뜀 ${r.skipped.length}\`` when skipped, plus `\` · 상한 초과 ${r.dropped}\`` when dropped
         - `setPersist(text, 'ok')`
       - not ok → `setPersist(\`저장된 목록을 읽지 못함: ${r.error}\`, 'err')`. Nothing is loaded, and the stored text is left untouched.
     - Always end with `savedVersion = txList.contentVersion; rxtx.render()`.
     - Called once, right after the closure definition.
   - **`const persistTick = (): void`**:
     - `if (!bound || txList.contentVersion === savedVersion) return`
     - `savedVersion = txList.contentVersion` (set before saving: a failure retries only on the next change)
     - `r = saveTxList(kv, channel, txList.entries, wallNow())`
     - `r.ok` → `setPersist(\`이 브라우저에 저장됨 (${channel}, ${txList.entries.length}개)\`, 'ok')`; else `setPersist(\`저장 안 됨: ${r.error}\`, 'err')`.
7. `updateStatus()`: add
   - `timeBox.hidden = timeMode !== 'rel'`
   - `refBtn.disabled = view.selected < 0`
   - `setText(refLbl, refLabel(timeRef()))`
   - `if (timeSel.value !== timeMode) timeSel.value = timeMode`
8. Frame callback: call `persistTick()` right after `updateStatus()`.
9. `dispose()`: first statement `persistTick();` (flush before teardown).
10. Do not change the manifest, the send path, ingest, or the unbound "no RPC on mount" behaviour. `persistLoad` uses no RPC.

---

## B1-SCAFFOLD-FIX (Opus — DONE before lease)

Applied: `timeFmt.ts` imports `fmtWall`; `txStore.ts` imports `parseSendLine`, `COMMENT_MAX`, `TX_LIST_MAX` as values; `traceView.ts` imports `refLabel`. Placeholder `void [...]` lines reference them only until the bodies replace the stubs. Sonnet removes every `void [...]` placeholder line in the stubs it implements.

---

## B1-REVIEW (Opus) — ACCEPTED

- Worker hash (9 files concatenated in lease order) `19fcf750db39fb4c3e2af4238aeb6d4c0f5a9afc99bcd6ae7feeb56c5e42d330` — re-verified by Opus.
- Both open items were test-side (Opus spec), fixed by Opus, no product change:
  1. `traceTime.test.ts`: expected wall now keeps sub-ms (ns split), matching §B1-TIME `wallMsOf` (the contract stands).
  2. `rxtxPanel.test.ts`: `localStorage.clear()` in `beforeEach`/`afterEach` (panels now persist per channel, CAN-3).
- After fixes: `vitest src/panels/can` (B1 files) 139/139; isolated app tsc 0; node tsc 0; eslint 0.
- Review notes (accepted, recorded as risk): a corrupt stored list shows an explicit error and is overwritten only by the user's next edit; two panels on the same channel in one browser are last-writer-wins (per-browser storage, as planned).

---

## B2 overview

B2 is split so the worker never waits on Opus:
- **B2a (this lease)**: pure modules `idFilter.ts` (CAN-8) and `rowDetail.ts` (CAN-7). No DOM, no RPC.
- **B2b (next)**: panel wiring (row detail line, keyboard selection, data-cell title, filter chip, `closeChannel.ts` CAN-6 + manifest `close`). Opus scaffolds B2b in canTrace/traceView/traceModel/rxtxView while B2a runs (those files are NOT in the B2a lease).

## B2a-ALLOW — exclusive file lease

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `idFilter.ts` | `IdFilter.constructor`, `IdFilter.has`, `fmtFilterId`, `parseIdFilter` |
| `rowDetail.ts` | `rowFacts`, `fullDataText`, `rowDetailFields`, `rowDetailText` |

Spec tests (Opus, read-only): `idFilter.test.ts`, `rowDetail.test.ts`. Rules §0 apply unchanged (`UNIMPLEMENTED(B2)` stubs, remove every `void [...]` placeholder line, no other edits; a missing import/helper → return the lease).
Acceptance (in `ui/`): `npx vitest run src/panels/can` → **all pass** (139 B1 + idFilter + rowDetail); isolated app/node tsc 0; `npx eslint src/panels/can` 0.
Scaffold hash: `cat ui/src/panels/can/idFilter.ts ui/src/panels/can/idFilter.test.ts ui/src/panels/can/rowDetail.ts ui/src/panels/can/rowDetail.test.ts | shasum -a 256` (value in STATUS.md). Return: worktree `work/reports/operator-upgrade/can/B2a-WORKER.md` (same 4 checks, result hash of the 2 product files `cat idFilter.ts rowDetail.ts | shasum -a 256`).

## B2-IDF — `idFilter.ts`

Grammar in the file header. Every number is hex (bare digits too).

**`parseIdFilter(s)`**
1. `tokens = s.split(/[\s,]+/).filter(Boolean)`; none → `return null`.
2. Per token `tok`: must match `/^([^-]+)(?:-([^-]+))?$/`, else `throw new Error(\`id "${tok}"\`)`.
3. Each end (`a`, optional `b`): strip one leading `0x`/`0X`, then one trailing `x`/`X`; the rest must match `/^[0-9a-fA-F]{1,8}$/` else `throw new Error(\`id "${tok}"\`)`. Value `parseInt(rest, 16)`.
4. Any end `> CAN_ID_MAX` → `throw new Error(\`id "${tok}" > 0x1FFFFFFF\`)`.
5. Range with `lo > hi` → `throw new Error(\`범위 "${tok}": 시작 > 끝\`)`. Range with `lo === hi` → treat as single id.
6. Singles go to a `Set`; when its size exceeds `ID_FILTER_MAX_IDS` → `throw new Error('id ≤ 256개')` (duplicates don't count). Range tokens (lo<hi) counted as given; more than `ID_FILTER_MAX_RANGES` → `throw new Error('범위 ≤ 64개')`. Checks happen while scanning, in token order (first violation wins).
7. `return new IdFilter(singles, ranges)`.

**`IdFilter.constructor(singles, ranges)`** (never throws)
- Sort ranges by `lo`; merge when `next.lo <= cur.hi + 1` (overlap or adjacent) → `this.ranges` (new `{lo, hi}` objects).
- `this.singles` = distinct singles not inside any merged range, ascending.
- `this.set = new Set(this.singles)`.
- `this.text` = items (each single as `fmtFilterId(id)`, each range as `` `${fmtFilterId(lo)}-${fmtFilterId(hi)}` ``) sorted by their start value, joined with `', '`.

**`has(id)`** → `this.set.has(id)` or some range with `lo <= id && id <= hi` (linear; ≤ 64 ranges).

**`fmtFilterId(id)`** → `id <= 0x7ff ? '0x' + hex.padStart(3,'0') : '0x' + hex.padStart(8,'0')`, `hex = id.toString(16).toUpperCase()`.

## B2-DETAIL — `rowDetail.ts`

**`rowFacts(model, abs)`**: `!Number.isInteger(abs) || !model.has(abs)` → `null`. Otherwise read once:
- `k = model.kindAt(abs)`; `kind = k === K_FRAME ? 'frame' : k === K_GAP ? 'gap' : 'txreq'`; `fl = model.flagsAt(abs)`.
- `id = kind === 'gap' || (fl & F_NOID) ? null : model.idAt(abs)`.
- booleans from flags: `ext F_EXT`, `fd F_FD`, `brs F_BRS`, `rtr F_RTR`, `tx F_TX`, `echo F_ECHO`.
- `len = model.lenAt(abs)`, `dlc = model.dlcAt(abs)`, `dataHex = kind === 'gap' ? '' : model.dataHexAt(abs)`, `origin = model.originAt(abs)`.
- `req = model.reqAt(abs)` → `requestId = req?.requestId ?? null`, `clientId = req?.clientId ?? null`, `error = req?.error ?? null`.
- `state = model.stateAt(abs)`, `risk = model.riskAt(abs)`, `frameIndex = model.frameIndexAt(abs)`.
- `t = model.timeAt(abs)` → `wallMs = t.wallMs`, `monoMs = t.monoMs`, `clockId = t.clockId`; `seq = t.seq`.
- `gap = model.gapAt(abs)`; `abs`.

**`fullDataText(dataHex)`**: `null → FD_SLOT_REUSED`; `'?' → NO_INLINE_BYTES`; `'' → NO_BYTES`; else `dataHex` unchanged.

**`rowDetailFields(f)`** — fields in this order; a row marked "omit if" is left out (never shown as 0/empty):

| key | label | value | present |
|---|---|---|---|
| kind | 종류 | frame: `f.tx ? 'TX 프레임' : 'RX 프레임'` + (`f.echo` ? `' · 에코'` : '') · txreq: `'송신 요청'` · gap: `` `갭 · ${f.gap?.kind ?? '?'}` `` | always |
| seq | seq | `String(f.seq)` | always |
| id | ID | `f.ext` ? `` `0x${hex8} (29-bit)` `` : `` `0x${hex3} (11-bit)` `` (upper case, padStart 8 / 3) | not gap, `f.id !== null` |
| fmt | 형식 | `[f.fd ? 'CAN FD' : 'CAN', f.brs && 'BRS', f.rtr && 'RTR']` filtered, joined `' · '` | not gap |
| len | 길이 | `` `${f.len} B · DLC ${f.dlc ?? '?'}` `` | not gap |
| data | 데이터 | `fullDataText(f.dataHex)` | not gap |
| origin | 출처 | `f.origin` | not gap |
| req | 요청 | `f.requestId` + (`f.clientId` ? `` ` · client ${f.clientId}` `` : '') | not gap, `requestId !== null` |
| frameIndex | 프레임 순번 | `String(f.frameIndex)` | not gap, `!== null` |
| state | 상태 | `f.state` | not gap, `!== null` |
| risk | 등급 | `f.risk` | not gap, `!== null` |
| error | 오류 | `f.error` | not gap, `!== null` |
| gap | 손실 | `count === 'unknown'` ? `'알 수 없음'` : `` `${count}행` `` | gap with `f.gap` |
| gapRange | 범위 | `` `seq ${first}–${last}` `` (en dash U+2013) | gap with `first !== null && last !== null` |
| cause | 원인 | `f.gap.cause` | gap with `f.gap` |
| wall | 벽시계 | `f.wallMs > 0` ? `` `${fmtWall(f.wallMs)} (${new Date(f.wallMs).toISOString()})` `` : `'알 수 없음'` | always |
| mono | mono | `` `${(f.monoMs / 1000).toFixed(6)} s` `` | always |
| clock | clock | `f.clockId ?? '알 수 없음'` | always |

**`rowDetailText(f)`**: `rowDetailFields(f).map((d) => \`${d.label}: ${d.value}\`).join('\n')`.

---

## B2a-REVIEW (Opus) — ACCEPTED

Worker hash `cat idFilter.ts rowDetail.ts | shasum -a 256` = `69059e558fc294e744a393dcec3c5dfbcf04cb8078f67ca5ba2470f8638e61f1` (verified). Spec tests untouched (mtimes before lease). vitest src/panels/can 159/159; isolated app/node tsc 0; eslint 0. Bodies match §B2-IDF/§B2-DETAIL line by line.

---

## B2b-ALLOW — exclusive file lease (pure CAN-6 module)

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `closeChannel.ts` | `parseJobs`, `parseChannelBrief`, `closeImpact`, `closeImpactLines`, `closeDoneText`, `channelStateOf` |

Spec test (Opus, read-only): `closeChannel.test.ts`. Rules §0 apply (`UNIMPLEMENTED(B2)` stubs).
Acceptance (in `ui/`): `npx vitest run src/panels/can` → all pass (171); isolated app/node tsc 0; eslint 0. Scaffold state: 12 failed / 159 passed.
Scaffold hash: `cat ui/src/panels/can/closeChannel.ts ui/src/panels/can/closeChannel.test.ts | shasum -a 256` (value in STATUS.md). Return `B2b-WORKER.md` with result hash `cat closeChannel.ts | shasum -a 256`.

## B2-CLOSE — `closeChannel.ts`

Helper rule: a "count" is `typeof v === 'number' && Number.isInteger(v) && v >= 0`. `errMsg(reason) = reason instanceof Error ? reason.message : String(reason)` (local `const` inside the function that needs it — no new top-level helper).

**`parseJobs(v, channel)`**: `v` must be a non-null object whose `jobs` is an array, else `null`. Each entry must be a non-null object with `job` string, `channel` string, `id` count, `ext` boolean, `owner` string, `origin` string, `period_ms` count, `state` string — any entry failing this → return `null` (the whole list is unknown). Entries with `channel !== channel` are skipped. Map to `{ job, id, ext, owner, origin, periodMs: period_ms, state }` in input order.

**`parseChannelBrief(v, channel)`**: `v` non-null object with array `channels`, else `null`. Find the first object entry with `channel === channel`; none → `null`. `isotp_endpoints`, `pending_bursts`, `nodes` must all be counts, else `null`. → `{ isotp, bursts, nodes }`.

**`closeImpact(channel, list, status)`**:
- list fulfilled → `jobs = parseJobs(list.value, channel)`; `null` → push `'주기 송신 목록 형식을 알 수 없음'`. Rejected → `jobs = null`, push `` `주기 송신 목록 조회 실패: ${errMsg(list.reason)}` ``.
- status the same with `parseChannelBrief`, texts `'채널 상태 형식을 알 수 없음'` / `` `채널 상태 조회 실패: ${errMsg(...)}` ``.
- Errors in that order (list first). Return `{ channel, jobs, status, errors }`.

**`closeImpactLines(i)`** — in order:
1. `` `채널 ${i.channel}을(를) 데몬에서 닫습니다 — 이 채널을 쓰는 모든 사용자·에이전트에 적용됩니다.` ``
2. jobs: `null` → `'주기 송신: 알 수 없음'`; `[]` → `'주기 송신: 없음'`; else `` `주기 송신 ${n}개 정지 (모든 소유자):` `` then for the first `CLOSE_JOBS_SHOWN` jobs `` `· ${job} ${fmtId(id, ext)} ${periodMs} ms — ${origin} ${owner} (${state})` ``, then when `n > CLOSE_JOBS_SHOWN`: `` `· … 외 ${n - CLOSE_JOBS_SHOWN}개` ``.
3. status `null` → `'ISO-TP 엔드포인트·대기 버스트·시뮬레이터 노드: 알 수 없음'`; else three lines: `` `ISO-TP 엔드포인트: ${isotp ? `${isotp}개 닫힘` : '없음'}` ``, `` `대기 중 버스트 송신: ${bursts ? `${bursts}개 취소` : '없음'}` ``, `` `시뮬레이터 노드: ${nodes ? `${nodes}개 분리` : '없음'}` ``.
4. each error as `` `⚠ ${e}` ``.
5. `VIEW_CLOSE_NOTE`.

**`closeDoneText(channel, result, i)`**: `` `CAN ${channel} 닫힘` `` + (result is a non-null object whose `generation` is a count → `` ` (generation ${g})` ``) + (`i && i.jobs && i.jobs.length > 0` → `` ` · 주기 송신 ${i.jobs.length}개 정지` ``).

**`channelStateOf(ev, channel)`**: `ev.kind === 'can.channel_state' && ev.channel === channel`, and `payload.state` is `'open'` or `'closed'` → that value; otherwise `null`.

---

## B2b-REVIEW (Opus) — ACCEPTED

Worker hash `closeChannel.ts` = `f81960291b4081c230eb30421fca34c541127bf65410d517a8691866c364bee2` (verified); spec untouched; vitest 171/171; app/node tsc 0; eslint 0. Bodies match §B2-CLOSE.

---

## B2c-ALLOW — exclusive file lease (panel wiring CAN-8/7/6)

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `traceView.ts` | `moveSelection`, `ensureVisible`, `onKey`, `onDbl` (stubs); in the constructor add the `keydown`/`dblclick` listeners; in `dispose` remove them; in `fillRow` the data-cell title (§B2-VIEW 5) |
| `rxtxView.ts` | `renderRx`: the first lines that use `rxFilterKey` (§B2-RXTX) |
| `canTrace.ts` | `createCanTrace` body only (§B2-PANEL): DOM additions, closures, `applyFilter`, TraceView option `onOpenDetail`, `updateStatus`, `dispose` |

Opus scaffold already done (do not change): `IdMatch` type in `TraceFilter.ids` (traceModel.ts) and the Receive hook; `rxFilterRef` field; `SELECT_SKIP_MAX`; `onOpenDetail` option; stub signatures; imports (`parseIdFilter`/`IdFilter`/`IdMatch`, rowDetail, closeChannel) and the `export { parseIdFilter }` re-export; the old local `parseIdFilter` is removed; manifest `close` action + `can.periodic.list` query; the type annotations `curIds: IdMatch | null` and `let ids: IdFilter | null`; CSS classes below. `traceModel.ts`, `idFilter.ts`, `rowDetail.ts`, `closeChannel.ts`, `can.css` and all tests are NOT leased.
Spec test (Opus, read-only): `canB2Panel.test.ts`.
Scaffold state (Opus measured): app tsc 3 errors (rxtxView `rxFilterKey` lines), eslint 9 unused imports in canTrace.ts; vitest fails in canB2Panel.test.ts only (plus anything the 3 tsc errors break at runtime). Acceptance: `npx vitest run src/panels/can` **all pass**; isolated app/node tsc 0; eslint 0.
Scaffold hash: `(git diff -- ui/src/panels/can/traceModel.ts ui/src/panels/can/traceView.ts ui/src/panels/can/rxtxView.ts ui/src/panels/can/canTrace.ts ui/src/panels/can/can.css; cat ui/src/panels/can/canB2Panel.test.ts) | shasum -a 256` (value in STATUS.md; the diff is against baseline 610029e, so it includes accepted B1). Return `B2c-WORKER.md` with result hash `cat ui/src/panels/can/traceView.ts ui/src/panels/can/rxtxView.ts ui/src/panels/can/canTrace.ts | shasum -a 256`.

## B2-VIEW — `traceView.ts`

1. **Constructor**: after the existing `click` listener add `this.scroll.addEventListener('keydown', this.onKey); this.scroll.addEventListener('dblclick', this.onDbl);`. **dispose**: remove both, next to the existing removals.
2. **`moveSelection(step)`**:
   - `n = this.count()`; `n === 0` → return.
   - `rowsInView = Math.max(1, Math.floor(this.viewH() / ROW_H))`.
   - Start index: `sel = this.selected`. If `sel >= 0`: `idx = this.m.dispIndexOf(sel)`; if `step > 0 && (idx >= n || this.m.dispAt(idx) !== sel)` then `idx -= 1` (the selected row is not shown: examine the entry at the lower bound next). If `sel < 0`: `idx = step > 0 ? this.firstVisible - 1 : Math.min(n, this.firstVisible + rowsInView)`.
   - Walk: `for (let k = idx + step, skipped = 0; k >= 0 && k < n && skipped <= SELECT_SKIP_MAX; k += step)`: `a = this.m.dispAt(k)`; `kindAt(a) === K_GAP` → `skipped++; continue`; else `this.select(a); this.ensureVisible(k); return`.
   - Nothing found → selection unchanged.
3. **`ensureVisible(idx)`**: `rows = Math.max(1, Math.floor(this.viewH() / ROW_H))`; `idx < this.firstVisible` → `this.scrollToIndex(idx)`; else `idx >= this.firstVisible + rows` → `this.scrollToIndex(idx - rows + 1)`; else nothing.
4. **`onKey(e)`**: `ArrowUp` / `ArrowDown` → `e.preventDefault(); this.moveSelection(-1 | 1)`. `Enter` → `const s = this.selected; if (s >= 0) { e.preventDefault(); this.o.onOpenDetail?.(s); }`. Other keys: nothing (Space/End keep bubbling to the panel).
5. **`fillRow`** (frame/txreq branch, where `data` is computed): set `titles[6] = fl & F_RTR ? '원격 프레임(RTR) — 데이터 없음' : fullDataText(data)`. Nothing else in `fillRow` changes. Remove the `void [e, fullDataText]` placeholder in `onDbl`.
6. **`onDbl(e)`**: `row = (e.target as HTMLElement).closest<HTMLElement>('.ct-row')`; none or `row.dataset.gap !== undefined` → return; `abs = Number(row.dataset.abs)`; `this.select(abs)`; if `this.selected === abs` → `this.o.onOpenDetail?.(abs)`.

## B2-RXTX — `rxtxView.ts` `renderRx`

Replace the first lines:
```
const f = this.hooks.idFilter();
if (this.rx.version === this.rxVersion && f === this.rxFilterRef) return;
this.rxVersion = this.rx.version;
this.rxFilterRef = f;
```
(the `fk` string goes away; `f.has(r.id)` below is unchanged).

## B2-PANEL — `canTrace.ts` (`createCanTrace` body)

Helpers already in the file: `el(tag, cls?, text?)`, `setText`. All device/daemon text → `textContent` (`el(..., text)` / `setText`). Each new element gets `type = 'button'` when it is a button.

**A. CAN-8 filter**
1. `idFilter.placeholder = 'id 0x300, 0x7E0-0x7EF'`; `aria-label` = `'id 필터 (16진수 · 쉼표/공백 구분 · 범위 a-b · Enter)'`.
2. `filterNorm = el('span', 'ct-filter-norm')`, testid `can-filter-norm`, `hidden = true`, `title = 'UI 필터의 숫자는 16진수입니다 (CLI --id 의 숫자만 쓴 값은 10진수)'`. In `top.append(...)` put it right after `idFilter`.
3. `applyFilter`: on success after `curIds = ids`: `filterNorm.hidden = ids === null; setText(filterNorm, ids ? \`적용: ${ids.text}\` : '')`. On a parse error nothing else changes (the previous filter and chip stay).

**B. CAN-7 detail**
1. Hoist the B1 clipboard lambda into `const copyText = (t: string): Promise<void> => (typeof navigator !== 'undefined' && navigator.clipboard?.writeText ? navigator.clipboard.writeText(t) : Promise.reject(new Error('clipboard 없음')))` defined before `rxtx`; the RxTxView option becomes `copyText`.
2. DOM: `detailBox = el('div', 'ct-detail')` testid `can-row-detail`, `role="region"`, `aria-label="행 상세"`, `hidden = true`; children: `detailFields = el('div', 'ct-detail-fields')`, `tools = el('div', 'ct-detail-tools')` with `detailCopy` (`'복사'`, testid `can-row-detail-copy`), `detailClose` (`'닫기'`, testid `can-row-detail-close`), `detailMsg = el('span', 'ct-detail-msg')` (testid `can-row-detail-msg`, `aria-live="polite"`).
3. `let detail: RowFacts | null = null`.
4. `openDetail(abs)`: `f = rowFacts(model, abs)`; `null` → return. `detail = f`; `detailFields.replaceChildren(...rowDetailFields(f).map((d) => { const s = el('span', 'ct-detail-f'); s.dataset.k = d.key; s.append(el('b', 'ct-detail-k', d.label), el('span', 'ct-detail-v', d.value)); return s; }))`; `setText(detailMsg, ''); detailMsg.classList.remove('err'); detailBox.hidden = false`. (Snapshot at open; not refreshed live.)
5. `closeDetail()`: `detail = null; detailBox.hidden = true; detailFields.replaceChildren()`.
6. `copyDetail()`: `detail` null → return; `void copyText(rowDetailText(detail)).then(() => { setText(detailMsg, '복사함'); detailMsg.classList.remove('err'); }, () => { setText(detailMsg, '복사 실패 — 상세 줄에서 직접 선택해 복사하세요'); detailMsg.classList.add('err'); })`.
7. Wiring: TraceView option `onOpenDetail: (abs) => openDetail(abs)`; `detailCopy` click → `copyDetail()`; `detailClose` click → `closeDetail()`; `detailBox` keydown: `Escape` → `e.preventDefault(); closeDetail()`; always `e.stopPropagation()` (Space never toggles pause from inside the detail).

**C. CAN-6 close** (only when `bound !== null`; unbound creates none of this and makes no call)
1. DOM: `closeBtn = el('button', 'ct-close-btn', '채널 닫기…')` testid `can-close`, `title = '데몬에서 이 CAN 채널을 닫습니다 — 모든 사용자·에이전트의 주기 송신·ISO-TP가 멈춤. 탭 ×는 보기만 닫음'`, appended last in `top`. `closedBadge = el('span', 'ct-closed', '채널 닫힘')` testid `can-closed`, `hidden = true`, `title = '데몬 채널이 닫혔습니다 — 다시 열려면 CAN 모듈에서 채널 열기'`, placed right after `chLabel`.
   `closeBox = el('div', 'ct-closebox')` testid `can-close-box`, `role="group"`, `aria-label="채널 닫기 확인"`, `hidden = true`; children `closeLines = el('div', 'ct-closebox-lines')` (testid `can-close-lines`) and `el('div', 'ct-closebox-row')` holding `closeGo = el('button', 'ct-close-go', '채널 닫기')` (testid `can-close-confirm`), `closeCancel = el('button', undefined, '취소')` (testid `can-close-cancel`), `closeMsg = el('span', 'ct-close-msg')` (testid `can-close-msg`, `aria-live="polite"`).
2. Layout: `root.append(latch, connect?, top, closeBox (bound only), tableHost, detailBox, rxtx.root, send, status)` — i.e. `closeBox` right after `top`, `detailBox` right after `tableHost`.
3. State: `let closing: 'idle' | 'reading' | 'preview' | 'acting' = 'idle'`, `let impact: CloseImpact | null = null`, `let chState: 'open' | 'closed' | null = null`, `let closeAbort: AbortController | null = null`.
4. `setCloseLines(lines: string[])`: `closeLines.replaceChildren(...lines.map((l) => el('div', undefined, l)))`.
5. `openClosePreview = async (): Promise<void>`: `if (!bound || closing !== 'idle') return`; `closing = 'reading'`; `impact = null`; `setCloseLines(['읽는 중 — 주기 송신 목록 · 채널 상태'])`; `setText(closeMsg, ''); closeMsg.classList.remove('err')`; `closeGo.disabled = true; closeCancel.disabled = false; closeBox.hidden = false`; `const ac = new AbortController(); closeAbort = ac`; `const [l, s] = await Promise.allSettled([ctx.query('can.periodic.list', { channel }, ac.signal), ctx.query('can.status', { channel }, ac.signal)])` (this order); `if (closing !== 'reading' || ac.signal.aborted) return`; `impact = closeImpact(channel, l, s)`; `setCloseLines(closeImpactLines(impact))`; `closing = 'preview'`; `closeGo.disabled = false`; `ctx.scheduler.invalidate()`.
6. `confirmClose = async (): Promise<void>`: `if (closing !== 'preview') return`; `closing = 'acting'`; `closeGo.disabled = closeCancel.disabled = true`; `setText(closeMsg, '닫는 중…')`; then
   - `try { const hd = await ctx.act('close', { channel }); ctx.toast(closeDoneText(channel, hd.result, impact)); chState = 'closed'; closing = 'idle'; impact = null; closeBox.hidden = true; setText(closeMsg, ''); }`
   - `catch (e) { closing = 'preview'; closeGo.disabled = closeCancel.disabled = false; if (e instanceof GateError && e.code === 'E_CANCELLED') { setText(closeMsg, '취소됨 — 채널은 열려 있음'); closeMsg.classList.remove('err'); } else { setText(closeMsg, \`✗ 닫기 실패: ${e instanceof Error ? e.message : String(e)}\`); closeMsg.classList.add('err'); } }`
   - `finally { ctx.scheduler.invalidate(); }`
7. `cancelClose = (): void`: `closeAbort?.abort(); closeAbort = null; closing = 'idle'; impact = null; closeBox.hidden = true; setText(closeMsg, ''); closeBtn.focus(); ctx.scheduler.invalidate()`. (While `'acting'` the cancel button is disabled.)
8. Wiring: `closeBtn` click → `void openClosePreview()`; `closeGo` click → `void confirmClose()`; `closeCancel` click → `cancelClose()`; `closeBox` keydown: `Escape` and `closing !== 'acting'` → `e.preventDefault(); cancelClose()`; always `e.stopPropagation()`.
9. Control path: `const offCtl = bound ? ctx.onControl((ev) => { const st = channelStateOf(ev, channel); if (st) { chState = st; ctx.scheduler.invalidate(); } }) : () => {}`.
10. `updateStatus()` (bound only, after `writable` is known): `closedBadge.hidden = chState !== 'closed'`; `closeBtn.disabled = !(hasRight && !ctx.writeLocked) || closing !== 'idle' || chState === 'closed'`.
11. `dispose()`: `closeAbort?.abort(); offCtl();` (before `offFrame()`).
12. No timers, no polling, no retries: one read pair per click on `채널 닫기…`, one `ctx.act('close')` per confirm (ActionGate shows the card).

## B2c-REVIEW (Opus) — ACCEPTED (B2 complete: CAN-6/7/8)

Worker hash `cat traceView.ts rxtxView.ts canTrace.ts | shasum -a 256` = `8300872d92f3e2ee002708bca9f300f35c1cd101404b39eb72aa0b0a25949808` (verified); spec/model/CSS untouched. vitest 182/182; app/node tsc 0; eslint 0. Deviation accepted: the contract said `ctx.toast`; the SDK path is `ctx.ui.toast` (contract typo, worker correct).

---

## B4a-ALLOW — exclusive file lease (Rust `can.export`, CAN-4)

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `crates/loom-can/src/export.rs` | `StagedExport::{create, push, summary, commit, abort}`, `safe_label`, `output_path`, `export_origin`, `frame_record`, `gap_record`, `wanted`, `run_export` (all `unimplemented!("UNIMPLEMENTED(B4): …")`) |

Opus scaffold done (do not change): constants, `ExportFormat`, `ExportParams`, `Cut`, `StagedExport` fields, `ExportH` + its `call`, signatures, the registration + params schema in gateway.rs (`RPC_EXPORT` after `RPC_STATUS`, `read_spec`, `EXPORT_MAX_WAIT`), `pub(crate)` on `IdArg`/`IdArg::range`/`row_matches`, lib.rs `pub mod export` + `RPC_EXPORT`, Cargo.toml optional dep under `runtime`. Cargo.lock gained exactly one line (`loom-can-export` in loom-can's deps) from `cargo check` — ROOT-owned generated file, reported. Spec tests (read-only): `crates/loom-can/tests/export.rs`, and `tests/gateway.rs` (registry list now includes `can.export`).
Imports: everything needed is imported at the top of export.rs (placeholder `let _ = (…)` lines reference them only until bodies exist; remove those lines). You may additionally `use` items that already exist in this crate/workspace inside function bodies only if a body needs them (e.g. `crate::gateway::{frame_filter, next_from, seq_cursor, resolve_channel, parse, usage, publish_activity, row_matches, MAX_ROWS, SCAN_BYTES}`, `crate::{KIND_FRAME, RPC_EXPORT}`, `loom_domain_api::JournalQuery`, `loom_proto::error::ErrorCode`, `loom_proto::cursor::TimeUnit`, `serde_json::{json, Map}`, `std::fs`, `std::io`); prefer a function-local `use`. No new dependencies, no other file.
Acceptance (worktree root, focused only): `CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo test -p loom-can` → all pass; same env `cargo clippy -p loom-can --tests -- -D warnings` → clean; `cargo fmt -p loom-can --check` → clean (run `cargo fmt -p loom-can` only if it touches export.rs alone — if it would touch other files, report instead). Scaffold state: `--test export` 8 failed / 1 passed; `--test gateway` 23/23.
Scaffold hash: `cat crates/loom-can/src/export.rs crates/loom-can/tests/export.rs | shasum -a 256` (value in STATUS.md). Return `B4a-WORKER.md` with result hash `shasum -a 256 crates/loom-can/src/export.rs`.

## B4-EXPORT — `export.rs`

Errors: `usage(msg)` → `E_USAGE`; `DomainError::new(ErrorCode::Internal, msg)` → `E_INTERNAL`. Never panic; no `unwrap`/`expect` (workspace lints).

**`safe_label(s)`**: take the first 64 chars; map every char not `[A-Za-z0-9._-]` to `'_'`; then trim leading `'.'`; empty → `"x"`.

**`output_path(dir, explicit, channel, tag, format)`**
- `Some(p)`: `PathBuf::from(p)`; not `is_absolute()` → `usage("export path must be absolute (daemon machine): {p}")`. If the path or `"{p}.loom.jsonl"` exists (`symlink_metadata` ok) → `usage("export path exists, refusing to overwrite: {p}")`. Else return it.
- `None`: `stem = format!("loom-can-{}-{}", safe_label(channel), safe_label(tag))`; for `n in 0..DEFAULT_FILE_ATTEMPTS`: name `"{stem}.{ext}"` (n = 0) else `"{stem}-{n}.{ext}"`; candidate `dir.join(name)`; usable when none of `p`, `p.loom.jsonl`, `p.partial`, `p.loom.jsonl.partial` exists → return it. All taken → `E_INTERNAL` `"{DEFAULT_FILE_ATTEMPTS} names taken for {stem}*.{ext} in {dir}"`. (The library's `create_new` on the partials still guards a race.)

**`export_origin(o)`**: `Agent/User/Device/Script` → same library variant.

**`frame_record(rec)`**: `rec.kind != KIND_FRAME` → `None`. `fp: FramePayload = serde_json::from_value(rec.payload.clone()).ok()?`; `frame = fp.to_frame().ok()?`; `dir = match rec.dir { Some(Dir::Tx) => Direction::Tx, _ => Direction::Rx }` (`loom_can_frame::Direction`, function-local use). `wall = rec.ts.wall_ns.get()`; `ExportFrame { ts_mono_ns: rec.ts.mono_ns.get(), wall_ns: (wall > 0).then_some(wall), channel_index: 0, dir, frame, origin: export_origin(rec.origin), device_ts: None, metadata: Metadata { request_id: rec.request_id.clone().filter(|r| !r.is_empty() && r.len() <= loom_can_export::MAX_LABEL_BYTES), source: Some(SourceRef { producer_id, producer_epoch, source_seq }) when producer_id is 1..=MAX_LABEL_BYTES bytes else None, time_quality: TimeQuality::Exact } }` → `Some(ExportRecord::Frame(..))`. (`producer_id`/`producer_epoch`/`source_seq` from the record; `.get()` on the U64 wrappers.)

**`gap_record(g, at)`**: `scope = if g.seq_range.is_none() && g.producer_range.is_some() { GapScope::Source } else { GapScope::Record }`; `count = match g.count { GapCount::Exact(n) if n.get() > 0 => Some(n.get()), _ => None }` (0 is not allowed by the library; unknown stays None); `reason` = `g.cause` truncated to ≤ 256 bytes on a char boundary (empty → `"unknown"`); `metadata: Metadata::default()` with `time_quality: TimeQuality::Exact`; `ts_mono_ns: at`.

**`wanted(scan, ids, dir)`**: `scan.events.iter().filter(|r| r.kind == KIND_FRAME && row_matches(r, ids, dir)).collect()`.

**`StagedExport`** — mirror `loom_can_export::export_file` (files.rs) step by step:
- `create(path, format, options)`: `paths = ExportPaths::new(path)?`; primary or sidecar already exists → `Err(Error::AlreadyExists)`; open `primary_partial` with `OpenOptions::new().write(true).create_new(true)`; open `sidecar_partial` the same (if this fails, remove `primary_partial`); `Exporter::new(BufWriter::with_capacity(64 * 1024, primary), BufWriter::with_capacity(64 * 1024, sidecar), format, options)` — on error remove both partials. → `Ok(Self { paths, exporter })`.
- `push(r)` → `self.exporter.push(r)`. `summary()` → `self.exporter.summary()`.
- `commit(self)`: on any error below, remove both partials (best effort) and return the error. `let (mut p, mut s, summary) = exporter.finish()?`; `p.flush()?; s.flush()?; p.get_ref().sync_all()?; s.get_ref().sync_all()?`; drop both; re-check primary/sidecar absent (else `Error::AlreadyExists`); `fs::rename(sidecar_partial, sidecar)?`; `fs::rename(primary_partial, primary)` — if it fails, `let _ = fs::rename(sidecar, sidecar_partial)` then remove both partials and return the error. → `Ok(summary)`.
- `abort(self)`: drop the exporter, `let _ = fs::remove_file(..)` for both partials.

**`run_export(ctx, table, params)`**
1. `p: ExportParams = parse(params)?`; `channel = resolve_channel(table, p.channel.as_deref())?`; `ids = p.ids.iter().map(IdArg::range).collect::<Result<Vec<_>, _>>()?`.
2. `tag = ctx.request_id()` (owned) or, when None, the current wall clock ns as a decimal string. `path = output_path(&std::env::temp_dir(), p.path.as_deref(), &channel, &tag, p.format)?` — before any scan.
3. `from = p.since.clone().unwrap_or(Cursor::Relative { amount: EXPORT_DEFAULT_SINCE_MINS, unit: TimeUnit::Mins })`.
4. State: `writer: Option<StagedExport> = None`, `clock: Option<String>`, `anchor_wall: Option<u64>`, `last_mono: u64`, `frames: u64`, `first_seq/last_seq: Option<U64>`, `seen: Vec<GapData>` (distinct, ≤ `EXPORT_MAX_GAPS`), `pending: Vec<GapData>`, `gaps_dropped: u64`, `scans_complete = true`, `cut: Option<Cut>`, `next: Option<Cursor>`.
5. For `i in 0..EXPORT_MAX_SCANS`: `q = JournalQuery { from: from.clone(), until: p.until.clone(), filter: frame_filter(&channel, &[KIND_FRAME], p.origins.clone()), request_id: None, max_events: MAX_ROWS }`; `scan = ctx.journal().scan(&q, SCAN_BYTES).await?` (on error: abort the writer if any, return the error); `scans_complete &= scan.complete`.
   - Gaps: for each `g` in `scan.gaps` not already in `seen` (`==`): `seen.len() < EXPORT_MAX_GAPS` → push to `seen` and `pending`; else `gaps_dropped += 1`.
   - Rows: for `rec` in `wanted(&scan, &ids, p.dir)`:
     - `frames == EXPORT_MAX_FRAMES` → `cut = Some(Cut::MaxFrames); next = Some(seq_cursor(rec, 0))`; stop all scanning.
     - `clock` is Some and `rec.ts.clock_id != clock` → `cut = Some(Cut::ClockChange); next = Some(seq_cursor(rec, 0))`; stop all scanning.
     - `Some(r) = frame_record(rec)` else skip the row (not counted).
     - First written row: `clock = Some(rec.ts.clock_id.clone())`; `anchor_wall` = the row's wall when > 0; `writer = Some(StagedExport::create(&path, p.format.format(), Options { anchor_mono_ns: mono, anchor_wall_ns: anchor_wall, compressed: false }))` — error → `E_INTERNAL` `"cannot create {path}: {e}"`.
     - Write each `pending` gap as `gap_record(g, mono)` (then clear), push `r`; any push error → abort the writer, `E_INTERNAL` `"export write: {e}"`. `frames += 1`, `last_mono = mono`, `first_seq.get_or_insert(rec.seq)`, `last_seq = Some(rec.seq)`.
   - After the rows of a scan, if the writer exists, write remaining `pending` gaps at `last_mono` (so gaps are not lost when the gap scan has no rows).
   - Continue: `more = scan.resume_from.is_some() || scan.events.len() as u32 == MAX_ROWS`; `match next_from(&scan) { Some(n) if more => if i + 1 == EXPORT_MAX_SCANS { cut = Some(Cut::ScanBudget); next = Some(n); stop } else { from = n }, _ => stop }`.
6. Finish: writer `Some(w)` → `w.commit()` (error → `E_INTERNAL` `"export commit: {e}"`); `None` → no file (`path`/`sidecar` null). `gaps = seen.len()`.
7. Activity: `range = first/last → {"from","to"}` (SeqRange JSON as in trace). `extra = {"action": "read", "range": range when Some}`; `publish_activity(ctx, RPC_EXPORT, &channel, format!("export {channel} ({frames} frames) → {path or 'no file'}"), extra)` (it already skips non-agent roles).
8. Result object (exact keys):
```
{ "channel", "format": "blf"|"asc", "location": "daemon",
  "path": String|null, "sidecar": String|null,          // sidecar = ExportPaths.sidecar
  "frames": u64, "gaps": u64, "gaps_dropped": u64,
  "truncated": cut.is_some(), "truncated_reason": cut.as_str()|null,
  "next_cursor": next.to_string()|null,                  // only when truncated
  "complete": cut.is_none() && scans_complete && gaps == 0 && gaps_dropped == 0,
  "range": {"from","to"}|null, "clock_id": String|null, "wall_anchor_known": anchor_wall.is_some() }
```
Paths as `to_string_lossy()`.

---

## B4a-REVIEW (Opus) — ACCEPTED

Worker hash `export.rs` = `a5844a2ebd97bd0a76ada0afeb2fa64b47c26fb6232a7e1b643ea861f8b594ae` (verified). `cargo test -p loom-can --no-fail-fast`: lib 8, export 9/9, gateway 24/24, generation_tail 4, isotp 15, periodic 16, signals 23, worker 17 — all pass; only the 2 B4b CLI specs fail (scaffold by design). clippy: only the 2 unused B4b stubs in cli.rs (mine). fmt: export.rs clean; Opus ran `cargo fmt -p loom-can` on its own files (gateway.rs, tests/export.rs, tests/gateway.rs, tests/cli.rs).
Deviation accepted: when `create` hits AlreadyExists on a *default* path (parallel exports with the same request tag), `run_export` re-picks a name via `output_path`, bounded by `DEFAULT_FILE_ATTEMPTS`; an explicit path is never changed.

## B5-SCHEMA (Opus, engine fix) — DONE

`can.trace.query` and `can.last` param schemas now list `decode: boolean` (handlers already read it; `loom can trace --decode` would otherwise fail schema validation, `additionalProperties: false`). Test `tests/gateway.rs::trace_and_last_schemas_accept_decode`. Backward compatible.

## B4b-ALLOW — exclusive file lease (CLI + UI export, CAN-4)

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `crates/loom-can/src/cli.rs` | `export_call`, `render_export` (stubs); in `to_call` the match arm `"export" => export_call(s, p),` (next to `"trace"`); in `render_human` the dispatch arm `RPC_EXPORT => render_export(d, out)?,` |
| `ui/src/panels/can/exportReq.ts` | `exportIdTokens`, `ringSeqs`, `exportRequest`, `exportResultLines` |
| `ui/src/panels/can/canTrace.ts` | `createCanTrace` body only (§B4-PANEL) |

Opus scaffold done: clap `export` subcommand (args `-c`, `--since`, `--until`, `--ids`, `--origin`, `--dir`, `--format` default blf, `-o/--out`), `EXPORT_WAIT`, `use crate::RPC_EXPORT`, stubs; exportReq.ts types/constants; manifest action `export` (job/watch/none); imports in canTrace.ts; CSS. Spec (read-only): `crates/loom-can/tests/cli.rs` (`export_conversion`, `export_render_golden`), `ui/src/panels/can/exportReq.test.ts`, `ui/src/panels/can/canB4Panel.test.ts`.
Acceptance: Rust (root, focused) `cargo test -p loom-can` all pass + `cargo clippy -p loom-can --tests -- -D warnings` + `cargo fmt -p loom-can --check`; UI (in `ui/`) `npx vitest run src/panels/can` all pass, isolated app/node tsc 0, `npx eslint src/panels/can` 0.
Scaffold hash: `cat crates/loom-can/src/cli.rs crates/loom-can/tests/cli.rs ui/src/panels/can/exportReq.ts ui/src/panels/can/exportReq.test.ts ui/src/panels/can/canTrace.ts ui/src/panels/can/canB4Panel.test.ts | shasum -a 256` (value in STATUS.md). Return `B4b-WORKER.md` with result hash `cat crates/loom-can/src/cli.rs ui/src/panels/can/exportReq.ts ui/src/panels/can/canTrace.ts | shasum -a 256`.

## B4-CLI — `cli.rs`

**`export_call(s, mut p)`** (`p` already holds `channel` when given):
- `since`/`until` via `cursor_param`, `ids` (`get_many::<String>("ids")` → array), `origins` (from `origin`), `dir` — exactly as the `"trace"` arm does.
- `format`: `s.get_one::<String>("format")` (clap default `"blf"`) → `p["format"]`.
- `out`: when given — empty string → `UsageError::new("--out needs a file path")`; absolute (`Path::is_absolute`) → as is; relative → `std::env::current_dir()` joined (error → `UsageError::new("cannot resolve a relative --out; give an absolute path")`); stored as `to_string_lossy()` in `p["path"]`.
- `let mut call = RpcCall::new(RPC_EXPORT, Value::Object(p)); call.wait = Some(EXPORT_WAIT + WAIT_MARGIN); Ok(call)`.

**`render_export(d, out)`** (golden in tests/cli.rs; `–` is U+2013):
- `d["path"]` is not a string → write `no frames in the selection — no file written\n`; then the INCOMPLETE line rule below; return.
- Line 1: `exported {frames} frames ({format}) → {path}  [daemon machine]` (two spaces before `[`).
- Line 2: `sidecar {sidecar}`.
- Line 3: `seq {from}–{to}` when `range` is an object (else `seq ?`), then ` · clock {clock_id}` when a string, ` · gaps {gaps}` always, ` (+{gaps_dropped} not listed)` when > 0, ` · wall clock unknown (times relative)` when `wall_anchor_known` is false.
- When `truncated`: `TRUNCATED ({truncated_reason}) — continue: --since {next_cursor}`.
- INCOMPLETE line: `gaps > 0 || gaps_dropped > 0` → `INCOMPLETE — journal gaps inside the range (gap records in the file and sidecar)`; else `complete == false && !truncated` → `INCOMPLETE — the journal scan did not cover the whole range`.

## B4-UI — `exportReq.ts`

**`exportIdTokens(f)`**: items sorted by start like `IdFilter.text`: singles → `fmtFilterId(id)`, ranges → `` `${fmtFilterId(lo)}-${fmtFilterId(hi)}` ``.

**`ringSeqs(m, gapKind)`**: `first` = for `abs` from `m.oldestAbs` up to `min(m.total, oldestAbs + RING_PROBE_ROWS) - 1`: first row with `kindAt(abs) !== gapKind && seqAt(abs) > 0` → its seq; `last` = same scanning down from `m.total - 1` to `max(m.oldestAbs, m.total - RING_PROBE_ROWS)`. None → `null`.

**`exportRequest(i)`** — order of checks and params keys exactly:
1. `params = { channel, format }`.
2. scope `'ring'`: `ringFirstSeq`/`ringLastSeq` null → `{ ok: false, error: '이 패널에 보관된 프레임이 없습니다 — "최근 10분"을 고르세요' }`; else `since: \`seq:${first}\``, `until: \`seq:${last}\``, `scopeText = \`이 패널 보관 범위 seq ${first}–${last}\``. scope `'recent'`: `since: EXPORT_RECENT_SINCE`, `scopeText = '최근 10분'`.
3. `path = i.path.trim()`; non-empty: length > `EXPORT_PATH_MAX` → `'경로가 너무 깁니다 (≤ 4096자)'`; not (`startsWith('/')` or `/^[A-Za-z]:[\\/]/`) → `'경로는 데몬 컴퓨터의 절대 경로여야 합니다 (비우면 임시 폴더)'`; else `params.path = path`.
4. `useFilter`: `ids` → `tokens = exportIdTokens(ids)`; `tokens.length > EXPORT_IDS_MAX` → `` `id 필터 항목 ${n}개 — 내보내기는 ${EXPORT_IDS_MAX}개까지 (범위로 묶으세요)` ``; else `params.ids = tokens`. `dir !== 'all'` → `params.dir`. `origins` non-null: size 0 → `'출처를 하나 이상 고르세요'`; else `params.origins = ORIGIN_ORDER.filter((o) => origins.has(o))`. `scopeText += ' · 현재 필터'`.
5. `{ ok: true, params, scopeText }`.

**`exportResultLines(r)`**: unknown shape (`r` not a non-null object, or `path` neither string nor null, or — when `path` is a string — `sidecar` not a string, `frames`/`gaps` not numbers, `format` not a string) → `{ lines: ['결과 형식을 알 수 없음'], path: null, warn: true }`.
- `gd = typeof gaps_dropped === 'number' ? gaps_dropped : 0`; `complete = r.complete === true`; `truncated = r.truncated === true`.
- `path === null` → `lines = ['선택 범위에 프레임이 없어 파일을 만들지 않았습니다']`.
- else: `` `데몬 컴퓨터에 저장됨 (브라우저 다운로드 아님): ${path}` ``, `` `사이드카: ${sidecar}` ``, then `` `${frames}프레임 · ${format.toUpperCase()} · seq ${from}–${to} · 갭 ${gaps}` `` (`seq ?` when `range` is not an object) + (`gd > 0` → `` ` (+${gd} 미기록)` ``) + (`wall_anchor_known !== true` → `' · 벽시계 없음 (상대 시각)'`); when `truncated`: `` `잘림 (${reason}) — 이어서: since ${next_cursor}` `` with reason `max_frames` → `파일당 1,000,000프레임 상한`, `scan_budget` → `스캔 예산`, `clock_change` → `데몬 시계 바뀜`, other → the raw text.
- Then (both branches): `gaps > 0 || gd > 0` → `'⚠ 범위 안에 저널 손실 있음 (파일·사이드카에 갭 기록)'`; else `!complete && !truncated` → `'⚠ 저널 스캔이 범위 전체를 덮지 못함'`.
- `{ lines, path, warn: !complete }`.

## B4-PANEL — `canTrace.ts` (`createCanTrace` body; bound only — unbound creates none of this)

1. DOM: `exportBtn = el('button', undefined, '내보내기…')` testid `can-export`, `title = 'BLF/ASC 파일을 데몬 컴퓨터에 만듭니다 (브라우저 다운로드 아님)'`, appended in `top` right before `closeBtn`.
   `exportBox = el('div', 'ct-exportbox')` testid `can-export-box`, `role="group"`, `aria-label="내보내기"`, `hidden = true`, placed in `root` right after `closeBox`. Row 1 `el('div', 'ct-exportbox-row')`: `fmtSel` select testid `can-export-format` (options `blf` "BLF", `asc` "ASC"); `scopeSel` select testid `can-export-scope` (options `ring` "이 패널 보관 범위", `recent` "최근 10분"); a `label` containing checkbox `useFilterCb` (testid `can-export-usefilter`) + text `' 현재 id·방향·출처 필터 적용'`; `pathIn = el('input', 'ct-export-path')` testid `can-export-path`, `placeholder = '비우면 데몬 임시 폴더에 새 파일 (데몬 컴퓨터 절대 경로)'`, `aria-label = '데몬 쪽 저장 경로'`. Row 2: `goBtn` `'내보내기'` testid `can-export-go`; `copyBtn` `'경로 복사'` testid `can-export-copy` `hidden = true`; `closeExp` `'닫기'` testid `can-export-close`; `expMsg = el('span', 'ct-export-msg')` testid `can-export-msg`, `aria-live="polite"`. Then `expResult = el('div', 'ct-export-result')` testid `can-export-result`.
2. State: `let exporting = false`, `let exportPath: string | null = null`, `let curFilter: TraceFilter = NO_FILTER`-equivalent — use the values `applyFilter` already computes: keep `let curDir: 'all' | 'rx' | 'tx' = 'all'` and `let curOrigins: ReadonlySet<Origin> | null = null` updated in `applyFilter` next to `curIds` (and `curIdFilter: IdFilter | null` holding the parsed `ids`).
3. `openExport()`: `exportBox.hidden = false; setText(expMsg, ''); expMsg.classList.remove('err')`. `closeExport()`: `exportBox.hidden = true`.
4. `runExport = async (): Promise<void>`: `if (!bound || exporting) return`; `const rs = ringSeqs(model, K_GAP)`; `req = exportRequest({ channel, format: fmtSel.value as ExportFormatUi, scope: scopeSel.value as ExportScope, ringFirstSeq: rs.first, ringLastSeq: rs.last, useFilter: useFilterCb.checked, ids: curIdFilter, dir: curDir, origins: curOrigins, path: pathIn.value })`; `!req.ok` → `setText(expMsg, req.error); expMsg.classList.add('err'); return`. Then `exporting = true; goBtn.disabled = true; setText(expMsg, \`내보내는 중… (${req.scopeText}, 최대 600초)\`); expMsg.classList.remove('err')`;
   `try { const hd = await ctx.act('export', req.params); const v = exportResultLines(hd.result); expResult.replaceChildren(...v.lines.map((l) => el('div', undefined, l))); expResult.classList.toggle('warn', v.warn); exportPath = v.path; copyBtn.hidden = v.path === null; setText(expMsg, ''); }`
   `catch (e) { setText(expMsg, \`✗ 내보내기 실패: ${e instanceof Error ? e.message : String(e)}\`); expMsg.classList.add('err'); }`
   `finally { exporting = false; goBtn.disabled = false; ctx.scheduler.invalidate(); }`
5. `copyBtn` click → `exportPath` null → return; `void copyText(exportPath).then(() => setText(expMsg, '경로를 복사함'), () => { setText(expMsg, '복사 실패 — 결과 줄에서 직접 선택하세요'); expMsg.classList.add('err'); })`.
6. Wiring: `exportBtn` click → `openExport()`; `goBtn` click → `void runExport()`; `closeExp` click → `closeExport()`; `exportBox` keydown: `Escape` → `e.preventDefault(); closeExport()`; always `e.stopPropagation()`.
7. `updateStatus()` (bound): `exportBtn.disabled = ctx.writeLocked` (ActionGate refuses while STALLED even for watch).
8. Imports are complete (Opus added `K_GAP` from `./traceModel`; `Origin`, `IdFilter`, exportReq symbols are imported). No import edits.

## B4b-REVIEW (Opus) — ACCEPTED (B4 complete: CAN-4)

Worker hash `cat cli.rs exportReq.ts canTrace.ts | shasum -a 256` = `1424929c16ab16a2931413daa27aa957e83e930040f6f5d7ef2ac0fb9e316950` (verified). The clippy finding was in Opus's spec (`tests/export.rs` constant assertion) — removed by Opus. After that: `cargo test -p loom-can --no-fail-fast` all suites pass (cli 16, export 9, gateway 24, …); clippy `-D warnings` clean; fmt clean; vitest CAN 200/200 (9 consecutive clean runs; 1 earlier run had 1 failure while cargo compiled in parallel — not reproducible, recorded as a load-sensitive flake risk); app/node tsc 0; eslint 0.

---

## B3 overview (CAN-2 periodic send)

- **B3a (this lease)**: pure `periodicJobs.ts` + DOM class `periodicView.ts`. No RPC.
- **B3b (next)**: wiring — TX editor Cycle Time, TX toolbar 주기 시작/정지, job table in the Transmit view, manifest actions `periodic.start|update|stop|takeover`, one `can.periodic.list` at bound mount + on refresh, live `can.job_state` via `ctx.onControl`, STOP latch.
- "Mine" = jobs this panel started or took over in this session (the panel cannot know its daemon client id). Every other job is read-only with an explicit 넘겨받기 behind a confirm card. The engine's `origin=user` bypass (periodic.rs `periodic_authorize`) is never used implicitly.

## B3a-ALLOW — exclusive file lease

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `periodicJobs.ts` | `JobTable.{applyList, applyEvent, get, list}`, `jobNumber`, `jobStateText`, `parseCycle`, `startParams` |
| `periodicView.ts` | `PeriodicView.{constructor, setStatus, setLatched, setWritable, render, fillRow}` |

Specs (read-only): `periodicJobs.test.ts`, `periodicView.test.ts`. CSS done by Opus. Acceptance (in `ui/`): vitest `src/panels/can` all pass (213); isolated app/node tsc 0; eslint 0. Scaffold state: 13 failed / 200 passed; tsc 0; eslint 0.
Scaffold hash: `cat ui/src/panels/can/periodicJobs.ts ui/src/panels/can/periodicJobs.test.ts ui/src/panels/can/periodicView.ts ui/src/panels/can/periodicView.test.ts | shasum -a 256` (STATUS.md). Return `B3a-WORKER.md`, result hash `cat periodicJobs.ts periodicView.ts | shasum -a 256`.

## B3-JOBS — `periodicJobs.ts`

`count(v)` = `typeof v === 'number' && Number.isInteger(v) && v >= 0` (function-local helper).

**`applyList(v, channel)`**: `v` must be a non-null object with array `jobs`, else `return false` (nothing changes). Take entries in order; skip entries that are not objects or lack any of: `job` string, `channel` string, `id` count, `ext` boolean, `owner` string, `origin` string, `period_ms` count, `state` string; skip `channel !== channel`; stop after `JOBS_MAX` accepted. Row: `{ job, id, ext, owner, origin, periodMs: period_ms, state, revision: count(revision) ? revision : null, sent: count|null, missed: count|null, leaseLeftMs: count(lease_left_ms) ? … : null, reason: null }`. Then: delete every existing row whose job is NOT in `this.stopped` (active rows are replaced wholesale); for each listed row: remove its job from `this.stopped` and set it. `loaded = true; version++; return true`.

**`applyEvent(ev, channel)`**: `ev.kind !== 'can.job_state' || ev.channel !== channel` → false. `p = ev.payload` (object, else false); `p.job` string and `p.state` string, else false.
- `prev = rows.get(job)`; base = `prev ?? { job, id: count(p.id) ? p.id : 0, ext: p.ext === true, owner: '?', origin: '?', periodMs: null, state: 'running', revision: null, sent: null, missed: null, leaseLeftMs: null, reason: null }` (copy, don't mutate prev in place before deciding).
- `owner`/`origin`: payload string → replace. `revision`: payload count → replace. `reason = typeof p.reason === 'string' ? p.reason : null`.
- state: `'started' | 'patched' | 'renewed' | 'running'` → `'running'`; `'paused'` → `'paused'`; `'stopped'` → `'stopped'`; anything else → the payload text.
- `'stopped'`: `stopped = [job, ...stopped.filter((j) => j !== job)]`; while `stopped.length > STOPPED_KEEP`: pop the oldest and `rows.delete` it. Otherwise remove `job` from `stopped`.
- `rows.set(job, row); version++; return true`.

**`get(job)`** → the row. **`list()`** → active rows (state !== 'stopped') sorted by `jobNumber` ascending, then rows of `this.stopped` in that order (skip missing).

**`jobNumber(job)`**: `/^p(\d+)$/` → Number of the digits; else `Number.MAX_SAFE_INTEGER`.

**`jobStateText(r)`**: `running` → `'실행 중'` (never with a reason); `paused` → `'일시정지'`, `stopped` → `'정지됨'`, any other state → `r.state` — for these three cases append `` ` (${r.reason})` `` when `reason` is non-null.

**`parseCycle(text)`**: `t = text.trim()`; `''` → `{ ok: true, ms: 0 }`; `/^\d+$/.test(t) && Number(t) <= CYCLE_MAX_MS` → `{ ok: true, ms: Number(t) }`; else `{ ok: false, error: 'Cycle Time: 0(수동) 또는 1–60000 ms 정수' }`.

**`startParams(e, channel)`**: `Number.isInteger(e.cycleMs) && e.cycleMs >= 1 && e.cycleMs <= CYCLE_MAX_MS` else `{ ok: false, error: '주기 0 = 수동 송신 — Cycle Time(ms)을 1–60000으로 정하세요' }`; → `{ ok: true, params: { channel, frame: cansendText(e.params), period_ms: e.cycleMs } }`.

## B3-VIEW — `periodicView.ts`

Local helpers allowed inside function bodies only (`document.createElement`, `textContent`). Each button `type = 'button'`.

**constructor**: build
- `this.root = div.crx-periodic` (`data-testid="can-periodic"`, `role="region"`, `aria-label="주기 송신 작업"`).
- head `div.crx-periodic-head`: `span.crx-periodic-title` `'주기 송신 작업 (데몬 · 모든 소유자)'`; `this.refreshBtn` `'목록 새로고침'` (testid `can-periodic-refresh`, click → `hooks.refresh()`); `this.status = span.crx-periodic-status` (testid `can-periodic-status`, `aria-live="polite"`).
- `this.banner = div.crx-periodic-stop` (testid `can-periodic-stop`, `hidden = true`, text `'STOP — 주기 송신 정지됨 (새 시작 불가 · 정지는 가능)'`).
- header `div.crx-job-head` with one `span` per `JOB_COLS` entry.
- `this.body = div.crx-job-body` (testid `can-periodic-body`); `this.empty = div.crx-job-empty.muted` (testid `can-periodic-empty`).
- `root.append(head, banner, header, body, empty)`. No render in the constructor.

**`setStatus(text, err)`**: set `status.textContent` (only if different); `status.classList.toggle('err', err)`.
**`setLatched(l)`**: `this.latched = l; this.banner.hidden = !l`.
**`setWritable(w)`**: if `w !== this.writable`: `this.writable = w; this.renderedVersion = -1`.
**`render(force = false)`**: `if (!force && this.renderedVersion === this.table.version) return`. `rows = table.list()`; `empty.hidden = rows.length > 0`; empty text `table.loaded ? '이 채널에 데몬 주기 송신 작업 없음' : '목록을 아직 읽지 않음 — 새로고침'`. Rebuild: `body.replaceChildren(); els.clear();` for each row: `e = div.crx-job`, `fillRow(e, r)`, append, `els.set(r.job, e)`. `renderedVersion = table.version`.
**`fillRow(e, r)`**: `mine = hooks.isMine(r.job)`; `e.dataset.job = r.job; e.dataset.state = r.state; e.dataset.mine = String(mine)`; `e.title = mine ? '이 보기가 시작했거나 넘겨받은 작업' : '다른 소유자의 작업 — 읽기 전용 (넘겨받기 = 확인 카드)'`. Eight `span` cells, text in order: `r.job`; `fmtId(r.id, r.ext)`; `r.periodMs === null ? '?' : String(r.periodMs)`; `` `${r.origin} ${r.owner}` ``; `jobStateText(r)`; `` `${r.sent ?? '?'} / ${r.missed ?? '?'}` ``; `r.leaseLeftMs === null ? '—' : \`${(r.leaseLeftMs / 1000).toFixed(1)} s\``; action cell: `state === 'stopped'` → empty; `mine` → button `'정지'` testid `can-job-stop`, title `'이 작업 정지 (안전 방향 — 확인 없이)'`, never disabled, click → `hooks.stop(r.job)`; else button `'넘겨받기'` testid `can-job-takeover`, `disabled = !this.writable`, title `'넘겨받으면 이 보기가 이 작업을 조작합니다 (확인 카드)'`, click → `hooks.takeover(r.job)`.

## B3a-REVIEW (Opus) — ACCEPTED

Worker hash `cat periodicJobs.ts periodicView.ts | shasum -a 256` = `e895097f2b8caa09f5a0d90d8ce6d71861b9672f9c89bc45c00d17dbc96f3783` (verified). vitest 213/213; app/node tsc 0; eslint 0. Bodies match §B3-JOBS/§B3-VIEW.

---

## B3b-ALLOW — exclusive file lease (periodic wiring, CAN-2)

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `txList.ts` | `TxList.upsert` body (new optional `cycleMs`) |
| `rxtxView.ts` | constructor (Cycle input, toolbar buttons, remove the old "미지원" label, `periodicHost`), `openEditor`, `validate`, `commitEditor`, `renderTx`, `setLatched`, `periodicChanged` |
| `canTrace.ts` | `createCanTrace` body only (§B3-PANEL) |

Opus scaffold done: `upsert` signature, RxTxHooks `startPeriodic/stopPeriodic/periodicOf`, fields `edCycle/perStartBtn/perStopBtn/periodicHost/latched`, stubs `setLatched/periodicChanged`, imports (`parseCycle`, `jobStateText`, `JobTable`, `startParams`, `PeriodicView`), manifest actions `periodicStart` (job/operate/none), `periodicStop` (job/operate/none, `safetyDirection: 'reduce'`), `periodicTakeover` (job/operate/card); CSS. Opus also updated its own older tests: `rxtxPanel.test.ts` (cycle input now enabled, "미지원" label gone) and `canB2Panel.test.ts` (close reads counted after the click, since the bound panel now reads the job list once at mount).
Spec (read-only): `canB3Panel.test.ts`. Acceptance (in `ui/`): vitest `src/panels/can` all pass; isolated app/node tsc 0; eslint 0.
Scaffold hash: `cat ui/src/panels/can/txList.ts ui/src/panels/can/rxtxView.ts ui/src/panels/can/canTrace.ts ui/src/panels/can/canB3Panel.test.ts ui/src/panels/can/rxtxPanel.test.ts ui/src/panels/can/canB2Panel.test.ts | shasum -a 256` (STATUS.md). Return `B3b-WORKER.md`, result hash `cat txList.ts rxtxView.ts canTrace.ts | shasum -a 256`.

## B3-RXTX — `txList.ts`, `rxtxView.ts`

**`TxList.upsert(uid, params, text, comment, cycleMs?)`**: as today, plus — edit of an existing entry: `if (cycleMs !== undefined) e.cycleMs = cycleMs`; new entry: `cycleMs: cycleMs ?? 0`. Versions bump as today.

**constructor** (rxtxView):
- Replace the local `cycle` input with `this.edCycle = tid(el('input', 'crx-cycle'), 'can-tx-cycle')`: `value = '0'`, `inputMode = 'numeric'`, enabled, `title = '0 = 수동 · 1–60000 ms = 데몬 주기 송신 (목록에서 "주기 시작")'`, `input` listener → `this.validate()`; `row2.append(this.labeled('Cycle Time ms', this.edCycle), types)`.
- Delete the `defer` span (`can-tx-periodic-deferred`) and its title; it is no longer appended.
- `this.perStartBtn = tid(el('button', undefined, '주기 시작'), 'can-tx-periodic-start')`, `type = 'button'`, `title = '선택 항목을 데몬 주기 송신 작업으로 시작 (Cycle Time > 0)'`, click → `if (this.tx.selected !== null) this.hooks.startPeriodic(this.tx.selected)`. `this.perStopBtn = tid(el('button', undefined, '주기 정지'), 'can-tx-periodic-stop')`, `type = 'button'`, `title = '이 보기가 시작한 선택 항목의 주기 작업 정지'`, click → `if (this.tx.selected !== null) this.hooks.stopPeriodic(this.tx.selected)`. `txTools.append(newBtn, editBtn, delBtn, sendBtn, perStartBtn, perStopBtn, exportBtn, importBtn, txMsg, persistEl)`.
- `this.periodicHost = el('div', 'crx-periodic-host')`; `txMain.append(txTools, txHead, this.txBody, this.editor, this.io, this.periodicHost)`.
**`openEditor`**: add `this.edCycle.value = String(e ? e.cycleMs : 0)` next to the other field fills.
**`validate`**: in the `r.ok` branch, first `const c = parseCycle(this.edCycle.value)`; if `!c.ok` → `setText(this.edErr, c.error); this.edErr.classList.add('err'); this.edOk.disabled = true;` (still call `this.hooks.draftChanged(r.params)`); else the existing ok handling. The not-ok branch is unchanged.
**`commitEditor`**: `const c = parseCycle(this.edCycle.value); if (!c.ok) return;` before upsert; pass `c.ms` as the 5th argument.
**`renderTx`**: before the version early-return, with `sel`: `const se = sel === null ? undefined : this.tx.get(sel); const pj = sel === null ? null : this.hooks.periodicOf(sel); const running = pj !== null && pj.state !== 'stopped'`; `this.perStartBtn.disabled = !se || se.cycleMs <= 0 || !this.writable || this.latched || running`; `this.perStopBtn.disabled = !running`; `this.perStartBtn.title = this.latched ? 'STOP 중 — 새 주기 송신 불가' : '선택 항목을 데몬 주기 송신 작업으로 시작 (Cycle Time > 0)'`. In the row loop: `const pjr = this.hooks.periodicOf(e.uid); const on = pjr !== null && pjr.state !== 'stopped'`; cell 4 = `e.cycleMs > 0 ? \`${e.cycleMs} ms\` : '수동'`; cell 6 = `on ? \`주기 ${pjr.job} · ${jobStateText(pjr)}\` : e.cycleMs > 0 ? '주기 (정지)' : '수동'`; `row.dataset.job = on ? pjr.job : ''`.
**`periodicChanged()`**: `this.txVersion = -1`. **`setLatched(l)`**: `if (l !== this.latched) { this.latched = l; this.txVersion = -1; }`.

## B3-PANEL — `canTrace.ts` (`createCanTrace` body; job table only when `bound`)

1. State: `const jobs = new JobTable(); const myJobs = new Set<string>(); const entryJob = new Map<number, string>(); let jobsSeen = -1;`.
2. RxTxView hooks (add to the options object): `startPeriodic: (uid) => void startEntry(uid)`, `stopPeriodic: (uid) => { const j = entryJob.get(uid); if (j) void stopJob(j); }`, `periodicOf: (uid) => { const j = entryJob.get(uid); if (!j) return null; const r = jobs.get(j); return { job: j, state: r ? r.state : 'requested', reason: r ? r.reason : null }; }`.
3. `perView = bound ? new PeriodicView(jobs, { isMine: (j) => myJobs.has(j), stop: (j) => void stopJob(j), takeover: (j) => void takeoverJob(j), refresh: () => void refreshJobs() }) : null`; when created, `rxtx.periodicHost.append(perView.root)`.
4. `const say = (t: string, err: boolean) => perView?.setStatus(t, err)`; `const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))`.
5. `refreshJobs = async (): Promise<void>`: `if (!bound) return`; `say('목록 읽는 중…', false)`; `try { const v = await ctx.query('can.periodic.list', { channel }); if (jobs.applyList(v, channel)) say(\`목록 읽음 — 작업 ${jobs.list().filter((r) => r.state !== 'stopped').length}개\`, false); else say('목록 형식을 알 수 없음', true); } catch (e) { say(\`목록 읽기 실패: ${errText(e)}\`, true); } finally { ctx.scheduler.invalidate(); }`. Called once right after setup: `if (bound) void refreshJobs();`.
6. `startEntry = async (uid: number): Promise<void>`: `e = txList.get(uid)`; none → return. `ctx.stopLatched` → `say('STOP 중 — 주기 송신을 시작할 수 없음', true); return`. `const cur = entryJob.get(uid); if (cur && jobs.get(cur)?.state !== 'stopped') { say(\`이미 실행 중: ${cur}\`, true); return; }`. `sp = startParams(e, channel)`; `!sp.ok` → `say(sp.error, true); return`. `try { const hd = await ctx.act('periodicStart', sp.params); const job = (hd.result as { job?: unknown } | undefined)?.job; if (typeof job === 'string') { myJobs.add(job); entryJob.set(uid, job); say(\`주기 시작 ${job} · ${e.cycleMs} ms\`, false); } else say('시작됨 — 작업 id 없음 (목록 새로고침)', true); } catch (err) { say(\`✗ 주기 시작 실패: ${errText(err)}\`, true); } finally { rxtx.periodicChanged(); perView?.render(true); ctx.scheduler.invalidate(); }`.
7. `stopJob = async (job: string): Promise<void>`: `try { await ctx.act('periodicStop', { job }); jobs.applyEvent({ kind: 'can.job_state', channel, payload: { job, state: 'stopped' } }, channel); say(\`정지됨 ${job}\`, false); } catch (err) { say(\`✗ 정지 실패 (${job}): ${errText(err)}\`, true); } finally { rxtx.periodicChanged(); perView?.render(true); ctx.scheduler.invalidate(); }` (the success result `{job, state:'stopped'}` is the daemon's confirmation; the later `can.job_state` event repeats it harmlessly).
8. `takeoverJob = async (job: string): Promise<void>`: `try { const hd = await ctx.act('periodicTakeover', { job }); const nj = (hd.result as { job?: unknown } | undefined)?.job; if (typeof nj === 'string') { myJobs.add(nj); for (const [u, j] of entryJob) if (j === job) entryJob.set(u, nj); say(\`넘겨받음: ${job} → ${nj}\`, false); } await refreshJobs(); } catch (err) { if (err instanceof GateError && err.code === 'E_CANCELLED') say('넘겨받기 취소됨', false); else say(\`✗ 넘겨받기 실패: ${errText(err)}\`, true); } finally { perView?.render(true); ctx.scheduler.invalidate(); }`.
9. Control path: in the existing bound `ctx.onControl` callback add `if (jobs.applyEvent(ev, channel)) ctx.scheduler.invalidate();` (keep the channel_state handling).
10. `updateStatus()` (bound, after `writable` is known): `perView.setWritable(writable); perView.setLatched(ctx.stopLatched); rxtx.setLatched(ctx.stopLatched); if (jobs.version !== jobsSeen) { jobsSeen = jobs.version; rxtx.periodicChanged(); } perView.render();` — before the existing `rxtx.render()` call in that function.
11. No timers, no polling, no renew loop: starts use the engine default `max_duration_ms` (1 h). Editing a running entry's data does not patch the job (stop + start); documented.

---

## B3b-REVIEW (Opus) — ACCEPTED (B3 complete: CAN-2)

Worker hash `cat txList.ts rxtxView.ts canTrace.ts | shasum -a 256` = `1dbd68131b7cc1711d1e18a4933530fe175d84c01ef5258f81077a60783f3b6f` (verified). The one failure was Opus's old `canTrace.test.ts` F2 assertion (no calls at mount) — amended by Opus to allow only the explicit bound-mount state reads (`can.periodic.list`, `can.db.info`); trace query/replay/send stay forbidden. Accepted additions: `rxtx.render()` after `periodicChanged()` in `startEntry`/`stopJob` finally blocks and in `updateStatus` when the job version moved (rxtx view only) — deterministic redraw, no timers. vitest 222/222 (`--maxWorkers=2`); app/node tsc 0; eslint 0; no timers/innerHTML added.

## B5a-ALLOW — exclusive file lease (pure DB helpers, CAN-5)

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `dbView.ts` | `parseDbInfo`, `dbChip`, `dbLoadParams`, `DbNames.{reset, addPage, nameOf}`, `decodedLines` |

Spec (read-only): `dbView.test.ts`. Acceptance (in `ui/`): `npx vitest run src/panels/can --maxWorkers=2` all pass; isolated app/node tsc 0; eslint 0.
Scaffold hash: `cat ui/src/panels/can/dbView.ts ui/src/panels/can/dbView.test.ts | shasum -a 256` (STATUS.md). Return `B5a-WORKER.md`, result hash `shasum -a 256 ui/src/panels/can/dbView.ts`.

## B5-DB — `dbView.ts`

`count(v)` = non-negative integer number (function-local). `basename(p)` = last non-empty segment of `p.split(/[\\/]/)`, else `'?'` (function-local).

**`parseDbInfo(v, channel)`**: `v` non-null object with array `channels`; first object entry with `channel === channel`, none → `null`. `loaded === false` → `{ loaded: false, path: null, sha: null, messages: null, encoding: null, encodingGuessed: false, warningsTotal: 0, blockedTotal: 0 }`. `loaded === true` → `{ loaded: true, path: string|null, sha: db_sha256 string|null, messages: count|null, encoding: string|null, encodingGuessed: encoding_guessed === true, warningsTotal: count(warnings_total) ? … : 0, blockedTotal: count(blocked_total) ? … : 0 }`. `loaded` not a boolean → `null`.

**`dbChip(info)`**:
- `null` → `{ text: 'DB: ?', title: 'DB 상태를 읽지 못함 — 눌러서 다시 읽기·불러오기', cls: 'ct-db-unknown' }`.
- `!loaded` → `{ text: 'DB: 없음', title: '이 채널에 DBC 없음 — 눌러서 데몬 컴퓨터 경로의 DBC 불러오기', cls: 'ct-db-none' }`.
- loaded → `text = \`DB: ${path ? basename(path) : '?'} · ${sha ? sha.slice(0, 8) : '?'}\``; `title = \`${path ?? '?'}\nsha256 ${sha ?? '?'} · 메시지 ${messages ?? '?'}개 · 인코딩 ${encoding ?? '?'}${encodingGuessed ? ' (추정)' : ''}\`` + (`warningsTotal > 0` → `\`\n경고 ${warningsTotal}\``) + (`blockedTotal > 0` → `\`\n디코드/인코드 막힘 ${blockedTotal}개\``); `cls = warningsTotal > 0 || blockedTotal > 0 || encodingGuessed ? 'ct-db-warn' : 'ct-db-ok'`.

**`dbLoadParams(path, encoding, channel)`**: `t = path.trim()`; `''` → `'데몬 컴퓨터의 DBC 절대 경로를 입력하세요'`; `t.length > DB_PATH_MAX` → `'경로가 너무 깁니다 (≤ 4096자)'`; not (`startsWith('/')` or `/^[A-Za-z]:[\\/]/`) → `'경로는 데몬 컴퓨터의 절대 경로여야 합니다'`; else `{ ok: true, params: { channel, path: t, ...(encoding !== 'auto' ? { encoding } : {}) } }` (key order channel, path, encoding).

**`DbNames.reset(sha)`**: `this.sha = sha; names.clear(); pages = 0; complete = false; cut = false; version++`.
**`DbNames.addPage(v)`**: `v` non-null object, `v.db_sha256 === this.sha` (string) and `messages` array — else `return null` with no change. For each object message with `name` string, `id_raw` count, `ext` boolean (others skipped): if `names.size >= DB_NAMES_MAX` → `cut = true`, stop; else `names.set(\`${ext ? 'x' : 's'}${id_raw}\`, name)`. Then `pages++; version++`. If `cut` → `null`. `next = count(v.next_cursor) ? v.next_cursor : null`; `next === null` → `complete = true; return null`; `pages >= DB_PAGES_MAX` → `cut = true; return null`; else `return next`.
**`DbNames.nameOf(id, ext)`** → `names.get(key) ?? null`.

**`decodedLines(d)`**:
- `d === undefined || d === null` → `['DB에 없는 id — 디코드할 메시지 없음']`.
- not an object → `['디코드 결과 형식을 알 수 없음']`.
- `typeof d.error === 'string'` → `[\`디코드 실패${typeof d.message === 'string' ? \` (${d.message})\` : ''}: ${d.error}\`]`.
- `typeof d.message === 'string' && Array.isArray(d.signals)`: head = `d.message` + (`length_mismatch` object with numbers → `\` · 길이 불일치 (DBC ${expected} B, 프레임 ${actual} B)\``) + (`fd_mismatch === true` → `' · FD 불일치'`). Then per signal object with `name` string (others skipped): `unavailable === 'short_frame'` → `\`  ${name}: 값 없음 (짧은 프레임 — 필요 ${needed} B, 실제 ${actual} B)\``; other string `unavailable` → `\`  ${name}: 값 없음 (${unavailable})\``; else `\`  ${name} = ${String(value)}${unit ? \` ${unit}\` : ''}${typeof label === 'string' ? \` (${label})\` : ''} · raw ${String(raw)}\`` (`unit` = string or '').
- anything else → `['디코드 결과 형식을 알 수 없음']`.

## B5a-REVIEW (Opus) — ACCEPTED

Worker hash `dbView.ts` = `e7674e1cf75e6d169a0e2be5cb596813a1214caf1836e1a3fb850a52e62c99cb` (verified); dbView 9/9; CAN suite 231/231 excluding the B5b spec; eslint/node tsc 0 (the one app tsc error is Opus's B5b scaffold hook). Bodies match §B5-DB.

---

## B5b-ALLOW — exclusive file lease (DB wiring, CAN-5)

| File | Bodies Sonnet writes (nothing else) |
|---|---|
| `rxtxView.ts` | `renderRx` (fill cell 7), `namesChanged` |
| `canTrace.ts` | `createCanTrace` body only (§B5-PANEL) |

Opus scaffold done: `RX_COLS` + 8th column `'메시지 (DB)'` (last; earlier indexes unchanged) and its CSS grid, hook `msgName(id, ext)`, stub `namesChanged`, imports from `./dbView`, manifest queries `can.db.info`, `can.db.catalog`, `can.db.load`, CSS (`ct-db*`, `ct-dbbox`, `ct-detail-decoded`). Spec (read-only): `canB5Panel.test.ts`.
Acceptance (in `ui/`): `npx vitest run src/panels/can --maxWorkers=2` all pass; isolated app/node tsc 0; eslint 0.
Scaffold hash: `cat ui/src/panels/can/rxtxView.ts ui/src/panels/can/canTrace.ts ui/src/panels/can/canB5Panel.test.ts ui/src/panels/can/canTrace.test.ts | shasum -a 256` (STATUS.md). Return `B5b-WORKER.md`, result hash `cat rxtxView.ts canTrace.ts | shasum -a 256`.

## B5-PANEL

**rxtxView.ts** — `renderRx`: in the per-row fill, `setText(c[7]!, this.hooks.msgName(r.id, r.ext) ?? '')`. `namesChanged()`: `this.rxVersion = -1`.

**canTrace.ts** (bound only, unless stated):
1. State: `let dbInfo: DbInfo | null = null; const names = new DbNames(); let dbReading = false; let dbAgain = false; let decoding = false;`.
2. DOM:
   - `dbChipBtn = el('button', 'ct-db ct-db-unknown', 'DB: ?')`, `type = 'button'`, testid `can-db`; in `top.append(...)` right after `envBadge`: `...(bound ? [dbChipBtn] : [])`.
   - `dbBox = el('div', 'ct-dbbox')` testid `can-db-box`, `role="group"`, `aria-label="DBC"`, `hidden = true`; children: `el('span', undefined, 'DBC 경로 (데몬 컴퓨터)')`; `dbPath = el('input', 'ct-db-path')` testid `can-db-path`, `placeholder = '/abs/path/body.dbc'`, `aria-label = 'DBC 경로 (데몬 컴퓨터 절대 경로)'`; `dbEnc = el('select')` testid `can-db-encoding` with options for `DB_ENCODINGS` (labels `auto` → `'인코딩 자동'`, `utf8` → `'UTF-8'`, `windows1252` → `'Windows-1252'`); `dbLoadBtn` `'불러오기'` testid `can-db-load`; `dbCloseBtn` `'닫기'` testid `can-db-close`; `dbMsg = el('span', 'ct-db-msg')` testid `can-db-msg`, `aria-live="polite"`. In `root.append`: `...(bound ? [closeBox, exportBox, dbBox] : [])`.
   - Row detail (also unbound, always disabled there): `detailDecode = el('button', undefined, '디코드')`, `type = 'button'`, testid `can-row-detail-decode`, `title = '이 행을 채널 DBC로 디코드 (저널 1행 질의)'`, inserted first in `detailTools`; `detailDecoded = el('div', 'ct-detail-decoded')` testid `can-row-detail-decoded`, appended to `detailBox` after `detailTools`.
3. `const syncDecode = (): void => { detailDecode.disabled = !(bound && dbInfo?.loaded === true && detail !== null && detail.kind === 'frame' && detail.seq > 0) || decoding; }`. Call it at the end of `openDetail` (which also does `detailDecoded.replaceChildren()`), in `closeDetail`, and in `updateStatus`.
4. `applyDbInfo(i: DbInfo | null)`: `dbInfo = i`; `if (i === null) return` (unknown keeps the current names); `const sha = i.loaded ? i.sha : null`; `if (sha !== names.sha) { names.reset(sha); rxtx.namesChanged(); if (sha) void readNames(sha); }`.
5. `readDbInfo = async (): Promise<void>`: `if (!bound) return; if (dbReading) { dbAgain = true; return; } dbReading = true;` `try { const v = await ctx.query('can.db.info', { channel }); applyDbInfo(parseDbInfo(v, channel)); } catch { applyDbInfo(null); } finally { dbReading = false; if (dbAgain) { dbAgain = false; void readDbInfo(); } ctx.scheduler.invalidate(); }`. Called once at setup: `if (bound) void readDbInfo();`.
6. `readNames = async (sha: string): Promise<void>`: `let cursor: number | null = 0; try { while (cursor !== null) { const v = await ctx.query('can.db.catalog', { channel, cursor, max: DB_PAGE_SIZE }); if (names.sha !== sha) return; cursor = names.addPage(v); } } catch (e) { setText(dbMsg, \`메시지 이름 읽기 실패: ${e instanceof Error ? e.message : String(e)}\`); dbMsg.classList.add('err'); } finally { rxtx.namesChanged(); ctx.scheduler.invalidate(); }` (bounded by `DbNames` page/name caps; no retries).
7. `loadDb = async (): Promise<void>`: `if (!(ctx.rights.has('operate') || ctx.rights.has('approve'))) { setText(dbMsg, 'DBC 불러오기는 operate 권한 필요'); dbMsg.classList.add('err'); return; }` `const lp = dbLoadParams(dbPath.value, dbEnc.value as DbEncoding, channel); if (!lp.ok) { setText(dbMsg, lp.error); dbMsg.classList.add('err'); return; }` `dbLoadBtn.disabled = true; setText(dbMsg, '불러오는 중…'); dbMsg.classList.remove('err');` `try { const r = await ctx.query('can.db.load', lp.params); const i = parseDbInfo({ channels: [r] }, channel); applyDbInfo(i); const base = i?.path?.split(/[\\/]/).filter(Boolean).pop() ?? '?'; setText(dbMsg, \`불러옴: ${base} · 메시지 ${i?.messages ?? '?'}개${(r as { changed?: unknown } | null)?.changed === false ? ' (같은 파일)' : ''}\`); } catch (e) { setText(dbMsg, \`✗ 불러오기 실패: ${e instanceof Error ? e.message : String(e)}\`); dbMsg.classList.add('err'); } finally { dbLoadBtn.disabled = false; ctx.scheduler.invalidate(); }`.
8. `decodeDetail = async (): Promise<void>`: `const f = detail; if (!bound || f === null || decoding) return; decoding = true; syncDecode();` `let lines: string[];` `try { const r = await ctx.query('can.trace.query', { channel, since: \`seq:${f.seq}\`, until: \`seq:${f.seq}\`, max: 1, decode: true }); const rows = (r as { rows?: unknown } | null)?.rows; const row = Array.isArray(rows) ? (rows as Record<string, unknown>[]).find((x) => typeof x === 'object' && x !== null && String(x.seq) === String(f.seq)) : undefined; lines = row ? decodedLines(row.decoded) : ['저널에서 이 행을 찾지 못함 (보관 범위 밖일 수 있음)']; } catch (e) { lines = [\`✗ 디코드 실패: ${e instanceof Error ? e.message : String(e)}\`]; } finally { decoding = false; }` `if (detail === f) detailDecoded.replaceChildren(...lines.map((l) => el('div', undefined, l)));` `syncDecode(); ctx.scheduler.invalidate();`.
9. Wiring: `dbChipBtn` click → `dbBox.hidden = false; void readDbInfo();`. `dbLoadBtn` → `void loadDb()`. `dbCloseBtn` → `dbBox.hidden = true`. `dbBox` keydown: `Escape` → `e.preventDefault(); dbBox.hidden = true`; always `e.stopPropagation()`. `detailDecode` click → `void decodeDetail()`. RxTxView hook `msgName: (id, ext) => names.nameOf(id, ext)`.
10. `consumeCan`: inside the event loop, `if (bound && ev.kind === 'can.db_changed' && ev.channel === channel) dbChanged = true;` (a local `let dbChanged = false` before the loop); after the loop `if (dbChanged) void readDbInfo();` (coalesced by `dbReading`/`dbAgain`).
11. `updateStatus` (bound): `const c = dbChip(dbInfo); setText(dbChipBtn, c.text); dbChipBtn.title = c.title; const cls = \`ct-db ${c.cls}\`; if (dbChipBtn.className !== cls) dbChipBtn.className = cls;` plus `syncDecode()` (all panels).
12. No timers/polling; DBC/catalog/decoded text → textContent only.


## B5b-REVIEW (Opus) — ACCEPTED (B5 complete: CAN-5)

Worker hash `cat rxtxView.ts canTrace.ts | shasum -a 256` = `62e5820093bf6b37fa6dee8655888c2bb807041ffd5bfaf691d797f68ee0610d` (verified). Specs untouched (canTrace.test.ts diff = Opus's F2 amendment only). vitest CAN 239/239 (`--maxWorkers=2`); app/node tsc 0; eslint 0; no timers/innerHTML added. Final module validation in STATUS.md.

---

## RT01/RT02 — ROOT grant 2026-11-16 (CAN-RT-01 core gate, CAN-RT-02 export selection)

Baseline `crates/loom-gateway/src/daemon.rs` (ROOT copy) sha256 `a7cc4af6a1d605c9227b1e410772d349e154bca70276c7dae16e2e27f695c5df`.
Specs (Opus, read-only): `crates/loom-gateway/tests/can_job_controls.rs` (G1–G4 + H1; all 5 fail first with E_NOT_IMPLEMENTED), `crates/loom-can/tests/export.rs` (`selected` asserts + `default_lookback_is_echoed_as_such`), `crates/loom-can/tests/cli.rs` (`export_render_golden` selected lines), `ui/src/panels/can/exportReq.test.ts`, `canB4Panel.test.ts`.

### RT01-ALLOW / RT01-GATE — `daemon.rs`, `Daemon::call` write_class match ONLY
Replace the two arms
`Some(WriteClass::Request) => self.write_request(c, domain, &spec, args, false).await,` and `Some(_) => self.envelope(… NotImplemented, "job/interactive write classes arrive in M2" …)` with:
```
// CAN-RT-01 (ROOT): Job = same gated path as Request (STOP / recording / audit / policy); Interactive unsupported
Some(WriteClass::Request | WriteClass::Job) => self.write_request(c, domain, &spec, args, false).await,
Some(WriteClass::Interactive) => self.envelope(c, m, Err(err_body(ErrorCode::NotImplemented, "interactive write class arrives in M2", None)), None),
```
Nothing else in daemon.rs (`write_inner` keeps `write_class: WriteClass::Request` in `WriteRequest`; no CAN hook reads it — documented limitation).

### RT02 — export selection echo
- `export.rs run_export`: before the scan loop, `let since_source = if p.since.is_some() { "explicit" } else { "default_lookback" }; let selected = json!({"since": from.to_string(), "until": p.until.as_ref().map(ToString::to_string), "since_source": since_source});` (from = the initial cursor, before it advances). Add `"selected": selected` to the result object (every success, incl. 0 frames).
- `cli.rs render_export`: right after line 3 (`seq …`), and right after `no frames in the selection — no file written` in the no-file branch: `selected: since {since}` + (` (default look-back — not whole history)` when since_source == "default_lookback") + ` · until {until or "now"}`. `selected` not an object / `since` not a string → `selected: unknown`.
- `exportReq.ts exportResultLines`: after the counts line (file case) and after the no-file line: `` `선택 범위: ${since} → ${until ?? '지금'}` `` + (`' (기본 최근 10분 — 전체 기록 아님)'` when default_lookback); missing/invalid → `'선택 범위: 알 수 없음'`. Then truncated/warning lines as before.
Acceptance: `cargo test -p loom-gateway --test can_job_controls` G1–G4 pass (H1 see below); `cargo test -p loom-can --no-fail-fast`; clippy -D warnings (loom-can, loom-gateway --tests); fmt check on touched files only (never reformat other gateway files); `npx vitest run src/panels/can --maxWorkers=2`; isolated tsc; eslint. CARGO_BUILD_JOBS=2.

### RT01-H1 (pending ROOT authorization — NOT in this lease)
Code reading: worker ownership compares the full client id `name#pid` (`periodic.rs periodic_authorize`, `all_mine` filter), so H1 (`a2` renew of `a1`'s job) is expected to FAIL after the gate fix. Proposed least CAN-owned fix: compare the stable prefix before `#` (same key the core uses for `@last`/others cursors; `daemon::client_key`) in `periodic_authorize` and the `all_mine` filter — a function-local `split_once('#')` helper, origin equality kept, no user/secret bypass. Awaits ROOT go.
