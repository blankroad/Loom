# REVIEW-FIX: independent read-only review of the final follow-up fixes (CAN-LIVE mux leases, UDS-STATE hydration)

Reviewer: fresh foreground Opus 5.5. No subagents, no production edits, no GUI/device/daemon, no Git/index. The only file
written is this one.
Scope: the follow-up source in `can-live/diff.patch` (11 files) plus `capacityLive.test.ts`, and the UDS-STATE files.
Nothing else from the historical dirty diff.

## Verdict: **CHANGES_REQUESTED** (1 Medium, 3 Low; none blocks the CAN-live/UDS root-cause fixes themselves)

The core design is sound and the stated invariants hold, with the exceptions listed below:
- The cap is enforced at placement.
- The ACK is drained before a slot is reused.
- The shell is folded to one pinned stream.
- CAN agent-read rides on the shell stream.
- Hidden panels are suspended least-recently-visible first, with a fresh re-snapshot.
- The UDS seq guards are correct.

The Medium is a small, local fix (one predicate in `client.ts`). Runtime/native PASS is **not** claimed here. ROOT's
actual Ego run and the pending Mac unlock are separate.

## Source fingerprints reviewed (sha256, all re-verified against the current tree)

The CAN-LIVE files match `can-live/after.sha256` exactly (`shasum -c`: all 13 OK):
```
8badaa16e42e82614a3e203195e3665cfd95a8567f74ec05710894b7221f6aae  ui/src/net/mux.ts
ffca2fa256eaf6868bf15e2d102292828ac2609dea5658917139f9951f6000f9  ui/src/net/mux.test.ts
0f6740717e368391a7e025995296ed345be91bffece2e19da2e66de94a0089c7  ui/src/net/api.ts
f4fa31bca3e2da24fe657a27eb21ca4620bffa67200639377ea34f0cf3f1ed25  ui/src/net/client.ts
86803c70afb010872c3f37f9940db7b4d81fe0f7ccf1cae130a76f08d8a886b9  ui/src/net/core.ts
53df80577a821821d2e79fe91a956ec24110120573086c3ba0dafe41f151be3d  ui/src/net/protocol.ts
427d62f2d14b806647c210cdaa1d33ce79103ef65d3b8cc9be0617e9c6e391ff  ui/src/store/testing/fakeNet.ts
ff6356de8b05a1326f43ba069059a02630ff3e1ba0bad6cb7353636ea9e11801  ui/src/sdk/context.ts
a55b091c3ba1e15dc6e759f5bc0cab14ac0a3ce0f8bf329d5d6ae499f5559702  ui/src/sdk/capacityLive.test.ts
d1a4ac68619ad99da471d0180efbf5aeebc9ac14f6498c6f27df2774b7c9c74a  ui/src/shell/services.ts
b04cacda091f642a190d04174d9d186668eb41f1d887504ee4d4dfb48764f904  ui/src/panels/can/canTrace.ts
3850628394a3f777e1c9454ce9a106bd9d9796ed0cf35e5037e8e73a4bc57668  ui/src/panels/can/canTrace.test.ts
768de00fc9eaf33f729f88a4dec117570e0ebcd8db748d9c10451fa0e8dcdd9c  ui/src/panels/can/fixtures/qaCanMarker.native.json
```
The UDS-STATE files match RETURN-UDS-STATE.md:
```
861d6f4a2605d5595f52ff95f26cbbe32f5cee9a01c6993a052db20f9ce59443  ui/src/panels/uds/consolePanel.ts
b462364cab36a58304896f9768310161db79ebb9f9eaed2f35b4c66630c2b51e  ui/src/panels/uds/uds.test.ts
c269f93acfc7091aade845a02a0f325f9c601020426f545bd3ab3b926b00e14b  ui/src/panels/uds/udsStateHydrate.test.ts
af7287329cb32c118e387c6db122098201268f8347724a7895d02f43394daa68  ui/src/panels/uds/udsModel.ts
```

I ran one focused test pass to confirm independently (no full suite):
`vitest run src/net/mux.test.ts src/sdk/capacityLive.test.ts src/panels/uds/udsStateHydrate.test.ts src/panels/can/canTrace.test.ts`
→ 4 files, **50 passed**, rc 0. The owner logs in `can-live/` show 68 files / 752 tests, tsc rc 0 and eslint rc 0 for the
same hashes.

## Findings

