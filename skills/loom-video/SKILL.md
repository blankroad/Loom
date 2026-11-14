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

Keep provenance honest. A loopback fixture or replay validates Loom's transport, UI and agent path, not a capture board. 60 fps counts only if measured `measured_fps` passes. TCC, HDMI/HDCP, board formats and audio stay unverified until tested on the user's hardware.
