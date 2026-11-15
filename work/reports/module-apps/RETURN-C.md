# RETURN-C — CAN · PSU · Logic · Video module applications

READY_FOR_REVIEW. Owner C (w33), foreground Opus 5.5, no subagents, no Git/index, no CUA/Ego, no daemon/app touched
(live app 52634 / daemon 52646 left alone). Desktop cargo not run.

## Scope delivered

Unbound = panel mounted with **no `channel` prop** (the shell seeds `can.trace {}`, `psu.gauge {}`, `logic.analyzer {}`,
`video.workbench {}`). Each one renders the existing full tool with empty data plus visible connect/config controls.
Navigation/mount never opens a device, sends a frame or queries the engine.

### CAN (`can.trace {}`)
- `channelOf()` now returns `string | null`, so the implicit `ch0` is gone. The unbound panel never calls `ctx.store`
  (subscriptions are lazy) and uses a never-fed local mirror and row view: **0 subscriptions, 0 RPCs**.
- The full trace grid, view tabs (수신/송신 · 트레이스), filters, Tx list composer and send line are all rendered. Send
  line, Tx-list send and the dry-run risk preview are disabled/inert until a channel is bound.
- New `canConnect.ts` section has: channel name, bus (optional, shares a sim bus), backend, nodes (`default` BCM / `none`),
  and an explicit **[연결]** button that runs `ctx.act('open')` → `can.open{channel, bus?, nodes}`. That matches the
  worker contract exactly (`crates/loom-can/src/worker.rs` `open`/`resolve_nodes`). A CLI-equivalent line is shown.
  Only `sim` is selectable. SocketCAN and PCAN/Vector/Kvaser are listed disabled as "엔진 미구현", because the engine
  has only `can_sim`.
- **[열린 채널 조회]** is an explicit read-only `can.status{}` query. It lists the open channels (several interfaces).
  [보기] opens the bound trace and only observes (no RPC).
- After a successful open: `ctx.ui.openPanel('can.trace', {channel, view})`.
- Manifest: query `can.status` and action `open` (`can.open`, request, operate) were added. Bound behaviour is unchanged.

### PSU (`psu.gauge {}`)
- The implicit `bench` is removed. When unbound there is no subscription, no `onControl`, no `psu.status` poll, and
  **no `psu.out` / `psu.set` / `psu.disconnect` path at all**: every handler checks `bound`, and `act()` itself
  rejects when unbound. Emergency OFF, output and apply are disabled and the `!` key does nothing.
- **Bound alias:** emergency OFF stays enabled even with an unknown or lost link (`connected:false`), as you specified.
  - Output toggle still needs a known `out`.
  - New: [적용] needs a state the device reported (`psu.state` or `psu.status`) before it sends anything (no blind write).
  - New: [연결 해제] sends `psu.disconnect{ch}` (operate). It only closes the link; the output is held as the engine does.
- New `psuConnect.ts` section:
  - Fields: alias, backend, SCPI address, vmax/imax (lower-only, as `Limits::from_value` applies them).
  - Backends: `sim` and `scpi-tcp` (address validated as loopback `ip:port`, mirroring `scpi::parse_loopback`).
    `visa`/USB/GPIB/LAN is listed disabled as "엔진 미구현".
  - Explicit [연결] sends `psu.connect{ch, backend, address?, vmax?, imax?}`, then the bound gauge opens.
  - Shows the CLI line, and notes that connect never switches the output and the worker drives one supply at a time.
  - [보기] opens the gauge for an already-connected alias (navigation only). There is no list RPC for PSU channels.
- I never send output-on or set during tests except to FakeNet. No real daemon was used.

### Logic (`logic.analyzer {}`)
- The config `<details>` is open by default when unpinned, so the source/driver/rate/decoder controls are visible.
- The old `toggle` listener would have fired `logic.status` when the panel opened itself. It is replaced by user
  gestures only: clicking the summary while closed, or the new **[엔진 확인]** button (read-only `logic.status`).
- Start stays an explicit click using the existing `startParams`. The truthful capability line is unchanged.

