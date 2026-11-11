# UI workbench shell — READY_FOR_REVIEW

Ownership #1. No nested/background agents, no security changes, no git index/commit/push operations. Main owns integration and git. Existing unrelated changes preserved.

## Files and resulting behavior

- `ui/src/shell/ShellApp.tsx`: substantial two-column workbench structure; ChannelBar moved out of sub-bar into aside; workspace heading, layout toolbar, settings disclosure, dock surface, activity/status footer. Banners and StopPanel remain above workbench body. NeedAuth automation text now says rights depend on daemon policy.
- `ui/src/shell/TopBar.tsx`: identity mark, explicit server connection label, environment, LIVE, REC, rights and STOP. Narrow layouts wrap status rather than hiding safety information. Existing STOP actions, rights checks, recording values and connection logic remain intact.
- `ui/src/shell/FocusPrompt.tsx`: tab behavior settings behind native keyboard-accessible details/summary; existing setting handlers retained.
- `ui/src/shell/ActivityStrip.tsx`: structured activity footer with count, useful empty message and associated expandable list.
- `ui/src/shell/shell.css`: consistent native input/select/button/textarea styling across shell and panel controls; explicit focus rings; panel header/toolbar spacing; dock tab contrast/active accent; responsive desktop/768/390 rules; bounded STOP and banner areas; no vertical wrapping of button labels. No virtual-table row-height or cell-padding math changed.
- New `ui/src/shell/WorkbenchWelcome.tsx`: useful default fresh-session welcome with 1–2–3 instructions and existing main-owned InterfaceConnect button. Shown only when no known channels, route is m1-default, and no saved layout has that name. DockLayout stays mounted and attached underneath (visibility hidden + inert + aria-hidden); first known channel removes welcome. Explicit deep links, other layouts and saved layouts remain uncovered. No implicit connection/write on render or selection.
- New `ui/src/shell/workbenchShell.test.tsx`: seven shell regression cases.

## Interface worker contract

`<aside className="workbench-sidebar" aria-label="인터페이스 탐색기">` is directly inside `.workbench-body`, alongside `.workbench-main` containing DockLayout. Aside owns width/scroll/border; ChannelBar/explorer.css owns explorer internals. Shell CSS no longer styles `.channel-bar` / `.chan`; shell guide uses distinct `.sidebar-guide`. Desktop aside 256px, intermediate/768 208px; below 600px aside stacks above main with bounded height. No ChannelBar API change required. Imported InterfaceConnect without modifying its implementation or CSS. No edits to channels store, routes, dock files, explorer CSS or panel implementations.

## Checks

- PASS: `pnpm --dir ui exec vitest run src/shell/workbenchShell.test.tsx src/shell/m1Shell.test.tsx src/store/stopController.test.ts` — 3 files, 17 tests. Covers default welcome with dock retained, first channel reveal, deep links, saved layout exclusion, policy-based auth copy, visible watch rights/REC/connection, watch STOP RPC, disconnected HTTP STOP fallback, acknowledged latch panel, settings handlers, activity expansion and no implicit interface open.
- PASS: `pnpm --dir ui typecheck` — final run after CSS refinements.
- PASS: `git diff --check --` the five assigned existing source files.
- Full `pnpm --dir ui check` attempted once: blocked before schema execution by sandbox `listen EPERM` for tsx local IPC socket. Main canceled the normal approval request; no permission changes or retries. Per latest instruction, aggregate check/build belongs to main. This is an environment block, not a passing aggregate check.
- Actual browser final screenshots and real-domain validation are QA ownership #3; not claimed here. Read baseline report and addressed shell findings: welcome, 768 settings crowding, 390 button wrapping/overflow containment, focus rings and inactive tab contrast. Final browser verification at 1440/768/390 remains required.

## Diff identity (SHA-256)

- ShellApp.tsx: `551654a265bee19a3eea5d6cc59843f312d191b5f964b408b08e310566c3ca10`
- shell.css: `319626d40e230a1e7f35221df0c673d71eac451d89c4954ca6b4038075635c2e`
- workbenchShell.test.tsx: `9652c702b1e34a248f4c841c2140d1e63031f39e5b2b5c7c3725247fab5cc343`

## Next action / release

Ownership #1 released to main for integration. Run aggregate check/build under main's normal environment, then signal QA final browser pass. In particular verify narrow screen overflow and first UI connect removing welcome against the real daemon. No browser resources were created by this worker.
