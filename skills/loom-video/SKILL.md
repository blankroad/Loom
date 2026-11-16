---
name: loom-video
description: Capture video from an explicitly selected source (loopback HTTP MJPEG, replay file, named AVFoundation capture device) through Loom's daemon-owned FFmpeg, with the user watching the same live frames in the Loom UI while the agent reads metadata, frame hashes, snapshots and bounded recordings. Use for loom video, capture boards, camera/HDMI capture, video snapshots or video recording evidence.
---

Use the [common Loom skill](../loom/SKILL.md) for viewer visibility, user intervention, STOP latch, errors and bounded envelopes. Start with `loom video help --json` and `loom video capabilities --json`; capabilities report the FFmpeg engine, implemented vs verified sources and the storage quota. Never open a camera or capture board yourself (ffmpeg, OpenCV, browser getUserMedia); Loom's worker owns the device.

Select the source explicitly. There is no default device and no automatic fallback:
- `http-mjpeg --url http://127.0.0.1:PORT/path`: loopback only, no `user:pass@` credentials, no redirects, no automatic reconnect.
- `replay --name FILE`: a file inside `$LOOM_HOME/video/replay`, by name only.
- `avfoundation --device "Exact Name"`: implemented but **not verified on hardware or macOS camera permission (TCC)**. `default`, `none` and numeric indexes are refused. List devices only when the user asks: `loom video sources --local` (it can trigger the macOS permission prompt). Confirm the user's capture-board model and the exact name before opening.
- RTSP, DeckLink and audio are unavailable. Say so; never claim them.

```sh
loom video open --source http-mjpeg --url http://127.0.0.1:8090/stream --size 1280x720 --fps 30
loom video wait --frames 60 --timeout 10s        # bounded; returns status + health
loom video status                                 # state, 1 s health: measured_fps, age_ms, stale, drops
loom video frame --id 120                         # metadata + sha256 of that ring frame, no bytes
loom video snapshot --frame 120                   # saves that exact JPEG; artifact_id + sha256
loom video record start ; loom video record stop  # MJPEG-copy Matroska, bounded
loom video read --kind all --tail 50
loom video close
```

What "live" means here: the journal carries `video.capture_state` (opening/live/stalled/stopping/closed/failed), `video.health` every second, `video.snapshot` and `video.record_state`. It never carries image bytes. Do not try to pull every frame. Use health for continuity, `frame --id` for identity, and `snapshot` when you need the image. `frame_id` is Loom's receive counter after normalization. It is not the source's frame number or any counter burned into the picture, and `source_pts` is null (timestamps are receive-time). To prove that the user and the agent see the same frame, compare the frame id and SHA-256 shown in the frozen UI panel with `loom video frame --id N`. Do not correlate burn-in counters with frame ids.

Frames reach the UI through `GET /api/v1/video/{capture_id}/frame` (agent bearer or viewer cookie). These routes are read-only: they cannot open a device or start a recording.

Failures are explicit. `stalled` means no frame for 2 s. A disconnect or EOF ends the capture with a reason; to reconnect, run `open` again yourself, which creates a new capture id. Corrupt or oversized input is counted in health (`oversize`, `truncated`, `junk_bytes`, FFmpeg `dup_frames`/`drop_frames`). STOP freezes publication immediately: the last frame stays readable and `stopped_at_frame_id` is reported. Recording is finalized and FFmpeg reaped within about 2 s. While STOP is latched, open, snapshot and record are refused. Report that the capture is STOP-latched and leave the latch to the user.

Storage is finite. Each session allows at most 500 artifacts and 2 GiB, all sessions together at most 8 GiB of video, and one recording at most 1 GiB or 600 s. Nothing is deleted automatically. A full quota returns `E_LIMIT` with a reason, and a recording that hits its cap ends with `partial: true` and `limit_reason`. Recordings copy the preview JPEGs into Matroska: files are large and **not H.264**. `partial_reasons` lists queue gaps or an unclean finalize; never call a recording lossless.

