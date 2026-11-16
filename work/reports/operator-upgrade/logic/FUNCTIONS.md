# Logic — FUNCTIONS (contracts)

**B1: ACCEPTED** (B1-REVIEW.md). **B2a: ACCEPTED** (B2A-REVIEW.md). **B2b + B2a.1: ACCEPTED** (B2B-REVIEW.md, incl. measured load). **B3 (a+b): ACCEPTED** (B3-REVIEW.md). **B4r: ACCEPTED** (B4R-REVIEW.md). **B5r: ACCEPTED** (B5R-REVIEW.md). **B45u: PLAN_READY_BATCH45U**. B2b and B3–B5 are listed at the end and get full contracts before their lease.
Worktree root: `/Users/ctmctm/workspace/Loom/.worktrees/operator-logic`. Paths below are relative to it.

## B1 lease and allowlist
**Exclusive Sonnet lease (function bodies only):** `ui/src/panels/logic/logicModel.ts`, `ui/src/panels/logic/logicPanel.ts`, `ui/src/panels/logic/waveform.ts`.

**Opus-owned, Sonnet must not edit:**
- `logicBatch1.test.ts`, `logic.test.ts`, `logicModule.test.ts`
- `logic.css`
- `crates/**`
- every exported type, interface, constant, signature and top-level import
- the option tables `UART_LINE_OPTS … ADV_SUMMARY` in logicPanel.ts
- `registerPanel(...)`

If an extra helper, import or constant is needed, stop and return the lease with the exact reason. Private inner closures inside an allowlisted body are fine.

| ID | File · function (body only) | Kind |
|---|---|---|
| B1-M1 | logicModel.ts · `sanitizeConfig(saved: unknown): StartConfig` | new body |
| B1-M2a | logicModel.ts · `decoderSpec(c: StartConfig): string \| null` | rewrite body |
| B1-M2b | logicModel.ts · `validateConfig(c: StartConfig): string[]` | extend body |
| B1-M3 | logicModel.ts · `estimatePublishLoad(c, shown = CAPTURE_CHANNELS): LoadEstimate` | new body |
| B1-M4 | logicModel.ts · `decodeCoverage(c: CaptureData): DecodeCoverage` | new body |
| B1-M5 | logicModel.ts · `rawCoverage(c: CaptureData): RawCoverage` | new body |
| B1-M6 | logicModel.ts · `followHeadSample(c: CaptureData): number` | new body |
| B1-M7 | logicModel.ts · `coverageNote(d, r): string` | new body |
| B1-M8 | logicModel.ts · `annCoverageText(d): string` | new body |
| B1-W1 | waveform.ts · `paint(...)`, only the axis-header statement | edit body |
| B1-P1 | logicPanel.ts · module fn `loadConfig(): StartConfig` | rewrite body |
| B1-P2 | logicPanel.ts · `createLogicAnalyzer`, function-local DOM construction of the config section | edit body |
| B1-P3 | logicPanel.ts · closures `writeForm`, `readForm` | edit bodies |
| B1-P4 | logicPanel.ts · `cfgSum` click listener (anonymous) | edit body |
| B1-P5 | logicPanel.ts · `draw` (config, counters/notice, view, ann-info sections) | edit body |

`cliLine`, `startParams` and `effectiveRate` are unchanged; they already use `decoderSpec`.

Number formatting everywhere below: `fmtN(n) = Math.round(n).toLocaleString('ko-KR')`, written inline in the body. The module has no shared helper and adding one is not allowed.

---
### B1-M1 `sanitizeConfig(saved: unknown): StartConfig`
- **Input:** any value from localStorage (old schema, junk, null).
- **Output:** a **new** object with exactly the keys of `DEFAULT_CONFIG`, never the `DEFAULT_CONFIG` reference.
- **Algorithm:** `out = {...DEFAULT_CONFIG}`. If `saved` is not a plain object (`typeof === 'object'`, not null, not Array), return `out`. For each key `k` of `DEFAULT_CONFIG`, with `v = saved[k]`:
  1. `v === undefined` → keep the default.
  2. `NULLABLE_CONFIG_KEYS.includes(k) && v === null` → `null`.
  3. `CONFIG_ENUMS[k]` exists → accept only if `CONFIG_ENUMS[k].includes(v)` (strict, so `2` ≠ `'2'`).
  4. Otherwise accept if `typeof v === typeof DEFAULT_CONFIG[k]`. For numbers also require `Number.isFinite(v)`. For a nullable pin key whose default is null (`spiMiso`), accept a finite number.
  5. Anything else → default.

  Keys not in `DEFAULT_CONFIG` are ignored. There is no range clamping here; `validateConfig` reports ranges.
- **Side effects:** none.
- **Tests:** `logicBatch1.test.ts` "sanitizeConfig…" and the L2 persistence panel test.

### B1-M2a `decoderSpec(c)`
- **Output:** `null` for `decoder==='none'`. Only options that differ from the backend default are emitted, in this fixed order:
  - **uart:** `uart:baudrate=${uartBaud}:` + (`rx=D${uartRx}` if `uartLine==='rx'`, else `tx=D${uartTx}`), then
    - `:data_bits=N` if ≠ 8
    - `:parity=P` if ≠ `'none'`
    - `:stop_bits=S` if ≠ `'1'`
    - `:bit_order=msb-first` if `uartBitOrder==='msb-first'`
  - **spi:** `spi:clk=D${spiClk}`, then
    - `:mosi=D${m}` if not null
    - `:miso=D${m}` if not null
    - `:cs=D${c}` if not null
    - `:cpol=1` if 1
    - `:cpha=1` if 1
    - `:wordsize=N` if ≠ 8
    - `:bitorder=lsb-first` if lsb
    - `:cs_polarity=active-high` if high
    - `:annotate=miso` iff `spiAnnotate==='miso' && spiMosi !== null`. With `spiMosi === null` the backend already annotates MISO, so nothing is emitted.
- **Golden examples:** every case in `crates/loom-logic/tests/fixtures/ui-decoder-specs.json` (Rust-verified: `cargo test -p loom-logic --test ui_decoder_specs` is green). The default specs `uart:baudrate=115200:rx=D0` and `spi:clk=D0:mosi=D1:cs=D2` are unchanged, and the old `logic.test.ts` depends on them.

### B1-M2b `validateConfig(c)` (keep every existing message and its order; append the new ones in this order)
1. Replace the UART channel check. If `uartLine==='rx'`, check `uartRx` with the existing message `'UART RX 채널이 캡처 채널 밖입니다'`. Otherwise check `uartTx` with `'UART TX 채널이 캡처 채널 밖입니다'`.
2. UART: `!(Number.isInteger(uartDataBits) && 5 ≤ x ≤ 9)` → `'UART 데이터 비트는 5–9 (정수)'`.
3. Synthetic + UART with `uartDataBits!==8 || uartParity!=='none' || uartStopBits!=='1' || uartBitOrder!=='lsb-first'` → `'합성 UART는 8N1 LSB-first만 생성합니다 (실제 장비/데모에서는 사용 가능)'`.
4. SPI: the pins list filters nulls (now including `spiMosi`). Existing range and overlap checks stay.
5. SPI: `spiMosi===null && spiMiso===null` → `'SPI는 MOSI 또는 MISO가 필요합니다'`.
6. SPI: `spiAnnotate==='miso' && spiMiso===null` → `'MISO 표시를 고르려면 MISO 핀이 필요합니다'`.
7. SPI: `spiAnnotate==='mosi' && spiMosi===null && spiMiso!==null` → `'MOSI 표시를 고르려면 MOSI 핀이 필요합니다'`. Rule 5 covers the both-null case.
8. SPI: `!(Number.isInteger(spiWordsize) && 1 ≤ x ≤ 32)` → `'SPI 워드 크기는 1–32 (정수)'`.
9. Synthetic + SPI with `spiWordsize !== 8` → `'합성 SPI는 8-bit 워드만 생성합니다 (실제 장비/데모에서는 사용 가능)'`.

Tests: the "refuses incomplete…" and "synthetic refuses…" cases. All old validation tests must stay green.

### B1-M3 `estimatePublishLoad(c, shown)`
`shown` is the display channel names (`'D0'`…). A line counts as shown iff `shown.includes('D'+bit)`.
- **Bytes per synthetic frame:** `B = new TextEncoder().encode(unescapeText(c.text).replaceAll('{n}', '0000')).length`, `periodS = c.framePeriodMs / 1000`, `rate = effectiveRate(c)`.
- **Rows:**

  | case | annotationsPerS | basis |
  |---|---|---|
  | decoder `none` | 0 | `none` |
  | synthetic UART | `B / max(periodS, (2 + 10·B) / uartBaud)` | `synthetic` |
  | synthetic SPI | `clk = floor(rate/10)`, `h = floor(rate/clk/2)`, `frameS = (8h + 16h·B)/rate`; rows = `B / max(periodS, frameS)` | `synthetic` |
  | demo/device UART | `uartBaud / (1 + uartDataBits + (parity≠none ? 1 : 0) + Number(uartStopBits))` | `worst_case` |
  | demo/device SPI | null | `unknown` |
- **Edges** (upper bounds, only lines in `shown`):

  | case | edgesPerS | basis |
  |---|---|---|
  | synthetic UART | `rows·10` if RX/TX line shown, else 0 | `synthetic` |
  | synthetic SPI | `framesPerS = 1/max(periodS, frameS)`; sum of: CLK shown `16·rows` + MOSI shown `8·rows` + MISO shown `8·rows` + CS shown `2·framesPerS` | `synthetic` |
  | demo/device UART | `rows · frameBits` if line shown, else 0 | `worst_case` |
  | demo/device SPI or decoder none | null | `unknown` |

  Synthetic with decoder none is unreachable (validate blocks it); return edges 0, basis `none`.
- **Warnings** (exact strings, this order):
  1. `annotationsPerS !== null && > PUBLISH_ANNOTATIONS_PER_S` → `` `예상 디코드 ${fmtN(a)}행/s${worst?' (연속 송신 시 최대)':''} > live 게시 상한 2,000행/s — 초과분은 live 미리보기·저널에 실리지 않고 '디코드 손실'로 셉니다` ``
  2. `edgesPerS !== null && > PUBLISH_EDGES_PER_S` → `` `예상 전이 ${fmtN(e)}개/s${worst?' (최대)':''} > live 게시 상한 10,240개/s — 초과 구간은 파형에 '데이터 없음'(빗금)으로 표시됩니다` ``
  3. `edgeBasis==='unknown' && c.decoder==='spi'` → `'SPI 클록을 알 수 없어 게시량을 추정하지 못했습니다 — 1 MHz SPI는 전이 약 2,000,000개/s로 live 게시 상한을 크게 넘습니다'`; else if `edgeBasis==='unknown' && c.source==='sigrok-device'` → `'실제 신호 밀도를 알 수 없어 전이 게시량을 추정하지 못했습니다 — 상한 10,240개/s 초과분은 빗금으로 표시됩니다'`.

  `worst` = the respective basis is `worst_case`. The budget numbers in the text come from the constants formatted with `fmtN` (they render as `2,000` / `10,240`).
- **Examples:**
  - default config, shown D0 → 110 rows/s, 1,100 edges/s, no warning.
  - 256 B / 25 ms / 115200 → 10,240 rows/s and 102,400 edges/s, 2 warnings.
  - device 115200 8N1 → 11,520 rows/s (worst case).

### B1-M4 `decodeCoverage(c)`
`k = c.info?.counters ?? null`, `st = c.info?.state ?? 'unknown'`.
- `published = k ? k.annotations : null`
- `engineDropped = max(k?.droppedAnnotations ?? 0, c.droppedAnnotations)`
- `held = c.annotations.length + c.rawLinesTotal + c.rejectedAnnotations`
- `evicted = c.evictedAnnotations`
- `notReceived = published === null ? 0 : max(0, published − held − evicted)`
- `state`:
  - `!k || st==='unknown'` → `unknown`
  - else `st ∈ {starting, running, stopping}` → `live`
  - else (stopped/failed) → `complete` iff `notReceived===0 && engineDropped===0 && evicted===0`, otherwise `partial`
- Pure; no allocation beyond the result.

### B1-M5 `rawCoverage(c)`
- `engineSamples = k ? k.samples : null`
- `heldStart/heldEnd = c.range?.start/end ?? null`
- `evicted = c.evictedTo`
- `notReceivedBefore = heldStart === null ? 0 : max(0, heldStart − c.evictedTo)`
- `notReceivedAfter`: terminal and `engineSamples !== null` → `max(0, engineSamples − (heldEnd ?? 0))`, else 0
- `unknownInside`: clip every `c.unknown` range to `[heldStart, heldEnd)`, sort by start, merge overlaps, sum the lengths. 0 when nothing is held.
- `state`:
  - unknown / live as in M4
  - terminal → `complete` iff `notReceivedBefore===0 && notReceivedAfter===0 && unknownInside===0 && evicted===0`, otherwise `partial`

  This covers both "nothing held + engineSamples 0" → complete and "nothing held + engineSamples > 0" → partial.

### B1-M6 `followHeadSample(c)`
`c.chunks.length ? c.range!.end : c.headSample`.

### B1-M7 `coverageNote(d, r)`
1. `d.state==='unknown' || r.state==='unknown'` → `' — 엔진 카운터 미수신: 이 패널 보유분이 전체인지 알 수 없습니다'`
2. else either is `live` → `''`
3. else both `complete` → `' — 이 패널이 엔진 게시분 전체를 보유합니다'`
4. else → `' — 이 패널은 일부만 보유합니다: ' + parts.join(' · ')`. Parts are included only when > 0, in this order:
   1. `디코드 ${fmtN(held)}/${fmtN(published)}행 (미수신 ${fmtN(notReceived)})`
   2. `게시 손실 ${fmtN(engineDropped)}행`
   3. `디코드 보관 밖 ${fmtN(d.evicted)}행`
   4. `원시 앞부분 sample ${fmtN(notReceivedBefore)} 미수신`
   5. `원시 뒷부분 sample ${fmtN(notReceivedAfter)} 미수신`
   6. `원시 데이터 없음 sample ${fmtN(unknownInside)}`
   7. `원시 보관 밖 sample ${fmtN(r.evicted)}`

The exact full string is tested.

### B1-M8 `annCoverageText(d)`
`d.published !== null && d.notReceived > 0` → `` ` · 패널 보유 ${fmtN(d.held)} / 엔진 게시 ${fmtN(d.published)} — ${fmtN(d.notReceived)}행 미수신` ``; otherwise `''`.

### B1-W1 `paint` axis header (waveform.ts)
Replace only the statement `ctx.fillText(inp.rate ? '시간(sample clock)' : '샘플', 6, AXIS_H / 2)` with this sequence:
```
ctx.save()
ctx.beginPath()
ctx.rect(0, 0, GUTTER - 4, AXIS_H)
ctx.clip()
ctx.fillText(inp.rate ? AXIS_HEADER.time : AXIS_HEADER.sample, 6, AXIS_H / 2)
ctx.restore()
```
Keep `fillStyle = pal.muted` before it. Nothing else in `paint` changes.

### B1-P1 `loadConfig()`
`return sanitizeConfig(loadJson(CFG_KEY, isCfg));`. This removes the per-key merge loop; `sanitizeConfig` now owns the migration.

### B1-P2 config DOM (inside `createLogicAnalyzer`, next to the existing `fSource … fCs` construction)
Use the existing local helpers `sel`, `inp`, `field`, `pinOpts`, `el`.
- **Change:** `fMosi = sel('mosi', pinOpts(true))` (adds `''`/'없음').
- **New main-grid controls:**
  - `fLine = sel('uart_line', UART_LINE_OPTS)` → `lLine = field('UART 라인', fLine)`
  - `fTx = sel('uart_tx', pinOpts())` → `lTx = field('TX', fTx)`

  `cfgGrid.append` order: lSource, lDriver, lConn, lPattern, lText, lPeriod, lRate, lDuration, lDecoder, lBaud, **lLine**, lRx, **lTx**, lClk, lMosi, lMiso, lCs.
