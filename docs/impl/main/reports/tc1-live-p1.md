# TC-1 live: powered virtual DUT + journal-judged session

- Feature / status / role: TC-1 mixed-sim acceptance / **READY_TO_INTEGRATE** / Herdr Claude Opus 5.5 foreground, done directly (no sub-agents). Self-review only, no independent review.
- Base: main `6d41c24` + current uncommitted changes from other owners, kept as-is. No index/commit/push. No manifest/lock changes.
- Placement: **`Config.workers = Inproc`**. PSU/UART/CAN workers run in-process with their real backends (PSU sim, real PTY, CAN sim hub) plus the real journal. UDS is a gateway producer over a real loopback DoIP entity. IPC worker domains are rejected by TEST as `fence_unflushed` (uncertain) and cannot PASS. Their IPC RPC paths are covered by the existing IPC smokes. This test does not claim IPC PASS.

## Changed files (owned, all new)

| File | Content |
|---|---|
| `crates/loom-gateway/tests/tc1_live_smoke.rs` | Real daemon (isolated temporary HOME) + WebSocket agent/viewer/fixture identities. Powered virtual DUT fixture and the full TC-1 session |
| `crates/loom-gateway/tests/fixtures/tc1_dut.dbc` | DUT `VehState` (0x3A0, `PowerMode` 4 bits, 2=RUN). sha256 `ec6c18ea…87bf` |
| `docs/impl/main/fixtures/TC-1.live.plan.yaml` | Canonical TC-1 plan (no finally) |
| `docs/impl/main/reports/tc1-live-p1.{deps.md,uart-sim-pty.patch,can-signal-range-args.patch}` | Dependency/proposal patches (main has applied both patches) |

## Canonical plan ↔ TC-1 text

| TC step | Plan | Judgment basis (journal re-read by TEST) |
|---|---|---|
| 1 power OFF → 2 s → ON | S1 `do: psu.out{ch:bench,off}`, `sleep 2s`, `psu.out{on}` | `done` = confirmed write rows inside the attempt |
| 2 `login:` ≤ 30 s after ON | S2 `uart.expect{channel:dut, patterns:[{kind:literal,pattern:"login:"}], timeout_ms:30000}`; expect `uart.text_contains` (device), anchor = the supply's own `psu.state` `/payload/out==true` within S1, `within_ms: 30000` | `verified`, witness `device` |
| 3 PowerMode=2 ≤ 5 s after boot | S3 `can.sig.wait_for{channel:ch0, expr:"VehState.PowerMode == 2", within_ms:5000}`; expect `can.signal_range{signal, between:[2,2], origin:device, db_sha256}`, anchor = S2's `uart.expect_result` `matched==true`, `within_ms: 5000` | `verified`, witness `device` |
| 4 DTC mask 09; if present, clear and re-check | S4 `uds.dtc.list{target:bcm, mask:0x09}` + `uds.dtc_absent`, `on_fail: goto S5`. S5 `when S4 fail`, `uds.request{bytes_hex:"14FFFFFF"}` → `uds.dtc.list` + `uds.dtc_absent` | S4 `mismatch` (2 DTCs) → fail; S5 `verified` device |

- **No finally**: the TC text has no final power-off, and the registered PSU safe state `psu.out off` is R3, which conflicts with the format's finally R0–R1 rule. The risk level was not lowered. `test.session_ended.finally_skipped: true`.
- Fixed versus the legacy `TC-1.plan.yaml`: `psu.out{name}`→`{ch}`, `uart.expect{port,pattern,timeout}`→`{channel,patterns,timeout_ms}`, `can.sig.wait-for{cond,timeout}`→`can.sig.wait_for{expr,within_ms}`, the unregistered `uds.dtc.clear`→`uds.request{bytes_hex:"14FFFFFF"}`, the `can.signal`/`exists` expectation→the real CAN verifier `can.signal_range`, and the anchor's nonexistent `/payload/state_out`→`psu.state /payload/out`. The legacy fixture (loom-test-format owner) is untouched.

## Virtual DUT (shared power state, reacts to measurements only)

- **Power source**: a fixture-only agent `tc1-dut-fixture` subscribes to the live stream (`psu.write`, ch `bench`). Power state is taken **only from rows where the PSU worker confirmed its read-back** (`write_state: effective_verified`, `action: out`, `readback.out`). Agent request results are not used.
- **UART**: `PtyDevice::spawn(PoweredBootshell, Splitter::interactive)` → the daemon opens the real PTY with `uart.open{target:/dev/ttysNNN, alias:dut, env:sim}`. While off: no output, input dropped. Power-on: 1.5 s ROM delay, then a fresh `Bootshell` (U-Boot → 200-line boot log → `login:`). Power-off: console discarded, `booted=false`.
- **CAN**: 300 ms after `login:`, the fixture identity opens a second sim channel `dutnode` on ch0's bus (`can.open{channel:dutnode, bus:ch0, nodes:<DUT_BCM TOML>}`). A real sim node transmits `VehState` every 100 ms, and ch0 records it as `origin: device`. Power-off closes it with `can.close{dutnode}`. This replaces `can.node.start/stop`, which are not exposed via the gateway (worker calls only); it uses only real gateway RPCs. The fixture's writes are on the `dutnode` channel and stay outside the ch0 judgment window. The TEST agent's results are never forged.
- **UDS**: `loom_uds::sim_support` real `DoipEntity` (always-on DoIP edge) + a bench `SimEcu` gated by power. While the application is down: no answer (the ECU instance is dropped, so volatile session/security state is lost). DTC memory survives power-off (non-volatile). Initial DTCs come from the bench profile, `923400/0x09` and `C10000/0x2F` (the precondition "DTC in any state").
- The fixture uses **no** `daemon.publish`, echo, replay or synthetic events.

