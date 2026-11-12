# UDS / DoIP / EthDiag — benchmark fidelity (Phase 1)

Status: **IMPLEMENTED — READY_FOR_REVIEW (frozen pending MAIN)** · base `26dc5e8` · 2026-11-12 · checkpoint 1 plan → MAIN GO → §7 implementation
Owner: UDS fidelity worker, model **Claude Opus 5.5 (`claude-opus-5-5`)**, foreground, no subagents, no git.
Primary benchmark: **Softing DTS.monaco**. Loom branding stays as is. Loom is not presented as DTS.monaco.

Evidence labels: **[SHOT]** a vendor screenshot I opened and looked at · **[DIAG]** a vendor concept graphic (not a UI screenshot) · **[DOC]** vendor page text, quoted or summarized · **[INF]** my inference · **[CODE]** current Loom source.

## 0. Reference assets (`work/reports/fidelity/assets/uds/`)
All assets were downloaded on 2026-11-12 from the official product page <https://automotive.softing.com/products/softing-dts/softing-dtsmonaco.html>, image path `/fileadmin/_processed_/…`. © Softing Automotive. They are kept only as internal reference evidence and must not be shipped in the product or the report UI. Small feature icons were downloaded once, then deleted.

| file | src path | kind | size | sha256 (prefix) |
|---|---|---|---|---|
| ref-01-diagservice-workspace.png | 8/d/csm_diagservice_pic2_10cd884286.png | [SHOT] "Diagnostic Services" control unit | 3001×2160 | 816048f4 |
| ref-02-quicktest.png | 7/9/csm_Quicktest_pic_b009c3038c.png | [SHOT] "Tool Quick Test" (ECUs / ECU Identification / Errors / Environment Data) | 3840×1625 | 78887cd8 |
| ref-03-workspaces-layouts-controlunits.png | 1/f/csm_Softing_DTS_monaco_workspaces_layouts_controlunits_5059a66fca.png | [DIAG] workspaces → layouts → control units | 880×368 | 041d21e7 |
| ref-04-graphicals.png | 9/1/csm_graphicals_pic_c7e21aeb6b.png | [SHOT] graphical instruments (out of scope, context only) | — | fc73e9ce |
| ref-05-flash.png | 7/b/csm_flash_pic2_abcb2a8b64.png | [SHOT] flash (out of scope, context only) | — | dc8a7ec5 |

Not inspected: the page's YouTube embed `ZfJayEoCfV8` (no frames captured) and the DTS manual (not public on that page). There is no public screenshot of the **Symbolic Trace**, **Bus Trace**, **Logical Link List** or **DoIP monitor**; only their [DOC] captions are available. Any layout I propose for those areas is therefore [INF].

## 1. What the screenshots actually show

**ref-01, Diagnostic Services [SHOT]**
- Top-left: group box "Diagnostic Services", with a `Search` field (placeholder `<Please enter>`) and a `Global Search` checkbox.
- Below that, a `Diagnostic` tab holding a **service tree** grouped by class (`DataTransmission`). Leaf labels have the form `[22] ReadDataByIdentifier_<DID-name>`: the SID in brackets, then the symbolic name.
- Top-right: `Settings | Description` tabs.
  - `Logical Link  LL_ACM_UDS.EV_ACM_001` and `Diagnostic Service  [22] ReadDataByIdentifier_ECUVoltage`.
  - A **Property / Value / Unit** grid (`Params → SID 0x22, DID 0x2222`, greyed).
  - A `PDU: (1/3)` editable hex field `22,22,22`, followed by ✖ (discard) and ✔ (apply).
- Middle: group "Current selection", with a `Transmit` button, a `Cyclic` checkbox and a `(ms) 2000` spinner.
- Bottom: **Process Values**, a 5-column grid `Name | Value | Unit | RangeInfo | Error`, newest response block on top (the earlier block is shaded).
  - Each transaction is a **nested block**: `[Tester] [715]`, then the service row `ReadDataByIdentifier_ECUVoltage = eALL_POSITIVE`, then the request params (`SID 0x22`, `DID 0x2222`, each `Value OK`), then `REQUEST_PDU 0x22,22,22`, then the logical link row `LL_… [77f]`, then `Response State = acknowledged`, then `RESPONSE_PDU 0x62,22,22,04,E8` with children `SID 0x62 · DID 0x22,22 · EV 12.56 V · Value OK`.
  - Blue text marks response-side / link rows. **Raw PDU and decoded params appear in the same block.**

