# ui-active-psu — READY_FOR_REVIEW / SOURCE_FROZEN

Native PSU follow-up complete. Main may rebuild native and produce the next frozen browser snapshot. No further source changes planned.

## Owned changes

- `crates/loom-psu/src/worker.rs`: emit `connected: true` in `Device.state_payload()`, shared by actual `psu.state` events and status. Remove the redundant status-only insertion. A Device represents a connected supply regardless of output OFF/ON; output is never used to infer connectivity.
- `crates/loom-psu/src/gateway.rs`: add `/payload/connected` as a Bool pointer. Keep the existing required-field list and schema version unchanged so older journal states without connected remain valid and unknown.
- `crates/loom-psu/tests/psu.rs`: add `connected_output_off_event_and_reducer_snapshot`. Exercise the real worker through the existing gateway/worker/journal harness, assert the initial output-OFF event explicitly contains connected=true, feed actual events into the reducer, and assert snapshot/status agreement. Replay a legacy event with connected absent and verify its payload remains unchanged/unknown; verify connected is not required and the Bool pointer is registered. Existing state tests retain their meaning.

## Validation

- Before the worker fix, the new regression failed at the actual emitted event: connected was Null, expected true.
- Final `cargo test -p loom-psu --locked --offline`: PASS, 15 integration tests; unit/doc targets also PASS. Includes STOP latch/user emergency OFF, risk classification, limits, interlocks, cycle and gap coverage.
- Final `cargo clippy -p loom-psu --all-targets --locked --offline -- -D warnings`: PASS.
- Final `cargo fmt -p loom-psu -- --check`: PASS.
- `git diff --check -- crates/loom-psu`: PASS.

STOP/output/risk implementation untouched. No full-workspace or browser checks performed here; main owns native rebuild/browser integration. No subagents, security changes, escalations, commits or unrelated file edits.
