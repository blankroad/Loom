# Logic-analyzer live logging — Sol research

2026-11-14 KST. Foreground web/source and repository inspection only; no installation, build, device scan/access, app settings, Git operations, agents, or product edits. Read `docs/plan/explore/v12-saleae-check.md` and prior sigrok findings. Hardware model remains unknown; no hardware success is claimed.

## Recommendation for ROOT decision

Implement a **real sigrok acquisition adapter plus an explicitly labelled simulator**, behind one Loom `la` domain. A supported hardware model/driver is a prerequisite, not an assumption about the user's Saleae. Make raw digital chunks and decoder annotations available before STOP through the existing canonical hub stream. Do not substitute UART telemetry for pin-level acquisition. Keep Saleae as a conditional adapter/spike: current evidence warrants testing live analyzer/HLA operation, not an absolute “unsupported” statement and not a promise of external raw streaming.

| Path | Evidence and practical limit |
|---|---|
| sigrok-cli / libsigrok | Actual device acquisition feeds `SR_DF_LOGIC` packets during a session. CLI raw binary output flushes each emitted packet; annotations flush each emitted line. Driver/device buffering still determines latency and whether continuous acquisition works. One CLI invocation with `-P` suppresses normal raw output; do not promise simultaneous raw + decoder output from that invocation. |
| PulseView | Its session appends incoming logic data and signals `data_received()` while acquiring. This establishes a working library/GUI model, not a public external subscription API. Loom should own acquisition rather than scrape PulseView or open the same USB device twice. |
| Serial Studio | Current API broadcasts parsed dashboard frames and base64 source bytes on its TCP API. Its reference explicitly excludes MCP sessions from these pushes; gRPC separately offers binary streaming. Useful shared-data UX/reference or future telemetry adapter, but “raw bytes” here does not establish a logic-analyzer sample clock, pin capture, trigger, or LA hardware driver. It supports more than UART, yet is not evidence of a pin-level LA engine. |
| Saleae current MCP / HLA | Official MCP is experimental, Streamable HTTP on default port 10530; official page lists Logic 8/Pro 8/Pro 16, MSO coming soon. The published MCP guide/llms file does not document live waveform/frame subscription tools. MCP transport streaming alone proves no acquisition-data stream. HLA `decode(frame)` receives lower-level analyzer frames; a custom bounded IPC bridge is a plausible decoded-only path requiring measurement and lifecycle validation. It does not expose raw edges. |

## New Saleae evidence takes precedence over old blanket constraints

**Logic 2.4.48 release, Oct 8 2026:** vendor explicitly fixes adding an analyzer through Automation **or MCP** during capture blocking until manual STOP. Thus the old Capture page's blanket stop/wait-before-other-operations rule is not sufficient to declare live analyzer addition impossible. The fix establishes return behavior for analyzer addition, not live exports, HLA delivery timing, a raw waveform subscription, or every operation's validity during capture. Test those separately on the installed app version after ROOT decision. Existing plans containing unconditional “capture streaming impossible” need qualification when implementation scope is decided; they were not edited here.

- Release: https://discuss.saleae.com/t/logic-2-4-48/3820
- Original hang discussion (older stop-first workaround): https://discuss.saleae.com/t/logic2-mcp-add-analyzer-hanging-forever/3816
- Current official MCP guide: https://docs.saleae.com/mcp/guides/getting-started
- Machine-readable MCP guide (currently setup information, no tool schemas): https://docs.saleae.com/llms-mcp.txt
- Capture API, older stop/wait constraint: https://docs.saleae.com/automation/api/automation/Capture
- HLA input-frame callback: https://docs.saleae.com/extensions/api/analyzers/HighLevelAnalyzer
- Older vendor real-time HLA discussion, historical context rather than current guarantee: https://discuss.saleae.com/t/realtime-hla-frame-inspection-when-using-automation-api/2957

## Exact sigrok stdout/raw findings

Stable 0.7.2 `show_pd_annotations()` prints optional decimal `start_sample-end_sample `, then `decoder_id: `, optional annotation class, then the longest annotation text, newline, **`fflush(stdout)`**. At normal warning verbosity with `--protocol-decoder-samplenum`, expected shape is `123-456 uart: <annotation text>` (illustrative, not observed hardware output). Verbosity changes quoting/class/abbreviations, so pin options; split only the numeric prefix and first decoder separator, preserve the remaining text. This is human-oriented text, not a reliable structured bus transaction schema. `--protocol-decoder-annotations` filters classes; it is not itself a structured-output switch. Master adds an explicit annotation-class option absent from the inspected stable function; do not require that option on 0.7.2.

- Stable decoder source: https://github.com/sigrokproject/sigrok-cli/blob/sigrok-cli-0.7.2/decode.c
- Current decoder source: https://github.com/sigrokproject/sigrok-cli/blob/master/decode.c
- CLI options: https://github.com/sigrokproject/sigrok-cli/blob/sigrok-cli-0.7.2/doc/sigrok-cli.1

