# P1 — `crates/loom-can/src/worker.rs` (K1: can-sim worker on the CORE worker SDK)

Rules: `docs/impl/can/tasks/p0-common.md` (all crates are now **workspace members**; check from the worktree root `/Users/ctmctm/workspace/Loom/.worktrees/can` with `cargo fmt --all --check`, `cargo clippy -p loom-can --all-targets -- -D warnings`, `cargo test -p loom-can`). Always use `/Users/ctmctm/workspace/Loom/...` paths. Do not commit.

**Scope (only these files):** `crates/loom-can/src/worker.rs`, new `crates/loom-can/tests/worker*.rs`, optional `crates/loom-can/src/sim_default.toml` (embedded default nodes). `lib.rs`, `schema.rs`, `Cargo.toml` belong to the lead — put needed changes (e.g. a new dependency) in the report. `gateway.rs`/`cli.rs` are being written by another worker at the same time — do not touch them. Report: `docs/impl/can/reports/p1-worker.md`.

## Read first
- Sample worker `crates/loom-echo/src/worker.rs`; SDK `crates/loom-worker-sdk/src/{lib,emit,clock}.rs`; testkit `crates/loom-testkit/src/fake_worker.rs`.
- Contract `docs/contract/02-events.md`, `05-worker-protocol.md`, `04-safety.md` (STOP latch A3).
- `crates/loom-can-sim` (SimHub, LocalEndpoint, NodeConfig/parse_nodes, Origin, latch), `crates/loom-can-frame`, `crates/loom-can/src/{lib,schema}.rs`, `docs/impl/requests/can-ui.md` (the event shapes promised to the UI).
- Plan `docs/plan/can/plan.md` §3.3.1 (threads), §9.1 (sim), §9.4 (bounds), §10.6 (STOP).

## Build `CanSimWorker: loom_worker_sdk::Worker` (`KIND = WORKER_KIND_SIM`)
1. **Device state:** one `loom_can_sim::SimHub` (real `MonotonicClock`; tests use `ManualClock` via a constructor that takes a clock). Channels are sim buses. `on_call("can.open", {channel, bus?, nodes?: "default"|toml})` attaches: one **sender endpoint per origin** (Agent/User/Script — SimHub fixes origin per endpoint) plus one **observer endpoint** whose `LocalReceiver` is drained into events; starts the default fake ECU (BCM: `0x300` → reply `0x301` copying data after 5 ms; periodic `0x310` every 100 ms whose byte0 follows the last `0x300` byte0) unless `nodes` says otherwise. Emit `can.channel_state{state:"open", backend:"sim", channel, generation}` on the priority lane. `can.close`, `can.status` (channels, endpoint drops, latch, node list), `can.node.list/start/stop` are calls too.
2. **Writes (`on_write`):** parse `schema::SendParams` from `w.op`; validate with `resolve_frame()` (usage errors → `Failed` + error). STOP: if `cx.latch()` is on and origin ≠ User → `Failed` with `E_STOPPED`-equivalent (see echo) and nothing on the bus. Emit `can.tx` write rows (`schema::TxPayload`, top-level `request_id`) for `accepted` then `sent` (and `failed`) on the priority lane; send `count` frames through the origin's sender endpoint with `gap_ms` spacing. Map each `send()` source_seq → `(request_id, frame_index)` in a **bounded** map so the observer drain can tag the tx echo `can.frame` with the same top-level `request_id` (+ `payload.frame_index` when count > 1) — one Trace row per request (can-ui.md §1). Return `WriteOutcome{state: Sent, detail:{source_seq, frames}}`. Bursts with gaps must not block the worker: schedule remaining frames in the pump (bounded queue, ≤ `MAX_SEND_COUNT`).
3. **Pump:** a dedicated thread (or the structure the SDK expects — read `run_worker`/`WorkerCtx` docs; `run_worker` itself is a CORE stub until P1 C12, so expose a `pump_once(now)`/`run_pump` the worker loop calls, and test it with FakeWorker) that calls `hub.poll()`, drains the observer receiver and emits `can.frame` for every record with `schema::FramePayload` (`dir`/`origin` from the SimRecord: Device → `Emitter::rx`; Agent/User/Script tx echo → `Emitter::priority` with origin + request_id, because the SDK has no data-lane draft method yet — note this in the report), sleeps until `next_deadline_ns` (or ≤ 1 ms). Observer drops → `source.gap`/gap counters (decide which per contract 02/A13 and explain).
4. **STOP latch:** `on_latch(on)` maps to `SimHub::latch(bus, &[Agent, Script])` / `release`; fake ECU nodes keep running (A3, plan §10.6).
5. **C2:** every map/queue bounded with explicit overflow behavior (request map, burst queue, channels ≤ 16).

## Tests (FakeWorker + ManualClock, no daemon)
- §2.0 loop: open ch0 → agent write `300#01` → events: `can.tx` accepted/sent (request_id r1), `can.frame` tx with request_id r1 origin agent, then after 5 ms `can.frame` rx `301#01` origin device; user write `300#00` → user tx + `301#00`. Check order, payload shape (`dlc`, `len`, hex bytes), request_id tagging.
- Burst `count=3 gap=10ms`: three echo frames with frame_index 0..2 at 10 ms spacing, one request.
- STOP: latch on → agent write fails, no frame; user write passes; periodic 0x310 continues; release → agent passes.
- Usage error (bad hex, count 0) → Failed, nothing sent. Bounds of the request map / burst queue.
- `can.status` / `can.close`.

Stop when done and the report is written.
