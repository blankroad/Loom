# Video operator upgrade — PLAN (Opus lead)

Status: **PLAN_READY_BATCH1** (Batch 1 fully specified in FUNCTIONS.md; Batches 2–7 architecture fixed here, function contracts written before each batch's lease).
Baseline `610029e` · worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-video` · input `work/reports/usability-benchmark/RETURN-video.md` (V-01…V-08) · constraints `operator-upgrade/COMMON.md` v1.
Owned source: `ui/src/panels/video/**`, `crates/loom-video/**`, `skills/loom-video/**`. Nothing else is edited without a ROOT assignment (see §4).

## 1. Finding coverage

| ID | Prio | Disposition | Batch |
|---|---|---|---|
| V-01 ROI + baseline + explainable metrics | P0 | In scope: named rectangular ROIs (≤8, normalized coords), per-capture baseline from an exact ring/pinned frame, metrics `mean_luma`, `luma_delta`, `mean_abs_diff` %, `changed_px` %, computed **once in the worker** from the same JPEG bytes (frame_id+sha256 identity). Colour/OCR/template/position-adjust excluded (RETURN §3 stage 3; not added by this instruction). | B4 (pure) → B5 (wiring) → B6 (UI) |
| V-02 judgment state machine, shared UI/agent path | P0 | In scope: per-ROI `unknown|normal|changed`, enter/exit hysteresis, dwell, cooldown, explicit UNKNOWN reasons; transition events `video.roi_judgment`, 1 Hz `video.roi_metric`, `video.wait --roi`, `video.read --kind roi`. UI renders journal only, never computes. | B4 → B5 → B6 |
| V-03 frozen frame expires before snapshot | P0 | In scope: daemon pin store (`video.pin/unpin`, bounded count/bytes/TTL, released on unpin/TTL/next open) + UI freeze = pin, visible retention countdown, snapshot disabled with reason when not retained. | B2 (engine) → B3 (UI) |
| V-04 before/after evidence + timeline | P1 | In scope: on transition → `changed`, evidence bundle (pre/post ring frames, byte-exact JPEGs, base64 in one JSON manifest artifact `evid-…`, quota-checked, `video.evidence` event, partial reasons). UI judgment history + in-panel evidence viewer with ◀▶, ring ◀▶ while frozen, recv_utc display. Cross-module unified timeline markers: **external** (ROOT synthesis, RETURN V-04 note). | B3 (ring nav) · B5 (bundle) · B6 (UI) |
| V-05 ended capture shows "fresh" | P1 | In scope: UI ended-summary health line (no fps/fresh/STALE when not live) + engine `stale:true` for terminal health and new optional `last_recv_utc_ns`. | B1 (UI) · B2 (engine field) |
| V-06 artifact links navigate the app away | P1 | In scope: no plain artifact anchors; snapshots open in an in-panel viewer (sha verified), save/download anchors carry `download`; engine adds `content-disposition: attachment` on artifact replies. Native nav behaviour proof = ROOT GUI. | B1 (UI) · B2 (header) |
| V-07 reopen resets the form | P2 | In scope: `↺ 같은 설정으로 다시 열기` prefill from the ended capture (no RPC; open stays explicit; URL query never restored — note shown). Inspection "recipe": ROI set + params + config_rev + baseline identity exportable via `video.roi.list` and re-applied via `video.roi.replace` (B5). Persisting named presets to disk: **excluded pending ROOT storage decision** (no shared storage location owned). | B1 · B5 |
| V-08 free-text device / replay | P2 | In scope: replay names from `video.sources {local:false}` when the user selects replay (once per mount); device names only on explicit `장치 목록 불러오기` (`local:true`, TCC warning). Datalists keep the existing validated inputs. | B1 |

COMMON video edge cases mapped: input loss/EOF/STOP/missing or mismatched baseline/resolution → UNKNOWN (B4 `UnknownReason`); config revision invalidates in-flight results (B4/B5 `config_rev` check before publish); inspection fps ≠ render fps, skipped analysis counted and no unseen transitions inferred (B4/B5 `analysis.skipped`, gap → UNKNOWN); evidence pin bounded and outlives display freeze with explicit release rules (B2); pixel change ≠ scene understanding (skill text B7, UI wording).

## 2. Architecture decisions

**A1 Judgment lives in the worker.** New pure module `crates/loom-video/src/analysis.rs` (decode → luma, ROI sampling, metrics, `RoiJudge` state machine). Worker thread `video-analysis` per capture analyses the newest ring frame at ≤ `ANALYSIS_HZ = 10`, publishes transitions (priority lane) and 1 Hz summaries (data lane). Same frame bytes as media/snapshot → identity by frame_id+sha256.
**A2 JPEG decode dependency** `zune-jpeg =0.5.15` (MIT/Apache/Zlib, MSRV 1.75, only dep `zune-core 0.5.1`, both in local cargo cache; pure Rust; luma output via `DecoderOptions::jpeg_set_out_colorspace(ColorSpace::Luma)`, max 4096×4096). Requires ROOT/CORE approval (§4 S1). Fallback considered and rejected: FFmpeg second gray output (needs fd 3 / frame alignment; RETURN (b)).
**A3 Sampling.** ROI rect in pixel space = round(normalized × frame dims), clamped, ≥2×2 px. Sample grid step `s = max(1, ceil(sqrt(area / 4096)))` → ≤ 4096 samples per ROI. Baseline stores the sample vector + frame dims + rect px + step; comparison requires identical dims and rect, else UNKNOWN `baseline_mismatch`.
**A4 State machine** (per ROI, deterministic, monotonic time = frame `recv_mono_ns`): magnitude metric `m ∈ {luma_delta, mean_abs_diff, changed_px}`; `exit ≤ enter`. UNKNOWN entered immediately (no dwell/cooldown) on: capture not `live` (incl. stalled/stopping/closed/failed/STOP), health stale, no baseline, baseline mismatch, decode error, analysis gap `> max(dwell_ms, 1000)` between analysed frames, config change. Leaving UNKNOWN / switching normal↔changed requires the candidate condition (`v ≥ enter` → changed, `v ≤ exit` → normal; from UNKNOWN: `v ≥ enter` vs `v < enter`) to hold continuously for `dwell_ms`, and normal↔changed transitions wait out `cooldown_ms` after the previous normal↔changed transition. Each transition increments a per-ROI `transition_seq`.
**A5 Inspection config is worker-scoped, baselines capture-scoped.** ROI specs + `config_rev` survive reopen (same `VideoWorker`); baselines are dropped at each new open (ROIs show UNKNOWN `no_baseline`). Edits use optional `expect_rev` (mismatch → `E_BUSY` detail `{reason:"config_rev_mismatch", config_rev}`). Every edit publishes `video.roi_config` (full set) so UI/agents/late joiners share one config via the reducer.
**A6 Pin store** (B2): `PinStore` in `model.rs` holds `Frame` clones (Arc bytes) by frame_id with owners `{user, evidence(n)}`; bounds `MAX_USER_PINS = 4`, `PIN_BYTES = 16 MiB` (all owners), `PIN_TTL_DEFAULT_MS = 120_000`, max 600_000. Frame lookup everywhere (`video.frame`, `video.snapshot`, media `FRAME n`, baseline) = ring then pins. Released on `video.unpin`, TTL expiry (checked lazily on every access + supervisor tick), and next `video.open` (history ends). Pins survive capture close/STOP (memory only, read-only) until TTL/next open. `video.pin` while STOP latched is allowed (memory, safe direction); snapshot still refused while latched.
**A7 Evidence bundle** (B5): on `→ changed` (per-ROI `evidence: bool`, default true; `evidence_pre`/`evidence_post` 0..=8, default 2/2). Pre frames + trigger pinned immediately (owner evidence); post frames pinned as they arrive or deadline `2 s + post/fps` → `partial:["post_deadline"]`. Max 2 pending bundles (else `video.evidence{state:"skipped",reason:"evidence_busy"}`); bundle bytes ≤ 24 MiB (frames beyond → omitted, partial). File `<session>/video/<cap>/evid-<cap>-<n>.json` (0600, create_new): manifest + per-frame `{role, frame_id, sha256, recv_utc_ns, recv_mono_ns, size, metrics|null, jpeg_b64}`. Quota via `scan_usage().refuse`. STOP during collection → `aborted`/`stop_latch`, pins released, nothing written. Served by the existing artifact route (`application/json`).
**A8 UI renders, never judges.** New journal kinds feed `VideoBook`; ROI overlay (SVG over the canvas, normalized coords), list with state chips/values, history (bounded 64 rows), evidence viewer (fetch artifact JSON via `ctx.media`, verify each frame sha256 in the browser). Inspection rate vs render fps shown separately.
**A9 Backward compatibility.** All new payload fields optional for readers; old events (no `last_recv_utc_ns`, no roi kinds) render as before; `video.wait` without `roi` unchanged; `video.read --kind all` adds new kinds (documented); `kind` enum grows (`roi`, `evidence`). Tests cover absent-field defaults.

## 3. Batches (each = one Sonnet lease on the listed source files; Opus-authored tests in separate test files)

| Batch | Findings | Leased source files (Sonnet bodies) | Opus test files | Gate |
|---|---|---|---|---|
| **B1** | V-05 UI, V-06 UI, V-07 reopen, V-08 | `ui/src/panels/video/videoModel.ts`, `videoPanel.ts` (`video.css` scaffolded by Opus, not leased) | `ui/src/panels/video/videoOperator.test.ts` | **ready now** |
| B2 | V-03 engine, V-05 engine, V-06 header | `crates/loom-video/src/{model,worker,gateway,cli,media}.rs` | `crates/loom-video/tests/pin_contract.rs` (+ e2e additions in a new `tests/operator_e2e.rs`) | contracts after B1 handoff (different files) |
| B3 | V-03 UI, V-04 ring ◀▶ | `videoModel.ts`, `videoPanel.ts` | `videoPin.test.ts` | after B1 accepted + B2 contracts |
| B4 | V-01/V-02 pure core | new `crates/loom-video/src/analysis.rs` (+ `lib.rs` mod line, Opus) | `tests/analysis_contract.rs` (embedded tiny JPEG fixtures) | needs S1 approval |
| B5 | V-01/V-02/V-04/V-07 wiring | `worker.rs`, `gateway.rs`, `cli.rs`, `model.rs` (evidence ids), `media.rs` | `tests/roi_e2e.rs` (lavfi-generated replay clip black→white in temp home) | after B2 accepted, B4 accepted |
| B6 | V-01/V-02/V-04 UI | `videoModel.ts`, `videoPanel.ts`, new `roiView.ts` | `videoRoi.test.ts` | after B5 contracts |
| B7 | docs | `skills/loom-video/SKILL.md` (Opus-authored docs, not product code) | — | after B6 |

Order inside each batch: functions in FUNCTIONS.md order; Sonnet stops at first contract ambiguity and returns to Opus. Opus does not edit leased files during a lease.

## 4. Shared / external needs (ROOT assignment required; none edited by this module)

| # | Need | Why | Proposed owner / action |
|---|---|---|---|
| S1 | Root `Cargo.toml` `[workspace.dependencies] zune-jpeg = "=0.5.15"`; `crates/loom-video/Cargo.toml` `zune-jpeg = { workspace = true, optional = true }` under feature `worker`; generated `Cargo.lock` adds `zune-jpeg`, `zune-core` | A2 decoder | CORE/ROOT approve; lockfile = declared generated side effect serialized by ROOT. Blocks B4/B5 only. |
| S2 | `crates/loom-video/Cargo.toml` `base64.workspace = true` (existing workspace dep) | A7 evidence `jpeg_b64` | module manifest (owned) — declared here; ROOT ack |
| S3 | New event kinds `video.roi_config`, `video.roi_judgment`, `video.roi_metric`, `video.evidence`; new RPCs `video.pin`, `video.unpin`, `video.roi.put/remove/replace/list/baseline`; `video.wait` params `roi`,`roi_state`,`new`; `video.read` kinds `roi`,`evidence` | registered by `loom-video` itself | ROOT regenerates shared catalogs/schemas once at integration |
| S4 | Existing loomd route `/api/v1/video/artifact/{id}` serves new prefix `evid-` and new header `content-disposition` (header passthrough is generic in `loom-gateway/src/server.rs::video_reply`) | A7, V-06 | no loomd edit; ROOT ack |
| S5 | Native GUI checks: V-06 save anchor download vs navigation, V-03 freeze→wait→snapshot on 1280×720 large-JPEG source, ROI judgment co-receive UI vs CLI | GUI is ROOT-only | ROOT after integration |
| S6 | Test media: B5 e2e generates a replay clip with local `ffmpeg -f lavfi` inside its temp `$LOOM_HOME` (no shared fixture edit); skipped with message when ffmpeg absent | deterministic scene change | module-owned |
| S7 | `ui/src/shell/videoWiring.test.ts` (shared shell test, not owned) pins the video panel's exact action map and topic kinds. B3 adds actions `pin` (watch) and `unpin` (watch, reduce); B6 adds ROI actions/kinds. Required edits: L38 `right` map += `pin: 'watch', unpin: 'watch'` (+ B6 ROI actions); L39 reduce list → `['close', 'record_stop', 'unpin']`; L41 kinds += the B5/B6 ROI/evidence kinds | the shared test fails until updated | ROOT edits (or assigns) at integration; exact final lines given in the READY report |
| — | No HTTP handler, core journal helper, desktop bridge, SDK, shared style or lock changes requested. |

## 5. Validation plan

Regression examples first (each fails on baseline 610029e):
1. V-05: closed(eof) capture + last health `{stale:false, age_ms:25}` → health line must not contain `fresh`/`수신 x/y fps`. (B1 test P1)
2. V-06: snapshot row rendered with an `<a>` lacking `download` → every anchor must carry `download`; view happens in-panel. (B1 P4)
3. V-07: ended capture `640×360@15` + form default `1280×720@30` → reuse prefill. (B1 P2)
4. V-03: ring overflow evicts pinned frame → `video.snapshot{frame_id}` must still succeed while pinned. (B2)
5. V-02: capture stalls while ROI `normal` → ROI must become `unknown(stale)` not stay `normal`. (B4/B5)
Function-level tests per FUNCTIONS.md; module integration: Rust `cargo test -p loom-video` (focused, `CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2`), UI `npx vitest run src/panels/video` + `npx tsc -b --pretty false` + `npx eslint src/panels/video` from `ui/`. Fixture e2e only on isolated temp homes (`/tmp/loom-video-*`), no device, no default daemon/app. Final simulator + native co-receive by ROOT.

## 6. Risks
- Decode cost at 1080p unmeasured (analysis capped 10 Hz, skipped counted; hardware sizing = ROOT/hardware item).
- Evidence JSON with base64 inflates 33 %; capped 24 MiB/bundle.
- `content-disposition: attachment` changes direct-browser opening of artifact URLs to download (intended; in-panel viewer replaces it).
