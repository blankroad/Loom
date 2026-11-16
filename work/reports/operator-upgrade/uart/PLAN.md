# UART operator upgrade — PLAN

Status: **READY_TO_INTEGRATE (2026-11-16) — B1–B8 delivered and lead-reviewed; see STATUS.md.** (History: PLAN v3 after ROOT review; batches released one at a time.)
Lead: Opus 5.5 (architecture, contracts, tests, review). Sonnet: product function bodies per leased batch only.
Input: `/Users/ctmctm/workspace/Loom/work/reports/usability-benchmark/RETURN-uart.md` (U1–U8). Rules: `../COMMON.md` v1, `TASK.md`.
Worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-uart`, baseline `610029e`. No Git/GUI/device IO by lead or worker.

## 1. Facts from current code that shape the design

- `uart.input` (viewer keys, paste, Send String) is provided by the **core** with a fixed `loom_proto::worker::Input` shape → pacing
  must not need new `uart.input` fields. Solution: pacing is **port state in the worker** (set at open / by a UART verb).
- TX actor (`crates/loom-uart-worker/src/tx.rs`) already picks lanes at ≤1 KiB chunk boundaries and supports cancellation by id/lane;
  pacing becomes "smaller chunks + per-lane not-before time" inside the same actor (no second timer thread).
- `uart.signal` is registered as milestone **M2 → `E_NOT_IMPLEMENTED`** in `commands.rs`, and the worker has no handler, so the
  existing **Break palette button fails today**. The TX actor already has `ControlOp::{Dtr,Rts,Break}` and every transport
  implements them. U6 therefore needs engine work (not "UI only" as the report assumed).
- `core.events.query` (gateway) already pages ring+journal with `{since, until, filter{domains,channels,kinds}, max≤1000, peek}`,
  60 KiB/page, `next_cursor`, `unreadable` ranges, `E_CURSOR_GAP` (not in recording) and `E_SCAN_LIMIT{resume_from, partial}`;
  no role gate (viewers may call it). → U1-B needs **no new RPC**: the panel declares the query and pages it.
- RX `uart.data` carries `rx_offset` (cumulative across generations) → missing RX bytes inside an export are detectable.
- `loom_worker_sdk::transport::loom_home()` exists → a worker-side log directory needs no shared change.
- `TerminalRegistry.remove()` already reports "last view of this channel closed" → U4 hook lives in owned `workbench.ts`.
- Shell toasts support action buttons (`ToastStore.push(text, {actions})`).

## 2. Finding coverage

| ID | Decision | Batch | Notes |
|---|---|---|---|
| U1-A native continuous log | Worker-owned **daemon log sink** to `$LOOM_HOME/uart/logs/<channel>/`, `.log` or `.jsonl`, rotation + quota, fail-closed. Verbs `uart.log.start/stop`, status in `uart.status.log` and new priority event `uart.log`. Optional `log` param on `uart.open` = start on every open of that session (each generation its own file part). Records carry source identity only — **no stream seq** (see §3 provenance). UI: REC dialog gains a "데몬 파일 기록" section showing the **daemon-side absolute path**, bytes, part, state/reason; existing ≤5 MiB buffer REC kept and renamed "짧은 구간 캡처 (≤5 MiB)". | B4 (engine), B6 (UI) | No browser FSA. Quota defaults: rotate 64 MiB, channel quota 1 GiB (max 8 GiB). At quota → stop `reason:"quota"`, never deletes. Writer thread with bounded 4 MiB queue; overflow → gap line + `dropped_bytes`, never blocks RX/TX. |
| U1-B past range export | Panel pages `core.events.query` (`peek:true`, filter domain uart + channel + kinds `uart.data,uart.state,uart.config,record.gap`) and builds `.log`/`.jsonl` with a header: requested from/to, actual first/last seq+time, epoch, generations, completeness (`complete` / `partial: <reasons>`), gaps (`rx_offset` jumps, `record.gap`, `unreadable`, scan limit, cap). Ranges: 보존 범위 전체(current epoch), 최근 N분, 현재 generation. Caps: 32 MiB output, 2 000 pages; cancel button (AbortSignal). Save through the existing Blob download path; toast says "다운로드 요청" (never "saved"). | B5 | Earlier daemon epochs are excluded and stated. Scrollback search hint points here. |
| U2 auto reconnect | Opt-in `uart.open.reconnect{enabled, window_ms (default 10 000, 1 000–120 000)}`. On reader disconnect the worker tears the port down (writes end per existing rules), emits `uart.state reconnecting{attempt, until_ms}`, polls every 300 ms for the **same validated identity** (same path; if VID:PID:serial was known at open it must match; a non-empty serial may be found at another path), reopens with the same config/profile as **generation+1** and emits `open{reconnect:{from_generation}}`; else `disconnected{reconnect:{gave_up:true, reason}}`. STOP latch, explicit `uart.close`, worker shutdown cancel it; no reopen while latched. | B3 (engine), B6 (UI) | UI: terminal separator `── 재연결 gen N ──`, overlay "재연결 대기 n s [취소]" (= `uart.close`). UI REC buffer and macros still stop on disconnect and never resume (unchanged). Daemon log restarts only via the session's explicit `log` open option. |
| U3 transmit pacing | Port pacing `{char_delay_ms 0–1000, line_delay_ms 0–10 000}` from `uart.open.pacing`, changeable by verb `uart.pacing`; agent `uart.send/run` may override per request (`char_delay_ms`, `line_delay_ms`). Applied in the TX actor to user and agent lanes; Ctrl-C lane never paced. Paced chunk = 1 byte (char delay) or ≤ one line (line delay only). A paced active request holds its lane and blocks lower lanes during its gap; Ctrl-C runs and also cancels an active **paced user** request (`interrupted_by_user`). STOP/HOLD/close cancel agent requests at the next byte boundary; no write can fire after `TxDone` (gaps are condvar waits inside the actor, woken by cancel). User request deadline = 5 s + paced estimate; estimate > 600 s → `input_ack discarded reason:"pacing_too_long"`. Agent estimate > remaining deadline → `E_USAGE` before admission. `uart.config` payload gains `pacing`. | B2 (engine), B6 (UI) | UI: session fields, status line "TX 지연 문자 n ms · 줄 n ms", Send String preview shows estimate. Large-paste confirm card text is shell ActionGate → only the always-visible status line covers paste (stated limitation). |
| U4 port stays open after view close | Keep semantics (closing a view never closes a port). When the **last view** of an **open** channel closes, workbench toast "`<alias>` 포트는 아직 열려 있습니다 (다른 도구가 쓰려면 닫으세요)" with actions **[포트 닫기]** (existing gated `uart.close`) and **[다시 보기]** (`placeChannel`). Session list open chip always readable (B1). | B1 (chip), B5 (toast) | Dock tab context menu "탭 닫고 포트 닫기" is shell/dock (not owned) → optional request R5 below; toast is the in-scope coverage. |
| U5 search / highlight / scrollback | B1: logical-line search across wraps, Aa / .* toggles, error shown as data, hit cap 10 000. Later: per-session highlight rules (≤16 rules, regex → one of 6 palette colours, xterm decorations only, bytes untouched, ≤2 000 live decorations) and scrollback size per session (1 000–50 000, live `term.options.scrollback`). | B1, B6 | Lines older than scrollback → U1-B export (hint text). |
| U6 DTR/RTS | Implement `uart.signal` in the worker as **M1**: `break{duration_ms 1–5000, default 250}`, `dtr|rts{value}` and `dtr|rts{value, duration_ms}` = pulse (set, wait, restore) through `TxHandle::control`; emits `uart.signal{kind,value,duration_ms,state}`; agent units refused while STOP latched (same rule as sends); risk stays R2 (`hooks.rs` unchanged). UI ⋯ row: DTR/RTS toggles (state "알 수 없음" until a signal event — no readback invented) and "리셋 펄스 (RTS n ms)" with `confirm:'card'`. Input lines (CTS/DSR) stay out of scope (engine has no readback). | B3 (engine), B6 (UI) | Fixes today's broken Break button. |
| U7 narrow pane | B1: header modes full/compact/narrow from header width; secondary tools move into the ⋯ row (same nodes, same listeners); alias/state dot/baud always visible; full identity in tooltip; state-coloured dots (pane + session list), tag colour as stripe; non-truncating open chip; distinct theme glyph. | B1 | |
| U8 "장치가 … 다시 열었습니다" toast | **ROOT assigned to UART (R4)**: narrow edits limited to the reopened/visible-panel toast logic in `ui/src/store/channels.ts` (origin wording, line 186), `ui/src/store/shellControl.ts` (new-vs-reopen decision), `ui/src/shell/services.ts` (toast text line 334, `[패널 열기]` action line 345). Contract: a UI-initiated open (this connection's request) shows no "장치가 … 다시" wording; `[패널 열기]` only when no visible panel shows that channel. No other shell logic. Separate contract + lease. | B8 | ETH ack (2026-11-15): no ETH edit planned; expectation — first opens via `ctx.ui.openPanel` (e.g. eth.packets "기존 채널 열기") must never say "다시 열었습니다"; ETH tests assert no shell toast text. |

Prior macro/REC backlog: unchanged semantics; REC buffer path and `exportBuffer` kept; tests for them must stay green each batch.

## 3. Fixed design decisions (contracts later batches will detail)

**RPC/schema (domain-owned, backward compatible; all new fields optional, absent = today's behaviour):**
- `uart.open` + `pacing{char_delay_ms,line_delay_ms}`, `reconnect{enabled,window_ms}`, `log{format:'log'|'jsonl', ts:bool, rotate_mib, quota_mib}`.
- `uart.send`/`uart.run` + `char_delay_ms`, `line_delay_ms` (override).
- New verbs: `uart.pacing{channel, char_delay_ms, line_delay_ms}` (write_class none, roles `RoleSet::WRITE`, M1);
  `uart.log.start{channel, format, ts, rotate_mib, quota_mib}` / `uart.log.stop{channel}` (write_class none, `RoleSet::WRITE`, M1).
- `uart.signal` milestone M2 → M1 (risk R2 unchanged, write_class Request unchanged).
- New event kind `uart.log` (priority) `{state:'recording'|'stopped', path, part, bytes, dropped_bytes, reason?}`;
  `uart.state` gains optional `reconnect{…}`; `uart.config` gains optional `pacing`; `uart.status` gains `pacing`, `reconnect`, `log`.
- Idempotent reopen rule (`worker.rs` open) compares pacing/reconnect/log too (different → existing `E_CONFIG` "close it first").
- Errors: existing codes only (`E_USAGE` bad/too-long pacing, `E_CONFIG`, `E_NOT_OPEN`, `E_STOPPED`, `E_IO` for log disk errors in status/event, not as RPC failure of unrelated calls).

**Generation/provenance (ROOT architecture review, PLAN v3):** reconnect always increments generation via the existing kept-core path;
nothing is reconstructed from current device state. Two provenance classes are kept distinct and labelled in every file header:
- **Native daemon log (U1-A, worker-written)** — written in the worker *before* hub publication. Verified in code: the worker's
  `Emitter` knows only the source identity `(producer_id, producer_epoch, source_seq)` (`reserve_seq()`, `producer_id()`,
  `producer_epoch()`; `loom-proto/src/event.rs:5` "source identity … but **no** stream seq"). The worker does **not** know the
  canonical stream epoch/seq, so the native file **never contains a journal/stream seq field**. Each record carries: `generation`,
  `dir`, `origin`, `rx_offset` (RX), `request_id`/`client_id` (TX), `producer_id`, `producer_epoch`, `source_seq`, host
  `mono_ns` + `wall_ns` + `clock_id` (as stamped by reader/TX actor), `len`, bytes, and `emit` = `ok` | `failed` (the journal
  emission of that same chunk was refused — e.g. queue full — so the journal may lack it; the native file still has it). The
  header states: `provenance: native-worker-log; stream_seq: not available (assigned by the daemon journal after publication);
  cross-reference via (producer_id, producer_epoch, source_seq)`.
- **Historical export (U1-B, from `core.events.query`)** — published records, so each line carries the canonical `stream_epoch`
  and `seq` plus the same source identity and `ts`. Header: `provenance: daemon-journal-export`.
No core change is implied by either. Native logging is **off unless the user opts in** (`uart.log.start` or the session's explicit
`log` open option); its queue (4 MiB) and quotas (rotate 64 MiB, channel quota 1 GiB default / 8 GiB max, stop at quota, never delete) are fixed bounds.

**Bounded work:** every queue/list has a cap stated in its batch (pacing estimate ≤600 s user; export ≤32 MiB/2 000 pages;
log queue 4 MiB; reconnect window ≤120 s, poll 300 ms; highlight ≤16 rules/≤2 000 decorations; search ≤10 000 hits).

## 4. Batches (lease = files; lead edits none of a batch's files while it is leased)

| Batch | Scope | Files (lease) | Depends |
|---|---|---|---|
| **B1** READY | U5a search, U7 | `ui/src/panels/uart/{scrollSearch.ts,paneLayout.ts,terminalPanel.ts,uart.css}`, `ui/src/workbench/uart/{icons.ts,UartWorkbench.tsx,bench.css}` | — |
| B2 | U3 pacing engine | `crates/loom-uart-worker/src/{tx.rs,worker/send.rs,worker/util.rs}` + pacing parse in `worker.rs`, `crates/loom-uart/src/commands.rs` | — |
| B3 | U6 signal + U2 reconnect engine | `crates/loom-uart-worker/src/{worker.rs,worker/port.rs,worker/reconnect.rs(new)}`, `crates/loom-uart/src/{commands.rs,kinds.rs}` | B2 returned (shares worker.rs/commands.rs) |
| B4 | U1-A log sink engine | `crates/loom-uart-worker/src/worker/logsink.rs(new)`, hooks in `worker/port.rs`, `worker.rs`; `commands.rs`, `kinds.rs` | B3 returned |
| B5 | U1-B export, U4 toast | `ui/src/panels/uart/journalExport.ts(new)`, `terminalPanel.ts` (REC dialog), `ui/src/workbench/uart/{workbench.ts,recorder.ts}` | B1 returned |
| B6 | UI for U1-A/U2/U3/U5b/U6 | `workbench.ts` (Session fields), `UartWorkbench.tsx` (dialog), `terminalPanel.ts`, `uartState.ts`, `sendString.ts`, `ui/src/panels/uart/highlight.ts(new)` | B2–B5 |
| B7 | CLI/help/skill | `crates/loom-uart/src/cli.rs`, `commands.rs` help, `skills/loom-uart/SKILL.md` | B2–B4 |
| B8 | U8 shell toast (R4) | `ui/src/store/channels.ts`, `ui/src/store/shellControl.ts`, `ui/src/shell/services.ts` — named toast functions only | any time; separate lease |

Engine batches B2–B4 touch no UI files, so the lead specifies B2 while B1 is leased.
Rust focused commands (all batches): `CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo test -p loom-uart-worker` and `-p loom-uart`, plus `cargo clippy -p … --all-targets`. No workspace-wide builds. Cargo.lock: no dependency change planned (none expected to change).

## 5. Tests (regression first, then function, then module)

- Regression examples (fail on baseline): wrapped-line search (B1, verified failing `0건`); case-insensitive default (B1, `1/2` on baseline);
  Break button → `E_NOT_IMPLEMENTED` today (B3); paced 3-line paste must not leave the device FIFO in one burst (B2, sim transport timing);
  disconnect → no reopen today (B3, pty unplug/replug harness); journal range older than REC start not exportable today (B5).
- Function tests per contract (each batch section in FUNCTIONS.md), e.g. pacing gap schedule, STOP during a paced gap leaves no later write, reconnect identity mismatch refuses, log quota stop without deletion, export completeness/gap header.
- Module: `crates/loom-uart-worker/tests/*` (sim + pty harness, owned fixtures), UI panel tests via `sdk/testing/harness`.
- ROOT later: simulator + native/browser co-receive evidence (GUI is ROOT-only). Physical USB re-enumeration is **not** claimed.

## 6. Shared requests / external dependencies (need ROOT owner assignment; nothing edited by UART)

- **R1** Generated schema/catalog integration for new UART verbs/params/event kind (`pnpm check:schema` / `scripts/gen-schema.ts`, any combined registry) after B2–B4 contracts are accepted.
- **R2** ACCEPTED by ROOT: read-only `core.events.query` with `peek:true` for export; truthful gaps/caps required.
- **R3** `loom_worker_sdk::transport::loom_home()` used from the UART worker for `$LOOM_HOME/uart/logs` (read-only use of SDK; no change).
- **R4** ASSIGNED to UART by ROOT → batch B8 (paths verified in this worktree).
- **R5** DEFERRED by ROOT (optional dock menu). Required coverage for U4 = last-view-close toast (B5).
- **R6** ROOT performs the actual native download verification; no desktop change requested.

## 7. Risks

- Pacing changes TX actor scheduling (highest risk): mitigated by keeping the unpaced path byte-identical (pacing 0 → today's code path) and by the existing tx/tx_cancel test suites staying green.
- Reconnect + kept-core generation transfer interacts with close/open locking: reconnect uses the same `lifecycle` mutex and `kept` map, never a second port slot.
- Search regex can stall on pathological user patterns: shown in the UI as a hint while regex mode is on; no latency guarantee is claimed; search is bounded (query 256, hits 10 000, logical line 256 rows).
- Header reparenting relies on `head.clientWidth`; hidden tabs report 0 and keep their mode.
