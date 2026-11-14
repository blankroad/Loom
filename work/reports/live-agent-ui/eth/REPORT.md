# eth live agent/UI check — REPORT

Status: **READY_TO_INTEGRATE** · AGENT_PASS (phases 1–6) · ROOT FINAL_NATIVE_PASS (phase 7) · no UI_FAIL open · DONE pending ROOT integration
Worker: eth-live-check-opus (w1P:p1), native foreground Opus 5.5, no child agents.
Initial baseline (before fixes): shared isolated daemon `LOOM_HOME=/tmp/loom-uds-live-20261114/home`, 127.0.0.1:52575, pid 95239,
epoch `1794612979`, env `sim`. CLI `target/debug/loom` sha256 `d54e4134f10b84fe…a3689`. Daemon was not stopped
or restarted during that initial phase. Later fixes and runtime identities are recorded chronologically below.

## Scope kept
- sim backend only (`iface sim`, `--backend sim`). No real NIC, no `send udp`. In sim mode `send udp` still
  binds a real OS UDP socket (`worker/source.rs` execute → `UdpSocket::bind`), so only `send raw` was used, which
  echoes in-process.
- Own channel only: `qa_eth_live`. Distinct identities: producer `LOOM_CLIENT=eth-producer-opus`, observer
  `LOOM_CLIENT=eth-observer-opus`.
- Bounded: a single 15 s `events --follow --timeout 15 --max 200`, no renew, no indefinite poll.

## Steps / evidence (raw/)
1. CLI help first: `loom eth help --json` → `raw/eth-help.json`; `eth ifaces` shows only `sim`, `replay`.
2. `loom eth capture start sim --backend sim --ch qa_eth_live` → request `w18`, state live (`raw/capture-start.json`).
3. Agent phase (one bounded run):
   - observer child: `events --since now --channel qa_eth_live --follow --timeout 15 --json --max 200`. Each line
     is wrapped with a host receive time `recv_wall_ns` → `raw/phase1-observer.ndjson` (17 lines / 38 events).
   - producer: `send raw <ARP> --dry-run` → R3 (broadcast MAC on sim), nothing sent (`raw/phase1-dryrun.json`).
   - producer: `send raw <ARP> --idempotency-key eth-live-p1-arp1` → `request_id w22`, `write_state sent`
     (`raw/phase1-send.json`, host send wall time in `raw/phase1-send-wall.txt`).
   - after the run: `trace qa_eth_live --since -25s` → `raw/phase1-trace.json` (26 rows, gaps [], complete true).

## Correlation (observer saw it live)
| seq | kind | origin | detail |
|---|---|---|---|
| 359 / 364 | eth.tx (gateway) | agent | method eth.send requested → sent(final) |
| 361 → 363, 366 | eth.tx (eth.qa_eth_live) | agent | requested/accepted/sent, `observed_src_seq 35` |
| **365** | eth.pkt | device (`attributed_origin agent`) | ARP 62 B, `link{request_id:w22, confidence:exact, role:tx}`, `layers.loom.request_id w22`, `source_seq 35` |
| 349,354,368,376,…,431 | eth.pkt | device | native sim SOME/IP-SD OfferService 0x1234.1 ttl=3, every ~1.02 s |

- REF: `eth:qa_eth_live@seq:365` ↔ `w22`. tx `observed_src_seq 35` == pkt `_capture.source_seq 35`.
- Real-time: the observer got the w22 batch 121 ms after the host send call (event wall→recv 104–126 ms).
  Native offers arrived continuously at about 1 Hz, with event wall→recv 2–192 ms (the follow delivers in
  batches of about 1 s).
- Native device traffic (SOME/IP-SD offers, origin device) is what proves device receive. The ARP is Loom's own
  send, so per the skill it is not device-behaviour evidence. It is used only for tx↔pkt correlation.

## Notes / observations (not product bugs, nothing changed)
- My phase-1 ARP hex had 2 extra zero bytes in the sender fields. The decoder honestly showed
  `Who has 0.0.192.0? Tell 0.0.192.0`. That was my input error and I did not resend (no auto-retry).
  Corrected 60-byte frame for the UI phase is below.
- The envelope `origin` of the echoed own packet is `device`, while `attributed_origin`/`layers.loom.origin`
  is `agent`. The trace ORG column follows attribution. Recorded for the ROOT UI compare (the UI should show AGT).
- `journal_available:false` for the sim capture. Data stays queryable after stop (see below), but it lives in
  the daemon's store only, so do not rely on it surviving a daemon restart.

