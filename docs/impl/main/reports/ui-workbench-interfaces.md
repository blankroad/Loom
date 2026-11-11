# UI workbench interfaces — READY_FOR_REVIEW

Ownership #2 is complete; source is frozen for main integration and browser QA. Base: `ea49d60`. No commits, nested agents, panel implementations, shell.css/ShellApp edits, or security changes were made by this worker.

## Files / diff identity

- `ui/src/shell/ChannelBar.tsx`
- `ui/src/shell/explorer.css`
- `ui/src/store/channels.ts`
- `ui/src/store/channels.test.ts`
- `ui/src/shell/channelExplorer.test.tsx`
- `ui/src/shell/routes.ts`
- `ui/src/shell/m1b.test.tsx`
- `ui/src/shell/dock/DockLayout.tsx`
- `ui/src/shell/dock/dockHost.ts`
- `ui/src/shell/dock/dock.test.ts`
- `ui/src/shell/dock/interfaces.test.ts`
- `ui/src/shell/dock/defaultLayout.test.tsx`

Owned source content SHA-256: `e955bf596e636cd3d89e37fea3ad61b8c052f84b75f7d7cd44ade37e47fb883c` (sorted paths followed by NUL, file bytes, NUL). Report excluded. All changes are uncommitted in the shared tree. Other workers' changes and existing dirty files are preserved.

## Behavior

- Grouped, sortable, collapsible explorer with multi-term case-insensitive search over channel/domain/state, explicit open/closed/unknown labels, generation marks, empty results and clear-search action. Canonical `data-chan="domain/channel"` remains intact. Dedicated `explorer.css`; header hint is `.explorer-subtitle` to avoid shell footer leakage. Main-owned `InterfaceConnect` is imported and rendered beside explorer search.
- Implemented channel-scoped view choices: UART terminal and trace; CAN trace/send; UDS diagnostic console with `targets`; ETH packets/capture; PSU voltage/current. Unsupported domains remain visible with an honest unsupported-view message. Selection only opens/focuses a view; it does not connect or send traffic.
- Stable tab keys distinguish view and channel/configuration, exclude reveal cursor, and normalize CAN `ch`/`channel` aliases. Existing readable manifest titles include channel/target identity. Repeated selection focuses existing tabs without remounting. Closing then reopening creates fresh panel state with the same deterministic identity; switching preserves live panel/input state.
- Mounted dock bypasses the legacy `MAX_EXTRA_SLOTS=4` fallback: regression covers eleven simultaneous panels with no eviction, dedup, readable target titles, snapshot serialization/restoration and stable IDs. Fallback services.ts was not changed.
- Only `m1-default` initialization is dynamic: start empty or initialize from currently known supported channels, at most `MAX_PANELS=32`, with a truthful overflow toast. Explorer still lists all channels. No reload on store ticks/new channel events, preserving user tabs. Other presets, saved layouts and deep links retain behavior. Preset labels now read 기본 작업 공간 / UART 터미널 / CAN·UDS 분석 / 테스트 실행 / Ethernet·DoIP, with unchanged IDs.
- Native state parser handles UDS `{link:{state}}`, ETH `live`/`replay_done` and canonical `ch`, native reducer snapshot shapes, and PSU `connected` without confusing output-off with disconnection. Real UDS fixture JSON is read by regression tests; ETH/PSU cases follow native Rust payload/reducer structures. Missing status remains unknown. Main separately owns removal of ShellControlStore's unknown→open inference; m1b regression was updated for that intentional behavior and confirmed-connection transition.

## Checks

- `pnpm --dir ui typecheck` plus targeted ESLint on owned TS/TSX files: PASS before final small default-cap/preset-label changes.
- `pnpm --dir ui exec vitest run src/store/channels.test.ts src/shell/channelExplorer.test.tsx src/shell/dock src/shell/m1b.test.tsx`: PASS, 6 files / 50 tests. `/private/tmp/loom-ui-interfaces-targeted.log`.
- After final cap/label changes: `pnpm --dir ui exec vitest run src/shell/dock/defaultLayout.test.tsx`: PASS, 3 tests. Includes first alpha/beta connection with no ghost dut/ch0/bcm, stable rerenders and navigation, saved alias/view restoration, 33 known channels bounded to 32 views without hiding the 33rd channel. `/private/tmp/loom-ui-interfaces-default.log`.
- Actual UART implementations exercised through DockHost: four independent terminal/trace subscriptions, matching scoped data, hidden-tab catch-up, independent input_seq/channel targeting, close cleanup and fresh reopen. FakeNet is used for transport; this is not actual simulator/browser validation. jsdom prints its known missing-canvas diagnostic; tests pass.
- `pnpm --dir ui build`: PASS before final small cap/label changes; existing >500 kB chunk warning. `/private/tmp/loom-ui-interfaces-build.log`. Main will run final build after freeze.
- `git diff --check` on owned scope: PASS before final cap/label edits.
- Worker full `pnpm check` stopped at schema tool `tsx` IPC listen EPERM in sandbox. Main canceled the requested normal escalation; no further escalation or fullcheck attempted. Main reports aggregate schema33/type/lint/416 tests PASS before the final bounded-default regression was added. Final aggregate integration is owned by main.

## Constraints / next action

Main may now build and signal Opus final browser QA. This worker has not performed actual browser/simulator validation; use the browser worker report for that evidence.

Persistence is explicit Save layout → saved layout route/reload; automatic last-workspace restoration is not implemented or claimed. Navigating away from a route and back intentionally initializes that route again. Existing vendor layout state and target/view props remain restorable.

Late-join snapshot can arrive after empty default initialization while join grace suppresses auto-open. This intentionally does not reset user tabs: channels appear in the explorer, whose visible hint says “채널을 선택해 보기를 엽니다.” Selecting them opens their matching views. No automatic first-snapshot initializer was added. A larger centered empty-dock hint can be an integration refinement if main wants it.

The initial default is bounded to 32. DockHost.open itself retains its pre-existing behavior without an enforced interactive 32-panel cap; main owns any broader panel-limit UX decision. No channels are silently removed from the explorer.
