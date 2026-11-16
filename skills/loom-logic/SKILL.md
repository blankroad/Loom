---
name: loom-logic
description: Capture logic-analyzer pin waveforms and UART/SPI decoder annotations through Loom's sigrok engine, with the user and agent observing the same live journal. Use for loom logic, digital timing, pin captures, or logic-analyzer logging; ordinary UART port logs use loom-uart.
---

Use the [common Loom skill](../loom/SKILL.md) for viewer visibility, user intervention, STOP latch, errors and bounded envelopes. Start with `loom logic help --json` and `loom logic status --json`; status inspects engine capabilities, not USB devices. Never bypass Loom with a separate device owner.

Choose the source explicitly; there is no fallback:
- `synthetic`: Loom-generated pin traces decoded by the real sigrok decoder. This tests integration, not hardware.
- `sigrok-demo`: actual libsigrok demo acquisition with pattern data; decoded content has no DUT meaning. A named pattern currently requires 200 kHz.
- `sigrok-device`: only `fx2lafw` is an accepted **8-bit candidate**, physically unverified. Confirm the user's model and connection before starting. `saleae-logic16` and `saleae-logic-pro` are refused in this slice because 16-channel packing is not supported. A compiled driver listing is not a hardware support guarantee. Do not scan USB or load firmware without authorization.

Acquisition always uses D0…D7, one byte/sample. `--channels` filters displayed bits, not the acquisition packing. Raw `logic.edges` and decoded `logic.annotation`/`logic.annotations` are separate evidence: annotations are interpretations, not raw waveform samples. Align both using absolute sample index/rate, not host receipt time. Preserve decoder IDs such as `uart-1`; class can be null. Report dropped edges/annotations and unknown spans; never interpolate them as measured levels.

```sh
loom logic start --source synthetic --rate 1M --decode uart:rx=D0:baudrate=100000 --text 'LOOM {n}' --duration 10s
loom logic read --kind all --tail 100
loom logic wait --text 4C --decoder uart --timeout 5s
loom logic stop

loom logic start --source synthetic --rate 1M --decode spi:clk=D0:mosi=D1:cs=D2 --text LOOM --duration 10s
loom logic start --source sigrok-demo --rate 200k --pattern graycode --duration 5s
```

Use `--capture ID` to bind read/wait and `stop ID` to target a capture. Read returns flattened rows capped at 1000 rows/64 KiB; check `complete` and `rows_left_out` before treating them as complete evidence. `read --follow --wait 10s` is **one** bounded long-poll (maximum 30 s), not an endless subscription; resume with `--since` using the returned `next` cursor. `wait --text` matches annotation text; `wait --ascii` reconstructs ASCII from decoded byte annotations. Wait has a finite timeout, at most 600 s. For original PublishedRecords or general multi-domain streaming reuse `loom events --domain logic --follow --timeout 10` (maximum 600 s).

**Live preview vs whole evidence.** Live `logic.annotations`/`logic.edges` are a bounded preview (about 2000 rows/s and 10,240 edges/s). Above that, rows are counted in `dropped_annotations` and edges become unknown spans. Measured on synthetic 115200 continuous UART: about 78 % of rows never reach the live preview. When the daemon has an archive (`capture_state.archive.state` is `recording`/`complete`/`partial`), it keeps every decoded line and a lossless D0…D7 record within quota. Use it, not the live tail, as complete evidence:

```sh
loom logic archive list                                   # state, complete, truncation, sizes
loom logic archive read lc-… --kind annotations --limit 1000          # page with --after-row <next_row>
loom logic archive read lc-… --kind edges --from 0 --to 100000 --mask 0x01   # page with --from <next_from>
loom logic export lc-… --kind annotations --format csv    # writes inside the archive; prints the daemon-side path
```

Only `complete: true` is whole evidence. `partial` names its cause: quota/queue truncation (`truncated_at`) or a decoder gap (the decoder saw fewer samples than were acquired, e.g. `decoder backpressure`). A disabled or missing archive means only the live preview exists. Say so instead of implying completeness.

**UART both directions and decoder errors.** `--decode uart:rx=D0:tx=D1:…` decodes both lines in one capture. `uart-1` is RX and `uart-2` is TX; `capture_state.decoder_tracks` states the mapping. Rows carry a Loom-derived `role`: `data` (a hex word) or `error` (`Frame error`, `Parity error`, break). `counters.error_rows` counts the published error rows. `class` stays null. Errors break `wait --ascii` contiguity on purpose. For a synthetic error demo, add `--glitch-every N`.

**Software trigger.** `--trigger D3:falling --pre 10ms --post 500ms` arms a Loom software trigger on the acquired stream, for any source. While it is armed, nothing is published, decoded or archived (`capture_state.trigger.state = armed`). On the trigger, publishing starts at the pre-window. Sample numbers stay absolute (`trigger.sample`), and the capture stops with reason `trigger_post`. If it never fires, the state is `not_fired` with nothing published: report that, not "no signal". Hardware triggers are not used.

For live acceptance, obtain nonzero raw edges and decoded annotations **before STOP**, and compare their published journal identities with the user's native logic panel. Post-stop exports or a process-start message alone do not establish live logging. Keep real/demo/synthetic provenance visible and state physical hardware verification separately.
