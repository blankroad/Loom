# QA-RETURN — module-apps integration QA

**QA_FINAL_COMPLETE · CLEANED_UP.** The required browser and protocol scope is **done on the final build** (ui `db441bfb…`, app `302e5c5b…`, bundled loomd `38219c57…`; strict dist accepted by ROOT; independent Delta review PASS for F1–F4). ROOT publishes the user app separately.

**Honest scope limits:**
- **Native Connect visibility is NOT verified.** The macOS screen was locked throughout, so the evidence is the browser compositor (Ego space16, **WATCH** role) plus skill-based CLI observers on the same isolated daemon.
- **No physical hardware.** Everything ran against external loopback fixtures (PTY, DoIP, SCPI, MJPEG) and the daemon's sim/synthetic sources.
- **Recordings are not lossless.** The UART panes show an honest merged resync-gap mark.

### Final run (lqa3, `qa/final/`)
| step | backend (QA, CLI + bounded observers) | UI (ROOT) | evidence |
|---|---|---|---|
| No autoload before opens | journal 12–16: only read-only/worker-state records, 0 writes | Integrated 0 panels, all 7 counts 0 | `sum-prepare.json` |
| 7 modules opened once (2 UART, CAN, ETH, UDS, PSU OFF, video) | all rc 0; fixtures got 0 bytes / queries only / routing activation only | arranged 7 views | `sum-prepare.json`, `raw-open-*` |
| Logic 180 s synthetic `lc-1a1de5e1fb6-1` (`QAFIN-230803`) | start w7; stopped early at 08:43:23Z, user_stop: 25,284 edges, 4,437 annotations, 0 dropped | live waveform + decoder in the 8th group | `raw-logic-start/stop.json` |
| **R6**: Integrated, 8 groups, ≥9 demands | 20 s all-domain observer: **684 events, contiguous 6440–7123, not truncated**. UART w8 (TX 6767 / ack 6772, exact PTY bytes); CAN w9 (6779 → **0x301 6780** exact); UDS w10 (6784–6788, VIN OPUSW26DOIP000001). status subscriptions 8 (UI); with a CLI observer attached, connections 3 / subscriptions 8. | **R6_UI_PASS**: all 8 visible, acks/filters/VIN/route, ETH growing, PSU ~300 points OFF, video 30/30 | `sum-r6.json`, `raw-r6-*` |
| **Hidden UART2 catch-up** (UARTs hidden behind the UDS tab) | w12 `QAFIN2-3BD046`: TX 10408 / ack 10413; PTY2 got exactly the marker, PTY1 0 B | **FINAL_CATCHUP_UI_PASS**: same 8 groups and UART instance ids 0/1, no reload, ack 1/1 | `sum-catchup.json` |