### Video (`video.workbench {}`)
- The source form is open by default when unpinned. No RPC and no media fetch happen on mount (tested with fetch stubbed).
- **[엔진 확인]** runs `video.capabilities` explicitly and shows a truthful line via the exported `capabilityText`.
  It covers FFmpeg path/version/error, implemented vs unverified (avfoundation) vs unsupported (rtsp/decklink), and audio.
- A `video.open` refusal now stays visible in `video-open-error` with code and message, as well as the toast. Windows
  `E_UNSUPPORTED` ("not available on this platform yet") stays truthful. The limits note says Windows capture is unsupported.

## Files (all inside my scope, plus the crate edit you authorized)
Modified:
- `ui/src/panels/can/canTrace.ts`, `can.css`. Note: `can.css` already had 6 lines of earlier dirty diff; I only appended.
- `ui/src/panels/psu/gaugePanel.ts`, `psu.css`
- `ui/src/panels/logic/logicPanel.ts`, `logic.css`
- `ui/src/panels/video/videoPanel.ts`, `video.css`. Note: `video.css` already had 18 lines of earlier dirty diff; I only appended.
- `crates/loom-psu/src/gateway.rs` (`panel_manifest` metadata only): adds actions `connect`/`disconnect` for UI parity
  and removes `"defaultChannel": "bench"` from the topic metadata. Nothing in the repo reads `defaultChannel` (grep).
  No handler, protocol or engine change.
- `crates/loom-psu/tests/psu.rs`: parity assertion (action ids/rpcs, no defaultChannel).

New:
- `ui/src/panels/can/canConnect.ts`, `canModule.test.ts`
- `ui/src/panels/psu/psuConnect.ts`, `psuModule.test.ts`
- `ui/src/panels/logic/logicModule.test.ts`
- `ui/src/panels/video/videoModule.test.ts`

No shell/SDK/shared files were touched. The CAN crate has no panel manifest, so it needed no parity edit.

## Verification (direct, exact rc)
- `cd ui && npx vitest run src/panels/can src/panels/psu src/panels/logic src/panels/video` → **13 files, 154 tests
  passed, rc=0**. That includes all pre-existing tests for these panels (bound behaviour unchanged). jsdom prints
  "HTMLCanvasElement getContext not implemented" noise, as before.
- `cd ui && npx eslint src/panels/can src/panels/psu src/panels/logic src/panels/video` → rc=0
- `cd ui && npx tsc --noEmit -p .` → rc=0. This is the shared typecheck at that moment; peers' partial edits were
  type-clean then.
- `cargo test -p loom-psu --test psu` → 15 passed, rc=0. `cargo fmt -p loom-psu --check` → rc=0. Root target only,
  not desktop.

Focused tests added:
- **Unbound:** zero subs and zero calls after timers; full tools present; sends, dry-run, emergency, output and set inert
  even when the buttons are forcibly re-enabled.
- **Explicit connect:** exact params for CAN, PSU sim and PSU scpi-tcp. LAN address and invalid name are refused
  before any RPC. Unimplemented backends are disabled. Watch right disables connect.
- **Bound:** emergency OFF on a lost link sends exactly `psu.out{channel:'rail', state:'off'}`. Set is blocked until a
  device-reported state arrives. Disconnect params checked.
- **Logic/video:** default-open config with no RPC; a synthetic `toggle` event doesn't query; the explicit engine check
  is exactly one query; the open/start click sends the real params; the platform refusal stays visible.