**ref-02, Tool Quick Test [SHOT]**
- Toolbar: `Mode: Read data ▾`, `Cyclic (s) 60`, `DTC mode: Supported DTCs ▾`, checkboxes `Environment data` / `Coding strings` / `Additional Services`, a `Start` button, and a full-width progress bar (`100 %`). `Text size 12` sits on the right.
- 2×2 quadrants with splitters:
  - **ECUs (3)** (with `Hide unidentified ECUs`): a grid `ECU | Errors | Link | ECU Variant | Status`, where each row has a coloured status square and a checkbox (`BV_ACM 12 LL_ACM_UDS EV_ACM_001 finished`).
  - **ECU Identification (BV_ACM)**: a 2-col `Identification Parameter | Value` grid.
  - **Errors (BV_ACM)**: `DTC | DTC (hex) | Fault Sym… | Status | Status Byte | Readiness | MIL | Stored | Active`.
  - **Environment Data (DTC B0002)**: tabs `DTC Environment Data | DTC Additional Information`, and a grid `Environment Parameter | Value | Unit | RangeInfo | Raw value (hex)`.
- Interaction: selecting the ECU drives the Identification and Errors panes, and selecting a DTC drives the Environment pane (titles carry the selection in parentheses). This is the master→detail chain.

**ref-03 [DIAG]**: A DTS project (ODX/OTX) holds workspaces (Testbench / Vehicle Update / Specification). Each workspace is a layout of control units (Trace, Measurement, Error Analysis, …). → Loom equivalent: a service view = a fixed layout of panels. The user cannot edit it.

**[DOC] captions**
- "Bus Trace — communication at bus level in hexadecimal notation".
- "Symbolic Trace — analysis of diagnostic communication at the application level in symbolic notation".
- "Logical Link List — monitoring and influencing the communication state of ECUs".
- "Communication Control — automated setup and tear-down of communication to ECUs".
- "Service Table — one-off or cyclical execution … in service or parameter notation".
- "DoIP monitor integrated in DTS generation 9".

