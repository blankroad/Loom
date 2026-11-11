# TC-1 live — build/source requirements (for main)

## Cargo (manifest / lock)

**No change needed.** `crates/loom-gateway/tests/tc1_live_smoke.rs` uses only crates that are
already `loom-gateway` dependencies with the default features (`uart can uds psu test`):

| Use in test | Crate | Already in loom-gateway |
|---|---|---|
| `PtyDevice`, `Bootshell`, `Device`, `Splitter`, `LOGIN_PROMPT` | `loom-uart-worker` (`sim`) | `[dependencies]` optional, feature `uart` |
| `DoipEntity`, `EntityConfig`, `SimEcu`, `SimEcuConfig` | via `loom_uds::sim_support` | `[dependencies]` optional, feature `uds` |
| daemon/config/home/policy | `loom-gateway` | self |
| `Rights` | `loom-proto` | `[dependencies]` |
| base64 decode of `uart.data` | `base64` | `[dependencies]` |
| hex decode | `hex` | `[dependencies]` |
| JSON, runtime | `serde_json`, `tokio` | `[dependencies]` |
| temp HOME, WebSocket | `tempfile`, `tungstenite` | `[dev-dependencies]` |

Cargo.lock: unchanged.

## Product source the test needs (other owners — proposals, not applied)

1. **Keep** the uncommitted `loom_uds::sim_support` re-export (`crates/loom-uds/src/lib.rs`,
   UDS owner). The DoIP entity/SimEcu of the DUT come through it.
2. **UART (macOS blocker)** — `reports/tc1-live-p1.uart-sim-pty.patch` (loom-uart-worker, 3
   files). Without it `uart.open` of the fixture pty fails on macOS with
   `E_NOT_OPEN "/dev/ttysNNN: Not a typewriter"`: the daemon's in-process `UartWorker` is not
   reachable for `register_sim_pty`, so it sends the baud ioctl that macOS ptys reject.
   Patch: `PtyDevice::spawn` records its slave path in a bounded (16) process-wide set while the
   device runs (removed on `PtyHandle` drop); `open_inner` treats such a path like a
   `register_sim_pty` path. Only ptys spawned by Loom's sim harness in the same process qualify
   (IPC workers are other processes → unchanged). Linux would not hit the ioctl failure.
3. **CAN verifier ↔ TEST args contract** —
   `reports/tc1-live-p1.can-signal-range-args.patch` (loom-can `signals.rs`; re-apply on
   `condition.rs` if `condition_from_args` moves there). TEST's `wire.rs` adds `filter` and
   `ch` to every domain predicate's args (test-runtime-p1 appendix §2), but `RangeArgs` is
   `deny_unknown_fields` → every plan `can.signal_range` check is `uncertain`
   (`invalid can.signal_range args: unknown field \`ch\``). Patch strips those two keys before
   parsing. (Alternative owner choice: TEST stops injecting them for registered predicates.)

## Validation placement

Main applied 2–3 to the original tree; the scratch copy is deleted. Validated in the original
tree (exit 0, 1 passed — see `tc1-live-p1.md`) with:

```
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=~/.cache/loom-main-target \
  cargo test -p loom-gateway --locked --offline --test tc1_live_smoke -- --nocapture
```
