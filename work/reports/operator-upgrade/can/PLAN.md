# CAN operator upgrade — PLAN (v1 · all batches done → READY_TO_INTEGRATE, see STATUS.md)

Lead: Opus 5.5 (architecture, contracts, scaffolding, tests, review). Worker: Sonnet, product function/callback bodies only (COMMON.md v1 + its latest clarification: CSS, top-level imports/types/constants/signatures/manifests are Opus scaffolding).
Baseline 610029e · worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-can` · input `/Users/ctmctm/workspace/Loom/work/reports/usability-benchmark/RETURN-can.md`.
Function contracts: `/Users/ctmctm/workspace/Loom/work/reports/operator-upgrade/can/FUNCTIONS.md` (B1 complete; B2–B5 to follow there, on files B1 doesn't touch where possible).

## 1. Finding coverage

| ID | Finding | Decision | Batch |
|---|---|---|---|
| CAN-1 P1 | Trace time is mono seconds only; no wall/relative/Δ | Time column modes `wall` (default, ts.wall_ns through the shared `fmtWall`, same as UART) · `rel` (s, against session start or the selected row) · `delta` (ms, against the previous **shown** row). Durations only between rows with the same `ts.clock_id`, otherwise `≠clock`. Missing wall → `--:--:--.---`, never derived from mono. Receive view header says `마지막 s (창 시작 기준)` and the cell title shows the row's wall time. | **B1** |
| CAN-3 P1 | TX list lives only in panel memory | Saved per channel alias in this browser profile (localStorage, same precedent as ETH filterBar). Restored on mount. Versioned JSON export/import by copy/paste (`loom.can.txlist` v1). Entries are cansend text accepted by both `loom can send` and the send line. `cycle_ms` is carried now for CAN-2. Counters are not saved. Daemon or team-shared storage is **excluded** (it would need a new shared/core store owner); teams share via the JSON document. | **B1** |
| CAN-7 P2 | Data column truncated, no full bytes | Data cell `title` = full hex. Row detail line on Enter or double-click (full bytes, len/DLC, seq, epoch, request_id, origin, wall/mono/clock), with a copy button. Keyboard ↑/↓ selection extends B1's row selection. Column resize is **excluded** (fixed grid; the detail line plus title covers full bytes at any width, FD64 too). | B2 |
| CAN-8 P2 | UI id filter has no ranges (CLI has) | `parseIdFilter` accepts `a-b` ranges, `0x`/bare hex, and the `x` suffix/prefix. ≤256 single ids + ≤64 ranges. Shows the normalised filter. Bare tokens stay **hex** (UI convention, the id column is hex). Bare decimal from CLI (`768`) is a documented difference: the normalised chip shows what was applied. `TraceFilter.ids` becomes an `IdMatch` (`has(id)`, `key`); a `Set` still satisfies it. | B2 |
| CAN-6 P1 | No UI channel close | Manifest action `close` → `can.close{channel}` (right operate, confirm `card`). The bound panel shows a `연결 끊기` button. Before the confirm card, one read of `can.periodic.list{channel}` + `can.status{channel}` shows jobs (all owners) and ISO-TP endpoints that will stop. Closing a pane is still view-only, and the button text and title say so. After close: toast, and the panel shows `채널 닫힘` from `can.channel_state` (onControl) or the close result. A user close is allowed under STOP (the worker already allows origin user); the engine stops that channel's jobs (`channel_closed`). | B2 |
| CAN-2 P1 | Periodic engine exists, UI says unsupported | TX editor `Cycle Time ms` (0 = manual, 1..60000). Row actions: start/stop/update via `can.periodic.start/stop/update` (server job, no browser timer). Job table from `can.periodic.list{channel}` (one read at mount and on demand) plus live `can.job_state` (onControl). Columns: job, owner, origin, state, period, sent/missed, lease_left/max duration. Jobs of other owners are read-only; `넘겨받기` = explicit `can.periodic.takeover` behind a confirm card. The engine lets any `origin=user` write bypass ownership (`periodic.rs:427`); the UI stays conservative and never uses that bypass implicitly. STOP latch shows `STOP — 주기 송신 정지됨`, and starts are disabled while latched. No lease renew loop: UI starts use the engine default `max_duration_ms` (1 h), which is shown, or a user value. | B3 |
| CAN-4 P1 | Export library not wired | New RPC `can.export{channel?, since?, until?, ids?, dir?, origins?, format: blf\|asc, path?}` in `crates/loom-can/src/export.rs`. It scans the journal with the same filter rules as `can.trace.query`, maps to `loom_can_export::ExportRecord` (frames + scan gaps as `Gap{scope}`), and calls `export_file` (partial → rename, sidecar provenance). The default path is a new daemon temp file `loom-can-<ch>-<rid>.<ext>` (create_new); an explicit `path` must be absolute and must not exist. Bounds: ≤1,000,000 frames, scan budget, 600 s. Result: `{path, sidecar, format, frames, gaps, truncated, complete, selected{since,until}, range{from,to}, next_cursor, wall_anchor_known}`. Role READ, manifest action class `job` right `watch` (same as `eth.export`). CLI `loom can export`. UI `내보내기` → result dialog stating the file is on the daemon machine, not a browser download. | B4 |
| CAN-5 P1/P2 | DBC decode not in UI | DB chip `DB: 없음 \| <file> · sha8`. `can.db.info{channel}` once at bound mount and on `can.db_changed`. Load dialog: absolute daemon path + encoding → `can.db.load` (query, UI requires operate as mistake prevention, policy unchanged). Message-name column in the Receive view from `can.db.catalog` pages (bounded ≤4,096 messages / ≤20 pages). Row expand → one `can.last{id, decode:true}`. Trace detail (B2) → `디코드` = one `can.trace.query{since:seq,until:seq,max:1,decode:true}`. Engine fix (owned): `decode` is missing from the `can.trace.query` / `can.last` param schemas (`additionalProperties:false`) although the handlers accept it → add it as an optional boolean (backward compatible) + a schema test. Signal graphs and editors are excluded. | B5 |
| §3 engine-unsupported | Real adapters, bitrate, listen-only, bus load/error counters, Error Generator, RTR send, CAN-TP diag | **Excluded / external dependency**: no engine backend exists. The UI keeps its honest disabled labels. CAN-TP belongs to the UDS module. No change. | — |

Prior macro/REC backlog: not touched. B1 changes no send, REC or STOP path.

## 2. Batches (small, reviewable; each = exclusive file lease to Sonnet)

- **B1 (ready)**: CAN-1 + CAN-3. Files: kv.ts, timeFmt.ts, txStore.ts (new) · traceModel.ts, traceView.ts, rxSummary.ts, txList.ts, rxtxView.ts, canTrace.ts (bodies). Scaffold, CSS and tests are done by Opus.
- **B2**: CAN-7, CAN-8, CAN-6. Files: new `idFilter.ts`, `rowDetail.ts`, `closeChannel.ts`; then traceView/canTrace/traceModel/rxtxView body edits after B1 review. Manifest: action `close`, query `can.periodic.list`.
- **B3**: CAN-2. New `periodicJobs.ts` (pure job table reducer) and `periodicView.ts`. Edits to txList/rxtxView/canTrace. Manifest: actions `periodic.start|stop|update|takeover`, all request class, right operate; takeover confirm `card`; stop has `safetyDirection: 'reduce'`.
- **B4**: CAN-4 Rust + CLI + UI button. New `crates/loom-can/src/export.rs`, `tests/export.rs`. Edits: gateway.rs registration, cli.rs, lib.rs const, Cargo.toml dep, skills/loom-can/SKILL.md.
- **B5**: CAN-5. New `dbChip.ts`, `dbNames.ts`. Schema edit in gateway.rs (B4 and B5 Rust are serialized on gateway.rs).

Opus may write B2+ architecture and tests on files B1 does not lease while Sonnet runs B1.

## 3. Shared needs / requests to ROOT (none block B1)

1. **Cargo.lock** (B4): `loom-can` gains the path dep `loom-can-export` (already a workspace member, lock entry exists). Only loom-can's dependency list changes. Generated side effect; ROOT serializes.
2. **No SDK / shell / NetMux / HTTP / desktop changes** in any batch. Export returns a daemon-side path; no download handler is requested.
3. **Generated catalogs**: if ROOT has a combined method/help catalog that lists `can.*` RPCs, regenerate it at integration for `can.export` (B4) and the `decode` schema fields (B5).
4. **GUI acceptance** (ROOT only, simulator): time modes on a live sim, TX list across close/reopen, close confirm, periodic start/stop/takeover with the agent CLI, export file on disk, DB decode with a fixture DBC.
5. `ui/node_modules` symlink is used read-only. No new npm or crates.io dependencies.

## 4. Invariants (all batches)

- Device/DB/import text → `textContent` only. Imported JSON is untrusted and validated by the single `parseSendLine`.
- Unknown is not zero: wall `0` → unknown; clock mismatch → `≠clock`; load/save failures are explicit statuses, never an empty list.
- Bounded: ClockDict ≤64 ids; TX list ≤32; import text ≤65,536 chars; Δ lookback ≤16 display entries; no timers added (persistence runs on the panel's existing frame callback plus a flush on dispose).
- Existing behaviour kept: send path, ActionGate, dry-run preview, gap rows, ring bounds, follow/pause, unbound "no RPC on mount".

## 5. Validation plan

- Regression examples (failing first, then fixed by B1): `timeFmt.test.ts`, `txStore.test.ts`, `traceTime.test.ts`, `canTime.test.ts`, `txPersist.test.ts` — 55 tests fail on the scaffold today; the existing 84 CAN tests pass.
- Per batch: `npx vitest run src/panels/can --maxWorkers=2`, isolated `npx tsc -p tsconfig.{app,node}.json --noEmit --tsBuildInfoFile ../work/typecheck/*.tsbuildinfo`, `npx eslint src/panels/can` (in `ui/`). Full UI suite: ROOT only at the final freeze. For B4/B5 also: `CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo test -p loom-can` (+ `-p loom-can-export`).
- Module integration (ROOT): sim daemon + UI co-receive, agent CLI vs UI parity (same journal rows / same jobs).