## Verification

Command (original tree, after main applied the patches):
`CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=~/.cache/loom-main-target cargo test -p loom-gateway --locked --offline --test tc1_live_smoke -- --nocapture`
→ **exit 0, 1 passed** (11.8 s and 11.6 s; same result twice). Log: `/private/tmp/claude-501/-Users-ctmctm-workspace-Loom/4c08df9f-98f9-46d9-8649-1e36828e20de/scratchpad/orig3.log`.

| Phase | Actual result |
|---|---|
| Precondition (power OFF) | `uds.request 3E00` → `E_NO_RESPONSE` (the ECU counted an unanswered request). `can.sig.wait_for` 400 ms → `E_TIMEOUT`, 0 samples. UART device bytes: 0. Boots: 0 |
| `test.start` | RUNNING, `tc_sha256 a90459bf…` matches. risk R0×5/R2×1/R3×2 |
| S1 | `done`. Checks S1#0/#1 `verified`/`own_tx_only` (the confirmed off/on write rows). Fixture: `power ON (psu.write effective_verified, readback.out=true)` |
| S2 | boot #1 → `uart.expect` matched. `pass`: check `verified`, witness `device`, `complete:true`, `uart:dut@e…:seq:75` |
| S3 | `pass`: `can.signal_range` `verified`, device, `observed {VehState.PowerMode raw 2 "RUN", db_sha256 ec6c…}` |
| S4 | `dtc.list` response `59 02 FF 92 34 00 09 C1 00 00 2F` → `fail`: check `mismatch`, `2 DTC(s) with status mask 0x09 bits reported` |
| S5 | `14 FF FF FF` → `54`. `dtc.list` → `59 02 FF` → `pass`: `verified`, device |
| `test.end` | **`passed`, exit 0, DONE**, `finally_skipped:true` |
| After the session, same DUT | PSU OFF → fixture: CAN node closed, console dead. `uds.request 22F190` → `E_NO_RESPONSE`. PowerMode wait times out. PSU ON → boot #2 (new U-Boot log), CAN back up, `dtc.list` → `[]` (cleared DTCs persist) |

Other checks: `cargo clippy -p loom-gateway --test tc1_live_smoke --locked --offline -- -D warnings` PASS. `rustfmt --check` (this file) PASS.

## Findings and proposals (other owners)

1. **(Applied by main) UART macOS PTY**: without `uart-sim-pty.patch`, `uart.open` fails with `E_NOT_OPEN "/dev/ttysNNN: Not a typewriter"` (baud ioctl).
2. **(Applied by main) CAN↔TEST args**: `wire.rs` adds `filter`/`ch` to domain predicate args, and `RangeArgs` uses `deny_unknown_fields`. Every `can.signal_range` check was `uncertain`. Check whether `can.frame_match` (`FrameMatch` args) has the same problem: CAN owner, not verified here.
3. **TEST `anchor.where` field check is not scoped by kind** (`wire.rs::anchor_ts` → `Expr::check(&common.specs)`): the first match in the pointer union of all domains wins. So `psu.write /payload/state == "on"` was rejected against UART's `/payload/state` enum (`does not fit field /payload/state (enum closed|opening|…)`). Worked around in the plan with `psu.state /payload/out`. Proposal: check against the pointers of `anchor.kind` (and of the expectation's `filter.kinds`). TEST owner.
4. **UDS quarantine (expected behavior, documented)**: an unanswered request blocks the same SID and the target's next request for ≥ P2* (5 s): `E_QUARANTINE`/`QuarantineQuiet`. That is why the off probes use 3E/22 rather than 19, and the post-session `dtc.list` retries only `E_QUARANTINE`, bounded to 8 s. The TEST session itself has no retry.
5. Existing gap (not mine): the plan `env: sim` versus the daemon/UDS row env (`bench`, host/port connect) is not compared. `can.node.start/stop` are not exposed via the gateway, which is why the fixture uses a shared-bus open/close.
6. Disk: the scratch-copy builds (6 runs, one copy) left path-specific workspace crate artifacts in the shared `~/.cache/loom-main-target` (size not measured). The copy is deleted; no further copies or target dirs were created. The artifacts can be cleaned when main decides to.

## Remaining / next

- Do not run this test in IPC placement (`fence_unflushed` by design). Formal 30-minute RSS and the browser check are separate tracks.
- Pending: commit by main (index belongs to main), integration check with other owners' WIP. `crates/loom-gateway/tests/uds_tc1_smoke.rs` is shown modified by someone else (not mine).
- After this return, the role is released; the session/pane is kept.

## 실제 브라우저 후속: 환경 배지 정합

실제 Ego TC-1 화면은 plan.env=sim인데 fixture daemon의 기본값 bench로 표시됐다. 모든 DUT 백엔드가 sim인 이 시험의 Config.env를 명시적으로 Env::Sim으로 맞췄다. 제품 환경 정책이나 실장비 설정은 바꾸지 않았다. 변경된 실제 TC-1 저널 판정 1 PASS(11.53초), `/private/tmp/loom-tc1-sim-env-test.log`. 최종 브라우저 검증은 재빌드된 test exe와 새 dist를 사용한다.
