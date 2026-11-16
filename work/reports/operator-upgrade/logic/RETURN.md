# Logic operator upgrade — RETURN
**READY_FOR_REVIEW** · 2026-11-15 · lead upgrade-logic-opus (Opus 5.5) · worker upgrade-logic-sonnet (function bodies only) · baseline 610029e.
**Worktree:** `/Users/ctmctm/workspace/Loom/.worktrees/operator-logic` (uncommitted; no Git operations were done).
**Contracts:** PLAN.md · FUNCTIONS.md (frozen per batch, hashes inside) · per-batch WORKER/REVIEW files (B1, B2A, B2B, B3, B4R, B5R, B45U).

## Finding coverage
| ID | Delivered | Evidence |
|---|---|---|
| L1-R1 pre-start overload warning | `estimatePublishLoad`; warning list (never blocks) + archive note | logicBatch1, logicBatch3b |
| L1-R2 whole evidence vs bounded preview | Capture archive: lossless D0–D7 record + every parsed decoder line before the rate limit. Quota/age/count rotation. Sticky partial at an exact sample/row. Decoder gap marked partial. `logic.archive.list/read`, `logic.export` (daemon-side path), CLI `archive list/read`, `export`. UI archive view (paged rows, windowed edges, not-loaded hatching), status chip, export | tests/archive.rs 16, archive_engine 3, cli_archive, logicBatch3b |
| L1-R3 row coalescing | **Excluded:** it changes the row contract that `wait --ascii` and readers use; the archive provides completeness instead | PLAN §2 |
| L1-M measured load | UART 115200 continuous 10 s: live preview misses 78.5 %, archive complete (111,390 rows, every sample). SPI 1 MHz @10 MS/s: decoder backpressure → capture `failed` at 1.64 s, archive partial (decode gap), raw complete to the stop | B2B-REVIEW + STATUS (re-run after B5r) |
| L2 decoder options | All backend UART/SPI options in the form; non-default-only spec; synthetic limits mirrored; saved-config migration | logicBatch1 + Rust ui_decoder_specs (shared fixture) |
| L3 radix/search/lines/copy/class | HEX/ASCII/DEC/BIN; CR/LF/CRLF/lone-CR line view; byte/text search with n/m and jump; clipboard TXT/CSV with range/coverage/source metadata; empty class column hidden | logicBatch3a |
| L4 cursors/measurement | A/B cursors (click, Shift+click, 1/2, Esc, row/line/search span), Δ/Δt/1/Δt, hover pulse width/period/duty from real transitions only | logicBatch3a |
| L5-R1 held vs engine | `decodeCoverage` / `rawCoverage`; the stop notice and table info name exactly what is missing | logicBatch1 |
| L5-R2 backfill | Archive view (B3b): the whole decoded history and raw windows from the archive, labelled as archive evidence | logicBatch3b |
| L6-1/2 UART TX+RX, error rows | `uart:rx=Da:tx=Db` → two sigrok stacks (uart-1 RX, uart-2 TX); `decoder_tracks`; Loom-derived row `role` data/error (Frame/Parity error, break); `counters.error_rows`; synthetic duplex + `glitch_every`; UI form, track labels, highlighted error rows, counter | tests/duplex.rs (real sigrok-cli), logicBatch45 |
| L6-3 multiple user decoders | **Excluded (scope);** stays E_NOT_IMPLEMENTED | PLAN §2 |
| L7-1/2 software trigger | `trigger {channel, edge, pre_ms, post_ms}` for any source. While armed nothing is published/decoded/archived; on fire, the pre-window is published on the absolute axis; stop reason `trigger_post`; `not_fired` is explicit. CLI `--trigger/--pre/--post`; UI form, chip, armed empty text, `T` marker | tests/trigger.rs (real sigrok-cli), logicBatch45 |
| L7-3 hardware trigger | **External dependency:** fx2lafw physical verification | — |
| L8a/b/c live/layout | Follow anchored at the raw end; axis header clipped to the gutter; config collapses once on reopen; follow turns off at stop | logicBatch1 |

## Final verification (lead, isolated worktree, 2026-11-15)
- **UI** (from `ui/`):
  - `vitest run src/panels/logic`: **100/100** (6 files)
  - `tsc -p tsconfig.app.json` / `tsconfig.node.json` with `--tsBuildInfoFile ../work/typecheck/{app,node}.tsbuildinfo`: 0 / 0
  - `eslint src/panels/logic`: 0
  - consumers `shell/logicWiring`, `moduleApps`, `serviceSeparation`, `sdk/capacityLive`: **19/19**
