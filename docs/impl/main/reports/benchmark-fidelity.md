# Service benchmark fidelity

Base: `26dc5e8`. Status: DONE.

Five foreground Claude Opus 5.5 sessions independently inspected official screenshots/manuals and implemented disjoint service panels. Actual model IDs were checked, all `claude-opus-5-5`. MAIN handled shared SDK, service route defaults, code review, native browser fixes and integration. All five owned Herdr workspaces were closed after normal CLI exit; MAIN w8 remains. No background coding subagents, global permission changes or hardware writes.

| Service | Primary reference | Final supported scope |
| --- | --- | --- |
| UART | CoolTerm | Toolbar and two-line status, original-byte HEX/ASCII view, Text/HEX Send String + explicit line ending and byte preview, existing terminal/input lease/large-input gate |
| CAN | PEAK PCAN-View | Receive/Transmit default in CAN service, bounded per-ID receive summary and gap semantics, structured manual transmit list/editor through existing validator/dry-run/gate; chronological Trace retained |
| Ethernet | Wireshark | Vertical packet/details/bytes panes, pointer/keyboard splitters, filter validity/apply/reset, selected packet/field feedback, HEX/ASCII sync, verified requestId relation glyphs |
| UDS | Softing DTS.monaco | Diagnostic service search/templates, logical-link grid, parameters and canonical PDU, explicit Transmit, progressive transaction detail, existing decoded DID/raw DTC tabs |
| PSU | Keysight BenchVue Power Supply | Dominant measured readbacks separate from targets, explicit Apply, output switch distinct channel emergency OFF, 60s memory observation lanes, native channel-scoped status reconciliation |

## Native integration corrections

Actual Ego clicks, rather than jsdom-only assertions, exposed xterm canvas overlapping the UART drawer and the CAN editor extending beneath the raw send bar. MAIN fits the actual terminal body with a disposed ResizeObserver, clips canvas overflow, and reserves/scrolls the editor area. Native PSU writes changed the device while push-only panel state remained stale; a non-overlapping 1 Hz `psu.status` query reconciles authoritative state and drives simulator polling, with abort/timer disposal. It never uses setpoints as measurements. Dark chart axes and lane height were also corrected. CAN daemon risk text now survives the next render (the former hardcoded R1 overwrite was removed).

QA scripts wait for actual DOM render and notification expiry; they do not force clicks through overlays. Virtualized packet selection uses the supported F8 flow, with an actual native ARP sample generated while the view is visible. Earlier failed runs are retained separately and are not claimed as passing evidence.

## Deliberate boundaries

Service separation is UI projection on shared engines/RPC/state. Multi-alias channels retain separate panels. Only active services appear. UDS manager remains DoIP/sim; CAN ISO-TP worker exists separately, with no newly claimed manager integration. Hyundai/Kia EthDiag stays distinct and unimplemented. TesterPresent is not ISO-TP. CAN cyclic transmit, bus-load/errors, UDS ODX/ECU-specific decoding and PSU hardware/protection writes/file logging were not added. Global STOP does not turn off PSU outputs. Physical hardware and commercial reference applications were not run. Unsaved panel-local drafts across service-route remounts remain a separate follow-up.

## Evidence

- Final source: per-domain files under `docs/impl/main/research/fidelity/` and `tasks/benchmark-fidelity/`.
- Aggregate UI: `pnpm --dir ui check`, schema33/type/lint and 554 tests/50 files PASS; log `/tmp/loom-fidelity-verified-check.log`.
- UI/native build PASS. Embedded UI asset contract: 11 tests PASS.
- Frozen final candidate: `~/.cache/loom-ui-workbench/benchmark-fidelity-verified`.
- Final Ego: **51 PASS / 0 FAIL**, `benchmark-fidelity-verified/fidelity-215210.out`; final canonical: **5/5 PASS, 18.4s**, skipped/unexpected/flaky0, `p1-suite-benchmark-fidelity-verified/`. Native daemons/control files cleaned, no leftovers. UI TaskSpace11 closed. HTML TaskSpace12 rendered 15/15 embedded images, zoom/Escape PASS, 1440/768/390px no page overflow; kept only final report for user viewing.
- Local standalone report artifact: `work/reports/loom-fidelity-report.html` (15 embedded images, generated from accepted final evidence).

No push was requested for this revision. Only owned files are included in feature commits; pre-existing dirty STATUS/HANDOFF documents are preserved.

Feature commits: shared `28191ee`, UART `bcf31e4`, CAN `397ac84`, Ethernet `9fc5ed2`, UDS `164815a`, PSU `9cb46e4`. Integration acceptance evidence committed separately. HTML and public reference screenshot assets remain local review artifacts under work/reports; they are not shipped in the application bundle.
