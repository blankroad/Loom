# PLAN — ETH panel closes on normal capture stop (UI defect candidate)

Owner: eth-live-check-opus (w1P:p1). Approved by ROOT 2026-11-14: narrow fix, ETH-owned files, plus shared shell
file(s) that are named to ROOT and owned exclusively for the duration. No daemon/CLI/Rust/other-module changes.
No asset swap in the ROOT browser until ROOT is notified.

## Repro (ROOT, native UI, viewers 1/1)
`eth capture stop --ch qa_eth_live` (w28, gen2) → the ETH packet panel is removed (row data-seq 656 is gone).
The inactive explorer does not offer an ETH row. The backend still serves `eth show`/`trace` for seq 656.

## Root cause (HEAD source, read-only)
1. Backend is OK. `eth.capture_state {state:"stopped", ch, generation}` is emitted (seq 507 gen1, 792 gen2). It is
   a control-path kind (`isControlEvent`: `*_state`). `readChannelSignal` maps `stopped` → `closed`, and
   `ShellControlStore` keeps the entry as `closed`; it never deletes. The ETH reducer snapshot
   (`crates/loom-eth/src/gateway/evidence.rs` snapshot) also lists stopped channels.
2. Defect: `ui/src/shell/dock/activeWorkspace.ts` `availableSlot()` keeps a domain slot only when its channel is
   `state === 'open'`. `DockHost.reconcile()` (`ui/src/shell/dock/dockHost.ts:291`) runs on every shell store change
   (each control event/heartbeat) and calls `api.removePanel` for every slot `availableSlot` rejects. So:
   - a normal stop removes the ETH panel, together with its in-memory rows, selection and detail;
   - re-opening it from the inactive explorer (`showSlot`) is undone by the next reconcile, a few ms later.
   For live-only devices (UART/CAN close) this pruning is the tested contract (`defaultLayout.test.tsx`
   "pruning inactive panes"). ETH captures, unlike those, keep queryable history (`eth.query` / `eth.show`
   work after stop), so pruning them hides retained evidence.
3. Explorer row: `ChannelBar` is scoped by route (`ShellApp.tsx:147`). `#/s/uds` → uds/can/eth, `#/s/<d>` → that
   domain, `#/p/<d>/…` → the panel's domain. Whether the missing row is scoping or something else has to be
   confirmed with ROOT's route hash. The store keeps `eth/qa_eth_live` as closed.

## Fix (narrow)
- `ui/src/shell/dock/activeWorkspace.ts` (shared shell, owned exclusively by ETH until ROOT integration):
  `availableSlot` also keeps a slot for a domain that retains history after close (`eth` only), provided the
  channel is known to the store with an explicit `closed` state. Missing/`unknown` channels are still pruned.
  Presets and auto-open are unchanged: stopped captures are never auto-opened, and nothing restarts or re-sends.
- Regression tests: `ui/src/shell/dock/activeWorkspace.test.ts` and a dock lifecycle test in
  `ui/src/shell/dock/defaultLayout.test.tsx`. ETH stop keeps the panel, and explicit reopen from the inactive
  explorer keeps it. UART/CAN close still prunes, and an unknown/missing ETH channel still prunes.
- Validation: vitest for the touched suites plus eth panel tests, `tsc`/eslint on the touched files, using the
  existing `ui/node_modules`. No `ui/dist` rebuild or asset swap until ROOT says so.

## Not in scope
Rust/gateway/CLI, other modules' panels, auto-restart of the capture, new sends.

## Result (implemented, not deployed)
- Diff: `ui-stop-retain.patch` (3 files: `ui/src/shell/dock/activeWorkspace.ts`, `activeWorkspace.test.ts`,
  `defaultLayout.test.tsx`). Shared shell file `activeWorkspace.ts` is owned exclusively by ETH until ROOT integrates it.
