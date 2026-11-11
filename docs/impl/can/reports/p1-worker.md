# P1 report: `crates/loom-can/src/worker.rs` (K1, can_sim worker)

## What I built
- `CanSimWorker: loom_worker_sdk::Worker` (`KIND = WORKER_KIND_SIM` = `"can_sim"`). It owns one `SimHub`. A channel is one sim bus. Per open channel it attaches 3 **sender endpoints** (Agent/User/Script, so the origin is fixed by SimHub) and 1 **observer endpoint**. The observer's `LocalReceiver` is drained into events.
- `sim_default.toml` (embedded as `DEFAULT_NODES_TOML`): the BCM. It replies `0x300 → 0x301` with a copy of the data after 5 ms. It also sends `0x310` every 100 ms (first one at +50 ms, the same as the `loom-can-sim` fixture); byte0 follows the last `0x300` byte0.
- Shared state sits in `Arc<Mutex<Core>>`. Critical sections are short and never sleep. The SDK callbacks and the pump share it.
- **Pump** `pump_once()`: `hub.poll()` → send due burst frames → `poll()` again (zero-delay replies) → drain observers → emit the final `can.tx` rows of bursts that ended → return `next_wake_ns`.
  - `PumpMode::Thread`: thread `can-sim-pump`, started by the first `can.open`. It sleeps until the next deadline, at most `PUMP_MAX_SLEEP` = 1 ms, and is woken by writes and calls. On drop it is stopped and joined.
  - `PumpMode::Manual`: the caller runs the pump (tests, or a future SDK loop).

### Event mapping
| What | Event | Lane |
|---|---|---|
| Device (node) frame | `can.frame`, `Emitter::rx` (origin device, dir rx) | data |
| Loom tx frame (agent/user/script) | `can.frame` with that origin and dir. The sending channel adds top-level `request_id` + `client_id`, and `payload.frame_index` when count > 1 | **priority** (the SDK has no data-lane draft with origin/request_id yet) |
| Write state | `can.tx` (`schema::TxPayload`, top-level `request_id`) | priority |
| Open/close | `can.channel_state{state, backend:"sim", channel, bus, generation}` | priority |
| Observer queue overflow | `source.gap{count:"<n>", cause:"sim_observer_overflow"}` on the channel, emitted before the next frame | priority (SDK `source_gap`) |

- All events carry `env: sim` and the channel `generation` (counter per open, starting at 1).
- `ts` is the SDK clock minus the frame's age on the bus, with `ts.source = synthetic`. Spacing is therefore exactly the bus spacing even when the pump runs late.
- **Why `source.gap` for observer drops (A13):** the sim bus plus the observer queue act as the driver. A record the observer missed never reached Loom: no Loom `source_seq` was assigned and nothing was journaled. That matches "loss before Loom received it" (like a driver overrun). `record.gap` (journal) and `display.gap` (screen only) do not fit.
  - Detection is a jump in the bus `source_seq`. The observer sees every record on its bus, so the count is exact.
  - Failed emits are counted and retried before the next record.

### Writes (`on_write`, `op` = `schema::SendParams`)
1. Parse the op and call `resolve_frame()`. Errors → `Failed` + `E_USAGE`, nothing sent and no row (`TxPayload.frame` needs a frame). `dry_run` → `E_USAGE`, because the gateway answers previews.
2. Unknown channel → `E_NOT_OPEN` plus a `failed` row.
3. STOP: latch on and origin ≠ User → `E_STOPPED` plus a `failed` row; nothing reaches the bus.
4. A gap burst when the burst queue is full → `E_BUSY` plus a `failed` row, before `accepted`.
5. Otherwise the rows are `accepted(sent 0)` → frames sent → `sent(sent n)`, then the tx echo is drained before returning. The return value is `WriteOutcome{Sent, detail:{source_seq, frames, sent, pending}}`. `source_seq` is the **sim bus seq** of the first frame.
- `count > 1, gap_ms = 0`: every frame is sent inside the call.
- `gap_ms > 0`: frame 0 is sent now and the rest is one `Burst` entry run by the pump.
  - The latch is checked again before every unit.
  - When the burst finishes, a final `sent(sent=count)` row follows after the last echo. A failure gives `failed(sent=k)`.