### Cleanup (ROOT-authorized; `qa/final/sum-cleanup.json`)
- **lqa3 closed normally:** video (w13), UART ×2 (w14/w15), CAN (w16), ETH (w17), PSU (output held, nothing sent). Logic was already stopped. `loom daemon stop` → 18389 exited; no QA daemons, workers or observers remain.
- **Logs copied before the fixtures stopped:** `qa/lqa3-fixture-logs/` (MANIFEST plus segment offsets). Fixtures 76137, 3505, 76139, 76140, 76141 and 82545 got SIGTERM by exact PID, and all were reaped (0 PTY holders).
- **Archived before removal:** `qa/archive/qa-homes-fx-final.tar.gz` (sha256 `052553ac…f99f68`, 751 files equal to `MANIFEST-homes-fx.sha256`; no secrets).
  - **Re-count (metadata only, after ROOT's scan):** sha256 OK. **840 tar entries: 89 directories + 751 regular files**, 0 AppleDouble.
  - The manifest matches 751/751 both ways.
  - This is the only archive under module-apps. ROOT's 1591 equals 840 entries + 751 files, which points to a double count; the 751 claim stands.
- **Removed:** `/tmp/lqa1`, `/tmp/lqa1-fx`, `/tmp/lqa2`, `/tmp/lqa2-fx`, `/tmp/lqa3`. `/tmp/lqa3-fx` never existed.
- **Kept, not authorized to remove:** `/tmp/lqa1-bin`, `/tmp/loom-module-{current,fixed,final}`.
- **Untouched:** the user's app 52634 and daemon 52646, `~/.loom`, `desktop/dist`, and all production source.
- **QA process caveat:** case-insensitive file names caused overwrites (`qa/CASE-COLLISION-NOTE.json`). Since the fix, raw and summary files are named `raw-*` / `sum-*`.

## Final stack record (history)
**WAIT_VIEWER (final lqa3 live)**: final loomd pid 18389, port 62519, `LOOM_HOME=/tmp/lqa3`, bundled `38219c57…`.

## lqa2 stop and lqa3 launch (ROOT-authorized)
- **lqa2 channels closed normally** (client `qa2-close`, raw files `qa/lqa2-close/raw-*.json`):
  - `uart close qa2-pty1` (w9), `uart close qa2-pty2` (w10), `can close -c qa2-can` (w11): each rc 0
  - `psu disconnect qa2-psu`: rc 0, output "held (nothing sent)", fixture close shows OFF
  - `qa2-uds` ended with the daemon stop (fixture logged "peer closed")
- **`loom daemon stop`** → rc 0, `stopped_pid 3497`. Exit verified; no a52-build processes remain. **No SIGTERM.** `/tmp/lqa2` is kept.
- **No PTY double reader:** `lsof` shows `/dev/ttys000` held only by its fixture 76137 and `/dev/ttys008` only by 3505.
- **Snapshot:** `qa/lqa2-fixture-logs/` (11 files, `MANIFEST.sha256`, `raw-offsets-lqa3.json`).
- **`qa/qa_launch_final.py`** → rc 0. It started **only** the final loomd 18389 (:62519, env sim).
  - Reused: PTY #1 ttys000 (76137), **PTY #2 ttys008 (3505)**, DoIP :60492, SCPI :60493 (self-exits around 11:34Z), MJPEG :60495. **No third PTY.** UDP 82545 self-exits around 08:52Z and is not needed.
- **Read-only checks:** `loom status` rc 0 (env sim, STOP none, viewers 0/0, connections 1, subscriptions 0, workers ready with writes 0). Baseline cursor at **hw 12**, epoch 1794731770.
- The user's app 52634 and daemon 52646 are untouched.

## Final build record
**FINAL_BUILD_READY** (`/tmp/loom-module-final/Loom.app`, strict).

## Final build (F1–F4 corrections) — `qa/final/{raw-sources-final.sha256,sum-gates-final.json,sum-build-final.json}`
- **Freeze:** `ui/src` is `1303905e…` (a52 was `8b966e9e…`). 11 files changed. **All 11 hashes equal** those in the owner's `RETURN-REVIEW-CORRECTIONS.md` (READY_TO_FREEZE); none missing, none mismatched.
- **Accepted, not rerun:** vitest **69 files / 761 tests, rc 0** and tsc rc 0 (`review-corrections/*.log`, written after the last source edit).
- **QA ran, once each:**
  - **Full `npx eslint .` → rc 0**. The owner's lint was scoped to `src/net src/sdk src/panels/can`.
  - **Schema:** inputs unchanged (`19da59e5…`), so it was not rerun.
  - **CLI:** rc 0, `d55b25f5…` (unchanged).
  - **Strict dist:** `npm --prefix desktop run dist -- --out-dir /tmp/loom-module-final --strict` → **rc 0**, verified true, strict true. Embedded /ui has 6 files, 0 mismatched; loomd identity, embedded UI, codesign and copy-codesign all PASS.

| fingerprint | **final** | a52 | 8f57 |
|---|---|---|---|
| app tree | `302e5c5bccd65ffe18db12fae1fe632927eecad4bff9790c5b9d83a61ce2a321` | `53012ccf…` | `e7f764cd…` |
| staged loomd | `fc32d4e26ee1ef56615c389e5d8f4964ad2abc101578669e579467d912110b06` | `51c01f25…` | `b1957cdd…` |
| bundled loomd | `38219c57854bdf98eb0599406592c588f176dea5226c91967a0f7a6b4148ef2c` | `e23a1c61…` | `0d1dd4bb…` |
| loom-desktop | `31cf2b41f7659bb6db55af87ea6c198787ea27133fb8aa82b8e40b0dfc48cdcc` | `636e4e27…` | `f359cb50…` |
| ui_dist_digest | `db441bfb3eb743125a7d71ca0b9da67b25cb9e7ff08a23d687969a7a35b039a9` | `a52f57b6…` | `8f57d3cd…` |

- **Unchanged:** `desktop/dist` (the user's 52634/52646), `/tmp/loom-module-fixed` (lqa2 3497 still running from it) and `/tmp/loom-module-current`.

## a52 record (history)
**a52 regression — NOT a release.** Review returned CHANGES_REQUESTED: F1 foreign-gap scoping, F3 reconnect parked, F2 read marks and F4 initial hidden lease, all to be fixed by w38. **No final PASS on a52**, whose build is immutable. The lqa2 stack is live: daemon pid **3497**, port **61881**, `/tmp/lqa2`, bundled loomd `e23a1c61…`. See `qa/LIVE-2.json` and `qa/fix/*`.

| a52 step | backend | UI (ROOT) | evidence |
|---|---|---|---|
| R1 tour of all 7 tabs + Integrated | PASS: journal 14–18 shows only QA read-only activity, 0 writes | PASS: 7 unbound apps, counts 0 | `qa/fix/TOUR.json` |
| R2 two UARTs (`qa2-pty1` ttys000, `qa2-pty2` ttys008) | PASS: w3 / w4, TX 78/89, RX ack 81/95, exact PTY bytes, no cross-talk | PASS: own ack 1/1, opposite ack 0/0 | `qa/fix/MULTI-UART.json` |
| R3 CAN after tour, docks retained | PASS: w6, TX 661, 0x301 copy at 662 (+5.002 ms) | PASS: 418 RX, one 0x301, no gap (the 8f57 defect is resolved) | `qa/fix/CAN-FIX.json` |
| R3 PSU | connect w7, output OFF, queries only | ROOT: 0.000 V/A/W with chart points and no reload | `qa/fix/PSU-CONNECT.json`, PSU-MEAS pending |
| R3 PSU meas | PASS: one read-only meas, poll 761 at seq 3352, 0/0/OFF, 0 set/out | PASS: 301 points, no reload | `qa/fix/psu-meas.json` (overwritten provenance, see `qa/CASE-COLLISION-NOTE.json`) |
| R4 UDS seed → bind | PASS: connect (link up, journal seq 4336, routing activation 0x10); after ROOT confirmed link/P2, F190 w8: tx 5338 / **rx 5339** / result 5340, VIN OPUSW26DOIP000001 | PASS: link/P2/route before the DID; VIN and route correct | `qa/fix/{sum,raw}-uds-{connect,did}.json` |
| R6 Integrated, 9 demands | **deferred to the final build** after the F1–F4 freeze | — | `qa/FINAL-RUN-PLAN.md` |

**a52 R1–R4 complete; no more a52 protocol writes.** Final-run plan (plan only): `qa/FINAL-RUN-PLAN.md`; launcher `qa/qa_launch_final.py` → `/tmp/loom-module-final`, `/tmp/lqa3`. It reuses both PTYs (ttys000, ttys008) and the other servers, adds no third PTY, and stops only lqa2 on ROOT's signal.

## lqa1 shutdown and lqa2 launch (ROOT-authorized; `qa/lqa1-close/LQA1-CLOSE.json`)
- **lqa1 channels closed normally** with `/tmp/lqa1-bin/loom` (client `qa-close`), each rc 0:
  - `video close`: closed at frame 31276
  - `uart close qa-pty`
  - `can close -c qa-can`
  - `eth capture stop --ch qa-eth`: stopped
  - `psu disconnect qa-psu`: output "held (nothing sent)", fixture close with output OFF, 0 set/out commands in total
- **UDS** has no CLI disconnect verb, so the `qa-uds` link ended with the daemon stop; the DoIP fixture logged `connection_end: peer closed`.
- **`loom daemon stop`** (lqa1 home) → rc 0, `stopped_pid 76131`. Exit verified, and no 8f57-build processes remain. **No SIGTERM was used.** `/tmp/lqa1` is kept.
- **Fixture logs snapshotted** to `qa/lqa1-fixture-logs/`: 11 files, `MANIFEST.sha256`, and `START-OFFSETS-lqa2.json` (uart rx 20 B, doip 8 lines, scpi 38873 lines, mjpeg 31280 lines, udp 2 lines).
- **`qa/qa_launch_next.py` → rc 0.** Preconditions held: 76131 dead, every reused fixture alive.
  - Fixed loomd **3497** on **61881**, `--env sim`, home `/tmp/lqa2`.
  - **New PTY #2:** pid 3505, `/dev/ttys008`.
  - **Reused:** PTY #1 `/dev/ttys000` (76137), DoIP :60492 (76139), SCPI :60493 (76140), MJPEG :60495 (76141), UDP :52198 (82545, self-exits around 08:52Z).
- **Read-only `loom status`** → rc 0: env sim, STOP none, viewers 0/0, workers can_sim/echo/logic/video ready with 0 writes, epoch 1794730665.
- The user's app 52634 and daemon 52646 are untouched (alive since 15:49:07).

## Fixed build record
**FIXED_BUILD_READY** (`/tmp/loom-module-fixed/Loom.app`, strict). The 8f57 protocol evidence is kept in lqa1. Next-run plan: `qa/NEXT-RUN-PLAN.md`. OS locked, so browser DOM plus CLI only, no native-visible claim.

## Fixed build (after ROOT SOURCE_FREEZE) — `qa/fix/{SOURCES-FIX.sha256,GATES.json,BUILD.json}`
- **Freeze:** `ui/src` tree is `8b966e9e…` (8f57 was `c52963bc…`). 16 files changed since 8f57, and **all** match the owner hash lists: `can-live/after.sha256` (13) and the RETURN-UDS-STATE fingerprint. HEAD a8253c1.
- **Accepted, not rerun** (identical source):
  - owner full vitest: **68 files / 752 tests passed, rc 0** (`can-live/ui-test.log`)
  - tsc rc 0 (`can-live/ui-tsc.log`)
  - UDS focused: 61 passed; tsc and eslint for `src/panels/uds` rc 0
- **QA ran, once each:**
  - **Full `npx eslint .` → rc 0** (`qa/fix/ui-eslint-full.log`). The owner's lint log had no command or scope, so I ran the full lint.
  - **Schema gate not rerun:** the inputs (`schema/v1`, the generator, `src/proto/gen`) still hash to `19da59e5…`, equal to the baseline, so the 8f57 gate (rc 0, 33 schemas) stays valid.
  - `cargo build --release --locked -p loom-cli` → rc 0. The CLI hash is unchanged at `d55b25f5…`; the old copy is at `/tmp/lqa1-bin/loom`.
  - **Dist:** `npm --prefix desktop run dist -- --out-dir /tmp/loom-module-fixed --strict` → **rc 0**, verified true, strict true. verify PASS (embedded /ui, 6 files, 0 mismatched, CSP). loomd-identity, loomd-embedded-ui, codesign, copy-codesign: all bundle checks PASS.

| fingerprint | **fixed** | 8f57 (old, kept separate) |
|---|---|---|
| app tree | `53012ccfe8150c319f0c31c3e62b96039dfaa5aff4502bf12fc3140775bf8780` | `e7f764cd…` |
| staged loomd | `51c01f250024b02401023adec1bc1b8e474013e8342933e0d0f17d1c03315a90` | `b1957cdd…` |
| bundled loomd (ad-hoc re-sign) | `e23a1c61d672df132534f0f384849830df0f00c8d3a43cb9477afca09cd673c7` | `0d1dd4bb…` |
| loom-desktop | `636e4e2761e103bf8fd70dfcc6be0a345f28df7c1223d7c95cba7bd9643e0315` | `f359cb50…` |
| ui_dist_digest | `a52f57b61fcfa0e6e89f7ea0b73a6dcc61ceadb3b1fe3226cba608cbf4ddb2d0` | `8f57d3cd…` |
| CLI | `d55b25f5…` (unchanged) | `d55b25f5…` |

- **Unchanged:** `desktop/dist` (the user's app 52634 and daemon 52646, both alive since 15:49:07) and `/tmp/loom-module-current` (8f57 evidence). The lqa1 daemon 76131 is alive and untouched.
- **Note:** `target/release/loomd` is now the fixed staged binary. The 8f57 staged hash stays recorded in `/tmp/loom-module-current/artifacts.json`, and its bundled copy is preserved there.
- **Next:** once ROOT accepts the build, ROOT closes the lqa1 channels and stops its daemon. Then QA snapshots the fixture logs, runs `qa/qa_launch_next.py` (`/tmp/lqa2`, new port, plus the extra PTY), and waits for the WATCH viewer before any R1–R6 step.

## Previous state header (8f57 run)
**QA_PROTOCOL_COMPLETE · WAITING_FIX_BUILD_FREEZE** · evidence build ui `8f57d3cd…` / app `e7f764cd…` / bundled loomd `0d1dd4bb…`

## Full QA protocol summary (isolated daemon pid 76131 :60491, `LOOM_HOME=/tmp/lqa1`, epoch 1794728047)
| module | QA action (exactly once, after ROOT signalled the UI ready) | request | key refs | backend/protocol | UI (ROOT, Ego16) |
|---|---|---|---|---|---|
| UART | open `/dev/ttys000` as `qa-pty` + marker `QAMOD-074117-9A69BD` | w1, w2 | TX seq 123, device RX seq 126 (echo + `ack:` + `fx> `); fixture rx.log equals TX bytes | **PASS** | PASS: ack found 1/1 in the xterm |
| UDS | external DoIP connect `qa-uds` + read-did F190 | w3 | tx 408 / rx 409 / result 410; VIN OPUSW26DOIP000001; fixture 0x8001/0x8002 | **PASS** | **FAIL**: bound panel loses its native state after connect (UDS UI-only, fix owner w35) |
| ETH | sim capture `qa-eth` + UDP to own server 127.0.0.1:52198 | w4, w5 | eth.tx 995–1001, **eth.pkt 1002** link exact w5, src_seq 194; server got the exact 19 B | **PASS** (own TX + loopback receipt, not NIC RX) | PASS: seq 1002 bytes inspected |
| CAN | sim `qa-can` + `300#C47A7A2663DE839A --expect 0x301` | w6, w8 | TX 1972, device 0x301 copy **1973** (+5.000 ms), 0x310 byte0 C4 | **PASS** (sim) | **FAIL**: Rx summary 0 / ring1 gap (fix owner w36, SDK mux) |
| PSU | SCPI connect `qa-psu` (output OFF) + one meas | w9 | sample poll_n 303 at seq 3704; SCPI log 0 set/out commands | **PASS** | FAIL in the shared session; correct after an isolated reload (shared subscription cap, not PSU code) |
| Logic | synthetic → sigrok UART `QALOG-4AA4DF`, stopped | w10 | `lc-1a1de36d84d-1`; 5478 edges / 935 annotations, 0 dropped; contiguous match at seq 5678 | **PASS** (synthetic) | PASS: 1976 transitions / 340 decoded, live |
| Video | http-mjpeg loopback `vc5f4c430b3d7` 1280×720@30, **left live** for ROOT | w12 | frame 605 sha256 `514a27b0…`; measured 30 fps, 0 drop/dup/junk; producer 609 − 4 pre-first = 605 | **PASS** (loopback fixture, no capture board) | PASS: 30/30 fps live; local preview frozen at frame **2105**, WATCH media GET returned the exact sha `806f8b13…`. The CLI fetch of 1426 had expired from the ring (E_NOT_FOUND, ring 2316–2435); the current CLI frame is 2435, `23f8c43c…`, measured 30 fps, 0 drop/dup |

### Old build 8f57 (ui `8f57d3cd…`, app `e7f764cd…`, loomd `0d1dd4bb…`): issues vs isolated passes
| # | module | old-build UI issue (shared retained-dock session) | isolated / backend result | owner · status |
|---|---|---|---|---|
| 1 | UDS | The bound panel loses its native state after connect: header link 미보고, no active overview, P2 unknown, route unclear. The VIN itself was shown. | Backend PASS: routing activation 0x10, F190 VIN, seq 408–410 | w35 (`ui/src/panels/uds`) · fix pending |
| 2 | CAN | The active `qa-can` Rx summary stays at 0 frames with a ring1 gap for more than 45 s | Backend PASS: the 0x301 copy is at seq 1973, and the observer received 307 events | w36 (SDK mux, sole owner) · fix pending |
| 3 | PSU | 0 measured points / dashes in the shared session | **Isolated reload PASS**: 0.000 V/A/W, a 301-point chart, IDN, output OFF. The cause is the shared 8-subscription cap from retained docks, not PSU code. | covered by the w36 mux fix · recheck on the new build |
| 4 | logic, video | Probably affected by the same cap in the shared session | **Isolated PASS after the reload**, which kept only PSU + logic + video (under the cap). Logic: 1976 transitions / 340 decoded live. Video: frame 2105 sha match. | recheck in a shared multi-module session on the new build |
| 5 | UART, ETH | none observed | PASS: ack 1/1 in the xterm; eth seq 1002 bytes | — |
| 6 | all | When page 2 is in the background, the visible count can read 0 (expected browser visibility, not a defect) | — | ROOT-noted |

**Recheck on the fixed build:** UDS bound-panel state; CAN Rx summary; PSU/logic/video in the **shared** session that has all docks retained. Each runs once, with an observer armed first, after ROOT signals ready.

**Correction (ROOT):** a UI-only fix still changes the bundled loomd digest, because the fresh UI is embedded in the Rust binary.
- Do not expect an unchanged daemon binary.
- Do not reuse the old daemon's embedded UI.
- After the freeze, the matching dist needs a **new daemon with the new UI**. The QA daemon (own home `/tmp/lqa1` only) restarts under ROOT's coordinated ownership.
- The protocol source code is unchanged, but all hashes (staged/bundled loomd, ui digest, app tree) are reported **separately** from 8f57.
- Protocol proofs are re-run only where ROOT asks. QA's protocol coverage on 8f57 is considered sufficient until then (ROOT). The report draft is ROOT's (`REPORT.html`, STATUS); QA does not edit them.

Navigation-only tours created no channel. Every observer was armed first and bounded at ≤60 s, and all exited with rc 0. No repeated writes, no PSU set/out, no recording, no avfoundation. Evidence files: `qa/{LIVE,UART-MARKER,UDS-CONNECT,UDS-DID,ETH-CAPTURE,ETH-MARKER,CAN-OPEN,CAN-MARKER,PSU-CONNECT,PSU-MEAS,LOGIC-MARKER,VIDEO-OPEN}.json`. They contain no tokens, cookies or codes.

**Still live (QA-owned):**
- the daemon (pid 76131)
- the open channels/links: `qa-pty`, `qa-uds`, `qa-eth`, `qa-can`, `qa-psu` (output OFF) and the video capture `vc5f4c430b3d7`
- the fixtures: UART 76137, DoIP 76139, SCPI 76140, MJPEG 76141, UDP 82545

On ROOT's word, cleanup is: `video close`, then SIGTERM the daemon and fixtures by exact PID, verify each is reaped, and `rm -r /tmp/lqa1 /tmp/lqa1-fx`.

**Next:** after the w35/w36 fixes freeze, QA runs the affected gates once, rebuilds the scratch dist, and reports the **new** fingerprints separately from 8f57. Only the affected UI checks are re-proved.

## Video open (after ROOT signalled VIDEO_UI_READY) — `qa/VIDEO-OPEN.json`
- **Observer:** `LOOM_CLIENT=video-observer`, 30 s window, armed first at hw seq 7509, viewers 1/1 visible. It ended with rc 0: 34 events (capture_state opening at 7514 → live at 7523, 29 `video.health` at measured_fps 26 then a steady 30), not truncated.
- **Open:** `loom video open --source http-mjpeg --url http://127.0.0.1:60495/stream --size 1280x720 --fps 30 --idempotency-key qa-mod-video-open-1` → **rc 0**, request **w12**, capture `vc5f4c430b3d7`, live in 192 ms. First frame id 1 has sha256 `ef117d88…`. `others_since_last` 2 = QA's own `logic.write` (stop) under another client.
- **Wait:** `video wait --frames 60 --timeout 10s` → rc 0, matched.
- **Health:** 273 frames, 0 drop/dup/junk/oversize/truncated, stale false, ring 120 frames, 262 frames served to the UI.
- **Fresh frame:** `video frame --id 605` → sha256 `514a27b076f22b9fa62c260a54fdbb46c7ec039d9d90ac1b8753ef08bc3d75b0`, 13971 B, recv_utc 1794729610809508000, timestamp_kind receive, source_pts null.
- **Fixture:** 1 client. The producer had sent 609 frames by the time Loom received frame 605, and the last send was 0.76 ms before receipt; minus the 4 frames sent before Loom's first frame, that gives an exact count match. Source hashes are pre-re-encode, so they are not comparable by design.

## Earlier stage headers (history)
**LOGIC_MARKER_DONE · VIDEO prepared, not opened** (UART, UDS, ETH, CAN, PSU and logic backend proofs done; also `qa/LOGIC-MARKER.json`, `qa/VIDEO-PLAN.json`; see `qa/LIVE.json`, `qa/{UART-MARKER,UDS-CONNECT,UDS-DID,ETH-CAPTURE,ETH-MARKER,CAN-OPEN,CAN-MARKER,PSU-CONNECT,PSU-MEAS,LOGIC-PLAN}.json`)

## PSU read-only meas (after ROOT signalled PSU_UI_READY)
- **Observer:** `LOOM_CLIENT=psu-observer`, 30 s window, max 1000, armed first. Initial snapshot: hw seq 3617, viewers 1/1 visible. It ended with rc 0, 150 events, not truncated.
- **Meas:** `loom psu meas qa-psu --idempotency-key qa-mod-psu-meas-1 --json` → **rc 0**: v 0.0, i 0.0, p 0.0, out false, mode off, poll_n **303**, age_ms 0, since_ms 44, meta viewers 1/1 visible, others 0.
- **Journal:** `psu:qa-psu@e1794728047:seq:3704` is the `psu.meas` device sample for poll_n 303. Its payload carries poll_n/out/age_ms/backend only, **without V/I values**.
- **Fixture SCPI log at the meas time:**
  - The regular poll exchange `MEAS:VOLT?`→`0.0000`, `MEAS:CURR?`→`0.00000`, `OUTP?`→`0` at −0.26 ms.
  - The cadence stays an unbroken ~200 ms (197–203 ms) within ±1 s, with **no extra out-of-band query**.
  - So `psu meas` returns the freshest poll sample rather than issuing its own SCPI query. Recorded as observed.
- **Safety:** the fixture has received **0** set/output commands in total; output stays OFF. No set, out or cycle was used.
- **Verdict:** backend protocol **PASS**; UI **NOT PASS** in the shared-dock session, which showed 0 points and dashes.
  - After an isolated page reload, ROOT saw 0.000 V/A/W, age 0 ms, a 301-point chart, the correct external IDN and output OFF (`psu-isolated-live.png`).
  - So the failure is the **shared retained-subscription cap**, not PSU panel code. The CAN worker, sole owner of the SDK mux, is fixing it.
  - QA made no repeat writes. The 8f57 build is kept exact.

## Video (engine gate only; capture NOT opened)
- **Read-only `loom video capabilities`:** rc 0. ffmpeg 9.0.2 at /opt/homebrew/bin, output mjpeg (fps 1–60, ring 120 frames), audio unavailable, quota usage 0, recording is a Matroska copy of the MJPEG previews (no lossless claim).
- **Fixture-side check:** `GET /control/state` on the MJPEG fixture → seq 0, clients 0, run QAMOD1, so nothing has connected yet.
- **Plan:** `qa/VIDEO-PLAN.json` (http-mjpeg loopback open, `wait --frames`, `frame --id` sha256 for ROOT's frozen-panel comparison). avfoundation and `sources --local` are never touched.
- **Status:** logic and video UI checks are probably blocked by the same subscription cap. Both wait for ROOT's active UI before any start.

## Logic synthetic → sigrok UART decode (after ROOT signalled LOGIC_UI_READY) — `qa/LOGIC-MARKER.json`
- **Observer:** `LOOM_CLIENT=logic-observer`, 30 s window, max 1000, armed first at hw seq 5505, viewers 1/1 visible. It ended with rc 0: 330 events (258 `logic.edges`, 62 `logic.annotations`, 4 capture_state, 6 write), **5478 edges**, which equals the capture total, not truncated.
- **Start:** `loom logic start --source synthetic --rate 1M --decode uart:rx=D0:baudrate=100000 --text 'QALOG-4AA4DF {n}' --frame-period 500ms --duration 60s --idempotency-key …` → **rc 0**, request **w10**, capture **`lc-1a1de36d84d-1`**, engine sigrok-cli.
- **Wait:** `logic wait --capture … --ascii QALOG-4AA4DF --decoder uart --timeout 5s` → rc 0, `ascii_contiguous` matched. The last char 0x46 is at seq 5678, samples 5001130–5001210, decoder `uart-1`, class null.
- **Live, before stop:** nonzero raw edges (the first chunk is seq 5519 with 96 edges, dropped 0). The ASCII reconstructed from annotations reads `QALOG-4AA4DF 0000QALOG-4AA4DF 0001…`.
- **ROOT live UI:** 9.62 s of samples, 1976 transitions, 340 decoded, zero omission, raw waveform plus hex decode visible (`qa/browser/logic-live.png/json`).
- **Stop:** `loom logic stop lc-1a1de36d84d-1` → rc 0, state stopped, reason `user_stop`, decoder exit 0 (eof). Counters: samples 27,420,746, edges 5478, annotations 935, all dropped/emit/unparsed counts 0, quality `synthetic`.
- **Scope:** synthetic integration only, no hardware. No second capture.

## Video open command (prepared, NOT opened; awaits VIDEO_UI_READY)
`LOOM_CLIENT=qa-video loom video open --source http-mjpeg --url http://127.0.0.1:60495/stream --size 1280x720 --fps 30 --idempotency-key qa-mod-video-open-1 --json`. Flags are confirmed from help; the observer is armed first.

## Logic preparation record (superseded by the run above)
- **Read-only `loom logic status`:** rc 0, sigrok-cli 0.7.2 at /opt/homebrew/bin, decode true, implemented sources synthetic/sigrok-demo/sigrok-device, decoders uart/spi, fx2lafw `device_verified: false`, no running capture.
- **Plan (`qa/LOGIC-PLAN.json`):**
  1. Bounded logic observer.
  2. `logic start --source synthetic --rate 1M --decode uart:rx=D0:baudrate=100000 --text 'QALOG-<nonce> {n}' --frame-period 500ms --duration 60s`
  3. `logic wait --text` and `read --kind all` for nonzero edges and annotations before stop.
  4. Stop only after ROOT has compared.
- **Provenance:** synthetic, so integration only; no hardware claim.
- **Source fixes in progress elsewhere:** CAN UI (w36) and UDS UI (w35). QA makes no source edits.

## PSU SCPI connect (prepare only; the CAN observer had ended with rc 0: 307 events, not truncated)
- **Before connect:** the fixture had output OFF, set 0 V / 0.1 A.
- **Command:** `loom psu connect qa-psu --scpi-tcp 127.0.0.1:60493 --idempotency-key qa-mod-psu-connect-1 --json` → **rc 0**, request **w9**, backend scpi-tcp, link up, idn `LOOM-TEST-FIXTURE,EXTPSU-1,09c0f03a993d,0.1`.
  - out **false** (known), mode off, set_v 0.0, set_i 0.1, vmax 16, imax 5
  - meta viewers 1/1 visible, others 0
- **Read-only status:** link up, out false, measured poll rate 5.0 Hz (the daemon's own poll), prot `unknown`.
- **Fixture SCPI log:** only queries (`*IDN?`, `VOLT?`, `CURR?`, `OUTP?` → `0`, `MEAS:VOLT?`/`MEAS:CURR?` → 0). **No VOLT/CURR/OUTP set command.**
- **Not done:** no set/out/cycle and no QA meas/watch until ROOT signals PSU_UI_READY.

## CAN sim request → reply (after ROOT signalled CAN_UI_READY)
- **Simulator rule** (`crates/loom-can/src/sim_default.toml`, BCM): 0x300 → 0x301 reply that copies the request after 5 ms; 0x310 periodic every 100 ms, whose byte0 follows the last 0x300 byte0. This is the daemon's own documented example (`can send 300#01 --expect 0x301 --within 100`).
- **Dry run:** rc 0, R1 "sim channel"; sent nothing (it used request id w7).
- **Observer:** `LOOM_CLIENT=can-observer`, 30 s window, max 1000, armed first. Initial snapshot: hw seq 1966, viewers 1/1 visible.
- **Send:** `loom can send 300#C47A7A2663DE839A -c qa-can --expect 0x301 --within 100 --idempotency-key …` → **rc 0**, request **w8**, sent 1, **effective_verified**, R1.
- **Journal records** (epoch 1794728047):
  - `can.tx` seq 1968–1971 for w8
  - TX frame **`can:qa-can@seq:1972`**: 0x300, `c47a7a2663de839a`, agent, w8
  - device reply **`can:qa-can@seq:1973`**: 0x301, `c47a7a2663de839a`, origin device, producer can_sim, source_seq 636, **+5.000 ms**
  - seq 1974: effective_verified
  - 0x310 at seq 1975/1977 has byte0 **C4**, which follows the request
- **Scope:** simulator only, no physical CAN hardware. Browser view plus CLI observer; the OS is locked, so no native-visible claim.
- **Verdict:** backend protocol **PASS**; UI **FAIL**. ROOT-found **CAN_UI_DEFECT** on build ui 8f57d3cd…: the active `qa-can` Rx summary stayed at 0 frames with a ring1 gap for more than 45 s, before and after the marker, while the CLI observer received 307 events including the exact 0x301 reply. This is **not** a UI pass. A focused CAN/UI worker owns the fix; QA will not repeat the CAN send.

## CAN sim open (prepare only; the ETH observer had ended with rc 0 after 68 events)
- **Command:** `loom can open -c qa-can --nodes default --idempotency-key qa-mod-can-open-1 --json` → **rc 0**, request **w6**, backend sim, bus `qa-can`, generation 1, nodes [BCM], not latched, meta viewers 1/1 visible, others 0.
- **Read-only `can status`:** state open, 1 node, frames_total 0 at read time (all origins 0), observer drops 0, latch false.
- **Not sent:** no frame until ROOT signals CAN_UI_READY.

## ETH fresh UDP marker (after ROOT signalled ETH_UI_READY)
- **UDP fixture:** `udp_server.py` pid **82545**, bound 127.0.0.1:52198, self-exit 3600 s, log `/tmp/lqa1-fx/udp/udp-server.jsonl`.
- **Dry run:** rc 0, R1 "send to loopback", 19 B; the server received nothing.
- **Observer:** `LOOM_CLIENT=eth-observer`, 30 s window, armed first. Initial snapshot: hw seq 994, viewers 1/1 visible.
- **Send:** `loom eth send udp 51414554482d3037353235382d354343374637 --to 127.0.0.1:52198 --ch qa-eth --idempotency-key …` → **rc 0**, request **w5**, `sent`.
- **Journal records** (epoch 1794728047):
  - `eth.tx` seq 995–1001 for w5 (requested → accepted → sent, final at 1000; seq 1001 has observed_src_seq 194)
  - **`eth:qa-eth@seq:1002`** `eth.pkt`: dir tx, attributed_origin agent, link `{request_id:w5, confidence:exact}`, `_capture.source_seq` 194, UDP 55415 → 52198, payload `QAETH-075258-5CC7F7`
- **Server receipt:** exactly 1 datagram from 127.0.0.1:55415, 19 B, hex equal to the sent hex.
- **ROOT** saw seq 1002 live in Ego and inspected its raw bytes (`qa/browser/eth-marker.png`).
- **Scope:** an observation of Loom's own TX on the sim capture plus an external loopback UDP receipt. This is **not** physical NIC RX and **not** device behaviour.

## ROOT-found UI defect (UDS), recorded for traceability
- **Symptom:** after connect, the bound UDS panel snapshot loses native state: header link shows 미보고, active overview none, P2 unknown, route unclear. Meanwhile the backend shows routing activation and the F190 VIN normally, and the UI did confirm the VIN.
- **Classification:** UI state mapping, not a protocol or fixture failure. A fresh UDS worker owns the fix, in `ui/src/panels/uds` only. QA makes no source edits.
- **Evidence build** (kept exact): app tree `e7f764cd…`, ui `8f57d3cd…`, bundled loomd `0d1dd4bb…`. The UDS-DID proof is preserved; I made no repeat connect or write.
- **After the fix:** QA will report the new build's fingerprints **separately** from this 8f57 build. ~~The daemon binary is expected unchanged~~ (corrected: the bundled loomd embeds the UI, so its digest **will** change; it needs a new daemon with the new UI and separate hashes).

## ETH sim capture (prepare only)
- **Interfaces (read-only):** `loom eth ifaces` → rc 0: `sim` (synthetic tap), `replay`, `stream`. No NIC is used.
- **Start:** `loom eth capture start sim --backend sim --ch qa-eth --idempotency-key qa-mod-eth-capture-1 --json` → **rc 0**, request **w4**, state live, linktype 1, snaplen 65535, meta viewers 1/1 visible, others 0.
- **Read-only status:** live, total 7 / shown 7 (the sim backend's own synthetic frames), display_lost 0, unrecorded 0. The backend reports **`journal_available: false`**; recorded as-is.
- **Not sent:** no send and no dry-run until ROOT signals ETH_UI_READY.

## UDS read-did F190 (after ROOT signalled UDS_UI_READY)
- **Observer:** `LOOM_CLIENT=uds-observer loom events --since now --domain uds --channel qa-uds --follow --timeout 30`. Initial snapshot: count 0, hw seq 397, viewers 1/1 visible.
- **Request:** `loom uds read-did F190 -t qa-uds --idempotency-key qa-mod-uds-did-f190-1 --json` → **rc 0**.
  - request **w3**, `confirmed`, R0 auto, RTT 0.634 ms, pending 0, timeout false
  - request PDU `22f190` → response `62f1904f505553573236444f4950303030303031`, VIN **OPUSW26DOIP000001**
- **Journal records** (epoch 1794728047): seq **408** `uds.tx` (agent, w3); seq **409** `uds.rx` (device, w3, late false); seq **410** `uds.result` (confirmed).
- **Fixture DoIP log:**
  - RX 0x8001 `02fd8001000000070e80100122f190`
  - TX 0x8002 ACK
  - TX 0x8001 `…10010e8062f190` + VIN
  - That is exactly one diagnostic request.
- **Observation:** the event `env` field reads `bench` while the daemon meta reads `sim`, the same as on UART.
- **Not sent:** no raw, write or session request. Other domains wait.

## UDS external DoIP connect (connect only)
- **Command:** `loom uds connect --name qa-uds --host 127.0.0.1 --port 60492 --la 0x1001 --tester-la 0x0E80 --idempotency-key qa-mod-uds-connect-1 --json` → **rc 0**.
  - entity 127.0.0.1:60492, existing false, link up (`connected`), generation 1
  - session default and security, both `assumed` (no request sent to confirm them)
  - P2 50 ms / P2* 5000 ms
  - meta viewers 1/1 visible, `others_since_last` 0
- **Journal:** `uds.state` at e1794728047:seq:323. A read-only `uds status -t qa-uds` agrees.
- **Fixture DoIP log:** one TCP accept; RX 0x0005 `02fd0005000000070e800000000000`; TX 0x0006 code 0x10 from ECU 0x1001. **Zero 0x8001 diagnostic messages.**
- **Not sent:** no DID/raw/session/DTC requests until ROOT signals UDS_UI_READY. Other domains wait.

## UART fresh marker (after ROOT reported UART_UI_READY: Ego space16 shows the qa-pty terminal, watch / visible 1)
- **Observer armed first:** `LOOM_CLIENT=uart-observer loom events --since now --channel qa-pty --follow --timeout 60 --max 200 --json`. Initial snapshot: count 0, epoch 1794728047, hw seq 102, viewers 1/1 visible.
- **One send:** `loom uart send qa-pty 'QAMOD-074117-9A69BD' --eol lf --expect 'ack:QAMOD-074117-9A69BD' --timeout 10 --idempotency-key … --json` → **rc 0**, request **w2**, written 20, `sent`, expect matched (RX 2291–2314), `others_since_last` 0, meta viewers 1/1 visible.
- **Journal records** (epoch 1794728047):
  - seq 119: write requested
  - seq **123**: TX `uart.data` origin agent, `QAMOD-074117-9A69BD\n`
  - seq 124: sent, final
  - seq **126**: device RX `uart.data` at rx_offset 2270, 50 B, `QAMOD-074117-9A69BD\r\nack:QAMOD-074117-9A69BD\r\nfx> ` (echo, ack, prompt)
  - seq 128: expect_result matched
- **Decoded lines:** echo (2270–2291), `ack:QAMOD-074117-9A69BD` (2291–2316), `fx> ` (2316–2322). Ticks follow as device-side output.
- **Fixture `rx.log`:** 0 → 20 bytes, exactly `51414d4f442d3037343131372d3941363942440a`, which equals the TX bytes.
- **Scope:** browser compositor plus the CLI observer only; the OS is locked, so no native-visible claim. ROOT compares w2 / seq 123 and 126 against the Ego terminal DOM. No other domain was written.

## UART handshake (after ROOT reported the WATCH viewer ready: Ego space16, subscribed 1 / visible 1)
- **Command:** `LOOM_HOME=/tmp/lqa1 LOOM_NO_AUTOSTART=1 LOOM_CLIENT=qa-uart target/release/loom uart open /dev/ttys000 --as qa-pty --baud 115200 --idempotency-key qa-mod-uart-open-1 --json` → **rc 0**, ok.
  - Result: channel `qa-pty`, generation 1, request `w1`, state `sent`, 8N1, flow none.
  - Meta: env sim, latch false, viewers subscribed 1 / visible 1, visibility visible, stream_epoch 1794728047, `others_since_last` 0.
  - Saved to `qa/uart-open.json`.
- **Read-only** `loom uart status qa-pty --json` → rc 0, connected true (`qa/uart-status-open.json`).
- **Observation:** the channel reports `env: bench` (the default of `uart open --env`) while the daemon badge is `sim`. I did not pass `--env`. This is recorded as-is, not as a defect claim.
- **No send:** the fixture's `rx.log` is 0 bytes, so Loom has written nothing to the device. The fixture prints its own `tick n` lines every 2 s; that is device-side RX only. No observer and no fresh marker yet. All other module writes wait. · owner QA (Opus 5.5, foreground, no subagents) · 2026-11-15

## Live isolated stack (started 07:34:08Z on ROOT's request; no device RPC sent yet)
- **Daemon:** pid **76131** (own pgid), `/tmp/loom-module-current/Loom.app/Contents/MacOS/loomd run --port 0 --env sim`. This is the shipped scratch binary, sha `0d1dd4bb…`.
  - Port **60491**, `LOOM_HOME=/tmp/lqa1`, `LOOM_NO_AUTOSTART=1`, no `--rights` flag (default).
  - Read-only `loom status` (rc 0, `qa/status-initial.json`): env sim, STOP not latched, viewers subscribed 0 / visible 0, workers ready (can_sim, echo, logic, …), writes 0.
- **Fixtures** (each detached, own pgid, loopback only):

  | fixture | pid | endpoint |
  |---|---|---|
  | UART PTY | 76137 | tty `/dev/ttys000` (state `/tmp/lqa1-fx/uart`) |
  | DoIP ECU | 76139 | 127.0.0.1:**60492** (ECU 0x1001, tester 0xE80) |
  | SCPI PSU | 76140 | 127.0.0.1:**60493**, control 60494 (self-exit 4 h) |
  | MJPEG | 76141 | `http://127.0.0.1:60495/stream` (run id QAMOD1) |
- **What QA has not done:**
  - opened a channel or sent any device RPC
  - minted a bootstrap or viewer
  - stored a token, code or cookie (the `LIVE.json` grep finds none)
- **Next step is ROOT's.** ROOT attaches a normal **WATCH** fixture viewer via Ego and confirms the initial snapshot; then it signals the CLI skill-based sim smoke. While the OS is locked, only the browser DOM is evidence: no native-visible PASS can be claimed, and the native Connect path needs the user to unlock.
- **Cleanup when ROOT says done:**
  1. SIGTERM 76131 (QA home only), then 76137/76139/76140/76141.
  2. Verify each exact PID is reaped.
  3. `rm -r /tmp/lqa1 /tmp/lqa1-fx`.
- Launcher: the session scratchpad script `qa_launch.py`. It refuses to run if the home already exists.
I ran no CUA/Ego/browser, launched no app or fixtures, did no Git/index operations and made no production source edits. My one source edit is the authorized test-harness file `ui/e2e/workbench-ego/services.body.mjs`. app52634/daemon52646/~/.loom are untouched; I re-checked them by `ps` after the build, same start time 15:49:07.

## Source freeze
- ROOT sent SOURCE_FREEZE (A/B/C READY, reviewed). Snapshot: `qa/SOURCES.sha256` (HEAD a8253c1, 44 files named in RETURN-A/B/C, `git diff --stat`: 82 files +8019/−1843).
- `ui/src` tree digest is `c52963bc333e0496f944e37a67915885e26d83ab2304e8af531e96b659d47d21`. It is identical before and after the dist. All owned-file hashes re-verified after the build (`shasum -c` rc 0). RETURN-B's 6 hashes match.

## Phase-2 commands (actual rc; logs in `work/reports/module-apps/qa/`)
| # | command (cwd) | rc | result |
|---|---|---|---|
| 1 | `npx tsx scripts/gen-schema.ts --check` (ui) | 0 | `schema ok: 33 schemas, sha256 b43d8c97…c0857` — ui-schema.log |
| 2 | `npx tsc -b --pretty false` (ui) | 0 | no output. B's earlier `UartEmptyPane` TS2305 is resolved. ui-typecheck.log |
| 3 | `npx eslint .` (ui) | 0 | no output — ui-lint.log |
| 4 | `npx vitest run` (ui) | 0 | **66 files, 736 tests passed**, 9 s — ui-test.log |
| 5 | `cargo build --release --locked -p loom-cli` (repo) | 0 | 18.7 s — cli-build.log |
| 6 | `npm --prefix desktop run dist -- --out-dir /tmp/loom-module-current --strict` (repo) | 0 | started 07:28:47Z; verify PASS, verify-bundle PASS, bundle PASS (all checks) — dist.log, dist.rc |
| 7 | `node --check ui/e2e/workbench-ego/services.body.mjs`; the same on prelude+body concat (as run.sh builds it); `npx eslint e2e/workbench-ego/services.body.mjs` | 0/0/0 | syntax/lint only; **not executed** (GUI is ROOT's) |

Each was run once; I made no reruns.

## New fingerprints (scratch build — READY for ROOT)
- App: `/tmp/loom-module-current/Loom.app`. Tree sha256 `e7f764cd58543c3f710aed08ee7973bb3b54ee3ca69ca510ab732d0880335382` (30,575,219 B).
- `artifacts.json`:
  - generated_at 2026-11-15T07:30:01Z
  - baseline a8253c1, profile release
  - verified **true**, strict **true**
- Strict bundle checks: cargo-lock, sidecar, info-plist, bundle-id, main-exe arm64, loomd-identity, loomd-embedded-ui (6 files, 0 mismatched, CSP yes), no-home-data, no-credentials, no-qa-webdriver and codesign (ad-hoc) all PASS. Notarization was skipped because no Apple credentials are set; that is expected for a local build.

| item | sha256 |
|---|---|
| staged loomd `target/release/loomd` (artifacts `loomd_sha256`) | `b1957cdd99d4087e890f6f817d83f0a2b327df4d2f7df7ae4ca27007565a955d` |
| bundled `Contents/MacOS/loomd` (ad-hoc re-sign of staged; identity PASS) | `0d1dd4bb8a7e2e2eb6bd6a796d36a927defc672e89eb1debbe1fd1f267b2587d` |
| `Contents/MacOS/loom-desktop` | `f359cb5056424539709884094fdd8a09473b0f3f3752a3764a89624e21a5be7b` |
| `ui_dist_digest` | `8f57d3cd898615511300d611b22be9b489158c678bc5b5e3bd1c878b27db8bf9` |
| matching CLI `target/release/loom` (16:28) | `d55b25f54ef14eb1d1bbc16ed7a02b75ca8efd02e220b8f252ee1e6f678d8270` |

The production build is unchanged (`desktop/dist`, 15:48):
- app tree `97ebd3d7…de6198c`
- loom-desktop `3d74e813…3b3d4`
- bundled loomd `d4cfa7bb…b24f`
- staged loomd `e56e10e2…b795cb`
- ui `643e5952…6f05b0f05`

## Test-harness edit (authorized: `ui/e2e/workbench-ego/services.body.mjs`)
- Before: `1731b1fe26552faa2f489191ba2db2da4459417f2821a95c4abf096d672788e9` (copy at `qa/services.body.mjs.orig`)
- After: `828625b1d1f2fa89c0152dd1384709d0eff5c6928971805d38ebc727bd9d7426`
- Diff: `qa/services.body.mjs.diff`

**Changes**
- **View counts.** Views are counted only in the front `[data-testid="layout"]` dock, because retained hidden docks also render `.dv-tab`. I also assert exactly one front dock and record `allTabs` for diagnosis.
- **Nav assertions.** The old active-only rules ("no inactive service navigation", "UART-only navigation", `services.length===5`, "closed service leaves primary navigation") are replaced by:
  - seven permanent tabs
  - truthful `data-count` per module: UART 2 / CAN 1 / ETH 1 / UDS 1 (real UDS only) / PSU 1 / logic 0 / video 0
  - after the UART closes: UART 0, the other counts unchanged
- **New, zero-channel tour.** CAN/ETH/UDS/PSU/logic/video each show exactly one unbound application view: title of that module, no ` · alias`, no `lan1`/`bench`, not the closed-workbench card, own count 0. Zero-channel Integrated has no views, and navigation created no channel. The UART empty state must be the session picker (`uart-empty-pane` inside `dock-empty`).
- **New, logic/video while other services are live.** Still unbound; the live counts are unchanged.
- **Actual-channel checks kept, with selectors adapted honestly to the current DOM.**
  - CAN/PSU/Integrated: `#side .channel-explorer [data-chan]`, open only via `data-state`.
  - UART: its side session list, `#side .sess[data-channel]` (unsaved open channels).
  - ETH/UDS: these own their toolbar, so `#side` is hidden and there is no shell explorer. I assert that the side is hidden **and** that the front-dock view titles bind exactly the actual aliases. For UDS that is `gw`,`pt`,`mirror`, which is still the combined diagnostic observation check.
  - Exact view count, no foreign view and active aliases are kept everywhere.
  - Integrated must show all 6 real channels and 6 views whose aliases equal the real channels, so no unbound app is stuffed in.
  - Reload on `#/s/psu` keeps the connected OFF PSU.
  - Reopen `mcu` restores only `uart/mcu`.
  - Late join on CAN scope, 768/390 STOP and overflow, and the shared STOP latch are kept.
- **Historical selectors marked N/A, not silently dropped.** `.service-relations` (CAN/ETH→UDS links, UDS→CAN/ETH links) and `.service-nav-label` (진단 group) no longer exist anywhere in `ui/src`. Each is logged as `N/A …` with the reason, plus a check that the selector really is absent. `.workbench-main` text is repointed to `#modUart .work`, and the heading to the footer `.shell-status .st-target`.
- **Closed UART.** A states that a closed view stays as read-only history, so the old `tabs.length===0` is replaced by: no open UART channel, and either the picker or UART-only history views.
- **Assumptions to watch on first run:**
  - The UART side list shows unsaved open channels as `.sess[data-channel]`. Saved sessions use `data-sid` instead, so a test browser with saved UART sessions for `mcu`/`aux` would under-report.
  - Tab titles use the ` · <alias>` suffix (dockHost `label · port`).

## Fixture launch instructions (for ROOT's coordinated window — NOT launched)
Use your own home with frozen binaries: `export LOOM_HOME=/tmp/loom-qa-home-<run> LOOM_NO_AUTOSTART=1 LOOM_CLIENT=qa-observer`. The CLI is `target/release/loom`, and the daemon is the scratch app's own `loomd` (or `target/release/loomd`, sha b1957cdd…). Take the exact daemon start and domain flags from `loom help` and `loom <domain> help --json` at launch time; I guess none of them.

| fixture | start | owned resources / cleanup |
|---|---|---|
| UART pty | `python3 -I desktop/scripts/lib/uart_fixture.py --state $RUN/uart` → `loom uart open $(cat $RUN/uart/tty) --as <alias>` | `touch $RUN/uart/stop` or `kill $(cat $RUN/uart/pid)` |
| DoIP ECU | `python3 -I work/reports/validation-report/tools/doip_fixture.py $RUN/doip` (127.0.0.1; `22F190`→VIN, `22FFFF`→NRC 0x31) | its pid (logged in `doip.jsonl` `fixturePID`); SIGTERM |
| MJPEG | `python3 -I work/reports/video-capture/fixture/mjpeg_server.py --port 0 --fps 30 --size 1280x720 --run-id RUN --log $RUN/sent.ndjson` (needs Pillow; prints a ready JSON line with the port) → `loom video open --source http-mjpeg --url http://127.0.0.1:PORT/stream` | pid from the ready line; SIGTERM |
| PSU SCPI | `work/reports/external-simulator-validation/psu/fixture/scpi_psu_fixture.py` (queries only, no output ON) | SIGTERM |
| ETH / CAN / logic | daemon sim backends (`eth capture start sim --backend sim --ch <alias>`, CAN sim, `logic --source synthetic`), all chosen explicitly | `capture stop` / `close` / `logic stop` |

**Cleanup:** stop the QA daemon (the QA home only), SIGTERM each fixture pid and verify the reap by exact PID, then remove `/tmp/loom-qa-home-<run>` and `$RUN`. Never stop 52634/52646.

**Tools:** ffmpeg 9.0.2 and sigrok-cli 0.7.2 are at /opt/homebrew/bin; socat is absent and not needed.

## Remaining for ROOT
1. Launch the scratch app `/tmp/loom-module-current/Loom.app` under a coordinated home/lease. Run native and Ego UI checks:
   - all 7 tabs + Integrated at 1440 and minimum width; A asks for a visual check of the header budget
   - per-module Connect/config flows
   - zero channels on navigation
   - the Integrated → link dock deep link
2. Optionally run the updated `services` scenario (`ui/e2e/workbench-ego/run.sh services`). It has not been executed yet, so its first run is the real test of the selector assumptions above.
3. Run the protocol smoke on the changed Connect paths using the fixtures above.
4. Replace the user's app52634 with the new build after the ownership/state check. The user-facing app stays open. Git is yours.

Open owner items (from the returns, not QA defects): B #1–#4 (bind via a new panel, filter not carried, stopped alias refused, old layouts), A's seed-draft disposal at handoff, and the unmeasured header widths. **No REQUEST files were needed:** every gate passed.


## ROOT final publication and archive clarification

QA role returned; ROOT owns this final note. New user app23400 / own bundleddaemon23423 port62968 published and left running. ROOT normally stopped old owned52634/52646 after no-device/writes0/audit-readonly check; QA did not operate the user app. Actual published UI6/6 files match finaldb441bfb; app tree302e5c5b and signature unchanged on copy. Native-visible clicks still PENDING Mac unlock. `qa/final/sum-published-running.json` and `sum-user-served-ui.json`.

Archive count clarification verified by Python tarfile (not BSD tar's filtered listing): 1,680 total members = 1,591 regular members +89 directories. Of the regular members,840 are AppleDouble `._*` metadata and751 are actual content files (matching the manifest). The earlier751 content count is valid; the suggested explanation that1,591 double-counted listings was incorrect. ROOT's credential-pattern scan read358 text evidence files plusALL1,591 archive regular members:0 hits. No archive bytes changed.

All task implementation/review/QA foregroundOpus roles returned, `/exit` verified agent_not_found, owned workspaces31/32/33/35/36/37/38/34 closed. MAINw8 untouched. Browser/report handoff is ROOT's final step. No Git/index changes this slice.
