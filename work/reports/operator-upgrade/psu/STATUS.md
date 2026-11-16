# PSU operator upgrade — STATUS

LEASE → ROOT (B6 imported, hash 561f6ce9…7e1c) — 2026-11-15T21:40:31Z · source/test frozen · no module IO · runtime proof accepted by ROOT · own outputs OFF, fixtures A 61218 / B 61220 sampling until ROOT final cleanup (self-exit 22:39:15Z) · B6 CSV/datetime recheck at ROOT all-seven build
B6 READY_FOR_REVIEW — 2026-11-15T21:39:30Z · ACCEPTED (Opus re-run) · RETURN-B6.md · incremental on FIRST3+B5 sha256 561f6ce9…7e1c (5 files) · full state 0fdb6d59…e07e
B6 LEASED — 2026-11-15T21:37:58Z · history provenance (ref e<epoch>:seq:<seq>, generation, clock_id; CSV +3 columns; CSS datetime width) · FUNCTIONS.md sha256 f9329e4a275a65620a8197b0a3989c0dcf793fcf1d8b3aada18314ccf33f94ea · runtime: watch scoped OFF PASS (w8), own outputs OFF, fixtures sampling
B5 READY_FOR_REVIEW — 2026-11-15T21:26:07Z · 5a+5b+B5-F ACCEPTED · RETURN-B5.md · incremental vs FIRST3 sha256 dd1518ea…1264 (17 files) · full state 01519b3b…e99a · UI watch emergency OFF still FAILED until a new build is proven
B5a ACCEPTED · B5b LEASED — 2026-11-15T21:19:31Z · FUNCTIONS.md sha256 43653295ac945b799b5ddfd57b7a76a512add88a270c43940853ad263c4280fa (§B5.7 r2: F5 tolerance with partial/resume/bytes, malformed→unchanged, non-advancing guard; B5-U2 tooltip)
B5a LEASED — 2026-11-15T21:13:33Z · FUNCTIONS.md §Batch 5 sha256 8aece29f6082f2b453eae8b464a5a6d38df38516e2b523f01f4ff7f99c77ad2b · Sonnet bodies: R5-OFF (psu.off fixed-OFF, watch), F1/F2 connect config, F3 scan resume (+ SKILL), F4 last-verified wording · 5b (F5 durable-path E_SCAN_LIMIT) awaiting ROOT authorization · runtime: own fixtures agent-cleanup OFF, connected, sampling
LEASE → ROOT — 2026-11-16 · ROOT integrating frozen source (diff sha256 b506d9d1…374b43, re-verified unchanged). Module lead makes no source/test edits until a ROOT correction request. Module-level pass only; fixture/native/combined checks ROOT pending.

READY_FOR_REVIEW — 2026-11-15 · B1–B4 ACCEPTED · B5 docs done · see RETURN.md (diff sha256 b506d9d1…374b43)

- Plan: /Users/ctmctm/workspace/Loom/work/reports/operator-upgrade/psu/PLAN.md (all 7 findings covered; PSU-06 corrected by test evidence)
- Batch 1 contract: /Users/ctmctm/workspace/Loom/work/reports/operator-upgrade/psu/FUNCTIONS.md §Batch 1
- B1 Sonnet lease: ui/src/panels/psu/{psuModel.ts,gaugePanel.ts,psuConnect.ts} — bodies only (allowlist B1-M1…M15, B1-C1…C3, B1-G1…G9)
- Opus-done before lease: types/signatures (PENDING bodies), psu.css rules, comments, spec tests (psu.test.ts, psuModule.test.ts)
- Red baseline (Opus, this worktree): `npx vitest run src/panels/psu` → 20 failed / 32 passed; `npx tsc -b --pretty false` → clean
- Evidence PSU-06: `cargo test -p loom-gateway --test r1 channels_reducer_and_per_channel_workers` → 1 passed
- Opus next (non-leased files only): R2 all-off regression test in crates/loom-gateway/tests/r1.rs; B2/B3 Rust contracts + spec tests
- No subagents spawned. No Git index ops, no GUI, no device IO.

