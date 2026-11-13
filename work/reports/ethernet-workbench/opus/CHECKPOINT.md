# CHECKPOINT — Opus Ethernet workbench (structure chosen)

- Model/session: `claude-opus-5-5`, native Claude Code foreground, session a84fa94b-0768-49d3-984f-00ad44d0fed4. No subagents, no browser, no git/install.
- Scope: `work/reports/ethernet-workbench/opus/` only. Read: REQUEST.md, RUN.md, eth/PLAN.md (selected), UART opus index.html/styles.css (shell conventions only; untouched).

## Chosen structure
Bench Light shell (44px header with UART | Ethernet nav, SIM chip, 26px status bar) + Ethernet module:

```
┌ shell header: Loom · [UART] [Ethernet*] ............. SIM · conn-chip ┐
├ module toolbar: Start/Stop ch · Pause view · Clear · Follow · Export · Socket test · Keys
├ channel strip (only open channels): [● ecu-a sim0 10.0.0.1 LIVE 1234] [● tester sim1 …] [+ 채널]
├ filter rows: Capture filter (per channel, read-only while live)  |  Display filter [ ... ] Apply/Clear  ·  shown/captured
├ ┌ packet list (dense table, virtual window) ───────────────────┐ ┌ Socket test drawer (toggle) ┐
│ ├ ═ drag ═                                                     │ │ target ch / proto / peer     │
│ ├ details tree (expand)  ║drag║  bytes hex/ascii (field↔byte)  │ │ conn state, HEX/text, send   │
│ └──────────────────────────────────────────────────────────────┘ │ transcript, templates        │
└ status bar: displayed/captured · drop notice · paused/live
```

- Traffic: deterministic generator per channel builds REAL byte arrays (Eth II + IPv4 w/ correct checksum + UDP/TCP/ARP + DoIP(13400) + SOME/IP(30490/30501)); decoder walks bytes → tree with [off,len] ranges → hex highlight. List length = bytes.length.
- Display filter: tiny parser (proto names, `ip.addr==`, `ip.src/dst==`, `udp.port/tcp.port==`, `frame.len >,<,==`, `ch ==`, `doip.type ==`, `&&`, `||`, `!`, parentheses). Invalid → red, previous results kept.
- Retention: per channel ring 2,000 packets, drop counter shown.
- Pause view ≠ Stop capture. Follow stream = selected 5-tuple conversation, directional colours. Export = CSV of displayed rows + classic libpcap `.pcap` (real headers) of displayed packets.
- Persist: localStorage prefix `loom.eth.opus.v1.` (channels config, templates, layout ratios, filter); reload never auto-starts socket send/repeat.

## Status
- [x] research read (Wireshark UG packet list, display filter, filter toolbar, capture filter, Follow Stream; Packet Sender README; Hercules page) — SURVEY.md written at freeze
- [x] index.html + styles.css (shell + module skeleton)
- [x] pkt.js engine: builders (Eth/IPv4/UDP/TCP/ARP/DoIP/SOME/IP(-SD)) with real checksums, decoder tree [off,len], display-filter subset, BPF capture subset, libpcap writer, HEX/text input parser. Node smoke: all 4 sample frames decode, checksums `correct`, filter errors as expected.
- [x] app.js (capture/list/detail/bytes/filters/pause/dividers) → sock.js (socket test/templates) → tools.js (follow/export)
- [x] root findings O1–O5 fixed
- [x] selfcheck (jsdom, existing ui/node_modules) 60/60 PASS
- [x] HANDOFF + SURVEY + `selfcheck/source-sha256-freeze.txt` → **READY_FOR_REVIEW (source freeze 1)**
