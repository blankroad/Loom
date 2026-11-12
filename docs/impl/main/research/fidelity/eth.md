# ETH fidelity — Wireshark main window → `eth.packets` — CHECKPOINT 1 (PHASE1)

- Owner: ETH fidelity worker. Model: **Claude Opus 5.5 (`claude-opus-5-5`)**, from the session system identity. Foreground, no subagents, no git.
- Date 2026-11-12, base `26dc5e8`. **No product code touched.** Wireshark and physical NICs were **not run**.
- Evidence labels: **[V]** = seen in an official screenshot (file under `work/reports/fidelity/assets/eth/`, attribution in `SOURCES.md`). **[D]** = documented in the guide text (`assets/eth/raw/*.html`). **[I]** = my inference. **[L]** = current Loom code or screenshot (`docs/impl/main/evidence/ui-workbench/service-split-final/eth-all-connected.png`).
- Primary reference = Qt UI `ws-main.png` (Fig 3.1). `ws-display-filter-tcp.png` is the old GTK UI and is used only for row coloring.

## What the reference actually looks like (ws-main.png, 1066×792)

Top to bottom, **every pane spans the full width**:

1. Menu bar (not built).
2. Icon toolbar (not built as such).
3. A one-line display-filter field with placeholder `Apply a display filter … <Ctrl-/>`. A bookmark icon is on its left. Inside the field on the right are a blue → apply arrow and a ▾ recent-filters arrow, followed by `Expression…` and `+`.
4. **Packet list** (~35% of height). Header row: `No. | Time | Source | Destination | Protocol | Length | Info`. `Length` is right-aligned and `Info` takes the remaining width. Text is dense, about 17 px per row. **Every row is filled with its coloring-rule colour**: pale green for TCP/HTTP and pale blue for DNS. The selected row is solid blue with white text in `ws-list-pane`, or a grey-blue band when unfocused in `ws-main`. A narrow strip left of `No.` holds the **related-packet glyphs**: → request, ← response, and a dashed line for the conversation. On the right, an **intelligent scrollbar** draws a colour minimap.
5. Horizontal splitter.
6. **Packet details** (~35%). A monospace tree with chevrons (›/⌄). The first line is the summary `Frame 349: 489 bytes on wire …`. Top layers are **collapsed** except the one being inspected. Generated fields appear as `[Time: …]`. Cross-packet links are underlined, e.g. `[Request In: 348]`. The selected field has a light-blue row highlight.
7. Horizontal splitter.
8. **Packet bytes** (~15%). Format: `offset(4 hex)` · 16 hex bytes **with an extra gap after byte 8** · ASCII **with the same gap**. The bytes of the selected field are highlighted **in both the hex and ASCII columns** (`21 88` ↔ `!.`).
9. **Status bar**: expert-level dot · `Identification of transaction (dns.id), 2 bytes` (the selected field and its filter name) · `Packets: 10299 · Displayed: 10299 (100.0%) · Load time` · `Profile`.

## Current Loom (eth-all-connected.png + code) — real deviations seen

- **Wrong order.** The capture status line is on top. Below it, in this order: filter, mode bar, list, a `송신 (1)` disclosure, the row-action button bar, and the tree│hex. There is no bottom status bar.
- **Tree and bytes are side by side** (`.treehex` is a two-column 1fr/1fr grid in `shell.css:285`). Wireshark **stacks** them. When nothing is selected, the lower half shows one centred hint line, which reads as empty space in the screenshot.
- **Panes are fixed** by flex 2/1.3. They have no splitters, so the user cannot resize them.
- **Row colours are dark tints** (`#13283a` DoIP, `#1c3318` SD). The tints are fine for the dark theme. However, **no selected-row style is visible**, and protocol colours are spread across the whole row with no legend.
- The filter field is a plain input. A separate `적용 / 저장 / 저장·이력` button cluster sits to its right, and validity is shown only as a status word. Wireshark shows validity as **green/red input background while you type**.
- The hex has **no 8-byte mid gap**, and **ASCII is not highlighted** for the selected field (only `.th-b` gets `.hl`).
- There is no `No.`-relative glyph column. The UDS link is a separate `UDS` column on the far right; that part is fine.
- `Packets / Displayed (%)` counts and the selected-field name are not shown anywhere.

## Fidelity matrix