## R2 (ROOT-assigned) — all-off with PerChannel workers: DEFECT FOUND (2026-11-15)
- Tests appended (shared test file, ROOT-assigned): crates/loom-gateway/tests/r1.rs
  `psu_emergency_off_reaches_every_per_channel_worker`, `psu_channelless_out_off_reaches_every_per_channel_worker`
- Run: `CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo test -p loom-gateway --test r1 psu_` → 2 FAILED (expected red until B2)
  1. emergency_off: safe-state item `failed`, reason `out needs state on|off` — daemon sends `psu.out{targets:["*"]}` (EmergencyOffSpec contract); PSU `Out` handler requires `state`.
  2. channel-less `psu.out{state:off}`: both results `effective_verified`/`out:false`, but `pb` stays ON (status pa=false, pb=true). Cause: PSU handlers use `ctx.worker(kind)` bound to the call channel (None) → pool.get(None) = any running instance → both writes hit `pa`'s worker, which ignores `op.ch`. **False success on a safety action.**
- Fix is PSU-owned (no shared production change needed: daemon already implements `CtxServices::worker_for`, used by UART): handlers route by resolved ch, Out accepts `targets`, worker refuses a foreign `ch`. Scheduled as B2 (FUNCTIONS.md §Batch 2).

## B1 review — ACCEPTED (Opus, 2026-11-15)
- Worker return: work/reports/operator-upgrade/psu/B1-WORKER.md (READY_FOR_REVIEW); lease returned.
- Diff reviewed against FUNCTIONS §Batch 1: M1–M15, C1–C3, G1–G9 match; import block per Amendment r2 (clockLabel dropped). No extra RPC/timer/export.
- Opus re-run (this worktree, ui/): isolated `npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo` exit 0; node counterpart exit 0; `npx eslint src/panels/psu` exit 0; `npx vitest run src/panels/psu src/shell/moduleApps.test.tsx src/sdk/capacityLive.test.ts` → 4 files, 63 passed.
- Post-review Opus edit: two worker-note comments in psuModel.ts Series reworded (comments only).
- GUI-only checks pending (ROOT): uPlot hover readout, wall-clock ticks, light/dark grid, y range on sim.
- Process rule noted: future amendments never authorize Sonnet outside product function bodies (imports/fields stay Opus).

## B2 contract ready (2026-11-15)
- Scope: R2 all-off routing fix (PSU-owned) + PSU-04 engine (scpi cycle, sim STOP-during-OFF, supersede, truthful `cycling`).
- Lease: crates/loom-psu/src/{gateway/handlers.rs,worker.rs,scpi.rs} — bodies only (B2-H1…H6, W1…W5, S1…S7).
- Opus skeleton (types/fields/variants/signatures with todo!(), doc/help text, test support worker_for probe) compiled: `cargo test -p loom-psu --no-run` ok (7 expected dead-code warnings).
- Red baseline: tests/psu 15 passed / 9 failed; tests/scpi 8 passed / 8 failed; gateway r1 `psu_` 2 failed.

## B2 review — ACCEPTED (Opus, 2026-11-15)
- Worker return: work/reports/operator-upgrade/psu/B2-WORKER.md; lease returned.
- Diff reviewed vs FUNCTIONS §Batch 2: H1–H6, W1–W5, S1–S7 match (routing via worker_for, foreign-ch guard before any device action,
  E_PARTIAL for partial all-off, cycle OFF-verify/pending/ON with one terminal row per path, cycling cleared on every path,
  supersede only after STOP/link checks, exit-path drop before queued answers).
