# RETURN-B — Ethernet + UDS unbound module applications

**READY_FOR_REVIEW** · owner B (w32) · foreground Opus 5.5, no subagents · no Git/index, no CUA/Ego, no daemon/app touched.

## Scope delivered
`eth.packets {}` and `uds.console {}` mount as the **full existing rich panels**, not wrappers, iframes or launcher cards. Data starts empty and real. Configuration/Connect is reachable before any connection. Opening the tab never captures, subscribes to a channel, sends or connects.

### Ethernet (`ui/src/panels/eth/**`)
- `ethPanel.ts`
  - `boundChannel(props)` is the channel when set. When it is missing the panel is **unbound**; it no longer falls back to `lan1`. This applied in two places: the panel body and the topic filter in `registerPanel`.
  - Unbound behaviour:
    - It never calls `ctx.store('pkts')`, so there is **no server subscription**. It uses an inert mirror instead.
    - It shows the full toolbar, filter bar, list/detail/bytes panes, socket drawer and status bar.
    - The channel card shows `채널 미선택 / 미연결` (`data-state=UNBOUND`). Its start/stop controls are replaced by the interface config picker: select (saved configs + `새 구성…`), `[연결…]`, `[구성 편집…]` and `[삭제]`.
  - Guarded while unbound: `eth.query` (pause, scroll-away, Space, autoscroll), `captureStop`, `export`, resend/edit-send, the socket drawer (open and writes), and display-filter pushdown (shows a toast instead).
  - Still allowed while unbound: the read-only `eth.capture.status` channel cards. They list every daemon alias (multiple aliases), and `[열기]` → `ctx.ui.openPanel('eth.packets',{channel})`. `onControl` `capture_state` from any channel only refreshes these cards.
  - Toolbar `채널 추가` / `캡처 시작…` open the same explicit connect dialog.
- `dialogs.ts`
  - `captureForm` add-mode now takes optional `initial` (prefill from a saved config), `onSave` (a **local-only** `[구성 저장]` button, no RPC), `title` and `testId`.
  - `onStarted` now also receives the config.
  - New `openConnectInterface`: it keeps the existing "never restart or overwrite an existing daemon channel" check.
  - `openAddChannel` and `openCaptureStart` for bound panels are unchanged. The `eth.capture.start` param shape is unchanged.
- New `ifaceConfigs.ts`: validated per-viewer saved configs (`loom.eth.ifaces.v1`, at most 16, corrupt or duplicate entries dropped). `upsertConfig` and `captureParams` mirror `CaptureStartParams`. Saved configs are never shown as channels.
- After a successful `[연결]`, the config is saved, a toast is shown and the shell opens that alias's bound panel. The unbound panel stays unbound; the bound panel is the existing channel-bound behaviour.
- The bound path, including the export-result patch, is untouched apart from `!bound` guards. All existing `eth.test.ts` tests pass, export included.

### UDS (`ui/src/panels/uds/consolePanel.ts`)
- Before this change it already mounted with `{}` and showed the editable presets and `[연결]`. The gap: with no `targets` prop, it allowed **send and dry-run against a preset-only alias the daemon never reported**.
- New `needsConnect()` applies only when there is no `targets` prop and the selected alias is not in daemon `uds.state`.
  - `writable()` = `editable() && !needsConnect()`, where `editable()` is the old `writable()`.
  - The DID/Raw inputs, catalogue templates, parameters and DTC templates use `editable()`, so they stay editable before connect. Send/Enter/tool-run use `writable()`.
  - The send button title and request message point to `[연결]`. The dry-run is skipped (`dry-run 미리보기는 연결 후`) and re-runs automatically when the alias appears.
- Unchanged:
  - The configured-target projection (`targets` prop): the gate/daemon still decides.
  - Live daemon targets are auto-selected and writable.
  - `uds.connect` presets and shape, raw/history/macros (tools), STOP/abort (always enabled), and CAN-TP/EthDiag still disabled.
  - There are no CAN/ETH observer changes.

## Added scope (user override message) — none used
No new actions/RPCs. Unbound uses only manifest-declared `eth.capture.status`, `eth.ifaces`, `eth.filter.check` and action `captureStart`; UDS uses declared `connect`/`send`. So I left `crates/loom-eth/src/gateway.rs` (daemon manifest) and loom-uds alone, with no SDK/shared/shell edits and no REQUEST needed.

## ROOT early-review fix
The TDZ bug (`chActs.replaceChildren(cfgSel,…)` placed before the declarations) is **fixed**: the call is now inside the `if (!bound)` block after the declarations. Tests mount the real `eth.packets {}` at runtime (`ethUnbound.test.ts` cases 1–5). That would have thrown before the fix.