## 2. Fidelity matrix
| # | Reference area / control (evidence) | Reference interaction | Current Loom [CODE] | Proposed Loom mapping | Retained Loom semantics | Deviation / unimplemented |
|---|---|---|---|---|---|---|
| 1 | Logical Link List + Quick Test "ECUs" grid ([DOC], ref-02 [SHOT]) | One row per ECU/link, with a status square, link name, variant and status | `uds-targets`: one mono text line per target via `targetLine()` (`udsModel.ts:232`) | **Target grid** with columns `● link · Target · Transport(backend) · Addr · Session✓ · Security✓ · TesterPresent · P2/P2* · Owner · [제어 가져오기 T]`. Use a status dot instead of the glyph string. Selecting a row selects the active target, replacing the current `<select>` sync. | Confidence marks ✓?!, a link that was never reported is never shown as up, `[ENV]`, lease owner, takeover via ActionGate `uds.lease.takeover` | No ECU Variant column and no "Errors" count (no ODX, no DTC count in snapshot). Transport shows only `doip`/`sim`. No `cantp`/EthDiag option. **"TP" is spelled out as TesterPresent.** |
| 2 | Communication Control ([DOC]) | Connect / disconnect ECUs | **Shell already has it**: the interface connect dialog issues `uds.connect` (DoIP host or sim) — `ui/src/shell/InterfaceConnect.tsx:34`. The panel itself has no connect control. | **Unchanged.** The panel's empty state points to the interface connect dialog (UDS DoIP) or `loom uds connect`. | Connect stays shell/agent-owned | *Corrected per MAIN*: no panel-local connect is an intentional placement, not a missing backend RPC. |
| 3 | Service tree + Search (ref-01 [SHOT]) | Pick a symbolic service from an ODX tree | Free hex input only | **Service picker (ISO 14229-1 SIDs only)**: a compact list `[10] DiagnosticSessionControl, [11] ECUReset, [14] ClearDiagnosticInformation, [19] ReadDTCInformation, [22] ReadDataByIdentifier, [27] SecurityAccess, [2E] WriteDataByIdentifier, [31] RoutineControl, [3E] TesterPresent` plus a filter box. Picking a service fills the hex prefix only. | Hex line remains the single source of truth. Dry-run preview, risk badge and second Enter for R2+ are unchanged. | No ODX DID names (`_ECUVoltage`), no `Global Search`, no tree classes beyond the ISO groupings. SID names come from the ISO table only, and only if main accepts a static UI table (see §5). |
| 4 | Settings: Logical Link / Diagnostic Service / Property-Value grid / `PDU: (1/3)` ✖✔ (ref-01) | Edit params → PDU, then ✔ | `uds-cmd`: target `<select>`, hex input, preview span, 보내기 ⏎ | **Request composer strip** with labeled cells `Target`, `Service` (from the picker or derived from the first byte), `PDU` (hex), `Preview` (the dry-run result, including risk). ✖ clears the input. The existing 보내기 ⏎ acts as Transmit. | Same `uds.request{dry_run}` debounce of 150 ms. Daemon policy decides. | No per-parameter editor (needs ODX). No `PDU (n/m)` multi-PDU. |
| 5 | `Transmit` + `Cyclic (ms)` (ref-01) | Cyclic resend | `폴링 정지` (`uds.poll.stop`) only | Keep `Transmit`. **Show the cyclic state read-only** (a poll job, if one is present in the snapshot) and keep the stop control. | Stop direction = reduce. Global STOP unchanged. | No UI cyclic start (would add a new UI-initiated write). |
| 6 | Process Values nested block (ref-01) / Symbolic Trace ([DOC]) | Request params + REQUEST_PDU + link + Response State + RESPONSE_PDU + decoded values in one block | RowTable: 대상 / 서비스 / 요약 / 결과 / p2-rtt, plus an expandable detail with raw, NRC and DTC lines | **Symbolic transaction trace**: keep one row per `request_id`. The expanded detail renders a **block** in ref-01 order: `Tester→Target`, `Service (SID name)`, `REQUEST_PDU`, `Link (backend addr)`, `Response state` (pending ×N / positive / NRC 0xXX name / timeout ⌛?), `RESPONSE_PDU`, decoded fields (only those present in the payload, e.g. DTC list, DID value). Columns become `Time · Target · Service · Request PDU · Response/NRC · p2/rtt`. | Progressive stages, `⌛?` uncertain, pending ×N, NRC name from daemon `nrc_name`, raw bytes always visible | No Unit/RangeInfo columns (no ODX scaling). No `eALL_POSITIVE` style enums. |
| 7 | Bus Trace (hex) / DoIP monitor ([DOC] only) | Hex-level bus view beside the symbolic trace | None in the UDS panel. CAN/ETH panels sit in the service view | **Optional lower tab "Transport (raw)"** listing only the hex request/response bytes of UDS rows. This is not a DoIP frame monitor. Link to the ETH/CAN panel when it is open. | CAN/ETH companions keep gated tx (not forced read-only). No implied ECU↔channel link. | No DoIP header decode, no CAN-TP frames, no EthDiag. Labeled [INF] layout. |
| 8 | Quick Test "Errors" grid (ref-02) / DTC control unit ([DOC]: read & clear) | Columns DTC / hex / symbol / status byte / MIL / stored / active; select a DTC to see env data | `dtcText()`, a joined string in the detail only | **DTC pane** (right column, tab next to "DID 값"): rows from the latest final `payload.dtcs` of the selected target, columns `DTC (hex) · 상태 바이트` exactly as the payload carries them, plus the source `request_id`. `읽기 템플릿 19 02 FF` / `지우기 템플릿 14 FF FF FF` only fill the composer; Clear then goes through the daemon dry-run (R2) and the second Enter. | Raw codes are never re-mapped. Clear is a write behind ActionGate and daemon policy. | **MAIN D2**: no status-bit flags, no fault symbol, no SAE P/B/C/U mapping (deferred). No environment data. |
| 9 | ECU Identification grid (ref-02) | Identification param / value | `uds-last`: decoded DID values of confirmed responses | Rename to **"DID values"**, as a 2-col grid `DID (hex) · Value · Age`. | Only confirmed responses, `MAX_LAST_VALUES` cap | No identification names (F190 VIN etc.) unless main approves the static ISO 14229-1 Annex C DID-name table. |
| 10 | Quick Test toolbar: Start + progress (ref-02) | Batch read all ECUs | None | **Not adopted.** | — | A mass read means multi-ECU writes. Excluded, same as the UDS scanner. |
| 11 | Workspaces / layouts ([DIAG] ref-03) | User-composed control-unit layouts | Loom service views (fixed docking) | Fixed UDS view: target grid on top; trace (left, 2fr) + DTC / DID tabs (right, 1fr) in the middle; composer at the bottom. | Shell layout owned by main | No user layout editor. |
| 12 | Flash, OTX, Variant coding, Graphical instruments (ref-04/05, [DOC]) | — | — | **Not adopted.** | — | Out of scope. No ODX/OTX. |