- Opus fix (own test file): tests/scpi.rs `terminal()` `.filter().last()` → `.rfind()` (clippy double_ended_iterator_last).
- Opus re-run (CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target, JOBS=2): loom-psu --test psu 24 passed; --test scpi 16 passed;
  --lib 2 passed; clippy -p loom-psu --all-targets -D warnings clean; loom-gateway --test r1 8 passed (incl. both R2 psu_ tests);
  loom-gateway --test preview_policy_smoke 2 passed.
- R2 defect FIXED in PSU-owned code; no shared production change requested.

## B3 contract ready + assigned (2026-11-15)
- Scope PSU-03 engine: `psu.history` read RPC (domain-owned schema), anchor index fed by reducer, paged scan, CLI `history`.
- Lease: crates/loom-psu/src/gateway/{common.rs,history.rs(new),evidence.rs}, crates/loom-psu/src/cli.rs — bodies only.
- Opus skeleton/schema/registration/clap/imports/tests in place; `cargo test -p loom-psu --no-run` ok.
- Red baseline: tests/psu 24 passed / 9 failed (all b3_*); `registration_is_complete` updated for the new method.
- Shared impact (for ROOT integration): new RPC `psu.history` (READ role, optional params only) → generated catalogs/schema regenerate at integration.
- B3 worker gap (SimClock wall ahead of SystemTime → future samples clamped away): Opus decision = contract r2 (§B3.4a): explicit `to_ms` not clamped, result adds `now_ms`; tests updated by Opus; FUNCTIONS.md sha256 0094b987…5ac57b. Worker re-running.
- B3 r3: TimeIndex::seek compares at ms resolution (worker gap: ms window vs ns anchor). FUNCTIONS sha256 75e4409bf7ab308b…

## B3 review — ACCEPTED (Opus, 2026-11-15)
- Return: B3-WORKER.md (r3). Diff reviewed vs §Batch 3 (+r2, r3): I1–I3, P1, R1–R2, H1–H3, W1, F1, Q1, C1 match.
- Opus edit: removed temporary `#[allow(dead_code)]` on evidence.rs `anchor_of`.
- Opus re-run: loom-psu --test psu 33 passed; --test scpi 16; --lib 2; clippy --all-targets -D warnings clean; loom-gateway --test r1 8 passed.
- Follow-up queued for B4 lease (B4-X1): history_window empty-window message still says "(to is clamped to now)" — stale since r2; text-only fix.
- ROOT integration check requested: `loom psu history <alias> --last 15m` on the real sim daemon (durable `seq:` path beyond the 4 MiB ring is only exercised by FakeJournal here).

## B4 contract ready + assigned (2026-11-15)
- Scope: PSU-03 UI history section/CSV (psuHistory.ts new), PSU-04 UI cycle control, B4-X1 message fix.
- Opus static metadata done: registerPanel + gateway panel_manifest (`cycle` action after `set`, `psu.history` query), psu.css, imports, skeleton, tests.
- Red baseline: `npx vitest run src/panels/psu` → 51 passed / 16 failed (14 new + 2 updated); isolated app tsc exit 0; cargo --test psu 33 passed (manifest test updated).
- B5 (Opus, docs only): skills/loom-psu/SKILL.md updated — scpi-tcp connect + limits, multi-alias (≤4, PerChannel), `history` paging/merge/completeness rules, scpi cycle semantics (ON not sent cases), E_PARTIAL all-off. No product code.

## B4 review — ACCEPTED (Opus, 2026-11-15)
- Return B4-WORKER.md. Worker gap: csvText test expected coverage always quoted; Opus decision = test was wrong (RFC 4180: quote only when needed); test fixed + partial (comma) case added. No contract change.
- Diff reviewed vs §Batch 4 (M1–M10, D1, G10–G12, X1) — match.
- Final Opus run: see RETURN.md (UI 297 passed incl. PSU 67, isolated tsc app/node 0, eslint 0, schema ok; Rust psu 33, scpi 16, lib 2, clippy clean, r1 8, preview_policy_smoke 2).
