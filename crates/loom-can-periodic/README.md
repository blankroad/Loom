# loom-can-periodic

CAN worker TX arbitration core, driven by caller-supplied monotonic nanoseconds.
No IO, thread, OS clock, runtime or signal/DBC engine; only loom-can-frame is a
dependency. All collections are bounded. The worker owns policy, approvals,
reserved IDs, rate limits, journaling and actual device submission.

## Driving

1. `Scheduler::new()`, then `start_at(JobSpec, now)` (or `start(spec)` at the last
   supplied time). First deadline = **start + period + offset**. Default max lifetime
   is one hour; lease is optional, with its real-hardware policy supplied above.
2. Wake at `poll_at()`, call `poll(now)` / `on_timer(now)`, and drain `pop_due()` and
   `pop_event()`. poll returns the number of newly queued frames.
3. Immediately before driver enqueue, on the same arbitration thread, check
   `is_due_current(&due, now)` and the worker's final policy/latch/device gate.
4. After actual driver acceptance, use **`on_dispatch_submitted(due.dispatch, actual_ns)`**.
   Failure uses `on_dispatch_failed`. These do not assert bus confirmation.
   The shorter `on_submitted(job, actual_ns)` / `on_send_failed(job, actual_ns)`
   are conveniences only for serialized integrations with no stale callbacks.
   A JobId alone cannot distinguish a cancelled dispatch from a new dispatch
   after pause/resume. Driver submission callbacks must be processed on the
   single TX arbitration thread in monotonic time order.

There is at most one outstanding dispatch per job. `Due` captures immutable
bytes/revision/owner/origin, so an admitted patch does not change an already
queued/removed frame. A bounded global sequence prevents old dispatch callbacks
from completing new work. Stop, latch, takeover and pause invalidate removed
Due values as well as deleting queued ones; the worker must still check them.

The min-heap has exactly one wake node per job (tick or lifecycle expiry).
Rebuilding it removes obsolete nodes rather than accumulating lazy tombstones.
poll precomputes a bounded action list and checks all output space before
changing anything. A poll emits at most one frame per job. If tick10ms is polled
at45ms, it emits scheduled=10ms/lateness=35ms, counts three skipped ticks and
leaves next=50ms. It does not emit a synthetic frame for every missed tick.
If enqueue acceptance is delayed, intervening ticks are also skipped before
the next dispatch; acknowledgement cannot cause an immediate catch-up burst.

## Mutations and lifecycle

- `patch(job, expected_revision, Patch::masked/replace(...))` checks CAS and
  validates the complete resulting CanFrame before admission. Multiple accepted
  patches compose atomically and apply on the next **actual** dispatch, not on a
  skipped tick. Admitted and effective revisions are separately visible in `job()`.
  ID/CC-vs-FD/flags stay unchanged; replacement FD length must already be legal.
  RTR payload patches are rejected. No hidden FD padding or signal encoding.
- `count` counts successful driver submissions. Failures and skipped ticks do
  not consume it, and failures are retried only at a later grid tick. All jobs,
  including no-lease jobs, still have a finite maximum lifetime.
- `renew(job, now)` extends to now+the original lease length, never beyond the
  immutable start-based lifetime cap. It cannot revive an expired job. At exact
  expiry, lease/max-duration stops take precedence over a tick. A lease equal to
  max lifetime is reported as MaxDuration when both expire together.
- `latch(true)` stops **Agent, Script and User periodic jobs** and rejects all
  periodic starts while latched. User one-shot writes are outside this crate.
  `latch(false)` recreates/resumes nothing. `_at` variants accept an explicit clock.
- Producer key is (channel, CanId including 11/29-bit format). `takeover` emits
  old-owner Stopped(Takeover), gives the new owner a fresh JobId, preserves the
  effective payload/absolute grid/remaining count/lease and cumulative cap, and
  discards old-owner unapplied patches. New identity starts revision/stats at0.
- `pause_channel(ErrorPassive/BusOff)` retains Agent/Script jobs and emits
  `Stopped {resumable:true}`. User jobs remain subject to the worker/device gate.
  The paused channel state also applies to subsequently admitted Agent/Script
  jobs. Only explicit `resume_channel` resumes, at the next grid point strictly
  after resume; expired jobs are deleted. Paused ticks are counted separately.
  Paused time counts toward lease and maximum lifetime.
- If a caller wrongly reports successful submission after expiry, the scheduler
  records that driver fact, increments `expired_submissions`, then stops the job.
  A pre-enqueue `is_due_current` check would have rejected it. Physical cancellation
  of accepted vendor TX is outside this scheduler.

## C2, pressure and metrics

Hard caps: jobs128, pending patches256, due outputs128, event outputs256 and
paused-channel entries128. Channel/owner strings are bounded to64/128 UTF-8 bytes.
Patch storage is a fixed-size value, masks/payloads at most64bytes. Each job uses
a fixed128-bin lateness histogram; stopped jobs are removed, with a compact final
snapshot in the bounded event queue. No ever-growing statistics or request map.
The jobs/patches/outputs/heap and temporary action/ID lists are all bounded by
these caps. Queue/counter identifiers do not wrap; counters saturate and admission
rejects identifier/revision exhaustion. Timestamp multiplication uses u128 and
admission rejects unrepresentable lifetimes/first deadlines.

OutputFull leaves the operation's clock/state/counters/revisions unchanged.
Drain and retry the same operation. For a submission callback error, retry only
the callback, never submit the physical frame again. All matching jobs are considered atomically;
lowered custom due/event capacities must accommodate the chosen simultaneous
batch or the caller must reduce it. Default empty queues can accommodate all128
jobs in a single poll/STOP. The worker must **set its independent final TX latch
first**, then call the scheduler latch; if event output is full, drain/retry while
keeping that physical gate latched. This respects atomic overflow semantics
without pretending that a bounded event queue can always accept128 more events.

Histogram p50/p99 are upper-bound approximations: zero has its own bin, the next
64 bins are125µs wide up to8ms, then power-of-two bounds extend to u64::MAX.
Maximum lateness is exact. Lateness is actual submission minus scheduled grid
time. Only accepted submissions enter the histogram; failures/misses/cancellations
are separate saturating counters. Snapshot queries clone only bounded strings.

Integration example: start/patch/renew/control operations use the latest supplied
clock; callers needing a fresh epoch use the explicit `_at` control methods.
This core does not classify signal ranges, implement shared worker pool admission,
or guarantee OS/wire timing. A raw memory payload estimate belongs in the report;
allocator, worker/vendor buffers and OS scheduling still require separate testing.

```sh
cargo fmt --all --check
cargo clippy -p loom-can-periodic --all-targets -- -D warnings
cargo test -p loom-can-periodic
cargo test -p loom-can-periodic --test scheduler realtime_10ms_500_submissions -- --ignored --nocapture
```

The optional ignored smoke uses one OS thread, std sleep and500 successful host
submission hooks, approximately5s with a9s wall limit. It prints exact nearest-rank
raw lateness/interval-error percentiles and the approximate histogram quantiles.
It has no driver/wire/FC measurements, spin tail, priority boost or CPU-cost probe.
