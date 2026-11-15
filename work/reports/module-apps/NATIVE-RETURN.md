# NATIVE-RETURN — Loom.app native acceptance (Opus lead)

**Final verdict: PASS. All requested native flows passed in the real visible Loom.app.** Every Connect/Start was a ROOT CUA click in the app; I did no CLI open/start. Each was then confirmed by a separate CLI observer on the same daemon.

## Identity (rechecked at start)
- App 23400 `desktop/dist/Loom.app`, daemon 23423 on 127.0.0.1:62968, home `~/.loom`, stream epoch 1794732461.
- loomd sha256 38219c57…; served `/ui` files 6/6 match the db441bfb set (`qa/native/raw-identity.json`).
- Viewer role `approve`; latch false throughout. No credentials minted, used in requests or persisted. One scan-only read is disclosed under Credential scan.
- Baseline seq 22: no channels open, no user events.

## Matrix
| Flow | Native UI action (UI client `ui-f6344db8e70f`, origin user) | Independent CLI receipt (observer armed first) | UI evidence | Verdict |
|---|---|---|---|---|
| UART1 | New session `qa-native-uart1` `/dev/ttys006` 115200 8N1 → 저장 후 열기. `uart.open` w1, seq 185–191, gen 1 | `QANUART1-A5E251`: TX w2 seq 218 → RX `ack:` seq 220, 4.8 ms; fixture rx.log has it | root-uart1-open, root-uart1-ack: search 1/1 at 20:27:11.922 = seq 220 wall | PASS |
| UART2 save-only | 저장 only | 0 uart2 events over 5 s (11:29:22Z / 11:29:28Z); `E_NOT_OPEN`; tty held only by the fixture | root-uart2-saved | PASS (no autoconnect) |
| UART2 split | 오른쪽 분할에 열기. `uart.open` w3, seq 404–408 | `QANUART2-9A8E3D`: w4 TX 440 → RX 443; reached the uart2 device only (uart1 rx.log 0) | root-uart2-split, root-uart2-ack: 1/1, both panes | PASS |
| CAN sim | `qa-native-can` sim, nodes default → 연결. `can.open` w5, seq 493–496 | `300#C4747F2718B7F316` w6 seq 748 → 0x301 exact copy seq 749, 5.0 ms | root-can-connected, root-can-ack | PASS |
| ETH sim | `qa-native-eth` NIC sim → start. `eth.capture.start` w7, seq 1184–1190 | UDP `QANETH-624BDB` w8 → eth.pkt seq 1380 (tx, Len 13); loopback sink got the exact 13 B | root-eth-open, root-eth-marker: `udp.dstport==65201`, seq 1380 bytes | PASS (own TX on sim tap, not NIC RX) |
| UDS / DoIP | Preset `qa-native-ecu` 127.0.0.1:64681, LA 0x1001, tester 0x0E80 → 연결. state seq 2286; fixture routing activation 0x10 | w9: 19 B VIN, raw OK, decoded absent (cause below). After the fixture-only fix, w10 seq 3664/3665 decoded **F190 VIN = WLMNATV2X2N202208** | root-uds-open, root-uds-response, root-uds-vin: 확정값 1 | PASS |
| PSU SCPI | `qa-native-psu` scpi-tcp 127.0.0.1:64682 → 연결. `psu.connect` w11, seq 4187–4192 | Read-only status/meas: OFF, 0.0 V / 0.0 A / 0.0 W. Fixture log has only MEAS?/OUTP? queries, 0 set/out commands | root-psu-open, root-psu-meas: 69 points | PASS (no ON/set/cycle) |
| Logic | synthetic `QANLOG-202208\r\n`, 1 MHz, UART D0 115200, 10 s → 캡처 시작. `logic.start` w12, seq 5309–5312, capture lc-1a1df00f399-1 | Real sigrok-cli PD: 1515 annotations = 101× marker, 8888 edges, 0 dropped | root-logic-start, root-logic-result, root-logic-full | PASS |
| Video | http-mjpeg 127.0.0.1:64684, 640×360@15 → open. `video.open` w13, seq 6850–6861, capture v4e0216eadb82 | Frame **1011** sha256 `af62715d…5596d` from CLI equals the UI value; burn-in F000675 = fixture producer_seq 675 | root-video-open, root-video-stable | PASS (exact 1011) |
| Integrated, 3 visible panes | UART1 + CAN trace + UDS | `QANINT2-B3FF78` w16 RX seq 10837; CAN `2EA4C7808A65A41B` w17 0x301 seq 10842 (effective_verified) | root-integrated-three, root-integrated-markers | PASS (native 3-pane; 8-pane is browser-only evidence, not claimed native) |
| Persistence | UART module ↔ Integrated switch | uart1/uart2 still gen 1, one `uart.open` each, no close/reopen | root-uart-return: 2 panes, both searches 1/1. root-integrated-return: CAN filter 0x301, QANINT2 search 1/1 | PASS |

## Findings (no production defect)
1. **UDS decoded VIN absent on the first read (w9).** My fixture's VIN was 19 B. The codec fixes F190 at 17 ASCII (`loom-uds-codec/src/did.rs:409`); `response.rs:290` returns LengthMismatch, so `decoded` is None and the UI shows that truthfully. This is correct standard behaviour.
   - Fixed in the fixture only: restarted on the same port with a 17 B VIN. The daemon auto-reconnected as gen 2 (down at seq 3457, up at 3463).
   - The first raw proof is kept.
