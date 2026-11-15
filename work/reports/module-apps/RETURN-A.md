# RETURN-A — shell + UART (owner A)

**READY_FOR_REVIEW** · source frozen 2026-11-15 16:25 · foreground Opus 5.5, no subagents, no Git/index, no daemon/app/CUA/Ego actions, no settings changes.

## What changed

1. **Seven permanent module tabs** (`ServiceNav.tsx`): Integrated (통합) plus UART, CAN, Ethernet, UDS, PSU, logic, video are always shown, whatever is connected (`SERVICE_TAB_ORDER` in `routes.ts`). Counts are truthful: OPEN channels of that service, and UDS counts real UDS connections only. Each tab has `data-count`, a full-name `title` and an `aria-label` ("UART, 열린 2개"). The badge appears only when the count is above 0. Added icons for PSU, logic and video.
2. **Header fit at 1440 and 900 px with no scaling** (`shell.css`, `TopBar.tsx`). The nav never shrinks above 900 px; the status chips on the right give way first:
   - ≤1600: agent and viewers hidden
   - ≤1500: tab icons (except 통합) and the interface count in the connection chip hidden
   - ≤1280: REC and rights hidden. A failing or degraded REC still shows.
   - ≤1180: short tab labels (ETH, UDS, 로직, 비디오); the full name stays in title/aria
   - ≤1060: brand label hidden; LIVE hidden only while it reads LIVE
   - ≤900: the nav scrolls horizontally with a thin visible scrollbar. ENV and STOP never drop.
   - The widths were estimated by hand; I have not measured them visually. ROOT should check this in the native QA.
3. **Unbound module seed** (`routes.ts`): `UNBOUND_MODULE_PANEL`, `isUnboundModuleSlot`, `unboundModuleSlot`.
   - `servicePresetForChannels` puts `can.trace {}`, `eth.packets {}`, `uds.console {}`, `psu.gauge {}`, `logic.analyzer {}` or `video.workbench {}` first, but only while that service has no OPEN channel of its own.
   - For UDS, CAN/ETH observations do not count as a binding, so the unbound console still shows.
   - UART has no unbound panel: its chrome is the application.
   - `availableSlot` keeps unbound module slots, which generalises the old logic/video exemption. Presets for Integrated never include them.
4. **One retained dock per workspace** (`DockLayout.tsx`, `dockHost.ts`). This is how I implemented layout and data continuity across tabs without replacing Dockview.
   - Integrated, each of the 7 modules and a `link` dock (for single-target deep links) are each their own DockHost. Each is created on first visit.
   - A dock stays mounted and keeps syncing while another tab is in front. `DockHost.setShown(false)` sets every panel `setVisible(false)`, so a hidden dock does no drawing work.
   - Switching tabs keeps panel instances, drafts, UART REC, running macros and split geometry.
   - `services.dock.current` is always the front dock. `uiState.noteDockChange()` makes chrome that reads it (UART toolbar, ShellBody) re-render.
   - Only the front dock has `data-testid="layout"`; the others have `layout-retained` and `hidden`.
   - In-module deep link = live focus, as before. A sync hold (`focusHold`) keeps a history link visible until the service route resumes or the user switches tabs.
   - A cross-module link, or one from Integrated, opens in the separate `link` dock, so neither Integrated nor any module workspace is replaced.
   - `#/l/<other>` re-applies a layout; `#/l/<same>` resumes it.
   - Module reconcile uses byDevice dedupe, so a CAN Trace/RxTx view picked in the panel is not added a second time.
   - A `loading` guard stops reconcile while `load()` mounts panels. Without it the shell's freshness emits caused duplicates.
5. **Seed → bound handoff.** When the service's first own bound panel appears, it moves into the seed's pane, is activated if the seed was active, and the seed is removed. After the user closes a seed, heartbeats do not re-seed it (`seedManaged`); the empty module state offers an explicit "작업대 다시 열기" button. When the last channel closes, the app is offered again; the retained-history views stay.
6. **Async completions are scoped to their dock** (ROOT review item). In `DockHost.scopedUi.openPanel`:
   - Front module dock: an accepted slot (UDS accepts uds, CAN and ETH) opens in place with no hash change.
   - Hidden dock: it opens in that hidden dock, never in the dock in front, and never navigates. A foreign domain is refused with a toast.
   - `holdUntilKnown` gives a requested view 10 s before it can be pruned as an unknown channel, so the B/C `ctx.ui.openPanel` after Connect is robust to the order of the state event.
   - Integrated or link dock in front: the old hash deep-link behaviour, unchanged.
   - Same guard in shell code that awaits: `InterfaceConnect` submit (skips `showSlot` if the front dock changed) and UART `openAndPlace` (opens into the UART dock captured at start).
7. **UART before any connection**:
   - The empty UART work area is now the REV6.1 pane picker (`UartEmptyPane`, port of opus `pickerHTML`). It lists saved sessions (click = the existing explicit `openAndPlace` through the gate), open channels without a saved session (view only), "새 세션…", "매크로" and split hints. It replaces the generic "활성 UART 인터페이스가 없습니다" card.
   - The sessions/macros side panel, toolbar (new session/macro, split, broadcast, import/export) and dialogs were already usable before a connection and are unchanged.
   - `TerminalRegistry` (`workbench.ts`): one channel can now have views in two retained docks. Lookups resolve to one preferred view (active, then visible, then newest), so macros and broadcast address the channel once. A running macro or broadcast recipient is dropped only when the **last** view closes.
8. **InterfaceConnect "MODULES"** now lists the seven module tabs as navigation only (`#/s/<svc>`): no RPC, no `showSlot`, no channel.

## Tests (direct rc)

