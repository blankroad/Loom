# UART operator upgrade — FUNCTIONS (function contracts)

Owner: Opus 5.5 lead (signatures, types, constants, tests, review). Sonnet: product function bodies **only** for the batch it holds a lease on.
Worktree: `/Users/ctmctm/workspace/Loom/.worktrees/operator-uart` (paths below are relative to it). Baseline `610029e`.
Plan: `/Users/ctmctm/workspace/Loom/work/reports/operator-upgrade/uart/PLAN.md`. Common rules: `../COMMON.md` v1.

Status of this file: **v10 — B1–B8 final (READY_TO_INTEGRATE, imported by ROOT). Correction batch B9 (ROOT-accepted F1/F2) is frozen and ready (PLAN_READY_BATCH9).** Base = ROOT's imported UART files (mechanical rustfmt applied).

---

## 0. Rules for every batch

- Edit only the files in the batch's **lease** table, and inside them only the functions/blocks in the **body allowlist**.
  Exported types, signatures, constants, test files, package manifests, method schemas, risk policy and `data-act`/`data-testid`
  names stay exactly as written. If a contract is missing, ambiguous or a new helper seems necessary: stop and report the exact
  question to the lead (do not invent it).
- Device text is untrusted: set it with `textContent` / attributes, never `innerHTML`. `innerHTML` is allowed only for the
  trusted constant `icon(...)` markup (as today).
- No new dependencies, no `node_modules` changes, no Git, no GUI, no device IO.
- Do not weaken or edit tests. Report failing output verbatim.
- **Static lines are lead-only, with no exceptions** (imports, types, constants, signatures, manifests, CSS, tests), errata included. If a body
  needs one, stop editing that file, return its lease with the exact need, and wait for RESUME with the new contract hash.
- Run the batch's acceptance commands and paste the exact tail of each output in the return.
- **Test resources (ROOT, 2026-11-16, COMMON appendix):** UI tests are run *targeted* with `npx vitest run --maxWorkers=2 <module
  paths + named consumers>`; the **full UI suite is ROOT-only** at the final integrated freeze — do not run it per batch (supersedes
  the "full suite once" line of earlier batches). Rust stays `CARGO_BUILD_JOBS=2`. Never weaken assertions or timeouts, never kill
  other sessions' tests. Already-passed checks need no re-run just to add the flag.

---

## Batch 1 — U5a scrollback search + U7 narrow-pane identity (UI only)

### Lease (Sonnet holds every file below for the batch; lead edits none of them meanwhile)

| file | what Sonnet may change |
|---|---|
| `ui/src/panels/uart/scrollSearch.ts` | bodies of the 6 exported functions (§B1.1). Nothing else. |
| `ui/src/panels/uart/paneLayout.ts` | bodies of the 7 exported functions (§B1.2). Nothing else. |
| `ui/src/panels/uart/terminalPanel.ts` | the function bodies / render blocks named in §B1.3 and §B1.4 only. |
| `ui/src/workbench/uart/UartWorkbench.tsx` | the two session-row render blocks in `SessionsSide` (§B1.5). |

Lead-owned and **already written** (read-only for Sonnet): imports, state declarations and the `toolEls` map in
`terminalPanel.ts` (see §B1.3/§B1.4 "already scaffolded"), the `paneLayout` import in `UartWorkbench.tsx`, the `"contrast"` icon
in `icons.ts`, every CSS rule of §B1.6 in `bench.css` / `uart.css`, and the tests `scrollSearch.test.ts`, `paneLayout.test.ts`,
`batch1.panel.test.ts`. Static files (CSS, icon table) and types/signatures/constants are not Sonnet's to change.

### Acceptance (run from `ui/`)