- **Advanced block:** `advBox = el('details', 'lg-adv')`, `advBox.setAttribute('data-testid', 'logic-decoder-advanced')`, `advSum = el('summary', undefined, ADV_SUMMARY)`, `advGrid = el('div', 'lg-cfg-grid')`. Controls:
  - `fDataBits = sel('uart_data_bits', UART_DATA_BITS_OPTS)` → `field('데이터 비트', …)`
  - `fParity = sel('uart_parity', UART_PARITY_OPTS)` → `'패리티'`
  - `fStop = sel('uart_stop_bits', UART_STOP_OPTS)` → `'정지 비트'`
  - `fUartOrder = sel('uart_bit_order', UART_ORDER_OPTS)` → `'비트 순서'`
  - `fSpiMode = sel('spi_mode', SPI_MODE_OPTS)` → `'SPI 모드'`
  - `fWordsize = inp('spi_wordsize', 'number')` with `min='1' max='32' step='1'` → `'워드 (bit)'`
  - `fSpiOrder = sel('spi_bit_order', SPI_ORDER_OPTS)` → `'비트 순서'`
  - `fCsPol = sel('spi_cs_polarity', SPI_CS_POL_OPTS)` → `'CS 극성'`
  - `fAnnotate = sel('spi_annotate', SPI_ANNOTATE_OPTS)` → `'디코드 표시 라인'`

  Then `advGrid.append(...labels)` and `advBox.append(advSum, advGrid)`.
- **Load warnings:** `cfgWarnings = el('ul', 'lg-warnings')` with `data-testid="logic-config-warnings"` and `role="status"`.
- **Order changes:**
  - `cfgFoot.append(startBtn, cfgProblems, cfgWarnings)`
  - `cfgBox.append(cfgSum, acqNote, cfgGrid, advBox, deviceNote, cfgFoot, …rest unchanged)`
- `sel()` takes `[string,string,boolean?][]`. The typed tables are assignable, so no casts beyond what TS needs.

### B1-P3 `writeForm` / `readForm`
- **writeForm additions:**
  - `fLine.value = cfg.uartLine`
  - `fTx.value = String(cfg.uartTx)`
  - `fDataBits.value = String(cfg.uartDataBits)`
  - parity, stop and order are assigned directly
  - `fMosi.value = cfg.spiMosi === null ? '' : String(cfg.spiMosi)`
  - `fSpiMode.value = String(cfg.spiCpol * 2 + cfg.spiCpha)`
  - `fWordsize.value = String(cfg.spiWordsize)`
  - spi order, CS polarity and annotate are assigned directly
  - Then `if (advNonDefault()) advBox.open = true`. Never set it false here. `advNonDefault` is a local inner closure: true if any of the 9 advanced fields differs from `DEFAULT_CONFIG`.
- **readForm additions** inside the `cfg = {...}` literal:
  - `uartLine: fLine.value as UartLine`
  - `uartTx: Number(fTx.value)`
  - `uartDataBits: Number(fDataBits.value)`
  - `uartParity: fParity.value as UartParity`
  - `uartStopBits: fStop.value as UartStopBits`
  - `uartBitOrder: fUartOrder.value as BitOrder`
  - `spiMosi: pin(fMosi)`
  - `spiCpol: (Number(fSpiMode.value) >> 1) as 0 | 1`
  - `spiCpha: (Number(fSpiMode.value) & 1) as 0 | 1`
  - `spiWordsize: Number(fWordsize.value)`
  - `spiBitOrder: fSpiOrder.value as BitOrder`
  - `spiCsPolarity: fCsPol.value as CsPolarity`
  - `spiAnnotate: fAnnotate.value as SpiAnnotate`

  Save and invalidate as today. Events: the existing `cfgBox` `change`/`input` listeners already cover the new controls (they are descendants).

### B1-P4 `cfgSum` click listener
Add `cfgTouched = true;` as the first statement, then the existing engine-check logic unchanged. Declare the new local state next to `capPending`: `let cfgTouched = false; let cfgAutoCollapsed = false;`. Also declare `let followStopped: string | null = null;` next to `follow`.

### B1-P5 `draw` edits
1. **Auto-collapse (L8c),** right after `const c = current();`:
   ```
   if (!cfgAutoCollapsed && c) {
     cfgAutoCollapsed = true;
     if (!cfgTouched && !pinned) cfgBox.open = false;
   }
   ```
2. **Stop notice (L5):** in the `stopped`/`failed` note, replace the literal `' — 기록은 이 패널에 남아 있습니다'` with `coverageNote(decodeCoverage(c!), rawCoverage(c!))`. `c` is non-null when `info` is. Compute both coverages once per draw (`const dCov = c ? decodeCoverage(c) : null`, same for raw) and reuse them in step 5.
3. **Config section (L2):**
   - Visibility:
     - `lBaud.hidden = lLine.hidden = cfg.decoder !== 'uart'`
     - `lRx.hidden = cfg.decoder !== 'uart' || cfg.uartLine !== 'rx'`
     - `lTx.hidden = cfg.decoder !== 'uart' || cfg.uartLine !== 'tx'`
     - the 4 UART advanced labels are hidden unless uart
     - the 5 SPI advanced labels are hidden unless spi; keep the existing CLK/MOSI/MISO/CS rule
     - `advBox.hidden = cfg.decoder === 'none'`
   - Warnings: `cfgWarnings.replaceChildren(...estimatePublishLoad(cfg, shownChannels()).warnings.map((t) => el('li', undefined, t)))`.
   - Warnings never touch `startBtn.disabled` or `cfgProblems`.
4. **View (L8a + L8c):**
   - Replace `c.headSample` in the follow line with `followHeadSample(c)`.
   - Immediately after that line (still after `clampView()` ordering as today), stop follow at the stop transition:
     ```
     if (c && follow && spp && (st === 'stopped' || st === 'failed') && followStopped !== c.id) {
       followStopped = c.id;
       followUi(false);
     }
     ```
     `followUi`, not `setFollow`, so there is no extra invalidate loop. If the user re-enables follow on that stopped capture it stays on, because the key is already applied.
   - `fit()` and Home keep using `headSample` (unchanged).
5. **Ann info (L5):** `${fmtN(c.annotations.length)}행${annCoverageText(dCov!)}${evicted…}${drops…} · 디코더 …` (insert right after `행`).
- Nothing else in `draw` changes: counters keep `max(engine, held)`, and the existing `evictedTo` note stays.
- DOM text uses `textContent` / `el(…, text)` only (untrusted device text).

