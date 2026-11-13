# UDS Read DID — Opus HANDOFF (Phase I) · SOURCE_FREEZE

2026-11-14 · Opus 5.5 · base main 4fb848a. Single interactive mockup at `/uds-read-did/app/index.html`. Served by the existing :4180 server (all 5 assets return 200). It follows `../DESIGN.md` plus REQUEST. No git, no deps, no product/backend changes. The UART/Ethernet mockups are unchanged: the Ethernet Opus freeze `shasum -c` is all OK.

## Files (SOURCE_FREEZE — `selfcheck/source-sha256-freeze.txt`)
```
42e0a4e539b73cf1995c095dd4bbaf0c2f25631b96509497410c865382f71dc6  index.html   shell (from selected Ethernet Opus) + module markup
31b1b054dfa7483f71d5d055bf78e2a71ee4f4d95ad695a8aef525ee422d8e18  styles.css   Bench Light tokens copied; module + 1180/860/560 breakpoints
28244ac6caaa530daf07c970fb91d5d3b188f60f335d37c2ef40cd207c841444  engine.js    fixtures, DID validation, PDU decode, sim, transaction/confirmed rules (no DOM)
7b5c217fca49bf2a6d1cffbe7d5f0914c90faa1c7b08aa9357f0687d3bd88fbd  app.js       DOM adapter: context strip, request, DID list, history, detail, confirmed values
b86235c7f6a62efbf60e6ed085aa8738c59c2b71fd97c0a46e5f722655950444  tools.js     SIM options, JSON/CSV export, clear, copy, pane collapse, divider
8286d068…f17a  selfcheck/harness.cjs · 87f1f376…d3d3  selfcheck/check.cjs
```

## Model (what is guaranteed)
- **Context** = ECU (`BCM`, `GW`, fixtures) × route (`DoIP` / `CAN-TP` / `EthDiag`). Each context has its own SIM link, which starts **offline**. `[연결]` (Connect) sends nothing.
  - DoIP shows ETH1, the mock host, and tester LA 0x0E00 → ECU LA 0x1001/0x1010.
  - CAN-TP shows CAN1 500k and Tx/Rx IDs 0x740/0x748 and 0x7E0/0x7E8.
  - EthDiag is labelled "vendor Ethernet adapter (mock) · not ISO 13400 DoIP · framing undefined".
  - `주소 상세` (address details) explains, for each route, that it is mock-only and says what native supports. The DoIP and EthDiag notes link to the Ethernet mockup with "no correlation".
- **Read** freezes the ECU, a route snapshot (deep copy), the link generation, the DID, the request bytes, and the SIM scenario and timings onto the transaction. Only **one active request per context** is allowed; other contexts run independently.
- **Response semantics:**
  - `7F 22 78` is non-terminal. It goes into the timeline and re-arms P2*.
  - A final `62 DID data` or `7F 22 NRC` is terminal.
  - Timeout (P2 1000 ms or P2* 5000 ms, SIM-scaled), cancel, and link-lost all end the transaction **without response bytes**.
  - Anything that arrives after a transaction ends is recorded as **late**, struck through, and marked "ignored". Status never changes after it ends.
  - Link drop bumps the generation, so the old generation's ECU replies can never arrive.
