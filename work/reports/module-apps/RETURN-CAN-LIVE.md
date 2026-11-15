# RETURN-CAN-LIVE: CAN actual-live defect fixed (shared subscription capacity)

READY_FOR_ROOT_REBUILD. Owner w-CAN-LIVE, foreground Opus 5.5, no subagents, no Git/index.

**Not touched:** browser, CUA/Ego, daemon, app, devices, backend.

**Scope used (ROOT-granted sole ownership):**
- `ui/src/net/{mux,mux.test,api,client,core,protocol}.ts`
- `ui/src/sdk/context.ts`
- `ui/src/store/testing/fakeNet.ts`
- `ui/src/shell/services.ts`
- `ui/src/panels/can/**`
- New: `ui/src/sdk/capacityLive.test.ts` and `ui/src/panels/can/fixtures/qaCanMarker.native.json`

**Not touched:** `shell/dock/*`, UDS, other module panels, `crates/*`.

## Root cause (not the CAN parser)
- **The cap.** loomd allows 8 server subscriptions per connection (`hub/mod.rs:59`), and the UI mux mirrors that with
  `maxSubs = 8`.
- **How the slots fill up.**
  - Retained docks keep each hidden panel's subscription (`dockHost.ts:7`).
  - The shell held 2 subscriptions (Activity, test).
  - Unbound Video and Logic subscribe to their whole domain.
  - A zero-channel tour therefore fills all 8 slots.
- **Why CAN never went live.**
  - The bound CAN topic was **parked** with one `onResync("subscription_cap")`. That resync is the "ring 1 (gap)" row.
  - A parked handle was re-placed only when an entry **closed**, and retained docks never close.
  - So CAN stayed parked forever, with 0 frames.
- **PSU was the same.** ROOT's reload result fits: a reload dropped the retained topics, and 301 points appeared at once.
- **Amplifier.** CAN's `activity` topic could only share the shell Activity entry while that entry's replay log was under
  256 items. After that it cost its own slot.

## Strategy implemented (bounded, no higher caps, no retry loops)
1. **One pinned shell stream** (`services.ts`).
   - `{domains:['core','test']}` with snapshot, `pinned:true`, replacing the two shell subscriptions.
   - It is dispatched to the existing stores:
     - `core.activity` / `core.agent_cursor` → ActivityStore.
     - `test` events → `TestSessionStore.applyMany`; test snapshot items → `applySnapshotState`.
     - All items still go to `noteSnapshotItems`.
   - The control path (STOP/latch/test live) is untouched.
   - Native check: `core.activity` is published as domain `core` (`daemon.rs:710`, `:2536`).
   - **Effect:** the shell uses 1 slot, so **shell + all 7 modules = 8**.
2. **CAN activity topic → `{domains:['core'], kinds:['core.activity']}`, live-only** (`canTrace.ts` manifest).
   - The filter is truly contained in the shell stream, as ROOT noted.
   - The mux now lets live-only handles join a covering stream *before* spending a slot. CAN agent-read ranges cost
     **0 server subscriptions**.
   - Trade-off: agent-read ranges from before the panel mounted are no longer replayed. They appear on the next agent read.
3. **Visibility leases** (mux `setActive`, plumbed through api, client, protocol and core).
   - The SDK context wraps `PanelContextHandle.setVisible`, which DockHost already calls for both tab and workspace
     visibility, and applies it to every topic, including topics opened lazily while hidden.
   - **Hiding does nothing at once.** There is zero churn under the cap (tested: 200 toggles → 0 calls).
   - **When a *visible* handle cannot be placed:**
     1. **Suspend** the least-recently-visible entry whose handles are all hidden (pinned entries never).
        - Its handles get `onResync("suspended_hidden")`.
        - Panel instances, drafts, splits and rings are untouched.
     2. Otherwise **fold** streams into one union subscription. In order of preference:
        - A waiting handle joins a union already being formed. This costs no server call.
        - "Absorb": one entry plus the waiting handle.
        - An entry pair, cheapest union first.
        - Exact unions come first, e.g. `uart/[u0..u5]`.
        - Same options, `ext` and origins are required; pinned entries are never folded.
        - Members are bound narrowly. The client filters events **and snapshot items** per handle (`narrowPart`).
     3. Otherwise **genuine capacity.** The handle stays parked with `subscription_cap` and is placed as soon as a view
        is hidden or closed.
   - **Handoff.** A freed slot is reused only after the daemon answers `core.unsubscribe` (existing `draining` rule).
     Order: merge groups → visible (most recent first) → hidden.
   - **Showing again.** A suspended handle is re-placed with a **fresh snapshot**.
     - `after_seq` is deliberately not used: `mirror.resync` clears reducer snapshot items, so UDS/PSU state must come back
       from a fresh snapshot.
     - CAN deduplicates the re-delivered backlog by epoch/seq (existing `resyncPending` path).
   - **Bounds.**
     - Each relief step lowers held entries by ≥ 1 net; the loop guard is `maxSubs*2`.
     - Parked handles: existing 256. Handle bookkeeping is removed on close (tested).
     - Client `bound` map: rebinding drops older eids and empty sets, so there is no leak across suspends and merges.
     - Memory per panel is unchanged (mirror and ring bounds as before).
4. **Capacity is visible.**
   - `mux.capacity()` returns maxSubs, entries, draining, handles, active, parked, suspended and shared (folded).
   - CAN shows `구독 대기 — 이 연결의 서버 구독 8개를 보이는 패널이 모두 쓰는 중 …` and badge "구독 대기"
     (`data-subscription="waiting"`) only for a real cap park, not for lease resyncs.