### B1 acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic/ui
npx vitest run src/panels/logic                       # expect 60/60 (31 existing + 29 B1)
mkdir -p ../work/typecheck
npx tsc -p tsconfig.app.json  --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo   # isolated cache (COMMON); never the shared node_modules/.tmp
npx tsc -p tsconfig.node.json --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/panels/logic                           # expect 0 problems (scaffold unused-import errors must disappear)
cd .. && CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo test -p loom-logic --test ui_decoder_specs
```
- No `throw new Error('B1-…')` may remain (`grep -n "Sonnet body pending" ui/src/panels/logic/*.ts` must be empty).
- Don't run the full UI suite or GUI.
- The `--tsBuildInfoFile ../work/typecheck/*` flags keep tsc caches per worktree (shared `node_modules/.tmp` must not be used; `tsc -b` is ROOT's final combined check only).
- Sonnet reports: changed functions, exact test output, anything it could not satisfy without a contract change (returned, not improvised).

### B1 regression examples (what the tests demonstrate)
- **L5:** stopped capture with engine `annotations: 5`, panel holds 3 → table says `패널 보유 3 / 엔진 게시 5 — 2행 미수신`, and the notice no longer claims "기록은 이 패널에 남아 있습니다". Missing counters → "알 수 없습니다", not complete. Raw arriving from sample 4,000 → `원시 앞부분 sample 4,000 미수신`.
- **L2:** an 8E1 device capture sends `uart:baudrate=115200:rx=D0:parity=even`. The same spec is in the CLI line, and Rust accepts the same golden list. Synthetic 7E2 is refused before the RPC, exactly as the backend would (`E_NOT_IMPLEMENTED`).
- **L1:** the ROOT check config shows 2 warnings and stays startable.
- **L8a:** the follow window ends at raw end 50 ms, not at decode end 61 ms.
- **L8c:** follow is off after stop; a reopened panel with a capture shows the config collapsed once.
- **L8b:** the header is drawn clipped to the gutter.

---
## B2a — archive core (`crates/loom-logic/src/archive.rs`) · PLAN_READY_BATCH2A
**Lease (exclusive, bodies only):** `crates/loom-logic/src/archive.rs`.

**Frozen, Sonnet must not edit:**
- `tests/archive.rs`
- all other crate files
- every pub/private item signature, struct/enum fields and constants in archive.rs

**Allowlist:** every body that currently is `unimplemented!("B2-… Sonnet body pending")` in archive.rs, IDs A1–A4, R1–R5, N1–N2, F1–F5, P1, S1–S6, Q1–Q4, E1–E3, H1–H9 (39 bodies). The doc comment on each item is part of the contract, and the rules below add to it. Closures inside a body are fine. Any other new fn: return the lease. Dependencies: std + serde_json only. No new crates, no unsafe, no `unwrap()` / `todo!` (workspace lints).

### Shared rules
- **Errors:** `ArchiveError { code, message }` with Usage (bad request), NotFound (no archive) or Internal (corrupt files / I/O on read). Never panic on file content.
- **JSON:** `serde_json::json!` / `to_string`, keys exactly as documented.
- **Strict params (Q1/E1):** unknown key → `Err("unknown parameter \"k\" (allowed: …)")`; wrong type → `Err("<k>: expected …")`. `capture_id` passes A4. `kind`: `edges|annotations`; `format`: `csv|txt`.

### A1–A4 (args)
- A1 key checks run in this order: `archive`, `archive_max_capture_mb`, `archive_max_total_mb`, `archive_max_age_h`, `archive_max_captures`. Message format: `"[logic] <key>: expected <type> <lo>..=<hi>"` (bool: `"[logic] archive: expected true or false"`).
- `ArchiveConfig` MiB/h values are converted with checked multiplication (`<< 20`, `* 3600`).
- A2/A3 wire keys are exactly the ones in the doc. A3 rejects a non-absolute `root` and non-integer limits.
- A4: the regex in the doc, written by hand (no regex crate).

### R1–R5 (codec)
- R1/R2 exactly per the doc comment. The `EncodedBlock.bytes` header is: start u64 LE, state u8, n u32 LE, payload_len u32 LE, then the payload (H1 varint + value).
- R2 must not reset `sample` (a second `finish` returns `(None, sample)`).
- R4: entries must be strictly increasing in both start_sample and offset; otherwise Internal "archive index corrupt".
- R5: every failure is Internal `"archive block corrupt at sample <start or 0>"`. A delta of 0 and a payload length or `n_records` mismatch are errors.

### N1–N2 (annotation lines)
- N1 builds the line with `serde_json::to_string` of a `Map` in insertion order `s, e, d, t` (serde_json `preserve_order` is not enabled, so write the line by hand: `{"s":` + num|null + `,"e":` + … + `,"t":` + `to_string(&text)` + `}\n`).
- N2 parses with serde_json, requires the 4 keys, and builds `Annotation { decoder, start, end, text, lost_before: 0 }`. Then `to_row(samplerate)` + `row` + `source:"archive"`.

### F1–F5 (manifest)
- F1/F2 nested exactly: `{"format","capture_id","source","samplerate","decoders","state","started_wall_ns","updated_wall_ns","max_capture_bytes","raw":{"first_sample","end_sample","acquired_samples","bytes","blocks","truncated_at_sample","error"},"annotations":{"rows","bytes","truncated_at_row","error"}}`.
- F3: write `manifest.json.tmp`, flush, then `rename`.
- F5: per the doc.

### P1 (prune)
- **Candidates:** direct subdirs of `cfg.root` that are real directories (`symlink_metadata`), pass A4 and whose manifest F4 parses. A missing root is not an error (empty report).
- **used** = Σ H3 over all candidates.
- **Order:** iterate candidates oldest → newest by `started_wall_ns` (tie: name), skipping `protect`. Remove one iff `age_ns > max_age_s·1e9` OR `remaining_count ≥ max_captures` OR `used + max_capture_bytes > max_total_bytes`. After each removal, update `remaining_count` and `used`.
- `remove_dir_all` errors go to `errors` as `"<id>: <err>"`, and that dir counts as kept.
- `kept_bytes` = used after pruning.

### S1–S6, H6 (writer)
- **S1:**
  1. `create_dir_all(root)`.
  2. `prune(cfg, Some(&meta.capture_id), now)`; the report is kept in `prune`.
  3. `create_dir` of `<root>/<id>`; if it already exists → Err "archive dir exists".
  4. Write `RAW_HEADER` to `raw.lgr` and create the empty other 3 files.
  5. Manifest Recording with `raw_first_sample = raw_end_sample = meta.first_sample`, `raw_bytes = 8`, `updated_wall_ns = now`.
  6. Spawn thread `logic-archive` running H6.

  Thread spawn failure → Err.
- **Quota accounting:** the sum of `raw.lgr` + `raw.idx` + `annotations.jsonl` + `annotations.idx` bytes, starting at 8 (the raw header). Before each write H6 checks `used + bytes_of_this_write (incl. its index entry) ≤ max_capture_bytes`; otherwise that part is truncated (sticky): raw at `block.start_sample`, ann at its row. Error text: `format!("archive quota {} MiB reached", max >> 20)`. When `max < 1 MiB` the text prints `0 MiB`; this is accepted, and tests only check `contains("archive quota")`.
- **Write order:**
  - raw block: append the bytes to `raw.lgr` → flush → append the R3 entry (offset = previous raw.lgr length) to `raw.idx` → flush → `raw_end_sample = block.end_sample`, update the raw_bytes/blocks atomics.
  - ann line: if `row % ANN_INDEX_EVERY == 0`, append `(row, offset)` to `annotations.idx`; append the line → flush.
  - Any I/O error truncates that part with `"archive write: <err>"`.
  - A block or row arriving after its part was truncated is dropped. Rows must arrive in row order; a row ≠ expected next row (because of a tap-side truncation) is not written.
- **Checkpoint:** `recv_timeout(CHECKPOINT)`. On timeout, or when ≥ CHECKPOINT has passed since the last manifest write, write the manifest from the current atomics (`updated_wall_ns` from `SystemTime::now`).
- **Finish:**
  - `raw_end_sample` = indexed end if raw is truncated, else `end_sample`.
  - `acquired_samples = Some(acquired)`.
  - state = Complete if F5 holds for the assembled manifest, else Partial.
  - Write the manifest and return it.
- **S2/S6 queue bound:** `shared.queued` (bytes); H6 subtracts after writing. S6 counts every non-marker line in `ann_offered`, even after truncation.
- **S4 status keys:**
  - `state` (`"recording"`)
  - `dir` (absolute string)
  - `raw_end_sample`, `raw_bytes`, `annotation_rows`
  - `raw_truncated_at_sample`, `annotation_truncated_at_row` (null when not truncated)
  - `error`
  - `pruned` (array of removed ids)
- **S5:**
  1. Send `Raw(last)` if any, then `Finish { end_sample, acquired }`.
  2. Drop `tx` (the taps may still hold clones — S5 is called after the decoder reader is joined, see B2b).
  3. Join, then assemble the S4 keys with `state = manifest.state.as_str()` and the final numbers.

### Q1–Q4 (readers)
- **Q2:** omit None fields.
- **Q3:** keys per the doc. `bytes` = H3. `complete` = F5. A missing root returns `{"root": <root>, "archives": [], "left_out": 0}`.
- **Q4 edges:**
  - `(dir, m) = H7`; `from = req.from ?? m.raw_first_sample`; `to = req.to ?? m.raw_end_sample (current)`. `cover_end = min(to, raw_end from the index`, i.e. the last indexed block's end = next block start, or the manifest `raw_end_sample` for the last block).
  - For the last block's end use the manifest value when the state is not Recording. While Recording, use the start of the block after it, or the manifest checkpoint value if that is ≥ the block start (never beyond what is indexed).
  - Find the first block with `start ≤ from` by binary search on the index, decode blocks from there, and compute `initial` = value at `from` (masked).
  - Emit transitions with `from < s < cover_end` whose masked value changes. Bits outside the mask are ignored; a record that changes no masked bit is skipped.
  - **Chunking:** a chunk is closed when it holds 1024 edges, at `sample_end = last_edge_sample + 1`; the next chunk starts there with `initial` = the value after that edge. When `limit` edges are emitted, stop: `next_from = that chunk's sample_end`. Otherwise the final chunk ends at `cover_end` and `next_from = null`.
  - Chunk keys: `capture_id, seq:null, source:"archive", samplerate, sample_start, sample_end, mask, initial, final, active` (OR of changed masked bits), `edges, dropped:0`.
  - `range.returned = [from, last sample_end]`, or `[from, from]` when empty.
  - `truncated_at = m.raw_truncated_at_sample`.
  - `complete = next_from == null && cover_end == to && (truncated_at == null || to ≤ truncated_at) && state != Recording`.
  - `from ≥ cover_end` → no chunks, `complete:false`.
  - Stop scanning at READ_SCAN_BYTES (then `next_from` = the end of the last full chunk, `complete:false`).
- **Q4 annotations:**
  - Start at the H9 entry with the greatest `row ≤ after_row+1` (or 0), skip to `after_row+1`, and read lines (BufRead) with N2.
  - When `from` or `to` is given, keep only rows with `start_sample` in `[from, to)`; rows with null samples are dropped in that case.
  - Stop at `limit` rows, READ_BYTES of serialized rows or READ_SCAN_BYTES read.
  - `next_row` = the last returned row when stopped early, else null. `complete = next_row == null && ann not truncated && state != Recording`.
  - `truncated_at = m.ann_truncated_at_row`. Malformed lines are skipped and counted in `"malformed": N`.

### E1–E3 (export)
- **File:** `export-<kind>-<from>-<to>-<k>.<csv|txt>` with the first free `k` in 1..=999 (else Usage "too many exports").
- **Quota:**
  - `H3(dir) ≥ max_capture_bytes` → Usage `"archive quota <MiB> MiB reached; export refused"`.
  - Mid-write: stop when the bytes written + `H3` before the export would exceed the quota → `limit_hit:true` and `complete:false`; the file keeps what was written.
  - EXPORT_MAX_BYTES and EXPORT_MAX_ROWS work the same way (`limit_hit`).
- **Content:** read through the same logic as Q4 (all pages, mask 0xFF). The metadata lines from the doc come first, written before the rows; `complete` in the metadata is the archive completeness for that range (as Q4 `complete`).
- **Times:** H4. **Annotations csv:** rows with null samples are written with empty sample/time fields, decoder `?`.
- **txt:** `"<t_start_s> <decoder>: <text>"`; unparsed rows are written as `"? ?: <text>"`.
- **Edges csv:** row 1 = `from` with the state at `from`; then one row per transition (all 8 bits).
- **Returns:**
  - `path` (absolute)
  - `bytes` (file size)
  - `rows` (data rows, excluding the header and the initial-state row for edges)
  - `complete`
  - `range`
  - `archive_state`
  - `truncated_at`
  - `limit_hit`

### B2a acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic
CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo test -p loom-logic --test archive        # 13/13
CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo test -p loom-logic                        # all other tests still green (engine tests SKIP/run per installed sigrok-cli)
CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo clippy -p loom-logic --all-targets -- -D warnings 2>&1 | grep -c "src/archive.rs"   # 0 archive.rs findings (warnings in other files belong to the B2b scaffold)
grep -n "Sonnet body pending" crates/loom-logic/src/archive.rs                                                         # empty
```
- **Report:** `work/reports/operator-upgrade/logic/B2A-WORKER.md`
- **Contents:** changed bodies, exact outputs, `git diff -- crates/loom-logic/src/archive.rs` sha256 (archive.rs is untracked → use `shasum -a 256 crates/loom-logic/src/archive.rs`), open issues.

---
## B2b — archive wiring (worker / gateway / CLI) + B2a.1 follow-ups · PLAN_READY_BATCH2B (FROZEN)
**Lease (exclusive, bodies only):**
- `crates/loom-logic/src/worker.rs`, `gateway.rs`, `cli.rs`
- `archive.rs`: only the B2a.1 bodies below

**Opus-owned (Sonnet must not edit):**
- `tests/{archive,archive_engine,cli_archive,load,engine,gateway,ui_decoder_specs}.rs`
- `lib.rs`
- registration/schemas/risk/hooks/help in gateway.rs, `command()` in cli.rs
- every signature, field, constant and type

The `// B2-W… (Sonnet)` comments mark call sites Sonnet must complete; remove each comment once it is done.

### B2a.1 (archive.rs bodies)
- **S7 `mark_decode_gap(&self, text)`:** lock `shared.trunc`; if `ann_error` is None, set it to `text`. Nothing else: no `ann_truncated` flag and no `ann_at_row`.
  - H6 already copies trunc errors into the manifest. Make sure the final manifest takes `ann_error` from trunc at Finish even when no write failed. S4/S5 `error` = raw_error, else ann_error.
- **F5:** add `&& self.raw_error.is_none() && self.ann_error.is_none()`. This makes H6's Finish state Partial.
- **E3:** bounded-memory export.
  1. Stream rows to `<final>.part` (BufWriter ≤ 1 MiB) while counting rows, bytes and limits.
  2. Write `<final>` = metadata header + `std::io::copy` of the part.
  3. Remove the part.

  Any error → remove both, then Internal (or Usage for quota). Quota is checked against `H3(dir)` + part bytes + header. Behaviour and return value are otherwise unchanged.
- **Q4 (edges):** after `decode_block`, `blk.start_sample != idx[bidx].start_sample` → Internal `"archive block corrupt at sample {idx start}"`.

**Tests:**
- `decoder_gap_makes_the_archive_partial_but_keeps_rows`
- `reader_refuses_a_block_that_disagrees_with_its_index`
- the amended `manifest_roundtrip_and_completeness_rules`
- `export_streams_to_its_final_name_without_leftovers` (already green; must stay green)

### Worker (`worker.rs`)
- **W0 `on_call`:** add the arms `"logic.archive.list" => self.archive_list(&call)`, `"logic.archive.read" => self.archive_read(&call)`, `"logic.export" => self.export(&call)`.
- **W1 `start`:**
  - Inside `prepared`, right after `StartConfig::from_params`: `ArchiveArgs::from_args(&call.args["archive"])`. An error → `err(ErrorCode::Usage, format!("archive: {e}"))`. This runs before the engine check, and nothing is emitted.
  - Return it with cfg/engine and put it in `Capture.archive`.
  - `archive_view` initial value: Disabled → `{"state":"disabled","reason":r}`; Enabled → `{"state":"pending"}`.
- **W2 `setup_archive`:**
  - Disabled → `None` (view unchanged).
  - Enabled → `ArchiveSink::open(cfg, &ManifestMeta { capture_id: id, source: cfg.source.as_str(), samplerate, decoders: canonical list, first_sample: 0 }, self.cx.clock().wall_ns())`.
    - `Ok(sink)` → view = `sink.status()`, `Some(ArchiveSide { enc: RawEncoder::new(0), sink })`.
    - `Err(e)` → view = `{"state":"failed","error":e,"dir":null}`, `None`. The capture continues.
- **W3 `run`:**
  - First statement in the `setup` closure: `let arch = self.setup_archive();` (before engine resolution, so even an engine error leaves a finalized archive).
  - Pass `arch.as_ref().map(|a| a.sink.tap())` to `setup_decoder`.
  - Setup `Err` → if `arch` is `Some`, call `finish(None, 0, 0)` and set the view; then the existing failed path. The failed state carries the view.
  - Success → `self.main(dec, acq, arch)`.
  - The closure's return type becomes `Result<(Option<DecSide>, Option<AcqSide>, Option<ArchiveSide>), (ErrorCode, String, Option<ArchiveSide>)>`. This is a local closure, so Sonnet may shape it.
- **W5 `pump`:** after the sample-bound truncation and the empty check, before `acc.feed`:
  `if let Some(a) = arch { let b = a.enc.feed(&chunk); if !b.is_empty() { a.sink.raw(b); } }`
  When `running` flips to true, refresh `archive_view = a.sink.status()` before `self.state("running", …)`. Both `pump` call sites in `main` pass `arch.as_mut()`.
- **W11 `main` stop order:**
  1. Refresh the view before `state("stopping")`.
  2. Run the existing acquisition stop/drain (pump keeps archiving the drained samples) and `flush_edges`.
  3. Run the existing decoder block (its reader thread is joined, so every tap clone is dropped).
  4. Then `if let Some(mut a) = arch.take() { let (last, end) = a.enc.finish(); if let Some(g) = gap { a.sink.mark_decode_gap(&g); } *lock(&self.archive_view) = a.sink.finish(last, end, samples); }`. `gap` is defined below.
  5. Then the final `self.state(…)`.

  `gap` (decoder present only):
  - `written = counters.decoder_input_bytes`
  - `clean` = the decoder exited with status success and was not killed
  - `gap = (written < samples || !clean).then(|| format!("decoder consumed {written} of {samples} samples{}", if clean { "" } else { "; decoder did not exit cleanly" }))`
  - no decoder → `None`
- **W12 `state`:** add `"archive": lock(&self.archive_view).clone()` to the payload (new optional key; old readers ignore it).
- **W13 `read_lines`:** for each non-empty line, after `parse_annotation`, call `if let Some(t) = tap { t.push(&a); }` before `try_send`. Trailing loss markers are never pushed.
- **W6/W7/W8:**
  - `ArchiveArgs::from_args(&call.args["archive"])`; an error → Usage `"archive: {e}"`.
  - Request parse: `ReadReq` / `ExportReq` `::from_params(&call.args["request"])`; an error → Usage.
  - **list:**
    - Disabled → `Ok({"root":null,"archives":[],"left_out":0,"disabled":reason})`.
    - Enabled → `run_blocking("logic-archive-list", move || Ok(archive::list(&root)))`.
  - **read / export:**
    - Disabled → `Err(Config, "logic archive disabled: <reason>")`.
    - Enabled → `run_blocking(…, move || archive::read(&root, &req).map_err(archive_err))`, and export with `max_capture_bytes`.

  Validation happens before any thread or file access.
- **W9 `run_blocking`:** `std::thread::Builder::new().name(name)`, a tokio oneshot, the future awaits it. Spawn error → Internal `"thread: {e}"`; dropped sender → Internal `"{name} ended"`.
- **W10 `archive_err`:** `WorkerError { code, message, detail: Value::Null }`.

### Gateway (`gateway.rs`)
- **G1:** `ArchiveArgs::from_config(&ctx.config(), ctx.storage_roots().as_ref().map(|r| r.home.as_path()))`, with `.map_err(|e| DomainError::new(ErrorCode::Config, e))`.
- **G2 `Start`:** add `"archive": archive_args(ctx)?.to_args()` to the worker args. A config error fails the start with E_CONFIG before the worker is called.
- **G3:** `strict(&params, &[])?` → worker `logic.archive.list` `{"archive": …}`.
- **G4/G5:** `ReadReq` / `ExportReq` `::from_params(&params).map_err(usage)?` → worker `{"archive": …, "request": req.to_params()}` with `ctx.deadline()`.

### CLI (`cli.rs`)
- **C1:** build params from the matches: `capture_id` ← ID, `kind`, `from_sample` ← --from, `to_sample` ← --to, `limit`, `after_row` ← --after-row.
  - `--mask`: `0x`/`0X` prefix → hex, else decimal. A parse failure → `UsageError("--mask: expected 1..255 or 0x01..0xFF")`. Range checks stay in ReadReq.
  - Then `ReadReq::from_params(&p).map_err(UsageError::new)?` → `RpcCall::new("logic.archive.read", req.to_params())`.
- **C2:** same with `ExportReq` → `"logic.export"`; `c.wait = Some(Duration::from_secs(62))`.
- **C3:** the `to_call` arms:
  - `("archive", a)` → `list` → `RpcCall::new("logic.archive.list", json!({}))`; `read` → C1; else Usage `"expected archive list or archive read"`.
  - `("export", s)` → C2.

### B2b acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2   # (zsh: do not use an unquoted $E prefix)
cargo test -p loom-logic --no-fail-fast            # all targets green: unit 15, archive 16, archive_engine 3, cli_archive 2, engine 10, gateway 4, ui_decoder_specs 2; load 2 ignored
cargo clippy -p loom-logic --all-targets -- -D warnings   # clean
cargo check -p loom-logic --no-default-features --features cli   # CLI-only build (invariant I1) still compiles
grep -rn "unimplemented!(\|B2-W.*(Sonnet)" crates/loom-logic/src   # empty
```
After acceptance, the lead runs once: `cargo test -p loom-logic --release --test load -- --ignored --nocapture` and records the numbers in STATUS (measured load, L1-M).

---
## B3 — UI: B3a decode table tools + cursors, B3b archive view + export · PLAN_READY_BATCH3 (FROZEN)
**Lease (bodies + function-local DOM only):** `ui/src/panels/logic/logicTools.ts`, `logicModel.ts` (only the B3b-M* bodies + `readCaptureState`), `logicPanel.ts`, `waveform.ts`.

The Opus-owned list below also covers `logicBatch3b.test.ts` and the panel manifest (`registerPanel`).

### B3a

**Opus-owned (Sonnet must not edit):**
- tests: `logicBatch3a.test.ts`, `logic.test.ts`, `logicBatch1.test.ts`, `logicModule.test.ts`
- `logic.css`, every logicModel.ts item outside B3b-M1…M9 + `readCaptureState`
- every type, signature, top-level import and constant, including the option tables `RADIX_KEY … LINE_END_LABEL` and `isRadix`

**Allowlist:** T1–T14 in logicTools.ts (bodies that currently throw "Sonnet body pending"), W2 in `paint`, P1–P9 in `createLogicAnalyzer`. Private closures inside those bodies are fine. Number formatting is inline, as in B1: `fmtN = Math.round(n).toLocaleString('ko-KR')`.

### logicTools.ts (pure)
- **T1–T3:** per the doc comments. T3 hex keeps the original text width (`'0041'` → `'0041'`) and upper-cases it.
- **T4 `splitLines`:**
  - Walk `rows` in the given order, keeping one open line per decoder.
  - **Data byte** (`v = wordValue`, `v ≤ 0xFF`):
    - **LF (0x0A):** if the previous byte of this decoder was a CR that ended a line, append the row to that ended line (`rowKeys`, `endSample`, `bytes+1`, `end:'crlf'`). Otherwise end the open line with `'lf'`, or emit an empty `''` line if none is open.
    - **CR (0x0D):** end the open line with `'cr'` (or an empty line).
    - **Any other byte:**
      - Render `r = asciiOf(v)`.
      - If a line is open and `open.text.length + r.length > maxChars` → end it with `'split'` and open a new one.
      - Append `r`, the row key and `endSample`; `bytes+1`.
    - Any non-CR byte clears the "previous was CR" mark.
  - **Non-data row** (null or > 0xFF): end the open line with `'none'`, then emit a `'note'` line `⟦<row text>⟧` (bytes 0), and clear the CR mark.
  - At the end, every open line ends with `'none'`.
  - A line takes `startSample`/`key` from its first row and `endSample` from its last row's `end`.
  - Output is sorted by `startSample`, stable (decoders interleave by time).
- **T5:**
  - Tokens are split on whitespace and commas, and each may carry a `0x`/`0X` prefix.
  - With more than one token, each token must be 1–2 hex digits.
  - A single token: 1–2 hex digits → 1 byte; otherwise an even number of hex digits → bytes.
  - Empty, malformed, or > 256 bytes → `null`.
- **T6 `searchRows`:**
  - Validation errors, in order:
    - an empty pattern (after trim for bytes mode, raw for text) → `'검색어가 비어 있습니다'`
    - text: the UTF-8 bytes of the pattern (`TextEncoder`), > 256 → `'검색어가 256바이트를 넘습니다'`
    - bytes: T5 → `null` → `'바이트 패턴: 16진수 바이트 (예: 4C 4F 4F 4D)'`
  - Each decoder has its own byte stream. A non-data row (null or > 0xFF) resets that decoder's partial match.
  - Case-insensitive (text mode with `caseSensitive` false): fold ASCII A–Z to a–z on both sides.
  - Matching is naive per position with **non-overlapping** hits: after a hit, the next search starts after its last byte.
  - A hit records the first and last row (key, `startSample` = first row start, `endSample` = last row end).
  - Collect every hit, sort by `startSample` (stable), and return the first `limit` with `truncated = total > limit`.
- **T7:** BigInt: `ns = BigInt(sample) * 1_000_000_000n / BigInt(Math.round(rate))`, seconds `ns / 1e9n`, fraction 9 digits with zeros trimmed. `rate` null or ≤ 0 → `#${sample}`.
- **T8–T11:** exact strings as in the tests. Header lines end with `\n`, and every row line ends with `\n`. CSV text uses RFC 4180 escaping (quote if it contains `,` `"` CR or LF). CSV times use T7 on `start` / `end`. Rows without a rate still print `#n`.
- **T12 `fmtHz`:** try units MHz (1e6), kHz (1e3), Hz in that order. Use the first unit where `+(f/u).toFixed(3) ≥ 1`; Hz always qualifies. Print `` `${+(f/u).toFixed(3)} ${unit}` ``.
- **T13:** `A`/`B` labels use `fmtSampleTime(x, rate, 4)` (`#n` without a rate), `Δt` uses `fmtSeconds(dt, 4)`, `1/Δt` uses T12. Parts are joined with `' · '`.
- **T14 `pulseAt`:**
  1. `st = cap.stateAt(sample)`; null → null. The level is the bit of `st`.
  2. **Backward:** find the latest real transition of `bit` at sample `p ≤ sample`, inside retained chunks.
     - Walk back through the chunk's edges, then previous chunks, only while they are contiguous (`prev.end === next.start`).
     - The value before each edge is the previous edge's mask, or `chunk.initial` for the first edge.
     - A chunk's start is a transition only if its `initial`'s bit differs from the previous contiguous chunk's `final`. Treat it as unknown if there is no previous contiguous chunk.
  3. **Forward:** likewise, the first transition at `n > sample`.
  4. If no `p` or no `n` is found, if any `cap.unknown` range overlaps `[p, n)`, or if `p < cap.evictedTo` → null.
  5. **Period:** the next transition `n2 > n` found the same way, with no unknown in `[n, n2)`; otherwise `nextEnd = null`.
  6. Scan at most `PULSE_SCAN_EDGES` edges per direction, else null / no period.
  7. `widthS = rate ? (n-p)/rate : null`; `periodS = rate && n2 ? (n2-p)/rate : null`.
  8. Duty = `level ? w/P*100 : (P-w)/P*100` using sample counts. With no rate, compute the duty from samples.
  9. **text:** `` `${level?'HIGH':'LOW'} ${rate ? `${fmtSeconds(widthS,4)} (${fmtN(w)} smp)` : `${fmtN(w)} smp`}` `` + (with a period: `` ` · 주기 ${rate ? fmtSeconds(periodS,4) : `${fmtN(P)} smp`}${rate ? ` · ${fmtHz(1/periodS)}` : ''} · duty ${duty.toFixed(1)}%` ``).

### W2 `paint` cursors (waveform.ts)
After the hover cursor, if `inp.cursors` is set:
1. **Band:** when both A and B are non-null, clamp both x positions to `[0, v.width]`. If the span is > 0: save, `globalAlpha = 0.08`, `fillStyle = pal.sel`, `fillRect(GUTTER+x0, AXIS_H, x1-x0, heightPx-AXIS_H)`, restore.
2. **Lines:** for each `[label, s]` of `[['A', a], ['B', b]]` with s non-null and `x = (s - v.s0)/v.spp` in `[0, v.width]`:
   - `strokeStyle = pal.sel`, `lineWidth = 1`, `setLineDash([4, 3])`
   - `beginPath()`, `moveTo(GUTTER + Math.round(x) + 0.5, AXIS_H)`, `lineTo(same, heightPx)`, `stroke()`
   - `setLineDash([])`
   - `fillStyle = pal.sel`, `fillText(label, GUTTER + Math.round(x) + 3, AXIS_H + 8)`

### Panel (logicPanel.ts, inside `createLogicAnalyzer`)
- **P1 state** (locals next to the existing view state):
  - `let radix: Radix = loadJson(RADIX_KEY, isRadix) ?? 'hex'`
  - `let tableMode: 'rows' | 'lines' = 'rows'`
  - `let cursors: Cursors = { a: null, b: null }`
  - `let hoverY: number | null = null`
  - search: `searchPattern = ''`, `searchMode: SearchMode = 'text'`, `searchCase = false`, `searchRes: SearchResult | null = null`, `searchIdx = -1`, `searchKey = ''`
  - lines: `lineItems: TextLine[] = []`, `selectedLine: TextLine | null = null`
  - `hasClass = false`

  `syncView`'s capture-change branch also resets `cursors`, `searchIdx = -1`, `selectedLine = null`.
- **P2 DOM:**
  - **`cursorBar`** = `el('div', 'lg-cursors')`, `data-testid="logic-cursors"`, containing:
    - `cursorText` = `el('span', 'lg-cursors-text lg-muted', CURSOR_HINT)`
    - `btn('지우기', '커서 지우기 (Esc)', 'logic-cursor-clear')`
  - **`toolsBar`** = `el('div', 'lg-tools')` containing, in order:
    - `sel('radix', RADIX_OPTS)`, testid `logic-radix`, `aria-label` '표시 진법'
    - `sel('table_mode', TABLE_MODE_OPTS)`, testid `logic-table-mode`
    - `inp('search', 'search')`, testid `logic-search`, placeholder `'검색 — 텍스트 login: · 바이트 4C 4F'`
    - `sel('search_mode', SEARCH_MODE_OPTS)`, testid `logic-search-mode`
    - a `label` wrapping `inp('search_case', 'checkbox')` (testid `logic-search-case`) + '대소문자'
    - `btn('◀', '이전 결과 (Shift+Enter)', 'logic-search-prev')`, `btn('▶', '다음 결과 (Enter)', 'logic-search-next')`
    - `searchStatus` = `el('span', 'lg-search-status')`, testid `logic-search-status`, `role="status"`
    - `sel('copy_scope', COPY_SCOPE_OPTS)`, testid `logic-copy-scope`
    - `sel('copy_format', COPY_FORMAT_OPTS)`, testid `logic-copy-format`
    - `btn('복사', '범위·완전성 메타데이터와 함께 클립보드로 복사', 'logic-copy')`
  - **`lineHost`** = `el('div', 'lg-lines')`, testid `logic-lines`, hidden.
  - Root order: `…, waveHost, readout, cursorBar, legend, annHead, toolsBar, tableHost, lineHost, rawBox`.
  - `radixSel.value = radix`.
- **P3 tables:**
  - **Rows table:** `const makeRowTable = (withClass: boolean) => new VirtualTable<Annotation>(tableHost, {…existing options…, columns})`, where `columns` is the existing column list minus `cls` unless `withClass`. The text cell is `{ text: formatWord(a.text, radix), cls: a.cls === '_raw' ? 'lg-rawline' : 'lg-anntext', title: a.text }`. `let table = makeRowTable(false)`.
    - `onSelect` (existing behaviour): `selected` + `centerOn`, then `cursors = { a: sel.start, b: sel.end }`.
  - **Lines table:** `lineTable = new VirtualTable<TextLine>(lineHost, …)`
    - `keyOf: l => l.key`, `rowAttrs: l => ({ 'data-start': String(l.startSample) })`
    - columns:
      - `s` 'sample 시작' 10em, `fmtN`
      - `t` '시각 (sample clock)' 9em, `fmtSeconds(l.startSample / rate, 4)` or `'?'` (rate of `current()`)
      - `dec` '디코더' 7em
      - `text` '줄 (ASCII)' 1fr, `{text: l.text, cls: 'lg-anntext'}`
      - `end` '끝' 5em, `{text: LINE_END_LABEL[l.end], cls: 'lg-lineend'}`
    - `onSelect`: `selectedLine` = the found line; then `centerSpan(l.startSample, l.endSample)` (local closure: same as `centerOn` on a span), `cursors = { a: l.startSample, b: l.endSample }`, invalidate.
    - `emptyText '디코드 결과 없음'`, `ariaLabel '디코드 줄'`.
  - `dispose()` also disposes `lineTable`.
- **P4 events:**
  - `radixSel` change → set `radix`, `saveJson(RADIX_KEY, radix)`, invalidate.
  - `modeSel` change → set `tableMode`, invalidate.
  - `searchInp` `input` → `searchPattern = value`, invalidate. `keydown` Enter → next, Shift+Enter → prev (`preventDefault`).
  - `searchModeSel` / `caseChk` change → update, invalidate.
  - prev/next click → `jump(±1)`.
  - copy click → `copy()`.
  - clear click → `cursors = { a: null, b: null }`, invalidate.
- **P5 `jump(dir)`:**
  1. No hits → return. Else `searchIdx = searchIdx < 0 ? (dir > 0 ? 0 : n-1) : (searchIdx + dir + n) % n`.
  2. `hit = hits[searchIdx]`.
  3. Rows mode: `table.select(hit.firstKey); table.reveal(hit.firstKey)`. Lines mode: the line whose `rowKeys` includes `firstKey` → `lineTable.select(key); lineTable.reveal(key)`.
  4. `centerSpan(hit.startSample, hit.endSample)`.
  5. Then `cursors = { a: hit.startSample, b: hit.endSample }`; `setFollow(false)`.
- **P6 `copy()`:**
  1. `c = current()`; none → toast `'복사할 캡처가 없습니다'`.
  2. `lo/hi = min/max(cursors)`.
  3. **Items:**
     - Rows mode:
       - `all` = `c.annotations`
       - `cursors`: both set → rows with `lo ≤ start < hi`, else toast `'커서 A와 B를 먼저 놓으세요'` and return
       - `selection`: `selected` ? `[selected]` : toast `'선택된 행이 없습니다'` and return
     - Lines mode: the same scopes over `lineItems` / `selectedLine`.
  4. **meta:** `{captureId: c.id, samplerate: rate, source: 'panel', scope, fromSample: scope==='cursors' ? lo : (first item start ?? 0), toSample: scope==='cursors' ? hi : (last item end ?? 0), coverage: decodeCoverage(c).state}`.
  5. **Text:**
     - rows + txt → `rowsToText(rows, radix, meta)`; rows + csv → `rowsToCsv`
     - lines + txt → `linesToText`; lines + csv → `rowsToCsv` of `c.annotations` whose key is in the chosen lines' `rowKeys`
  6. `navigator.clipboard?.writeText(text)`:
     - resolve → toast `` `복사됨: ${fmtN(count)}${tableMode === 'rows' ? '행' : '줄'}` ``
     - reject or missing clipboard → toast `'복사 실패: 클립보드 권한 없음'`
- **P7 pointer/keys (waveHost):**
  - **pointermove:** also `hoverY = e.clientY - waveHost.getBoundingClientRect().top`. pointerleave → `hoverY = null`.
  - **pointerup** (replaces `endDrag` for `pointerup` only; `pointercancel` keeps `endDrag`): if `drag && Math.abs(e.clientX - drag.x) <= 2 && spp && localX(e) >= 0` → `s = Math.floor(s0 + localX(e) * spp)`; `e.shiftKey ? cursors.b = s : cursors.a = s`; invalidate. Then `drag = null`.
  - **keydown** additions:
    - `'1'` / `'2'` with `hoverX !== null && hoverX >= 0 && spp` → set A / B at `Math.floor(s0 + hoverX * spp)`
    - `'Escape'` → clear both
    - `preventDefault` for these keys
- **P8 draw additions:**
  1. **hasClass:** `hc = !!c && c.annotations.some((a) => a.cls !== null)`. If `hc !== hasClass` → `table.dispose(); tableHost.replaceChildren(); table = makeRowTable(hc); hasClass = hc; tableKey = ''`.
  2. **Table key:** `` `${c?.id}|${c?.version}|${radix}|${tableMode}` ``.
     - Rows mode → existing `setItems`.
     - Lines mode → `lineItems = c ? splitLines(c.annotations) : []`, `lineTable.setItems(lineItems)`, `followTail` if following.
     - Show/hide: `tableHost.hidden = tableMode !== 'rows'`; `lineHost.hidden = !tableHost.hidden`. Render the visible table.
  3. **Search:**
     - Key `` `${c?.id}|${c?.version}|${searchMode}|${searchCase}|${searchPattern}` ``. When it changes: `searchRes = c && searchPattern ? searchRows(c.annotations, {mode, pattern, caseSensitive}) : null`, `searchIdx = -1`.
     - Status:
       - `searchRes?.error` → that text + class `lg-search-status lg-bad`
       - a result → `` `${searchIdx+1}/${hits.length}${truncated ? '+' : ''}` ``
       - none → `''`
  4. **Cursor bar:** `m = measureCursors(cursors, rate)`; text = `m ? m.text` : exactly one set ? `` `${a!==null?'A':'B'} ${fmtSampleTime(a ?? b, rate, 4)}` `` : CURSOR_HINT. Toggle class `lg-muted` iff the hint is shown.
  5. **paint:** pass `cursors`, and add `cursors.a` / `cursors.b` to the paint key `pk`.
  6. **Readout pulse:** inside the existing hover readout, when `hoverY !== null`:
     - `li = Math.floor((hoverY - AXIS_H) / LANE_H)`; if `0 ≤ li < ls.length`, `p = pulseAt(c, ls[li].bit, s, rate)`.
     - If `p`, append `` ` · ${ls[li].name} ${p.text}` ``. If not, append nothing (never a guess).
- **P9 `copyHeader` is used only through T9–T11.** No other text composition in the panel.

### B3b — archive view, export, archive status (L1-R2 / L5-R2)
**Model (logicModel.ts):**
- **M1 `readArchiveInfo`:**
  - `state`: `str` of the 7 names, else `'unknown'`
  - `dir`, `reason`, `error`: `str`
  - numbers via `num`: `raw_end_sample`, `raw_bytes`, `annotation_rows`, `raw_truncated_at_sample`, `annotation_truncated_at_row`
  - `pruned`: `strList`
  - non-plain-object → `null`
- **M2 `archiveLabel`:** exact texts. `title` = `dir ?? ''`, except where noted.
  - `null`/`undefined` → `{text:'아카이브 정보 없음', tone:'muted', title:'이 캡처의 capture_state에 archive가 없습니다 (이전 엔진이거나 아직 수신 전)'}`
  - disabled → `` `아카이브 꺼짐 — ${reason ?? '사유 미보고'}` `` muted
  - pending → `'아카이브 준비 중'` muted
  - recording → `` `아카이브 기록 중 · 디코드 ${fmtN(rows ?? 0)}행` `` ok
  - complete → `` `아카이브 완전 · 디코드 ${fmtN(rows ?? 0)}행 · 원시 sample ${fmtN(rawEnd ?? 0)}` `` ok
  - partial → `` `아카이브 일부 — ${error ?? '잘림'}` `` + (rawTruncatedAt ≠ null ? `` ` · 원시 sample ${fmtN}에서 잘림` ``) + (annotationTruncatedAt ≠ null ? `` ` · 디코드 ${fmtN}행에서 잘림` ``), warn
  - failed → `` `아카이브 실패 — ${error ?? '사유 미보고'}` `` bad
  - unknown → `'아카이브 상태 미확인'` muted
- **M3 `readCaptureState`:** `archive: has('archive') ? readArchiveInfo(p.archive) : (prev?.archive ?? null)`.
- **M5 / M6:** per the doc comments. `markNotLoaded` uses the private `addUnknown`.
- **M7 `readArchiveEdges`:**
  - `chunks`: an array → each through `readEdges(obj, captureId, -1n, null)`, nulls dropped; anything else → `[]`
  - `nextFrom = num(next_from)`, `complete = result.complete === true`, `truncatedAt = num(truncated_at)`
- **M8 `readArchiveRows`:** `readAnnotations('logic.annotations', {capture_id: captureId, samplerate, annotations: rows}, captureId, -1n, samplerate)` → rows / rawLines / rejected. `nextRow = num(next_row)`, `complete === true`, `truncatedAt = num(truncated_at)`, `count = rows array length`.
- **M9 `archiveViewCoverage`:** per the doc.

**Panel (inside `createLogicAnalyzer`):**
- **P10 state:**
  - `viewSrc: 'live' | 'archive' = 'live'`
  - `archCap: CaptureData | null`, `archFor: string | null`
  - `archRowsNext: number | null`, `archRowsDone = false`
  - `archWin: {from: number; to: number} | null`
  - `archRowsBusy = false`, `archEdgesBusy = false`
  - `archErr: string | null`, `archGen = 0` (bumped on every reset; a page result applies only if its gen still matches)
  - `exportLine = ''`
- **P11 DOM:** `archBar` = `el('div', 'lg-archive')` with testid `logic-archive`, placed right after `counters`. Children:
  - `chip` = `el('span', 'lg-archive-chip')`, testid `logic-archive-state`
  - `srcSel` = `sel('view_source', VIEW_SOURCE_OPTS)`, testid `logic-view-source`
  - `moreBtn` = `btn('디코드 더 불러오기', '아카이브에서 다음 1000행 × 20쪽', 'logic-archive-more')`
  - `expKind` = `sel('export_kind', EXPORT_KIND_OPTS)`, testid `logic-export-kind`
  - `expBtn` = `btn('파일로 내보내기 (daemon)', '아카이브에서 daemon 쪽 파일로 씁니다 — 브라우저 다운로드가 아닙니다', 'logic-export')`
  - `expLine` = `el('span', 'lg-export-result')`, testid `logic-export-result`, `role="status"`

  Also add `archNote` = `el('p', 'lg-archive-note', ARCHIVE_NOTE)`, testid `logic-archive-note`, hidden. Append it to `cfgFoot` after `cfgWarnings`.
- **P12 `resetArchiveView(c)`:** `archGen++`; `archCap = new CaptureData(c.id)`; `archFor = c.id`; `archRowsNext = null`; `archRowsDone = false`; `archWin = null`; `archErr = null`; `selected = null`; `cursors = {a: null, b: null}`; `setFollow(false)`; then `loadRows()`.
- **P13 `loadRows()`:**
  - Guard: `archRowsBusy || archRowsDone || !archCap` → return. Capture `gen = archGen` and `id = archFor`; set `archRowsBusy = true`.
  - Loop at most `ARCHIVE_ROW_PAGES` times:
    1. `r = await ctx.query('logic.archive.read', {capture_id: id, kind: 'annotations', limit: 1000, ...(archRowsNext !== null ? {after_row: archRowsNext} : {})})`
    2. `gen !== archGen` → stop silently.
    3. `pg = readArchiveRows(r, id, archCap.samplerate ?? c.samplerate)`; `archCap.addAnnotations(pg.rows, 0)`; `archCap.addRawLines(pg.rawLines, pg.rejected)`; `archRowsNext = pg.nextRow`; invalidate.
    4. `pg.nextRow === null` → `archRowsDone = true`; break.
  - Errors → `archErr = '아카이브 읽기 실패: ' + message`.
  - `finally`: `archRowsBusy = false` (when gen still matches); invalidate.
  - The first page sends no `after_row` (the test asserts the exact params).
- **P14 `loadEdges(from, to)`:**
  - Guard: `archEdgesBusy || !archCap` → return. Capture gen/id; set `archEdgesBusy = true`; `archWin = {from, to}` (set first, so the next frames do not refetch); `archCap.clearChunks()`.
  - Loop at most `ARCHIVE_EDGE_PAGES` times with `cur = from`:
    1. `r = await ctx.query('logic.archive.read', {capture_id: id, kind: 'edges', from_sample: cur, to_sample: to, limit: 2048})`
    2. Gen check.
    3. `pg = readArchiveEdges(r, id)`; `addChunk` each chunk.
    4. `pg.nextFrom === null` → done; else `cur = pg.nextFrom`.
  - After the loop, if more remains (`nextFrom !== null` and `< to`): `archCap.markNotLoaded(cur, to, NOT_LOADED_DETAIL)`.
  - Errors → `archErr`. `finally`: `archEdgesBusy = false`; invalidate.
- **P15 draw additions:**
  1. **Chip:** `l = archiveLabel(c?.info?.archive)`; `chip.textContent = l.text`, `chip.dataset.tone = l.tone`, `chip.title = l.title`. `archBar.hidden = !c`.
  2. **Readable:** `readable = !!c && ARCHIVE_READABLE.includes(c.info?.archive?.state ?? 'unknown')`.
     - `srcSel.disabled = !readable`.
     - If `!readable && viewSrc === 'archive'`: `viewSrc = 'live'`, `srcSel.value = 'live'`.
     - `expBtn.disabled = !readable || ctx.writeLocked`.
     - `moreBtn.hidden = viewSrc !== 'archive' || archRowsDone`; `moreBtn.disabled = archRowsBusy`.
  3. **Shown capture:** `shown = viewSrc === 'archive' && archCap && archFor === c?.id ? archCap : c`.
     - With `shown === archCap`: set `archCap.info = c.info`.
     - If `archFor !== c?.id` while `viewSrc === 'archive'` → `resetArchiveView(c)`.
     - Everything below that draws or acts on data uses `shown`: lanes, decoders, the follow line (follow is off in archive mode), syncView/fit/centerOn targets, `paint` cap, readout/pulse, tables, search, copy, `annInfo`.
     - Counters, the stop notice/coverage note and the status line keep using `c` (live facts).
  4. **Edges trigger:** `shown === archCap && spp && !archEdgesBusy` → `need = [max(0, floor(s0)), ceil(s0 + width*spp)]`; `span = need[1] - need[0]`. If `!archWin || need[0] < archWin.from || need[1] > archWin.to` → `loadEdges(max(0, need[0] - span), need[1] + span)`.
  5. **annInfo (archive mode)** replaces the live text:
     `` `${fmtN(shown.annotations.length)}행 · 아카이브에서 불러옴 (전체 ${a.annotationRows === null ? '?' : fmtN(a.annotationRows)}행${archRowsDone ? '' : ' · 더 있음'})${archErr ? ` · ${archErr}` : ''} · 디코더 ${decs.join(', ') || '—'} · 해석 결과(원시 아님)` ``
  6. **Copy meta (P6):** `source = shown === archCap ? 'archive' : 'panel'`; coverage = `archiveViewCoverage(c.info?.archive, archRowsDone)` in archive mode, else `decodeCoverage(c).state`.
  7. **`archNote.hidden`** = `estimate.warnings.length === 0`. Reuse the estimate already computed for `cfgWarnings`; compute it once per draw.
  8. **`expLine.textContent = exportLine`.**
- **P16 events:**
  - `srcSel` change → `viewSrc = value`; if archive and `current()` → `resetArchiveView(current())`; invalidate.
  - `moreBtn` → `loadRows()`.
  - `expBtn` click:
    1. `[kind, format] = expKind.value.split(':')`.
    2. `ctx.act('export', {capture_id: c.id, kind, format})`.
    3. Then: `r = obj(h.result)`; `exportLine = ` `` `daemon에 저장됨: ${r.path} · ${fmtN(num(r.rows) ?? 0)}${kind === 'edges' ? '개 전이' : '행'} · ${r.complete === true ? '완전' : '일부'}${r.limit_hit === true ? ' (상한 도달)' : ''}` ``.
    4. Catch: `` exportLine = `내보내기 실패: ${message}` ``.
    5. Invalidate.

    The text never says 다운로드 or 저장 완료 in the browser sense. `num` is the logicModel export; import it if needed. A missing import → return the lease.
- **Rule:** while in archive mode, live events keep updating `c` as before; the archive view does not mix live rows in.

### B3 acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic/ui
npx vitest run src/panels/logic                      # all files green: B1 60 (incl. the amended class assertion), B3a 19, B3b 12 → 91
npx tsc -p tsconfig.app.json  --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
npx tsc -p tsconfig.node.json --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/panels/logic                          # 0 problems
npx vitest run src/shell/logicWiring.test.ts src/shell/moduleApps.test.tsx src/shell/serviceSeparation.test.tsx src/sdk/capacityLive.test.ts   # 19/19
grep -n "Sonnet body pending" src/panels/logic/*.ts   # empty
```

---
## B4r — UART TX+RX in one capture, decoder error rows (Rust, finding L6) · PLAN_READY_BATCH4R (FROZEN)
**Lease (bodies only):** `crates/loom-logic/src/model.rs`, `worker.rs`, `cli.rs`. The B5 trigger skeletons/fields/markers already in those files (`B5-…`) are NOT part of B4r: leave them unchanged, including `unimplemented!` bodies and the `B5-W1` initializer comment.

**Opus-owned:**
- `tests/duplex.rs`, `tests/cli_archive.rs` (new glitch test), the model unit tests
- `gateway.rs` (schema/help already updated)
- all fields/signatures

The `// B4-M… (Sonnet)` markers are initializer placeholders that Sonnet replaces.

**Probe basis (local sigrok-cli 0.7.2, stdin):**
- two `-P uart` stacks print `uart-1` / `uart-2`, interleaved and streamed
- `-A uart=rx-data:rx-warnings:rx-parity-err:rx-break:tx-data:tx-warnings:tx-parity-err:tx-break` applies to both stacks
- framing errors print `Frame error`; parity errors print `Parity error`

### model.rs
- **M1 `parse_uart`:**
  - Both `rx` and `tx` given → if equal: Usage `"uart rx and tx on the same channel D{n}"`; else `ch = rx`, `tx = false`, `duplex_tx = Some(tx)`.
  - Only one → as today, with `duplex_tx: None`.
  - Neither → Usage `"uart needs rx= and/or tx="`.
- **`canonical`:** duplex → `uart:rx=D{ch}:tx=D{t}:baudrate=…:data_bits=…:parity=…:stop_bits=…:bit_order=…`; single line unchanged.
- **`pd_arg`:** unchanged (first stack, no format), kept for compatibility.
- **M2 `pd_args`:** per the doc. The duplex TX stack is `uart:tx={t}:` + the same options + `:format=hex`.
- **`annotation_filter`:** uart → `uart=` + for each annotated line in stack order (`rx` and/or `tx`): `<l>-data:<l>-warnings:<l>-parity-err:<l>-break`, joined with `:`. SPI unchanged.
- **`mask`:** includes `duplex_tx`.
- **M3 `tracks`:** per the doc.
- **M4 `row_role`:** per the doc.
- **M5 `merge_segs`:** per the doc.
- **`decoder_args`:** for each `pd_args()` element push `"-P", v` (replacing the single `-P pd_arg()`). Everything else is unchanged.
- **`Annotation::to_row`:** parsed rows add `"role": row_role(&self.text)`; `_raw` rows add `"role": null`.
- **M6 `from_params`:** `glitch_every = get_u64(o, "glitch_every")?`.
  - `Some(n)`: source ≠ synthetic or decoder not uart → Usage `"glitch_every applies only to synthetic uart"`; `n` outside 1..=1000 → Usage `"glitch_every: expected 1..=1000"`.
  - Store it as `u32`. `to_params`: emit `glitch_every` only when `Some` (inside the synthetic block).
- **M7 `Synth::new`:** `SynthKind::Uart { ch, baud, tx: u.duplex_tx, glitch_every: cfg.glitch_every.map_or(0, u64::from) }`. The existing 8N1/LSB and rate checks apply to duplex too.
- **M8 `idle`:** uart → `1<<ch | tx.map_or(0, |t| 1<<t)`.
- **M9 `build_frame` uart:**
  - **RX segments:** exactly today's construction using value bit `ch`. Per byte: `bytes_done += 1`; the stop bit is `0` iff `glitch_every > 0 && bytes_done % glitch_every == 0`.
  - **No `tx`:** today's tail (`rest = period.saturating_sub(used).max(1)` idle). Single-line output must be byte-identical to today (`synth_uart_shape` stays green).
  - **With `tx`:**
    1. `tx_frame` = the same lead + bytes on bit `t`, never glitched.
    2. `offset = max(period/2, rx_used)`; `total = max(period, offset + tx_used + 1)`.
    3. `rx_list` = RX segments + idle (`1<<ch`) to `total`.
    4. `tx_list` = idle (`1<<t`) for `offset` + `tx_frame` + idle to `total`.
    5. `segs = merge_segs(&rx_list, &tx_list)`.

### worker.rs
- **`Counters::json`:** add `"error_rows"`.
- **`batch_annotations`:** when a row is accepted by the rate guard and `row["role"] == "error"` → `counters.error_rows += 1`.
- **`state`:** add `"decoder_tracks": self.cfg.decoder.as_ref().map(model::Decoder::tracks)` (null without a decoder).

### cli.rs
`to_call` `start`: add `("glitch-every", "glitch_every")` to the u64 mapping list. Validation stays in `StartConfig::from_params`.

### B4r acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-logic --no-fail-fast        # all green: unit 15, archive 16, archive_engine 3, cli_archive 3, duplex 6, engine 10, gateway 4, ui_decoder_specs 2; load 2 ignored
cargo clippy -p loom-logic --all-targets -- -D warnings   # AMENDED: only these 4 B5-scaffold diagnostics may remain (lead's scaffold, consumed by B5r): worker.rs `unused variable: base` (read_lines), model.rs `fields mask, edge, prev never read` (TriggerDetector), model.rs `fields cap, buf, end never read` (PreRing), worker.rs `field trigger_view never read`
cargo check -p loom-logic --no-default-features --features cli
grep -rn 'B4-M[0-9].*Sonnet' crates/loom-logic/src   # empty (B5-* skeletons/markers stay: they belong to B5r)
```

---
## B5r — software trigger (Rust, finding L7) · PLAN_READY_BATCH5R (FROZEN)
**Lease (bodies only):** `crates/loom-logic/src/model.rs`, `worker.rs`, `cli.rs`, `archive.rs` — B5 items only.

**Opus-owned:**
- `tests/trigger.rs`, `tests/cli_archive.rs`, `tests/fixtures/*`, all other tests
- `gateway.rs` (schema/help done)
- every signature, field, type and struct (including `TrigSide`)

**Design:**
- Loom detects the trigger in software on the acquired byte stream, for every source. While armed:
  - samples are acquired and counted (`counters.samples`) and kept only in a bounded pre-ring (≤ `pre_ms`, ≤ 10 M samples)
  - nothing is published, decoded or archived
- On the trigger at absolute sample T:
  - `first = ring start`
  - edges / decoder / archive start at `first` on the **absolute axis**: `EdgeAcc.sample = first`, decoder rows shifted by `first`, `RawEncoder::new(first)`
  - the capture stops after `T + post_samples` with reason `trigger_post`
- Not fired → nothing published; the archive range is empty and exact; trigger state `not_fired`.
- Hardware triggers are not used (L7-3 is an external dependency).

### model.rs
- **M1 `from_params`:** `trigger = match o.get("trigger") { None | Some(Null) => None, Some(v) => Some(Trigger::parse(v, duration_ms)?) }`, after `duration_ms` is validated. `to_params`: `v["trigger"] = t.to_json()` when `Some`, for every source.
- **M2–M9:** per the doc comments.
  - **M3:** unknown key → Usage `"trigger option {k:?}: expected channel, edge, pre_ms, post_ms"`; channel via the existing `chan()`; non-object → Usage `"trigger: expected an object"`; required-key messages are `"trigger.<k> is required"`; bounds per the doc with messages naming the key.
  - **M6:** `prev = Some(last byte)` after every non-empty call.
  - **M7:** `cap == 0` keeps nothing.

### worker.rs
- **W1 `start`:** `trigger_view` = `cfg.trigger` → `{"state":"armed","sample":null,"channel","edge","pre_ms","post_ms"}` (the `to_json` keys merged in), else `Null`. Remove the `B5-W1` comment.
- **W5 `state`:** add `"trigger": lock(&self.trigger_view).clone()`.
- **W3 `bound` / W4 `publish`:** per the doc. `publish` is the second half of the old `pump`; `bound` is its first half plus `post_end`.
- **W2 `feed`:**
  1. **First acquired, non-empty chunk:** `*running = true`, refresh the archive view, `state("running")`. With a trigger this state carries `"armed"`.
  2. **No trigger, or already fired:** `(c, why) = bound(chunk, post_end)`; `publish(&c, …)`; return `why`.
  3. **Armed:**
     1. `(c, why) = bound(chunk, None)`; `base = samples − c.len()` (absolute index of `c[0]`).
     2. `det.feed(&c)`:
        - `None` → `ring.push(&c)`; return `why`.
        - `Some(i)`:
          1. `ring.push(&c[..i])`; `(first, pre) = ring.take()`; `T = base + i`.
          2. Set `fired_at` and `post_end = T + t.post_samples(rate)`.
          3. `acc.sample = first`.
          4. `self.sample_base.store(first, Release)`, before any byte reaches the tee.
          5. If `arch`: `a.enc = RawEncoder::new(first)`; `a.sink.set_first_sample(first)`.
          6. `trigger_view` = fired (`sample: T`), then `state("running")` again.
          7. `publish(&pre)`.
          8. Apply the post bound to `rest = c[i..]`: truncate it to `post_end − T`; if it was truncated, `why2 = Some("trigger_post")`.
          9. `publish(&rest)`; return `why.or(why2)`.
- **main loop:**
  - Build `trig: Option<TrigSide>` from `cfg.trigger` (`ring = PreRing::new(t.pre_samples(rate))`).
  - Both former `pump` call sites → `feed(..)`, setting `reason` from the returned value (keep `sample_bound_hit` semantics: any `Some` stops the loop; drained chunks after a bound are not used).
  - Delete `pump`.
  - **Stop:** if a trigger exists and never fired:
    - `trigger_view` → `{"state":"not_fired","sample":null,…}`
    - with `arch`: `a.enc = RawEncoder::new(samples)` and `a.sink.set_first_sample(samples)` before the existing archive finish
  - The final state carries the view.
  - The duration bound still uses wall time from start (armed time included).
- **`read_lines`:** `let a = parse_annotation(line).shifted(base.load(Ordering::Acquire));`. The tap and `try_send` use the shifted annotation.

### archive.rs
- **S8 `set_first_sample`:** store `shared.first_sample`, set `first_set`.
- **H6:** when writing any manifest, if `first_set`: `raw_first_sample = first_sample`; and while no block has been indexed, `raw_end_sample = first_sample`. At Finish this keeps the not-fired case `first == end == acquired` (complete).
- **Q3/Q4 and export:** unchanged. They already default `from` to `raw_first_sample`.

### cli.rs
- In `to_call` `start`: if `--trigger "Dn:edge"` is given, `split_once(':')` → `{"channel": Dn, "edge": edge, "pre_ms": ms(--pre or 0), "post_ms": ms(--post)}`.
  - No `:` → Usage `"--trigger: expected CH:EDGE (e.g. D3:rising)"`.
  - Missing `--post` → Usage `"--trigger needs --post"`.
- All other validation stays in `StartConfig::from_params`.

### B5r acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-logic --no-fail-fast        # all green: unit 15, archive 16, archive_engine 3, cli_archive 4, duplex 6, engine 10, gateway 4, trigger 6, ui_decoder_specs 2; load 2 ignored
cargo clippy -p loom-logic --all-targets -- -D warnings   # fully clean now (the 4 B5 scaffold diagnostics must be consumed)
cargo check -p loom-logic --no-default-features --features cli
grep -rn 'unimplemented!(\|B5-W1\|(Sonnet)' crates/loom-logic/src   # empty
```

---
## B45u — UI for duplex UART, error rows, software trigger (findings L6, L7) · PLAN_READY_BATCH45U (FROZEN)
**Lease (bodies + function-local DOM only):** `ui/src/panels/logic/logicModel.ts`, `logicPanel.ts`, `waveform.ts`.

**Opus-owned:**
- all tests (`logicBatch45.test.ts` new), `logic.css`
- every type, constant, import and option table (`TRIGGER_EDGE_OPTS`, `TRIGGER_WAIT_TEXT`, `UART_LINE_OPTS` with `both`)
- `registerPanel`

The lead already removed the B3 `copyHeader` import together with its `void` line.

### logicModel.ts
- **U1 `readCounters`:** `errorRows: Object.prototype.hasOwnProperty.call(c, 'error_rows') ? (num(c.error_rows) ?? 0) : undefined`.
- **U2 `readCaptureState`:**
  - `decoderTracks: has('decoder_tracks') ? readDecoderTracks(p.decoder_tracks) : (prev?.decoderTracks ?? null)`
  - `trigger: has('trigger') ? readTriggerInfo(p.trigger) : (prev?.trigger ?? null)`
- **U3 `readOneAnnotation`:** `role: a.role === 'data' || a.role === 'error' ? a.role : null`. The key is unchanged.
- **U4 `decoderSpec`:** `uartLine === 'both'` → `uart:baudrate=B:rx=D{uartRx}:tx=D{uartTx}` + the same non-default options.
- **U5 `validateConfig`** additions:
  - **UART channels:** `rx` checks RX, `tx` checks TX, `both` checks RX then TX (existing messages). `both` with `uartRx === uartTx` → `'UART RX와 TX가 같은 채널입니다'`.
  - **Glitch** (after the synthetic-UART check): when `glitchEvery !== 0`:
    - not (synthetic && uart) → `'글리치는 합성 UART에서만 쓸 수 있습니다'`
    - else not an integer in 1..1000 → `'글리치 주기는 1–1000 (정수, 0=끔)'`
  - **Trigger** (only when `triggerOn`), at the end:
    - channel range → `'트리거 채널이 캡처 채널 밖입니다'`
    - pre not an integer in 0..1000 → `'트리거 앞 구간 0–1000 ms (정수)'`
    - post not an integer in 1..`Math.round(durationS*1000)` → `'트리거 뒤 구간 1 ms – 캡처 시간 (정수 ms)'`
- **U6 `startParams`:**
  - `glitch_every` iff synthetic && uart && `glitchEvery > 0`
  - `trigger: {channel: 'D'+triggerCh, edge, pre_ms: triggerPreMs, post_ms: triggerPostMs}` iff `triggerOn`
- **U7 `cliLine`:**
  - `--glitch-every N` right after `--frame-period …`, same condition as U6
  - `--trigger D{ch}:{edge} --pre {pre}ms --post {post}ms` appended last, iff `triggerOn`
- **U8 `estimatePublishLoad`**, `uartLine === 'both'`:
  - **Synthetic:** `f = (2+10B)/baud`, `frameEff = max(periodS, max(periodS/2, f) + f)`, `perLine = B/frameEff`, `annotationsPerS = 2·perLine`, edges = `10·perLine` for each shown line (RX, TX).
  - **Device/demo:** `perLine = baud/frameBits`, `annotationsPerS = 2·perLine`, edges = `perLine·frameBits` per shown line.

  Single-line behaviour is unchanged. Warnings follow the same rules.
- **U9 (M1–M4):** `readDecoderTracks`, `trackLabel`, `readTriggerInfo`, `triggerText` per the doc comments and tests.
  - `readTriggerInfo`: state ∈ {armed, fired, not_fired} else `'unknown'`; `sample` / `pre_ms` / `post_ms` via `num`; `channel` / `edge` via `str`.

### waveform.ts (`paint`)
- **W3a:** decoded-track gutter label `` `⟦${inp.decoderLabels?.[dec] ?? dec}⟧` ``.
- **W3b:** an annotation box with `a.role === 'error'` (not selected) uses `pal.sel` for fill and stroke instead of `pal.ann`. Selected boxes keep the selected style.
- **W3c:** if `inp.triggerSample != null` and `x = (T - s0)/spp` lies in `[0, width]`: `strokeStyle = pal.cursor`, `lineWidth = 1.5`, `setLineDash([])`, `beginPath`, `moveTo(GUTTER + Math.round(x) + 0.5, AXIS_H)`, `lineTo(same, heightPx)`, `stroke`; then `fillStyle = pal.cursor`, `fillText('T', GUTTER + Math.round(x) + 3, AXIS_H + 18)`. Draw it after the cursors.

### logicPanel.ts (inside `createLogicAnalyzer`)
- **P1 DOM:**
  - `fGlitch = inp('glitch_every', 'number')` (min 0, max 1000, step 1) → `lGlitch = field('글리치 (n번째 바이트마다 프레임 오류, 0=끔)', fGlitch)`
  - `fTrigOn = inp('trigger_on', 'checkbox')` → `lTrigOn = field('소프트웨어 트리거', fTrigOn)`
  - `fTrigCh = sel('trigger_ch', pinOpts())` → `'트리거 채널'`
  - `fTrigEdge = sel('trigger_edge', TRIGGER_EDGE_OPTS)` → `'트리거 에지'`
  - `fTrigPre = inp('trigger_pre_ms', 'number')` (0..1000) → `'트리거 앞 (ms)'`
  - `fTrigPost = inp('trigger_post_ms', 'number')` (min 1) → `'트리거 뒤 (ms)'`
  - `cfgGrid` order: …, lText, lPeriod, **lGlitch**, lRate, lDuration, **lTrigOn, lTrigCh, lTrigEdge, lTrigPre, lTrigPost**, lDecoder, …
  - `trigChip = el('span', 'lg-trigger')`, testid `logic-trigger`, hidden. Status order: `capSel, stateChip, srcBadge, trigChip, meta, stopBtn`.
- **P2 `writeForm` / `readForm`:** the six new fields. The checkbox uses `checked`; numbers use `Number(value)`; `triggerEdge` is cast.
- **P3 draw:**
  - **Visibility:**
    - `lRx.hidden = decoder !== 'uart' || uartLine === 'tx'`
    - `lTx.hidden = decoder !== 'uart' || uartLine === 'rx'`
    - `lGlitch.hidden = !(source === 'synthetic' && decoder === 'uart')`
    - the four trigger detail labels hidden unless `triggerOn`
  - **Trigger chip:** `t = info?.trigger`; `trigChip.hidden = !t`; `textContent = triggerText(t)`; `dataset.state = t?.state ?? ''`.
  - **Empty text:** `info?.trigger?.state === 'armed'` → `TRIGGER_WAIT_TEXT` (before the other cases).
  - **Counters:** after `dropped_annotations` add `['error_rows', `` `디코드 오류 ${fmtN(errs)}` ``, errs > 0]`, where `errs = Math.max(k?.errorRows ?? 0, heldErr)`. `heldErr` counts `shown.annotations` with role `'error'`; recompute it only when the table key changes and cache it in a local.
  - **Labels:** `labels = Object.fromEntries(decs.map((d) => [d, trackLabel(d, info?.decoderTracks)]))` → `paint({…, decoderLabels: labels, triggerSample: info?.trigger?.state === 'fired' ? info.trigger.sample : null})`.
  - **waveHost aria-label:** `디코드 트랙 ${decs.map((d) => labels[d]).join(', ') || '없음'}`.
  - **Paint key:** add the trigger sample and the joined labels.
- **P4 row table** (`makeRowTable`): the decoder cell is `trackLabel(a.decoder, current()?.info?.decoderTracks)`; `rowClass: (a) => (a.role === 'error' ? 'lg-annerr' : '')`.

### B45u acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic/ui
npx vitest run src/panels/logic        # all green: 100 (91 + B45u 9) + the shared fixture duplex case
npx tsc -p tsconfig.app.json  --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
npx tsc -p tsconfig.node.json --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/panels/logic            # 0 problems
npx vitest run src/shell/logicWiring.test.ts src/shell/moduleApps.test.tsx src/shell/serviceSeparation.test.tsx src/sdk/capacityLive.test.ts   # 19/19
grep -n "Sonnet body pending" src/panels/logic/*.ts   # empty
```

---
## Next batches (contracts are written before each lease; architecture is in PLAN §4)
- **B2b (Rust wiring; scaffold already in place, contract in progress):** worker (`setup_archive`, tap, pump encoding, stop-order finish, capture_state `archive`, archive/export methods, `on_call` arms, start arg parse), gateway (`archive_args`, Start arg, 3 handlers), CLI (`archive read`, `export`, `to_call` arms), `tests/archive_engine.rs` (real sigrok-cli synthetic: archive rows == decoded total) + `tests/load.rs` (ignored, measured).
- **B4u (UI, after the B3 review):** uartLine `both` (decoderSpec `uart:baudrate=B:rx=Da:tx=Db…`, synthetic allowed), track labels from capture_state `decoder_tracks` (e.g. `uart-1 RX D0`), `role:'error'` rows highlighted (table row class, red waveform box), counter `error_rows`, glitch option for synthetic, shared fixture gets a duplex case.
- **B45u (UI, after B5r):** duplex/glitch/trigger form, row roles, track labels, error counter, trigger chip/marker/empty text (scaffold + `logicBatch45.test.ts` already in place); skill doc update by the lead.

## B1 frozen inputs (sha256 at PLAN_READY_BATCH1; Sonnet must not change these files)
```
5e196607d191e8a53d65e2355172ddd907183c04c946acc5c212256ff1eebd90  ui/src/panels/logic/logicBatch1.test.ts
6da4129575a05055f7a0eed8f914d6f82db843e8e8a3bc0f316eea5f8ef20220  ui/src/panels/logic/logic.test.ts
3c2d8b44cd4eaa47eb2696960ae2eb79d3a5324efd456005caf8305328e6f138  ui/src/panels/logic/logicModule.test.ts
ff3ce56e6d4141869eefe4b73ee8e2919357b727b15ec89b5e7e0aea13d5ef16  crates/loom-logic/tests/fixtures/ui-decoder-specs.json
ef0d39df7bbb5cafb02734aabd9c34a3f91cf1e4e4cfb0bde016e14e3b574f41  crates/loom-logic/tests/ui_decoder_specs.rs
7ea545b4abab75f3b63ce5da754fbd1884408ddfdbdaaa479a94fa883f5de3a9  ui/src/panels/logic/logic.css
```
Scaffold baseline (`git diff` of tracked files at handoff): sha256 `06a7e2c5ff184ddb9cda8e19a06e9922a91007d706af519453046218c97a34e5`.
State at handoff: isolated tsc app 0 / node 0 errors (work/typecheck caches); logic tests 31 pass + 29 B1 spec fail (red by design); eslint 17 unused scaffold symbols (logicPanel.ts) that the B1 bodies consume; `cargo test --test ui_decoder_specs` 2/2 pass.

## B2a frozen inputs (sha256 at PLAN_READY_BATCH2A)
```
58293b846e41e579a72d2f96b655d397c3031ffe296655c15aaba5e01d20b187  crates/loom-logic/tests/archive.rs
0e3fa63947a1a09aba3c6cacb691522c555da2823f47e076999dce06645d4486  crates/loom-logic/src/archive.rs   (scaffold baseline = lease start)
```
State at handoff: `cargo test --test archive` 0/13 (red by design); every other loom-logic test is green; the crate compiles with unused-scaffold warnings in worker.rs/gateway.rs/cli.rs (B2b).

## B2b + B2a.1 frozen inputs (sha256 at PLAN_READY_BATCH2B; tests/lib Opus-owned, src = lease baselines)
```
5f2a454239418ac0b67efcc4ddd01a71519e4fe707195bfe6ef718a930502b83  crates/loom-logic/tests/archive.rs
87ba04260b4c37ea9f181ad88b20e86222b3e70ff2fde8bc6a9a5c35e1dcf137  crates/loom-logic/tests/archive_engine.rs
571d5607a27d304f813d4019ecedcc0040f68c0dffa6022ec269bd99c596d462  crates/loom-logic/tests/cli_archive.rs
87bc5afc3a36c0d5ad82c038bf5513a592b5ae5cfba06ef7e328a94bdb5435bb  crates/loom-logic/tests/load.rs
69d1efb0b190f2349ae3163011b30d05231c07305cf8ce95dfa62f472f93fe93  crates/loom-logic/tests/engine.rs
4aec594b0c989ab602a442718dbde90386a28c8f3da9d2522934b19e8df8b62c  crates/loom-logic/src/archive.rs
d0208cf342c8601f577888566c29ea478049df4cf0784788a87bc5c2268bf3e6  crates/loom-logic/src/worker.rs
6664317e8c86c0fdb6af6c3762ed88d578d6db799fce72337fb9dc9e214c3712  crates/loom-logic/src/gateway.rs
9c4692ddc813eb008a01fa0a763b779afd11d58665efb540bd7e2f74da7d5eae  crates/loom-logic/src/cli.rs
a922e569e5e1cd1002d6dc7b6c46f1e4a9aea7d2461b8dba7846e68ef9b3931c  crates/loom-logic/src/lib.rs
```
State at handoff: archive 13/16 (3 B2a.1 red), archive_engine 0/3, cli_archive 0/2 (red by design); unit 15, engine 10, gateway 4, ui_decoder_specs 2 green.

## B3 frozen inputs (sha256 at PLAN_READY_BATCH3; tests + css Opus-owned, others = lease baselines)
```
99e0706a895e4002c977e21808973aa2b389dd670dc5b324a3332e4fb47a1613  ui/src/panels/logic/logic.test.ts
5e196607d191e8a53d65e2355172ddd907183c04c946acc5c212256ff1eebd90  ui/src/panels/logic/logicBatch1.test.ts
33b9af2845330559fc6bc9b232258ab6073dd5802913bf4d7034374a936314d5  ui/src/panels/logic/logicBatch3a.test.ts
8556f30eaed5a63119d5610035146fe9780abc34578e8e8f9482970fb0701d33  ui/src/panels/logic/logicBatch3b.test.ts
00b2039165edb84c8442dcf64e895a69177d3c474b77ac2a100cd2db1991c35d  ui/src/panels/logic/logicModel.ts
3c2d8b44cd4eaa47eb2696960ae2eb79d3a5324efd456005caf8305328e6f138  ui/src/panels/logic/logicModule.test.ts
2465478fd30db50a75341078b77645ea950e2ab7b6990fd036dc56d45fa9de17  ui/src/panels/logic/logicPanel.ts
d6298070ccdb5f0a51b8d3c396606cbb8434a5bf6950796c7dff8a75368d6d16  ui/src/panels/logic/logicTools.ts
94d08d77671fa5227acc51c82114a090824e83f16218e2c08ba5cc2349a882eb  ui/src/panels/logic/waveform.ts
849fa3aaf681637bd640a3691df91d50270f70da93bc46da0fd8c16f097e3f64  ui/src/panels/logic/logic.css
```
State at handoff: isolated tsc app/node 0; vitest logic 59 pass / 32 red (B3a 19 + B3b 12 + the amended class assertion); eslint 33 = unused scaffold imports/constants consumed by the B3 bodies.

## B4r frozen inputs (sha256 at PLAN_READY_BATCH4R)
```
b48685c6e204544fc3cd8b12c08d06a092a5ade78ab70d49afc2d007d939d5f3  crates/loom-logic/src/model.rs
88f554254f1783b2f3f531d6bf7656ce790e266a220adb6ed764cf9441ac7937  crates/loom-logic/src/worker.rs
f46e53dbcb7cfaab6e31064f30b18491e61b740395711e3ff2fc4f2ce442232d  crates/loom-logic/src/cli.rs
53d9d1e9cbbb93815c494176ba2995790876b35e8cd399e22f986851b09e8ed1  crates/loom-logic/tests/duplex.rs
54a4867a0ac459f9fc3a385b768307467bc460dfa29787d4db1147138c223e96  crates/loom-logic/tests/cli_archive.rs
```
State at handoff: duplex 0/6 and cli_archive 2/3 red by design; every other target green.

## B5r frozen inputs (sha256 at PLAN_READY_BATCH5R)
```
ab1bec10ffd2fb2a8424da07e1924b2845b1a3910fee8f649b114d42350ffc4a  crates/loom-logic/src/archive.rs
0f19cdb50772dff3fa6dc9af630ab429c7b93018731d4687e4509e024cb6701f  crates/loom-logic/src/cli.rs
dee51541b47aa7276582d84779c53db50660127fbcb94ab10903a419db503e4e  crates/loom-logic/src/gateway.rs
a922e569e5e1cd1002d6dc7b6c46f1e4a9aea7d2461b8dba7846e68ef9b3931c  crates/loom-logic/src/lib.rs
f96482fea10f809cc7d3c8b857ed815a9cdfa5a113d5e8392e20ddbd8b82339d  crates/loom-logic/src/model.rs
5df2b743981908e070cbd93e10f26685c6c6a6345f20dd1ed8e6fb9065d99f6a  crates/loom-logic/src/worker.rs
e9b2c856eaf65a3bb8cb286a7cd68901b0d57b4079d8d330d71368eeb43ee078  crates/loom-logic/tests/trigger.rs
94d51fe2c994815d0c718bbcfdbac148986119b4ace3cb688f4ae9dc7444d37a  crates/loom-logic/tests/cli_archive.rs
ff6d51e40151163b86b6fabcaba853d6ee9d51450ee4d44413f974a75e33ec49  crates/loom-logic/tests/fixtures/ui-decoder-specs.json
```
State at handoff: trigger 0/6 and cli_archive 3/4 red by design; every other target green.

## B45u frozen inputs (sha256 at PLAN_READY_BATCH45U)
```
99e0706a895e4002c977e21808973aa2b389dd670dc5b324a3332e4fb47a1613  ui/src/panels/logic/logic.test.ts
5e196607d191e8a53d65e2355172ddd907183c04c946acc5c212256ff1eebd90  ui/src/panels/logic/logicBatch1.test.ts
33b9af2845330559fc6bc9b232258ab6073dd5802913bf4d7034374a936314d5  ui/src/panels/logic/logicBatch3a.test.ts
8556f30eaed5a63119d5610035146fe9780abc34578e8e8f9482970fb0701d33  ui/src/panels/logic/logicBatch3b.test.ts
7d7cbd64d2a6e3ecae66141c1e038aafdb4c4df84a261ba836e2b63a227dca93  ui/src/panels/logic/logicBatch45.test.ts
33eae54b39d3854db59dbb1fc8fb3949b24c8afcf0ea79051975510bf81d0514  ui/src/panels/logic/logicModel.ts
3c2d8b44cd4eaa47eb2696960ae2eb79d3a5324efd456005caf8305328e6f138  ui/src/panels/logic/logicModule.test.ts
7489f4ba31fd031ed17f4c2c8f02134dbe6fc352cdd0318c87dd1bcc33a3c94e  ui/src/panels/logic/logicPanel.ts
f53860c65000011e74c8a0c093cb28522364158a5d76f2bb833062364cec1fb4  ui/src/panels/logic/logicTools.ts
ddebe3d4202000fa3b25f2d0363955941b1b70a97bc2d5ed18693d70d07237eb  ui/src/panels/logic/waveform.ts
aa5847511efd289ae61f82345997e85f06020f28914a24b8bdb0965b9c74be07  ui/src/panels/logic/logic.css
ff6d51e40151163b86b6fabcaba853d6ee9d51450ee4d44413f974a75e33ec49  crates/loom-logic/tests/fixtures/ui-decoder-specs.json
```
State at handoff: isolated tsc 0; vitest logic 90 pass / 10 red (B45u 9 + the shared-fixture duplex case in logicBatch1); everything else green.

---

## B6 — bounded archive row window (ROOT browser defect D-L5W) · PLAN_READY_BATCH6 (FROZEN)

**Defect (ROOT, first3 build):** archive mode on `lc-1a1e0eef471-1` (27,499 archived rows). Clicking `디코드 더 불러오기` to EOF left 20,000 rows: `CaptureData.addAnnotations` evicted the first 7,499 (`MAX_ANNOTATIONS_PER_CAPTURE`) while `archRowsDone = true`. The label said `아카이브에서 불러옴 (전체 27,499행)`, and copy used `archiveViewCoverage(a, archRowsDone)`, which cannot see the eviction, so it could claim `coverage=complete`. The archive data itself is complete.

**Fix:** the archive view holds one bounded window of consecutive archive rows (≤ `ARCHIVE_WINDOW_ROWS` = 20,000 = the per-capture cap, so it never evicts):
- previous / next / seek move the window;
- the label names the held range, the total and the cap;
- search and copy say their scope;
- coverage is `complete` only when the window really holds every row.

Archive row numbers are the daemon's 0-based `row` (`next_row` = last row returned; `null` at the end; `after_row` = rows after it). This was verified on the live daemon 2026-11-16. The UI shows rows 1-based.

**Baseline:** the imported first3 module (`fix-archive-window/BASELINE.sha256`).

**Lease (bodies + function-local DOM only):**
- `ui/src/panels/logic/logicModel.ts`: B6-M1…M10 bodies only.
- `ui/src/panels/logic/logicTools.ts`: the `copyHeader` body only (B6-T1).
- `ui/src/panels/logic/logicPanel.ts`: the archive-view state, closures, function-local DOM, draw and events below.

**Opus-owned:** `logicBatch6.test.ts`, `logicBatch3b.test.ts` (one amended label assertion), `logic.css`, all types/constants/signatures/imports. Unused imports after the bodies are handled by Opus at review; in particular, Opus removes `archiveViewCoverage` from the panel import when it becomes unused. Do not edit imports.

### Model (logicModel.ts) — exact rules are in each doc comment
| id | function | rule summary |
|---|---|---|
| B6-M1 | `archiveWindowStart(startAfter)` | `{startAfter, first: null, last: null, held: 0, eof: false}` |
| B6-M2 | `archiveWindowAdd(w, page)` | new object; base = (w.last ?? w.startAfter ?? −1)+1; count 0 → `{...w, eof: page.nextRow === null}`; else first = w.first ?? base, last = page.nextRow ?? base+count−1, held += count, eof = page.nextRow === null |
| B6-M3 | `archiveWindowAfterRow(w)` | `w.last ?? w.startAfter` |
| B6-M4 | `archiveWindowLimit(w)` | `max(0, min(ARCHIVE_PAGE_ROWS, ARCHIVE_WINDOW_ROWS − w.held))` |
| B6-M5 | `archiveWindowFull(w)` | `w.held >= ARCHIVE_WINDOW_ROWS` |
| B6-M6 | `archivePrevStart(w)` | null if first null/0; start = max(0, first − ARCHIVE_WINDOW_ROWS) → `{startAfter: start === 0 ? null : start − 1}` |
| B6-M7 | `archiveNextStart(w)` | null if eof or last null; else `{startAfter: w.last}` |
| B6-M8 | `archiveSeekStart(row1, total)` | null unless finite integer ≥ 1 and (total null or ≤ total); start0 = row1−1 → `{startAfter: start0 === 0 ? null : start0 − 1}` |
| B6-M9 | `archiveWindowCoverage(a, w, evicted)` | unknown / live / complete only when complete state ∧ startAfter null ∧ first ∈ {null,0} ∧ eof ∧ evicted 0 ∧ row count agrees; else partial |
| B6-M10 | `archiveWindowText(w, total, busy)` | exact strings in the doc comment (`행 1–20,000 / 전체 27,499행 · 뒷부분 미표시 (창 최대 20,000행)`, …), numbers `toLocaleString('ko-KR')` |

### Tools (logicTools.ts)
- **B6-T1 `copyHeader`:** after the `# coverage=…` line, when `meta.archiveRows` is defined, add `` `# archive_rows=${first}-${last}/${total ?? '?'}\n` ``. Then the radix line as before. Nothing else changes.

### Panel (logicPanel.ts)
- **P-B6-1 state:** delete `archRowsNext` and `archRowsDone`; add `let archRows: ArchiveRowWindow = archiveWindowStart(null);`. Every former use moves to `archRows`.
- **P-B6-2 DOM (archBar):**
  - `prevBtn = btn('◀ 이전 구간', '아카이브의 앞쪽 창 (최대 20,000행)', 'logic-archive-prev')`
  - `nextBtn = btn('다음 구간 ▶', '아카이브의 다음 창 (최대 20,000행)', 'logic-archive-next')`
  - `seekInp = tid(inp('archive_seek', 'number'), 'logic-archive-seek')`, with `min = '1'`, `step = '1'`, `placeholder = '행 번호'`, class `lg-archive-seek` added (`classList.add`).
  - `seekBtn = btn('이동', '이 행부터 창을 엽니다 (최대 20,000행)', 'logic-archive-seek-go')`
  - `winLabel = el('span', 'lg-archive-window')`, testid `logic-archive-window`, `role="status"`.
  - prevBtn, nextBtn, seekInp and seekBtn also get class `lg-archive-nav`.
  - moreBtn keeps its label; its title becomes `'이 창에 아카이브 행을 더 불러옵니다 (창 최대 20,000행)'`.
  - Append order: `chip, srcSel, prevBtn, moreBtn, nextBtn, seekInp, seekBtn, winLabel, expKind, expBtn, expLine`.
- **P-B6-3 `openWindow(startAfter: number | null)`:**
  1. Return if `archFor === null || !archCap`.
  2. `archGen++`; `const keep = archCap.info`; `archCap = new CaptureData(archFor)`; `archCap.info = keep`.
  3. `archRows = archiveWindowStart(startAfter)`; `archRowsBusy = false`; `archEdgesBusy = false`; `archWin = null`; `archErr = null`; `selected = null`; `searchIdx = -1`; `searchKey = ''`.
  4. Invalidate; `void loadRows()`.
  5. Cursors and follow are unchanged (follow is already off in archive mode).
- **`resetArchiveView(c)`:** as before, with `archRows = archiveWindowStart(null)` in place of the two deleted variables.
- **P-B6-4 `loadRows()`:**
  - Guard: `archRowsBusy || archRows.eof || archiveWindowFull(archRows) || !archCap` → return. Capture gen/id/cap as before; `archRowsBusy = true`.
  - Loop at most `ARCHIVE_ROW_PAGES` times:
    1. `limit = archiveWindowLimit(archRows)`; if 0, break.
    2. `after = archiveWindowAfterRow(archRows)`.
    3. `r = await ctx.query('logic.archive.read', {capture_id: id, kind: 'annotations', limit, ...(after !== null ? {after_row: after} : {})})`.
    4. Gen check (stale → return).
    5. `pg = readArchiveRows(…)` as before; `cap.addAnnotations(pg.rows, 0)`; `cap.addRawLines(pg.rawLines, pg.rejected)`; `archRows = archiveWindowAdd(archRows, pg)`; invalidate.
    6. Break if `archRows.eof || archiveWindowFull(archRows) || pg.count === 0`.
  - The errors and `finally` handling are unchanged.
- **P-B6-5 draw:**
  - `navHidden = viewSrc !== 'archive'`. `prevBtn.hidden = nextBtn.hidden = seekInp.hidden = seekBtn.hidden = winLabel.hidden = navHidden`.
  - `moreBtn.hidden = navHidden || archRows.eof || archiveWindowFull(archRows)`; `moreBtn.disabled = archRowsBusy`.
  - `prevBtn.disabled = archRowsBusy || archivePrevStart(archRows) === null`; `nextBtn.disabled = archRowsBusy || archiveNextStart(archRows) === null`; `seekBtn.disabled = archRowsBusy`.
  - `total = c?.info?.archive?.annotationRows ?? null`; `winLabel.textContent = archiveWindowText(archRows, total, archRowsBusy)`.
  - **annInfo (archive mode)** replaces the B3b text:
    `` `${fmtN(shown.annotations.length)}행 · 아카이브에서 불러옴 · ${archiveWindowText(archRows, total, archRowsBusy)}${archErr ? ` · ${archErr}` : ''} · 디코더 ${decs.join(', ') || '—'} · 해석 결과(원시 아님)` ``
  - **Search status:** in archive mode (`shown === archCap`), with `searchRes` present and no error, `archRows.first`/`last` non-null, and `archiveWindowCoverage(c?.info?.archive, archRows, archCap.evictedAnnotations) !== 'complete'`: append `` ` · 창 행 ${fmtN(first + 1)}–${fmtN(last + 1)} 안에서` `` to the existing `i/n` text.
- **P-B6-6 copy meta:**
  - `coverage`: `archiveWindowCoverage(live.info?.archive, archRows, c.evictedAnnotations)` in archive mode; otherwise unchanged.
  - Archive mode with first/last non-null: also `archiveRows: {first, last, total: live.info?.archive?.annotationRows ?? null}`. Spread it in only then, so the key is absent otherwise.
- **P-B6-7 events:**
  - `prevBtn` click → `s = archivePrevStart(archRows)`; `if (s) openWindow(s.startAfter)`. `nextBtn` works the same way with `archiveNextStart`.
  - `go()`: `t = current()?.info?.archive?.annotationRows ?? null`; `s = archiveSeekStart(Number(seekInp.value), t)`.
    - `!s` → `` ctx.ui.toast(`행 번호는 1–${t === null ? '?' : fmtN(t)} 범위의 정수입니다`) `` and return.
    - Otherwise `openWindow(s.startAfter)`.
  - `seekBtn` click → `go()`; `seekInp` keydown Enter → `preventDefault()`, then `go()`.
- **Rules:**
  - The archive view never mixes live rows in.
  - The archive CaptureData must never evict. The limit ensures that; coverage still checks `evictedAnnotations` as a guard.

### B6 acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic/ui
npx vitest run src/panels/logic                      # 110/110 (previous 100 incl. the amended B3b label + B6 10)
npx tsc -p tsconfig.app.json  --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
npx tsc -p tsconfig.node.json --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/panels/logic                          # only `archiveViewCoverage` unused-import may remain (Opus removes it at review)
npx vitest run src/shell/logicWiring.test.ts src/shell/moduleApps.test.tsx src/shell/serviceSeparation.test.tsx src/sdk/capacityLive.test.ts   # 19/19
grep -n "Sonnet body pending" src/panels/logic/*.ts   # empty
```
Write `work/reports/operator-upgrade/logic/B6-WORKER.md`: READY_FOR_REVIEW, the commands and their results, and any interpretations. Do not touch the frozen inputs below.

## B6 frozen inputs (sha256 at PLAN_READY_BATCH6; tests + css Opus-owned, others = lease baselines)
```
99e0706a895e4002c977e21808973aa2b389dd670dc5b324a3332e4fb47a1613  ui/src/panels/logic/logic.test.ts
5e196607d191e8a53d65e2355172ddd907183c04c946acc5c212256ff1eebd90  ui/src/panels/logic/logicBatch1.test.ts
33b9af2845330559fc6bc9b232258ab6073dd5802913bf4d7034374a936314d5  ui/src/panels/logic/logicBatch3a.test.ts
68044627a38b3831418d8875a059af39cd661962f7bd84fa6f2f40b8ddd953d2  ui/src/panels/logic/logicBatch3b.test.ts
7d7cbd64d2a6e3ecae66141c1e038aafdb4c4df84a261ba836e2b63a227dca93  ui/src/panels/logic/logicBatch45.test.ts
58addc27641a86e212eac14f4f817d0c52a5bb2fd17fadaa8dc87146270c35b1  ui/src/panels/logic/logicBatch6.test.ts
6a384b2cfd4fdc3543b05a2831352f018fdf32bf221c7b27f3afe3f16816792a  ui/src/panels/logic/logicModel.ts
3c2d8b44cd4eaa47eb2696960ae2eb79d3a5324efd456005caf8305328e6f138  ui/src/panels/logic/logicModule.test.ts
c38e1bff123f7ef535f39c27a28fead7d18ed01f677e6d968cec8d36ece4e29b  ui/src/panels/logic/logicPanel.ts
40e820873b112e352b5c6191135f405a823fcb9943b4893d97779d2b3bd3888e  ui/src/panels/logic/logicTools.ts
af12497ce1569c4f730b6a25951fa64067fb1201ced46a39eaf3e8852639f773  ui/src/panels/logic/waveform.ts
0829ba850b9bfa3f35b587a7a4338a7b22211bb01a529fed3f924405ef50012b  ui/src/panels/logic/logic.css
```
State at handoff: isolated tsc app 0 / node 0; vitest logic 100 pass / 10 red (B6 9 + the amended B3b label; the B6 constant test is green); eslint: the B6 imports are unused until the bodies consume them.

---

## B6.1 — search selection survives live appends (ROOT follow-up, live marker run) · PLAN_READY_BATCH61 (FROZEN)

**Defect (ROOT, first3, capture `lc-1a1e10534e9-5`):** the panel's search key includes `shown.version`. Every live append therefore re-ran the search and set `searchIdx = -1` (logicPanel draw, search block). The status stayed `0/2`, and Next/Enter could not advance RX → TX while rows kept arriving.

**Baseline:** the B6 result that ROOT imported (`fix-search-follow/BASELINE.sha256` = `fix-archive-window/B6-RESULT.sha256` + unchanged files).

**Lease (bodies only):**
- `ui/src/panels/logic/logicTools.ts`: the `relocateHit` body (B61-T1).
- `ui/src/panels/logic/logicPanel.ts`: the search block in draw, plus one new function-local state variable.

**Do not edit** tests, CSS, imports, types or signatures. The `relocateHit` import is already in place.

- **B61-T1 `relocateHit(prev, hits)`:** −1 if `prev` is null. Otherwise return the index of the first hit with `decoder === prev.decoder && startSample === prev.startSample && endSample === prev.endSample`, or −1 if there is none.
- **P-B61-1 state:** next to `searchKey`, add `let searchQKey = '';`.
- **P-B61-2 draw search block:** replace the `sk` computation and the `if (sk !== searchKey) {…}` block with:
  ```ts
  const qk = `${shown?.id ?? ''}|${viewSrc}|${archGen}|${searchMode}|${searchCase}|${searchPattern}`;
  const sk = `${qk}|${shown?.version ?? 0}`;
  if (sk !== searchKey) {
    const prev = qk === searchQKey && searchRes && searchIdx >= 0 ? (searchRes.hits[searchIdx] ?? null) : null;
    searchKey = sk;
    searchQKey = qk;
    searchRes = /* unchanged searchRows(...) expression */;
    searchIdx = relocateHit(prev, searchRes?.hits ?? []);
  }
  ```
  - The selection is kept by stable identity (decoder + span) across appends, including the archive "더 불러오기" within one window.
  - It resets (−1) when the query (mode, case, pattern), the shown capture, the view source or the archive window (`archGen`) changes, or when the hit is lost.
  - The existing resets in `syncView` (capture switch) and `openWindow` (`searchKey = ''`, `searchIdx = -1`) stay.
- Nothing else changes. Whole-line clipping at 200 characters is an explicit ellipsis and out of scope.

### B6.1 acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic/ui
npx vitest run src/panels/logic                      # 114/114 (B6.1 3 red → green; the query-reset test already green)
npx tsc -p tsconfig.app.json  --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
npx tsc -p tsconfig.node.json --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/panels/logic                          # 0 problems
npx vitest run src/shell/logicWiring.test.ts src/shell/moduleApps.test.tsx src/shell/serviceSeparation.test.tsx src/sdk/capacityLive.test.ts   # 19/19
grep -n "Sonnet body pending" src/panels/logic/*.ts   # empty
```
Write `work/reports/operator-upgrade/logic/B61-WORKER.md` (READY_FOR_REVIEW, results, interpretations).