- The new tests fail on HEAD's `activeWorkspace.ts` (2 failed / 61 passed) and pass with the fix.
- `tsc -p tsconfig.app.json --noEmit` clean. eslint on the touched files clean. Full `vitest run`: 50 files /
  557 tests passed.
- No `ui/dist` build and no asset swap in ROOT's browser. No Rust/CLI/daemon change, no new capture or send.
- Open item: the missing ETH row in the inactive explorer is not reproduced in tests. ROOT's route hash at that
  moment is needed: a `#/p/uds/...` or `#/s/<non-eth>` scope hides ETH by design (`ShellApp.tsx:147`). After deploy, the
  history view is `#/s/eth` or `#/p/eth/packets?channel=qa_eth_live`, then the inactive explorer row.

## Second UI defect: eth.query bare numeric cursor (ROOT native repro on 53814)
- Repro: applying display filter `arp` → `eth.query bad cursor "94": invalid cursor bad HH:MM:SS`. The pause button
  fails the same way (`until "308"`). w5 ARP seq 170 existed (observer) but the filtered UI was empty.
- Cause: `ui/src/panels/eth/ethPanel.ts` `goComplete()` sent `range:{to:String(seq)}`, and export sent
  `range:{from:String(..),to:String(..)}`. The proto cursor grammar (`crates/loom-proto/src/cursor.rs`) takes
  `seq:N` / `e<E>:seq:N`. A bare number is parsed as a clock time. Responses still return bare decimal `range` (checked on the
  live RPC: `--until seq:308 -Y arp` → range {from:"170",to:"170"}, `eth:qa_eth_fixed@seq:170`).
- Fix: `ethModel.ts` `seqCursor`/`seqRange` emit `seq:N` and leave out an unknown end. `goComplete` (filter + pause)
  and export use them, and an empty head (`0n`) becomes an open range. Filter semantics are unchanged (the filter is passed as is).
  The panel has no stream epoch in its context, so epoch-less `seq:N` is used (current epoch); the CLI uses the same.
- Tests: the existing `eth.test.ts` assertion of bare `'41280'` (which encoded the bug) → `{to:'seq:41280'}`. New regression test
  covers pause with an empty head, pause with packets, filter `arp`, and export from/to.
  Mutation check: HEAD's ethPanel.ts → 2 failed / 33 passed. Fixed → 35/35.
- Checks: `tsc -b` clean, eslint (`src/panels/eth`, `src/shell/dock`) clean, full vitest 50 files / 558 tests passed.
- Build p2: `pnpm build` rc 0 → ui/dist `index-zZUL1YL_.js` 8b0916c0…, index.html f3655b0c… (`ui-dist-p2.sha256`,
  `ui-build-p2.log`). The bundle contains `seq:${e}` and `new Set(["eth"])`. ROOT reloads the disk bundle.

## Skill (ETH-owned)
- `skills/loom-eth/SKILL.md`: `capture start sim0` → `capture start sim --backend sim --ch <ch>`, using the returned iface.
  This was a proven mismatch: `eth ifaces` returns `sim` and `replay` only.
- New `skills/loom-eth/references/live-observer.md`: bounded simultaneous observer, field correlation, own packet ≠ device
  proof, UI pass conditions. ROOT must add the entry `loom-eth/references/live-observer.md` to `crates/loom-cli/src/skills.rs`
  (shared file, not edited by ETH) when refreshing the embedded CLI.

## Third UI defect: empty welcome hides retained ETH history after the last live close (ROOT native repro 09:12:18)
- Repro: ETH `qa_eth_fixed` stopped, then reopened from inactive with 841 visible. The UART owner then normally closes `qa_uart_fixed`, the last
  open channel. ShellApp shows the global welcome ("현재 연결된 인터페이스가 없습니다") and makes the dock `inert`/`aria-hidden`.
  The ETH region is still in the DOM but covered, so phase4-reopen.json reported retainedTab/selected841/detail841=false.
  The store still knows the inactive ETH row. ROOT's FINAL_UI_PASS was corrected to FAIL.
