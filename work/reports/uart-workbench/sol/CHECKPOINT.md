# UART checkpoint

- State: WORKING, 2026-11-13, foreground direct implementation; no subagents.
- Scope: only `work/reports/uart-workbench/sol/`; one human-operated UART workbench. No product, git, old plan, server or browser mutations.
- Direction: “Relay / UART desk” — compact saved-session rail, independent tabbed terminals in a recursive resizable split tree, collapsible macro library + explicit target/run progress.
- Priorities: nested split + pointer resize, native tab move, session-local settings/input, macro CRUD + stop cancellation + namespaced persistence.
- Research: MobaXterm primary; Windows Terminal and WezTerm official documentation. Observe documents/images; distinguish inference.
- Verification: syntax and targeted DOM/state checks. Browser QA belongs to the existing owner per REQUEST.md; do not claim it here.
- Model/session: REQUEST assigns gpt-6.1-sol native foreground; runtime exact model/session ID is not exposed in this session. No substituted worker.

## Source freeze / return

- 2026-11-13 13:04:21 UTC: implementation frozen; syntax + 29 DOM/state checks passed. HANDOFF.md has selectors, geometry protocol, theme independence and explicit browser-QA gap.
- Shared shell steering applied: F2 Bench Light outside; UART alone inside. Default Dark output / per-connection Light selection. Existing plans and old files unchanged.
- Runtime confirmed by MAIN: GPT-6.1-Sol high Fast / 01a1d4f8-a02e-7300-bf2a-b99d05c6f10b.
- MAIN verified preview HTTP 200. No optional curl escalation retried. Browser QA remains Opus-owned.
- State: READY_FOR_REVIEW; no final acceptance claim. No further cosmetic source edits; native QA defects only.


## QA defect correction / refreeze

- 2026-11-13 13:10:04 UTC: native QA reported responsive rail persistence leak, child minima violations and desktop content-driven height growth. Fixed only these layout/persistence defects.
- State: READY_FOR_REVIEW again; 38 DOM/state checks + syntax pass. Native recheck pending; no browser used.
- New hashes: index 76095ffc / styles 69242e25 / app 33f49dc0. Full hashes and repro/recheck instructions in HANDOFF.md. No further source edits absent actual QA defects.
