# UDS Read DID — Opus research (Phase R)

2026-11-14 · Opus 5.5 · base main 4fb848a · scope: **one** Read DID slice (target/route → 0x22 request → response inspection). No UI written.
Evidence tags: **[D]** documented text · **[V]** visually observed in an official screenshot · **[I]** my inference (not claimed by vendor).

## 1. Sources and image provenance

| # | Source | Access | Used for |
|---|---|---|---|
| S1 | Vector AN-IND-1-001 *CANoe and CANalyzer as diagnostic tools* v1.4, 2012-07-17 (Schmidt, Webler). Vector-authored PDF from a third-party mirror `https://robertscaplblog.wordpress.com/wp-content/uploads/2016/02/an-ind-1-001_canoe_canalyzer_as_diagnostic_tools.pdf`, sha256 `b9690a80…4f23d`. The current official v2.0 (2026-02-24) at `https://cdn.vector.com/cms/content/know-how/_application-notes/AN-IND-1-001_CANoe_CANalyzer_as_Diagnostic_Tools.pdf` returns 403 (CloudFront), so v2.0 was not read. | Text §2.1–2.4, §3.1–3.2. Fig 1 (Trace, p.7) and Fig 2 (Diagnostics Console, p.8) extracted with `pdfimages` to the scratchpad and viewed natively. | Vector workflow and console/trace layout. The UI is from 2012, so current CANoe styling may differ. |
| S2 | Vector CANoe 17 SP3 release notes `https://help.vector.com/CANoe/17-SP3/en/ReleaseNotes/ReleaseNotes.html` | WebFetch summary | Current feature names (Diagnostics/ISO TP Configuration dialog, Diagnostic Trace, DoIP pending) |
| S3 | Softing DTS.monaco product page `https://automotive.softing.com/products/softing-dts/softing-dtsmonaco.html` | WebFetch | Element descriptions (Logical Link List, Bus vs Symbolic Trace, Service Table) |
| S4 | Softing official screenshot `…/fileadmin/_processed_/8/d/csm_diagservice_pic2_10cd884286.png` saved by root as `qa/softing-diagnostic-console-full.png` (3001×2160, sha256 `816048f4…aa43b1`). Viewed natively. | Root-saved | Softing Diagnostic Services console |
| — | `qa/softing-diagnostic-services.png` and `qa/softing-service-table.png` are icons. `qa/softing-workspaces.png` is an architecture infographic. | — | **Not used as UI evidence** |

Gaps: I found no official screenshot showing **0x78 pending**, **timeout**, or **cancel** in either tool. Vector KB 982 returned 403. The DoIP note AN-IND-1-026 was not readable. Those behaviours below come from ISO 14229 semantics plus the Loom UDS PLAN, and are tagged [I].

## 2. Observations

### Vector CANoe/CANalyzer
- **Target is configured per network and description, not per request.** [D S1 §3.1.1–3.1.2] A Diagnostic Description (CDD/ODX/PDX/MDX, or a generic UDS CDD) is added in *Configuration → Diagnostics/ISO TP Configuration*, then assigned to a network or node. The **Interface** combo "defines an ISO TP addressing mode and address parameters"; the addressing mode can't be changed, only a different interface chosen. **Variant** determines which services the Console offers. Physical network requests and functional group requests are separate usages. Takeaway: transport/addressing sits **below** and apart from the service list.
- **Console workflow.** [D S1 §3.2.1] The user selects a request in an "Explorer-like" tree, edits parameters by drop-down or typed value, sends it, and "the response will be presented accordingly in this window". The Console sends to the description's configured target. [D S1 §2.4.2] Raw byte streams are also possible.
- **Console screenshot (Fig 2), [V]:**
  - The window title reads `Door - Diagnostics Console`, so the **target ECU is named in the console title**.
  - Top right shows an `Online` state.
  - A `Symbolic` mode drop-down sits next to a `[CommonDiagnostics] Door` variant/target label.
  - The request combo reads `5: 22 00 92 - Serial Number Read`, next to an **Execute** button.
  - The tree is grouped by class (Sessions / Identification / Security Access / …). **Every leaf shows its raw request bytes before its name**, e.g. `22 00 9F - Ecu Identification Read`.
  - The upper-right grid has Name | Value | Unit, with `PDU  22 00 92`.
  - The lower-right log has Type/Parameter | Service/Value and reads `[15:40:33] - Tx  Serial Number Read`, `[15:40:33] Door`, `[15:40:33] - Rx  Positive response:`, `Serial Number  170821`. It is a **timestamped Tx/Rx log that names the target, with the decoded value shown under its Rx**.