## Capture stopped (per ROOT: avoid unbounded recording)
`loom eth capture stop --ch qa_eth_live` → request `w23`, state stopped, total 80 / shown 80, unrecorded 0,
display_lost 0. Status: `qa_eth_live generation 1 state stopped`. Post-stop
`trace qa_eth_live --since seq:359 --until seq:367 -Y arp` still returns `eth:qa_eth_live@seq:365` with link w22
(`raw/post-stop-trace-arp.json`). Channel data is preserved.

## READY_FOR_UI — handoff (run only after ROOT BEGIN_UI)
Common env:
```
cd /Users/ctmctm/workspace/Loom
export LOOM_HOME=/tmp/loom-uds-live-20261114/home LOOM_NO_AUTOSTART=1
```
- **Channel:** `qa_eth_live` (same channel; restart → generation 2, sim iface `sim`)
- **Resume capture (producer identity):**
  `LOOM_CLIENT=eth-producer-opus target/debug/loom eth capture start sim --backend sim --ch qa_eth_live --json`
- **Observer (bounded, ≤30 s):**
  `LOOM_CLIENT=eth-observer-opus target/debug/loom events --since now --channel qa_eth_live --follow --timeout 25 --json --max 200 | python3 -I -u <stamp.py> raw/phase2-observer.ndjson`
  (stamp.py wraps each line as `{"recv_wall_ns", "line"}`; the copy used in phase 1 is at
  `/private/tmp/claude-501/-Users-ctmctm-workspace-Loom/fddaa0aa-19a8-4589-bf5f-da21d564acf8/scratchpad/stamp.py`)
- **Producer (dry-run, then one send):**
  `LOOM_CLIENT=eth-producer-opus target/debug/loom eth send raw ffffffffffff02000000000108060001080006040001020000000001c0000201000000000000c0000202000000000000000000000000000000000000 --ch qa_eth_live --dry-run --json`
  then the same command without `--dry-run`, with `--idempotency-key eth-live-p2-arp1`.
  Expected decode: `Who has 192.0.2.2? Tell 192.0.2.1`.
- **UI compare:** in the native UI, panel eth channel `qa_eth_live`. Compare epoch `1794612979`, seq of the eth.pkt
  ARP and the SOME/IP-SD offers, request_id of the send, and exact bytes. Also the ORG column (expect AGT on the ARP).
- **Teardown after UI acceptance:** `LOOM_CLIENT=eth-producer-opus target/debug/loom eth capture stop --ch qa_eth_live --json`

Hashes: `raw/SHA256SUMS`.

## Phase 2 — cross-consumer (BEGIN_UI) · observer AGENT_PASS · UI verdict pending ROOT
- Capture restarted on the same channel: `qa_eth_live` generation 2, request `w26`, viewers 1/1 visible (`raw/phase2-capture-start.json`).
- Ready file: `raw/phase2-ready.json`. Observer `eth-observer-opus`, 25 s bounded, start wall `1794614096194671000`.
  It exited cleanly (exit 0/0, `raw/phase2-observer-exit.txt`) with no renew.
- ROOT sent the ARP (client `eth-ui-root#10879`), `request_id w27`. The observer got 58 events
  (`raw/phase2-observer.ndjson`): 26 eth.pkt, 25 eth.stats, 7 eth.tx.
  - seq 650/651/655 gateway eth.tx w27 requested/accepted/sent(final). 652/653/654/657 channel eth.tx w27,
    `observed_src_seq 109`.
  - **seq 656** eth.pkt ARP `Who has 192.0.2.2? Tell 192.0.2.1`, 60 B, `link{request_id:w27, confidence:exact}`,
    `layers.loom{origin:agent, request_id:w27}`, `source_seq 109`. Bytes are identical to the producer hex
    `ffffffffffff02000000000108060001080006040001020000000001c0000201000000000000c0000202000000000000000000000000000000000000`.
    Event wall→observer recv was 20.9 ms.
  - 25 native SOME/IP-SD offers (origin device), seq 606…708, at about 1 Hz.
- Post-window trace seq:600..710: 28 rows, gaps [], complete. ARP REF `eth:qa_eth_live@seq:656` (origin device /
  attributed agent) (`raw/phase2-trace.json`).
- UI side: compare with ROOT's native DOM for epoch 1794612979, seq 656, w27, the bytes above and offer seqs.
  UI PASS/FAIL is ROOT's verdict.
- ROOT UI confirmation: the native DOM row `data-seq=656` shows ARP, 60 B, 192.0.2.1→192.0.2.2, ORG AGT. This is the same
  backend at viewers 1/1, and it matches the observer evidence for w27, seq 656 and the bytes. ROOT will supply the
  DOM/PNG paths after saving them. Not written here yet.