## B6.1 frozen inputs (sha256 at PLAN_READY_BATCH61)
```
99e0706a895e4002c977e21808973aa2b389dd670dc5b324a3332e4fb47a1613  ui/src/panels/logic/logic.test.ts
5e196607d191e8a53d65e2355172ddd907183c04c946acc5c212256ff1eebd90  ui/src/panels/logic/logicBatch1.test.ts
33b9af2845330559fc6bc9b232258ab6073dd5802913bf4d7034374a936314d5  ui/src/panels/logic/logicBatch3a.test.ts
68044627a38b3831418d8875a059af39cd661962f7bd84fa6f2f40b8ddd953d2  ui/src/panels/logic/logicBatch3b.test.ts
7d7cbd64d2a6e3ecae66141c1e038aafdb4c4df84a261ba836e2b63a227dca93  ui/src/panels/logic/logicBatch45.test.ts
58addc27641a86e212eac14f4f817d0c52a5bb2fd17fadaa8dc87146270c35b1  ui/src/panels/logic/logicBatch6.test.ts
1adf61b24fccb8bb5b0e007c8106d3c666f031b1c5975b8d9d895f3610bd1de7  ui/src/panels/logic/logicBatch61.test.ts
8d49c5a950a34a0cc313e5901946e81469ad13025c3af84cd31b74f163a62e85  ui/src/panels/logic/logicModel.ts
3c2d8b44cd4eaa47eb2696960ae2eb79d3a5324efd456005caf8305328e6f138  ui/src/panels/logic/logicModule.test.ts
5a3db33d61ccd33f0473083fa1bb27ae0c71cde0fce962f0a036da241f6ef16e  ui/src/panels/logic/logicPanel.ts
c654ab7a57d3e1e71eebe3074ce681cb00a7e1a453643a1c6d48f62dae09b1c1  ui/src/panels/logic/logicTools.ts
af12497ce1569c4f730b6a25951fa64067fb1201ced46a39eaf3e8852639f773  ui/src/panels/logic/waveform.ts
0829ba850b9bfa3f35b587a7a4338a7b22211bb01a529fed3f924405ef50012b  ui/src/panels/logic/logic.css
```
State at handoff: tsc app/node 0; vitest logic 111 pass / 3 red (B6.1); eslint: relocateHit unused until the body consumes it.

