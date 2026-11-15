# RETURN-UDS-STATE — bound UDS console mounted after connect shows "disconnected / link unreported"

Worker: foreground Opus 5.5, UDS source owner. No Git/index, no subagents, no browser, no daemon/device calls (not even read-only CLI; the diagnosis came entirely from source). Only `ui/src/panels/uds/**` edited.

## Verdict
**Fixed in UDS scope (bound panels only).** Root cause is in **shared SDK/shell code** (not in the UDS parser). It is diagnosed below with a concrete proposed patch for ROOT to route to the SDK owner (w36). I made no shared edits.

## Native snapshot shape (verified from source; the parser was already correct)
- `crates/loom-uds/src/module.rs:350` `StateReducer::snapshot` → `{"kind":"uds.state","payload":<StatePayload>}` for each target (last state per target).
- `crates/loom-gateway/src/hub/mod.rs:1303-1311` wraps it as `SnapshotItem{domain:"uds", channel:state["channel"] (absent → none), kind:"uds.snapshot", state}`, plus `h = next_seq-1` and backlog.
- `ui/src/sdk/storeMirror.ts:79-108` keeps the items unchanged and exposes `{items, throughSeq:h, version}`.
- `consolePanel.ts` `draw()` reads `item.kind=='uds.snapshot' && item.state.kind=='uds.state'` → `reduceTargetState(state.payload)`. **This matches.** The `StatePayload` fields (target/backend/addr/link{state,cause}/session/security/p2_ms/p2_star_mstester_present/lease) also match `reduceTargetState`. The existing unit mocks were not obsolete.

## Root cause: a stale shared snapshot plus a `uds.state` that only travels on the control path
1. `ui/src/shell/dock/DockLayout.tsx:106` → `host.reconcile(bound…)` **opens** the bound `uds.console{targets:'qa-uds'}` first. Only then does `:114-121` (hand-off) **remove** the seeded unbound `uds.console{}`.
2. Both panels declare the same topic filter `{domains:['uds']}` with `snapshot:'required'`, so they get the same `subscriptionKey`. While the seed's entry is still open, `ui/src/net/mux.ts:299-303 place()` finds it `joinable` and **joins** it. `join()` (`:329-335`) replays the seed's logged items: its **original snapshot, taken before `uds.connect`**, which has no `uds.snapshot` item for qa-uds, plus later data batches.
3. `uds.state` is a priority kind, so it reaches data subscriptions *only* through a snapshot or backlog. The live one (seq 323) went out through `net.onControl` **before the bound panel existed** (it is the very event that caused the channel to appear and the bound slot to open).
4. As a result the bound panel never sees qa-uds state. Rows from the later F190 request create an empty target, so the header goes "연결 안 됨" → "링크 미보고", the route shows `—`/"프리셋 없음", and P2 shows `?`. The defect is **persistent** until the next `uds.state` (no reconnect or new session happens), which matches ROOT's "persistent for minutes".
5. A related path: when the per-connection subscription cap is hit, `place()` falls back (`:316-324`) to joining any *covering* entry with `filterCovers`. That gives the same stale-replay hazard. This is relevant to w36's 8-subs/conn issue.

The seed→bound hand-off is the guaranteed trigger. Any later joiner of an older snapshot entry for any domain whose `*_state` is priority-only (PSU, CAN, …) has the same class of bug.

## Fix (UDS only): one-shot `uds.status` hydration for bound panels
`ui/src/panels/uds/consolePanel.ts`:
- `hydrateFromStatus()` runs after the first native snapshot has been applied in `draw()`. It is **bound panels only** (`targets` prop) and **once per panel**: no polling, never for the unbound application, never a connect.
- It calls `ctx.query('uds.status', {})`, a read-only RPC (`RoleSet::READ`, `crates/loom-uds/src/handlers.rs:139`). The reply is the same native `StatePayload` list (`{targets:[…], others_since_last}` inside a `loom_proto::Envelope`, unwrapped by `unwrapDomainResult`). It applies only targets in scope, through the same `reduceTargetState`.
- Race guards:
  - At ask time it captures the per-target `targetSeq` and the snapshot version. On reply, a target whose `targetSeq` changed while in flight (live link-down via control, or a newer state) is **dropped**. If the snapshot version changed, the whole reply is dropped.
  - Applied states are stamped with `max(snapshot.throughSeq, mirror.headSeq, applied targetSeqs)` *as of the ask*. That is a valid lower bound of the reply's as-of point. So a later live `uds.state` (seq > stamp) still wins. A later fresh native snapshot (h ≥ stamp) still replaces it, and still drops a target it no longer lists. Older backlog/control rows (seq ≤ stamp) cannot override it.
  - If the call fails or the reply is empty, nothing is applied and the link stays unreported. Connection is never inferred from the requested target name or from a successful response.
- Manifest `queries: ['uds.request', 'uds.status']`. `uds.request` stays dry-run-only, as before.
- Unchanged: the per-target seq guard, delayed-snapshot-vs-newer-live logic, unbound no-auto-connect/send guards, editable presets, raw, history, route-at-first-sight provenance.