## Tests (direct, run from `ui/`)
- `npx vitest run src/panels/eth src/panels/uds` → **5 files, 110 tests passed**, rc 0. This includes the new tests:
  - `src/panels/eth/ethUnbound.test.ts` (7):
    - Zero-channel full controls: no subscription, no `lan1` in calls/DOM, the only call is `eth.capture.status`, alias-dependent controls disabled, pause/Space/export/stop/Alt+S make no writes or `eth.query`.
    - Multiple alias cards: control refresh is read-only.
    - Explicit connect: the dialog opens with no write; `[연결]` sends exactly one `eth.capture.start {iface:'sim',ch:'lan7',backend:'sim',snaplen:262144,promisc:false,buffer_mib:64}`; the config is saved.
    - An existing alias is refused.
    - Saved config choose/edit/save/delete are local only.
    - Corrupt configs are dropped.
    - Topic filter: `{}` → no `channels`; a bound `lan2` panel subscribes to `['lan2']` only.
  - `src/panels/uds/udsUnbound.test.ts` (4):
    - Zero targets: the full workbench is editable, there is no request/dry-run/connect on mount, edit, Enter, click or DID pick, and the history is empty.
    - `[연결]` → `uds.connect {name:'ecu1',sim:'bench',timeout_ms:5000}` only. Sending stays disabled until `uds.state` is up; then a fresh dry-run, and send → `uds.request {target:'ecu1',bytes_hex:'22F190'}`.
    - Multiple daemon targets auto-select and are writable, while a preset-only alias is not.
    - `targets:'ecu1'` keeps the old projection.
- `npx eslint src/panels/eth src/panels/uds src/workbench/eth` → rc 0, no output.
- `npx tsc -p tsconfig.app.json --noEmit` → rc 2. The **only** error is in the peer A file: `src/shell/dock/DockLayout.tsx(24,10) TS2305 'UartEmptyPane'` not exported from `workbench/uart/UartWorkbench` (A's in-progress edit). There are 0 errors in eth/uds/workbench-eth.
- I did not run the full UI suite (scoped tests only, per the task).

## Files (sha256)
| file | sha256 |
|---|---|
| ui/src/panels/eth/ethPanel.ts (mod) | 39a39be8cb782bdbfb728a070a2042e4cf1f916a034a60c17f814df234bd88df |
| ui/src/panels/eth/dialogs.ts (mod) | a357136214aa7bf0b5d10684800f0e0cd4bc648828374c2b509a4e9d22de1c45 |
| ui/src/panels/eth/ifaceConfigs.ts (new) | c642f7c1c0c0d2ae09fa81292f458d667b05a47022c5da7d8cb3fc8cfdc51719 |
| ui/src/panels/eth/ethUnbound.test.ts (new) | 23303c383088a2665f96f71bf900a0ee21e40206eca299d9de2813a5488f155d |
| ui/src/panels/uds/consolePanel.ts (mod) | c58c05a68366d9a670f681f42a9dd74efaa07a97395f62974203177bb4ca3951 |
| ui/src/panels/uds/udsUnbound.test.ts (new) | 142bf04353e48598081b216a7b5dd2db2dfb7d7de6fb3b869cfc8fa161dac0cd |

## Self-review / remaining issues
1. **Bind is via a new panel.** Panel props are frozen and the topic filter derives from props, so an unbound panel cannot rebind in place without an SDK change. `[연결]` and `[열기]` call `ctx.ui.openPanel('eth.packets',{channel})`, which in the current shell sets `#/p/eth/packets?channel=X`. **Shell owner A / ROOT** should confirm that this is acceptable on a service route (versus docking a tab next to the unbound one). The harness `openPanel` is a no-op, so tests check the toast and RPC, not the navigation.
2. **Display filter.** A filter applied in the unbound panel is not carried into the newly opened bound panel (`propsSchema` has only `channel`). The unbound panel shows a toast saying so.
3. **Stopped daemon channel.** A saved config whose alias already exists in the daemon (for example a stopped channel) is refused by design, so it never restarts another channel. The user opens that channel's card `[열기]` and uses its own `[캡처 시작…]`.
4. **Old layouts.** Any persisted layout that stored `eth.packets` without `channel` and relied on the old implicit `lan1` now shows the unbound app. This is intended by CONTRACT, but ROOT may want to check for layout migrations.
5. **UDS before snapshot.** In an unbound UDS panel, sending is briefly disabled until the snapshot reports targets. This is safe and intended.
6. **Not covered.** No native/visible verification and no hardware or simulator smoke were run; those belong to ROOT.