2. **Video frame 246 cannot be matched by hash.** The 120-frame ring (about 8 s) had dropped it. Published video events and the journal carry no per-frame sha256, and the UI freeze is display-only (daemon `frozen:false`).
   - I paused the source with the fixture's stall control so the newest frame stayed fixed, which allowed the exact match on frame 1011.
3. **The stall closed the video capture.** Sequence: `stalled/no_frames` at +2 s (seq 8468), then FFmpeg exited 0 at +5 s, giving `closed/eof` with stopped_at_frame 1011 (seq 8537).
   - ROOT's source review: this is the documented design (manual reconnect, `rw_timeout` 5 s). Not a blocker.
   - The frozen UI frame stays shown truthfully after EOF.
4. **The shell "+ 추가" UDS form has no port field** (fixed 13400). The UDS 진단 preset form was used instead. Observation only.
5. **On the first open of each UART session, a toast says "…를 다시 열었습니다" ("reopened").** Wording observation only, not claimed as a defect.
6. **Fast typed alias entry yielded only "t1"; the alias was pasted instead.** Automation-entry fallback, not claimed as a product bug.
7. **My QA mistakes, no product impact:**
   - UART1 `expect --from now` was armed after the send and timed out; the ack was proven by observer seq 220.
   - CAN `--expect 301` was parsed as decimal, so the CLI reported `effect_unverified`; 0x301 was proven by seq 749.
   - One integrated marker pair (`QANINT1-9D7D9F` / `1E1F4C4D132F0AF4`, 11:42:09Z) was sent after ROOT's first "UI ready" message, but ROOT had paused before that send. It is not counted.

## Cleanup (done, own aliases only)
- **Channels closed via CLI, once each:**
  - uart1 w18, uart2 w19, CAN w20 (closed);
  - ETH capture stopped;
  - PSU disconnected; output held, nothing sent.
- **Already ended:** video (closed by EOF), logic (stopped after its 10 s).
- **Fixtures:** all six stopped (UART ×2, DoIP, SCPI, MJPEG, UDP sink). Logs archived to `qa/native/fixtures/`. `/tmp/loom-nativeqa` and the scratchpad run directory were removed.
- **Left in place:**
  - UDS target `qa-native-ecu`: a historical, link-down target stays listed until the daemon restarts. The CLI has no disconnect verb, and the UI removes only views and profiles. No active socket remains: lsof shows 0 daemon TCP connections to the QA fixture ports 64681, 64682, 64684 and 65201.
  - Idle per-channel worker hosts `uart.qa-native-uart1/2`, `eth.qa-native-eth`, `psu.qa-native-psu`: ready with nothing open, ttys freed.
  - UI-local saved QA profiles: ROOT removed the UART1/2 saved sessions and the QA DoIP profile in the UI. I did not edit WebKit storage.
  - ROOT UI cleanup (`root-ui-cleanup.{png,ax.txt}`): closed QA view groups with native pane buttons in all 7 modules and Integrated. Result: all 7 tab counts 0, Integrated empty, 8 inactive historical aliases retained as normal. No policy change and no app or daemon stop.
- **Not touched:** app 23400 and daemon 23423 keep running. No Git, source, policy, STOP or credential changes.
- **User events:** none during cleanup. All user-origin events in the run were ROOT's CUA actions on qa-native aliases.

## Credential scan
- Pattern scan of evidence only: 0 matches for bearer, authorization, cookie, login-code, token, password, secret or api-key patterns across `qa/native/**`, `NATIVE-LIVE.json` and `NATIVE-NEXT.md`.
- Disclosure: before ROOT's "do not read daemon.json" instruction, one scan loaded the daemon token into a shell variable to count occurrences in evidence. It was never printed or persisted, only an aggregate count (0) was output, and the variable was unset immediately. No further token reads.
- The role text `자격: approve` is not a credential.

## Curated evidence (`qa/native/`)
- **Summaries:** `sum-uart1.json`, `sum-uart2-saved.json`, `sum-uart2.json`, `sum-can.json`, `sum-eth.json`, `sum-uds.json`, `sum-psu.json`, `sum-logic.json`, `sum-video.json`, `sum-integrated.json`, `sum-persistence-state.txt`, `sum-*-marker*.txt`.
- **Raw CLI:** `raw-identity.json`, `raw-events-baseline.json`, `raw-*-open-events.json`, `raw-*-observer.out`, `raw-*-send.json`, `raw-uds-f190{,b}.json`, `raw-psu-{status,meas}.json`, `raw-logic-status.json`, `raw-video-1007-1012.txt`, `raw-video-close-events.json`, `raw-int2-*`, `raw-cleanup-*.json`, `raw-post-cleanup-status.json`.
- **ROOT native screenshots and AX:** `root-*.png`, `root-*.ax.txt`, including the final `root-ui-cleanup.*`.
- **Fixtures:** `fixtures/`, including `doip2/fixture-copy.diff`, which shows the only change to the copied DoIP fixture (port from env).
- Endpoints: `../NATIVE-LIVE.json`.

Allowlist: `qa/native/EVIDENCE-ALLOWLIST.txt`.

Source: final, no changes. No source, Git, policy or STOP edits by this slice. ROOT reports source hash groups L1–L5 unchanged from PUSH-SCOPE.