## 3. Panel layout plan (on GO; `ui/src/panels/uds/` only)
```
┌ Target grid ─────────────────────────────────────────────────────────────────┐
│ ● ECU_A  doip  0x1001  Session default ✓  Security locked ✓  TesterPresent ✓×12  P2 50/5000  USR me  [제어 가져오기 T] │
├ Symbolic trace (RowTable, 2fr) ──────────────────────┬ [DTC] [DID values] (1fr) ┤
│ time  target  [22] ReadDataByIdentifier  22 F1 90  ✓ 62 F1 90…  12/50ms │ DTC 923400  st 0x09  TF·CDTC │
│  ▸ block: Tester→ECU_A · REQUEST_PDU · Link doip 0x1001 · Response …     │ [Read 19 02 FF] [Clear 14…] │
├ Composer: Target ▾ · Service [22 ReadDataByIdentifier ▾ filter] · PDU [22 F1 90] · preview(risk) · ✖ · 보내기 ⏎ · 중단 ! · 폴링 정지 ┤
└ (optional) Transport (raw) tab — hex only; link to open ETH/CAN view ─────────┘
```
- Files to add or modify:
  - `consolePanel.ts` (structure).
  - `udsModel.ts` (pure helpers: SID table, status-bit decode, block rows).
  - new `udsPanel.css.ts` or a panel-local style module.
  - `uds.test.ts` (extended, not weakened).

## 4. Acceptance checks
1. The target grid renders one row per target. It never shows `●` for an unreported link. The header reads `TesterPresent`, never bare `TP` or `ISO-TP`.
2. The transport column shows exactly the snapshot `backend` (`doip` / `sim`). There is no UI path that offers `cantp`, `canoe` or EthDiag.
3. Picking a service only fills the hex prefix. Nothing is sent without Enter. Previews of R2+ still need a second Enter. The dry-run debounce stays at 150 ms (the existing tests keep passing).
4. The trace detail block shows REQUEST_PDU and RESPONSE_PDU raw bytes for every completed row. The NRC name comes from payload `nrc_name` only. `⌛?` uncertain is preserved.
5. DTC Clear goes through `uds.request` → dry-run → risk badge → confirm. There is no direct clear RPC. DTC codes are displayed as raw hex identical to the payload.
6. There is no cyclic-start, batch-scan or connect control. Global STOP behaviour is untouched (and it does not touch PSU output).
7. The panel opens with no targets and still shows the empty-state text. It also works with 32 targets plus overflow.
8. Navigating to the UDS view causes no RPC with side effects (only snapshot subscribe and dry-run on typed input).

