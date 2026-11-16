# ETH operator upgrade — READY_TO_INTEGRATE

Lead: ETH Opus 5.5 · worker: upgrade-eth-sonnet (function bodies only) · worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-eth` · baseline 610029e · 2026-11-15. No git add/commit (ROOT owns the index). No GUI, device IO, default daemon/app, STOP or policy touched.

## Findings → result
| ID | Delivered | Batches |
|---|---|---|
| ETH-01 (P0, observed false "complete · 0") | Daemon: every journal read (`eth.query/stats/follow/export`) returns `scope`, `scanned`, ordered `partial_reasons`, `resume_cursor`, strict `complete` (= no partial reason; never with default look-back, loss, row/output cap, scan/decode stop) and separate `scan_complete` (coverage only). Capture-start anchor `start_cursor` (`e<E>:seq:<N>`, observed by the reducer, `null` when unknown) in `eth.capture.status` + snapshot. UI: complete views always send an explicit `from` (anchor, else labelled "최근 10분 (캡처 시작 위치 모름)", recent N, typed seq), badge 완전/부분/미확인 from daemon fields, scope text, 1,000-row pages (older/newer/계속 스캔), export range dialog (current view / capture / selected row / seq + filter) with requested range + 완전성 in the result, follow with the on-screen range. CLI trailer `complete` / `partial: …` / `resume: --since`. | B1, B2, B3a |
| ETH-02 (P1) | `eth.show` tree nodes carry `name` + daemon-made, compile-checked `filter`; SDK TreeHex optional `name/filter` + `onNodeContext` (default behaviour unchanged); field menu (적용 / 준비 / &&·|| 준비 / 필드명·필터 식 복사) via right click, ContextMenu/Shift+F10, detail-bar button; status bar shows the field name. Apply-as-Column deferred (P2 per report). | B3a, B4a |
| ETH-03 (P1) | Decoder `udp.stream` (own index space, bounded 1,024, idle/LRU eviction with warning); worker publishes `stream` + `stream_proto`; gateway uses the worker number for `udp.stream` filters and never exposes request-local numbers; follow filters by index space; UI follows by `stream_proto` (DoIP-over-UDP rows follow UDP). | B3a, B4a |
| ETH-04 (P1) | GUI file replay with a typed **daemon-machine** path, speed, repeat (capture form backend `replay`, unbound "파일 열기 (replay)…", saved replay configs, REPLAY chips). Native file picker deferred by ROOT. BPF stays live-only (engine); original-file frame number not in payload (not invented). | B4a |
| ETH-05 (P1) | Unbound start: existing daemon channels first under "기존 채널 이어 보기", guidance as wrapped whole lines, duplicate toolbar start/add/stop hidden, same-alias config → "기존 채널 열기" (no RPC; dialog still refuses restarting it). Shell reopened toast = UART lead (ROOT assignment; ETH expectations sent). | B2 |
| ETH-06 (P2) | Read-time Decode As (`decode_as` ≤ 8 rules on query/show/stats/export, `--decode-as tcp|udp:PORT=doip|someip`), rule overrides well-known ports, `decode_as` warning per packet, `decode_as_applied` rows, echo in responses; UI per-channel rules dialog (per-viewer storage), markers and notes. Live list / records unchanged by design. | B3b, B4b |

Agent docs: `skills/loom-eth/SKILL.md` (ranges/`complete` semantics, `start_cursor`, UDP follow, tree filters, replay, `--decode-as`); every documented command line is parsed by test `skill_examples_parse`.

## Changed files (all inside ETH ownership; no Cargo.lock / package manifest / shared file)
Rust: `crates/loom-eth-decode/src/{fields,lib,net,stream}.rs`, new `decode_as.rs`; `crates/loom-eth/src/{cli,gateway,model}.rs`, `gateway/{common,evidence,handlers}.rs`, `worker/{pipeline,tests}.rs`; tests `crates/loom-eth/tests/{cli,support/mod}.rs`, new `range_scope.rs`, `udp_follow_tree.rs`, `decode_as.rs`; new `crates/loom-eth-decode/tests/{udp_stream,decode_as}.rs`.
UI: `ui/src/panels/eth/{dialogs,ethModel,ethPanel,ifaceConfigs}.ts`, `eth.css`, new `decodeAs.ts`; `ui/src/sdk/treeHex.ts`; tests `eth.test.ts`, `ethUnbound.test.ts`, `treeHex.test.ts`, new `ethRange/ethComplete/ethB4/ethFields/ethDecodeAs.test.ts`.
Docs: `skills/loom-eth/SKILL.md`.
`git diff --stat`: 24 files changed, 2488 insertions(+), 318 deletions(-) + 12 new files.
Module hash (`git diff` + the 12 untracked files, sorted): `5ed72bd8660002fc82d091853be1e31162e0eef4ee4abd4129c36fbbe313b2ed`.

## Verification (Opus, final, 2026-11-15)
```
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test --no-fail-fast -p loom-eth-decode -p loom-eth -p loom-eth-filter -p loom-eth-pcapng   132 passed, 0 failed (+ cli skill_examples_parse: cli 10/10)
cargo clippy -p loom-eth-decode -p loom-eth --all-targets -- -D warnings                         clean
cargo fmt -p loom-eth-decode -p loom-eth --check                                                  clean
cd ui
npx vitest run src/panels/eth src/sdk/treeHex.test.ts                                             8 files, 103/103
npx vitest run                                                                                    74 files, 807/807 (whole UI suite, once)
npx tsc -p tsconfig.app.json  --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo         exit 0
npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo        exit 0
npx eslint src/panels/eth src/sdk/treeHex.ts src/sdk/treeHex.test.ts                              exit 0
scaffold/todo markers in product files: 0
```
Not run (ROOT integration scope): cross-module `loom-gateway` tests (only `can_eth_smoke` touches eth: default `eth.query` → still ok:true), generated catalogs (none in-tree carry eth command schemas; `loom-schema` drift covers loom-proto only), external simulator + native/browser co-receive.

## Wire / compatibility notes for ROOT
- Additive: response fields `scope/scanned/partial_reasons/resume_cursor/scan_complete`, export `filter`, status/snapshot `start_cursor`/`start_seq`, pkt payload + rows `stream_proto`, tree `filter`, `udp.stream` field, `decode_as` param/echo, rows `decode_as_applied`.
- **Semantic change (intended):** `complete` is strict. No in-repo consumer relied on the old meaning (checked); agents are told in the skill.
- CLI requests now include `decode_as: null` when the flag is absent (like `filter: null`).
- Domain param schemas (`gateway.rs`) gained optional `decode_as` on eth.query/show/stats/export.

## Risks / known limitations
- Without the reducer having seen a generation's `starting` record (daemon restart, mid-generation join) `start_cursor` is `null`; the UI then uses an explicit, labelled 10-minute fallback — honest but not whole history. Reverse/tail journal scan remains a CORE ask.
- Scan budget 64 MiB per request unchanged; long captures are paged/continued (`resume_cursor`), not single-shot.
- Decode As is read-time only: the live (sampled) list and UDS↔DoIP links are not re-interpreted (stated in UI and skill).
- Decode As per-viewer storage: all-digit aliases are evicted first beyond 32 stored channels (JS key order); preference loss only.
- Self-reported: one early plain `tsc -p tsconfig.app.json` wrote the shared `ui/node_modules/.tmp/tsconfig.app.tsbuildinfo` (21:44); all later checks isolated. ROOT's forced combined check covers it.

## Needed from ROOT
Integration of the worktree diff; simulator + native UI evidence: (1) stopped-record repro — default `eth.query` now `complete:false, partial_reasons:[default_lookback]`, UI 캡처 시작 이후 range finds `udp.dstport == 65201`; (2) UDP row → 스트림 따라가기 enabled (UDP); (3) replay of a ROOT-provided fixture via 파일 열기; (4) field menu → filter applied; (5) Decode As on the DoIP fixture port.
Reviews: `B1-REVIEW.md`, `B2-REVIEW.md`, `B3A-REVIEW.md`, `B4A-REVIEW.md`, `B3B-REVIEW.md`, `B4B-REVIEW.md`; contracts `FUNCTIONS.md`; plan `PLAN.md`.