- Cause: `ui/src/shell/ShellApp.tsx:103`. `welcome = no channel open && m1-default route`. It ignores docked retained history.
- Fix (shared shell, named to ROOT): `retainedHistory` = some `closed` channel of a history domain (`retainsHistoryAfterClose`, eth only,
  exported from `activeWorkspace.ts`) whose explorer view is actually docked (`dock.current.find`). The welcome is suppressed only then.
  When the user closes that view, the welcome returns on the next shell update (heartbeat about 1 s). Closed UART/CAN views are still
  pruned and still give the welcome (the existing `workbenchShell.test.tsx` case is unchanged and passes).
- Regression: new `ui/src/shell/historyWelcome.test.tsx` with real DockLayout and ShellApp. Steps: UART open, ETH live → stopped →
  close → explicit reopen, then the last UART closes and heartbeat/tick run. Expected: ETH view docked, no welcome, dock not inert.
  Then the ETH view is closed and the welcome returns. No send, capture or open calls.
  Mutation check: HEAD's ShellApp.tsx → 1 failed. Fixed → pass.
- Checks: `tsc -b` clean. eslint (ShellApp, new test, shell/dock, panels/eth) clean. Full vitest 51 files / 559 tests passed.
- Build p3: `pnpm build` rc 0 → `index-BzEw2Iyf.js` 2244b047…, index.html 9239b842… (`ui-dist-p3.sha256`, `ui-build-p3.log`).
  The bundle contains `seq:${e}`, `new Set([`eth`])` and `state==="closed"&&<retains>(t.domain)` in the welcome condition.

## Fourth defect: relative time base ("캡처 시작 기준") drifted 90.18 s on late join
- Evidence: same seq 841 shows 241.354517 in `phase4-live-arp.png` and 331.534763 in `phase5-reopen.png`. Ground truth from the observer:
  841 `ts.mono_ns` 331539992667 − capture `started_mono_ns` 5229250 (same worker clock `proc-19903`) = **331.534763 s**. Phase 5 is
  correct and phase 4 was wrong.
- Cause (two parts):
  1. Backend: `ChannelBook.update` (`crates/loom-eth/src/gateway/common.rs`) dropped `started_mono_ns`, so the ETH reducer snapshot
     (`evidence.rs` `snapshot()`) had no anchor. A tab that joins after the start event has left the backlog cannot learn the real start.
  2. UI: `ethPanel.ts` time cell used `capture?.startMonoNs ?? <first visible live packet mono>`, which silently relabels the first
     packet *this tab* saw as the capture start. The panel also ignored reducer snapshot items, and `readCaptureState`'s
     `prev.startMonoNs` fallback could carry an old generation's anchor over a restart. Phase 4 loaded the p2 bundle late, so
     the base was the first backlog packet, about 90 s after the start. Phase 5 was correct because the `stopped`
     capture_state event carries `started_mono_ns`.
- Fix (ROOT-approved scope):
  - `common.rs`: `ChanInfo.started_mono_ns: Option<u64>`, parsed exactly with `json_u64`. It is reset when a new generation arrives and kept when a later
    payload omits it. `evidence.rs`: the snapshot emits `started_mono_ns` (decimal string) only when known. Nothing is invented and
    there is no clock conversion. Send and decoder are untouched.
  - `ethPanel.ts`: consumes the `eth` reducer snapshot item (`state.kind == eth.capture_state`, matching `ch`). Capture updates are
    generation-aware: a new generation drops the old anchor. The first-visible-packet fallback is removed, so an unknown start
    renders the existing honest `?`.
