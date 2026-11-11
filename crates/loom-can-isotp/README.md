# loom-can-isotp (P0)

A bounded sans-IO ISO-TP endpoint for the CAN worker's single TX arbitration
thread. It does not own threads, clocks, sockets, drivers, policy or RPC.
Normal 11/29-bit identifiers and Classic CAN / CAN FD are supported.
Extended/mixed address bytes are reserved in `Addressing` and return Unsupported.
Functional/group addressing is outside P0.

## Driving an endpoint

1. Build validated `IsoTpConfig::new(tx_id, rx_id)`, then `Engine::new(config)`.
2. `send(pdu, now_mono_ns)` admits one outgoing PDU or returns Busy / a bound error.
3. Feed matching ingress to `on_frame_in_generation(frame, stamped_generation, now)`.
   The synchronous `on_frame(frame, now)` uses the current generation.
4. Drain **both** `pop_frame()` and `pop_event()` after each operation. Submit a
   FrameRequest only if is_frame_pending(handle) is true and now >= not_before_ns.
   Publish priority RxStart before processing its FC request.
5. Call `on_frame_submitted_at(handle, actual_submission_ns)` on successful driver
   submission. The short `on_frame_submitted(handle)` uses the most recently
   supplied clock instant; it must not be used with a stale clock.
6. Call `on_frame_confirmed(handle)` only for correlated backend bus TX completion.
   A successful driver write is not that evidence.
7. Wake at `poll_at()` and call `on_timer(now)`. One timer call produces at most
   one CF and one FC. At STmin0, repeatedly drain/poll at the same virtual instant.
   Pending requests removed from the engine are the caller's scheduling responsibility.

A transfer emits TxDone(DriverAccepted) once all its emitted data frames have
been submitted. It can subsequently emit TxDone(BusConfirmed), with the **same
transfer id**, once every such frame is confirmed. Consumers must treat these as
monotonic transport progress, not two distinct successful requests. UDS response
validation and write_state remain UDS-owned. Failed transfers never upgrade.

The completed-transfer ledger is capped at four records by default. Fully
confirmed records are automatically released. For a backend without TX confirms,
or after an upper-layer confirmation timeout, call `release_transfer(id)` after
DriverAccepted/failure and record its returned unconfirmed submission count.
This explicit release forfeits later confirmation upgrades. An active transfer
cannot be released. Full retained history rejects a new send before admission.
The engine does not invent a confirmation timeout or silently discard that evidence.

## Timing and receive semantics

All time inputs are monotonic nanoseconds supplied by the caller. Backward time
returns TimeWentBackwards. N_Bs starts when the FF / block-ending CF is actually
submitted; WAIT resets it until wft_max is exceeded. CF separation uses actual
reported CF submission time, including across block boundaries. N_Cr is refreshed
by valid CF and CTS submission. During delayed/pending FC it has a finite deadline
at the scheduled FC time + N_Cr. Deadline equality expires **before** incoming data
is processed. poll_at excludes driver submission/confirmation waits: the worker
must impose its own bounded enqueue/submission/confirmation deadlines.

RxStart is queued immediately on an accepted FF, before requesting FC, and is
independent of PDU completion. Malformed PCI/length, unexpected new SF/FF, wrong
SN and timeouts produce typed error events. A new valid SF/FF replaces partial
reassembly, cancelling its queued FC. Unrelated FC / idle CF / stale generations
and callbacks are ignored with fixed-size counters. Unexpected CAN mode/remote
frames produce a receive error; identifier format (11 vs 29) is part of matching.
RX CAN_DL is taken from the FF, independently of the endpoint's outgoing TX_DL.

Defaults: CC DL8, padding Some(CC), BS8, STmin0, N_Bs/N_Cr1000ms, wft_max10,
max_pdu4095. TX_DL accepts 8,12,16,20,24,32,48,64 for FD. With padding=None,
FD rounding to the next legal length still fills required tail bytes with CC.
Actual CAN_DL>8 uses the SF escape header **also for a short padded PDU**; this
avoids an invalid short-nibble SF in a 64-byte CAN FD frame. FD SF capacity is
TX_DL-2 above DL8; CC/DL8 capacity is7. 32-bit FF length encoding is not
supported: an escape FF with length above 4095 (or above max_pdu for a 12-bit FF)
gets `RxError(PduTooLarge)` **and an FC OVFLW**, so the sender stops at once instead
of waiting for N_Bs; an escape FF with a length that fits 12 bits is malformed
(`InvalidLength`, no FC). Zero PDU length is invalid. Received reserved STmin bytes
(0x80–0xF0, 0xFA–0xFF) mean 127 ms; the endpoint's own `st_min` (sent in its FC)
must be 0x00–0x7F or 0xF1–0xF9, otherwise `InvalidConfig`.