| Command (cwd `ui/`) | rc | Result |
|---|---|---|
| `npx tsc -b --pretty false` | 0 | no output |
| `npx eslint src/shell src/workbench/uart src/panels/uart` | 0 | clean |
| `npx vitest run src/shell src/workbench/uart src/panels/uart` | 0 | 24 files, 235 tests passed |
| `npx vitest run src/store/schemaAdapters.test.tsx src/panels/{can,eth,psu,uds,logic,video}` | 0 | 19 files, 274 tests passed. Run once only: it is the one store test that imports shell, plus the frozen peer panels against the new shell. |

Logs: `/private/tmp/claude-501/-Users-ctmctm-workspace-Loom/6610301a-e9d2-42ed-8227-c8936edf2eb1/scratchpad/{vitest-final.log,tsc-final.log,eslint-a.log,vitest-a3.log}`

**New `ui/src/shell/moduleApps.test.tsx`** (8 tests, with probe panels):
- **Every tab before connection:** each of the 7 tabs is its unbound application with zero channels, zero RPC and Integrated left empty. After all tabs are visited there are 8 retained docks. This is the tab-vs-channel distinction.
- **Seed rule:** the projection rules for the seed, including UDS ignoring CAN/ETH observations.
- **Handoff, for each of ETH, CAN and PSU:** the seed hands off to the bound panel through the panel's `ctx.ui.openPanel` after Connect plus the state event:
  - no duplicate placeholder
  - same pane, bound view active
  - Integrated instances untouched
  - after close, the app is offered again
- **Delayed completion while hidden:** ETH and PSU completions after a switch to Integrated land in their own hidden docks. There is no navigation, the front dock is unchanged and a foreign domain is refused.
- **Draft kept:** switching CAN → ETH → UDS → Integrated → CAN keeps the same instance and unsent draft (no remount); a hidden dock has `hidden`; only one `[data-testid=layout]` exists.
- **Closed seed:** it is not re-seeded by heartbeats or arrivals, and the reopen button restores it.

**Existing tests I changed on purpose**, for the new user rule:
- `serviceSeparation.test.tsx`: active-only nav replaced by permanent tabs with `data-count` and aria. The logic tab is permanent and the seed returns after the capture stops. The UART empty state is the picker. UDS counts only real UDS. The integrated and actual-channel projection assertions are kept.
- `defaultLayout.test.tsx`: a deep link from Integrated goes to the link dock and returning resumes the same instances. The cross-module test asserts the retained UART split, an explicit saved layout applying in Integrated, the stored layout untouched, and no device RPC.
- `InterfaceConnect.test.tsx`: the seven launchers are navigation only.
- `workbench.test.ts`: added a `TerminalRegistry` test with two views.

## Fingerprint (sha256, first 16 hex)
```
55fb365026f26a15  ui/src/shell/routes.ts
bbcd12db1d2c25da  ui/src/shell/ServiceNav.tsx
d4a71ea1493ff338  ui/src/shell/TopBar.tsx
00f63a1207d04b41  ui/src/shell/ShellApp.tsx
8c8a16ab51e971ea  ui/src/shell/InterfaceConnect.tsx
a6419cfd660f866c  ui/src/shell/services.ts
048b298167b73ec0  ui/src/shell/shell.css
caed85d6f2957915  ui/src/shell/dock/DockLayout.tsx
b2eb77f4d5d58d44  ui/src/shell/dock/dockHost.ts
f3b16a721f59600c  ui/src/shell/dock/activeWorkspace.ts
acc5756317641bd1  ui/src/shell/serviceSeparation.test.tsx
61066b59af50de78  ui/src/shell/InterfaceConnect.test.tsx
840baed90576597b  ui/src/shell/dock/defaultLayout.test.tsx
2b5d637ae8f022da  ui/src/shell/moduleApps.test.tsx   (new)
37ff4741a4eac08a  ui/src/workbench/uart/UartWorkbench.tsx
fefac18ca90d38f1  ui/src/workbench/uart/workbench.ts
e88ccad928dea02b  ui/src/workbench/uart/bench.css
67314c531e8ed01e  ui/src/workbench/uart/workbench.test.ts
```
No other files touched. The temporary debug test was removed. No SDK or peer panel files were edited.

## Self-review / remaining issues

- **Handoff for ROOT to assign, outside my ownership:** `ui/e2e/workbench-ego/services.body.mjs` still encodes the old rules: "no inactive service navigation" (services.length === 0), "UART-only navigation", `wait(services.length===5)`, and the exact view count per service. These now fail by design: the tabs are permanent and the unbound app adds a tab on services without an open channel. `combinations.body.mjs` uses `[data-testid=layout]`, which still matches only the front dock. I did not edit `ui/e2e`.
- **Behaviour change to verify in native QA:** a deep link from Integrated (agent focus or ChannelBar view in another module) now shows in the separate link dock instead of replacing Integrated. Clicking 통합 returns to the untouched Integrated workspace.
- **Pre-connect drafts in the seed panel** (for example ETH/UDS form text) are disposed at the seed → bound handoff. The bound panel is the full app for the channel. Carrying drafts over would need panel-owned model persistence (B/C), not shell work.
- **Retained docks:** at most 9 docks × MAX_PANELS panels. Hidden docks keep their subscriptions (by design: REC, macros, drafts) but do not draw. They are created only on first visit and live for the shell's lifetime; there is no LRU eviction.
- **Hidden-dock prune grace:** a requested view whose channel never opens is pruned after 10 s at the next sync.
- **Header widths:** budgeted from estimated text widths, not measured. Please check visually at 1440×900 and 900 px wide, including the case where several services have counts.
- **Layout persistence across a page reload:** only the existing saved-layout mechanism. The per-module arrangements are kept in memory only, for the life of the shell.
