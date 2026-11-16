# Integration and operator acceptance — ROOT

Status PENDING. Baseline CI evidence is baseline-only, never new implementation evidence.

## Freeze/build
Each module Opus must return all batches reviewed, no production TODO stubs, owned diff hash/files and exact focused checks. ROOT integrates disjoint files serially, resolves declared shared declarations/lockfile, runs combined UI schema/type/lint/tests and affected Rust tests/clippy/fmt. Baseline rustfmt failures must be cleared explicitly before release. Rebuild/stage matching UI+loomd and verify native distribution fingerprints; no use of baseline app to validate new functions.

## Isolated actual acceptance
Use a new private LOOM_HOME and only owned fixture processes/ports. Keep user app23400/default daemon23423 untouched. GUI is ROOT-only via CUA/Ego Lite. Test external endpoint where available; engine simulators explicitly labelled. Capture command output and UI journal identity side by side, with seq/generation/frame identity and timing. Unit/jsdom tests prove contracts only; they are not native or hardware evidence.

| Module | Endpoint | Required actual acceptance |
| --- | --- | --- |
| UART | independent PTY fixture (desktop/scripts/lib/uart_fixture.py) | connect, bidirectional bytes, Tab/Enter, bounded-paced send/cancel, opt-in reconnect new generation, native continuous log + historic export, search and narrow split; compare CLI/UI exact marker |
| CAN | loom-can-sim (no physical CAN claim) | timestamp modes, persisted TX list, server periodic job ownership/stop, DBC decode, selected-range export parsed back, disconnect; compare trace seq/payload |
| Ethernet | own UDP/TCP fixture + capture sim/replay fixture | old-record outside lookback cannot report whole-history zero; explicit range returns marker, tree→filter, UDP/TCP follow, replay, scoped export and gaps; compare refs/seq |
| UDS | independent DoIP TCP ECU fixture | connect/disconnect/reconnect generation, raw+DID/DTC, stamped historical route, TP skips/stops correctly, DB decode, bounded configured key provider via policy-approved existing flow; compare request/result IDs |
| PSU | independent SCPI TCP supply fixture + sim aliases | limit prevalidation without write, chart/readout, bounded history stats/CSV, cycle OFF/readback/ON and cancellation stays OFF, two aliases/all-off fanout; compare measurement seq |
| Logic | real sigrok decoder fed generated digital input (no probe hardware claim) | supported decoder options, full bounded archive vs throttled preview and loss flags, search/export, cursor timing/gaps, trigger; compare sample offsets and archived counts |
| Video | independent MJPEG server / ffmpeg replay with deterministic ROI change | EOF no false fresh; freeze survives ring eviction through explicit pin TTL; in-panel snapshot SHA and native save routing; ROI normal→changed→unknown on loss, hysteresis/dwell/cooldown, config/source mismatch, evidence bytes SHA/frame IDs match agent/UI |

## Report
Write one HTML evidence report after acceptance with source/bundle hashes, screenshots, fixture endpoints, exact assertions, agent start/detection latency where measured, failures/fixes/rechecks and clear simulator-vs-hardware limits. Earlier final2 macro/native backlog remains distinct. PENDING items never promoted by inspection. Preserve final result page, close only own temporary browser/workspaces and owned fixture processes after handoff.

## Final STOP ordering
Do not trigger a disposable daemon's global STOP in mid-acceptance and then release it or change homes to bypass it. If a real STOP test is needed, schedule it LAST after all normal writes, native reconnect and fixture checks. After STOP perform only reads/cleanup; never auto-release. Unit-level injected latch tests remain distinct from actual daemon STOP evidence. A stopped test that needs further writes requires the user's explicit intervention under the Loom skill; do not manufacture credentials or quietly restart around it.

Current browser fixture is watch-only. Actual watch-allowed emergency OFF must work under that role without broadening ON/general write access; its observed server refusal is an OPEN product contract defect. Do not call a privileged API through page internals or mint operate/approve credentials to make the test pass. Data display + CLI actor actions and unit UI behaviour are reported separately.
