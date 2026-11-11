# Active interfaces ownership1 — READY_FOR_REVIEW

Source frozen for main integration. No subagents, commit/push, permission changes, security escalation or aggregate/fullcheck performed.

## Implementation

- `state === 'open'` is the sole device eligibility predicate; closed/unknown history never supplies automatic workspace panes or picker options.
- Preset definitions contain no invented device aliases. `presetForChannels` resolves actual OPEN aliases, including multiple devices per domain. `activePresets` exposes only eligible choices. Added CAN/UDS/PSU single-domain presets; ETH is labelled `Ethernet 캡처`. Direct legacy CAN·UDS routes project whichever requested domains are available, without adding unrelated domains.
- DockLayout subscribes to shell and test-session changes, covering snapshots, late joins, open/close/reopen and epoch invalidation emitted by main's shell store. It incrementally reconciles device panels rather than reloading the dock. Existing panel objects, selected tabs, trace contents, input and group geometry survive unrelated updates. A user-closed view is not recreated on heartbeats; an existing alternate view can represent its device.
- Explicit panel deep links remain single-target diagnostic/history views and suppress automatic unrelated device additions. The explicit test route may show the test screen without an existing session, and never adds PSU. The test picker requires a known session.
- Saved layouts are projected into a separate live layout. Unavailable device leaves and individual unavailable UDS targets are filtered; empty groups collapse, surviving selection/relative weights remain. Source objects and stored/vendor data are untouched. Reopening restores available source panes incrementally, beside existing tabs, without resetting unaffected panels. The stored original geometry remains available on route reload.
- Managed docks own automatic lifecycle; standalone dock/service compatibility remains. No-dock duplicate/focus detection resolves actual current presets. Empty snapshots no longer invent a demo panel.

## Exact files

- ui/src/shell/routes.ts
- ui/src/shell/services.ts
- ui/src/shell/dock/DockLayout.tsx
- ui/src/shell/dock/dockHost.ts
- ui/src/shell/dock/layoutModel.ts
- ui/src/shell/dock/activeWorkspace.ts (new)
- ui/src/shell/dock/activeWorkspace.test.ts (new)
- ui/src/shell/dock/defaultLayout.test.tsx
- ui/src/shell/dock/dock.test.ts
- docs/impl/main/reports/ui-active-interfaces.md (this report)

Main-owned ChannelBar/ShellApp/explorer.css/channelExplorer.test.tsx/workbenchShell.test.tsx/m1b.test.tsx were not edited.

## Verification

- PASS: focused Vitest, 4 files / 59 tests: activeWorkspace.test.ts, dock.test.ts, defaultLayout.test.tsx, interfaces.test.ts.
- Includes all 31 nonempty UART/CAN/ETH/UDS/PSU subsets with two actual aliases per domain plus closed/unknown histories, eligible single/combo choices, direct-route filtering, saved-layout projection/reopen and partial UDS targets.
- Includes real UART panel integration for silent snapshots, open/close/reopen, reactive picker options, retained panel identity/current trace/DOM input focus and draft value, user-closed tabs, deep-link isolation, no device commands, and existing per-channel input sequence tests.
- PASS: `pnpm --dir ui typecheck`.
- PASS: targeted ESLint of all changed TypeScript/TSX files.
- PASS: `git diff --check` scoped to owned source files.
- jsdom emits its existing missing-canvas-context notices; tests pass. Actual browser combinations and aggregate/build are pending Opus/main, not claimed here.

## Limits / integration notes

- Live auto-generated views remain bounded at MAX_PANELS (32); explorer state retains all known channels. The limit toast appears once per route entry.
- Stored unavailable panes remain in their original saved entry. Explicit save continues to create a separate name through the existing backend, never overwriting that entry.
- Production PRESETS now contain metadata and empty slots; code/tests requiring devices must call presetForChannels/activePresets or use explicit fixture slots. Legacy fixed-device assumptions must not be restored.

## Review follow-up — auto-open preference / refrozen

READY_FOR_REVIEW; source refrozen after the narrow review fix.

- Modified only DockLayout.tsx, dockHost.ts and defaultLayout.test.tsx for this follow-up, plus this report. Main's services.ts no-dock fallback correction is preserved.
- Each live reconciliation reads the current `settings.autoOpenPanels`. Inactive device panes are always pruned; automatic additions occur only while the setting is on. Existing unaffected instances and active view are retained. Arrivals observed while off are tracked, so enabling on and receiving a heartbeat does not retrospectively open them. A later new arrival or close/reopen while on uses normal automatic opening.
- Explicit route initialization and `showSlot` continue opening requested views while auto-open is off. The user's setting is never changed by layout code.
- Regression covers live and silent-snapshot arrivals while off remaining in channel/explorer state without dock additions; pruning and reopening while off; unchanged unaffected terminal/trace instances; explicit showSlot/layout selection while off; re-enabling on and opening a new channel; existing on close/reopen coverage remains passing.
- Ineligible legacy combo routes with an actual available CAN pane now show `(현재 연결에 맞춰 표시)` in the picker; only an absence of OPEN channels shows `(연결된 인터페이스 없음)`. Added regression for CAN-only on a can-uds direct route.
- PASS: focused 4 dock test files / 59 tests, targeted ESLint on the 3 follow-up TypeScript/TSX files, typecheck, and scoped diff whitespace check. No fullcheck, browser run, commit, escalation or edits to main-owned files.