## Fingerprint (sha256)
```
87bf13169dc1f5838a39c2b86eea1a3b79681c1557f229bb568fec9ffaaed179  ui/src/panels/can/canTrace.ts
bb0ac1cd6527a51057a723ce7e95c714b0b19b413c4d395a6900758a89e64d1a  ui/src/panels/can/canConnect.ts
b833b8eee0401c2ce6deeda01a89303b797c57151be46fb9272b1ab9c91446d2  ui/src/panels/can/canModule.test.ts
b356d10bf623519484346ea3da8e4b9ac32bf1198b36facf6ca3577b33cf9aa3  ui/src/panels/can/can.css
395662484361e4f1d49d95052e422befcdc38c0854ec087c50f1ded3562dc8b5  ui/src/panels/psu/gaugePanel.ts
f83b11dce5204fe821953cfac1448088219fc9b4a2d3d29ffc7058708dd79292  ui/src/panels/psu/psuConnect.ts
1bd9d427ba99d17691636a880f765c896973d6a13a4d2959fe45064033648f1d  ui/src/panels/psu/psuModule.test.ts
e15800d4f633c05a192922302f300e3963d4571ba4574dd3e149e431ff52e78a  ui/src/panels/psu/psu.css
fe133b85bda6a4fe24a69df2d6f5a6b7001274e54254586fcab7efc441e24c5f  ui/src/panels/logic/logicPanel.ts
3c2d8b44cd4eaa47eb2696960ae2eb79d3a5324efd456005caf8305328e6f138  ui/src/panels/logic/logicModule.test.ts
bca0fc1aa2a09d0c81b952a5f1abc4baecfbbbcc6f23892d0427853cc7322111  ui/src/panels/logic/logic.css
97ad206fea387e4b64450a7082eeb2d68f6e63f6709c6629eedeaa83b0957265  ui/src/panels/video/videoPanel.ts
fa63826e3fffbfdcf533ef7d302ee1807ffabc5b10442ad1bab1c30ff5948f79  ui/src/panels/video/videoModule.test.ts
37611a0eb6e49b5eb0ee730669c5fcb9ea35a10ba03477af79ab54063a1a889e  ui/src/panels/video/video.css
95944c9847782989485630b1db692f07e3a11bffb4f987030c4e9d5dae9fb228  crates/loom-psu/src/gateway.rs
0d89d1f9419ec3ef0a02d2e5c083e515a8fae1a471f6b64908468b09dd6faf10  crates/loom-psu/tests/psu.rs
```

## Self-review / remaining issues
1. **Post-connect navigation (shell-owned).** After a successful connect, the unbound CAN and PSU panels call
   `ctx.ui.openPanel(id, {channel})`. Today `services.openPanel` routes to `#/p/<domain>/<name>?channel=…`, a standalone
   route outside the service tab. Owner A/ROOT may want that to land in the service tab or Integrated instead. This is
   integration only; the panels don't depend on it. No REQUEST file is needed unless ROOT wants a different hook.
2. **The unbound panel doesn't rebind in place.** Topic filters are fixed at creation, so binding is a navigation to
   the bound panel, which is the existing panel.
3. **PSU has no list RPC.** `psu.status` without `ch` resolves the single supply implicitly, so it isn't used for
   listing. [보기] takes a typed alias. The PSU worker drives one supply at a time (E_PORT_BUSY on a second alias).
   The UI documents this rather than inventing support for more.
4. **New bound-gauge rule:** [적용] now needs a device-reported state first. Existing tests always deliver state first
   and pass. A gauge bound to a freshly connected sim gets state immediately (connect emits `psu.state`, status poll 1 s).
5. **e2e `ui/e2e/m1/test-screen.spec.ts` "psu.gauge (SimPsu bench)"** opens `/l/test` and expects a `bench` gauge in
   it. The current (earlier, shell-owned) `routes.ts` test preset seeds only `shell.test`. This is not from my change,
   but ROOT should check it when running the full e2e. Nothing in my panels injects `bench`.
6. **Logic/video stay observation-subscribed on mount.** That's the domain-wide `core.subscribe` that shows existing
   captures, as before. It never starts a capture or opens a device.
7. **Not verified:** a visible native run or a real daemon for these connect paths. ROOT handles the native check.
   Suggested sim smoke:
   - CAN tab → [연결] `ch9`/sim → frames in the bound trace.
   - PSU tab → [연결] sim alias → gauge shows state with output unchanged. Do not press output ON.
   - Logic/Video tabs → no capture until [캡처 시작]/[열기].