---

## VT-K1 — shared SDK VirtualTable: keyboard selection stays visible (ROOT-assigned, narrow) · PLAN_READY_VTK1 (FROZEN)

**Root cause (ROOT, live marker run):** the search selected the TX hit at 61,500,026; then 19 × ArrowDown moved the selection, cursor and waveform to 61,501,675–61,501,745, but the selected row stayed off-screen (the visible DOM rows ended at 61,500,633). `ui/src/sdk/virtualTable.ts` `onKey` (lines 305–313) only calls `select()`, while `reveal()` (line 146) also scrolls. The selection was never lost.

**Ownership (new, shared, ROOT-assigned):** only `ui/src/sdk/virtualTable.ts` (the `onKey` body) and `ui/src/sdk/virtualTable.test.ts` (Opus tests).

**Baseline:** main's current SDK (`fix-vt-keyboard/BASELINE.sha256`: virtualTable.ts `7f8f32da…`, test `72dafed3…`), equal to the worktree.

**Lease (Sonnet):** the `private onKey` arrow-function body in `ui/src/sdk/virtualTable.ts`. Nothing else in the SDK: no signature, field, import, option or other method changes.

**VT-K1 `onKey(e)` body (exact):**
```ts
if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Escape') return;   // unchanged
e.preventDefault();                                                               // unchanged
if (e.key === 'Escape') return this.select(null);                                 // unchanged (no scroll, no follow change)
const keys = this.items.map((it) => this.o.keyOf(it));                            // unchanged
const i = this._selected === null ? keys.length : keys.indexOf(this._selected);   // unchanged
const j = e.key === 'ArrowDown' ? Math.min(keys.length - 1, i + 1) : Math.max(0, i - 1);  // unchanged
const key = keys[j];
if (key === undefined) return this.select(null);                                  // empty table: as before
// the user navigates: leave the live tail (once), like a user scroll-back
if (this.follow) {
  this.follow = false;
  this.o.onScrollAway?.();
}
// minimal scroll so row j is fully inside the viewport (no layout height: make it the first row)
const top = j * this.h;
const view = this.scroll.clientHeight;
if (view <= 0) this.scroll.scrollTop = top;
else if (top < this.scroll.scrollTop) this.scroll.scrollTop = top;
else if (top + this.h > this.scroll.scrollTop + view) this.scroll.scrollTop = top + this.h - view;
this.select(key);
this.o.onNeedRender();
```
- `ArrowUp`/`ArrowDown` are already in `SCROLL_KEYS`, so `onScrollKey` (registered first) marks the user input; the scroll this triggers counts as the user's.
- Escape is unchanged, and so are click, `reveal`, `render`, `onScroll` and the follow rules for wheel/drag/layout.
- **Known, unchanged rule:** navigating onto the very last row puts the view at the bottom, and the existing at-bottom check in `onScroll` re-joins the tail.