- **Trace screenshot (Fig 1), [V]:**
  - Columns: Time | Chn | ID | Name | Type | Src | Dst | Co… | Len | Protocol service | Service | Data.
  - **TP frames and UDS rows are separate rows in one trace.**
    - TP rows (`<OTP>`, Type `SF`/`FF`/`FC.CTS`/`CF`, IDs 700/600, Src Tester/ECU).
    - Diagnostic rows (Type `req`/`pos`/`neg`, Src `<tester>`, Dst `Door`). A 16-byte response appears as FF + FC + 2×CF followed by **one reassembled `pos` row** `62 00 9F 43 41 …`.
  - `neg` rows are red and `pos` rows are green.
  - Expanding a `neg` row shows `SID-NR 0x7F`, `SIDRQ-NR 0x10`, `RESPONSE CODE  Security access denied  33`, and a raw `0000: 7F 10 33` line.
  - Expanding a `pos` 0x22 row shows `SID-PR 0x62`, `Identifier`, `Part Number  CANoe UDS ECU`, plus a hex+ASCII dump line.
  - [I] The `Co…` column carries the same number for a request, its TP frames and its response, so it acts as a transaction correlation id.
- [D S2] Current CANoe has a *Diagnostics/ISO TP Configuration* dialog, a Diagnostic Trace that can be filtered by Diagnostic Feature and exported to XML, and DoIP handling where "the final response is detected … following a response pending".

### Softing DTS.monaco
- [D S3] The **Logical Link List** is for "monitoring and influencing the communication state of ECUs". **Bus Trace** is "bus level in hexadecimal notation" and **Symbolic Trace** is "application level in symbolic notation", so they are two separate views. The **Service Table** does "one-off or cyclical execution … in service or parameter notation". DTS 9 has a DoIP monitor.
- **Diagnostic Services screenshot (S4), [V]:**
  - Search box with a `Global Search` checkbox. The `Diagnostic` tab tree shows `DataTransmission` with leaves `[22] ReadDataByIdentifier_<Name>` (AirbagActive, ECUserialNumber, ECUVoltage selected, ProgrammingDate, …).
  - Settings tab: `Logical Link  LL_ACM_UDS.EV_ACM_001`, `Diagnostic Service  [22] ReadDataByIdentifier_ECUVoltage`.
  - Params grid: Property | Value | Unit, with greyed constant `SID 0x22`, `DID 0x2222`. Below it, an editable raw `PDU: (1/3)` field `22,22,22` with ✗ (discard) and ✓ (apply) buttons.
  - `Current selection` group: **Transmit** button plus `Cyclic` checkbox with `(ms) 2000`, disabled until checked.
  - **Process Values** table: Name | Value | Unit | RangeInfo | Error. Each transaction is one block:
    - `ReadDataByIdentifier_ECUVoltage  eALL_POSITIVE`, then `SID 0x22`, `DID 0x2222`
    - `REQUEST_PDU 0x22,22,22`, `[Tester] [715]`
    - `LL_ACM_UDS.EV_ACM_001 [77f]`, `Response State acknowledged`
    - `RESPONSE_PDU 0x62,22,22,04,E8`, then `SID 0x62`, `DID 0x22,22`, `EV 12.56 V  Value OK`
  - An earlier transaction block (`…04,E7`, `12.55 V`) is kept above it, so **history is preserved and each decoded value sits right under the raw response it came from**.
  - [I] `[715]`/`[77f]` look like CAN request/response IDs bound to the tester and the logical link. `0x04E7 = 1255 → 12.55 V` implies a scale of 0.01. `(1/3)` is unexplained; it may be a 1-of-3-byte cursor.
- [I] The logical link name `LL_<ECU>_<protocol>.<variant>` follows the ASAM MCD-3D concept of a link: ECU + protocol + physical interface. The user picks a link, not raw addresses.

### Convergent pattern across both tools (basis for the layout)
1. A target/link is chosen **before** and **apart from** the service, and stays visible in the header or settings (Vector title + variant label; Softing `Logical Link`).
2. A service list with **raw bytes and name together** (`22 00 92 - …`, `[22] ReadDataByIdentifier_…`).
3. A parameter grid with constant SID/DID plus an **editable raw PDU line** and an explicit send (Execute/Transmit). No send on selection.
4. A response shown as **raw PDU → decoded parameters with unit and range status**, per transaction, with **history kept**.
5. Negative responses shown as SID-NR / SIDRQ-NR / NRC name + raw bytes, in a distinct colour (Vector).
6. Transport-level frames are separate from the UDS PDU (Vector Trace TP rows; Softing Bus vs Symbolic Trace).