- Capture stopped at ROOT's request: `eth capture stop --ch qa_eth_live` → `w28`, state stopped, generation 2,
  total 81 / shown 81, unrecorded 0, display_lost 0 (`raw/phase2-capture-stop.json`, `raw/phase2-status-after-stop.json`).
  Post-stop `trace --since seq:650 --until seq:658 -Y arp` still returns `eth:qa_eth_live@seq:656` w27
  (`raw/phase2-post-stop-trace-arp.json`).
- No extra sends, repeats or restarts. The worker sent nothing in phase 2. The only send was ROOT's w27.

## Phase 3 — new isolated daemon with fixed UI (p1 bundle) · AGENT_PASS
- Backend: `LOOM_HOME=/tmp/loom-uart-secret-native/home`, 127.0.0.1:53814, pid 17384, epoch 1794614502, viewers 1/1.
  It serves `--ui-dir ui/dist`. The old 52575 gateway embeds the old bundle (index-CITTW32i).
- CLI used for phases 1–3 (capture, observer, correlation): `target/debug/loom` sha256 d54e4134…a3689. ROOT later
  rebuilt the CLI as 22077e2af3e6bd448efc87561bffe299f384d79905f66b069f9a127a90ab824a (embedded docs/schema only, same event-follow
  code). The next phase must record which binary it used.
- Channel `qa_eth_fixed` gen1 started as `w4` (`raw/phase3-capture-start.json`). Observer `eth-observer-opus` 30 s, exit 0/0. Ready file
  written after the initial (empty) batch: `raw/phase3-ready.json`.
- ROOT sent once (client `eth-ui-root-phase3#20599`) → `w5` (`raw/phase3-root-send.json`). The observer got 68 events
  (`raw/phase3-observer.ndjson`): eth.tx w5 at seq 163–169, `observed_src_seq 39`. **eth.pkt seq 170** ARP `Who has 192.0.2.3? Tell 192.0.2.1`,
  60 B, `link{request_id:w5, confidence:exact}`, attributed agent, `source_seq 39`. Bytes are identical to the sent hex. Event→recv 58 ms.
  30 native SOME/IP-SD offers (origin device), seq 132…258.
- ROOT saved and inspected `phase3-live.png` (live device rows). The ARP could not be reached in the UI because of the filter/pause
  cursor bug (second defect, see PLAN). After the fix, the live RPC `trace --until seq:308 -Y arp` returns `eth:qa_eth_fixed@seq:170`
  (`raw/phase3-rpc-seqcursor-check.json`).
- Capture `qa_eth_fixed` is kept LIVE until ROOT finishes the final filter + stop acceptance on the p2 bundle. Stop command:
  `LOOM_HOME=/tmp/loom-uart-secret-native/home LOOM_NO_AUTOSTART=1 LOOM_CLIENT=eth-producer-opus target/debug/loom eth capture stop --ch qa_eth_fixed --json`