## 5. Shared dependencies — MAIN decisions (2026-11-12)
- **D1 approved**: a static standard SID picker, used as UI templates only. Transaction labels stay daemon-supplied.
- **D2**: show only the raw 24-bit code and status from `payload.dtcs`. Status-bit decoding and SAE mapping are deferred.
- **D3 approved**: the layout lives inside the current `uds.console` panel.
- **D4 approved**: the block is rendered with the existing RowTable expanded detail pairs.
- **D5 deferred**: no raw transport tab.

Original proposals, for the record:
- **D1**: Is a static ISO 14229-1 SID name table in the UI acceptable, or must SID names come from the daemon (`service` field already exists)? My default: UI-side table for the picker only. Rows keep the daemon `service`.
- **D2**: A DTC status-bit decode (ISO 14229-1 DTCStatusMask bits 0–7) as a pure UI helper. Is it OK? Also, is the SAE J2012 P/B/C/U prefix display allowed? My default: bits yes, prefix no.
- **D3**: Does the UDS service view layout (target grid / trace + side tabs / composer) need a shell or service-view slot change, or does it fit inside the single `uds.console` panel? My default: inside the panel. No shell edit.
- **D4**: A RowTable expandable multi-line block, if `createRowTable` detail only supports `[label, value]` pairs. This could be an SDK change. My default: render the block with existing label/value pairs and indentation.
- **D5**: Should the "Transport (raw)" tab be dropped if main judges it redundant with ETH/CAN views? My default: deferred, not built in the first pass.

## 6. Limits
- I have not run the commercial app. No hardware was connected. Symbolic Trace, Bus Trace, Logical Link List and DoIP monitor have no public screenshots, so those mappings are [INF].
- I did not view the YouTube demo frames. Next step if time allows: inspect the Softing product catalog PDF (DTS chapter) on the same page for additional screenshots.

## 7. Implementation (after MAIN GO) — correspondence, deviations, selectors
Files (UDS-owned only):
- `ui/src/panels/uds/consolePanel.ts` (layout and composer)
- `ui/src/panels/uds/udsModel.ts` (pure helpers: `SERVICE_CATALOG`, `buildPdu`, `parsePdu`, `paramIssues`, `filterCatalog`, `targetCells`, `detailPairs`, `latestDtcs`)
- new `ui/src/panels/uds/uds.css` (panel-local; every selector is scoped under `.uds-ds`, shell tokens only, a container query stacks the columns below 760px)
- `ui/src/panels/uds/uds.test.ts`

No SDK, shell, global CSS, Rust, package or config edits.

**Correspondence (reference → Loom)**
| Reference (evidence) | Loom element | Selector |
|---|---|---|
| Logical Link List / Quick Test "ECUs" grid (ref-02 [SHOT], [DOC]) | "논리 링크" grid. Columns: 링크 · 대상 · 전송 · 주소 · 세션 · 보안 · **TesterPresent** · P2/P2* · 환경 · 제어 · [제어 가져오기 T]. Clicking a row selects the composer target. The row `title` keeps the canonical `targetLine()` text. | `[data-testid=uds-targets]`, row `[data-target=<name>]`, cell `[data-col=link\|backend\|addr\|session\|security\|tester-present\|p2\|env\|lease]` |
| Diagnostic Services tree + Search (ref-01 [SHOT]) | "진단 서비스" catalog, grouped by ISO 14229-1 functional unit, labels like `[22] ReadDataByIdentifier`. The search box filters it. Picking an item only fills the PDU. | `[data-testid=uds-catalog]`, `[aria-label=서비스 검색]`, item `[data-sid=22]` with `aria-pressed` |
| Settings: Logical Link · Diagnostic Service · Property/Value grid · `PDU` ✖ (ref-01) | "설정": a 논리 링크 select with backend/addr, a 진단 서비스 label, a parameter grid (파라미터 · 값 (hex) · 의미, with SID fixed), the canonical PDU hex line, ✖ (clears without sending), the dry-run preview, and required-field hints | `[data-testid=uds-settings]`, `[aria-label=대상]`, `[data-testid=uds-service]`, `[data-testid=uds-params] [aria-label=<ISO param name>]`, `[aria-label="UDS 요청 (hex)"]`, `[aria-label=PDU 지우기]`, `[data-testid=uds-preview]`, `[data-testid=uds-param-issues]` |
| "Current selection": Transmit · Cyclic (ref-01) | "현재 선택": 보내기 ⏎ (Transmit → existing `send` action `uds.request`), 중단 !, 폴링 정지. No cyclic start. | `[data-testid=uds-current]`, `.uds-ds-transmit` |
| Process Values block (ref-01) | The RowTable expanded detail pairs, in reference order: 테스터 → 서비스 (daemon `service` + result) → 요청 (REQUEST_PDU) → 논리 링크 → 응답 상태 (stage / pending 0x78 ×N / timeout / late) → 응답 (RESPONSE_PDU) → NRC → DID values (daemon `decoded`) → DTC → 차단 → p2/rtt → 등급 → request_id → 갱신 | `.vt-row[data-request-id]`, `.vt-detail .disp-row dt/dd` |
| Quick Test "Errors" (ref-02) | "DTC" tab: raw `payload.dtcs` code and status, plus the source request_id, and read/clear template buttons | `[data-testid=uds-dtc]`, row `[data-dtc=<code>]` |
| Quick Test "ECU Identification" (ref-02) | "DID 값" tab: a grid of 대상 · DID · 이름 · 값 · 단위 · 경과 (daemon-decoded confirmed responses only) | `[data-testid=uds-last-values]`, row `[data-did=<target>:<DID>]` |