## 3. Concrete human Read DID workflow (target behaviour for the mockup)
1. **Pick the target**: ECU (e.g. `BCM`, `GW`) and its route. The route chip shows the transport separately: `CAN-TP · CAN1 · 0x7E0→0x7E8`, `DoIP · ETH1 · tester 0x0E00 → LA 0x1001`, `EthDiag (mock adapter) · ETH1`. Link state is shown as `Online`/`Offline`. Selection never sends anything.
2. **Pick the DID**: from a short list (`F190 VIN`, plus 1–2 clearly synthetic DIDs), shown as `22 F1 90 — VIN`, or type hex. The parameter grid shows `SID 0x22` (constant) and `DID 0xF190`. The raw PDU preview `22 F1 90` updates live. Validation: 4 hex digits, 0x0000–0xFFFF, rejects empty, odd-length or non-hex input, and disables Read.
3. **Press Read**; Enter in the DID field also sends. A row is added with **frozen snapshot** `{target, route, addresses, DID, request PDU, t0}`. Status reads `sent`.
4. **Watch the response sequence** in the detail timeline. Each event is its own row with raw bytes and elapsed time from t0:
   - `Tx 22 F1 90` → `Rx 7F 22 78 pending` (×n, P2* window) → `Rx 62 F1 90 …` positive
   - or `Rx 7F 22 31 requestOutOfRange` (NRC)
   - or `timeout (P2 / P2*)`
   - or `cancelled` / `link lost`

   [I, ISO 14229]
5. **Inspect the result** in the Softing-style process-values block: REQUEST_PDU, route snapshot, response state, RESPONSE_PDU, then SID 0x62 / DID / data bytes / decoded value with unit and encoding (ASCII for VIN). Byte offsets are cross-highlighted between raw and decoded.
6. **Confirmed value panel**: per (target, DID), only a **positive** response updates the value. It shows "from txn #n at hh:mm:ss.mmm via <route>". NRC, timeout, cancel and late responses do not change the value. Switching target re-scopes the panel; history rows keep their own snapshot.
7. **Repeat or copy**: re-read the same DID, or "Edit as new" from a history row. Copy raw hex. Export the local trace (JSON/CSV).

## 4. Chosen layout (narrow; one Read DID page, Bench Light shell from Ethernet Opus)

```
┌ shell-hdr: Loom | UART  Ethernet  [UDS] ·························· SIM | ● BCM DoIP ┐
├ target bar: ECU [BCM ▾]  Route [DoIP·ETH1 0x0E00→0x1001 ▾] Online · P2 50/P2* 5000 ms │ (SIM options ▸) ┤
├──────────────┬──────────────────────────────────────────┬────────────────────────────┤
│ DID list     │ Request: target/route (read-only echo)   │ Confirmed values (target)  │
│ search ____  │ SID 0x22 (const)  DID [F190] VIN         │ F190 VIN LOOMSIM…01        │
│ 22 F1 90 VIN │ PDU 22 F1 90  (3 B)  [Read ⏎] [Cancel]   │  #7 12:01:03.412 DoIP      │
│ 22 F1 8C Ser │ ───────────── Transactions ───────────── │ F18C …  (no value yet)     │
│ 22 01 0A Syn │ # time  target route DID req  resp st ms │                            │
│ manual hex   │ (rows: pending ⏳ / ✓ / ✗NRC / ⌛ / ⛔)   │                            │
│              ├──────────────────────────────────────────┴────────────────────────────┤
│              │ Detail (selected txn): event timeline | process values (raw→decoded) | bytes │
└──────────────┴───────────────────────────────────────────────────────────────────────┘
 status: target · route · last txn  ············· 프로토타입 · 로컬 시뮬레이션, 실제 버스/소켓 없음
```

- **Target bar** (Vector title/variant + Softing Logical Link): the ECU logical target and the transport route are two separate controls. The route shows transport-specific addresses. DoIP/EthDiag routes link to the Ethernet Opus module, and a CAN-TP route shows its bus config locally, with no CAN nav. The EthDiag label says "vendor Ethernet adapter (mock) — not ISO 13400 DoIP; raw = UDS PDU only".
- **Left: DID list** with raw-bytes prefix (convergent pattern 2) + search + manual hex entry.
- **Centre top: request editor** (pattern 3): constant SID, DID field, live PDU preview, explicit Read button, and Cancel while in flight.
- **Centre: transaction table** (Vector trace diag rows): `# · time · target · route · DID · request · response · status · pending n · elapsed`. Colours follow Vector: pos green, neg red, pending amber. Rows are **UDS-level only**. No fabricated TP or DoIP frames.
- **Bottom: detail** with a resizable split:
  - (a) event timeline: Tx, each Rx incl. every 0x78, and timeout/cancel with Δt
  - (b) Softing-style process values (raw PDU → SID/DID/data/decoded/unit/encoding)
  - (c) hex bytes with offset highlight
- **Right: confirmed values**, scoped to the current target. Positive responses only, each with provenance (txn #, time, route).
- **SIM options**: a compact collapsible section in the target bar. Per-next-request outcome: positive / NRC (0x31, 0x22, 0x33) / pending×N then positive / timeout / link drop. P2 and P2* are shown. No auto retry, no cyclic send (Softing's Cyclic is deliberately omitted), no send on reload.
- **Responsive**:
  - At 1440: all three columns.
  - At 768: the confirmed-values column folds under the detail.
  - At 390: stacked target bar → request → table → detail, with the DID list as a select.
- **Not included**: session/security/DTC/write/routine, ODX import, cyclic polling, agent chat, or KPI tiles.

READY_RESEARCH
