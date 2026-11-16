# CAN operator upgrade — STATUS

**READY_TO_INTEGRATE** (2026-11-16) · ROOT imported the accepted scopes; worktree frozen (lease = ROOT) · **RUNTIME (PRE_FINAL7) RUN** — DBC PASS · export PASS · periodic BLOCKED by core gate (CAN-RT-01) · CAN-RT-02 correction candidate · marker not run (awaits GO) — RUNTIME.md · **PAUSED** for ROOT's daemon restart (no writes in flight, none unknown; channels kept open; plan R0–R7 in RUNTIME-PLAN.md addendum) — all batches B1–B5 accepted by Opus review. Worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-can`, branch operator/can, baseline 610029e. No Git/index, GUI, default daemon or device IO was touched.

## Coverage (RETURN-can.md)

| Finding | Result |
|---|---|
| CAN-1 time column | B1: wall (fmtWall, UART parity) / relative (session start or selected row) / Δ to the previous shown row; only between rows of the same `clock_id` (`≠clock` otherwise); missing wall → `--:--:--.---`, never derived from mono. Receive header `마지막 s (창 시작 기준)` + wall-clock title. |
| CAN-3 TX list persistence | B1: per-channel localStorage (this browser), restored on mount; `loom.can.txlist` v1 JSON copy/paste (cansend text accepted by `loom can send` and the send line); `cycle_ms` carried. Team/daemon storage excluded (no shared owner). |
| CAN-7 full data | B2: data-cell title = full bytes; row detail line (Enter / double-click, ↑/↓ selection) with copy. Column resize excluded. |
| CAN-8 id ranges | B2: `a-b` ranges, 0x/bare-hex/x-suffix, ≤256 ids + ≤64 ranges, normalised `적용:` chip. Bare digits stay hex (CLI: decimal) — the chip shows what was applied. |
| CAN-6 channel close | B2: `채널 닫기…` → one read of `can.periodic.list` + `can.status` → impact preview (unknown ≠ none) → `can.close` behind a confirm card; `채널 닫힘` badge from `can.channel_state`; view-close difference stated. |
| CAN-2 periodic | B3: Cycle Time ms in the editor; `주기 시작/정지` on TX entries → `can.periodic.start/stop` server jobs (no browser timers, no renew loop); job table of all owners (one list at mount + refresh + live `can.job_state`); others read-only, `넘겨받기` = card-confirmed `can.periodic.takeover`; STOP banner, starts disabled. |
| CAN-4 export | B4: new RPC `can.export` (read role, streaming, ≤1,000,000 frames / 4,096 scans / 600 s; one clock per file; journal gaps → Gap records; partial → fsync → rename; never overwrites); CLI `loom can export`; panel `내보내기…` box (ring / last 10 min, optional current filter, daemon path) — result says "daemon machine, not a browser download". |
| CAN-5 DBC | B5: `decode` added to `can.trace.query`/`can.last` schemas (fixes `--decode` vs `additionalProperties:false`); DB chip (`DB: 없음 / ? / file · sha8`), load box (absolute daemon path, operate as mistake prevention), Receive `메시지 (DB)` column from bounded catalog pages, `디코드` in the row detail (one trace query with decode). |
| §3 engine-unsupported | Excluded, unchanged honest labels. |

## Accepted batches (worker result hashes, verified)
B1 `19fcf750…d330` · B2a `69059e55…61f1` · B2b `f8196029…bee2` · B2c `8300872d…9808` · B4a `a5844a2e…94ae` · B4b `1424929c…6950` · B3a `e895097f…3783` · B3b `1dbd6813…3b6f` · B5a `e7674e1c…99cb` · B5b `62e58200…610d`. Details per batch in FUNCTIONS.md (§*-REVIEW).

