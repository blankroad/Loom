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

For live acceptance, obtain nonzero raw edges and decoded annotations **before STOP**, and compare their published journal identities with the user's native logic panel. Post-stop exports or a process-start message alone do not establish live logging. Keep real/demo/synthetic provenance visible and state physical hardware verification separately.