- Tag map: one `VecDeque<Tag>` per channel, in bus-seq order. It maps bus `source_seq` → (`request_id`, `client_id`, `frame_index`). Older tags whose echo was dropped are discarded as stale.

### STOP (`on_latch`)
- On: `SimHub::latch(bus, [Agent, Script])` on every open bus, and queued agent/script bursts end with `failed E_STOPPED`. User sends and user bursts continue. Nodes keep running (A3, §10.6). A channel opened while latched is latched immediately.
- Off: `release`.

### Calls
| Call | Args | Result |
|---|---|---|
| `can.open` | `{channel, bus?=channel, nodes?: "default"(default) \| "none" \| <TOML>}` | `{channel, bus, backend, generation, latched, nodes, nodes_already_running}` |
| `can.close` | `{channel}` | Ends pending bursts (`failed E_NOT_OPEN`), drains, detaches the endpoints, removes the nodes this channel started, emits `channel_state closed` |
| `can.status` (`RPC_STATUS`) | `{channel?}` | Per channel: frames total / by origin, latched origins, `latch_rejected`, observer `{delivered, drops, gaps, queued, capacity}`, `source_gap_frames`, `emit_failed`, `tags{live,max,evicted,stale,untagged_echo}`, `pending_bursts`. Top level: latch, nodes, burst counters |
| `can.node.list` | — | `{nodes:[{name, bus, channels, state, tx_frames, fault_dropped, deadline_overflow, reply_errors, pending}]}` |
| `can.node.start` | `{channel, nodes}` | Starts nodes on the channel's bus |
| `can.node.stop` | `{channel, name}` | Stops a running node by name |

- Nodes always run on the bus of the channel; their TOML `channel` is overwritten.
- A node whose name already runs on that bus is skipped (no double BCM when two channels share a bus).
- If `can.open` fails half-way, everything it attached or started is rolled back.
- `on_job` → `E_NOT_IMPLEMENTED` (periodic TX is M2). `on_input` → `E_UNSUPPORTED`.

### Bounds (C2): `WorkerLimits` (each value raised to at least 1)
| Limit | Default | When full |
|---|---|---|
| `max_channels` | 16 (`MAX_CHANNELS`) | `can.open` → `E_BUSY` |
| `max_pending_bursts` | `MAX_SEND_COUNT` = 100 (one entry per request, ≤ 100 frames each) | write refused before `accepted` with `E_BUSY` |
| `max_tags_per_channel` | 1,024 | oldest tag evicted (that echo has no `request_id`); counted |
| `observer_queue` | 8,192 | bus drop → `source.gap`; counted |
| `max_drain_per_pump` | 4,096 | the rest waits for the next pump, with no sleep |

- Sender endpoint queues hold 1 record and are never read (tx only). Their bus-side drop counters grow by design, so `can.status` reports only the observer's drops.
- SimHub's own limits are the defaults: 16 buses, 16 endpoints per bus (= 4 channels per bus), 16 nodes.

## Public API (`loom_can::worker`)
- `CanSimWorker::{new(), with_options(clock, PumpMode, WorkerLimits), manual(clock), pump_once(), call(&cx,&Call), write(&cx, Write), set_latch(bool)}`
- `PumpMode`, `PumpReport`, `WorkerLimits`
- Constants: `CALL_OPEN`/`CALL_CLOSE`/`CALL_NODE_LIST`/`CALL_NODE_START`/`CALL_NODE_STOP`, `BACKEND`, `DEFAULT_NODES_TOML`, `MAX_CHANNELS`, `MAX_NAME_BYTES`, `PUMP_MAX_SLEEP`, `GAP_CAUSE_OBSERVER`