**Pins (keep an exact frame).** The ring evicts old frames. To keep the frame you are inspecting, run `loom video pin --frame N [--ttl 2m]`. Pins live in daemon memory only. Each caller may hold at most 4 frames and each capture at most 16 MiB, for at most 600 s. A pin is released by `loom video unpin [--frame N]`, when its TTL ends, or at the next `open`. It is never evicted silently: a full store refuses with `E_LIMIT` and `pin_user_limit`/`pin_bytes`. `frame`, `snapshot` and the media `FRAME n` read pinned frames after eviction, even after close or STOP (`retained_by: "pin"`). Freezing in the UI pins the frame and shows the countdown. A UI snapshot is disabled with a reason when the frozen frame is not retained. Ended captures report `stale: true` and `last_recv_utc_ns`. Never call an ended capture fresh.

**ROI judgments (pixel change, computed once by the daemon).** The UI and agents read the same `video.roi_judgment` events. Do not compute your own verdict from snapshots.
```sh
loom video roi put --name lamp --rect 0.10,0.20,0.05,0.05 --enter 12 [--exit 6 --metric mean_abs_diff --dwell 300ms --cooldown 1s]
loom video roi baseline [--frame N] [--name lamp]      # baseline = that exact frame (ring or pin); default latest
loom video wait --roi lamp --roi-state changed --new --timeout 30s
loom video roi list                                    # config_rev + ROIs + judgments, history, baselines
loom video read --kind roi | --kind metric | --kind evidence
```
- The ROI config holds at most 8 ROIs in normalized `x,y,w,h` and is worker-scoped: it survives `open`, and `config_rev` grows on every edit.
  - Pass `--expect-rev N` so you never overwrite someone else's edit. A mismatch returns `E_BUSY` with `config_rev_mismatch`.
  - `roi list` output is a reusable recipe: `roi replace --rois '<that JSON>'` (`--json` stays the global output flag). Named recipe files on disk are not provided.
- Baselines belong to one capture: a new `open` drops them, and every ROI becomes `unknown(no_baseline)`.
- Metrics, per ROI against its baseline on ≤ 4096 grid samples:
  - `mean_luma`;
  - `luma_delta` (signed; judged on magnitude);
  - `mean_abs_diff` (% of 255);
  - `changed_px` (% of samples with |Δ| ≥ `pixel_threshold`).
- States are `normal`, `changed` and `unknown`, with enter/exit hysteresis, a `dwell` and a `cooldown`. Each transition carries `transition_seq`, the exact `frame_id`/`sha256`, the metrics and the baseline frame.
- **UNKNOWN is not normal.** It is reported, never smoothed over, for:
  - `not_live` (incl. STOP, EOF, close);
  - `stale`;
  - `no_baseline`, `baseline_mismatch` (resolution or rect changed);
  - `roi_too_small`, `decode_error`;
  - `analysis_gap` (analysed frames further apart than max(dwell, 1 s));
  - `config_changed`.
  Report UNKNOWN with its reason; never as a pass.
- Inspection runs at most 10 Hz, separately from the UI's render fps. `analysis.skipped` counts frames that were never analysed, and transitions are never inferred across them.
- These are pixel-change measurements, not scene understanding. Say "the ROI's pixels changed", not "the lamp turned on", unless the user confirms that meaning.

**Before/after evidence.** On `→ changed` (unless `--no-evidence`), the daemon bundles `--pre` and `--post` frames (default 2/2, at most 8 each) around the trigger.
- The bundle is one JSON artifact `evid-<capture>-<n>`: byte-exact base64 JPEGs, each with its sha256, plus the ROI spec, `config_rev`, baseline, transition and source config.
- Its lifecycle appears as `video.evidence`: `collecting` → `written` | `skipped` (`evidence_busy`, quota) | `aborted` (`stop_latch`) | `failed`.
- `partial` lists missing parts: `pre_unavailable`, `post_deadline`, `frame_unavailable`, `bytes_cap`, `capture_ended`, `pin_bytes`.
- At most 2 bundles are pending; each is ≤ 24 MiB of image data and quota-checked. Verify the frame hashes before citing a bundle.
- The UI opens bundles in-panel with ◀▶ and per-frame sha256 checks. Artifact links download rather than navigate.

Keep provenance honest. A loopback fixture or replay validates Loom's transport, UI and agent path, not a capture board. 60 fps counts only if measured `measured_fps` passes. TCC, HDMI/HDCP, board formats and audio stay unverified until tested on the user's hardware.
