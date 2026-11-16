# Logic operator upgrade — PLAN (v1)

**Status: READY_FOR_REVIEW** (all batches accepted; see RETURN.md) · baseline 610029e · module lead foreground Opus 5.5 · 2026-11-15
Worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-logic` · input `work/reports/usability-benchmark/RETURN-logic.md` · constraints `../COMMON.md` v1 (incl. the "함수 구현만 소넷이" clarification).
Function contracts: `FUNCTIONS.md` (B1 is complete; B2–B5 are specified here at the architecture level and get full contracts before their lease).

## 0. Role split (applied)
- **Opus (me):** architecture, types, signatures, top-level imports/constants/option tables, CSS, fixtures, tests, review. Already written for B1: types, constants and skeleton signatures in `logicModel.ts`, `AXIS_HEADER` in `waveform.ts`, imports and option tables in `logicPanel.ts`, CSS in `logic.css`, `logicBatch1.test.ts`, the Rust contract test `tests/ui_decoder_specs.rs` and its shared fixture.
- **Sonnet:** only the function and callback bodies listed in FUNCTIONS.md §B1 allowlist, including the function-local DOM construction inside `createLogicAnalyzer`. While it holds the lease on a file, I don't edit that file. A missing import, type or helper goes back to me with the reason. Sonnet doesn't change tests.

## 1. Evidence I added during planning (stdin decode only, no device)
Local `sigrok-cli 0.7.2` / libsigrokdecode 0.5.3, synthetic binary on stdin (scratchpad):
- `--protocol-decoder-ann-class` **does not exist** in 0.7.2, so text output never carries the annotation class.
- One PD instance with `rx=`+`tx=` prints both lines as `uart-1` with no direction. **Two `-P` stacks** print `uart-1` / `uart-2` separately, interleaved and streamed incrementally. This is the L6 design.
- `-A uart=rx-data:rx-warnings:rx-parity-err` adds `Frame error` / `Parity error` rows. Data rows stay hex (`format=hex` default; 9-bit → 3 hex digits `141`).
- 1 s of continuous 115200 8N1 at 1 MS/s: 11,519 rows decoded in 0.50 s wall. So the decoder keeps up (≈2×) and the live publish budget (2,000 rows/s) really is the limiter in L1. This is a single run on this host, not a load acceptance; that is B2-T-load.

## 2. Finding coverage
| ID | Item | Decision | Batch |
|---|---|---|---|
| L1-R1 | pre-start overload warning | `estimatePublishLoad` + separate warning list (never blocks) | **B1** |
| L1-R2 | whole-evidence archive vs bounded live preview | worker-side capture archive (lossless raw RLE of D0–D7 + every parsed decoder line before rate limit/queue drop), quotas, sticky partial marking, `logic.archive.*` RPCs | **B2** (engine) + B3 (UI) |
| L1-R3 | coalesce byte runs/lines into one published row | **Excluded.** It would change the `logic.annotations` row contract that `logic.wait --ascii`, the reducer and journal readers use. Completeness comes from the archive (L1-R2) instead, and the line view is derived client-side (L3). Live caps are kept as live protection (finding's own caution). | — |
| L1-M | measured load acceptance | `tests/load.rs` (explicit `--ignored`) on synthetic + real sigrok-cli stdin: published+dropped == archived == decoded, raw archive sample count == acquired | B2 |
| L2 | decoder options in GUI | all backend UART/SPI options, non-default-only spec emission, synthetic limits mirrored, saved-config migration | **B1** |
| L3-1..5 | radix / search+next / CR·LF·CR-only line view / CSV·TXT copy (selection, all) / hide empty class column | new pure `logicText.ts` + table wiring; copy header states range/completeness/source (panel-held vs archive) | B3 |
| L4-1..3 | 2 cursors (keys 1/2, click / Shift-click) with Δt, 1/Δt, Δsample; hover pulse width/period; row selection → cursors | pure measurement fns in `waveform.ts`/`logicModel.ts`, painter draws A/B | B3 |
| L5-R1 | held vs engine mismatch shown | `decodeCoverage`/`rawCoverage`/`coverageNote`/`annCoverageText`; stop notice no longer claims completeness | **B1** |
| L5-R2 | bounded backfill on reopen | [아카이브에서 불러오기] via `logic.archive.read` paging into the same `CaptureData` (dedupe by span/key); journal-only fallback is labelled partial | B3 (needs B2) |
| L5-cause | 1,515 vs 1,365 root cause | candidates: reducer snapshot 200 rows + `mirrorRows` 4,000 backlog window. B1 makes it visible regardless; ROOT GUI check §5-1 confirms | B1 visible / ROOT check |
| L6-1 | UART TX+RX in one capture | `uart:rx=Da:tx=Db` → two `-P` stacks (uart-1 = RX, uart-2 = TX), `decoder_tracks` in capture_state, UI line `both` | B4 |
| L6-2 | framing/parity/break errors | `-A` adds `*-warnings,*-parity-err,*-break`; `format=hex` pinned; Loom-derived row field `role: data|error` (rule: `^[0-9A-F]{1,3}$` → data). `class` stays null (not invented); table/track highlight; ASCII matcher breaks runs on error rows (already: non-hex clears) | B4 |
| L6-3 | multiple user decoders | **Excluded (scope).** User-facing `decoders.len() ≤ 1` stays `E_NOT_IMPLEMENTED`. The cited scenario is covered by L6-1. I2C etc. are outside the TASK-allowed engine scope. | — |
| L7-1/2 | software trigger, pre/post budget, marker | start param `trigger {channel, edge rising/falling/either, pre_ms ≤1000, post_ms}`; pre ring ≤16 MiB; publish/decode/archive start at pre-start with **absolute sample axis kept**; decoder sample base offset; capture_state `trigger {state armed/fired/not_fired, sample}`; UI form + marker | B5 |
| L7-3 | hardware `--triggers` | **External dependency:** fx2lafw physical verification, not done here | — |
| L8a | live raw lanes empty | follow anchor = received raw end (`followHeadSample`) | **B1** |
| L8b | axis label overlap | `AXIS_HEADER` short text drawn clipped to gutter | **B1** |
| L8c | config form dominates on reopen; follow ON after stop | auto-collapse once when a capture exists (never fights a user toggle); follow turns off at the stop transition | **B1** |
| ROOT-1/2 | GUI checks from the finding | listed in §5 (ROOT only) | ROOT |

Already present and kept unchanged (finding §3 "이미 있어서 제외"): hover readout, honest counters/hatching, row→center, zoom/pan/keys, lanes, picker, CLI line, badges, journal read/wait, STOP latch, closed-workbench landing.

## 3. Batches and file leases (no two at once on a file)
| Batch | Sonnet lease (function bodies only) | Opus meanwhile (disjoint files) |
|---|---|---|
| **B1** UI: L1-R1, L2, L5-R1, L8a/b/c | `ui/src/panels/logic/logicModel.ts`, `logicPanel.ts`, `waveform.ts` | B2 contracts + scaffold in `crates/loom-logic/src/{archive.rs(new),model.rs,worker.rs,gateway.rs,cli.rs,lib.rs}` + Rust tests |
| **B2** engine: archive, archive RPCs, export, load test | `crates/loom-logic/src/*` (bodies listed then) | B3 UI scaffold (`logicText.ts` new, types) after B1 review |
| **B3** UI: L3, L4, L5-R2, archive status | UI files | B4 Rust scaffold |
| **B4** L6 duplex + error role (Rust then UI) | Rust first, then UI | — |
| **B5** L7 trigger (Rust then UI), skill doc | Rust then UI | `skills/loom-logic/SKILL.md` (doc, Opus) |
Each return: my diff review + targeted tests. Corrections go only through the named Herdr surface ROOT gives me. The module is **DONE only after B1–B5 plus the ROOT simulator/GUI acceptance.**

## 4. Architecture for later batches (decisions; full contracts come before each lease)
**Archive (B2).**
- **Location:** `<LOOM_HOME>/logic-archive/<capture_id>/` from `CallCtx::storage_roots().home`, passed to the worker in start args (video precedent). If there are no storage roots, archive state is `unavailable`, explicit and never a default home.
- **Config** (`[logic]`): `archive` (bool, default true), `archive_max_capture_mb` 256, `archive_max_total_mb` 2048, `archive_max_age_h` 168, `archive_max_captures` 32.
- **Rotation:** pruning runs before each new capture: oldest first, only dirs matching `^lc-[0-9a-f]+-\d+$` with a valid `manifest.json` format tag, never the running one.
- **Files:**
  - `manifest.json`: atomic rename; checkpoint ≤1 s and final. Fields: format `loom-logic-archive/1`, capture_id, samplerate, decoder canonical, state `recording|complete|partial|failed`, raw `{bytes, end_sample, truncated_at_sample, error}`, annotations `{rows, bytes, truncated_at_row, error}`.
  - `raw.lgr`: blocks with an absolute header (start_sample u64, state u8, n), each followed by varint(delta) + u8 value; all 8 bits, lossless.
  - `raw.idx`: fixed 24-byte block entries.
  - `annotations.jsonl`: every parsed line, absolute samples.
- **Writer:** dedicated thread, 16 MiB bounded queue. Overflow, IO error or quota means a sticky truncation at an exact sample/row. The capture continues, and the partial state lands in capture_state `archive{…}` and the manifest. Annotation archiving happens in `read_lines` before the batcher queue, with non-blocking `try_send`.
- **Reads:**
  - `logic.archive.list {}` (READ role): ≤32 manifests.
  - `logic.archive.read {capture_id, kind: edges|annotations, from_sample, to_sample, limit, cursor}` (READ role) answers in the logic.edges payload shape / annotation row shape plus `source:"archive"`. It returns `range{requested, returned}`, `complete`, `truncated_at` and a `next` cursor, within ≤64 KiB per result and the existing scan/row caps.
  - `logic.export {capture_id, kind, from_sample?, to_sample?, format: csv|txt}`: request class, R0, READ roles (like stop), audited. It writes `<archive>/<id>/export-<n>.<ext>` (≤64 MiB, counted in quota) and returns the daemon-side path. The UI says "daemon에 저장됨: <path>" and never claims a browser download.
  - CLI: `loom logic archive list|read|export`.
- **Error contract:** existing codes only (`E_USAGE`, `E_NOT_FOUND`, `E_CONFIG` for missing roots). Unknown/partial is never reported as complete.
- **Decoder backpressure** keeps today's semantics (capture stops `failed`, honest), and the archive keeps the raw data up to the stop sample.

**B3 UI:**
- `logicText.ts` (pure): radix format, ASCII rendering with `\xHH` for non-printables, line splitter (CR, LF, CRLF, lone CR), search over a byte string with hit→row mapping, CSV/TXT serializer with header (capture, from/to sample, rows, completeness, source).
- Cursor state lives in the panel; measurements are pure.
- Backfill pages through `logic.archive.read`, bounded (≤ 20 pages per click, MAX caps respected). Backfilled ranges merge through existing `addChunk`/`addAnnotations` dedupe.
- RPCs added to the manifest `queries`; the export action is `request` class.

**B4:** Rust `Decoder::Uart` gains an optional second line; `decoder_args` returns two `-P` + a combined `-A`; the synthetic generator gets a TX line (same text, half-frame offset) so the engine test can verify duplex; rows carry `role`.

**B5:** pure `TriggerDetector` + `PreRing` in model, wiring in worker, `trigger` key in START_KEYS/schema/CLI `--trigger D3:rising --pre 10ms --post 1s`; UI form + marker.

## 5. Acceptance
- **Per batch (Opus, isolated):**
  - `cd ui && npx vitest run src/panels/logic`
  - `npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile <scratch>`
  - `npx eslint src/panels/logic`
  - `CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo test -p loom-logic` (plus `cargo clippy -p loom-logic --all-targets` for Rust batches)
- **ROOT simulator/GUI (ROOT only):**
  - (1) L5: reopen a stopped synthetic capture, press [전체], scroll the table to the top; the coverage note must name what is missing.
  - (2) L1: synthetic text 256 B / 25 ms / 115200 / 10 s; the warning appears before start, `디코드 손실` grows, and after B2 the archive shows complete.
  - (3) after B4: dual-line UART tracks plus an error row; after B5: trigger marker.
- No hardware claims.

## 6. Shared / cross-module needs
- **B1: none.** All edits are inside the owned files. I wrote a Rust test plus a fixture under `crates/loom-logic/tests/` (owned).
- **B2:** uses the existing `CallCtx::storage_roots()` (no core change). The new domain RPCs, schemas and help are registered inside `loom-logic`; no generated catalog references logic RPCs today (checked). ROOT integrates combined registrations once. Cargo.lock: no new dependencies planned (varint and JSON by hand, serde_json is already a dependency).
- **UI manifest** (`registerPanel` in `logicPanel.ts`, module-owned): B3 adds `queries: logic.archive.list, logic.archive.read` and an `export` action. This is a declaration edit by Opus before the B3 lease.
- **Not touched:** shell/NetMux/SDK, core crates, root Cargo.toml, desktop, root styles.

## 7. Risks / notes
- **Lint red during the B1 scaffold window:** the imports and option tables in `logicPanel.ts` are unused until Sonnet's bodies use them. Lint-clean is part of B1 acceptance.
- **Shared `ui/node_modules/.tmp/tsconfig.app.tsbuildinfo`:** my first typecheck (21:35) wrote this cache file inside the shared node_modules (it is a generated tsc cache, not a dependency change). Every command now uses `--tsBuildInfoFile` in the scratchpad.
- **Auto-collapse:** the config also collapses once after the first capture appears even if the user started it from the form (this is intended, so the waveform shows). It never re-collapses after a user toggle.