| # | Reference area / control | Evidence | Actual interaction | Current Loom | Exact mapping (proposed) | Retained Loom semantics | Deviation / unimplemented |
|---|---|---|---|---|---|---|---|
| 1 | Three-pane vertical stack (list / details / bytes) | [V] ws-main; [D] MainWindow "clicking packets controls other two panes" | Selecting in the list drives both lower panes | [L] list, then tx, then action bar, then a side-by-side tree│hex | `.eth-panel` becomes a grid: `toolbar / filter / list / split / details / split / bytes / statusbar`. The tree and hex are stacked by panel-scoped CSS on `.eth-panel .treehex` (rows, not columns). | `eth.show` fetch-on-select, abort on reselect | Packet Diagram pane omitted |
| 2 | Resizable splitters | [V] ws-main dividers; [I] Qt QSplitter | Drag to resize; sizes persist | [L] none | Two panel-local drag handles (pointer + keyboard ↑/↓ 8 px). Ratios go to localStorage (per-viewer, try/catch). Defaults are list 45 %, details 35 %, bytes 20 %. | — | Wireshark's alternative layouts (side-by-side) are omitted |
| 3 | Display filter field | [V] ws-filter-toolbar (green); [D] FilterToolbar "red if invalid, green valid; Enter/Apply" | Live syntax check; Enter applies; ▾ recent; bookmark = saved | [L] `filterBar.ts`: server `eth.filter.check` debounce, Enter, a save/history select, a status word | The input background shows `data-state` ok → green tint, err → red tint, idle → neutral. Bookmark (saved) goes on the left inside the field. ✕ clear and → apply go inside it on the right, then ▾ for recent+saved. The error caret line stays. | Server-checked Loom grammar. Apply → resubscribe + `goComplete('filter')`. | No `Expression…` builder and no filter-button bar. The Wireshark grammar is not claimed. |
| 4 | Capture filter ≠ display filter | [D] FAQ / DisplayFilter section | BPF is set in the capture options; the display filter only hides rows | [L] BPF in `dialogs.ts` start dialog; status shows `캡처 필터:` | Keep it. Show the BPF as a read-only chip in the toolbar ("캡처 필터 bpf…"), styled differently from the editable display filter. | BPF changes the recording; the display filter only changes the view | — |
| 5 | Packet list columns and density | [V] ws-list-pane / ws-main | `No. Time Source Destination Protocol Length Info`; Length right-aligned; Info fills the rest | [L] `seq 시간 출발지 목적지 프로토콜 길이 정보 UDS 출처`, Length left-aligned | Header order stays. `seq` shows as `No.`; Length becomes right-aligned with tabular numbers. `UDS` and `출처` stay last. Row height is compacted to about 18 px within the panel. | `seq` = journal seq (not a renumbered index); origin badge | Columns are not user-configurable |
| 6 | Row colour + selected row | [V] coloured rows; solid-blue selection; [D] Colorization section | Rules colour each row; selection overrides | [L] `colorOf()` classes. VirtualTable already emits `.vt-row.selected` + `aria-selected` (`virtualTable.ts:190`). But `.vt-row.selected` (`shell.css:206`) and `.c-doip` etc. (`:275–280`) have the same specificity, and the `c-*` rules come later, **so a coloured row's selection is invisible**. | Keep the classes. Add panel-scoped `.eth-panel .vt-row.selected` (accent fill + bright text + outline) that beats `c-*`, plus a small legend popover ("DoIP / SOME-IP-SD / NRC / TCP 경고 / ARP / 손실"). | The gap row stripe stays the strongest signal | No user-editable colouring-rules dialog |
| 7 | Related-packet glyph column | [V] ws-list-pane arrows; [D] PacketList "Related packet symbols" | → request, ← response, dashed = same conversation | [L] none; only the `↔UDS` column | Add a narrow 1.2em glyph column. Glyphs are drawn **only from facts already in data**: UDS link `requestId` matches between rows, and same DoIP src/dst pair = dashed. Nothing is computed when unknown. | `exact` vs `inferred` confidence: inferred gets a hollow glyph | TCP ACK/dup-ACK glyphs need a backend analysis field (**gap**) |
| 8 | Details tree | [V] ws-details-pane; [D] MainWindow keyboard table | Chevrons; ←/→ fold/unfold; Shift+→ unfolds all; clicking selects and highlights bytes | [L] `TreeHex` tree; top layers open; click only | Uses the same SDK TreeHex. Panel CSS gives it chevrons and denser rows. **Arrow-key fold and the "top layers collapsed except last" default are SDK changes → MAIN.** | Domain-supplied labels (UI never decodes) | `[Request In]` links are not available unless the backend tree supplies them |
| 9 | Bytes pane | [V] ws-bytes-pane, ws-main | offset · 8+8 hex · 8+8 ASCII; selection highlighted in hex **and** ASCII; hover-highlight; tabs for reassembled data | [L] no mid gap, ASCII not highlighted | The 8-byte gap is a panel CSS `.th-b:nth-of-type(9)` margin, if the selector holds. **ASCII highlight needs per-char spans in `sdk/treeHex.ts` → shared change request.** | snaplen-truncated / large-frame raw link note | No reassembly tabs; hover highlight deferred |
| 10 | Status bar (bottom) | [V] ws-statusbar-loaded/selected; [D] Statusbar section | Left: selected field `name (abbrev), N bytes`. Middle: `Packets · Displayed (%)`. Right: profile. | [L] a top line mixing capture state, pps, drops, rec, ts | Split it in two. The **top toolbar** holds capture state ● · `[SIM tap]` · start/stop/pcapng · BPF chip · LIVE/COMPLETE + pause. The **bottom status bar** holds the selected field label + byte length (from the TreeHex node) · `표시 N / 보관 M` or `완전 N행 seq a–b` · pps/bps · drops (≥ / 알 수 없음) · rec · ts. | Drop lower-bound/unknown wording; sampling text; the 2,000-row truncation notice | No profiles; no expert-info dot. Filter abbrev is shown only if the tree node carries `field`. |
| 11 | Capture running vs browsing | [V] toolbar start/stop/"auto-scroll" icons; [D] Main toolbar | Live auto-scroll can be turned off without stopping the capture | [L] Space/scroll-back switches to journal COMPLETE; capture continues | Same behaviour, shown visually as a toolbar toggle `⇣ 라이브 따라가기` (pressed = LIVE). The mode badge goes next to it. Stop stays a separate danger button. | Pause ≠ captureStop; stop is a safety-direction `watch` right | — |
| 12 | Keyboard navigation | [D] Table 3.1 | ↑/↓ moves the selection and updates the panes; Ctrl+↓/F8 works even when the list is not focused | [L] VirtualTable ↑/↓ + Space pause | Keep ↑/↓. Add F7/F8 (prev/next packet) at panel scope and Ctrl+/ to focus the filter, guarded against input targets. | Global STOP key is untouched | Conversation hop (Ctrl+. / Ctrl+,) is deferred |