## Tests: `crates/loom-can/tests/worker.rs`, 17 passed
All use FakeWorker + ManualClock (the two clocks are stepped together), except `pump_thread_delivers_replies`.
- `kind_and_default_nodes`, `open_emits_channel_state`
- `walking_skeleton_loop` (§2.0): agent `300#01` → `can.tx` accepted/sent (rid) → tx echo (rid, origin agent, exact payload shape: dlc/len/hex) → `301#01` device on the data lane exactly 5 ms later → `0x310` at 50 ms with byte0 01 → user `300#00` → user echo + `301#00`; `assert_write_rows`, `assert_no_gap_lies`
- `structured_fd_frame_payload` (dlc 9 / len 12)
- `burst_with_gap_is_one_request`: frame_index 0..2 at 10 ms spacing, rows accepted / sent(1) / sent(3)
- `burst_without_gap_is_sent_at_once`
- `stop_latch_refuses_agent_and_script_only`: agent burst cancelled, agent/script refused, no frames on the bus, user send and user burst pass, BCM replies and `0x310` continue, release → agent passes
- `usage_errors_send_nothing`: bad hex, count 0/101, bad frame text, non-object op, dry_run, not open, device origin
- `burst_queue_is_bounded`, `request_tags_are_bounded`, `observer_overflow_is_a_source_gap`, `channels_are_bounded`
- `status_and_close` (incl. reopen → generation 2), `node_calls` (default/none/custom TOML, stop, list, errors), `two_channels_share_a_bus`, `jobs_and_input_are_refused`
- `pump_thread_delivers_replies`: real clock and the thread; it re-sends because `MemorySink` drops a push when it loses its `try_lock` race against the test's `drain`

**Checks (from the worktree root):**
- `cargo fmt --all --check` passes.
- `cargo clippy -p loom-can --all-targets -- -D warnings` passes.
- `cargo test -p loom-can` passes: 5 lib + 17 worker + 0 doc. The worker suite was run 5 times in a row with no flakes.
- Note: during my work `gateway.rs` (another worker) briefly did not compile. It compiled again before the final runs. I did not touch it.

## Deviations / decisions (defaults chosen without asking)
1. **Publish-before-reply and gap bursts.** A burst with `gap_ms > 0` returns after frame 0 so the worker is not blocked. Its later echo frames and the final `can.tx` row arrive **after** the reply. `loom_testkit::check_publish_before_reply` would flag these rows. Bursts with gap 0 and single frames comply. → The lead or CORE should choose: deferred replies in the SDK, or the contract allowing progress rows after the reply for bursts.
2. No `can.tx` row for usage errors, because `TxPayload.frame` is required. Every other refusal (STOP, not open, busy) has a `failed` row, as contract 02 §2.3 asks.
3. `WriteOutcome.detail.source_seq` is the sim **bus** seq, not the Loom producer seq of the echo event.
4. Rows carry no `risk` (the worker does not receive it in `Write`) and no `requested` row (the gateway owns it). `expect` is ignored by the worker (the gateway matches from its mirror). `Write.deadline_mono_ns` is not applied to bursts yet.
5. A frame sent by a channel on a shared bus is shown on the other channel with its real origin and `dir`, without `request_id`, as the brief says ("dir/origin from the SimRecord").
6. The observer endpoint is attached with `Origin::Device`. It never sends; the value only matters for `send`.
7. `sim_node=BCM` (plan §2.0 annotation) is not emitted: `FramePayload` has no field for it.

## Requests to the lead
- **lib.rs:** please consider moving `CALL_OPEN`/`CALL_CLOSE`/`CALL_NODE_*` (now in `worker.rs`) next to `RPC_STATUS`, so the gateway routes the same names. The gateway worker should use them.
- **schema.rs:** none required. Optional: a `sim_node` field on `FramePayload`, and a typed `ChannelStatePayload` (`{state, backend, channel, bus, generation}` is what the worker emits now).
- **Cargo.toml:** no new dependencies.
- **SDK (CORE):**
  1. a data-lane emit method that takes origin/request_id/dir (tx echoes are on the priority lane now);
  2. `Emitter::source_gap` with `detail`/range, so the bus seq range of the loss can be attached;
  3. deferred replies for long writes (item 1 above).
- **loom-can-sim:** there is no API to remove a bus. Closed channels leave their bus in the hub, so 16 *distinct* bus names over a worker's lifetime exhaust `max_buses`; reopening the same name reuses its bus.