- Tests:
  - Rust `reducer_snapshot_retains_capture_start_anchor_per_generation`: unknown is omitted, live anchor exact, stop keeps it,
    new generation resets, u64::MAX exact. Mutation check: HEAD common.rs+evidence.rs → FAILED. Fixed → ok. Full `loom-eth --test gateway`
    13/13, `--lib gateway` ok, `clippy -p loom-eth --all-targets -D warnings` clean, `cargo fmt --check` clean. Cache
    `CARGO_TARGET_DIR=/tmp/loom-uart-secret-build CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`. Note: one earlier focused
    test run, before ROOT's cache instruction, compiled loom-eth test deps into the repo `target/`. It did not replace `target/debug/loom`.
  - UI `eth.test.ts` "relative time uses the real capture start …": late join without an anchor → `?` (not 0.000000), snapshot
    anchor → 41.230000, partial stop keeps it, gen2 starting → `?`. Three mutations (fallback back, snapshot ignored, no gen reset)
    each → 1 failed. Fixed 36/36.
  - Full UI gate: `tsc -b` clean, eslint clean, vitest 51 files / 560 tests passed.
- Build p4: `pnpm build` rc 0 → `ui/dist/assets/index-DCXMffOL.js` c722ea65…, index.html 1876bb6e… (`ui-dist-p4.sha256`,
  `ui-build-p4.log`). The bundle contains the snapshot consumer (`e.domain==="eth"&&n.kind==="eth.capture_state"`), `seq:${e}`,
  `new Set(["eth"])`, and no first-packet fallback.
- Native acceptance needs the new gateway (Rust change). ROOT builds the final default gateway and CLI. **SOURCE FROZEN** at patch
  `ui-stop-retain.patch` sha256 e3438d48…43fd.

## Fifth defect: retained panel header stays LIVE and stop stays enabled after the real stop (ROOT native, 54790)
- Repro: ROOT stopped `qa_eth_final` (w3, backend `stopped`). The shell saw the channel as closed, but the retained ETH panel header still showed
  `● LIVE` and the stop button was enabled for more than 8 s. p2 waitSTOPPED timed out.
- Cause: per the hub contract (`crates/loom-gateway/src/hub/mod.rs` header), priority kinds (`is_priority_kind`: every `*_state`,
  including `eth.capture_state`) go only to each connection's control queue, never to data subscriptions. The ETH panel read
  capture state from its topic mirror only, which means just the mount-time snapshot/backlog. `PanelContext` had no control feed.
- Fix (ROOT-approved narrow SDK hook):
  - Shared SDK: `ui/src/sdk/context.ts`, `PanelContext.onControl(cb)`. It is read-only and delivers net control events that match
    one of the panel's own declared topic filters (domains/channels/kinds). It is removed on panel dispose (`unsubs`).
    `ContextDeps.net` type adds `'onControl'` (`Pick<NetClient, …>`). No rights or behaviour change for other panels, which do not call it.
  - `ui/src/panels/eth/ethPanel.ts`: `ctx.onControl` handles `eth.capture_state` for its own channel, applies it
    generation-aware, and invalidates. A `captureSeq` high-water guard ensures an older state (backlog, snapshot `throughSeq`,
    or a late duplicate) never overrides a newer one. The stop button keeps the safety direction (enabled while live/unknown)
    and is disabled only when the state is known terminal (`stopped|error|permission_denied|replay_done`). Unsubscribed on dispose.
- Regression: `eth.test.ts` "capture state on the control path only …". With no data batch and no snapshot: live → `● LIVE`, stop
  enabled. stopped → `● STOPPED`, data-state stopped, stop disabled. Stale seq 15 'live' and other channel ignored. gen2 live → LIVE
  again. No capture/send calls. After dispose the listener count drops by 1. Mutations (no control wiring / no seq guard / stop
  always enabled) each → 1 failed. Fixed 37/37.
- Gate: `tsc -b` clean, eslint clean, vitest 51 files / 561 tests passed.
- Build p5: `ui/dist/assets/index-CxluPpWw.js` 3264e4d9…, index.html 6ca900c1… (`ui-dist-p5.sha256`, `ui-build-p5.log`). No Rust change in this
  step (gateway 8e4cc56b… already has the snapshot anchor). The bundle must be bundled or loaded by ROOT for the gen2 start→stop check.