The TX list and row actions (`재전송 / 편집 후 송신 / UDP/TCP 송신 / 기준 / REF 복사`) are **not in Wireshark**. They move out of the list↔details path into a right-click/`⋯` row menu and a collapsible `송신 (n)` drawer under the status bar. All of them stay ActionGate-gated (`operate`, `writeLocked`).

## Panel layout plan (scoped to `ui/src/panels/eth/` + new `eth.css` panel module)

```
┌ toolbar ─ ● LIVE mirror [SIM tap] │ ■캡처 정지 │ 캡처 시작… │ ⇣라이브 따라가기 │ 시간: 캡처시작 ▾ │ ⇩pcapng │ BPF: … ┐
├ filter ─ [★│ doip && …                         ✕ → ▾] (green/red bg)                                         ┤
├ list ─ ·│No.│Time│Source│Destination│Protocol│Length│Info…│UDS│출처   (coloured rows, strong selection)     ┤
╞═ splitter ═╡
├ details ─ ⌄ Frame 43: 82 bytes … / › Ethernet II / › IPv4 / ⌄ DoIP …                                   ┤
╞═ splitter ═╡
├ bytes ─ 0000  02 00 …  ..  (8|8 gap)   ascii                                                             ┤
├ status ─ doip.payload_type, 2 bytes │ 표시 6 / 보관 6 · 라이브 1/1 샘플 · 기록 100% │ pps bps │ 드롭 ≥0 │ ts ┤
└ ▸ 송신 (1)  [⋯ row menu holds 재전송/편집/소켓/기준/REF]                                                     ┘
```

## Acceptance checks (to add to `eth.test.ts` + Ego by MAIN)

1. DOM order is toolbar, filter, list, splitter, details, splitter, bytes, status. The tree and hex are stacked (hex `offsetTop` > tree `offsetTop`).
2. Splitter drag and keyboard change the pane heights within min clamps. A reload with blocked storage still uses the defaults.
3. Filter input `data-state`: invalid → red class, valid → green class, empty → neutral. Enter applies, and the mode becomes COMPLETE with the range text.
4. The selected row has `.selected` and an accent background that differs from every `c-*` class background.
5. Length cells are right-aligned. The `No.` header shows the journal seq.
6. The related glyph appears only on rows that share a `link.requestId` with the selection. The inferred link gets a hollow glyph. With no link, no glyph is shown.
7. Clicking a tree node updates the status-bar field text and its `N bytes`. Clicking a byte selects the deepest node (existing test).
8. Space/⇣ toggles LIVE/COMPLETE and never calls `captureStop`. Stop stays enabled for `watch`.
9. The status bar shows `표시 N / 보관 M`, drop `≥` / `알 수 없음` wording, and the 2,000-row truncation notice (existing semantics kept).
10. Row actions are reachable from the row menu and disabled without `operate` or when `writeLocked`. The TX drawer count still matches.