## Final validation (Opus, focused, isolated)
- UI (in `ui/`, `--maxWorkers=2`): `vitest run src/panels/can` **239/239** (23 files); named consumers `sdk/m1Sdk`, `shell/{channelExplorer,m1b,moduleApps,routes,serviceSeparation}`, `shell/dock/{activeWorkspace,defaultLayout,dock}` **119/119**; isolated `tsc -p tsconfig.app.json` 0, `tsconfig.node.json` 0; `eslint src/panels/can` 0.
- Rust (`CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2`): `cargo test -p loom-can -p loom-can-export --no-fail-fast` — loom-can lib 8, cli 16, export 9, gateway 24, generation_tail 4, isotp 15, periodic 16, signals 23, worker 17; loom-can-export lib 1, formats 7, recovery 7, volume 1 — all pass (volume failed once in the parallel run at host load ≈29 and passed on rerun in 9.6 s; the crate is unchanged → load-sensitive, not a regression). `cargo clippy -p loom-can -p loom-can-export --tests -- -D warnings` clean; `cargo fmt --check` clean.

## Change set
- Final diff hash: `(git diff; for f in $(git ls-files --others --exclude-standard | grep -v "^ui/node_modules\|^work/" | sort); do echo "== $f"; cat "$f"; done) | shasum -a 256` = `c0ccf498526143eedf2ed506f61792f12f84ea8ca4f455f417934174096a406d`.
- Modified (17): Cargo.lock; crates/loom-can/{Cargo.toml, src/cli.rs, src/gateway.rs, src/lib.rs, tests/cli.rs, tests/gateway.rs}; skills/loom-can/SKILL.md; ui/src/panels/can/{can.css, canTrace.ts, canTrace.test.ts, rxSummary.ts, rxtxPanel.test.ts, rxtxView.ts, traceModel.ts, traceView.ts, txList.ts}.
- New (28): crates/loom-can/src/export.rs, crates/loom-can/tests/export.rs; ui/src/panels/can/{kv, timeFmt, txStore, idFilter, rowDetail, closeChannel, exportReq, periodicJobs, periodicView, dbView}.ts and specs {timeFmt, txStore, traceTime, canTime, txPersist, idFilter, rowDetail, closeChannel, canB2Panel, exportReq, canB4Panel, periodicJobs, periodicView, canB3Panel, dbView, canB5Panel}.test.ts.
- Not for commit: `ui/node_modules` (shared symlink, pre-existing), `work/typecheck/` (isolated tsc build info), `work/reports/…` (reports; canonical copies in the main repo).

## Requests to ROOT
1. **Cargo.lock**: +1 line (`loom-can-export` in loom-can's dependency list) — generated by cargo for the new path dep; serialize at integration.
2. If a combined method/help catalog exists outside crates/loom-can, regenerate it for `can.export` and the `decode` schema fields (none found inside this worktree).
3. GUI acceptance (ROOT only, simulator): time modes; TX list across close/reopen + JSON copy/paste; id ranges + chip; detail/keyboard; close preview/card/badge; periodic start/stop/takeover with an agent CLI job; export file on the daemon disk (BLF/ASC + sidecar, CLI parity); DBC load/chip/names/decode with a fixture DBC.

## Risks / notes
- "Mine" for periodic jobs = started/taken over by this panel in this session (the panel cannot learn its daemon client id); after a reload, own jobs show as other owners and need an explicit takeover. Editing a running TX entry does not patch its job (stop + start). The engine's `origin=user` ownership bypass is never used implicitly.
- TX list storage is per browser; two panels on one channel = last writer wins; a corrupt stored list shows an error and is replaced only by the user's next edit.
- `can.export` writes synchronously inside the async handler (same as `eth.export`); bounded by frames/scans/600 s. One clock per file by design.
- The bound panel now makes two explicit state reads at mount (`can.periodic.list`, `can.db.info`); no trace replay or send.
- Load-sensitive tests observed once each under heavy host load (one CAN UI test during parallel cargo, `loom-can-export` volume): both pass on rerun; ROOT's `--maxWorkers=2` rule applies.