**Retained semantics, test-backed**
- The canonical hex line is still the only thing sent. Previews still go through a 150 ms debounced `uds.request{dry_run:true}`. R2+ previews still need a second Enter. Template selection and parameter edits never send, and only trigger the dry-run.
- One row per request_id with progressive stages. `⌛?` stays unknown, and an NRC row is never shown as ✓. Dry-run rows stay hidden, and TesterPresent rows fold into the link grid.
- The TesterPresent column header says "TesterPresent (0x3E) … ISO-TP/CAN-TP 전송 상태가 아님". Transport shows only the snapshot `backend` (doip/sim). There is no cantp, CANoe or EthDiag option anywhere.
- Unknown link `? 미보고` is never shown as up. Confidence marks stay: ✓ confirmed, ? assumed, ! stale.
- The write lock disables the composer, catalog, parameters and Transmit. Abort is always enabled. Global STOP and ActionGate are untouched.
- The catalog lists only SIDs in the daemon risk table, minus R4 transfer/flash, immutable-deny 0x2A/0x86, and 0x3E: `10 11 27 28 85 22 2E 14 19 31`.

**Intentional deviations / unimplemented**
- No ODX: no DID or routine names in the catalog, and no units or RangeInfo beyond daemon `decoded`.
- No Description tab, no Global Search, no multi-PDU `(n/m)`, no Cyclic start, no Quick Test batch read.
- The DTC tab has no status bits or symbols (D2).
- No panel connect, because connect lives in the shell interface connect dialog.
- No raw transport tab (D5).
- The process-values block lives in the RowTable drawer rather than a separate full-width grid (D4).
- No DoIP or CAN-TP frame view. The existing CAN/ETH companions keep their gated tx.

**Verification (this worker)**
- `vitest run src/panels/uds`: 33/33. That is 22 pre-existing tests, with selectors moved to the aria-label / row-title / cell form and the same assertions, plus 11 new fidelity tests.
- `vitest run src/shell src/sdk`: 182/182.
- `tsc --noEmit` and eslint `src/panels/uds`: clean.
- **Not done by me**: browser/Ego visual run, aggregate build, e2e. MAIN owns those.
- e2e selectors used by `t04`, `t10` and `real-smoke` are preserved: `getByLabel('UDS 요청 (hex)')`, `uds-preview`, `uds-targets` (with the `제어 가져오기` button and `제어: 나` text), `.uds-panel [data-request-id]`.

**Proposed for MAIN (shared; not edited)**
- `shell.css:244-251`: the `.uds-target`, `.uds-last`, `.uds-cmd` and `.uds-cmd input` rules are now unused by the panel and can be removed.
- `shell.css:425`: `.workbench-main .uds-targets` still pads and colours the link box. It is harmless, but could be dropped for a cleaner group-box frame.