## Shared / SDK dependencies → MAIN (not done by ETH)

- `sdk/treeHex.ts`: per-char ASCII spans with `.hl` sync; ←/→/Shift+→ fold keys; an option `collapseTopLayers`; an `onSelectNode(node)` callback so the status bar can show the label and len.
- `sdk/virtualTable.ts`: a per-column `align: 'end'` option (or panel CSS by `data-col` if it already emits one); and an optional leading glyph column. (`.selected` is already emitted, so it needs no change.)
- `shell/shell.css:255–304` currently holds the eth styles. Proposal: move them into a panel-local `panels/eth/eth.css` imported by `ethPanel.ts`. MAIN deletes the shell copies, and `.workbench-main :is(.eth-…)` overrides at `shell.css:425–447` must be reconciled.
- Backend gaps (not mocked): TCP analysis flags for ACK glyphs, `field` abbrev on tree nodes, reassembly tabs, conversation index.

## Safety and truth retained

- LIVE may be sampled. COMPLETE = `eth.query` from the journal, with a cap of 2,000 rows.
- Gaps are shown as gap rows. Drops are shown as a lower bound or as unknown.
- `[SIM]` stays labelled.
- `↔UDS` exact vs `↔UDS?` inferred is kept. A DoIP decode never implies a UDS session, and EthDiag is not claimed.
- Capture stop is the safety direction. The global STOP is untouched. TX stays gated.

**READY_FOR_REVIEW (PHASE1 checkpoint).** Waiting for MAIN GO before any `ui/src/panels/eth/` edit.

---

## IMPLEMENTATION CHECKPOINT 2 (after MAIN GO) — frozen mapping

Scope: `ui/src/panels/eth/{ethPanel.ts, filterBar.ts, eth.css (new, panel-scoped), eth.test.ts}`. There are no edits to SDK, shell or global CSS. The `TreeHex` options `{collapseTopLayers, onSelectNode}` and `.th-char` were supplied by MAIN.

Checks run: `vitest src/panels/eth src/sdk` → 71/71 pass (eth: 28 existing + 6 new). `tsc -b` is clean, and `eslint src/panels/eth` is clean.

**Not yet run:** browser/Ego render, `e2e/m1/eth.spec.ts`, and the aggregate build. I have not visually checked the CSS grid stack in a real browser yet.