### F1 · Medium · Merged and narrow streams deliver other channels' and domains' `*.gap` events to every handle (false loss markers, including in REC)
- **Where:** `ui/src/net/client.ts:247`:
  `evs = item.batch.events.filter((e) => matchesFilter(filter, e) || e.kind.endsWith('.gap'))`.
  Since `mux.ts:547`, it applies to every merge-group member: `narrow = filterKey(own) !== filterKey(entry)`.
- **Why it is new in scope:** before this fix, narrow binding happened only on a rare covering join at the cap. Now
  `mergeOne` (`mux.ts:299`) is the standard relief path, and the test "many visible UART ports share one exact union"
  makes it a supported flow. Per contract 02 / UI C-4, `source.gap` and `record.gap` carry **the affected stream's
  domain·channel** (for example `replay/scenario.rs:255-266`, `"can"` driver_overrun). The CAN, ETH and PSU workers
  emit them.
- **Scenario:** in Integrated, all 7 modules and a second CAN channel are visible, so 9 streams are wanted.
  1. `mergeOne` absorbs `can/[a]` + `can/[b]` into the exact union `can/[a,b]`.
  2. A `source.gap` (driver_overrun) arrives on `can/a`.
  3. CAN panel `b` receives it through the `.gap` bypass. `canTrace.ts:328-332` adds a gap to its summary and trace.
- **The same happens elsewhere:**
  - `eth/[lan1,lan2]`: a lan1 capture drop is marked in lan2's packet list.
  - A cross-domain union such as `{domains:[can,psu]}` (same options: snapshot, no maxBatchMs): a CAN overrun breaks
    every PSU chart lane and sets `staleAfterGap` (`gaugePanel.ts:359-364`).
  - UART terminals turn a mirror gap into `term.markGap`, and with REC active into
    `rec.note("데몬 보고 누락…")` (`terminalPanel.ts:756-761`), so a foreign gap would be written into the user's
    recording evidence.