## Phase 4 — p2 bundle (index-zZUL1YL_.js) on 53814 · AGENT_PASS · UI live/filter/pause PASS (ROOT)
- CLI for phase 4 (observer, stop, trace): `target/debug/loom` sha256 22077e2a…824a (ROOT's rebuilt CLI). Phases 1–3 used d54e4134…
- Observer `eth-observer-opus` 30 s, exit 0/0. Ready file written after the initial (empty) batch: `raw/phase4-ready.json`, start wall 1794615062272874000.
- ROOT sent once (client `eth-ui-root-final#26113`) → **w6** (`raw/phase4-root-send.json`). The observer got 68 events (`raw/phase4-observer.ndjson`):
  | seq | event | detail |
  |---|---|---|
  | 835/836/840 | eth.tx gateway | w6 requested/accepted/sent |
  | 837/838/839/842 | eth.tx eth.qa_eth_fixed | w6 requested/accepted/sent, `observed_src_seq 335` |
  | **841** | eth.pkt ARP | `Who has 192.0.2.4? Tell 192.0.2.1`, 60 B, `link{request_id:w6, confidence:exact}`, attributed agent, `source_seq 335` |
  | 785…912 (30) | eth.pkt SOME/IP-SD | native device OfferService 0x1234.1 at about 1 Hz, origin device |
  Bytes of 841 = sent hex `ffffffffffff02000000000108060001080006040001020000000001c0000201000000000000c0000204000000000000000000000000000000000000`.
  Event→observer recv about 195 ms (one follow batch).
- ROOT native UI (watch, same daemon/epoch): live row 841 w6 192.0.2.4 with exact 60 B hex (`raw/phase4-live-arp.png`, `raw/phase4-live-dom-rows.json`).
  Device offers 797..847 were visible together with it. The `arp` filter gave the complete range, 1 row 841, with detail bytes (`raw/phase4-filter-detail.png/.txt`).
  Pause had no query error and selection 841 stayed. Both cursor fix and filter were confirmed on native UI.
- Normal stop at ROOT's request: `w7`, state stopped, gen1, total 370 / shown 370, unrecorded 0, display_lost 0, viewers 2/2
  (`raw/phase4-capture-stop.json`, `raw/phase4-status-after-stop.json`). Post-stop `trace --since seq:830 --until seq:845 -Y arp` →
  `eth:qa_eth_fixed@seq:841` w6, gaps [], complete (`raw/phase4-post-stop-trace-arp.json`).
- Pending (ROOT): the panel stays open after the stop with row/selection/detail, then close → inactive explorer → reopen `qa_eth_fixed (닫힘)`.
  Workspace is kept open. No further sends or captures from ETH.

## Phase 4 follow-up — ROOT correction
- ROOT's FINAL_UI_PASS was premature. After the UART owner closed the last live channel `qa_uart_fixed` at 09:12:18, ShellApp's empty welcome
  covered the reopened ETH view (`raw/phase4-reopen.json/png`: retainedTab/selected841/detail841=false). Stop-retain itself had passed
  (`raw/phase4-after-stop.txt/png`).
- Third fix: see PLAN "Third UI defect". p3 bundle `index-BzEw2Iyf.js` (2244b047…). No new send or capture is needed: 841 is still queryable on 53814.
- ROOT check on p3 (route `#/l/m1-default`, both channels closed): reopen `qa_eth_fixed (닫힘)` from the inactive explorer, check that the view stays
  visible and not inert with no welcome, apply `arp` → 841, select it, and confirm detail bytes survive heartbeats.

## Phase 5 (ROOT, p3) and source freeze
- ROOT native PASS on p3 `index-BzEw2Iyf` (`raw/phase5-both-closed-reopen.json/png`): both channels closed, inactive reopen, arp → 841 with
  detail bytes, still there after a heartbeat (welcome=false, ETH tab present, selection/detail 841, no query error). An explicit close
  brings the welcome back.
- ROOT found that seq 841's relative time differs: 241.354517 (phase 4 live) vs 331.534763 (phase 5). The truth is 331.534763, from observer
  mono 331539992667 − started_mono_ns 5229250. Cause and fix: PLAN "Fourth defect". This needs ROOT's gateway and CLI rebuild.
- All patches (UI + `crates/loom-eth` gateway + skill): `ui-stop-retain.patch` sha256 e3438d48e66310d8a42ba0cdae09f3a438c6eb77792615b259dcedc0dccb43fd.
  Files: crates/loom-eth/src/gateway/{common,evidence}.rs, crates/loom-eth/tests/gateway.rs, ui/src/panels/eth/{ethPanel,ethModel,eth.test}.ts,
  ui/src/shell/ShellApp.tsx, ui/src/shell/historyWelcome.test.tsx (new), ui/src/shell/dock/{activeWorkspace,activeWorkspace.test}.ts,
  ui/src/shell/dock/defaultLayout.test.tsx, skills/loom-eth/SKILL.md, skills/loom-eth/references/live-observer.md (new; the manifest entry in
  crates/loom-cli/src/skills.rs is done by ROOT).
- Final native fixture (ROOT): new home/port pending. ETH will start one capture and one bounded observer only on ROOT BEGIN, with no own sends.

## Phase 6 — final default gateway 8e4cc56b… (loomd), CLI 2b99f097…, bundled p4 `index-DCXMffOL`, fixture 54790 · AGENT_PASS
- Fixture `LOOM_HOME=/tmp/loom-eth-anchor-final/home`, pid 34236, epoch 1794615752, viewers 1/1 (no --ui-dir).
- Capture `qa_eth_final` started as `w1` gen1, **started_mono_ns 421959** (`raw/phase6-capture-start.json`).
- Observer `eth-observer-opus` 30 s, exit 0/0 (`raw/phase6-ready.json`, `raw/phase6-observer.ndjson`, 68 events).
- ROOT sent once (client `eth-ui-root-phase6#34944`) → **w2**. eth.tx at seq 76–82, `observed_src_seq 29`. **eth.pkt seq 83** ARP
  `Who has 192.0.2.5? Tell 192.0.2.1`, 60 B, `link{w2, exact}`, `source_seq 29`, bytes identical, event→recv 4.8 ms.
  30 native SOME/IP-SD offers (seq 86…203).
- Time anchor ground truth (`raw/phase6-expected-time.json`): seq 83 mono 25854522000 (clock proc-34529) − 421959 = **25.854100**.
  This must be the UI value for seq 83 at every step: live, reload late join, stop and reopen.
- Live UI compare (`raw/phase6-live-time-compare.json`): ROOT's `phase6-live-dom.json` offer rows 155…195 (11 rows) equal
  `mono − started_mono_ns` exactly, 11/11. The DOM sample does not include seq 83.
- Pending ROOT: seq 83 time after reload (late join, snapshot anchor) and after stop/reopen, then SCREENSHOT_DONE → ETH stops `qa_eth_final`.

## Phase 6 follow-up — header stale after stop
- ROOT: time math PASS natively (seq 83 = 25.854100 before and after reload; `raw/phase6-root-time-compare.json`, `phase6-arp-detail.png`).
  ROOT stopped `qa_eth_final` (w3, `raw/phase6-capture-stop.json`). All channels at zero active, panel retained. New defect: header `LIVE` +
  stop enabled after the stop.
- Fixed in p5 `index-CxluPpWw` (see PLAN "Fifth defect"). Touched shared SDK file: `ui/src/sdk/context.ts` (PanelContext interface +
  createPanelContext implementation + ContextDeps net Pick). ETH: `ui/src/panels/eth/ethPanel.ts`, `eth.test.ts`.
- Patch (all ETH work): `ui-stop-retain.patch` sha256 713bf4552ec27e5868303413dc443b9e3fd14b86c535917bc265340dd708bb6e. 11 files.
- ETH started no capture and sent nothing. No shell is left running.

## FINAL — READY_TO_INTEGRATE (ROOT FINAL_NATIVE_PASS)
- Final native: loomd c56f59ea003e6f8c0c88e274b690af8e3cd29666406cfdc45d9956b70e793772, CLI 539da6219841bcfb12e4d5a999d295a612db6628421e1313acd23b5563501ede
  (docs-only UART skill update). Clean home `/tmp/loom-eth-control-final/home`, port 55104, bundled p5 `index-CxluPpWw`.
- Phase 7 (ROOT, `qa_eth_control`): start w1 → `LIVE`, stop enabled. Stop w2 → `STOPPED` within 5 s, stop disabled. With zero live channels, no
  welcome and the tab is retained with 11 rows. PNG inspected (`raw/phase7-live.json`, `raw/phase7-stopped.json/.png`, capture-start/stop). No extra send.
- Phase 6 time base: seq 83 = 25.854100 live / reload / stop / reopen (`raw/phase6-final-ui.json` timePass/reopenPass true). 11 DEV rows
  match the agent observer.
- Gates summary:
  | Gate | Result |
  |---|---|
  | Agent real-time receive (native sim device offers + own-send correlation) | PASS phases 1–6 |
  | Native UI live row / exact bytes / ORG AGT | PASS (p2+) |
  | Display filter + pause + export cursor (`seq:N`) | PASS |
  | Stop retains panel/row/selection/detail | PASS |
  | Zero-active reopen from inactive (no welcome over history) | PASS |
  | Relative time = capture start anchor (late join / reload / reopen) | PASS |
  | Header/stop button follow live capture state after stop | PASS |
- Integration set (source review accepted by ROOT): `ui-stop-retain.patch` sha256 713bf4552ec27e5868303413dc443b9e3fd14b86c535917bc265340dd708bb6e
  - Rust: `crates/loom-eth/src/gateway/{common,evidence}.rs`, `crates/loom-eth/tests/gateway.rs`
  - UI shared: `ui/src/sdk/context.ts`, `ui/src/shell/ShellApp.tsx`, `ui/src/shell/dock/activeWorkspace.ts` (+tests
    `activeWorkspace.test.ts`, `defaultLayout.test.tsx`, new `ui/src/shell/historyWelcome.test.tsx`)
  - UI ETH: `ui/src/panels/eth/{ethPanel,ethModel,eth.test}.ts`
  - Skill: `skills/loom-eth/SKILL.md`, new `skills/loom-eth/references/live-observer.md` (manifest entry in `crates/loom-cli/src/skills.rs` done by ROOT)
- Test evidence: loom-eth gateway 13/13 + clippy `-D warnings` + fmt clean (cache /tmp/loom-uart-secret-build). UI `tsc -b`, eslint, vitest
  51 files / 561 tests. Every new regression was mutation-checked (fails on HEAD/mutant, passes fixed). Build logs: `ui-build*.log`, `ui-dist-p*.sha256`.
- Runtime cleanup is owned by ROOT. ETH leaves the fixtures, captures (all stopped) and daemons as they are. No shell or background process remains.
