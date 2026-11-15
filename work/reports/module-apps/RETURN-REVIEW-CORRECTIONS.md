# RETURN-REVIEW-CORRECTIONS: REVIEW-FIX F1–F4 fixed

**READY_TO_FREEZE.** ROOT has declared the final SOURCE_FREEZE. I made no source edits after it. Everything after it was
read-only: the self-review, the hashes and this file.

**How it was done.** Fresh native foreground Opus 5.5, with no subagents.
**Not touched:** Git/index, browser, devices, daemon, app, shell/dock, other modules, backend.

**UDS preserved.** All 4 UDS files are byte-identical to RETURN-UDS-STATE.md (`shasum -c`: 4/4 OK):
- consolePanel `861d6f4a…`
- uds.test `b462364c…`
- udsStateHydrate.test `c269f93a…`
- udsModel `af728732…`

**Unchanged a52 files** (`can-live/after.sha256` entries I did not touch, still OK):
- `core.ts`
- `protocol.ts`
- `fakeNet.ts`
- `services.ts`
- `qaCanMarker.native.json`

The old a52 actual-runtime evidence is untouched.

## Fixes (minimal, no redesign of the lease/merge)
**F1 (Medium), `client.ts`: a gap goes only to the handles it concerns.**
- The narrow-handle bypass `|| e.kind.endsWith('.gap')` is replaced by `gapConcerns(filter, e)`. A `*.gap` event reaches
  a narrow handle only when its **domain and channel** match the handle's. The kinds and origins dimensions are
  ignored.
- Domain-wide gaps (no channel) reach every handle of that domain.
- Gaps whose domain names **no affected stream** reach every member of the subscription:
  - `display`: hub overload / subscribe-start resync gap, or a local `display.gap` of a cross-domain union.
  - `core`: the hub's subscribe-start `display.gap` (`stream.rs:114`).
  - `record`: daemon journal `record.gap` (`daemon.rs:951`, `pool.rs:112`).
  - `source`: listed defensively; no emitter found.
- The batch-level `display_gap` summary is still shared.
- Non-narrow (exact) handles are unchanged.

**F3 (Low), `mux.ts`: `onOpen()` re-places parked handles.**
- `onOpen()` ends with `this.placeParked()`. That is the one call.
- It is idempotent and gated by `entries + draining < maxSubs`, as before. `relieve` keeps its `maxSubs*2` guard.

**F4 (Low), `api.ts`, `mux.ts` and `context.ts`: the initial lease travels with the subscribe.**
- **`api.ts`:** `SubscribeOptions.active?: boolean` sets the initial lease (default true).
- **`mux.ts`:** `mux.subscribe` strips `active` before the entry options are built. So it is never part of
  `subscriptionKey`, merge compatibility or `subscribeParams` (the wire). Pinned handles are always active.
- **`context.ts`:** passes `active:false` inside the subscribe when the panel is hidden. The follow-up
  `setActive(false)` is removed.
- **Worker/inline parity:** the same `sub` message and the same `core.ts → mux` path are used. The test asserts the
  message survives `structuredClone`.
- **New behaviour:** a hidden lazy open at the cap now parks honestly (`subscription_cap` on that hidden handle only).
  It relieves pressure when the panel is shown.

**F2 (Low): pre-mount agent-read marks are seeded from the shell's ActivityStore.**
- **`context.ts`:** new read-only `ctx.agentReads(limit, channel?)`.
  - It returns the panel domain's ranged activity entries from `deps.agentCursor.recent` (the same connection's
    ActivityStore that DockHost already passes; no shell change).
  - It returns the newest `limit`, oldest first, bounded by the 5 000-entry ring.
  - It makes no subscription and no call.
- **`canTrace.ts` (bound panels only):** seeds the model once, at creation, with `AGENT_RANGES` (16) ranges.
  - It raises `lastActSeq`, so the existing seq dedup of the live topic skips those events.
  - The live-only activity topic on the shell stream is unchanged.
  - Unbound CAN reads nothing.
- **`traceModel.ts`:** tiny shared `addAgentRange()` helper, also used by `applyActivity` (no behaviour change).
- **`sdk/testing/harness.ts`:** test-only `opts.activity`, which pre-fills the harness ActivityStore.

## Tests and negative controls (each fails on the old code, passes on the fix; logs in `review-corrections/`)
| Finding | Test | Negative control |
|---|---|---|
| F1 | `net/client.test.ts` (new, 4 tests): merged `can/[a,b]` own/foreign/domain-wide/`display`/`core` gaps; cross-domain `{can,psu}` (PSU gets no CAN or other-channel gap, no empty batch); kinds ignored for own gap; batch `display_gap` shared; exact handle unchanged | old predicate → **2 failed** (`f1-negative.log`) |
| F3 | `mux.test.ts` "F3: disconnect while a suspended entry's subscribe is in flight…": reject while offline → reopen → C subscribed, bind + snapshot + end reach it, busy 0, maxHeld ≤ 2 | no `placeParked` in `onOpen` → **1 failed** (`f3-negative.log`) |
| F4 (mux) | `mux.test.ts` "F4: … hidden lazy open … merges/resyncs nothing visible": 0 merges, 0 suspends, 0 calls, only its own `subscription_cap`; pinned+`active:false` stays active; identical filter with `active:false` shares the entry; no `active` in wire params; shown → placed | mux ignores `active` → **1 failed** (`f4-negative.log`) |
| F4 (SDK e2e) | `capacityLive.test.ts`: lease test now asserts `opts.active` `[undefined,false]` and `active` `[[false,true],[true]]` (honest: no "true then false"). New e2e: Integrated 8/8 with real inline client and cap-enforcing daemon; a hidden lazy open → 0 subscribes, 0 unsubscribes, 0 resync marks on the 7 visible mirrors, busy 0 | old context ordering → **2 failed**, including e2e `expected 10 to be 8` (merge happened) (`f4-context-negative.log`) |
| F2 | `canTrace.test.ts`: (1) a read already in the store before mount, frames from the CAN snapshot after mount → rows marked `[t,t,f]`. Also checked: still 2 subs (activity `snapshot:false`), 0 calls, other channel's read not applied, re-delivery of the same activity seq leaves 1 range, a newer live read adds one. (2) Unbound panel: 0 subs, 0 calls, no mark | seed disabled → **1 failed** (`f2-negative.log`) |

