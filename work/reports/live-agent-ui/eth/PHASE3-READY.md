# ETH phase 3 — READY (not started)

Purpose: on the fixed UI, prove in the browser that a normal capture stop keeps the ETH panel row, selection and detail,
and that an explicit reopen from the inactive explorer shows retained history. Same fresh capture/frame, seen by an
independent observer at the same time. No auto restart, no renew, one send.

## UI candidate (built, not deployed by me)
- Source: `ui-stop-retain.patch` (sha256 756f7af5c04d0062bb7f819c185117febc32b4225d208bfa1c939f406024be8b), fix in `ui/src/shell/dock/activeWorkspace.ts`.
- Built with `pnpm build` (tsc -b + vite build, existing node_modules), rc 0 → `/Users/ctmctm/workspace/Loom/ui/dist`. Log: `ui-build.log`.
- Asset hashes: `ui-dist.sha256` (index.html 9c97a2f1…, assets/index-l1ex_1Dk.js 81efb7ec…, index-CtDP4cbS.css 27ea090f…,
  net.worker-BleoIIWk.js 98ccabf8…). Bundle contains `new Set(["eth"])` + `state==="closed"&&` in availableSlot.
- Not loaded in ROOT's browser by me. ROOT runs its own isolated backend with `--ui-dir /Users/ctmctm/workspace/Loom/ui/dist` and reloads.

## Needed from ROOT at BEGIN_PHASE3
`LOOM_HOME` and port of the backend under test. Phase 3 runs on that backend (default: the current shared
`/tmp/loom-uds-live-20261114/home`, if ROOT keeps it). The backend must have the eth worker.

## Sequence (ETH worker steps marked W, ROOT steps marked R)
1. W: `LOOM_CLIENT=eth-producer-opus loom eth capture start sim --backend sim --ch qa_eth_live --json`
   (ROOT decision: same channel `qa_eth_live`, expected generation 3; no new alias).
2. W: start an independent bounded observer (≤ 25 s, no renew). It writes `raw/phase3-ready.json` with start/expiry wall times:
   `LOOM_CLIENT=eth-observer-opus loom events --since now --channel qa_eth_live --follow --timeout 25 --json --max 200` → `raw/phase3-observer.ndjson` (recv_wall_ns per line).
3. R: open the ETH panel (`#/s/eth` or `#/p/eth/packets?channel=qa_eth_live`) and send exactly one fresh frame (distinct target 192.0.2.3 → new bytes):
   `LOOM_CLIENT=<root-id> loom eth send raw ffffffffffff02000000000108060001080006040001020000000001c0000201000000000000c0000203000000000000000000000000000000000000 --ch qa_eth_live --idempotency-key eth-live-p3-arp1 --json`
   Expected decode `Who has 192.0.2.3? Tell 192.0.2.1`, 60 B, ORG AGT. Select the row, open detail, then save the DOM/PNG (live).
4. W (on ROOT signal, after the live PNG): `LOOM_CLIENT=eth-producer-opus loom eth capture stop --ch qa_eth_live --json`.
5. R: verify the panel stays open after stop, and the same row, selection and detail remain (PNG #2). Close the panel, toggle
   `비활성 인터페이스 N개 보기`, choose ETH `qa_eth_live (닫힘)`, then check the row and detail again (PNG #3). Also try once after a tick or heartbeat.
6. W: correlate the observer NDJSON ↔ the send request_id, eth.tx observed_src_seq ↔ eth.pkt source_seq, exact bytes ↔ ROOT DOM data-seq. Post-stop `trace -Y arp`.
   Then report AGENT_PASS / UI_PASS|UI_FAIL. DONE only after ROOT's saved PNGs are inspected.

## ROOT gating (latest)
Wait for ROOT's next BEGIN before steps 1–2. After the observer starts, return READY_UI. No sends and no stop until ROOT has captured the PNG.

## ROOT correction (supersedes the channel/backend above)
- The old 52575 gateway embeds the old bundle (index-CITTW32i). Phase 3 runs on the NEW isolated daemon instead:
  `LOOM_HOME=/tmp/loom-uart-secret-native/home`, 127.0.0.1:53814, pid 17384, epoch 1794614502. It serves the current
  `ui/dist` via --ui-dir. Owned by ROOT/UART; do not stop it.
- Channel: `qa_eth_fixed` (ETH-owned). Capture started: `LOOM_CLIENT=eth-producer-opus target/debug/loom eth capture start sim --backend sim --ch qa_eth_fixed --json`
  → request `w4`, generation 1, state live, viewers 1/1 (`raw/phase3-capture-start.json`).
- Observer not started yet: it waits for ROOT BEGIN. Then:
  `LOOM_HOME=/tmp/loom-uart-secret-native/home LOOM_NO_AUTOSTART=1 LOOM_CLIENT=eth-observer-opus target/debug/loom events --since now --channel qa_eth_fixed --follow --timeout 25 --json --max 200` → `raw/phase3-observer.ndjson`, `raw/phase3-ready.json`.
- No marker send by ETH. Stop only after ROOT's screenshot and signal:
  `LOOM_HOME=/tmp/loom-uart-secret-native/home LOOM_NO_AUTOSTART=1 LOOM_CLIENT=eth-producer-opus target/debug/loom eth capture stop --ch qa_eth_fixed --json`