| Matrix row | Implemented | Source | Browser selector / test |
|---|---|---|---|
| 1 Three-pane stack | One CSS grid with rows list │ sep │ action bar │ tree │ sep │ bytes. The TreeHex root is `display: contents`, so the tree and the bytes are separate rows. DOM order matches visual order. | `ethPanel.ts` "pane stack", `eth.css` "pane stack" | `[data-testid=eth-panes]`, `.eth-list`, `.th-tree`, `.th-right`; test "vertical order…" |
| 2 Resizable boundaries | `role=separator` with pointer drag, ↑/↓ ±5, Home/End, and double-click to reset. Minimum 10 fr per pane. Stored in localStorage `loom.eth.panes` (try/catch; corrupt → defaults 45/35/20). | `ethPanel.ts` `setupSplit`/`moveSplit` | `[data-testid=eth-split-list]`, `[data-testid=eth-split-bytes]`, CSS vars `--eth-fr-{list,tree,bytes}`; test "pane boundaries…" |
| 3 Filter validity / apply / reset | The field frame has `data-validity` idle/checking/ok/err/fail, shown as a green/red/amber background, plus `aria-invalid`. Inside the frame: ★ save · input · status · ✕ clear · → apply · ▾ saved/recent. `data-dirty` marks an edit that differs from the applied filter; Esc reverts it. ✕ empties the field and applies "no filter". An outside label reads 필터 없음 / 적용 중 / 편집 미적용. | `filterBar.ts` | `[data-testid=eth-filter-frame]`, `eth-filter-input`, `eth-filter-apply`, `eth-filter-clear`, `eth-filter-applied`; the existing `eth-filter-err(-mark)`/`eth-filter-status` are kept; test "filter field validity…" |
| 4 Capture vs display filter | Unchanged. The BPF stays in the capture strip, ✕ clear never calls capture start/stop, and the placeholder says "Loom 문법". | — | asserted in the filter test |
| 5 Columns | A glyph column comes first. `길이` is right-aligned with tabular numbers. The `seq` label is **kept** (it is the journal seq, not Wireshark's ordinal `No.`, an intentional deviation). | `cols` | `.vt-td.eth-num` |
| 6 Selection | `.eth-panel .eth-list .vt-row.selected` is a solid blue fill with an accent bar and overrides the `c-*` tints. The tree's selected node gets the same blue. Bytes and ASCII `.hl` are highlighted. | `eth.css` | `.vt-row.selected`, `.th-b.hl`, `.th-char.hl` |
| 7 Related-packet glyphs | These appear **only** for rows whose `eth.link.request_id` equals the selected row's: request → / response ←, with ⇢/⇠ when not `exact`. There is no src/dst heuristic. | `relGlyph`, column `rel` | `.eth-rel-exact`, `.eth-rel-probable`; test "related-packet glyphs…" (an NRC row with the same addresses gets no glyph) |
| 8 Details tree | Layers start collapsed (`collapseTopLayers`). ←/→/Enter come from SDK TreeHex. Space inside the tree does not trigger pause. | `ethPanel.ts` TreeHex ctor and keydown guard | test "selection feedback…" |
| 9 Bytes pane | 8│8 gap in both hex and ASCII (CSS nth-child). ASCII click/highlight comes from the SDK. | `eth.css` | `.th-line > .th-b:nth-child(10)`, `.th-ascii > .th-char:nth-child(9)` |
| 10 Status bar | Bottom `role=status` bar: selected field `label · N bytes @0xOFF`, otherwise `선택 seq · proto · len · UDS rid`, otherwise a hint. Also shows the shown-packet count (live or complete) with the gap-row count and filter-applied state, plus the kept-rows/evicted text (moved from the mode bar). The capture strip `eth-status` (state, pps, drops, rec, ts, BPF) is **kept on top** for canonical selectors and safety facts. | draw() "bottom status bar" | `[data-testid=eth-statusbar]` `[data-selected=none|packet|gap]`, `eth-sb-field`, `eth-sb-count`, `eth-sb-keep` |
| 11 Live vs browse | Unchanged: Space / ⏸ toggles LIVE↔COMPLETE (journal) and never stops the capture. | — | `eth-mode` |
| 12 Keys | Ctrl/⌘+/ focuses the filter. F8 or Ctrl+↓ selects the next packet; F7 or Ctrl+↑ the previous. Gap rows are skipped. | root keydown | test "F8 / F7…" |

**Intentional deviations and gaps.**
- No menus, statistics, expert info, profiles, colouring-rule editor, packet diagram, reassembly tabs, conversation hop, or `Expression…` builder.
- The `seq` label is kept (row 5).
- The capture strip stays on top instead of merging into the bottom bar.
- The row action buttons (`재전송` / `편집 후 송신…` / …) stay visible in the details header bar. Exact texts are kept for the e2e/unit selectors. Wireshark has no such bar.
- The display percentage (Wireshark's "Displayed N (x%)") is **not shown**: with server-side pushdown the unfiltered total is unknown.
- TCP ACK/dup-ACK glyphs need backend analysis fields and are not built.

**Retained semantics.** LIVE sampled vs COMPLETE journal (2,000-row cap), gap rows, drop ≥/unknown wording, `[SIM tap]`, `↔UDS` vs `↔UDS?`, stop as the safety direction (watch), and send gated by `operate`/`writeLocked`. All existing tests pass unchanged.

**Requests to MAIN.**
1. Run `e2e/m1/eth.spec.ts`, plus an Ego screenshot of `#/s/eth` with a row selected, to check that the grid stack renders and the separators drag.
2. Optional: delete the now-overridden eth rules in `shell.css:270–274` (`.eth-list` flex, `.eth-detail`, `.eth-treehex`). They are harmless as they stand.

READY_FOR_REVIEW (implementation checkpoint 2).

## STATUS: READY_TO_INTEGRATE — source frozen (2026-11-12)

- MAIN accepted the source review. The shared SDK tests passed and the ETH lint is clean.
- Frozen ETH files: `ui/src/panels/eth/{ethPanel.ts, filterBar.ts, eth.css, eth.test.ts}`. The ETH worker makes no further edits.
- MAIN owns the browser/Ego verification, the `e2e/m1/eth.spec.ts` run, the aggregate build and any browser fixes from here.