Every negative control was run on a temporary in-place revert. The fixed file was restored right away and verified by
sha256.

## Gates (run once at the final source; ROOT verified)
- **Full UI** `npx vitest run`: **69 files, 761 tests passed, rc 0**. That is the 752 baseline plus 9 new (`ui-test.log`).
  - A full run was justified because the PanelContext/mux/SDK context changes are shared by every panel.
- `npx tsc -b`: rc 0 (`ui-tsc.log`).
- `npx eslint src/net src/sdk src/panels/can`: rc 0 (`ui-lint.log`).
- Desktop cargo was not run (no backend change).

## Hashes
**How the diff was proven (no Git):**
- I rebuilt the pre-change files by reversing my edits.
- They hash **identical to the frozen a52 fingerprints for all 8 recorded files** (api, mux, client, mux.test, context,
  capacityLive.test, canTrace, canTrace.test).
- So `review-corrections/diff.patch` contains only this change.
- The before-hashes of `traceModel.ts` and `harness.ts` come from that same reconstruction (no a52 record exists for
  them).

**Files written:**
- `review-corrections/before.sha256` and `review-corrections/after.sha256`.
- `review-corrections/LOGS.sha256`: hashes of the logs and the patch.

**After (sha256):**
```
0bb7f6e02d797737dd46a6137ce0a26559306c713fd00dc995b92d7d34d30c80  ui/src/net/api.ts
f76fb015dc3555d402ddda087705d44b54c2a04d1493bee6493a98986bc6e75a  ui/src/net/mux.ts
0d60df4d7408bdbcb51760c9a5ccc0ea59fe5331dcfbba69c6e9f931b24d6e40  ui/src/net/client.ts
f5d6e62adaf009c7ae499afdc16997226eae91ff6fb9f8dc6777b84c7c9e8578  ui/src/net/mux.test.ts
d9a276c4e1d11d995d10671fa92c6d304f5b959e5e2274caf1d8e17b060ada34  ui/src/net/client.test.ts   (new)
897dd8b5204633d5b97b8bb9fb692c37a94395984253920e3e13cfa36da26c72  ui/src/sdk/context.ts
acce9f1037eb30043f37069b1f37606186fb6fbf7959dba59f4cad3e02a74c62  ui/src/sdk/capacityLive.test.ts
db5f983154346d8ded8b4c88e14d17315e392f2ece6c56d6065976e71c2e4df7  ui/src/sdk/testing/harness.ts
b3656cbf855e1770fc182efc365aebda9f6a9f03f0dc224ecdb2669f64a1afda  ui/src/panels/can/canTrace.ts
74a51f4fbbafb378486386a31abc6412299ba89e485f934439eca1c0f2a02388  ui/src/panels/can/traceModel.ts
857cf277fcf28b86ec44a88b41d4994b770046cf46a115f0f36342c2094cc97b  ui/src/panels/can/canTrace.test.ts
```
**Reviewer delta set:**
- The three files REVIEW-FIX asked to re-hash: `client.ts`, `mux.ts`, `mux.test.ts`.
- The rest of the delta: `api.ts`, `context.ts`, `capacityLive.test.ts`, `canTrace.ts`, `traceModel.ts`,
  `canTrace.test.ts`, `harness.ts`, `client.test.ts`.

## Self-review and residuals (no action taken: source frozen; ROOT decides)
- **Snapshot parts of narrow handles are unchanged.**
  - `narrowPart` filters snapshot events with `matchesFilter` only. A kind-filtered narrow handle therefore does not get
    its own stream's gap from a snapshot backlog.
  - This is pre-existing: there was never a bypass there. It is now consistent in the "no foreign gaps" direction.
- **The F2 seed is a one-shot at panel creation.**
  - Case: the shell snapshot lands *after* a CAN panel mounted at reload.
    - Ranges from that snapshot reach CAN only if its live-only handle was bound to the shell entry during its
      snapshot phase (it receives the untargeted parts).
    - Otherwise they do not.
    - The specified case (the store already holds the range) is covered.
  - Activity seq dedup is epoch-unaware (ActivityEntry has no epoch). That is the same as the existing live path. After
    a daemon epoch change within one tab, seeded old-epoch ranges could mark wrong seqs until newer reads arrive.
    Fixing this would need an epoch on ActivityStore entries (a store change, not done).
- **`core` is treated as a streamless gap domain.** In practice `core` is carried only by the pinned shell stream, which
  is never merged. Its covering joiners are core-activity handles.
- **A hidden lazy open at the cap now shows "구독 대기" on that hidden CAN panel** (an honest park) until it is shown.
  Before, it relieved pressure at once.
- REVIEW-FIX's Info items (REC-active hidden UART, hydration across a lease resync) were not in scope and are unchanged.

## Next steps
QA: run the final strict build on these hashes. Reviewer w37: focused delta review only. The old a52 runtime evidence
stays as recorded.