### VT-K1 acceptance (exact)
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic/ui
npx vitest run src/sdk/virtualTable.test.ts                       # 9/9 (4 red at handoff)
npx vitest run src/sdk src/panels src/shell src/workbench          # all green (VirtualTable consumers: can traceView, eth, logic, uart terminal/trace, uds console, sdk rowTable)
npx tsc -p tsconfig.app.json  --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo
npx tsc -p tsconfig.node.json --noEmit --pretty false --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo
npx eslint src/sdk/virtualTable.ts src/sdk/virtualTable.test.ts src/panels/logic   # 0
```
Write `work/reports/operator-upgrade/logic/VTK1-WORKER.md` (READY_FOR_REVIEW, results, interpretations).

## VT-K1 frozen inputs (sha256 at PLAN_READY_VTK1)
```
7f8f32dab9b1248072fc7d4e61c739e07a64cd02a5e528c36c3f9dd296e6441b  ui/src/sdk/virtualTable.ts
35a7616e25b5caca47ee0710cdc7740e13d26b65d85622a8f928b8efb02038c2  ui/src/sdk/virtualTable.test.ts
99e0706a895e4002c977e21808973aa2b389dd670dc5b324a3332e4fb47a1613  ui/src/panels/logic/logic.test.ts
5e196607d191e8a53d65e2355172ddd907183c04c946acc5c212256ff1eebd90  ui/src/panels/logic/logicBatch1.test.ts
33b9af2845330559fc6bc9b232258ab6073dd5802913bf4d7034374a936314d5  ui/src/panels/logic/logicBatch3a.test.ts
68044627a38b3831418d8875a059af39cd661962f7bd84fa6f2f40b8ddd953d2  ui/src/panels/logic/logicBatch3b.test.ts
7d7cbd64d2a6e3ecae66141c1e038aafdb4c4df84a261ba836e2b63a227dca93  ui/src/panels/logic/logicBatch45.test.ts
58addc27641a86e212eac14f4f817d0c52a5bb2fd17fadaa8dc87146270c35b1  ui/src/panels/logic/logicBatch6.test.ts
1adf61b24fccb8bb5b0e007c8106d3c666f031b1c5975b8d9d895f3610bd1de7  ui/src/panels/logic/logicBatch61.test.ts
8d49c5a950a34a0cc313e5901946e81469ad13025c3af84cd31b74f163a62e85  ui/src/panels/logic/logicModel.ts
3c2d8b44cd4eaa47eb2696960ae2eb79d3a5324efd456005caf8305328e6f138  ui/src/panels/logic/logicModule.test.ts
6eb74f66cdcbeb28e4b4752fce97c3934010246d4c0e79777500bda7564ad06f  ui/src/panels/logic/logicPanel.ts
11f5020888cddd0f7dca00a5171da3bd46db5f1f80ab689019a9018e5f4fc645  ui/src/panels/logic/logicTools.ts
af12497ce1569c4f730b6a25951fa64067fb1201ced46a39eaf3e8852639f773  ui/src/panels/logic/waveform.ts
0829ba850b9bfa3f35b587a7a4338a7b22211bb01a529fed3f924405ef50012b  ui/src/panels/logic/logic.css
```
State at handoff: virtualTable.test.ts 5 pass / 4 red; logic 114/114.

### VT-K1 acceptance — AMENDMENT A1 (ROOT resource scheduling, 2026-11-16; not a permission change)
Host load is high, and concurrent full Vitest pools cause layout timeouts. From now on every Vitest run is **targeted** and uses `--maxWorkers=2`. The full UI suite is ROOT-only, at the final integrated freeze. No assertion or timeout is weakened, and running tests are not killed. This replaces the `src/sdk src/panels src/shell src/workbench` line above:
```
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-logic/ui
npx vitest run --maxWorkers=2 src/sdk/virtualTable.test.ts                                    # 9/9
npx vitest run --maxWorkers=2 src/sdk/m1Sdk.test.ts src/sdk/p1.test.ts src/panels/logic \
  src/panels/eth/eth.test.ts src/panels/eth/ethUnbound.test.ts src/panels/uart/uart.test.ts src/panels/uart/uartFidelity.test.ts \
  src/panels/uds/uds.test.ts src/panels/uds/udsStateHydrate.test.ts src/panels/uds/udsUnbound.test.ts src/panels/uds/udsWorkbench.test.ts \
  src/panels/can/canTrace.test.ts \
  src/shell/logicWiring.test.ts src/shell/moduleApps.test.tsx src/shell/serviceSeparation.test.tsx src/sdk/capacityLive.test.ts   # VirtualTable + logic consumers: all green
```
tsc app/node (isolated caches) and eslint are unchanged. `CARGO_BUILD_JOBS=2` remains. Checks that already passed need no re-run just to use the flag.