Current `session.c` sends logic packets to decoding when `opt_pds` is set, but runs normal output only under `!opt_pds`. Output uses `fwrite` then `fflush`. The binary output module copies `logic->data` directly for `SR_DF_LOGIC`: **no in-band samplerate/channel metadata**. Thus `-O binary` provides sequential live samples, not live random-access queries into a finished capture. Raw data must be retained/indexed by Loom with an explicit channel map, unit size/packing, samplerate, starting sample index and capture epoch. Preserve partial sample bytes between pipe reads. Never treat a growing `.sr` ZIP as a completed queryable artifact.

- Session branch and flushing: https://github.com/sigrokproject/sigrok-cli/blob/master/session.c
- Binary output: https://github.com/sigrokproject/libsigrok/blob/master/src/output/binary.c
- Stable stdin input uses POSIX `read(fd, ..., 4 MiB)`, then `sr_input_send` in its loop; the requested size alone does not require a pipe to fill 4 MiB: https://github.com/sigrokproject/sigrok-cli/blob/sigrok-cli-0.7.2/input.c
- PulseView acquisition: https://github.com/sigrokproject/pulseview/blob/master/pv/session.cpp

Smallest subprocess design: one device-owning `sigrok-cli ... --continuous -O binary` → Loom raw collector → raw retention / edge-preserving waveform summaries / a separate `sigrok-cli -i - -I binary:<explicit metadata> -P ... --protocol-decoder-samplenum` decoder. Exact input-module options must be checked against the eventual installed version. This two-process path has not been run here; decoder buffering, rate/packing, cancellation and stalled-pipe behavior remain gates. If it fails the latency gate, use a helper owning libsigrok + libsigrokdecode callbacks, not two hardware opens. No PATH-installed sigrok-cli is available per task context; no install or runtime claims were made.

Hardware compatibility is model-specific: Original Logic differs from Logic 8; experimental Pro 16 driver does not establish compatibility with new revisions mentioned in 2.4.48. Relevant source: https://github.com/sigrokproject/libsigrok/blob/master/src/hardware/saleae-logic-pro/api.c ; model references: https://sigrok.org/wiki/Saleae_Logic8 and https://sigrok.org/wiki/Saleae_Logic_Pro_16 . No new purchase recommendation is implied.

## Minimal scoped contract and UI

Use the existing `loom-domain-api` registry/worker model, `loom-proto::SourceRecord` → gateway `PublishedRecord`, and `core.subscribe` hub in `crates/loom-gateway/src/hub/{mod,stream}.rs`. Both viewer and CLI/agent consume the same published sequence/epoch; never give the agent a separate hidden vendor stream. Domain names below are proposals pending ROOT/backend owner agreement.

- Commands: `la.open`, `la.start`, `la.stop`, `la.status`, `la.close`; bounded capture/channel/rate configuration. Capability fields independently report `raw_live`, `decode_live`, `post_capture_export`, `decoder_change_live`; absent engine returns a clear unavailable result, never silently simulates hardware.
- Canonical events: capture/config/state, bounded raw chunk or retrievable chunk reference, waveform summary, annotation, error, stop acknowledgement. Include capture/decoder epoch, sample span, channel map/rate, source provenance and simulator flag. Sample time derives from sample index/rate; host receipt timestamp is separate, with synchronization uncertainty exposed.
- UI: visible real/sim and backend badge, channel lanes with time axis/zoom/live-follow, decoder rows with sample/time spans, shared start/STOP controls, capture progress/lag and explicit unsupported capability states. Retain previous rows after STOP; annotations alone must not be presented as raw-waveform support. Preserve digital edges rather than applying generic analog decimation.
- Bound memory, disk and pipe queues. Decoder stall must not silently block raw acquisition; either report decoder gap/epoch restart or stop explicitly. Distinguish `source.gap`, `record.gap`, and subscriber-only `display.gap`; control/STOP stays prioritized.
- Simulator generates deterministic raw I2C/SPI or UART pin transitions through the same chunk/decoder/publication contract, paced while capture is running, with source `sim`/synthetic timestamps. Prefer at least I2C or SPI to establish scope beyond UART telemetry. Fixtures may test parser failures but cannot establish installed engine/hardware support.
- Acceptance after decision: before STOP, UI and agent observe matching published event identities and nonzero raw samples + annotations; late join snapshot/replay works; STOP cancels/reaps both subprocesses; decoder stalls, truncated sample/line, missing executable and source loss are explicit. Real-engine demo validates process/decoder integration only; actual hardware model, driver, samplerate and latency need separate evidence.

Serial Studio primary sources: https://github.com/Serial-Studio/Serial-Studio/blob/master/doc/help/API-Reference.md (server-push formats, MCP exclusion, gRPC distinction), https://github.com/Serial-Studio/Serial-Studio/blob/master/doc/help/Data-Flow.md . These and master-branch sigrok sources can change; findings are source-inspected, not installed-version certification.

ROOT decision remains pending. Opus owns subsequent backend implementation per task instruction; no implementation or test execution started here. Browser research space closed. Ego Lite reported an available update; no upgrade was attempted.
