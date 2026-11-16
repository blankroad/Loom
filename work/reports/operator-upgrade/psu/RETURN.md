# PSU operator upgrade — RETURN

**READY_FOR_REVIEW** (2026-11-15) — module lead Opus 5.5; product function bodies by `upgrade-psu-sonnet` (B1–B4), each batch reviewed and re-run by Opus.
Worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-psu` @ base 610029e, uncommitted (no Git index operations).
Diff hash (sha256 of `git diff` + the two new files appended as `--- new <path>` + content):
`b506d9d1837d321222c81968d6bcd8f9b7825bc5118c1e46c8131670ed374b43` (19 modified files, 2 new; +2252/−109 in tracked files).
Contracts: PLAN.md, FUNCTIONS.md (§Batch 1–4 with amendments r2/r3), STATUS.md (per-batch evidence), B1–B4-WORKER.md.

## Finding coverage
| ID | Result |
|---|---|
| PSU-01 limits | Done (UI): active `vmax/imax` shown in the setpoint block (`미보고` when not reported); over-limit input flagged before [적용] (no RPC; server refusal stays final guard); connect form shows defaults, refuses a limit above default; CLI preview includes `--vmax/--imax`. |
| PSU-02 chart | Done (UI): wall-clock tick labels from each point's own `ts.wall_ns` (+mono offset); theme-token axis/grid colours with rebuild on change; y range never 0–100 (limit/flat padding); dashed reported-setpoint lines; latest/cursor readout line. |
| PSU-03 history/export | Done (engine + UI): new read RPC `psu.history` (paged, mergeable stats, per-page completeness, `before_index`, `journal_gaps`, `now_ms`), per-supply time→seq anchor index fed by the reducer, CLI `loom psu history`; gauge history section (presets/custom range, coverage line with every partial reason, stats table, CSV ≤ 250 000 rows, "download requested" wording). |
| PSU-04 power cycle | Done (engine + UI): scpi-tcp cycle on the IO thread (OFF verified → monotonic wait with polling → ON verified); ON never sent after STOP (agent/script), a later output write, link loss, disconnect or unconfirmed OFF; sim got the same STOP rule + supersede; truthful `cycling`; gauge `전원 사이클 [OFF ms]` R3 action. |
| PSU-05 labels | Done (UI): `off` chip hidden, `unknown` → `모드 미보고`, `보호 미보고`, accessible names. Excluded: vendor status-register profile (STAT:QUES/OPER) — vendor-specific bits, no instrument/profile in scope (external dependency). |
| PSU-06 two supplies | Finding corrected by evidence: daemon already runs one PSU worker per alias (≤ 4) (`r1 channels_reducer_and_per_channel_workers`). UI copy fixed. Excluded: multi-output instruments. |
| PSU-07 empty state | Done (UI): unbound tile collapsed to one placeholder line after the connect form; no disabled emergency button / empty plots while unbound. |
| R2 (ROOT-assigned) | **Safety defect found and fixed**: declared emergency OFF failed (`out needs state on|off`) and channel-less all-off confirmed `pb` OFF while it stayed ON (writes routed to `pa`'s worker). PSU-owned fix: `worker_for(ch)` routing, `targets` support, `E_PARTIAL` on partial all-off, worker refuses a foreign `ch`. Regression tests in `crates/loom-gateway/tests/r1.rs`. |

## Exact results (Opus final run, this worktree)
UI (`ui/`): `npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo` → 0; node counterpart → 0;
`npx eslint src/panels/psu` → 0; `npx vitest run src/panels/psu src/shell src/sdk` → 28 files, 297 passed (PSU: 67);
`npx tsx scripts/gen-schema.ts --check` → `schema ok: 33 schemas, sha256 b43d8c97…7c0857` (no regeneration needed).
Rust (`CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2`): `loom-psu --test psu` 33 passed; `--test scpi` 16;
`--lib` 2; `clippy -p loom-psu --all-targets -D warnings` clean; `loom-gateway --test r1` 8 passed; `--test preview_policy_smoke` 2 passed.
Generated side effects: `work/typecheck/*.tsbuildinfo` (ignored, local). No Cargo.lock / package manifest change. No new dependencies.

## Changed files
Engine: `crates/loom-psu/src/{model.rs,cli.rs,scpi.rs,worker.rs,gateway.rs}`, `crates/loom-psu/src/gateway/{common.rs,evidence.rs,handlers.rs,history.rs(new)}`.
UI: `ui/src/panels/psu/{psuModel.ts,gaugePanel.ts,psuConnect.ts,psuHistory.ts(new),psu.css}`.
Tests: `crates/loom-psu/tests/{psu.rs,scpi.rs,support/mod.rs}`, `ui/src/panels/psu/{psu.test.ts,psuModule.test.ts}`, shared test (ROOT-assigned) `crates/loom-gateway/tests/r1.rs`.
Docs: `skills/loom-psu/SKILL.md`.

## Shared / integration notes for ROOT
- New domain RPC `psu.history` (READ role, optional params) and panel action `cycle` + query `psu.history` in both `registerPanel` and gateway `panel_manifest` (action order now: emergencyOff, output, set, cycle, connect, disconnect). Combined catalogs (if any outside the UI schema) regenerate at integration.
- Mock daemon `ui/tools/mock/psu.ts` not updated (deferred per ROOT); final proof on the real sim daemon.
- Wire behaviour changes: channel-less `psu.out{state:off}` with any unconfirmed supply now returns `E_PARTIAL` (was a false overall success); scpi-tcp `cycle` is now executed (was `E_NOT_IMPLEMENTED`); `psu.state.cycling` is truthful for scpi-tcp.

## Risks / remaining (GUI and integration — ROOT only)
1. uPlot wiring cannot run in jsdom: hover readout (`커서 …`), wall-clock ticks, light/dark grid rebuild, y range on non-zero sim data, dashed setpoint lines — GUI check on sim.
2. `psu.history` durable path beyond the 4 MiB/channel ring is exercised only with the in-test FakeJournal: run `loom psu history <alias> --last 15m` (and > 30 min of sim data if time allows) on the real sim daemon; confirm `complete`/`journal_gaps`.
3. History index lives in gateway memory: after a daemon restart, older windows report `before_index` (truthful, not complete). Wall-clock step back resets the index (flagged `dropped`).
4. Emergency-OFF fan-out is serial per supply (one write each); with 4 supplies latency adds up (bounded by each write's deadline).
5. scpi-tcp cycle verified only against the in-test loopback server; no physical instrument claimed.