```
mkdir -p ../work/typecheck
npx vitest run src/panels/uart src/workbench/uart
npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/panels/uart src/workbench/uart src/sdk/terminalView.ts
```
(COMMON "Typecheck cache isolation": never `tsc -b` — its incremental cache is shared through the symlinked `node_modules`.
Report each command's exit status. `work/typecheck/` is untracked, not ignored, in this repo: leave the files there, do not add them to Git.)
Baseline (lead, 2026-11-15, before scaffolding): 6 pre-existing files / 60 tests pass, `eslint` clean; the `tsc -b` pass then used the shared cache and is superseded by the isolated check (app/node exit 0 on 2026-11-15, taken on the in-progress B1 tree).
Scaffold state handed to Sonnet (expected, not regressions to fix outside the allowlist): `tsc` reports only the old `runSearch`
body using `{x,y}` hits (§B1.3.4 replaces it); `eslint` reports only unused scaffold imports/`headMode`/`toolEls` that the
allowlisted bodies consume; pre-existing panel tests that draw now reach `paneLayout.connClass` (throws until §B1.2 is done —
implement §B1.1/§B1.2 first). Expected after batch 1: all tests in the 9 files pass, `tsc` and `eslint` clean.

---

### §B1.1 `ui/src/panels/uart/scrollSearch.ts` (pure, no DOM)

Types/constants already written: `CellLike`, `LineLike`, `BufferLike` (subsets of xterm 6 `IBufferCell/IBufferLine/IBuffer`),
`SearchOptions`, `DEFAULT_SEARCH_OPTIONS = {caseSensitive:false, regex:false}`, `CompiledQuery`, `MAX_QUERY_CHARS = 256`,
`MAX_LOGICAL_ROWS = 256`, `MAX_HITS = 10_000`, `LogicalLine`, `SearchHit`, `SearchResult`.

**`compileQuery(q: string, o: SearchOptions): CompiledQuery`**
- `q === ''` → `{kind:'empty'}`.
- `q.length > MAX_QUERY_CHARS` → `{kind:'error', message:`검색어가 너무 깁니다 (최대 ${MAX_QUERY_CHARS}자)`}`.
- source = `o.regex ? q : q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')`; flags = `'g' + (o.caseSensitive ? '' : 'i')` (no `u`, no `m`).
- `new RegExp(source, flags)`; on throw → `{kind:'error', message: `정규식 오류: ${err.message}`}`. Never throws.
- Examples: `('', *)`→empty; `('a.b(1)', literal)` matches `"xa.b(1)y"` at 1, not `"axb1"`; `('(', regex)`→error `정규식 오류: …`;
  `('(', literal)`→ok.

**`logicalLines(buf: BufferLike): Iterable<LogicalLine>`** (lazy; body may `return (function* () { … })()`)
- Walk `y = 0 … buf.length-1`. A row starts a new logical line when `y === 0`, or the row is not `isWrapped`, or the current
  chain already has `MAX_LOGICAL_ROWS` rows. Following rows with `isWrapped === true` join the chain.
- `getLine(y) === undefined` → that row is an empty, unwrapped row (`''`).
- Row text: `translateToString(false)` when the next row continues this chain (next row exists, `isWrapped`, chain not full);
  otherwise `translateToString(true)` (end of the logical line is right-trimmed). A chain split by `MAX_LOGICAL_ROWS` keeps the
  last row untrimmed (`false`), because physically it continues.
- `rowStarts[i]` = sum of lengths of the previous rows' texts; `text` = concatenation.
- Example (cols 6): rows `'hello'`, `{w:'ab cd '}`, `{w:'ef'}`, `'next  '` → `{startRow:0,rowStarts:[0,6,12],text:'hello ab cd ef'}`,
  `{startRow:3,rowStarts:[0],text:'next'}`.

**`cellColumn(line: LineLike, index: number): number`**
- Walk cells `x = 0 … line.length-1`, `acc = 0`. For each cell: `w = cell?.getWidth() ?? 1`; skip when `w === 0`;
  `chars = cell?.getChars() || ' '`; if `index < acc + chars.length` return `x`; else `acc += chars.length`.
- After the loop return `line.length`.
- Example: `'가나ab'` (cols 6, wide chars 2 cells) → index 0,1,2,3 → 0,2,4,5.

**`searchBuffer(buf: BufferLike, cols: number, q: CompiledQuery): SearchResult`**
- `q.kind !== 'ok'` → `{hits:[], truncated:false, error: q.kind==='error' ? q.message : null, rows: buf.length}`.
- For each logical line (in order): `re.lastIndex = 0`; loop `m = re.exec(text)`:
  - `m === null` → next line.
  - `m[0].length === 0` → `re.lastIndex = m.index + 1`; if `re.lastIndex > text.length` → next line; continue (zero-length
    matches are never hits; no infinite loop).
  - start offset `s = m.index`, last offset `e = m.index + m[0].length - 1`.
  - `i` = last index with `rowStarts[i] <= s`; `j` = last index with `rowStarts[j] <= e`.
  - `row = startRow + i`; `col = cellColumn(lineAt(row), s - rowStarts[i])`.
  - `endRow = startRow + j`; `endCol = cellColumn(lineAt(endRow), e - rowStarts[j])`; `endW = max(1, cell(endCol).getWidth())`.
  - `cells = (endRow*cols + endCol + endW) - (row*cols + col)`; push `{row, col, cells}`.
  - when `hits.length === MAX_HITS` → return immediately with `truncated: true`.
  - `lineAt(y)` = `buf.getLine(y)`; if undefined (cannot happen for a hit) skip the hit.
- Return `{hits, truncated:false, error:null, rows: buf.length}`.
- Examples: cols 5 `'xxxPA'`,`{w:'NIC'}` + `panic` → `[{row:0,col:3,cells:5}]`; `x*` on `'axxb'` → `[{row:0,col:1,cells:2}]`;
  `'가나ab'` + `나a` → `[{row:0,col:2,cells:3}]`.
- Bounds: query ≤ 256 chars, logical line ≤ `MAX_LOGICAL_ROWS` rows (≤ 256 × cols chars), ≤ `MAX_HITS` hits, scrollback ≤ 50 000 rows
  (TerminalView cap). Known limitation, **visible in the UI** (§B1.3 hint) and not hidden by any latency claim: a user regex with
  catastrophic backtracking can stall the UI thread (JS regex cannot be interrupted); matches spanning a `MAX_LOGICAL_ROWS` split are missed.

**`stepHit(current: number, count: number, dir: 1 | -1): number`**
- `count <= 0` → `-1`. `current < 0` → `dir > 0 ? 0 : count-1`. Else `(current + dir + count) % count`.

**`searchCountText(q: CompiledQuery, r: SearchResult, at: number): string`**
- empty → `''`; error → `q.message`; `r.hits.length === 0` → `'0건'`; `at < 0` → `${n}건` (+ `'+'` when truncated);
  else `${at+1}/${n}` + (`'+'` when truncated). Example: `(ok, 3 hits, 1)` → `'2/3'`; `(ok, MAX_HITS truncated, 0)` → `'1/10000+'`.

Tests: `ui/src/panels/uart/scrollSearch.test.ts` (19).

---

### §B1.2 `ui/src/panels/uart/paneLayout.ts` (pure, no DOM)

Types/constants already written: `ConnClass`, `HeaderMode`, `ToolId`, `TOOL_ORDER`, `HEADER_FULL_MIN_PX = 900`,
`HEADER_COMPACT_MIN_PX = 520`, `ToolPlacement`, `IdentityPlan`, `IdentityInput`, `ChannelState`, `SessionDot`.

**`connClass(status: string): ConnClass`** — exactly today's `terminalPanel.ts` mapping (that const is deleted in §B1.4):
`isOpenStatus(status)` (import from `./uartState`) → `'connected'`; `/disconn|lost/i` → `'lost'`; `/reconn|wait/i` → `'waiting'`;
`status === '?'` → `'unknown'`; else `'disconnected'`.

**`headerMode(widthPx: number): HeaderMode | null`** — `!Number.isFinite(widthPx) || widthPx <= 0` → `null` (caller keeps its mode;
a hidden dock tab reports 0). `>= HEADER_FULL_MIN_PX` → `'full'`; `>= HEADER_COMPACT_MIN_PX` → `'compact'`; else `'narrow'`.

**`toolPlacement(mode: HeaderMode): ToolPlacement`** — header set per mode, overflow = the rest, both filtered from `TOOL_ORDER`
(so both keep toolbar order; always fresh arrays):
- full: header = all.
- compact: header = `view, pause, rec, search, send, plug, more`.
- narrow: header = `rec, search, plug, more`.
Invariant (tested): partition of TOOL_ORDER; `rec`, `plug`, `more` always in header.

**`identityPlan(mode: HeaderMode): IdentityPlan`** — full `{path:true, frame:'full', counts:true}`; compact and narrow
`{path:false, frame:'baud', counts:false}`. Alias, dot and state are never hidden by mode.

**`identityTitle(i: IdentityInput): string`** — `[channel, stateLabel, path ?? '경로 없음', frame || '프레임 미확인', `RX ${rx}`, `TX ${tx}`].join(' · ')`.

**`openTagText(state: ChannelState, paneNo: number | null): string | null`** — `'open'` → `paneNo ? `pane ${paneNo}` : '열림'`; else `null`.

**`sessionDot(state: ChannelState): SessionDot`** — open → `'on'`; unknown → `'unknown'`; closed/none → `'off'`.

Tests: `ui/src/panels/uart/paneLayout.test.ts` (18).

---

### §B1.3 `terminalPanel.ts` — search wiring (U5a)

Reuse: the existing search bar DOM (`search`, `sq`, `scount`, `sclose`), `setSearch`, `ibtn/pill` helpers, xterm
`term.term.buffer.active` (satisfies `BufferLike`), `term.term.cols`, `term.term.select`, `term.term.scrollToLine`, `term.term.rows`.
Imports: already added by the lead (`./scrollSearch`).

Already scaffolded by the lead (do not change): the import line from `./scrollSearch`; `let hits: SearchHit[]`;
`const searchOpts: SearchOptions = { ...DEFAULT_SEARCH_OPTIONS }` (mutated in place, never reassigned); `let hitAt` unchanged.

Allowlisted blocks:
2. **Search bar construction** (block that builds `search`): create two toggle buttons with `pill(...)`:
   - `caseBtn`: label `Aa`, `data-act="search-case"`, `data-testid="uart-search-case"`, title `대소문자 구분 (켜면 정확히 일치)`, `aria-pressed` = `String(searchOpts.caseSensitive)`.
   - `regexBtn`: label `.*`, `data-act="search-regex"`, `data-testid="uart-search-regex"`, title `정규식 (JavaScript 문법) — 복잡한 패턴은 화면을 잠시 멈출 수 있음`, `aria-pressed` = `String(searchOpts.regex)`.
   - `searchHint = el('span', 'search-hint', '정규식: 복잡한 패턴은 화면을 잠시 멈출 수 있습니다 · 최대 10,000건 · 스크롤백 범위만')`,
     `data-role="search-hint"`, `hidden = !searchOpts.regex`.
   - click → `setSearchOption('caseSensitive' | 'regex')`.
   - `search.append(sq, caseBtn, regexBtn, searchHint, scount, sclose)` (order fixed).
   - `sq.title` = `xterm 스크롤백 안에서 검색 · 줄바꿈된 긴 줄은 이어서 검색 · 더 오래된 기록은 데몬 저널` (static text).
3. **`function setSearchOption(key: keyof SearchOptions): void`** (new closure): `searchOpts[key] = !searchOpts[key]`; update both
   buttons' `aria-pressed`; `searchHint.hidden = !searchOpts.regex`; `hitAt = -1; runSearch(1)`; `sq.focus()`.
4. **`runSearch(dir: 1 | -1)`** (replace body):
   ```
   const cq = compileQuery(sq.value, searchOpts)
   const res = searchBuffer(term.term.buffer.active, term.term.cols, cq)
   hits = res.hits
   scount.dataset.state = cq.kind === 'error' ? 'error' : (delete)
   if (!hits.length) { hitAt = -1; scount.textContent = searchCountText(cq, res, -1); term.term.clearSelection(); return }
   hitAt = stepHit(hitAt, hits.length, dir)
   const h = hits[hitAt]!
   term.term.select(h.col, h.row, h.cells)
   term.term.scrollToLine(Math.max(0, h.row - Math.floor(term.term.rows / 2)))
   scount.textContent = searchCountText(cq, res, hitAt)
   ```
   Event wiring unchanged: `input` → `hitAt = -1; runSearch(1)`; Enter → `runSearch(+1)`, Shift+Enter → `runSearch(-1)`; Escape closes.
   Searching never sends device bytes and never changes the terminal content.

Observable contract (tested in `batch1.panel.test.ts`): wrapped-row match `1/1`; `err` over `error 1/err 2/ERR 3` → `1/3`, Enter `2/3`,
Shift+Enter `1/3`; Aa → `1/2`; hint hidden until `.*`, then visible with `멈출 수` and `10,000`; `.*` + `^err \d$` → `1/1`; `(` → text starts `정규식 오류`, `data-state="error"`; no `uart.input` calls.

---

### §B1.4 `terminalPanel.ts` — narrow-pane identity (U7)

Already scaffolded by the lead (do not change): the `./paneLayout` import; the local `connClass` const is deleted (the imported
one has the same mapping); `let headMode: HeaderMode = 'full'` (next to `hits`); `const toolEls: Record<ToolId, HTMLElement>`
right after `head.append(target, chipEl, tools)`.

Allowlisted blocks:
1. **Header construction** (after the `toolEls` map): `head.dataset.mode = 'full'`; `aliasEl.title = channel`.
2. **Lease row construction**: `const overflow = el('span', 'uart-overflow')`, `role="group"`, `aria-label="숨긴 도구"`, `hidden = true`;
   `const overflowLbl = el('span', 'uart-lease-l', '도구')`; `lease.prepend(overflow)` (before `leaseLbl`).
3. **`function applyHeaderMode(): void`** (new closure):
   ```
   const m = headerMode(head.clientWidth); if (m === null || m === headMode) return
   headMode = m; head.dataset.mode = m
   const p = toolPlacement(m)
   tools.replaceChildren(...p.header.map((id) => toolEls[id]))      // nodes are moved, listeners kept
   overflow.replaceChildren(overflowLbl, ...p.overflow.map((id) => toolEls[id])); overflow.hidden = p.overflow.length === 0
   moreBtn.title = p.overflow.length ? '더보기: 숨긴 도구 · 입력권 · 특수 키' : '더보기: 입력권·특수 키, 로그 저장'
   ctx.scheduler.invalidate()
   ```
   Callers: `mount()` right after `host.append(root)`; `resize()` before `fitSoon()`; a `ResizeObserver` on `head`
   (`const headObserver = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => applyHeaderMode())`,
   `observe(head)` in `mount`, `disconnect()` in `dispose`). No timers.
4. **`draw()` identity part** (replace the lines that set `pathEl`, `frameEl`, `stateEl`, `countEl` text; keep all other draw code):
   - `const cc = connClass(port.status)`; `dot.dataset.conn = cc` (always, also before the port is known → `'unknown'`).
   - `const ip = identityPlan(headMode)`; `pathEl.hidden = !ip.path`; `countEl.hidden = !ip.counts`.
   - full frame text = today's expression (`[baud+frame, flow, eol].filter(Boolean).join(' · ')`) → `fullFrame`;
     `frameEl.textContent = ip.frame === 'full' ? fullFrame : port.baud ? String(port.baud) : ''`; `frameEl.title` unchanged.
   - `target.title = identityTitle({channel, stateLabel: CONN_LABEL[port.status] ?? port.status, path: port.path ?? sess?.port ?? null, frame: fullFrame, rx: fmtBytes(rxObserved), tx: fmtBytes(txObserved)})`.
   - state text/class and counters text unchanged.
5. **`applyTheme()`** body: `themeBtn.innerHTML = icon('contrast')` (both themes; the `contrast` icon is already in `icons.ts`); titles/aria unchanged.

Observable contract (tested): dot `data-conn` unknown → connected → lost; width 400 → `data-mode="narrow"`, header children
`[rec, search, disconnect, more]`, frame text `115200`, path/counters hidden, target title with all fields, overflow row contains
theme and HEX, HEX from the overflow still switches the view; 700 → compact; width 0 keeps compact; 1200 → full with 13 header
children, full frame text, overflow hidden; theme glyph ≠ gear glyph and contains `fill="currentColor"`.

---

### §B1.5 `UartWorkbench.tsx` — session list identity (U7)

Import already added by the lead. Allowlisted blocks: the saved-session row (`items.map((x) => …)`) and the ad-hoc row (`adhoc.map((c) => …)`) in `SessionsSide`.
- Dot: `<span className={`dot ${sessionDot(st)}`} />` (ad-hoc rows are always open → `dot on`). Add `<span className="tagsw" aria-hidden="true" />`
  before the dot (tag colour swatch, uses `--tag`).
- Name: `<span className="sess-name"><span className="sess-alias">{alias}</span>{tag && <span className="open-tag" title="연결됨">{tag}</span>}</span>`
  with `tag = openTagText(st, pane)` (ad-hoc: `openTagText('open', paneNoOfChannel(api, c.channel))`).
- Row `title`: unchanged text + `` ` · ${st === 'open' ? '연결됨' : st === 'unknown' ? '상태 확인 중' : '닫힘'}` ``.
No behaviour change to open/place actions.

### §B1.6 CSS (bench.css / uart.css) — **already written by the lead**; listed so the DOM classes above match

Rules present (scoped like neighbours with `:where(.mod-uart, .ub-layer, .uart-panel)`):
- `.ph-dot` background by connection: `[data-conn=connected]` `var(--ok)`; `[data-conn=lost]` `var(--err)`; `[data-conn=waiting]`
  `var(--warn-bd)`; `[data-conn=unknown]` transparent + `box-shadow: inset 0 0 0 1.5px var(--muted)`; `[data-conn=disconnected]`
  transparent + `box-shadow: inset 0 0 0 1.5px var(--faint)`. Tag colour moves to `.phead { box-shadow: inset 3px 0 0 var(--tag, transparent) }`.
- `.ph-alias { flex: 0 1 auto; min-width: 6ch; max-width: 18ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }`;
  `.ph-state { flex: none }`; `.ph-frame { flex: none }`.
- `.phead[data-mode=narrow] { gap: 6px; padding-left: 8px }`.
- The Aa/.* toggles are `pill` buttons, so the existing `.pill[aria-pressed="true"]` rule shows the pressed state (no new rule);
  add `.search .pill { flex: none; font: 600 11px var(--mono) }` and `.search .count[data-state=error] { color: var(--err) }`.
- `.uart-overflow { display: inline-flex; align-items: center; gap: 2px; flex-wrap: wrap; margin-right: 8px }`, `.uart-overflow[hidden] { display: none }`.
- Session list: `.sess .dot.on { background: var(--ok) }`, `.sess .dot.off { background: transparent; box-shadow: inset 0 0 0 1.5px var(--faint) }`,
  `.sess .dot.unknown { background: transparent; box-shadow: inset 0 0 0 1.5px var(--muted) }`;
  `.sess .tagsw { width: 3px; height: 14px; border-radius: 2px; background: var(--tag, var(--muted)) }`;
  `.sess-name .sess-alias { min-width: 0; overflow: hidden; text-overflow: ellipsis }`; `.sess-name .open-tag { flex: none; white-space: nowrap }`.
  `.sess` grid becomes `grid-template-columns: 3px 8px minmax(0, 1fr) auto` (swatch, dot, name, actions) and `.sess-meta` moves to
  `grid-column: 3 / 5` (was `2 / 4`). No other `.sess` rule changes.

---

## Batch 2 — U3 transmit pacing engine (Rust worker only) — PLAN_READY_BATCH2

Goal: a paced multi-line send/paste leaves the worker one byte or one line per gap instead of one burst; STOP / HOLD / close /
cancel / Ctrl-C / shutdown inside a gap leave **no delayed write**; unpaced ports keep today's byte-identical path.
Pacing is worker **port state** (core-owned `uart.input` has a fixed shape), set by `uart.open{pacing}` or the new verb
`uart.pacing`, and overridable per agent request (`char_delay_ms`, `line_delay_ms` on `uart.send`/`uart.run`).

### Lease (Sonnet holds every file below; the lead edits none of them meanwhile)

| file | Sonnet may change (bodies only) |
|---|---|
| `crates/loom-uart-worker/src/tx.rs` | the 7 new skeleton bodies of §B2.1 (`Pacing::is_none`, `is_line_end`, `paced_chunk_end`, `gap_after`, `pacing_estimate`, `trailing_gap`, `TxHandle::paced_backlog`) and the existing bodies `TxHandle::submit`, `Worker::run`, `Worker::step`, `Worker::finish_inner` (rules A1–A11). Nothing else, except: at most **two** new private `fn pace_*` helpers inside `impl Worker`, each with a one-line doc comment, reported in the return. |
| `crates/loom-uart-worker/src/worker/util.rs` | bodies of `pacing_fields`, `parse_pacing`, `pacing_json` (§B2.2). |
| `crates/loom-uart-worker/src/worker/port.rs` | bodies of `Port::new` (init the `pacing` field), `Port::pacing`, `Port::set_pacing`, and the `TxEvent::Done` arm of `Port::tx_loop` (§B2.3). |
| `crates/loom-uart-worker/src/worker/send.rs` | bodies of `parse_send`, `agent_write`, `result_detail`, `submit_user` (§B2.3). |
| `crates/loom-uart-worker/src/worker.rs` | bodies of `UartWorker::call` (one new match arm), `open_inner`, `set_pacing`, `status`, `open_result` (§B2.4). |

**Already written by the lead (read-only for Sonnet):** every type, field, constant, signature, import and doc comment added for
B2 in the five files above (`Pacing`, `PACING_CHAR_MAX/LINE_MAX`, `TxRequest.pacing`, `LaneQ.paced`, `State.not_before`,
`USER_TX_DEADLINE`, `USER_PACING_MAX`, `SendOp.pacing`, `PACING_KEYS`, `TxWait.input_seq`, `Port.pacing`, `Port::new(…, pacing)`,
`open_result(info, pacing)`, `UartWorker::set_pacing` signature); the catalog/schema data in `crates/loom-uart/src/commands.rs`
(verb `uart.pacing` M1 `RoleSet::WRITE` write_class none; `uart.open.pacing`; `uart.send|run.char_delay_ms|line_delay_ms`;
`uart.pacing` params) and `crates/loom-uart/src/kinds.rs` (`uart.config.pacing`; `uart.input_ack` state `partial`, reasons
`pacing_too_long|interrupted_by_user|deadline|io_error|shutdown`, `written_bytes`, `remaining_bytes`); the tests
`crates/loom-uart-worker/tests/tx_pacing.rs` (23) and `tests/worker_pacing.rs` (19), and the `pacing: Pacing::NONE` field added to
the request builders of `tests/{tx.rs,tx_cancel.rs,common/mod.rs}`.

**Scaffold state handed over (expected, not regressions):** `cargo check -p loom-uart-worker` fails with exactly 9 errors, all
"missing field/argument" at the body sites you own: `Port::new` (`pacing`), `SendOp` (`pacing`), two `TxRequest` literals
(`pacing`), two `TxWait` literals (`input_seq`), two `open_result` calls and one `Port::new` call in `worker.rs`; plus unused-import
warnings that your bodies consume. All test files compile once those are filled (the lead verified this with a throw-away shim that
was removed; scaffold hashes: worker.rs `afde9dfd…`, send.rs `0c85822c…`, port.rs `29409343…`).

### Acceptance (from the worktree root)

```
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-uart-worker
cargo test -p loom-uart
cargo clippy -p loom-uart-worker -p loom-uart --all-targets
rustfmt --edition 2024 --check crates/loom-uart-worker/src/tx.rs crates/loom-uart-worker/src/worker.rs crates/loom-uart-worker/src/worker/util.rs crates/loom-uart-worker/src/worker/send.rs
```
Expected: every test passes, including all pre-existing worker/tx/channel suites unchanged; clippy reports nothing in the leased
files; rustfmt clean on the four files (`port.rs` already has one pre-existing rustfmt diff at baseline line ~460 — do not reformat
that file, keep your edits formatted). No full-workspace build. Paste each command's exit status and tail.

---

### §B2.1 `tx.rs` — pacing model and TX actor

Pure functions (deterministic, no clock):

- `Pacing::is_none()` → `char_delay.is_zero() && line_delay.is_zero()`.
- `is_line_end(bytes, i)` → `i < len && (bytes[i] == b'\n' || (bytes[i] == b'\r' && bytes.get(i + 1) != Some(&b'\n')))`.
  Example `b"a\r\nb\rc\n\r"` → line ends at 2, 4, 6, 7 (CR before LF is not one; a trailing CR is).
- `paced_chunk_end(bytes, start, p)` (`start < len`): `cap = min(start + TX_CHUNK_MAX, len)`; `p.char_delay > 0` → `start + 1`;
  else `p.line_delay > 0` → `i + 1` for the first `i` in `start..cap` with `is_line_end`, else `cap`; else (`NONE`) `cap`.
  Examples (line 100): `b"ab\r\ncd\rxy"` from 0 → 4, from 4 → 7, from 7 → 9; 3000 bytes without line end → 1024.
- `gap_after(bytes, end, p)` (`end ≥ 1`; `end == 0` → zero): `(char_delay if > 0) + (line_delay if > 0 and is_line_end(bytes, end - 1))`.
  It always looks at the **whole request** bytes, so a CR whose LF is the next chunk is not a line end.
- `pacing_estimate(bytes, p)`: zero for `NONE` or empty; else walk `start = 0 → paced_chunk_end → …` and sum `gap_after(end)` for
  every chunk **except the last** (saturating). Example `b"ab\r\ncd\r\n"`: char 10 / line 100 → 170 ms; line-only 100 → 100 ms.
- `trailing_gap(bytes, p)`: zero for empty; else `gap_after(bytes, len, p)`. Same example: 110 ms / 100 ms. `b"k"`, char 50 → 50 ms.
- `TxHandle::paced_backlog(lane)`: under the lock, `lanes[lane].paced + (not_before[lane] − now, saturating; zero if none)`.

Actor rules (the effective pacing of a request is `Pacing::NONE` on `TxLane::CtrlC`, else `req.pacing`):

- **A1 submit:** unchanged validation and limits. If the effective pacing is not none, add `pacing_estimate + trailing_gap` of the
  request to `lanes[lane].paced` (saturating) in the same critical section that queues it.
- **A2 release:** every terminal path goes through `finish_inner`; there, **before** the payload is dropped, subtract the same
  charge (recomputed from the bytes; saturating) when the effective pacing is not none. Covers sent, failed, cancelled-from-queue,
  shutdown.
- **A3 chunking:** `step` writes `bytes[written..paced_chunk_end(bytes, written, eff)]` instead of `written + TX_CHUNK_MAX`.
- **A4 gap:** after a write with `r.written > 0` and a non-none effective pacing, set `not_before[lane] = Some(Instant::now() + g)`
  with `g = gap_after(bytes, written_after_this_write, eff)`, or `None` when `g` is zero. This also happens after the request's last
  byte (trailing gap): it delays the **next** request on that lane, never this request's `TxDone`.
- **A5 wait:** when the lane picked at a boundary is User/Agent, its active request has no cancel claim, `now < its deadline`, and
  `not_before[lane] > now`, the actor does **not** step it and does **not** step any lower lane. After running pending controls and
  the cancelled-request completions it blocks in `cv.wait_timeout` until `min(not_before, deadline)` (lock released), then
  re-evaluates from the top. No busy loop: with nothing else to do it must be blocked in that wait. `not_before ≤ now` → clear it and
  step. A queued request activated on a lane whose `not_before` is still ahead waits the same way.
- **A6 deadline:** once `now ≥ deadline`, `step` runs and its existing deadline check ends the request (`Timeout`; `OutcomeUnknown`
  if bytes were written) without another write — even if `not_before` is still ahead.
- **A7 Ctrl-C:** in the critical section where `pick == CtrlC` (beside today's `cancel_agent` decision): (a) an active User request
  with non-none pacing is finished with `Some(TxErrorCode::InterruptedByUser)` exactly like the agent is today; (b) every **queued**
  User request with non-none pacing moves to the cancelled mailbox with cause `InterruptedByUser` (respect
  `TX_CANCEL_QUEUE_ITEMS/BYTES`; one that does not fit stays queued); (c) `not_before[User] = None`. Unpaced User requests are left
  alone (today's behaviour). The Ctrl-C bytes themselves are never paced and never wait.
- **A8 cancel / STOP / HOLD / close in a gap:** `cancel`/`cancel_lane` already notify the condvar; the woken loop finishes the
  claimed request via `finish_cancelled_active`. After `cancel` returns `InProgress` for a request that is in a gap, **zero** further
  bytes of it are written.
- **A9 shutdown in a gap:** `stopping` wakes the wait at once; `drain_on_shutdown` as today.
- **A10 unpaced path:** a `Pacing::NONE` request on a lane without `not_before` behaves exactly as before (1 KiB chunks, no wait,
  no change in event order). All pre-existing tests pass unchanged.
- **A11 controls:** DTR/RTS/break requests are still drained at every boundary, including during a gap (a control submitted in a
  gap runs within one wake-up).

### §B2.2 `worker/util.rs` — argument parsing

- `pacing_fields(v, base)`: for `char_delay_ms` (max `PACING_CHAR_MAX`) and `line_delay_ms` (max `PACING_LINE_MAX`): key absent or
  `null` → keep `base`'s value; otherwise it must be `as_u64()` and ≤ the max → `Duration::from_millis(n)`; anything else (negative,
  fraction, string, > max) → `E_USAGE` with message `bad {key}: {value} (0–{max} ms)`. Validate both keys before returning (no
  partial result). Other keys of `v` are ignored (it is also used on the whole `uart.send` op and the `uart.pacing` args).
- `parse_pacing(v)`: `None` / `null` → `Ok(None)`; an object → any key not in `PACING_KEYS` → `E_USAGE` `unknown pacing field '{k}'`,
  else `Ok(Some(pacing_fields(v, Pacing::NONE)?))` (`{}` → `Some(NONE)`); any other JSON type → `E_USAGE` `pacing must be an object`.
- `pacing_json(p)` → `{"char_delay_ms": <ms>, "line_delay_ms": <ms>}` as integers (use `u64::try_from(d.as_millis())`, saturating; no
  `as` casts).

### §B2.3 `worker/port.rs`, `worker/send.rs` — port state, viewer input, agent writes

- `Port::new`: initialise `pacing: Mutex::new(pacing)`. `Port::pacing()` → the value under the lock; `Port::set_pacing(p)` →
  replace, return the previous value. Pacing is captured per request at submit; changing it never alters queued/active requests.
- `submit_user` (viewer keys, `uart.input.clear/yield/takeover` resends): keep the preempt/empty/lane logic. Then
  `pacing = NONE` for the Ctrl-C lane, else `port.pacing()`. If not none: `total = pacing_estimate(send, pacing) +
  port.tx.paced_backlog(lane)` (saturating). If `total > USER_PACING_MAX`: emit `uart.input_ack` `{input_seq: "<seq>", state:
  "discarded", reason: "pacing_too_long", estimate_ms: <total ms>}` (same draft/origin/ts as today's `queue_full` correction), call
  `port.arb().interrupt_written(1)` if `interrupt` (as the submit-error path does), write nothing, and return **before** the
  `TxWait` insert. Deadline = `now + USER_TX_DEADLINE` (+ `total` when paced). `TxWait.input_seq = Some(input_seq)`;
  `TxRequest.pacing = pacing`. The existing `queue_full` path stays.
- `Port::tx_loop`, `TxEvent::Done` arm: keep everything; additionally, when the removed wait has `agent_request == None`,
  `input_seq == Some(seq)` and `d.state != TxState::Sent`, emit `uart.input_ack` `{input_seq: "<seq>", state: "discarded" if
  written_bytes == 0 else "partial", reason: d.cause (fallback "shutdown"), written_bytes, remaining_bytes}` with origin `User`
  and `ts_now(HostReadComplete)`. (A Ctrl-C-cut paste therefore reports `partial` + `interrupted_by_user` + exact counts.)
- `parse_send`: `pacing: pacing_fields(op, port.pacing())?` (bad override → `E_USAGE`, through today's parse-error path:
  `requested` + `failed`, nothing written).
- `agent_write`: the `requested` row gains `pacing: pacing_json(op.pacing)` when not none. After the STOP and `connected` checks and
  **before** admission (①): if not none and `pacing_estimate(&op.bytes, op.pacing) > remaining(cx, w.deadline_mono_ns.get())` →
  `rows.fail(Failed, werr_detail(E_USAGE, "pacing needs {e} ms but the request deadline leaves {b} ms; raise the timeout or lower
  the pacing", {"estimate_ms": e, "budget_ms": b}), {})` (no admission, no `accepted` row, no bytes). The agent `TxRequest` gets
  `pacing: op.pacing`, its `TxWait` `input_seq: None`. Everything else unchanged.
- `result_detail`: add `"pacing": pacing_json(op.pacing)` only when not none (unpaced results stay byte-identical).

### §B2.4 `worker.rs` — open, verb, status

- `open_inner`: `let pacing = parse_pacing(op.get("pacing"))?;` before the port slot is locked (so a bad value opens nothing).
  Idempotent reopen (same target/config/profile): pacing absent or equal to `p.pacing()` → `Ok(open_result(&p.info, p.pacing()))`;
  different → `E_CONFIG` `worker already owns '<target>' with other pacing; use uart.pacing or close it first`. Other differences keep
  today's `E_CONFIG`. New port: pass `pacing.unwrap_or(Pacing::NONE)` to `Port::new`; the `uart.config` event payload gains
  `pacing: pacing_json(..)`; result = `open_result(&port.info, port.pacing())`.
- `open_result(info, pacing)`: today's object + `"pacing": pacing_json(pacing)`.
- `call`: add the arm `"uart.pacing" => self.set_pacing(args),` (before the request-write arm).
- `set_pacing(args)`: `port_for(args)?`; `new = pacing_fields(args, port.pacing())?`; `old = port.set_pacing(new)`; when
  `old != new` emit one `uart.config` with the **full** payload (`config_json` + `profile` + `env` + `pacing`, as at open) — no event
  when unchanged. Return `{"channel", "generation", "pacing": pacing_json(new), "changed": old != new}`. Not open → `E_NOT_OPEN`
  (from `port_for`).
- `status`: when a port is open, `extra` gains `"pacing": pacing_json(p.pacing())`.

### Test map (lead-written; do not edit)

`tests/tx_pacing.rs`: constants; `is_none`; line ends; unpaced/char/line chunking; `gap_after`; estimate/trailing incl. a full
64 KiB lane; 3-line send does not burst (one line per write, ≥ 80 ms apart); char pacing; unpaced 1 KiB chunks never wait; trailing
gap spaces the next request but not `TxDone`; paced user line blocks the agent lane; Ctrl-C lane never paced; `cancel` in a gap ends
< 150 ms with no later byte; `cancel_lane(held_by_user)` in a gap; shutdown in a gap < 200 ms; deadline inside a gap; Ctrl-C cancels
active+queued paced user requests, goes out < 150 ms, clears the user gap; Ctrl-C leaves unpaced user requests alone; controls run
in a gap; `paced_backlog` charge/release.
`tests/worker_pacing.rs`: open pacing in result/`uart.config`/status; default zeros (`null`, `{}`); 8 bad values → `E_USAGE`, nothing
opened; boundary 1000/10 000; reopen rules; verb change/partial/no-change/query + one full `uart.config`; verb all-or-nothing
`E_USAGE`; verb `E_NOT_OPEN`; line-paced agent send (3 writes ≥ 60 ms apart, `pacing` in result and `requested` row); per-request
overrides; bad override; estimate over deadline refused before admission (`estimate_ms` 7900); STOP / HOLD / close inside a gap
leave no later write; paced viewer input; viewer cap → `discarded`/`pacing_too_long`; Ctrl-C cuts a paced paste with a `partial`
ack and exact counts; unpaced input has only the arbiter's `sent` ack.

### Return

Write `/Users/ctmctm/workspace/Loom/.worktrees/operator-uart/work/reports/operator-upgrade/uart/B2-WORKER.md`: files changed with
sha256, any `pace_*` helper signatures, deviations/questions, each acceptance command's exit status and tail. Then stop and wait.

---

## Batch 8 — U8 "장치가 … 다시 열었습니다" open toast (ROOT R4, narrow shell edits) — PLAN_READY_BATCH8

Problem (RETURN-uart U8): a port the user opened from the UART workbench toasts "장치가 uart X를 다시 열었습니다 [패널 열기]" while the
panel is already visible. Causes: (1) "다시" is chosen when the channel *entry existed* (any earlier control event creates it), not
when this tab saw it open before; (2) `uart.state` is emitted with origin `device`, so the subject is "장치가"; (3) the shell cannot
tell a UI-initiated open from an agent's; (4) `[패널 열기]` is offered even when that channel's panel is shown.
ROOT R4 scope: only the open-toast logic in `store/shellControl.ts`, `store/channels.ts`, `shell/services.ts`; plus UART's own
`workbench.ts` call. ETH expectation (acknowledged): first opens never say "다시"; ETH tests do not assert shell toast text.

### Lease (bodies only; the lead edits none of these files meanwhile)

| file | Sonnet may change |
|---|---|
| `ui/src/store/shellControl.ts` | bodies of `noteLocalOpen` and `applyChannelSignal` only. |
| `ui/src/store/channels.ts` | body of `openedToastText` only. |
| `ui/src/shell/services.ts` | the body of the `shell.onChannelOpened(({ … }) => { … })` callback inside `bind()` only (destructure `local` there). |
| `ui/src/workbench/uart/workbench.ts` | body of `UartBench.openSession` only. |

Lead-written (read-only): `ChannelEntry.everOpen?`, `ChannelOpened.local`, `LOCAL_OPEN_TTL_MS` (30 000), `MAX_LOCAL_OPENS` (64),
the private `localOpens` map, the `noteLocalOpen` / `openedToastText` signatures and docs, the `services.ts` import line
(`openedToastText`, `viewsForChannel`), tests `src/store/shellControl.b8.test.ts` (6), `src/workbench/uart/workbench.b8.test.ts` (2),
and in `src/shell/m1b.test.tsx` four new `UART B8:` cases plus one updated expectation (join-grace psu case: "열었습니다", not "다시").
Scaffold state: `tsc` reports exactly one error — the `ChannelOpened` literal in `applyChannelSignal` lacks `local`; eslint may report
the two not-yet-used imports in `services.ts`; the 11 B8 tests fail with UNIMPLEMENTED / old text.

### §B8.1 `shellControl.ts`

- `noteLocalOpen(domain, channel, nowMs)`: key `${domain}/${channel}`; first delete every entry whose expiry `< nowMs`; then set/refresh
  `key → nowMs + LOCAL_OPEN_TTL_MS` (re-insert so it becomes newest); while `size > MAX_LOCAL_OPENS` delete the oldest inserted key.
- `applyChannelSignal(sig, nowMs)`: keep every existing rule (stale seq, join grace, `opened` definition, listeners). Changes:
  `const reopen = e.everOpen === true` is read **before** this signal can set it; whenever `e.state === 'open'` after applying the
  signal set `e.everOpen = true` (also in the join-grace early-return path, so a channel open at join is "seen open").
  When `opened`, compute `local`: the key is in `localOpens` with expiry `≥ nowMs` → `true` and delete it (consumed once); an expired
  entry is deleted and gives `false`. Emit `{ entry: e, signal: sig, isNew: !reopen, local }`. A local note is consumed only by an
  `opened` transition (not by bar-only grace reports).

### §B8.2 `channels.ts` — `openedToastText(origin, what, reopen)`

- `origin === 'device'` → `` `${what} ${reopen ? '다시 열림' : '열림'}` `` (the state event's origin is the channel itself, never a
  device "actor").
- otherwise → `` `${ORIGIN_SUBJECT[origin] ?? `${origin}가`} ${what}를 ${reopen ? '다시 열었습니다' : '열었습니다'}` `` (today's wording).
- `ORIGIN_SUBJECT` itself stays unchanged.

### §B8.3 `services.ts` — the `onChannelOpened` callback

`({ entry, signal, isNew, local })`:
1. `local` → return (no toast, no auto-open: the UI that asked for the open shows its own panel/feedback).
2. `dp = defaultPanelFor(...)`; `what` and `key` as today; `text = openedToastText(signal.origin, what, !isNew)`.
3. `shown = viewsForChannel(entry.domain, entry.channel).some((v) => { const slot = { panel: v.panel, props: v.props };
   return dock.current ? dock.current.find(slot) !== null : (route.kind === 'panel' && sameSlot(route.slot, slot)) ||
   uiState.extraSlots.some((x) => sameSlot(x, slot)); })` with `route = parseRoute(currentHash())`.
4. Auto-open branch (same condition as today): `r = showSlot(dp, { activate: false })`; toast `` `${text}${r === 'added' ? ' — 패널을 열었습니다' : ''}` ``
   (`key`, `ttlMs: 8_000`).
5. Else: toast `text` (`key`, `ttlMs: 12_000`) with `actions: dp && !shown ? [{ label: '패널 열기', run: () => void showSlot(dp) }] : []`.

### §B8.4 `workbench.ts` — `openSession`

Keep every check. Immediately before `await sv.gate.run(OPEN_SPEC, …)` call `sv.shell.noteLocalOpen('uart', s.alias, sv.now())`.
(Not when the channel is already open — the early return stays first.)

### Acceptance (from `ui/`)

```
mkdir -p ../work/typecheck
npx vitest run src/store src/shell src/workbench/uart src/panels/uart
npx vitest run
npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/store/shellControl.ts src/store/channels.ts src/shell/services.ts src/workbench/uart
```
Expected: all tests pass (full suite once), both `tsc` exit 0, eslint clean. Never `tsc -b`. No other shell behaviour changes.

### Return

Write `/Users/ctmctm/workspace/Loom/.worktrees/operator-uart/work/reports/operator-upgrade/uart/B8-WORKER.md` (files + sha256,
deviations/questions, each command's exit status and tail). Then stop and wait.

---

## Batch 3 — U6 line signals + U2 opt-in bounded reconnect (Rust worker) — PLAN_READY_BATCH3

Goals: (U6) `uart.signal` works — today the palette Break answers `E_NOT_IMPLEMENTED`; a break or pulse is always released.
(U2) opt-in reconnect after an unplug: same target, same recorded USB identity when one is known, a **new generation**, bounded
window, STOP/close end it, nothing that was running is resumed (COMMON: "Reconnect is opt-in, same validated device identity, new
generation; no automatic macro/REC continuation by accident").

### Lease (bodies only; the lead edits none of these files meanwhile)

| file | Sonnet may change |
|---|---|
| `crates/loom-uart-worker/src/worker/signal.rs` (new) | bodies of `parse_signal`, `line_json`, `signal_write`; at most one private helper (io error → `ErrorCode`). |
| `crates/loom-uart-worker/src/worker/reconnect.rs` (new) | bodies of `PortIdentity::is_known`, `PortIdentity::matches`, `SystemReopener::{identity, open}`, `parse_reconnect`, `reconnect_json`, `identity_json`, `supervise`; at most two private helpers. |
| `crates/loom-uart-worker/src/worker.rs` | bodies of `UartWorker::{call, write, open_inner, status, set_reopener, reopener, spawn_port, reopen_after_disconnect}`. `open_inner` keeps its parsing/validation/idempotence and moves the port start into `spawn_port`. |
| `crates/loom-uart-worker/src/worker/port.rs` | bodies of `Port::new` (init `lines`, `reconnect`, `disconnect_tx`), `Port::set_disconnect_notify`, `Port::shutdown` (drop the notifier), and the `BridgeItem::Disconnected` arm of `Port::ingest`. |

Lead-written (read-only): all new types/consts/signatures/imports/docs (`SignalKind`, `SignalOp`, `LineState`, `BREAK_DEFAULT_MS` 250,
`SIGNAL_MAX_MS` 5000, `SIGNAL_CONTROL_TIMEOUT` 2 s, `ReconnectCfg` (+`OFF`), `PortIdentity`, `Reopener`, `SystemReopener`,
`ReconnectState`, `ReopenStep`, `RECONNECT_*` (poll 300 ms, window 10 s default, 1–120 s), `PortSpec`, `PortInfo.{sim_pty,
reconnect, identity}`, `Port.{lines, reconnect, disconnect_tx}`, `kind::SIGNAL`, `Rows` made `pub(crate)` in send.rs); catalog/schema
data (`uart.signal` → M1; `uart.open.reconnect`; `uart.state` `reconnecting` + `reason|reconnect|reopen|expected|found`;
`uart.signal` event `result|error`); tests `tests/worker_signal.rs` (10), `tests/worker_reconnect.rs` (12), and
`crates/loom-uart/tests/registry.rs` (signal/pacing now available).
Scaffold state: `cargo check -p loom-uart-worker` = 2 errors (missing fields in the `Port` and `PortInfo` literals at your sites);
the new tests compile once those are filled (lead-verified with a removed shim).

### §B3.1 `signal.rs` — `uart.signal{channel, kind, value?, duration_ms?}` (write class request)

- `parse_signal`: `kind` must be `"break"|"dtr"|"rts"` (else `E_USAGE` "bad signal kind"). `duration_ms`: absent/null → none; else
  `as_u64()` in `1..=SIGNAL_MAX_MS` (else `E_USAGE`). **break**: `value` absent/null/`true` (`false` → `E_USAGE` "break is a pulse"),
  duration defaults to `BREAK_DEFAULT_MS`; `value = true`. **dtr/rts**: `value` must be a JSON bool (else `E_USAGE`); duration present
  = pulse (set `value`, wait, set `!value`), absent = set and keep.
- `line_json(l)` → `{"dtr": bool|null, "rts": bool|null, "break": bool}`.
- `signal_write(port, cx, w)` (mirror `agent_write` shape; use `Rows {port, origin: w.origin, client: w.client_id or "agent",
  request_id: w.request_id}`):
  1. parse; error → `requested` row `{}` then `rows.fail(Failed, err, {})`. Else `requested` row `{"signal": "<kind>", "value",
     "duration_ms"?}`.
  2. Refusals (no line touched, no `uart.signal` event): STOP latched and `w.origin != User` → `E_STOPPED`; HOLD on and origin not
     User → `E_HELD_BY_USER`; `!port.connected` → `E_DISCONNECTED`. Then `accepted` row `{}`.
  3. Run each control with `tokio::task::spawn_blocking` on a clone of `port.tx`: `tx.control(op, SIGNAL_CONTROL_TIMEOUT)`.
     break: `Break(true)`; on Ok `lines.break_on = true`; `tokio::time::sleep(duration)`; then **always** `Break(false)` (even if
     the assert failed, even if STOP/HOLD arrived meanwhile); on Ok `break_on = false`. dtr/rts set: one op; on Ok record the level.
     pulse: set `value`, sleep, **always** set `!value`; record each level that succeeded.
  4. Result: first op failed → `Failed`, event `result:"failed"`; first ok but release failed → `OutcomeUnknown`, message
     "release failed — the line may still be held", event `result:"release_failed"` (`break_on` stays true); else `Sent`, `"ok"`.
     io error → code: `Unsupported`→`E_UNSUPPORTED`, `TimedOut`→`E_TIMEOUT`, `WouldBlock`→`E_RATE_LIMIT`, `BrokenPipe`→`E_NOT_OPEN`,
     other → `E_DISCONNECTED`. A spawn_blocking join error → `E_INTERNAL`.
  5. After the ops, emit one `uart.signal` event (`kind::SIGNAL`, `ts_now(HostReadComplete)`, origin `w.origin`, `client_id`,
     `request_id`) payload `{"kind", "value", "duration_ms"? , "result", "error"?}`; then the final row (`sent` `{"final": true}` or
     `rows.fail(...)`). `WriteOutcome.detail` = `{"request_id", "kind", "value", "duration_ms"?, "lines": line_json(..),
     "generation"}` (also on failure).
- `worker.rs`: `write` gets the arm `"uart.signal" => match self.port_for(&w.op) { Ok(port) => signal_write(port, cx, w).await,
  Err(e) => failed outcome }`; `call` lists `"uart.signal"` with the other request writes (`E_USAGE` "… is a request write").

### §B3.2 `reconnect.rs`

- `PortIdentity::is_known` = any field `Some`. `matches(found)` = for every field `Some` in `self`, `found` has the same `Some`.
- `SystemReopener::identity(target)`: `transport::list_ports()`; error or `truncated` → `None`; the entry whose `path == target` →
  `Some({vid, pid, serial})`; none → `None`. `open` = `transport::open(target, config, opts)`.
- `parse_reconnect(v)`: absent/null → `Ok(None)`; object with keys ⊆ {`enabled`, `window_ms`} (else `E_USAGE`); `enabled` absent →
  `true`, present must be bool; `window_ms` absent → `RECONNECT_WINDOW_DEFAULT`, present `as_u64()` within MIN..=MAX ms (else
  `E_USAGE`); any other JSON type → `E_USAGE`.
- `reconnect_json(c, st)` → `{"enabled", "window_ms", "active", "attempts"}`; `identity_json(i)` → `{"vid", "pid", "serial"}` (nulls kept).
- `supervise(w, cx, rx, generation, cfg)` (its own thread, one per port generation with reconnect enabled):
  1. `rx.recv()`: `Err` (port shut down) → return.
  2. The current port must still be generation `generation` (else return). Set its `reconnect` state `{active: true, attempts: 0}`;
     emit `uart.state` `{"state": "reconnecting", "path", "reconnect": reconnect_json(..)}` through that port.
  3. Loop: STOP latched (`cx.latch().is_on()`) → end `"stopped"`; `elapsed ≥ cfg.window` → end `"gave_up"`;
     `thread::sleep(RECONNECT_POLL)`; `attempt += 1` (store in state); `w.reopen_after_disconnect(&cx, generation, attempt,
     elapsed_ms)`: `Reopened` → return; `NotYet` → continue; `IdentityMismatch{expected, found}` → end `"identity_mismatch"`;
     `Gone` → return silently.
  4. End(reason): only if the slot still holds generation `generation`: state `{active: false, attempts}`, emit `uart.state`
     `{"state": "disconnected", "path", "reason", "reconnect": reconnect_json(..)}` (+ `"expected"`, `"found"` identity_json for a
     mismatch). The port stays in the slot, disconnected, as today (close it to reopen manually).

### §B3.3 `worker.rs`, `port.rs` — start, reopen, status

- `set_reopener(r)` stores it (`inner.reopener`); `reopener()` returns it or `Arc::new(SystemReopener)`.
- `open_inner`: also `let reconnect = parse_reconnect(op.get("reconnect"))?;` before locking the slot. Idempotent reopen: as B2,
  plus a given `reconnect` different from `p.info.reconnect` → `E_CONFIG` "… with other reconnect setting; close it first".
  New port: `identity` = when `reconnect.enabled`, `self.reopener().identity(&target)` kept only if `is_known()` (else `None`);
  when disabled, `None` (no port enumeration on ordinary opens). Build `PortSpec { …, pacing, reconnect: reconnect.unwrap_or(OFF),
  identity, hold: false }`, transport as today (injected or `transport::open`), `spawn_port(cx, &spec, transport, None)`,
  store in the slot, return `open_result`.
- `spawn_port(cx, spec, transport, reopen)` — today's start sequence moved here unchanged (kind/rx_handle, kept core +
  generation, emitter, reader, TX actor, arbiter, `PortInfo` with the 3 new fields, `Port::new(…, spec.pacing)`, `port.start`,
  `inner.opens += 1`), then: `spec.hold` → `set_hold(true, now)` on the new arbiter, emit its events, dispatch its actions;
  `spec.reconnect.enabled` → `sync_channel(1)`, `port.set_disconnect_notify(tx)`, spawn thread `loom-uart-reconnect` running
  `supervise(self.clone(), cx.clone(), rx, generation, spec.reconnect)` (spawn error → `open_error`); emit `uart.state open`
  (today's payload + `"reopen": reopen` when given) and `uart.config` (with `pacing`, as B2). It never locks the port slot.
- `reopen_after_disconnect(cx, expect_gen, attempt, after_ms)`: hold the lifecycle lock, then the slot lock. Slot empty, other
  generation, or `connected` → `Gone`. `found = self.reopener().identity(&target)`; recorded identity `Some(exp)`: `found` `None`
  → `NotYet("device absent")`, `Some(f)` with `!exp.matches(&f)` → `IdentityMismatch{expected: exp, found: f}`; recorded `None` →
  path-only, continue. `reopener().open(&target, &config, &OpenOpts{sim_pty: info.sim_pty, ..default})` error →
  `NotYet(e.to_string())`. Then take the old port out of the slot: `cancel_agents("closed", E_NOT_OPEN)`, read `hold_on()` and
  `pacing()`, `shutdown()`, put its core into `kept` (as `close` does) — **no** `closed` state event. Spec from the old `PortInfo`
  (`Profile::named(&info.profile)`; `env`, `config`, `sim_pty`, `reconnect`, `identity`) + live pacing/hold;
  `reopen = {"attempt", "after_ms", "identity": "usb"|"path_only", "previous_generation": expect_gen}`; `spawn_port` → store,
  `Reopened(new_gen)`; a `spawn_port` error → `Gone` (the channel is then closed; its ring is kept).
- `status`: when open, `extra` also gets `"lines": line_json(..)`, `"reconnect": reconnect_json(info.reconnect, state)`,
  `"identity": identity_json(..)` or `null`.
- `port.rs`: `Port::new` initialises `lines`/`reconnect` to default and `disconnect_tx` to `None`; `set_disconnect_notify` stores
  the sender; the `Disconnected` arm keeps today's work and then `take()`s the sender and `try_send(())` once; `shutdown` `take()`s
  (drops) the sender first so an idle supervisor ends.

### Acceptance (from the worktree root)

```
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-uart-worker
cargo test -p loom-uart
cargo clippy -p loom-uart-worker -p loom-uart --all-targets
rustfmt --edition 2024 --check crates/loom-uart-worker/src/worker/signal.rs crates/loom-uart-worker/src/worker/reconnect.rs crates/loom-uart-worker/src/tx.rs
```
Expected: all suites pass (B2 suites unchanged), clippy no warnings, rustfmt clean on those files; keep your `worker.rs`/`port.rs`
edits formatted (port.rs has one pre-existing baseline hunk — leave it). No real serial device is touched (tests use fakes).

### Return

`/Users/ctmctm/workspace/Loom/.worktrees/operator-uart/work/reports/operator-upgrade/uart/B3-WORKER.md` (files + sha256, helpers,
deviations/questions, each command's exit status and tail). Then stop and wait.

---

## Batch 5 — U1-B past-range journal export + U4 last-view toast (UI) — PLAN_READY_BATCH5

U1-B: export a past range of one channel from the daemon journal (also before REC was started) with a truthful header — ROOT R2:
read-only `core.events.query` with `peek: true`, gaps/caps stated. U4: closing the last view of an open channel never closes the
port (unchanged), but now says so with [포트 닫기] / [다시 보기].

### Lease (bodies only; the lead edits none of these files meanwhile)

| file | Sonnet may change |
|---|---|
| `ui/src/panels/uart/journalExport.ts` (new) | every function / method body (§B5.1). Nothing else. |
| `ui/src/panels/uart/terminalPanel.ts` | bodies of `startJournalExport`, `fillRecDialog`, and the panel's `dispose` (one added line). |
| `ui/src/workbench/uart/workbench.ts` | bodies of `UartBench.lastViewClosed` and `UartBench.registerTerminal`. |

Lead-written (read-only): all types/constants/signatures in `journalExport.ts` (incl. `ANSI_RE`, limits, `EXPORT_KINDS`); in
`terminalPanel.ts` the import line, `queries: [EXPORT_QUERY]`, the `jx` / `jxLast` / `jxSel` state; in `workbench.ts`
`LAST_VIEW_TOAST_MS` (400) and `lastViewTimers`; tests `journalExport.test.ts` (27), `batch5.panel.test.ts` (5),
`workbench.b5.test.ts` (5). Scaffold: `tsc` clean; the new tests fail with UNIMPLEMENTED / missing DOM.

### §B5.1 `journalExport.ts`

**Erratum E1 (2026-11-16, lead-authorized, worker adds it under the lease):** `journalExport.ts` also imports
`import { decodePayloadBytes } from '../../sdk/terminalView';` (needed for byte lengths and decoding).

Helpers:
- `wallMsOf(ev)`: `ev.ts.wall_ns` decimal string → `Number(BigInt(ns) / 1_000_000n)`; missing/invalid → 0.
- `cursorAt(epoch, seq)` → `` `e${epoch}:seq:${seq}` ``. `fmtWall(ms)` → local `YYYY-MM-DD HH:mm:ss.SSS`.
- `exportFileName(ch, fmt, now)` → `` `${ch}-journal-${YYYYMMDD}-${HHmmss}.${fmt}` `` (local). `rangeText`: `'보존 범위 전체'`,
  `` `최근 ${m}분` ``, `` `연결 세대 ${g}` ``.
- `errCode(e)` → `e.dataCode` if a string else null. `errDetail(e)` → `e.data.detail` if an object, else `e.data` if an object, else null.
- `findFirstSeq(lo, hi, test, maxProbes)`: binary search for the first `true` of a monotone predicate in `[lo, hi]`; each `test`
  call is one probe; stop at `maxProbes` with `exhausted: true` (then `seq` = smallest seq known true so far, or null).
  Returns `{seq, probes, exhausted}`.

`GapTracker.observe(ev)`:
- `record.gap` → `{kind: 'record_gap', atSeq: ev.seq, detail: 'record.gap' + (payload.lost_bytes is a number ? `` ` ${n} B` `` : '')}`
  (+ `bytes` when known).
- `uart.data` with `dir === 'rx'` and `payload.rx_offset` (number or decimal string): `len` = `payload.len` (number) or the decoded
  byte length (`decodePayloadBytes(payload.bytes)`); if a previous RX set `nextRx` and `off > nextRx` → gap `{kind: 'rx_offset',
  atSeq, bytes: off − nextRx, detail: `` `RX ${n} B 누락 (rx_offset ${nextRx}→${off})` ``}`; then `nextRx = max(nextRx, off + len)`.
  Everything else → null.

`ExportWriter` (`put(s)` = add `utf8(s).length` to `bytes` unless that exceeds `maxBytes` → set `capped`, drop it and everything
after; `body()` = parts joined, so `utf8(body()).length === bytes`):
- `.log` `uart.data` with bytes: decode per direction with the streaming `dec[dir]` and apply REC's line rules exactly
  (`ANSI_RE` removed; `\n` ends the line; `\r` skipped; `\b`/`\x7f` delete the last character; other chars `< ' '` except `\t`
  dropped; a pending line of `EXPORT_LINE_MAX` characters is written with `` ` ⟨줄바꿈 없이 64 KiB — 다음 줄에 이어짐⟩` ``). A line is
  `` `${fmtWall(t)} #${seq} ${DIR}  ${text}\n` `` where `t`/`seq` belong to the record whose chunk **started** the line. RX and TX
  keep separate pending lines.
- `.log` `uart.data` without decodable bytes (secret / raw_ref): flush that direction's pending line, then
  `` `${fmtWall(t)} #${seq} ${DIR}  ⟨바이트 없음 (${payload.secret ? 'secret' : 'raw_ref'})⟩\n` ``.
- `.log` other kinds: `` `-- ${fmtWall(t)} #${seq} ${kind} ${JSON.stringify(payload)}\n` ``.
- `note(text, wallMs, seq)` → `.log` `` `-- ${fmtWall(wallMs)} #${seq ?? '-'} ${text}\n` ``; `.jsonl` `{type:'note', seq, t, ms, text}`.
- `.jsonl` data: `{type:'data', epoch, seq, t: ISO, ms, dir, origin, generation ?? null, rx_offset: string|null, len, hex (null
  without bytes), text (streaming decode, null without bytes), secret?: true, producer_id, producer_epoch, source_seq,
  request_id ?? null, client_id ?? null}`; other kinds `{type:'event', epoch, seq, kind, t, ms, generation ?? null, payload}`.
- `finish()`: flush decoders; each pending line → `` `${fmtWall(t)} #${seq} ${DIR}  ${text} ⟨줄바꿈 없음⟩\n` ``.

`runExport(o)` (limits = `{...DEFAULT_EXPORT_LIMITS, ...o.limits}`; every call `o.query(EXPORT_QUERY, {...p, filter: {domains:
['uart'], channels: [o.channel], kinds: [...EXPORT_KINDS]}, peek: true}, o.signal)`):
1. Head: `{max: 1}` (no since) → `epoch = String(stream_epoch)`, `hw = BigInt(stream_seq_hw)`. hw 0 → empty, complete.
2. Oldest retained: `findFirstSeq(1n, hw, probe-ok)` where a probe is `{since: cursorAt(epoch, s), until: cursorAt(epoch, hw), max:
   1}`; `E_CURSOR_GAP` → false; success or `E_SCAN_LIMIT` → true; other errors abort the export as `query_failed`. Probes count
   toward `probes` / `maxProbes` (shared budget for all locate steps); exhausted → `probe_limit`. None retained → gap
   `{kind:'retention', atSeq:null, detail:'요청 범위가 모두 보존 범위 밖'}` and stop.
3. Start: `retained` → oldest. `recent` → `findFirstSeq(oldest, hw, s => first event of the probe is missing or its wall ≥ now −
   m·60 000)`. `generation` → same with `(ev.generation ?? 0) >= g`. For `recent`/`generation`: start === oldest and oldest > 1n →
   gap `{kind:'retention', atSeq: null, detail: '요청 시작이 보존 범위보다 오래됨 (보존 시작 seq N)'}`. Start null → empty, complete.
4. Read pages `{since, until: cursorAt(epoch, hw), max: pageRows}` from `cursorAt(epoch, start)`: before each page check
   `signal.aborted` → `cancelled`, `pages >= maxPages` → `cap_pages`. `unreadable[]` → gaps `{kind:'unreadable', atSeq: from,
   detail: `` `seq ${from}–${to} ${cause}` ``}`. `E_SCAN_LIMIT` → take `errDetail(e).partial` (array) as this page's events and
   continue from `resume_from` (missing → `query_failed`); `E_CURSOR_GAP` → `retention_moved`; an abort error or aborted signal →
   `cancelled`; other errors → `query_failed` (`error` = message). For each event: generation range and `generation > g` → stop;
   tracker gap → push (≤ `EXPORT_MAX_GAPS`, else `gapsDropped++`) and `writer.note('공백: ' + detail, wallMsOf(ev), ev.seq)`;
   `writer.record(ev)`; `writer.capped` → `cap_bytes`, stop; counters (`events`, `rxBytes`/`txBytes` by `payload.len` or decoded
   length, `generations` sorted unique, `first`/`last`). After each page `onProgress({phase:'read', …})`. Continue with
   `next_cursor` while `truncated`; else done.
5. `writer.finish()`; gaps present → `'gaps'` first in `incomplete` (unique). `outBytes = writer.bytes`; `text = exportHeader(s) +
   body`; `fileName = exportFileName(...)`; `onProgress({phase:'done'})`. Never throws for daemon errors.

`exportHeader(s)` `.log` lines (each `# …\n`, in this order):
`# Loom UART 저널 내보내기 · <channel> · <fmtWall(startedMs)>`;
`# provenance: daemon-journal-export · core.events.query (peek) · stream epoch <epoch|?> · 이전 데몬 실행(epoch)의 기록은 포함하지 않음`;
`# 요청 범위: <rangeText> · 상한 seq <hwSeq|?> · 시작 seq <startSeq|->`; `# 보존 시작 seq: <oldestRetainedSeq|?>`;
`# 실제 범위: seq <first>–<last> · <fmtWall(first)> – <fmtWall(last)>` or `# 실제 범위: 없음 (해당 기록 0건)`;
`# 세대(generation): <a, b|없음> · 기록 <events>건 · RX <rx> B · TX <tx> B · 페이지 <pages> · 탐색 <probes>`;
`# 완전성: complete` or `# 완전성: partial: <incomplete.join(', ')>` (+ `` ` — ${error}` `` when set);
when gaps: `# 공백 <gaps.length + gapsDropped>건: ` + the first 20 as `` `${kind}@${atSeq ?? '-'} ${detail}` `` joined by `'; '`;
`# 한도: 출력 <maxBytes/1 MiB> MiB · <maxPages> 페이지 · 페이지당 <pageRows>건 · 탐색 <maxProbes>`;
`# 열: 시각 #seq 방향  텍스트 (ANSI 제거·줄 단위; 원문 바이트는 .jsonl)`.
`.jsonl`: one line `JSON.stringify({type:'header', provenance:'daemon-journal-export', ...summary fields, complete:
incomplete.length === 0})` + `\n`.
`summaryText(s)` → `` `${complete ? 'complete' : `partial: ${incomplete.join(', ')}`} · ${events}건 · ${outBytes} B` `` +
(gaps ? `` ` · 공백 ${n}건` `` : '').
`downloadText(name, text, fmt)` = `Recorder.exportBuffer`'s Blob/`a[download]` path (type `application/x-ndjson` for jsonl, else
`text/plain;charset=utf-8`), returns `{requested, bytes: blob.size}`.

### §B5.2 `terminalPanel.ts` — REC dialog section

In `fillRecDialog` (keep everything that is there), after the existing hint paragraph add:
`<fieldset data-role="jx" ${jx ? 'disabled' : ''}><legend>지난 구간 내보내기 (데몬 저널)</legend><div class="chk-row">` with three
radios `name="jxrange"`: `value="retained"` "보존 범위 전체"; `value="recent"` "최근 " + `<select data-act="jx-minutes">` (options
`EXPORT_RECENT_MINUTES`, value = minutes, text `${m}분`); `value="generation"` "현재 연결 (gen ${port.generation ?? '?'})",
`disabled` when `port.generation === null`; `checked`/`selected` from `jxSel`; `</div></fieldset>`, then
`<div class="hint" data-role="jxstat"></div>` and `<p class="hint">REC를 켜기 전 구간도 데몬 저널 보존 범위 안이면 내보낼 수
있습니다. 형식은 위 선택(.log/.jsonl)을 따릅니다. 이전 데몬 실행의 기록은 포함하지 않으며, 파일 머리글에 실제 범위·완전성·공백을
적습니다.</p>`. In `dlg-f` before the `grow` span: `jx ? <button class="btn" data-act="jx-cancel">{icon('stop')}내보내기 취소</button>
: <button class="btn" data-act="jx-start">{icon('down')}저널에서 내보내기</button>`.
`jxstat.textContent` = running: phase `locate` → `'범위 찾는 중…'`, else `` `저널 읽는 중 · 페이지 ${pages} · ${events}건 ·
${fmtBytes(outBytes)}` ``; idle with `jxLast` → `` `마지막 내보내기: ${summaryText(jxLast)}` ``; else `''` (always `textContent`).
Handlers: radio/select `change` → update `jxSel`; `jx-start` → range from `jxSel` (`generation` uses `port.generation`) and format
`fmt()` → `void startJournalExport(range, fmt)`; `jx-cancel` → `jx?.ac.abort()`.
`startJournalExport(range, format)`: if `jx` return; `jx = {ac, progress: {phase:'locate', pages:0, events:0, outBytes:0}}`;
`recChanged()`; `r = await runExport({query: (rpc, p, sig) => ctx.query(rpc, p, sig), channel, range, format, signal: ac.signal,
nowMs: Date.now(), onProgress: (p) => { if (jx) jx.progress = p; recChanged(); }})`; `jxLast = r`; `r.incomplete` includes
`'cancelled'` → toast `'저널 내보내기 취소 — 파일을 만들지 않았습니다'` (no download); else `downloadText(r.fileName, r.text, format)`
→ toast `` `다운로드 요청: ${r.fileName} (${fmtBytes(bytes)}) · ${summaryText(r)} — 저장 완료 여부는 브라우저/앱의 다운로드에서
확인하세요` `` or `` `다운로드를 요청할 수 없습니다 (${r.fileName}) — 이 환경은 다운로드를 지원하지 않습니다` ``; a thrown error →
`` `저널 내보내기 실패: ${message}` ``; finally `jx = null; recChanged()`. `dispose`: add `jx?.ac.abort();`.

### §B5.3 `workbench.ts` — last view closed (U4)

- `registerTerminal(h)`: keep everything; on add, clear and delete a pending `lastViewTimers` entry for `h.channel`; in the
  disposer's last-view branch also call `this.lastViewClosed(h.channel)`.
- `lastViewClosed(ch)`: replace any pending timer; after `LAST_VIEW_TOAST_MS`: delete the entry; if `this.terminals.all(ch)` is
  empty, `this.channelState(ch) === 'open'` and `services` is attached → `services.toasts.push(`${ch} 포트는 아직 열려 있습니다
  (다른 도구가 쓰려면 닫으세요)`, { key: `uart-last-view-${ch}`, ttlMs: 10_000, actions: [{ label: '포트 닫기', run: () => void
  this.closeChannel(ch).catch((e) => this.services?.toasts.push(`포트 닫기 실패: ${message}`)) }, { label: '다시 보기', run: () =>
  this.services?.showSlot({ panel: 'uart.terminal', props: { channel: ch } }) }] })`. Never closes a port by itself.

### Acceptance (from `ui/`; ROOT resource rule: targeted, `--maxWorkers=2`, no full suite)

```
mkdir -p ../work/typecheck
npx vitest run --maxWorkers=2 src/panels/uart src/workbench/uart
npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/panels/uart src/workbench/uart
```

### Return

`…/work/reports/operator-upgrade/uart/B5-WORKER.md` (files + sha256, deviations/questions, each command's exit status and tail).

---

## Batch 4 — U1-A opt-in native log files (Rust worker) — PLAN_READY_BATCH4

Continuous, bounded, opt-in logging of one channel to `$LOOM_HOME/uart/logs/<channel>/`, written by the worker before hub
publication (PLAN v3 provenance: source identity + generation + rx_offset + host clocks + `emit` flag; **never** a stream seq).
Off unless `uart.open{log:{…}}` or `uart.log.start`. Never deletes files; stops at the quota; never blocks the device path.

### Lease (bodies only; the lead edits none of these files meanwhile)

| file | Sonnet may change |
|---|---|
| `crates/loom-uart-worker/src/logsink.rs` (new) | every function / method body (§B4.1), incl. `Drop`; at most three private helpers (e.g. the writer loop). |
| `crates/loom-uart-worker/src/worker/util.rs` | bodies of `parse_log_cfg`, `log_json`. |
| `crates/loom-uart-worker/src/worker.rs` | bodies of `set_log_home`, `log_start`, `log_stop`, `call` (two arms), `open_inner`, `close`, `status`, `spawn_port` (one argument). |
| `crates/loom-uart-worker/src/worker/port.rs` | bodies of `Port::new` (init `log`), `Port::ingest` (`Data` arm), `Port::tx_loop` (`Chunk` arm). |

Lead-written (read-only): all types/consts/signatures/imports/docs (`LogSink`, `LogCfg`, `LogFormat`, `LogDir`, `LogRecord`,
`LogHeader`, `LogStats`, `LogStartError`, `LineAssembler` (+`last_prefix`), `LOG_*` incl. `LOG_FOOTER_RESERVE` 256,
`LOG_QUEUE_BYTES` 4 MiB, `LogSlot`, `Port.log`, `Port::new(…, log)`, `Inner.{log, log_home}`, `kind::LOG`, `LOG_KEYS`);
`lib.rs` `pub mod logsink`; catalog/schema (`uart.log.start`, `uart.log.stop` M1 `RoleSet::WRITE`; `uart.open.log`; event kind
`uart.log`); tests `tests/logsink.rs` (13), `tests/worker_log.rs` (5).
Scaffold: `cargo check -p loom-uart-worker` = 2 errors (`Port` literal lacks `log`; `Port::new` call lacks its argument —
pass `Arc::clone(&self.inner.log)`); the tests compile once filled (lead-verified, shim removed).

### §B4.1 `logsink.rs`

**Erratum E2 (2026-11-16, lead-authorized, worker adds it under the lease):** `impl Port` gains
`pub(crate) fn producer(&self) -> (String, u64)` (emitter's producer_id / producer_epoch) for the log header.

Pure:
- `utc_iso(ns)` → `YYYY-MM-DDTHH:MM:SS.mmmZ` (days-from-civil, no crate). `log_dir(home, ch)` → `home/uart/logs/ch`.
  `file_name(ch, started_ns, gen, n, fmt)` → `` `${ch}-${YYYYMMDD}T${HHMMSS}Z-g${gen}-${n:03}.${log|jsonl}` `` (UTC).
- `line_prefix(r)` → `` `${utc_iso(wall_ns)} g${gen} ${RX|TX} src=${producer_epoch}:${source_seq} off=${rx_offset|-} emit=${ok|FAILED} | ` ``.
- `LineAssembler::feed(r)` (one per direction): `secret` → flush own pending (as in `flush`) then `` prefix + `[secret ${len} B]\n` ``.
  Else decode `carry + bytes` as UTF-8: an incomplete sequence at the end is carried (≤ 3 bytes); invalid bytes → `\xNN`.
  Per char: `\n` ends the line → `pending_prefix + text + "\n"` (a `\n` with nothing pending → this record's prefix + `"\n"`); `\r`
  dropped; other controls (`< 0x20` except `\t`, and `0x7f`) → `\xNN` (lowercase hex); a pending line starts with the prefix of
  the record whose chunk produced its first char; at `LOG_LINE_MAX_CHARS` characters it is written with `" ⟨continued⟩\n"`.
  Remember `last_prefix`. `flush()` → pending (+ carried bytes as `\xNN`) → `prefix + text + " ⟨no newline⟩\n"`, or `""`.
- `jsonl_line(r)` → one JSON object + `\n`: `t` (utc_iso), `wall_ns`, `mono_ns`, `producer_epoch`, `source_seq`, `rx_offset` as
  decimal **strings** (`rx_offset` null for TX), `clock_id`, `generation`, `dir` (`rx|tx`), `origin`, `request_id`, `client_id`
  (null when absent), `producer_id`, `len`, `emit` (`ok|failed`), and `hex` — or, for `secret`, `"secret": {"len": n}` and no
  `hex`. No `seq`/`stream_seq` field.
- `header_text(h, cfg, n, started)`: `.log` four `# ` lines — `# Loom UART native log · channel <ch> · file <n> · started <utc>`;
  `# provenance: native-worker-log; stream_seq: not available (assigned by the daemon journal after publication); cross-reference
  via (producer_id, producer_epoch, source_seq)`; `# target <t> · producer_id <id> · producer_epoch <e> · generation <g> ·
  clock_id <c>`; `# format log · rotate <B> B · quota <B> B · columns: <utc> g<gen> <RX|TX> src=<epoch>:<source_seq>
  off=<rx_offset|-> emit=<ok|FAILED> | text (\r dropped, controls as \xNN)`. `.jsonl`: `{"type":"header","provenance":
  "native-worker-log","stream_seq":"not available","channel","file_index":n,"started":utc,"target","producer_id",
  "producer_epoch":string,"generation","clock_id","format","rotate_bytes","quota_bytes"}`.
- `gap_text(fmt, recs, bytes, ns)`: `.log` `` `# GAP ${utc} dropped ${recs} records / ${bytes} bytes (log queue full; the daemon
  journal may still have them)\n` ``; `.jsonl` `{"type":"gap","t","dropped_records","dropped_bytes"}`.
- `footer_text(fmt, reason, records, dr, db, ns)`: `.log` `` `# END ${utc} reason=${reason} records=${records}
  dropped_records=${dr} dropped_bytes=${db}\n` ``; `.jsonl` `{"type":"end","t","reason","records","dropped_records","dropped_bytes"}`.

Sink:
- `start(dir, header, cfg, started_ns)`: `create_dir_all`; `dir_bytes` = sum of the regular files in `dir`; `dir_bytes ≥ quota` or
  `dir_bytes + header + LOG_FOOTER_RESERVE > quota` → `Err(Quota{dir_bytes, quota_bytes})`; create file 1
  (`create_new`, never overwrite) with its header; stats `{active: true, dir, file, files: 1, bytes_written, dir_bytes}`; spawn
  thread `loom-uart-log` (spawn error → `Io`).
- `record(r)`: not active → ignore. Cost = `r.bytes.len()`; `queued + cost > LOG_QUEUE_BYTES` → `dropped_records += 1`,
  `dropped_bytes += cost`, add to `pending_gap`, return; else `queued += cost` and send. Never blocks.
- Writer thread: per record — `queued -= cost`; pending gap → write `gap_text` first (reset); text = `.log`:
  `assembler[dir].feed(r)` / `.jsonl`: `jsonl_line(r)`; write with the limits: if `file_bytes + len + LOG_FOOTER_RESERVE >
  rotate_bytes` and the file holds more than its header → start file n+1 (no footer on the rotated file; new header), and if
  `dir_bytes + len + LOG_FOOTER_RESERVE > quota_bytes` (also checked for a new header) → write the footer `quota`, close, stats
  `{active: false, stopped: "quota"}`, set `stop_notice`, ignore later records. `records += 1` per record written. Write
  errors → stop `io_error: <e>` (footer attempted, notice set). Flush to the OS when idle ≥ 200 ms (`recv_timeout`) and on stop.
- `stop(reason)`: send `Stop`; the writer drains what is queued, flushes both assemblers, writes the footer (if the quota
  allows), closes; join; return the final stats (`active: false`, `stopped: reason` unless it had stopped earlier).
  `take_stop_notice()` returns the self-stop reason once. `Drop` without `stop` → same as `stop("closed")`.
- `stats().bytes_written` = all bytes this sink wrote; `dir_bytes` = initial + written.

### §B4.2 `worker/util.rs`, `worker.rs`

- `parse_log_cfg(v, allow_other_keys)`: `v` must be an object (else `E_USAGE`); `!allow_other_keys` and a key outside
  `LOG_KEYS` → `E_USAGE`; `format` absent → `log`, `"log"|"jsonl"` else `E_USAGE`; `rotate_mib` absent → 64, else integer
  1–1024; `quota_mib` absent → 1024, else 1–8192 (else `E_USAGE`). Bytes = MiB × 1 048 576.
- `log_json(st, cfg)` → `{"active", "dir", "file", "format", "rotate_mib", "quota_mib", "files", "bytes_written", "dir_bytes",
  "dropped_records", "dropped_bytes", "stopped"}` (paths as strings, `null` when absent).
- `set_log_home(p)` stores it; the home used is it or `loom_worker_sdk::transport::loom_home()`.
- `call`: `"uart.log.start" => self.log_start(args, parse_log_cfg(args, true)?)`, `"uart.log.stop" => self.log_stop(args, "user")`.
- `log_start(args, cfg)`: `port_for(args)?`. Slot holds an active sink: same cfg → `{"channel", "log": log_json, "already":
  true}`; else `E_CONFIG` "already logging (<file>); stop it first". A stopped sink in the slot is replaced. Start
  `LogSink::start(log_dir(home, channel), header{channel, target, producer_id, producer_epoch, generation, clock_id}, cfg,
  wall_now)`: `Quota` → `E_LIMIT` (message from the error, detail `{dir_bytes, quota_bytes}`), `Io` → `E_INTERNAL` "log
  directory error: …". Emit `uart.log` `{"state": "started", "provenance": "native-worker-log", …log_json}` through the port;
  return `{"channel", "log": log_json}`.
- `log_stop(args, reason)`: `close` calls it before the port goes; otherwise `port_for(args)?`. No active sink → `{"channel",
  "stopped": false, "log": …}` (no event). Else `take` the sink, `stop(reason)`, emit `uart.log` `{"state": "stopped",
  "reason", …}`, return `{"channel", "stopped": true, "log": log_json(final)}`.
- `open_inner`: `let log = op.get("log").map(|v| parse_log_cfg(v, false)).transpose()?;` before the slot is locked (a bad value opens
  nothing). After the new port is in the slot and `log` is `Some`: `log_start` for it — on error the port stays open and the
  open result carries `"log_error": {code, message}` (opening must not fail because of the disk). Idempotent reopen ignores `log`.
- `close`: before shutting the port down, stop an active sink with reason `closed` (event emitted while the port can still emit).
- `status`: `"log": log_json(..)` (`{"active": false, …}` when none).
- A reconnect (B3) keeps the sink: `spawn_port` passes the same `LogSlot`; records carry the new generation.

### §B4.3 `worker/port.rs`

- `Port::new` stores `log`.
- `ingest` `Data` arm (RX), after the emission attempt: if a sink is in `log`, `record(LogRecord{dir: Rx, origin: "device",
  generation, rx_offset: Some(chunk rx_offset), request_id/client_id: None, producer_id/epoch from the emitter, source_seq = the
  reserved seq (or the chunk's source seq when reservation failed), mono/wall from the chunk, clock_id, bytes, secret: false,
  emit_ok: emission succeeded})`; then `take_stop_notice()` → emit one `uart.log` `{"state":"stopped","reason",…}`.
- `tx_loop` `Chunk` arm: the same for TX (`origin` from the wait/lane as for the `uart.data` row, `request_id`/`client_id`,
  `secret` from the chunk, `rx_offset: None`).
- Never hold the `log` lock while emitting; recording must not block (it does not: `record` only queues).

### Acceptance (from the worktree root)

```
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-uart-worker
cargo test -p loom-uart
cargo clippy -p loom-uart-worker -p loom-uart --all-targets
rustfmt --edition 2024 --check crates/loom-uart-worker/src/logsink.rs
```
Expected: all suites pass (B2/B3 suites unchanged), clippy no warnings. Tests write only under the OS temp dir. Baseline rustfmt
differences in other files are ROOT's at final integration.

### Return

`…/work/reports/operator-upgrade/uart/B4-WORKER.md` (files + sha256, helpers, deviations/questions, each command's exit status and tail).

---

## Batch 7 — CLI + help for U1-A/U2/U3/U6 — PLAN_READY_BATCH7

### Lease (bodies only)

| file | Sonnet may change |
|---|---|
| `crates/loom-uart/src/cli.rs` | bodies of `UartCli::command` and `UartCli::to_call` (new args/subcommands/arms below). |
| `crates/loom-uart/src/commands.rs` | body of `help_spec` (examples and notes). |

Lead-written (read-only): `skills/loom-uart/SKILL.md` section "Opt-in operator features"; tests `crates/loom-uart/tests/cli_b7.rs` (8).

### §B7.1 `command` / `to_call`

- `open`: `--char-delay MS` (u64 0..=1000), `--line-delay MS` (0..=10000) → `pacing: {char_delay_ms?, line_delay_ms?}` only when
  either is given; `--reconnect` (flag) → `reconnect: {enabled: true}` + `--reconnect-window S` (u64 1..=120, `requires
  reconnect`) → `window_ms: S·1000`; `--log [FORMAT]` (`num_args(0..=1)`, values `log|jsonl`, `default_missing_value("log")`)
  → `log: {format}` + `--log-rotate MIB` (1..=1024, `requires log`) → `rotate_mib`, `--log-quota MIB` (1..=8192, `requires log`)
  → `quota_mib`. None of these keys is sent unless its flag is given.
- `send` and `run`: `--char-delay MS`, `--line-delay MS` (same bounds) → `char_delay_ms` / `line_delay_ms` (each only when given).
- `pacing <channel> [--char MS] [--line MS]` → `uart.pacing {channel, char_delay_ms?, line_delay_ms?}`.
- `signal <channel> <break|dtr|rts> [--on | --off] [--ms N]` (`--on`/`--off` conflict; `--ms` u64 1..=5000) →
  `uart.signal {channel, kind, value?, duration_ms?}`: `--on` → `value: true`, `--off` → `value: false`; dtr/rts without either →
  `UsageError` "dtr/rts need --on or --off"; break with `--off` → `UsageError` "break is a pulse". `wait = 10 s`.
- `log <channel> <start|stop> [--format log|jsonl] [--rotate MIB] [--quota MIB]` → `uart.log.start {channel, format?,
  rotate_mib?, quota_mib?}` / `uart.log.stop {channel}` (bounds as above).
- `render_human`: unchanged (new methods use the existing pretty-JSON arm).

### §B7.2 `help_spec`

Add examples `loom uart pacing dut --line 50` ("slow line-by-line input for this channel"), `loom uart signal dut break --ms 250`
("send a 250 ms break"), `loom uart open /dev/ttyUSB0 --as dut --reconnect --log` ("reopen the same device after an unplug and
keep a native log file"), `loom uart log dut stop` ("stop the native log file"); notes: "Pacing, reconnect and native logs are
opt-in; a reconnect never resends anything." and "Native log files carry no stream seq (written before publication); use the
journal for seq-aligned evidence."

### Acceptance

```
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-uart
cargo clippy -p loom-uart --all-targets
```

---

## Batch 6 — operator UI for U1-A / U2 / U3 / U5b / U6 — PLAN_READY_BATCH6

### Lease (bodies only; the lead edits none of these files meanwhile)

| file | Sonnet may change |
|---|---|
| `ui/src/panels/uart/highlight.ts` (new) | bodies of `compileRules`, `matchLine`, `sanitizeRules`. |
| `ui/src/panels/uart/uartState.ts` | bodies of `portExtrasText`, `reducePort`, `reducePortState`. |
| `ui/src/workbench/uart/workbench.ts` | bodies of `openParams`, `sanitizeSession`, `UartBench.openSession`, `UartBench.applyPacing`. |
| `ui/src/workbench/uart/UartWorkbench.tsx` | the `SessionDialog` component body (new fieldset; state wiring). |
| `ui/src/panels/uart/terminalPanel.ts` | bodies of `applyHighlights`, `writeChunk` (one call), `draw` (one status part), `fillRecDialog` (native-log section), the palette button callback, `dispose` (dispose decorations), and (re-freeze R1, 2026-11-16) `applyPort`: replace its first kind check with `if (!PORT_KINDS.includes(ev.kind)) return false;` and (re-freeze R2) right after `port = reducePort(port, ev);` add `if (ev.kind === 'uart.log' && recDialog?.open) fillRecDialog();` — everything else in it unchanged. |

Lead-written (read-only): `highlight.ts` types/constants (`HIGHLIGHT_*`, `HIGHLIGHT_CSS`); `UartPortState.{pacing, reconnect, log,
lines}`, `PORT_KINDS`; `Session` optional fields + `SESSION_*` bounds, `PACING_SPEC`, exported `sanitizeSession`; panel `PALETTE`
entries (DTR/RTS pulses), actions `signalLine` (confirm `card`), `logStart`, `logStop`, imports, `hlRules`/`hlKey`/`hlDecos`;
dialog imports. Tests: `highlight.test.ts` (6), `uartState.b6.test.ts` (6), `workbench.b6.test.ts` (4), `batch6.panel.test.ts` (4).
Scaffold: `tsc` clean; eslint reports only the unused scaffold names your bodies consume; the 20 new tests fail until bodies exist.

**Re-freeze R1 (2026-11-16):** the lead added `PORT_KINDS` to the `./uartState` import of `terminalPanel.ts` (static edit, lease
returned and resumed) and added `applyPort` to the allowlist above, so `uart.log` / `uart.signal` reach `reducePort`.

**Errata E3/E4 — REVOKED (ROOT, 2026-11-16).** Static lines (imports, types, constants, signatures, manifests) are lead-only, also as
errata. Process for a missing import: the worker returns that file's lease, the lead edits the static line, re-freezes and sends
"B6 RESUME" with the new hash. Deviation on record: the worker had already applied E3 (`workbench.ts` line 16 →
`import { sanitizeRules, type HighlightRule } from '../../panels/uart/highlight';`); kept, reviewed as a static delta in B6-REVIEW.md.

### §B6.1 `highlight.ts`
- `compileRules(rules)`: for each rule in order, skip empty patterns; literal → escape `[.*+?^${}()|[\]\\]`; flags `g` + (`i` unless
  `caseSensitive`); a `RegExp` error → `errors.push({index, message})` (never throws); stop after `HIGHLIGHT_MAX_RULES` compiled.
- `matchLine(text, compiled)`: walk positions left to right; at position `p` try every rule (in order) with `lastIndex = p` and keep
  the earliest match start among rules, ties → lower rule index; skip zero-length matches (advance by 1); emit `{start, end,
  color}`, continue at `end`; stop at `HIGHLIGHT_MAX_PER_LINE`. Reset `lastIndex` so results never depend on earlier calls.
- `sanitizeRules(v)`: array only; keep objects with string `pattern`, `color` in `HIGHLIGHT_COLORS`; `regex`/`caseSensitive` →
  `=== true`; pattern sliced to `HIGHLIGHT_MAX_PATTERN`; at most `HIGHLIGHT_MAX_RULES`.

### §B6.2 `uartState.ts`
- `reducePort(st, ev)`: kinds outside `PORT_KINDS` → `st`. `uart.state`/`uart.config` → `reducePortState` (today's rules) plus:
  config `payload.pacing` `{char_delay_ms, line_delay_ms}` (numbers) → `pacing {charMs, lineMs}`; state `payload.reconnect` object →
  `reconnect {enabled, windowMs: window_ms, active, attempts, reason: payload.reason ?? null}`; state `open` (or `closed`) with an
  existing `reconnect` → `{...reconnect, active: false, reason: null}`. `uart.log` → only `log {active: state === 'started', format,
  file, stopped: state === 'stopped' ? (reason ?? null) : null}` (status untouched). `uart.signal` with `result === 'ok'` and kind
  `dtr`/`rts` → `lines` (create `{dtr: null, rts: null}`) level = `duration_ms` present ? `!value` : `value`; other cases → `st`.
- `portExtrasText(p)`: parts joined `' · '`, null when none: pacing with any delay > 0 → `` `TX 지연 문자 ${charMs} ms · 줄 ${lineMs} ms` ``;
  reconnect active → `` `재연결 중 (시도 ${attempts}, 최대 ${windowMs / 1000} s)` ``, else with a reason → `재연결 중단: ` + (`gave_up`
  → `시간 초과`, `identity_mismatch` → `다른 장치`, `stopped` → `STOP`, else the reason); log active → `` `로그 기록 중 (.${format})` ``,
  stopped with a reason other than `user`/`closed` → `` `로그 중지: ${stopped}` ``; lines with any non-null →
  `` `DTR ${on|off|?} · RTS ${on|off|?}` ``.

### §B6.3 `workbench.ts`
- `openParams(s)` → `{target: s.port, alias: s.alias, config: <today's mapping in openSession>}` + `pacing: {char_delay_ms,
  line_delay_ms}` when either delay > 0; `reconnect: {enabled: true, window_ms: reconnectWindowS·1000}` when `reconnect`;
  `log: {format: s.log}` when `log` is `log`/`jsonl`. `openSession` keeps its checks and the B8 note and calls
  `sv.gate.run(OPEN_SPEC, openParams(s))`.
- `sanitizeSession`: today's fields unchanged; plus `charDelayMs`/`lineDelayMs` = integer clamped to `0..SESSION_*_MAX` (non-number →
  0); `reconnect = x.reconnect === true`; `reconnectWindowS` clamped 1..120 (absent/NaN → 10); `log` ∈ {off, log, jsonl} else
  `off`; `scrollback` clamped 1000..50 000 (absent/NaN → 10 000); `highlights = sanitizeRules(x.highlights)`.
- `applyPacing(ch, c, l)` → `this.need().gate.run(PACING_SPEC, {channel: ch, char_delay_ms: c, line_delay_ms: l})`.

### §B6.4 UI
- `UartWorkbench.tsx` `SessionDialog`: after the frame fieldset add `<fieldset data-role="ops"><legend>운영 옵션</legend>` with:
  number inputs `name="charDelayMs"` (0–1000, label "문자 지연 ms"), `name="lineDelayMs"` (0–10 000, "줄 지연 ms"); checkbox
  `name="reconnect"` ("분리되면 같은 장치를 다시 연결 (새 세대)") + number `name="reconnectWindowS"` (1–120, disabled unless
  reconnect, "최대 s"); select `name="log"` (`off` 끔 · `log` .log · `jsonl` .jsonl, label "열 때 네이티브 로그"); number
  `name="scrollback"` (1000–50 000 step 1000, "스크롤백 줄"); highlight rows (≤ 16): text `name="hlPattern"`, checkbox `.*`
  (`name="hlRegex"`), checkbox `Aa` (`name="hlCase"`), select colour (`HIGHLIGHT_COLORS`), remove button `data-act="hl-del"`; add
  button `data-act="hl-add"` disabled at 16; `compileRules` errors listed in `<div data-role="hlerr">` (React text only). When the
  channel is open (`openNow`) show button `data-act="pacing-apply"` "지금 적용" → `uartBench.applyPacing(alias, c, l)` then toast
  "전송 지연 적용됨" (error → toast with message) and the hint "재연결·로그 설정은 다음 열기부터 적용". Saved via the existing
  submit path (sanitizeSession clamps).
- `terminalPanel.ts`:
  - palette callback: `k.signal` → `act(k.line ? 'signalLine' : 'signal', {channel, kind: k.signal, ...(k.value !== undefined ? {value: k.value} : {}), ...(k.ms ? {duration_ms: k.ms} : {})})`.
  - `draw`: after the `rx` rate part, `const ex2 = portExtrasText(port); if (ex2) parts.push(ex2);`.
  - `fillRecDialog`: after the journal section add `<fieldset data-role="nlog"><legend>네이티브 로그 파일 (데몬 쪽, 계속 기록)</legend>`
    with hint `` `$LOOM_HOME/uart/logs/${channel}/ — 열기 옵션 또는 여기서 시작 · 용량 한도에서 멈춤(삭제 없음) · 파일에 stream seq 없음` ``,
    `<div class="hint" data-role="nlogstat">` (textContent: active → `` `기록 중 · ${file}` ``, stopped → `` `중지됨 (${stopped ?? '-'})` ``,
    none → `꺼짐`), and button `port.log?.active ? data-act="nlog-stop" "로그 중지" : data-act="nlog-start" "로그 시작"`;
    handlers → `act('logStart', {channel, format: fmt()})` / `act('logStop', {channel})`, errors → `ctx.ui.toast`.
    `channel` is a validated alias; device text only via `textContent`.
  - `applyHighlights(fromRow)`: session = `uartBench.sessionFor(channel)`; key = JSON of its `highlights` + `scrollback`; on change
    recompile (`hlRules`), dispose all `hlDecos`, set `term.term.options.scrollback = session?.scrollback ?? 10_000`, set
    `wrap.dataset.hlRules = String(hlRules.length)` and `wrap.dataset.hlErrors = String(errors.length)`. With rules: for each
    buffer line from `fromRow` to the end, `matchLine(line.translateToString(true), hlRules)` and for each span
    `term.term.registerMarker(row - cursorAbsRow)` + `registerDecoration({marker, x: start, width: end - start,
    backgroundColor: HIGHLIGHT_CSS[color]})`; push to `hlDecos`, dispose oldest beyond `HIGHLIGHT_MAX_DECORATIONS`. Every xterm
    call in try/catch — failures never affect writing. Wrapped logical lines are matched per buffer row (stated limitation).
  - `writeChunk`: after the write completes (`term.term.write('', cb)` as today) call `applyHighlights(<first row written>)`.
  - `dispose`: dispose `hlDecos`.

### Acceptance (from `ui/`; targeted, `--maxWorkers=2`)

```
npx vitest run --maxWorkers=2 src/panels/uart src/workbench/uart
npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/panels/uart src/workbench/uart
```

### Return
`…/B6-WORKER.md` (files + sha256, deviations/questions, exit status and tails).


---

## Batch 9 — correction F1 (pacing vs caller window) + F2 (ENOTTY → E_UNSUPPORTED) — PLAN_READY_BATCH9

Source: PRE_FINAL7 PTY run (SIM-VALIDATION.md). F1: `w11` (200 B, `--char-delay 100 --timeout 5 --defer-timeout 1`) was admitted and
paced to completion (`sent 200`) after the CLI had stopped waiting at 8 s with `outcome_unknown`. The worker compared the estimate
only with the RPC deadline it received (far away), not with the caller's own window. **w11's outcome is known (`sent 200`); it is
never retried.** F2: DTR/RTS on a PTY failed with ENOTTY ("Not a typewriter") and were reported as `E_DISCONNECTED`. Base: ROOT's
imported files (the worktree was synced to them; the only difference was ROOT's mechanical rustfmt). No shared core, CLI or
deadline-API change.

### Lease (bodies only; the lead edits none of these files meanwhile)
| file | Sonnet may change |
|---|---|
| `crates/loom-uart-worker/src/worker/send.rs` | bodies of `op_window`, `effective_budget`, `agent_write`. |
| `crates/loom-uart-worker/src/transport/serial.rs` | body of `line_control_error` and, **in the unix `mod imp` only**, the `map_err` of `set_dtr`, `set_rts`, `set_break` (use `line_control_error`). |

Lead-written: the two signatures in `send.rs`, `line_control_error`'s signature and doc, the test `tests/b9_regression.rs` (5
slow-transport tests + 3 real-PTY tests on macOS), the SKILL note. Scaffold: 3 unused-function warnings only.

### §B9.1 F1 — semantics (paced requests only; an unpaced request behaves exactly as today)
- `defer_timeout_ms` = longest wait for admission; `timeout_ms` = the caller's wait for the rest of the operation. The caller (the
  CLI waits `timeout + defer + 2 s`) stops waiting after **window = defer + timeout**. Both values are already clamped to
  `MAX_WAIT_MS` by `parse_send`; `op_window` = `Duration::from_millis(timeout_ms.saturating_add(defer_timeout_ms))`.
- `effective_budget(rpc_remaining, window, elapsed)` = `rpc_remaining.min(window.saturating_sub(elapsed))`.
- `agent_write`: take `let started = Instant::now();` as its first statement. When `op.pacing` is not none:
  1. **Before admission** (where today's check is): `budget = effective_budget(remaining(cx, w.deadline_mono_ns.get()),
     window, started.elapsed())`; `estimate > budget` → `rows.fail(Failed, werr_detail(E_USAGE, "pacing needs {e} ms but the request
     leaves {b} ms (caller window {w} ms = defer + timeout); raise --timeout or lower the pacing", {"estimate_ms", "budget_ms",
     "window_ms", "rpc_remaining_ms"}), {})`. No admission, no byte.
  2. **After admission, before submit**: compute the budget again (the wait for the user used part of the window); `estimate >
     budget` → release the lease (as the other pre-TX failures do) and fail the same way, plus `"after_admission": true`. No byte.
  3. **TX deadline** = `Instant::now().checked_add(budget)` (None → `Instant::now() + Duration::from_millis(MAX_WAIT_MS)`), with this
     budget instead of the RPC budget; wait for `TxDone` with `budget + INTERNAL_GRACE`. The TX actor already stops at the deadline
     between chunks (`Timeout`, `OutcomeUnknown` with exact `written_bytes`, no later write); STOP/HOLD/close/cancel stay as B2.
  4. **Expect wait** (send `--expect` / run) = `min(timeout_ms, effective_budget(remaining(cx, deadline), window, started.elapsed()))`.
- Unpaced requests: no change at all (TX deadline = RPC budget, expect wait = `timeout_ms`).

### §B9.2 F2 — `line_control_error(e)`
`let io = io::Error::from(e);` if `io.kind() == Unsupported` → `io`; else if its message contains `Errno::ENOTTY.desc()` (or
`Errno::ENOTSUP.desc()` / `Errno::EOPNOTSUPP.desc()`) → `io::Error::new(io::ErrorKind::Unsupported, format!("modem control not
supported by this device ({io})"))`; else `io` unchanged (a real unplug, EIO/ENXIO, stays a disconnect). `signal.rs`'s existing
mapping then reports `E_UNSUPPORTED`; the event stays `failed` and failed levels are not recorded (unchanged).

### Acceptance (from the worktree root)
```
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-uart-worker
cargo test -p loom-uart
cargo clippy -p loom-uart-worker -p loom-uart --all-targets
```
Expected: all suites pass incl. `b9_regression` (8 on macOS) and every B2/B3 suite unchanged; clippy no warnings. Formatting:
ROOT's mechanical rustfmt applies at integration — keep your edits formatted.

### Return
`…/B9-WORKER.md` (files + sha256, deviations/questions, exit status and tails).
