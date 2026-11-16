# Actual browser findings, NEXT_PENDING_CAN_NOT_RELEASE

ROOT ran the existing Opus simulator plan against immutable binaries in integration/LIVE-NEXT-PENDING-CAN.json. These are findings for Opus contract/review and Sonnet bodies; no new product code was changed by ROOT. Claude quota blocks the assigned models; no automatic retry or substitution.

## V-UI-STATE: visible capture remains opening while actually live

Observed from 07:25 through 07:29 KST. The visible service panel's video-status element has data-state=opening, text 여는 중. CLI video.status and video.wait both report capture.state=live, health.state=live, measured_fps=15; thousands of frames and new ROI judgments arrive in the same visible panel. At EOF the same panel correctly becomes closed.

Evidence: root-next/06-before-status.json, 11-status.json, 12-ui-changed.json; integration/browser/video-next-baseline.png and video-next-changed.png. The public DOM also includes an inactive hidden video panel with an older live snapshot; do not compare its fields to the visible panel. A first wait used querySelector and hit that hidden normal ROI, causing a harness timeout; the visible changed ROI is verified separately, not a product change-detection failure.

Source inspection points at VideoBook.applyState / applySnapshotItem ordering. Snapshot high-water can make an old opening capture outrank a genuine live event. This is a hypothesis until reviewed; do not fabricate event sequences or infer capture state from fresh frames. Require a regression using actual snapshot/live ordering and a real CLI open while the service panel is mounted.

## V-UI-OVERLAY: ROI outlines effectively invisible

The visible normalized SVG aligns with the actual canvas (within CSS rounding), but computed stroke-width is 0.004px while vector-effect=non-scaling-stroke; changed width is 0.008px. Both are subpixel and not visible in the real screenshot. UNKNOWN dash lengths are similarly 0.02/0.01. This mixes normalized geometry with non-scaling CSS stroke units.

Evidence: root-next/32-overlay-style.json, video-next-baseline.png / video-next-changed.png. Plan a visible constant pixel stroke/dash treatment across normal/changed/unknown and resized panels, preserving normalized ROI geometry and daemon-owned judgments. Opus must specify the exact change and review before Sonnet implements. No browser CSS injection was used.

## Already verified independently

root-next/EVIDENCE-COMPARE.json: 14 assertions pass. Lamp transition #4, frame3977, SHA2c7553b8…ea8a4 equals CLI wait, journal epoch1794781383 seq568 and downloaded artifact trigger. The visible UI shows #4/frame3977 and artifact SHA match. UI does not expose journal seq, so direct UI seq assertion is NOT claimed. Five pre/pre/trigger/post/post JPEGs match hashes and byte lengths; no partial reasons; evidence pins released. Reference ROI remained normal. Pixel change only, not scene understanding; physical capture boards/TCC not tested.

## Fresh main-only target recheck

V-UI-STATE is NOT REPRODUCED on fresh isolated source build. `root-isolated/10-ui-changed.json` and `integration/browser/video-isolated-changed.png` show visible LIVE, lamp changed #4/frame50, ref normal. Original opening observation stays tied to the stale-cache NEXT binary; no new reducer fix is authorized from that observation alone.

V-UI-OVERLAY remains reproduced on fresh build: root-isolated/11-overlay.json gives changed0.008px / normal0.004px and non-scaling-stroke. Opus-owned static CSS correction/review remains needed. Fresh exact artifact/transition/GUI checks8/8 pass in root-isolated/COMPARE.json; capture closed and fixture producer stopped. Final native and Opus review pending.