5. **Multi-UART.**
   - Many visible UART ports fold into one exact `uart/[…]` stream. They never park, and the other modules are not
     disturbed (tested).
   - This beats the baseline, where ≥ 7 visible ports parked.

## Tests (real native shapes; negative control done)
- **`ui/src/sdk/capacityLive.test.ts`: full-wire end to end.**
  - Setup: a fake loomd **enforcing 8/conn with E_BUSY**, the real inline NetClient + mux + SDK context, and the real
    `can.trace`.
  - It replays the **QA native records seq 1967–1977**, copied verbatim from `qa/can-observer.out`.
  - Tour: shell + 8 hidden docks, with real topic filters, including 2 UART and 2 ETH.
  - Then a visible bound CAN gets its slot. The rows show:
    - `tx:agent:0x300` with request_id `w8`
    - `rx:device:0x301` (exact copy)
    - ≥ 2 × `rx:device:0x310`
  - Also checked:
    - Activity reaches the shell and causes no CAN subscription.
    - No resubscribe churn while frames flow.
    - Hidden→shown UART gets a fresh snapshot without `after_seq`.
    - **busy = 0, maxHeld ≤ 8.**
  - A second test checks that visibility drives the lease of every topic, including ones opened lazily while hidden.
- **`mux.test.ts`: 7 new or rewritten tests.**
  - The tour fits (shell + 6 modules + CAN).
  - Over the cap, LRU suspension with the ACK handoff, then snapshot/end/live.
  - Hidden→shown fresh snapshot.
  - No churn under the cap.
  - **Integrated: shell + 7 modules visible together fit in 8**, and +5 visible UART ports fold into one exact union while
    the others are each subscribed once.
  - Genuine-capacity park, then placement on hide.
  - Cleanup when closing parked or suspended handles.
  - The old cap test is rewritten to the new semantics, because it pinned the "park forever" behaviour.
- **`canTrace.test.ts`:**
  - The cap-wait status shows, then goes live, and is not shown for `suspended_hidden`.
  - The activity topic shape is updated.
  - Unbound no-query/sub/write and explicit Connect are unchanged (`canModule.test.ts` passes).
- **Negative control:** with mux relief disabled, **6 tests fail**, including the e2e. With it restored, all pass.

## Runs (once each; logs in `work/reports/module-apps/can-live/`, copied to `qa/can-live-fix/`)
At SOURCE_FREEZE, `shasum -c qa/can-live-fix/after.sha256` → 13/13 OK against the frozen tree. Log hashes are in
`qa/can-live-fix/LOGS.sha256`.

- `npx vitest run` (full UI): **68 files, 752 tests passed, rc 0** (`ui-test.log`).
- `npx tsc -b`: rc 0 (`ui-tsc.log`).
- `npx eslint src/net src/sdk src/panels/can src/shell/services.ts src/store/testing/fakeNet.ts`: rc 0 (`ui-lint.log`).
- After the full run I made two test-only fixes: a TS props-schema literal and an eslint useless assignment. I then
  reran `mux.test.ts` and `capacityLive.test.ts` (25/25 passed) and reran tsc and eslint (rc 0). I did not rerun the full
  suite after those test-only edits.
- Desktop cargo was not run.

## Hashes
- Before: `can-live/before.sha256`. After: `can-live/after.sha256`.
- Key after-hashes:
  - `mux.ts` 8badaa16…
  - `context.ts` ff6356de…
  - `services.ts` d1a4ac68…
  - `canTrace.ts` b04cacda…
  - `capacityLive.test.ts` a55b091c…
- `can-live/diff.patch` is the working-tree diff of the touched tracked files. It also contains earlier dirty hunks that
  are not mine:
  - `context.ts` `WriteHandle.result`
  - `services.ts` `dockEpoch`
  - `canTrace.ts` / `can.css` from the former C
  - I did not touch `can.css` at all.

## Diff review / known limits (for ROOT)
- **Lazy subscribe while hidden.** The `sub` message is processed as active before the following `active:false`, so it
  can suspend one older hidden entry. This is bounded (one step) and harmless. A future refinement is to pass the
  initial active flag in `sub`.
- **Lease resyncs leave a row.** `suspended_hidden` and `merged` resyncs add one honest resync gap row in panels that show
  gaps, such as CAN and UART. A folded UART port gets one `merged` mark.
- **Shell snapshot semantics.**
  - The shell snapshot filter changed from `{kinds:[core.activity]}` to `{domains:[core,test]}`.
  - Channel discovery still comes from the always-included `core.channels` item (`hub/mod.rs:1302`).
  - Domain reducer items no longer reach the shell's own snapshot. Panels still get theirs.
  - Please confirm in the actual flow that the channel bar and late join are unchanged.
- **UDS stale-replay.** w35's diagnosis is not coupled here. Covering joins at the cap keep their old behaviour.
  Merged unions always take a fresh snapshot, so they avoid the stale-replay class.
- **Backend observation (not changed, likely):** `loom-can/src/gateway.rs:372` publishes `core.activity` through
  `ctx.publish`, and `daemon.rs:2560` refuses non-`can.` kinds. CAN-domain activity drafts are therefore probably
  dropped as best-effort. Gateway `ctx.action()` activity (domain `core`) is unaffected.
- **Not mine; recommendation to the owners:** unbound Video/Logic should not subscribe to a whole domain. They still
  work, now as suspendable hidden leases.

ROOT next: rebuild the matching app, then run the actual flow:
- Zero-channel tour, then `#/s/can` bound with QA CAN marker → 0x301/0x310 live.
- PSU live chart points.
- Integrated with 7 modules.
- Multi-UART.