## Abort and generations

`abort(now)` clears TX/RX state, delayed CF/FC, retained confirmations and pending
output queues, reports discarded event/cancelled-frame counts and increments the
generation. It works even when queues are full. It emits Aborted errors for the
previous active directions. `residual_tx_possible` conservatively counts submitted,
unconfirmed retained data/control frames, including FC from replaced/failed RX
where completion was not proved. It is **not a count of frames certainly on wire**.
Normal peer CF progress resolves its preceding FC records; discarded old control
identities can therefore make the residual estimate conservative. A terminal FC
(OVFLW for an oversized FF, or the OVFLW fault) has no receive state and no peer
CF to resolve it, so its record is retired when it is submitted (counted in the
residual estimate). Without this, a backend without TX confirms would fill the 64
control records and refuse every later FF. Work already released by the caller is
outside this estimate and was reported on release.

The worker must recheck is_frame_pending immediately before driver enqueue (on
the same arbitration thread). It catches old generations **and** FC invalidated
by SF/FF replacement within a generation. The worker must also stamp
queued ingress **before** enqueueing it. Old handles cannot update a new transfer.
CAN frames carry no transport generation: a physically late FC/CF received *after*
a new transfer starts at the same IDs cannot be identified from bytes alone.
The worker needs an address drain/quarantine boundary for that case. This crate
provides generation fencing for already queued work and does not claim to solve
that wire ambiguity. Abort is not physical cancellation of accepted driver TX.

## C2 bounds and errors

- PDU length1..=configured max, hard maximum4095. One active TX and one active RX.
  Each reassembly Vec reserves max_pdu; incoming lengths are checked before allocation.
- Frame and event VecDeque: defaults32/16, hard maximum64 each. Limits are validated
  (minimum2 frame /4 event slots). Each event PDU is at most max_pdu.
- Every public ingress operation reserves worst-case1 frame/4 event slots before
  changing protocol state. Timer reserves2/3; callbacks reserve2 events. This can
  return OutputFull while a few slots remain. Drain outputs then retry the **same**
  input/callback; the caller must never silently drop that error. Abort is the
  explicit escape hatch and reports what it discards.
- Retained transfer records: default/hard max4. Each has at most586 data-frame
  statuses (worst case4095B, DL8). At most64 control statuses. Collections check
  limits before growth; counters saturate, timestamps use saturating addition.
- Approximate payload bound at default limits: TX/RX8KiB + event PDUs64KiB + four
  data-frame ledgers (4*586*size_of(Tracked), approximately75KiB on64bit) + frame/
  control/queue metadata. The maximum event limit raises event payload to256KiB.
  These are **per endpoint**, not the complete worker RSS. Worker admission and
  shared PDU-pool budgeting must account for endpoint count and chosen limits.

FaultPlan defaults off: one-based CF drop/wrong-SN selectors, delayed FC up to60s,
up to32 WAIT responses per block, and FC OVFLW. Faults are deterministic, use the
same supplied clock, and never sleep. A dropped CF is omitted on the simulated
link while the sender advances as if it was submitted. This is a test fault,
not a successful physical submission claim. Only the sim worker may enable faults;
upper layers are responsible for rejecting them on real hardware.

Dependency: only the sibling loom-can-frame crate. No unsafe, Python, OS timing,
async runtime or IO. Seeded randomized and boundary virtual-link tests replace
a property-test dependency for P0. The default run covers all 18 brief
configurations (CC/FD × BS 0/1/8 × STmin 0/1 ms/100 µs) at every segmentation
edge plus a seeded sample (debug < 1 s). The exhaustive 1..=4095 sweep of the same
configurations (73,710 transfers) is `#[ignore]`d. An independent external codec
oracle remains a useful later cross-check; self round trips are not a conformance
certification.

From the workspace root:

```sh
cargo fmt -p loom-can-isotp --check
cargo clippy -p loom-can-isotp --all-targets -- -D warnings
cargo test -p loom-can-isotp
cargo test -p loom-can-isotp --release -- --ignored   # exhaustive matrix, ~4 s
```