### Limitation compared with a shared SDK fix
- It fixes UDS only. PSU/CAN/ETH/… bound panels that join a stale entry stay affected.
- It costs one extra read-only RPC per bound UDS panel mount. Side effect: `uds.status` advances **this UI connection's own** `others_since_last` cursor (`manager.rs:187-207`, keyed by the caller's client id). Agents' cursors are untouched, but if the UI ever shows that counter, its baseline moves. **API question:** is that acceptable, or does the daemon want a pure-read status variant (or a `core.snapshot`-style query for one domain's reducer)?
- The stamp is a lower bound. In theory a `uds.state` published *before* the daemon computed the reply, but delivered *after* the reply and with seq above the stamp, could repaint an older state. That needs the daemon to reorder a push behind a later RPC reply on the same socket, which is not expected. A shared fix has no such window.
- The data mirror itself is still stale (backlog rows the seed had not seen are not refetched). Request rows come from the shared RowStore, so the history table is not affected.

## Proposed shared patch (NOT applied; for ROOT → w36, SDK mux/shell dock owner)
Option A, preferred, smallest and general, in `ui/src/net/mux.ts` (`place()`): a snapshot handle must not reuse a replay that is older than control-path priority events matching the entry's filter. For example:
```ts
// Entry: lastControlSeq set by core when a control event matching e.filter passes
const joinable = (e: Entry) => !e.closed && e.phase !== 'failed'
  && (!opts.snapshot || (e.opts.snapshot && e.replay !== null && !e.controlSinceSnapshot));
```
`core.ts` would set `e.controlSinceSnapshot = true` when it routes a control event that `matches(e.filter)` (cleared in `resetForSnapshot`). A snapshot joiner after a matching priority event then opens its own server subscription and gets a fresh reducer snapshot. This costs one extra sub only in that case, so it interacts with w36's cap work. Alternative: append matching control events to `e.replay` so joiners replay them through `onControl`-equivalent delivery. That is cheaper on subs but changes the delivery contract.

Option B, shell only: in `DockLayout.tsx` hand-off, remove the seeded panel **before** `reconcile` opens the bound one. The entry then closes and the bound panel gets a fresh server snapshot. This only covers the seed hand-off, not other joiners, and it reorders a visible UI transition.

## Tests and evidence
- New `ui/src/panels/uds/udsStateHydrate.test.ts` (6 tests) uses native shapes: the gateway `SnapshotItem` around the `StateReducer` item, the `Envelope` around `handlers::status`, and the QA-captured qa-uds `StatePayload` (UDS-CONNECT.json: doip, DoIP 0x1001, P2 50/5000, session default assumed). Coverage:
  1. Stale pre-connect replay plus `uds.status` → "연결됨", route `doip · DoIP 0x1001`, overview `qa-uds · doip`, `P2 50/5000`. Exactly one `uds.status {}`, no connect, no sends, no repeat on later frames or snapshots, out-of-scope target ignored.
  2. A live link-down that arrives while the call is in flight wins over the delayed "up" reply.
  3. After hydration: an older seq 299 is ignored, a newer live down (seq 340) wins, and resync plus a fresh native snapshot (gen 2) replaces it.
  4. A native snapshot already carrying the state, then live down at 324, then a delayed reply → stays down.
  5. A failure envelope leaves the link unreported, and a successful R0 result row does not make it "up".
  6. The unbound app never calls `uds.status` or `uds.connect`.
- The negative control is confirmed. With the hydrate call disabled, tests 1 and 3 fail (2 failed / 4 passed). After restoring it, everything passes.
- `uds.test.ts`: one assertion updated. The bound panel's call log is now exactly `[{uds.status,{}}]` instead of `[]`.
- Runs, once each: `vitest run src/panels/uds` → 4 files, **61 passed**, rc 0. `tsc -b` → rc 0 (full UI). `eslint src/panels/uds` → rc 0. The full UI suite was not rerun; ROOT/QA rebuild covers it.

## Source fingerprint (sha256)
```
861d6f4a2605d5595f52ff95f26cbbe32f5cee9a01c6993a052db20f9ce59443  ui/src/panels/uds/consolePanel.ts
b462364cab36a58304896f9768310161db79ebb9f9eaed2f35b4c66630c2b51e  ui/src/panels/uds/uds.test.ts
c269f93acfc7091aade845a02a0f325f9c601020426f545bd3ab3b926b00e14b  ui/src/panels/uds/udsStateHydrate.test.ts (new)
af7287329cb32c118e387c6db122098201268f8347724a7895d02f43394daa68  ui/src/panels/uds/udsModel.ts (unchanged)
e3349599db33d1bfcf874989028d3c228a7fef674d96a9be3ba0b84415e6fa22  ui/src/panels/uds/udsPrefs.ts (unchanged)
c9cea2bb8da3a0e5c99a482932d26cf0140e9007f7b807cec89fac3b64de34fd  ui/src/panels/uds/udsTools.ts (unchanged)
142bf04353e48598081b216a7b5dd2db2dfb7d7de6fb3b869cfc8fa161dac0cd  ui/src/panels/uds/udsUnbound.test.ts (unchanged)
f39343f76fd91d8de94d32cb0685ff990b08ca9b919f5feeb39ee7fca7ae9b97  ui/src/panels/uds/udsWorkbench.test.ts (unchanged)
```
No engine/Rust change. No shared SDK/shell/net edit.

## For ROOT/QA re-run
Rebuild the app with this UI, then repeat the real flow: the UDS service seed, then `uds.connect qa-uds`, then the bound panel appears. Expected without any request: the header shows "연결됨", the route shows `doip · DoIP 0x1001`, the overview shows `qa-uds · doip`, and the session chip shows `P2 50/5000`. The daemon journal should show one `uds.status` from the UI connection per bound-panel mount and nothing else new.