- **Rust** (`CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2`):
  - `cargo test -p loom-logic --no-fail-fast`: **66 passed, 0 failed** (unit 15, archive 16, archive_engine 3, cli_archive 4, duplex 6, engine 10, gateway 4, trigger 6, ui_decoder_specs 2); load 2 ignored, run separately in release, numbers above
  - `cargo clippy -p loom-logic --all-targets -- -D warnings`: 0
  - `cargo check -p loom-logic --no-default-features --features cli`: OK
- No stubs (`unimplemented!(` / "Sonnet body pending") remain.
- **Not done here:** GUI, daemon/desktop bundle, shared catalogs, full UI/Rust workspace suites, physical hardware.

## Changed files (module-owned only)
- **Tracked:** `git diff 610029e -- crates/loom-logic skills/loom-logic ui/src/panels/logic`: 12 files, +2658/−165, sha256 `04ea4d765cdd3b7dfa2c24e425b57bf396ee349e1e1249e565d57b36f9c2d8e9`. Files: cli.rs, gateway.rs, lib.rs, model.rs, worker.rs, tests/engine.rs, SKILL.md, logic.css, logic.test.ts, logicModel.ts, logicPanel.ts, waveform.ts.
- **New (untracked, 14 files, 5434 lines):** src/archive.rs; tests archive, archive_engine, cli_archive, duplex, load, trigger, ui_decoder_specs, fixtures/ui-decoder-specs.json; ui logicTools.ts, logicBatch1/3a/3b/45.test.ts. Per-file sha256 in the B45U handoff; combined (tracked diff + untracked sha list) `309f65ce80331cbdd5bcb80f672ac18dec7859fe36f08901d38bd6f00ee6e064`.
- **Not for integration:** `work/reports/operator-upgrade/**` in the worktree (local contract copies) and `work/typecheck/` (tsc caches).

## Shared / integration notes for ROOT
- **No shared file edits.**
  - New domain RPCs: `logic.archive.list`, `logic.archive.read` (READ), `logic.export` (request class, R0, READ roles, audited).
  - New optional start keys: `glitch_every`, `trigger`.
  - New optional capture_state keys: `archive`, `decoder_tracks`, `trigger`, `counters.error_rows`.
  - New annotation row key: `role`.
  - All additive; older readers ignore them.
  - No generated catalog references logic RPCs today; combined registration/schema integration is ROOT's.
- **Panel manifest** (module-owned): queries `+logic.archive.list`, `+logic.archive.read`; action `export` (`logic.export`, right watch, request).
- **Storage:** the archive lives under `<LOOM_HOME>/logic-archive/` via `CallCtx::storage_roots()`, the existing video precedent. With no roots it is disabled and the UI says so. `[logic]` keys: `archive`, `archive_max_capture_mb` (256), `archive_max_total_mb` (2048), `archive_max_age_h` (168), `archive_max_captures` (32).
- **No new dependencies.** Cargo.lock is unchanged by this module.
- **Incident (recorded):** my first typecheck wrote `ui/node_modules/.tmp/tsconfig.app.tsbuildinfo` in the shared store (a generated cache). All later checks used isolated caches.

## Decisions / risks for ROOT
1. **Decoder overrun policy:** at dense 10 MS/s SPI the libsigrok decoder cannot keep up. The current contract stops the capture (`failed`); the archive keeps raw and says the decode is partial. The option "continue raw-only on decoder overrun" is not implemented because it changes the capture error contract.
2. **Archive reads** in the pathological every-sample-toggle case may overlap by one sample. The UI refuses that chunk and counts it under `형식 오류` (visible, not silent).
3. **Archive defaults** use up to 2 GiB under LOOM_HOME by default; adjust the `[logic]` quotas if needed.

## ROOT simulator / GUI checks (ROOT only; no device IO needed)
1. **L5:** reopen a stopped synthetic capture → the stop notice names the missing rows/raw part; the archive view shows the whole history (`아카이브에서 불러옴 (전체 N행)`).
2. **L1:** synthetic text 256 B / 25 ms / 115200 / 10 s → two load warnings + archive note before start; `디코드 손실` grows live; the archive chip ends `아카이브 완전 · 디코드 N행 …`; export prints a daemon path.
3. **L6:** UART line `RX + TX`, glitch 5 → tracks `uart-1 RX D0` / `uart-2 TX D1`; red error rows; `디코드 오류 N`.
4. **L7:** trigger D1 falling, pre 1 ms, post 120 ms on the duplex synthetic → chip `트리거 대기 …` → `트리거 sample …`; `T` marker; stop reason `트리거 후 지정 길이 도달`.
5. **L3/L4:** ASCII radix, line view, search `LOOM` with Enter, copy, A/B cursors, hover pulse readout.
6. **L8:** live follow shows raw lanes; the axis header does not overlap `0 µs`.
