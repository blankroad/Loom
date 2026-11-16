# Video operator upgrade — READY_TO_INTEGRATE (Opus lead → ROOT)

Worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-video`, branch `operator/video`, baseline `610029e`. Nothing is staged or committed (ROOT owns the index).

Process:
- Batches B1–B7 are all accepted. Sonnet (`upgrade-video-sonnet`) wrote product function bodies only.
- Opus wrote the plan, contracts, types/imports/constants/scaffolds, CSS, the skill doc and every test.
- Contracts and amendments A1–A7 are in `FUNCTIONS.md`; the per-batch history and freeze hashes are in `STATUS.md`.

## What changed (findings → result)
| Finding | Result |
|---|---|
| V-01 ROI + baseline + metrics | Up to 8 named ROIs (normalized `x,y,w,h`). Each has a baseline taken from an exact ring or pinned frame, sampled on ≤ 4096 grid points per ROI. Metrics: `mean_luma`, `luma_delta`, `mean_abs_diff`, `changed_px`. JPEGs are decoded to luma by `zune-jpeg` in the worker (S1). |
| V-02 shared judgment | Per-ROI `unknown/normal/changed` with hysteresis, dwell and cooldown. The worker computes it once, at most 10 Hz, and publishes **`video.roi_judgment`** (transition, exact frame id/sha, metrics, baseline) plus a 1 s `video.roi_metric`. The UI renders those events and never computes. Agents use `loom video wait --roi … --roi-state … --new`, `read --kind roi`, `roi list`. UNKNOWN reasons: not_live, stale, no_baseline, baseline_mismatch, roi_too_small, decode_error, analysis_gap, config_changed. Config edits happen under the lock rounds use, so results from an old revision are never published. |
| V-03 frozen frame expires | Daemon pin store: `video.pin/unpin`, ≤ 4 user pins, 16 MiB per capture, TTL ≤ 600 s, released on unpin, TTL or next open, memory only. Freezing in the UI pins the frame and shows a countdown. Snapshot is disabled with a reason when the frame is not retained. |
| V-04 before/after evidence | On `→ changed` the daemon writes one `evid-<cap>-<n>.json`: byte-exact base64 JPEGs for pre/trigger/post, each with its sha256, plus spec, config_rev, baseline, transition and source. It is quota-checked, `create_new` 0600, ≤ 24 MiB, at most 2 pending, and STOP aborts it. Lifecycle events `video.evidence`. The UI has evidence rows and an in-panel viewer with per-frame sha256 checks and ◀▶; ring ◀▶ works while frozen. Cross-module timeline markers: external (ROOT synthesis). |
| V-05 ended capture "fresh" | Terminal health is `stale:true` and carries `last_recv_utc_ns`. The UI shows an ended summary line. |
| V-06 artifact links navigate away | Artifact replies carry `content-disposition: attachment`. The UI views artifacts in-panel and its save anchors carry `download`. |
| V-07 reuse settings / recipe | "Reopen with same settings" prefills the form. The ROI config is worker-scoped and survives reopen; `roi list` output re-applies with `roi replace --json` (with `expect_rev`). Named preset files on disk are **excluded** (no owned storage location). |
| V-08 source lists | The replay list loads when replay is selected; the device list loads only on an explicit click (TCC warning). |

## Files (module-owned unless noted)
Modified (tracked diff `git diff` sha256 `9054391e9a4b4f1c5d4cb2e5bd1312a4c2c34b814e3d69fd0715fef8a636420c`, 13 files, +2427/−106):
- `Cargo.toml` (S1: the single `zune-jpeg = "=0.5.15"` line) and `Cargo.lock` (generated: `zune-jpeg 0.5.15`, `zune-core 0.5.1`, plus loom-video edges).
- `crates/loom-video/Cargo.toml` and `crates/loom-video/src/{lib,model,worker,gateway,cli,media}.rs`.
- `skills/loom-video/SKILL.md`.
- `ui/src/panels/video/{video.css,videoModel.ts,videoPanel.ts}`.

New (untracked; sha256 at handoff):
```
293ba83f35cd81a94b54ce8ebacd84774ae1a947374a67e0efa19f0292006913  crates/loom-video/src/analysis.rs
abcb4c9e2d05a2fe43b2b5fb16a89a4cc1252f70d32390d56eb20ae2e525ef8f  crates/loom-video/src/inspect.rs
c24671ed3f60f1ccfcc9991740d24039902abe43b8114d5ce1d886a8c65bbf96  ui/src/panels/video/roiView.ts
d8b0499cba626ce102b39ca8ca68fb469a1a026925a6aa20114acae7fe0c0e4e  crates/loom-video/tests/analysis_contract.rs
4d3de77359042f93f4477e444006e49073f80e32d430a1533dabbcff625566d7  crates/loom-video/tests/inspect_contract.rs
01a18744802b518aeddd41438e5cb1207336c3507951fef44533641a166539c6  crates/loom-video/tests/operator_e2e.rs
9719e559af593dfbd6b3f112e084088b761ff0152245ceb3c799d01936e73bb2  crates/loom-video/tests/pin_contract.rs
03d16d4ae46a715e1e98f1568e0a33b07d2ff1f05463b413336bf31b5783aaf7  crates/loom-video/tests/roi_contract.rs
1d41cc1b47dcfc18eb08cf74f40719db8cefe19aefd277f51068271edaa0b986  crates/loom-video/tests/roi_e2e.rs
crates/loom-video/tests/fixtures/*.jpg (6 ffmpeg-lavfi JPEGs, 226–605 B; hashes in STATUS B4)
15ead88350e1448cd4dc591bfe7c7da45698387a85f41b910259955cab5d02f2  ui/src/panels/video/videoOperator.test.ts
e3b3e2b780ddb5449afae9fca7d29f6e393175c1c0fe0d1de0bde7b7f3030043  ui/src/panels/video/videoPin.test.ts
9d46c219cc7998a25224c4e0b321adf5487cef9b7e67022d6e0aa16b223ec97f  ui/src/panels/video/videoRoi.test.ts
```
Do not commit: `ui/node_modules` (shared symlink) and `work/typecheck/` (isolated tsbuildinfo caches).

## Verification (Opus, final state, worktree root)
Rust (`CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2`):
- `cargo test -p loom-video --no-fail-fast` → unit 12, analysis_contract 21, fixture_e2e 4, inspect_contract 10, operator_e2e 3, pin_contract 8, platform_unsupported 0, roi_contract 6, roi_e2e 3 — **67 passed, 0 failed**. FFmpeg 9.0.2 and python3+Pillow were present, so nothing was skipped.
- `cargo clippy -p loom-video --tests` → 0 warnings.
- `cargo check -p loom-cli -p loom-gateway` (crates that depend on loom-video) → 0 warnings/errors. Neither has video-named tests.

UI (from `ui/`, ROOT resource rule):
- `npx vitest run src/panels/video --maxWorkers=2` → 5 files, **54/54** (B6 run).
- After Opus's lint-only test fix: `videoRoi.test.ts` 15/15.
- `npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo` → 0; `tsconfig.node.json` with `../work/typecheck/node.tsbuildinfo` → 0.
- `npx eslint src/panels/video` → 0.
- Full UI suite: **not run** (ROOT-only at the integrated freeze).

End-to-end coverage is real FFmpeg inside private `/tmp/loom-video-{op,roi}-*` homes, using the Python MJPEG fixture and a self-generated lavfi black→white replay clip. No device, default daemon/app, GUI or credentials were touched.

## Needs ROOT (shared / external)
1. **S7 — DONE by Opus under ROOT's explicit test-only grant.** Patch `S7-videoWiring.patch` (sha256 `2153ce9d02afd9448a6a849e798b552260b56f047571a8ebc17f95c98585ea41`); file sha256 `4adf2923cef9d83d21a1b0a24c99b392e56893d7105a78075b62f78eee03e84d`. `npx vitest run src/shell/videoWiring.test.ts --maxWorkers=2` → 4/4; eslint 0. Lines applied:
   - L38: `expect(right).toEqual({ open: 'operate', snapshot: 'operate', record_start: 'operate', record_stop: 'watch', close: 'watch', pin: 'watch', unpin: 'watch', roi_put: 'operate', roi_remove: 'operate', roi_baseline: 'operate' });`
   - L39: `...toEqual(['close', 'record_stop', 'unpin']);`
   - L41: `...kinds).toEqual(['video.capture_state', 'video.health', 'video.snapshot', 'video.record_state', 'video.roi_config', 'video.roi_judgment', 'video.roi_metric', 'video.evidence']);`
2. **S3: regenerate shared catalogs/schemas**:
   - New RPCs: `video.pin`, `video.unpin`, `video.roi.put/remove/replace/list/baseline`.
   - `video.wait` params `roi`, `roi_state`, `new`.
   - `video.read` kinds `roi`, `metric`, `evidence`.
   - Event kinds `video.roi_config`, `video.roi_judgment`, `video.roi_metric`, `video.evidence`.
   - Artifact prefix `evid-` (served by the existing generic route, S4).
3. **S5: native/GUI checks (ROOT-only, not claimed):**
   - Do the save/download anchors actually deliver files in the native app, or navigate? The `download` attribute and header alone are not proof.
   - In-panel snapshot and evidence viewer on the real daemon.
   - Ended-health line on a real EOF.
   - Freeze → wait > ring span → snapshot on a 1280×720 source.
   - ROI overlay alignment on a real canvas (jsdom has no layout).
   - UI vs CLI co-receive of `video.roi_judgment`.
4. **Integration finding fixed module-locally (A6):** the hub `is_channel_state` and UI `isStateKind` treat `<domain>.*_state` on a channel as connectivity. The ROI kind is therefore `video.roi_judgment`, and both test suites assert that no ROI kind matches the rule. No shared edit was needed. Please keep this in mind for any future `*_state` kinds.
5. Native fixture runs and Git/push are ROOT's, after full integration.

## Risks / limits
- Decode cost at 1080p is unmeasured on bench hardware. Inspection is capped at 10 Hz and skipped frames are counted.
- Evidence base64 inflates bundles by about 33% (capped at 24 MiB). `content-disposition: attachment` makes direct browser opens of artifact URLs download (intended).
- The FakeWorker SimClock keeps monotonic time at 0, so e2e covers dwell/cooldown 0 and count-based evidence completion. Timed dwell, cooldown, gap and evidence-deadline behaviour is covered by the pure contract tests.
- The B6 overlay is positioned from the canvas box (`offset*`). Layout under `object-fit: contain` letterboxing needs the GUI check (S5).
- Pixel change ≠ scene understanding. Payloads, UI and skill all say so.

## Source freeze and runtime plan
- Source freeze manifest: `SOURCE-FREEZE.sha256` (31 files: every modified and new module file, sha256 `b66e50ca5257273a5c707726dcc84105ddbc62fba0aad5ba13ac2f836ef23034`), plus the S7 test file above. No further source edits by Opus or Sonnet; the worker is idle with every lease returned.
- ROOT push is authorized after completion; Opus does not stage, commit or push.
- Runtime QA: `runtime/PLAN.md` (**PLAN ONLY**; nothing executed). It needs `runtime/DAEMON.json` (ROOT phase-2 metadata) before any IO. Inert fixtures: `roi_mjpeg.py` (independent operator-switched MJPEG scene, stall/disconnect), `gen_clip.sh` (lavfi replay clip), `roi-recipe.json`, `EXPECTED.json`. Coverage: fresh co-receive with known journal seq, loss ⇒ UNKNOWN with nothing inferred, bounded pin/evidence before-after sha256, and the render vs analysis rate distinction. No STOP, GUI (Opus) or default daemon.

## RF-1 fix (ROOT-authorized after the freeze) — incremental
- **Defect** (found by the phase-2 runtime; failure evidence kept in `runtime/evidence/02-replace.json`, `02-replace-bare.txt`, `runtime/RUNTIME-STATUS.md`): `loom video roi replace --json <payload>` panicked (exit 101) because its arg id collided with the global `--json` output flag.
- **Fix**: the payload flag is now `--rois`; the global `--json` is unchanged. The product change is 3 lines in `crates/loom-video/src/cli.rs` (Sonnet, contract A8): arg definition, read, error text.
- **Specs (Opus)**: `roi_contract.rs` (`--rois`; `--json` rejected for replace), `SKILL.md`, and the new ROOT-assigned shared test `crates/loom-cli/tests/video_roi_replace.rs`. That test parses through the integrated `loom` command with and without the global `--json`, checks for no panic and the exact `video.roi.replace` params, and guards that no video arg shadows `--json` / `--idempotency-key`.
- **Baseline → after** (sha256; baselines re-derived by reversing the edits, matching the recorded `RF1-BASELINE.sha256`):
```
cli.rs                     32b04057c544f137… → 0da220e3f82a791d…
tests/roi_contract.rs      03d16d4ae46a715e… → 7f05577dddb2e140…
skills/loom-video/SKILL.md 4cf7d3337c583586… → edeeab05f33a7ed5…
crates/loom-cli/tests/video_roi_replace.rs (new) → bd0a00255ccf1a49…
```
- **Patches**:
  - `RF1-incremental.patch` (4 files), sha256 `aae5ce727fde1896e8286eb85d857bfeb1d3700138b7c0088195779cdfaa634b`.
  - Product-only `RF1-cli.patch`, sha256 `b7169946bb09bbcb6d9c327f43e6e4eca1936474ab6aeb312835f5e15657fde2`.
- **Verification (Opus re-run, `CARGO_BUILD_JOBS=2`)**:
  - `cargo test -p loom-cli --test video_roi_replace` → 3/3 (expected red before the fix: 2 failed / 1 passed).
  - `cargo test -p loom-video --no-fail-fast` → 67/67 (re-run after the rename); `roi_contract` 6/6 after the message fix.
  - `cargo clippy -p loom-video --tests` → 0; no clippy findings in the new test.
- **Source freeze v2**: `SOURCE-FREEZE.sha256` (33 files; sha256 `59af4206aa670e570daf8dd7517fdf4dc7d54a9a2725c7a20732853591d66388`), tracked `git diff` sha256 `d86f63e0c93b53a6cf1cfb1cf53852a9a76dad84006c8c5c902d17d1e24b3e54`. v1 is kept as `SOURCE-FREEZE-v1.sha256`. The delta from v1 is exactly the three files above plus the two ROOT-assigned shared tests (S7 `videoWiring.test.ts`, RF-1 `video_roi_replace.rs`).
