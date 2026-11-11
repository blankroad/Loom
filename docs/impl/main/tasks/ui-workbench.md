# UI workbench / DONE

기준 ea49d60, main shared tree. Main alone owns git index/commit. Preserve existing dirty docs/untracked files. User requests benchmarking, plan, implement, real browser validation, commit; fast parallel Herdr foreground; Sol first, Opus fallback allowed. No nested/background agents, permission bypass, global skill install.

## Plan and acceptance
- Benchmark official VS Code UI/layout, Postman navigation, Saleae Logic navigation: grouped explorer, persistent target-specific tabs, quiet chrome with visible safety/status, useful empty states and progressive settings disclosure. Adapt patterns; no copied branding/assets.
- Workbench: calm dark surfaces, consistent compact controls/spacing, clear active state, keyboard focus, bounded responsive chrome, readable data density. Global STOP/rights/connection/recording remain visible and enforced.
- Multi-interface: searchable domain-grouped explorer with channel identity + explicit connection state; distinct UART/CAN/UDS/ETH/PSU interfaces open distinct stable panels. Repeated click focuses existing slot. Multiple UART terminal/trace panes keep channel-scoped state/input. No implicit connect/write on selection. Existing deep links/layout persistence remain compatible.
- Provide view choices for each domain, useful empty state and truthful unsupported states. Do not claim connection management if backend/open UI isn't implemented.
- Verify meaningful regression tests, full UI check/build, actual browser screenshots at 1440 and narrow widths. Actual simulator two UART channels, separate matching data and input targeting, switching/reopening; CAN/UDS/ETH and global STOP regression. Ego first, allowed Playwright fallback. Previous root Mach/XPC sandbox restriction documented; do not security-bypass it. Opus normal approvals previously worked, validate environment honestly.

## Ownership (disjoint shared files)
1. ui-shell / Herdr wC:p1 / Sol: ShellApp.tsx, TopBar.tsx, shell.css, FocusPrompt.tsx, ActivityStrip.tsx, own new presentational components/tests. NOT ChannelBar.tsx, store/channels.ts, dock/*, routes.ts. Put ChannelBar in aside inside `.workbench-body` next to `.workbench-main` containing DockLayout. Remove ChannelBar from sub-bar. Banners/StopPanel must remain visible. Other controls progressive disclosure allowed. Coordinate only through report; don't edit other ownership.
2. ui-interfaces / Herdr wD:p1 / Sol: ChannelBar.tsx, store/channels.ts, shell/dock/*, routes.ts and dedicated tests. NOT shell.css/ShellApp. New explorer.css allowed, import from ChannelBar. Implement functional grouped searchable explorer; actual distinct panel identity/title, multi-UART views and tests. No panel implementation changes until request ownership from main.
3. ui-qa / Herdr wE:p1 / Opus: read-only implementation review and actual browser baseline/final; owns new ui/e2e/workbench* files and docs/impl/main/reports/ui-workbench-browser.md + evidence/ui-workbench/*. No implementation edits without handoff. Start baseline now while others implement; final when main signals. Use ego-browser skill and tools; native normal approvals, no skip-permissions/security hacks. Own resources cleanup. Preserve original browser failures, no fake/mock as actual-domain PASS.
4. main: benchmark/plan/report, integration/review/fixes assigned after workers release, checks/commit, workspace lifecycle. Current scope limits backend changes to actual multi-interface bugs if found and owned.

## Reports
Each worker writes one report `docs/impl/main/reports/ui-workbench-{shell,interfaces,browser}.md`: READY_FOR_REVIEW with files, diff identity, commands/results, constraints and next action. Don't commit/push. No worker subagents. Don't overwrite others. Workspaces closed by main only after report/integration and normal agent exit. Time budget about 25 min per initial implementation, checkpoint if larger. User expects completion, not plan only.

## Sources
- https://code.visualstudio.com/docs/editing/getting-started/userinterface
- https://code.visualstudio.com/docs/configure/custom-layout
- https://learning.postman.com/docs/getting-started/basics/navigating-postman
- https://www.saleae.com/support/logic-software/viewing-and-analyzing-data/navigating-the-software

## Integration evidence
Source integrated after disjoint Sol shell/interfaces work and native-snapshot follow-up; main reviewed/fixed empty dock, UART metadata hydration and inactive autoopen. `pnpm --dir ui check`: schema33/types/lint/428tests PASS, build PASS. Embedded loomd/loom build and UI-assets11 PASS. Opus Ego strict final2 61/61 PASS + unmodified canonical Playwright5/5 PASS (CAN p95 82.9ms). Reports and evidence in ui-workbench paths. Own browser/server/process/control cleanup complete. wC/wD/wF normal Codex exit and wE normal Claude exit, module workspaces closed; final Herdr list only MAIN w8. Local commit owned by main, no push in this request.