- **Fix (client.ts only):** pass a gap event through when it matches the handle's **domains/channels**, ignoring the
  kinds dimension. Pass it through to all handles only when it carries no domain or channel of the affected stream
  (`display.gap` for the subscription, or a domain-wide gap). For example:
  `matchesFilter(filter, e) || (e.kind.endsWith('.gap') && matchesFilter({ domains: filter.domains, channels: e.channel == null ? undefined : filter.channels }, e))`.
  Batch-level `display_gap` (the subscription's own loss) correctly stays shared.
- **Test:** a merged `can/[a,b]` with a `source.gap` on `a` must reach handle a and must not reach handle b. A
  negative control must fail on the current line 247.

### F2 · Low · CAN agent-read marks lost for reads made before the panel mounted or after a reload (agent-monitoring regression)
- **Where:** `canTrace.ts:800-801`. The activity topic changed from `{kinds:[core.activity]}, snapshot:'required'` to
  `{domains:[core], kinds:[core.activity]}, snapshot:'none'`.
- **Before:** the topic shared the shell entry with its replay (the activity backlog), or opened its own entry with a
  fresh snapshot once the replay overflowed. Either way `traceModel.applyActivity` (`traceModel.ts:642-659`) received
  ranges read **before** the mount.
- **Now:** a live-only narrow join gets no replay (`mux.ts join`: replay only when `opts.snapshot`).
- **Scenario:**
  1. The agent runs `can.trace.query` or a read that publishes `core.activity{range}`.
  2. Then the user binds or opens the CAN panel, or reloads the browser.
  3. The frames appear from the CAN snapshot, but no agent-read highlight appears for them.
  4. Only reads made after the mount, or after the next re-snapshot of the shell stream, are marked.
- Shell Activity itself is unaffected.
- **Fix options:**
  - Seed `applyActivity` once from the shell `ActivityStore` (already holds the ring) when the panel binds.
  - Or make the topic `snapshot:'required'` on the same pinned `SHELL_STREAM_FILTER` key. That costs no slot, but the
    replay is lost once the 256-item log overflows. The ActivityStore seed is more robust.
- **Test:** an activity range published before the mount is marked after the mount.

### F3 · Low · `onOpen()` never re-places parked handles: a visible handle can stay parked silently after a reconnect
- **Where:** `mux.ts:421-431` (`onOpen` only restarts entries) and `mux.ts:694-699`. The catch of a closed in-flight
  subscribe re-places only `if (e.closed && this.connected)`.
- **Scenario:**
  1. After a reconnect, every entry is `subscribing`.
  2. The user shows CAN. `park → relieve → suspendOne` closes a hidden entry that is still subscribing
     (`releaseLater(null)`, draining+1).
  3. `relieve` counts that slot as free, so **no `subscription_cap` resync** is emitted. CAN shows "live" in
     `subWaitOf`.
  4. The socket drops before the subscribe answer. `failAll` runs, then `onDisconnect` resets `draining=0`, and the
     rejected subscribe's catch sees `connected=false`, so it does not call `placeParked`.
  5. `onOpen` restarts the entries but never calls `placeParked`.
  6. CAN stays parked with no data and no wait indication until some unrelated sub/unsub/`setActive` happens.
- The unsubscribe variant self-heals, because its `done` calls `placeParked`. Only the in-flight subscribe variant
  sticks.
- **Fix:** call `this.placeParked()` at the end of `onOpen()`. It is idempotent and bounded.
- **Test:** a disconnect while a suspended entry's subscribe is in flight; then `onOpen`; then the waiting active
  handle is subscribed.

### F4 · Low · A topic opened lazily while hidden relieves pressure as if it were visible (can merge or resnapshot visible panels)
- **Where:** `context.ts:299`. `subscribe` is posted first, then `setActive(false)` as a second message.
  `mux.subscribe` (`mux.ts:200`) starts the handle `active: true`, and at the cap `park` (`mux.ts:240`) runs `relieve()`
  synchronously, before the `active:false` message is processed.
- **Scenario:**
  1. The visible Integrated workspace already holds 8 entries, each with an active handle.
  2. A retained hidden panel opens a topic lazily.
  3. `suspendOne` finds nothing, so `mergeOne` folds two **visible** entries (both get `onResync("merged")` and a
     re-snapshot) for a panel nobody is looking at.
  4. The merge group then gets placed with rank 0 even though its trigger is hidden.
- With hidden-only entries present, the result is a needless swap between two hidden panels instead.
- **Fix:** carry the initial lease with the `sub` message, for example a non-key `SubscribeOptions.active?: false`
  read in `mux.subscribe`. Or post `active:false` before `relieve` can run.
- **Test:** at the cap, a hidden lazy open causes 0 merges and 0 visible resyncs.

## Invariants checked (PASS unless noted)
- **Cap 8 per connection, no E_BUSY, no inflation: PASS.**
  - `place()` still gates on `entries + draining < maxSubs`.
  - `relieve()` counts draining slots as freed but only *places* after the ACK arrives (`releaseLater` → `placeParked`).
  - `suspendOne`/`mergeOne` reduce `entries` by ≥1 each; the guard is 2×maxSubs.
  - The parked list stays bounded (256).
  - `handles` is deleted on unsubscribe and on park overflow.
  - The tests use a cap-enforcing server (`busy()==0`, `maxHeld≤8`).
  - 32 global is untouched; there is no extra connection.
- **Seven modules plus shell in 8: PASS.**
  - Real manifests: shell `{core,test}` pinned, UART, ETH(+ext none), UDS, CAN, PSU, Logic and Video each take 1.
  - CAN activity takes a covering live-only join on the shell stream.
  - Different `maxBatchMs` (Video 100, Logic 50, ETH 40) correctly make those unmergeable with the others.
- **Exact filter dispatch after merge:**
  - **PASS for events and snapshot items.** `narrowPart` keeps channel-less (domain-wide) items, and
    `ext`/`origins` must be equal to merge.
  - **The `.gap` bypass is not exact (F1).**
- **Unsubscribe/drain ACK before placement: PASS.** `closeEntry` keeps the in-flight/unsubscribe accounting identical to
  the old unsubscribe path.
- **Disconnect/epoch/resync:**
  - Resume, epoch change and `resume_failed` are unchanged.
  - The client's `bind` drops stale eid bindings, and eids are never reused.
  - **F3** is the gap.
- **Hidden→visible catch-up: PASS.**
  - A suspended handle is re-placed with a fresh snapshot (no `after_seq`). The mirror resync marks the gap honestly.
  - Showing and hiding below the cap makes 0 calls (tested).
- **UDS hydration races (RETURN-UDS-STATE): PASS.**
  - The guards hold: `askedSeq` per target, `askedVersion`, and stamp = max(throughSeq, headSeq, applied).
  - A live control link-down beats a delayed reply.
  - A later fresh snapshot replaces the hydrated state.
  - The bound panel calls `uds.status` only once; the unbound panel never does.
  - Interaction with leases: suspend and merge always re-subscribe with a fresh server snapshot. The panel keeps
    `targetSeq` across a resync (no resync handler resets it), so control states seen while it was suspended survive an
    older replay.
  - Residual risk (Info): if a hydration reply is in flight across a lease resync, it is dropped and never re-asked.
    That is safe because the new snapshot is fresh, except when the re-place exact-joins another handle's older
    replay. That is a rare edge and the panel stays honest ("unreported", not wrong).
- **Retained drafts/splits/REC/macros: PASS (by code).**
  - The lease only moves the server subscription. The instance, mirror and handlers stay as they are, and nothing in
    the dock is torn down.
  - Info: a hidden UART terminal with an active client-side REC can be suspended under pressure. The resync gap is
    noted honestly in REC, and bytes inside the daemon backlog are recovered via `mirror.after(lastSeq)`. Consider
    keeping a REC-active panel's lease active if lossless background recording is a requirement.
- **Shell Activity/test/STOP monitoring: PASS.**
  - STOP and latch are on the control path and untouched.
  - Activity and test are now one pinned stream that is never suspended or merged.
  - `core.activity` is domain `core` (`daemon.rs:707-710`, `:2534-2537`).
  - `core.agent_cursor` application is monotonic, so double delivery is harmless.
- **No auto-connect or send from navigation: PASS.**
  - An unbound CAN calls no `ctx.store`/`rows`.
  - `canConnect.ts` acts only on click (`:161`, `:190`).
  - Every send path is guarded by `!bound`.
  - The manifest's `can.open` action is explicit.
- **Many-UART: PASS** (exact union, other modules never resynced; tested), subject to F1 for gap attribution.

## Test-shape notes
- `capacityLive.test.ts` uses a cap-enforcing fake daemon and the real CAN panel with a native-shaped fixture. That is a
  good shape.
- `mux.test` lease tests check the server-side cap, but they lack negative controls for F1, F3 and F4.
- The "Integrated" test uses bound Video/Logic filters with channels, while the tour test uses unbound filters. Both
  shapes exist in the real manifests, so this is fine.

## Requested before acceptance
1. Fix F1 (client.ts) and add a test with a negative control.
2. Fix F3 (one line in `onOpen`) and add a test.
3. F2 and F4: fix them now, or ROOT records them as accepted known issues.

After the fix, re-verify the hashes of `client.ts`, `mux.ts` and `mux.test.ts` only, plus the scoped UI gates. No full
re-review is needed.

---

# Delta re-review: F1–F4 corrections (FINAL SOURCE_FREEZE)

## Delta verdict: **PASS** (F1 Medium and F2/F3/F4 Low are all fixed; no new findings above Info)

Method:
- I rebuilt the previously reviewed versions in scratch from HEAD plus `can-live/diff.patch`. Their hashes equal the
  REVIEW-FIX hashes above.
- I diffed them against the current tree, so the review covers only the corrections. No full review and no full suite.
- No production edits, Git, GUI or device operations.

## Fingerprints
All 11 `review-corrections/after.sha256` entries are OK against the current tree (`shasum -c`):
```
0bb7f6e02d797737dd46a6137ce0a26559306c713fd00dc995b92d7d34d30c80  ui/src/net/api.ts
0d60df4d7408bdbcb51760c9a5ccc0ea59fe5331dcfbba69c6e9f931b24d6e40  ui/src/net/client.ts
d9a276c4e1d11d995d10671fa92c6d304f5b959e5e2274caf1d8e17b060ada34  ui/src/net/client.test.ts (new)
f76fb015dc3555d402ddda087705d44b54c2a04d1493bee6493a98986bc6e75a  ui/src/net/mux.ts
f5d6e62adaf009c7ae499afdc16997226eae91ff6fb9f8dc6777b84c7c9e8578  ui/src/net/mux.test.ts
897dd8b5204633d5b97b8bb9fb692c37a94395984253920e3e13cfa36da26c72  ui/src/sdk/context.ts
acce9f1037eb30043f37069b1f37606186fb6fbf7959dba59f4cad3e02a74c62  ui/src/sdk/capacityLive.test.ts
db5f983154346d8ded8b4c88e14d17315e392f2ece6c56d6065976e71c2e4df7  ui/src/sdk/testing/harness.ts
b3656cbf855e1770fc182efc365aebda9f6a9f03f0dc224ecdb2669f64a1afda  ui/src/panels/can/canTrace.ts
857cf277fcf28b86ec44a88b41d4994b770046cf46a115f0f36342c2094cc97b  ui/src/panels/can/canTrace.test.ts
74a51f4fbbafb378486386a31abc6412299ba89e485f934439eca1c0f2a02388  ui/src/panels/can/traceModel.ts
```
- Unchanged since REVIEW-FIX: `core.ts`, `protocol.ts`, `fakeNet.ts`, `services.ts`, the CAN fixture and all UDS files.
- Owner logs: 69 files / 761 tests, tsc rc 0, eslint rc 0.
- My single focused run (`client.test`, `mux.test`, `capacityLive.test`, `canTrace.test`): 4 files, 53 passed.
- Negative-control logs `f1`, `f2`, `f3`, `f4` and `f4-context` each show the new tests failing against the old code.

## Per finding
- **F1: fixed.** `client.ts` `gapConcerns()`.
  - A `*.gap` event reaches a narrow handle only when the event's domain is in the handle's domains, and its channel is
    null or in the handle's channels. Kinds and origins are ignored on purpose: a `can.frame`-only topic still sees its
    own CAN overrun.
  - Gaps in the streamless domains `core`, `display`, `record` and `source` stay shared. I checked them against the
    daemon:
    - `hub/mod.rs:1337` `core_source(clock, domain, …)`.
    - The hub's subscription `display.gap` uses domain `"display"` (`hub/mod.rs:1070-1076`).
    - The pool's `record.gap` uses domain `"record"` (`pool.rs:112`).
    - The scenario/worker gaps name the affected stream, such as `"can"`.
  - Batch-level `display_gap` stays shared.
  - Exact (non-narrow) handles are unchanged.
  - The tests cover the merged `can/[a,b]` case, the cross-domain `{can,psu}` case, the own-gap-despite-kinds case, the
    shared `display_gap` and the exact handle.
- **F3: fixed.** `mux.onOpen()` now ends with `placeParked()`.
  - Placement is still gated by `entries + draining < maxSubs`, and `draining` is reset on disconnect.
  - Any `relieve` here suspends or merges only through the same ACK-gated path, so there is no E_BUSY risk.
  - The test reproduces the in-flight-subscribe disconnect.
- **F4: fixed.**
  - `SubscribeOptions.active?` is destructured out in `mux.subscribe`, so it never enters `HandleInfo.opts`, the entry
    options, `subscriptionKey` or `subscribeParams`.
  - Pinned handles are forced active.
  - The context now sends `active:false` inside the single `sub` message and has dropped the follow-up `setActive`.
  - The mux, client and end-to-end `capacityLive` tests show 0 merges and 0 visible resyncs at the cap.
  - Info: a hidden lazy open at the cap now gets an honest `subscription_cap` resync and stays parked until shown.
    Showing it runs the normal relieve, so the CAN status may show "구독 대기" for about one round trip. That is
    acceptable.
- **F2: fixed.**
  - `ctx.agentReads(limit, channel)` reads only the shell's bounded `ActivityStore.recent()` ring. It makes no
    subscription and no call.
  - Its domain test uses `ActivityEntry.domain`, which `readActivity` derives the same way as
    `traceModel.applyActivity`: payload.domain, then the method prefix, then ev.domain.
  - It accepts the same range keys: from/to and seq_a/seq_b.
  - The real wiring passes `s.activity` (`dockHost.ts:122`).
  - `canTrace` seeds only when bound, once, through `traceModel.addAgentRange` (`applyActivity` now delegates to it, so
    the cap of AGENT_RANGES is shared). It deduplicates against the live topic by raising `lastActSeq`.
  - An unbound panel reads nothing (tested).
  - Reload case: if the panel binds before the shell snapshot has arrived, the live-only activity handle on the shell
    stream still receives the shell's later snapshot parts. If it binds after, the seed covers it.

## Remaining (Info only, not blocking)
- The earlier Info notes still stand: a hidden UART REC can be suspended under pressure (the gap is noted honestly),
  and the rare UDS hydration-vs-lease-resync edge.
- Runtime/native PASS is not claimed. ROOT's actual Ego run and the Mac unlock remain separate.
