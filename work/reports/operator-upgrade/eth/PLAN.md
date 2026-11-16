# ETH operator upgrade — PLAN (compact, all findings)

Lead: ETH Opus 5.5 (architecture, contracts, tests, review). Sonnet: product function bodies only, per FUNCTIONS.md batch allowlists.
Worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-eth` (610029e). Input: `work/reports/usability-benchmark/RETURN-eth.md`. Constraints: TASK.md + COMMON.md v1.
Intent: a usable operator tool at today's information density; no vendor-suite cloning.

## Architecture decisions
1. **Honest range scope is a daemon fact, not a UI guess (ETH-01).** Every journal read (`query/stats/follow/export`) returns `scope` (applied from/to, `from_source`), `scanned` seqs, ordered `partial_reasons`, `resume_cursor`. `complete` is strict (= no partial reason: explicit range, full scan, no loss, no row/output cap) and is the only flag UI/agents may treat as 완전/evidence; `scan_complete` separately reports scan coverage of the applied range (ROOT review). No reverse/tail scan is added (CORE ask stays external); instead partial results are labelled and continuable.
2. **Capture-start anchor = observed journal position.** The reducer records `(epoch, seq)` of a generation's `starting` capture_state; published as `start_cursor` in `eth.capture.status` and the snapshot. Unknown (daemon restart / joined mid-generation) stays `null` — never inferred from packets or current state (COMMON provenance rule).
3. **UI ranges are always explicit (ETH-01 UI).** Complete mode never sends a `from`-less query: range = capture start anchor, else an explicit "최근 N분" labelled as such, or a typed seq range. Page size = server cap 1,000 (today's UI claims 2,000 but the daemon clamps to 1,000). Bounded page stack for older/newer pages; "계속 스캔" uses `resume_cursor`.
4. **UDP conversations get `udp.stream` in the decoder (ETH-03)**, a separate index from `tcp.stream` (Wireshark semantics), plus `stream_proto` in the pkt payload/rows so the UI never guesses TCP vs UDP from the protocol column (DoIP-over-UDP rows say "DoIP").
5. **Tree → filter uses daemon-made, compile-checked expressions (ETH-02).** `eth.show` tree nodes carry optional `filter` (only emitted when `Filter::compile` accepts it); the UI never builds literals from display labels. SDK `TreeNode` gains optional `name`/`filter` and TreeHex an optional `onNodeContext` hook — other consumers unaffected.
6. **Decode As is read-time (ETH-06, P2),** like Wireshark: optional `decode_as` (≤ 8 rules `{transport, port, proto}`) on `eth.query/show/stats/export` changes decoding for filters, detail tree and re-summarised rows; every response echoes the rules applied. No capture-time/worker change, so no per-generation provenance problem; the live (sampled) list stays undecoded and the UI says so.
7. **File replay from the GUI (ETH-04)** reuses the existing `replay` backend: the capture dialog gains a `replay` backend with a daemon-machine path, speed, repeat; REPLAY chip on cards. No native file picker (desktop bridge not owned).

## Finding coverage
| ID | Pri | Work (batch) | Explicit exclusions / external deps |
|---|---|---|---|
| ETH-01 | P0 | B1 daemon scope/reasons/anchor (FUNCTIONS F1.*) · B2 UI explicit ranges, honest badge, paging, export range dialog + result scope, follow range | Reverse/tail journal scan = CORE ask (not added). 64 MiB scan budget kept, surfaced as `scan_budget` + resume. |
| ETH-02 | P1 | B3 daemon tree `filter`/`name` · B4 SDK TreeNode/TreeHex hook, panel menu "필터로 적용 / 준비(and·or) / 필드명 복사", status-bar field name, FilterBar `draft` getter | Apply-as-Column deferred (P2 per report). Field autocomplete not added (not sourced). |
| ETH-03 | P1 | B3 decoder `udp.stream` + registry field, worker `stream`/`stream_proto`, gateway `decode_record` overrides `udp.stream` · B4 UI `stream_proto` for follow | — |
| ETH-04 | P1 | B4 capture dialog replay backend with **typed daemon-machine path (required scope)**, speed, repeat; saved replay configs; REPLAY chip | Native picker / drag-drop = optional, deferred by ROOT (desktop bridge, not ETH); BPF read-filter on replay is live-only in the engine → display filter after open; original-file frame number not in payload (not invented). |
| ETH-05 | P1 | B2 unbound start: existing-channel list first, wrap (no ellipsis) guidance, same-alias config → "기존 채널 열기" instead of a refused [연결…], hide duplicate toolbar start/add when unbound | Shell-common reopened/visible-panel toast: ROOT assigned to the UART Opus lead (`ui/src/store/channels.ts`, `ui/src/store/shellControl.ts`, `ui/src/shell/services.ts`, separate UART contract). ETH edits no shared toast file; ETH expectation: no ETH-side change, ETH tests must not assert shell toast text. Closed landing is intended (no change). |
| ETH-06 | P2 | B3 decoder `DecodeAs` rules + gateway param · B4 Decode As dialog (per-channel, per-viewer storage), badge + detail note | Capture-time Decode As (live list, UDS ↔ link for remapped ports) not provided — stated in UI. |
| report "미검증" resend `confirm:'none'` | — | Not a finding; unchanged (risk policy not owned). | ROOT/policy if wanted. |

## Batches (file leases; one Sonnet batch at a time per file)
| Batch | Files (Sonnet body lease) | Depends | Status |
|---|---|---|---|
| **B1** ETH-01 daemon | `crates/loom-eth/src/gateway/{common,handlers,evidence}.rs` | — | **PLAN_READY_BATCH1** (scaffold + failing spec tests in place) |
| B2 ETH-01/05 UI | `ui/src/panels/eth/{ethModel.ts,ethPanel.ts,eth.css}` | B1 wire | Opus specifying next (different files) |
| B3 ETH-02/03/06 engine | `crates/loom-eth-decode/src/{stream,net,fields,lib}.rs`, `crates/loom-eth/src/worker/pipeline.rs`, `crates/loom-eth/src/model.rs`, `crates/loom-eth/src/gateway/{common,handlers}.rs`, `crates/loom-eth/src/gateway.rs` (param schemas) | B1 lease returned | planned |
| B4 ETH-02/03/04/06 UI | `ui/src/sdk/treeHex.ts`, `ui/src/panels/eth/{filterBar,dialogs,ifaceConfigs,ethModel,ethPanel}.ts` | B2, B3 wire | planned |
| B5 docs | `skills/loom-eth/**` (Opus-authored docs: new response fields, `complete` semantics, start_cursor, udp.stream, decode_as, replay from GUI) | B1–B4 | planned |

Tests are Opus-owned and written before each lease (B1: `crates/loom-eth/tests/range_scope.rs`, `common.rs` `range_scope_tests`). UI tests: `ui/src/panels/eth/{eth,ethUnbound}.test.ts`, `ui/src/sdk/treeHex.test.ts` (existing export/pause tests that assume a `from`-less query or one-click export will be rewritten by Opus in B2 to the new contract, never weakened).

## Regression examples (must stay true)
- Old record false 0: stopped `qa-native-eth`, `udp.dstport == 65201` → default query now `complete:false` + `default_lookback`; UI range "캡처 시작 이후" → 1 row `eth:qa-native-eth@seq:1380`.
- Seq cursor grammar `seq:N` / `eN:seq:N` only, never bare numbers (existing native regression).
- Unbound panel: no subscription, no lan1, no writes; existing alias never restarted.
- Kernel drop unknown never "0"; gap rows; sampled live list never labelled complete.
- Export file lives on the daemon machine; dialog never implies browser download.
- Macro/REC backlog untouched (no ETH macro code changes).

## Shared / ROOT requests
- B1: none. B3: `gateway.rs` param schema gains `decode_as` on query/show/stats/export (domain-owned). Generated catalogs/schema are regenerated once at ROOT integration (ROOT confirmed); ETH does not regenerate them. No new RPC, HTTP handler, dependency, package manifest or desktop change. Cargo.lock: none expected.
- Native replay file picker: optional/deferred (ROOT). Shell toast: UART lead (ROOT assignment); ETH coordinates expectations only.

## Verification plan
Per batch: targeted failing-first tests → Sonnet bodies → Opus diff review + same commands. Module: `cargo test -p loom-eth -p loom-eth-decode`, clippy `-D warnings`, `npx vitest run src/panels/eth src/sdk/treeHex.test.ts`, `npx tsc --noEmit` (shared node_modules, no install). Final simulator + native UI evidence = ROOT only (sim channel, stopped-record repro, UDP follow, replay of a fixture file ROOT provides).