- **Confirmed values** are keyed by ECU+DID and updated **only** by a validated positive response (DID echo matches, decoder OK). They are **monotonic by request number**: an older transaction that completes later is marked `값 미반영` (value not applied) and noted on that transaction.
  - NRC, timeout, cancel and link-lost keep the previous value with its old provenance. The values panel warns `최근 #n … — 아래는 이전 값` (latest #n failed; the value below is the earlier one).
  - Provenance shows txn #, time, route label and summary. The link selects the source transaction.
- **Fixtures:**
  - F190 VIN `LOOMSIM0000000001` (ASCII 17)
  - F18C serial `SIM-BCM-000417` / `SIM-GW-000233`
  - 0101 "Supply Voltage", marked synthetic: u16 × 0.01 V, value cycles +0..6 per read
  - Any other DID gets NRC 0x31 from the SIM ECU.
  - Decoders are labelled "fixture decoder (not ODX)".
- **Validation:** an optional `0x`, spaces and underscores are accepted. The result must be exactly 4 hex digits (0000–FFFF). Empty, non-hex, odd, short, or >4-digit input (incl. `22 F1 90`, with a hint that SID 22 is added automatically) is rejected and sends nothing.
- **Persistence:** only `loom.udsReadDid.v1`, holding ECU, route, DID text, SIM options, filter, pane sizes and collapse state. History and link state are not persisted. A reload makes no connect, send, poll or retry. History is bounded to 200, and in-flight transactions are never evicted.

## Selectors / scenarios for native QA
- **Context:** `#selEcu`, `#segRoute [data-tr=doip|cantp|ethdiag]` (←/→), `#routeSum`, `#linkSt`, `#btnLink`, `#btnRouteInfo`→`#routeInfo`, `#btnSim`→`#simPanel` (`#simSeg [data-sc=positive|nrc|pending|timeout|disconnect]`, `#simLat`, `#simNrc`, `#simPend`, `#simLate`), `#shellConn`.
- **Request:** `#didInput` (Enter = Read), `#didQuick` (shown when the DID list is hidden or collapsed), `#reqPdu`, `#didMsg`, `#btnRead`, `#btnCancel`. DID list: `#didList li[data-did]` (click or ↑↓ fills the editor only; Enter focuses the input), `#didSearch`.
- **History:** `#txBody .tx-row[data-id]` with class `s-<status>`, `.unapplied`, `.late`. ↑↓/Home/End/PgUp/PgDn work when `#txBody` has focus. `#segFilter [data-filter=all|ecu]`, `[data-cmd=export-json|export-csv|clear-history]`.
- **Detail:** `#seq li[data-i]` (↑↓; picking an event pins it, otherwise the detail follows the latest response), `#fields tr.fd` ↔ `#hex .hx-h b[data-o]` highlight, `[data-cmd=copy-hex]`. Divider `#divHist` (drag, ↑↓ ±4 %, Home/dblclick reset, persisted).
- **Values:** `#valsBody .cv`, `a[data-goto]`. Collapse: `[data-cmd=collapse-did|expand-did|collapse-vals|expand-vals]`.
- **Suggested native sequences:**
  1. Connect → F190 → Read → `62 F1 90 …` and VIN #1.
  2. SIM Pending ×2 → Read, then switch route to CAN-TP mid-flight → row and detail stay DoIP. Final result is confirmed #2.
  3. SIM NRC 0x22 → Read → the VIN stays #2 with a warning.
  4. Timeout + late → timeout at 1000 ms, then a struck-through late Rx, nothing applied.
  5. Pending → Cancel → late events are ignored.
  6. Disconnect → link-lost, route offline, no retry.
  7. Run CAN-TP Pending ×3 on 0101 and, in parallel, DoIP positive on 0101 → when the older CAN-TP read finishes, it shows `값 미반영`.

## Source checks (jsdom + fake clock, not browser proof)
`node work/reports/uds-read-did/app/selfcheck/check.cjs` → **70/70 PASS** (`selfcheck/selfcheck-result.json`). The harness closes every jsdom window and clears the 200 ms tick, so the process exits.
- **Initial state:** U1 DID list populated, offline, no auto send or timers.
- **Validation:** 8 invalid inputs send nothing; normalization works.
- **Positive path:** exact bytes; decoded VIN; provenance; field↔byte highlight.
- **Snapshot isolation:** the ECU/route snapshot survives a context switch.
- **Error paths:** pending ×2 sequence and elapsed time; NRC keeps the old value; timeout at exactly P2 and the late reply is ignored; cancel with late replies ignored; disconnect bumps the generation with no retry.
- **Concurrency:** a busy context rejects a second Read; parallel routes work; the older positive is not applied.
- **DID handling:** an unknown DID gets NRC 0x31; raw and decoded values match the source bytes for every confirmed value.
- **History:** filter and selection stay consistent; keyboard navigation; provenance link.
- **Export:** JSON/CSV content and downloads.
- **SIM and storage:** SIM panel; only our own storage key is written; clearing history keeps confirmed values and in-flight requests; reload restores prefs without sending; corrupt prefs fall back to defaults; history bound keeps in-flight requests.

## Root QA fixes applied
- **U1** (empty initial `#didList`): `render()` now calls `renderDidList()`. Covered by the first check.
- **U2** (390 px: the confirmed values covered the detail panes; the first fix failed native recheck).
  - Root cause: at ≤560 px `.mod-uds`, `.wsp` and `.center` were still grids inside a definite-height scroller. Their items have `min-height: 0`, so the `auto` rows could only grow into the leftover space. The center was clamped to 456 px while the detail was 612 px.
  - Fix, inside the 560 px block only: `.mod-uds` becomes `display: block` (the single vertical scroller). `.wsp` and `.detail` become flex columns, and `.center` becomes block. `.wsp > *` gets `flex: none; min-height: auto`. History stays at a fixed 260 px, and each detail pane is 150–300 px and scrolls internally. There is no `grid-template-rows` left in the block.
  - Every container now takes its intrinsic content height, in the order context → request → history → sequence → fields → bytes → values.
  - Check #70 is a static source guard only; native geometry is root's proof. Only `styles.css` changed and the JS is unchanged. 1440/768 rules are untouched (the 1180/860 blocks did not change).

## Known limits
- All routes are local mocks. There are no TP/DoIP/EthDiag frames, no sockets, and no native daemon. Timing is SIM-scaled: P2 1000 ms vs a real ~50 ms.
- There is no functional addressing, session, security, DTC or cyclic read (out of scope).
- The CAN-TP route has no CAN-module nav (none exists). The Ethernet link is a plain navigation to an independent mockup.
- Clipboard copy needs a secure context, otherwise it shows an error toast. The elapsed time of in-flight requests is refreshed every 200 ms.
- Layout below 1440 px has only been checked in source (CSS breakpoints 1180, 860 and 560). It needs root's native 768 px and 390 px pass.

## Native acceptance (root-owned)
Root native QA is recorded in `../qa/NATIVE.md`.
- The positive path passes, and so do the pending/NRC/timeout/cancel/disconnect paths, the cross-route rule that an older positive is not applied, the JSON and CSV exports, and the divider.
- U1 and U2 are fixed. At 390 px, root measured the center at 1023 px fully containing the 612 px detail, with the values below it. 768 and 1440 pass.
- Root accepted the final SOURCE_FREEZE above (7 hashes verified). No further code or test changes.
