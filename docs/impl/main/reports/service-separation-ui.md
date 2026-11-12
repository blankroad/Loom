# Service separation UI — READY_FOR_REVIEW

## Owned implementation

- Added canonical `#/s/uart`, `#/s/can`, `#/s/eth`, `#/s/uds`, and `#/s/psu` routes. Each resolves the existing domain preset from actual OPEN aliases; legacy `#/l/...` layouts and `#/p/...` deep links remain available.
- Added service navigation with counts of OPEN aliases only and a distinct, always available `#/l/m1-default` integration entry. The service heading names its domain, hides the integration layout picker, and scopes the channel explorer to that domain. Empty direct service routes explain how to connect an interface.
- Service defaults open aliases in one tab group. Foreign channel arrivals do not add panels to a service route. `showSlot` and accepted agent focus requests route explicitly to a foreign deep link instead of inserting a foreign panel into the current service dock. Navigation itself sends no device RPC.
- Included service routes in bootstrap and remembered route allowlists.

## Verification

- `pnpm exec vitest run src/shell/serviceSeparation.test.tsx src/shell/dock/defaultLayout.test.tsx src/shell/channelExplorer.test.tsx src/shell/m1b.test.tsx` — 4 files, 37 tests passed.
- After the last test additions, `pnpm exec vitest run src/shell/serviceSeparation.test.tsx src/shell/m1b.test.tsx` — 2 files, 27 tests passed.
- `pnpm typecheck` — passed.
- `pnpm lint` — passed.
- `git diff --check` — passed.

## Limits and integration handoff

- This first stage retains the existing dock panel lifecycle and shared engine, state, and RPC. Switching service routes reloads the dock route; it does not preserve a panel instance across route changes. Existing control, STOP, rights, recording, focus and auto-open behavior remains shared.
- Main owns aggregate/build checks and actual daemon/browser QA. Source is frozen for review; no commit or index changes made here.

## UDS application-layer follow-up — READY_FOR_REVIEW

- Navigation now groups UART, CAN, Ethernet, and PSU as interface services, with active UDS under a separate 진단 group. CAN and Ethernet service screens link to an active UDS workspace; that workspace links back to active CAN and Ethernet services. Links only change routes.
- `#/s/uds` projects the existing UDS console and default raw CAN/Ethernet observation panels for **every actual OPEN alias**. Its explorer shows UDS, CAN, and Ethernet channels. Other service routes remain single-domain. The projection neither invents a transport binding nor pairs observation channels to an ECU.
- UDS is an application-layer diagnostic protocol that may use CAN-TP over CAN or DoIP over Ethernet. Hyundai-Kia EthDiag is a distinct Ethernet diagnostic transport similar to DoIP, not an alias for it. Current native UDS runner connection support is DoIP/simulation; this UI adds no EthDiag or CAN-TP connection control and starts no capture or device connection. The in-app context note states the current support and warns that observation panels do not imply ECU correspondence.
- Follow-up focused verification after the final context note and late CAN arrival assertion: `pnpm exec vitest run src/shell/serviceSeparation.test.tsx src/shell/dock/defaultLayout.test.tsx src/shell/channelExplorer.test.tsx src/shell/m1b.test.tsx --reporter=dot` — 4 files, 39 tests passed; `pnpm typecheck`, `pnpm lint`, and `git diff --check` passed. Source frozen for main's aggregate/build/browser integration.
