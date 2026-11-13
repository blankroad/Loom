# HANDOFF — Opus Ethernet workbench (one proposal) · READY_FOR_REVIEW (source freeze 1)

- **Model / session:** `claude-opus-5-5`, native Claude Code foreground, session `a84fa94b-0768-49d3-984f-00ad44d0fed4`. No subagents, no fallback, **no browser operated** (root owns Ego). Base `main 4fb848a` untouched; no git, install, product, STATUS, package or settings changes. Sol files were not read.
- **Owned files:** `work/reports/ethernet-workbench/opus/` → `index.html`, `styles.css`, `pkt.js`, `app.js`, `sock.js`, `tools.js`, `SURVEY.md`, `HANDOFF.md`, `CHECKPOINT.md`, `selfcheck/{check.cjs, selfcheck-result.json, source-sha256-freeze.txt}`.
- **Run:** existing preview server at `http://127.0.0.1:4180/ethernet-workbench/opus/index.html`. No build, no CDN, no network. Scripts load in this order: `pkt.js → app.js → sock.js → tools.js`.
- **Source sha256 (freeze 1):** `selfcheck/source-sha256-freeze.txt`

| file | sha256 (prefix) |
|---|---|
| index.html | `afc51f89…` |
| styles.css | `d3b22f7d…` |
| pkt.js | `70dd5bf4…` |
| app.js | `78f74d20…` |
| sock.js | `0c7cdf95…` |
| tools.js | `7c77056a…` |
| selfcheck/check.cjs | `9d764f84…` |

## Root findings addressed
| id | fix | check |
|---|---|---|
| O1 | `parseText` collects literal spans and encodes whole code points with TextEncoder. The escapes `\r \n \t \\ \xHH` are unchanged. | `O1 parseText encodes full code points + escapes`, plus a node case: `A😀한` → `[65,240,159,152,128,237,149,156]` |
| O2 | `rebuildView()` (on filter, scope, clear or resume): if the selection is excluded, the first match is selected with a toast. If nothing matches, the selection and the detail/bytes panes are cleared with a toast. There is no silent mismatch. | `O2 filter excludes…`, `O2 empty filter result…`, plus scope-change and kept-selection cases |
| O3 | `minmax(0,1fr)` columns on `.shell`, `.mod-eth`, `.cap` and narrow `.lower`. `min-width:0` on module and pane children. `.pl-rows` is `width:max-content` so the table scrolls horizontally inside `#plBody`, and `.pl-head` scrollLeft is synced. There is no whole-document overflow mask. | root native PASS at 390/768 |
| O4 | `.pl-notice[hidden], .pl-empty[hidden] { display:none }`. The notice is also emptied when hidden. | `O4 resumed: paused notice hidden…` |
| O5 | Socket traffic, repeat progress and RX use the change-detecting `render()`, so the panel is rebuilt only on structural changes (state, proto, peer, port, repeat on/off, reason, editing, format, channels, templates). The log updates by version. Repeat count and interval are kept in the draft, so a rebuild no longer resets them to 10/200. The paused banner keeps its Resume button node and updates only the count. | `O5 repeat traffic keeps Stop/inputs/log DOM nodes stable…`, `O5 Stop click on the captured node stops repeat`, `paused banner resume button node stable` |

An extra fix found by the checks: programmatic selection with scroll (F7/F8, O2 auto-select, transcript `#frame` link) now pauses autoscroll, so the selected row stays in view.

## Selectors
- **Shell:** `.svcnav a[data-svc=uart]` → `../../uart-workbench/opus/index.html`, `a[data-svc=eth][aria-current=page]`, `.env-chip` SIM, `#shellConn` (`ETH Nch · N LIVE`), `#stTarget`, `#stExtra`.
- **Toolbar:** `[data-cmd=add-channel|start-all|stop-all|pause-view|autoscroll|colorize|follow|export|clear-view|toggle-socket|keys]`. `#tbPause[aria-pressed]`.
- **Channels:** `.chcard[data-ch]`, `[data-role=cap-state]` LIVE/STOPPED, `[data-role=cap-count]`, `[data-act=ch-start|ch-stop|ch-menu|ch-select]`. The menu `#menu [data-mi]` has start/stop, this channel only, settings, burst 500, close. The add/settings dialog is `#dlgChannel form` (fields `alias`, `iface`, `profile`, `cf`, `start`). At most 4 channels; `sim0–sim3` are SIM names only.
- **Scope & filters:** `#scopeSeg [data-scope=all|<chId>]`, `#capfSummary`, `#dfInput` (.valid/.invalid/.applied-mismatch, aria-invalid), `#dfSug li[data-v]`, `#dfApply`, `#dfClear`, `#dfCount [data-role=shown|captured]`, `#dfMsg(.err)`.
- **Capture:** `#plBody` (grid, keyboard), `.prow[data-no].sel/.own/.conv`, `#plNotice` (paused `[data-role=pending]` / retention `[data-role=dropped]`), `#divList` (row drag), `#divLower` (col drag, row drag at ≤860px), `#tree .tn[data-id][data-o][data-n]`, `#hex .hx-h b[data-o]`, `.hx-a b[data-o]`, `#bSel`.
- **Socket drawer `#sock`:**
  - Endpoint: `#skCh`, `[data-role=sk-state]`, `[data-act=sk-proto][data-v=tcp|udp]`, `#skHost`, `#skPort`, `[data-act=sk-connect|sk-disconnect]`, `[data-role=sk-why]`.
  - Send: `[data-act=sk-fmt][data-v=hex|text]`, `#skData`, `#skErr`, `#skPrev`, `[data-act=sk-send]` (Ctrl/⌘+Enter in the textarea).
  - Repeat: `#skRepN`, `#skRepMs`, `[data-act=sk-rep|sk-rep-stop]`.
  - Log and templates: `#skLog [data-frame]`, `#skTplName`, `[data-act=tpl-save|tpl-update|tpl-cancel]`, `.tpl[data-tpl] [data-act=tpl-run|tpl-edit|tpl-del]`.
- **Dialogs:**
  - `#dlgFollow`: `[data-fs=dir|as]` selects, `[data-fs=refresh|only|exclude|close]`, `#fsBody .c/.s`.
  - `#dlgExport`: `[data-ex=csv|pcap|close]`.
  - Also `#dlgKeys` and `#dlgConfirm [data-r]`.
- **Test hooks:** `window.loomEthOpus.snapshot()` and `.tick(n, ms)`; `window.LoomEth` / `LoomEthSock` / `LoomEthTools`; `window.__loomEthLastExport`. `__loomEthManualClock` and `__loomEthRetain` are for jsdom only.

## Scenarios (for native QA)
1. **Load:**
   - Two SIM channels are LIVE: `ecu-doip` (sim0, 192.168.10.10/24) and `body-someip` (sim1, 10.0.3.10/24).
   - ecu-doip shows 3× DoIP vehicle announcements (UDP 13400 broadcast), then ARP, the TCP handshake, routing activation 0x0005/0x0006, TesterPresent every 2 s and ReadDID F190 every 5 s.
   - body-someip shows SOME/IP-SD offers every 1 s, notifications (0x1234/0x8001) every 0.5 s, and a TCP 7000 `STAT?` text exchange every 2 s.
2. **Select a row:**
   - The tree shows Frame → Ethernet II → IPv4 (`[correct]` checksum) → UDP/TCP (relative seq, `[correct]`) → DoIP/UDS or SOME/IP(-SD).
   - Clicking a field highlights its bytes; clicking a byte selects the deepest field. ↑↓, F7/F8, Home/End and PgUp/PgDn work.
   - Dragging `#divList` and `#divLower` resizes the panes, and the sizes persist.
3. **Display filter:**
   - Examples: `doip`, `someipsd`, `tcp.port == 7000`, `ip.addr == 192.168.10.0/24`, `loom.ch == "ecu-doip"`, `frame.len > 100`.
   - `doip.type ==` shows red with a message; Apply keeps the previous filter and its rows.
   - The count reads `표시 n / 캡처 N` per scope.
4. **Stop and add channels:**
   - Stopping one channel freezes only that channel (history kept); the other keeps counting.
   - Add a channel `lab-gen` with capture filter `not arp`: no ARP is stored, the card shows the `필터 제외` count, and the capture filter is locked while LIVE.
5. **Pause view vs Stop:** Pause view freezes the list with a pending count while capture continues; Resume catches up. Burst 500 (channel menu) passes the 2,000-per-channel bound eventually, then the drop notice appears.
6. **Socket test:** open it with Alt+S, target ecu-doip, then run these steps.
   1. Send is disabled while disconnected, and running a template is blocked with a reason.
   2. Connect: SYN, SYN-ACK and ACK appear in the list with `TX` marks.
   3. Template "DoIP 라우팅 활성화" gets RX 0x0006 success; "ReadDID F190" gets ACK 0x8002 plus 0x62 F190 with the VIN.
   4. Transcript `#n` links select the frame.
   5. Hex `zz` is an error and nothing is sent.
   6. Repeat 50×100 ms, then Stop.
   7. Disconnect sends FIN and blocks Send.
   8. Port 1234 gets RST and the state shows refused.
   - For body-someip, switch to UDP SOME/IP GetVersion; it gets a Response 0x80. The two channels' sessions are independent.
7. **Follow and export:**
   - Select a TCP 7000 row → Follow: client red `STAT?`, server blue `STAT seq=…`, with ASCII/Hex Dump/Raw views. "이 스트림만 표시" applies `tcp.stream == N && loom.ch == "body-someip"`.
   - Export → CSV or pcap of the displayed rows; the toast says how it is delivered through the browser download.
8. **Reload:** channel config, filter, layout, templates and the composer draft are restored. No socket session is restored, no repeat restarts, and no TX is sent.

## Checks (jsdom only, not browser acceptance)
- `node work/reports/ethernet-workbench/opus/selfcheck/check.cjs` (run from repo root; jsdom resolved relatively from the existing `ui/node_modules`, nothing installed) → **60/60 PASS** (`selfcheck/selfcheck-result.json`), run on the freeze-1 hashes above.
- It covers: fixture protocol coverage; frame coherence for every displayed packet (re-decoded info, all checksums `correct`, ip.src bytes, length); select/keyboard/field↔byte; valid/invalid/CIDR/loom.ch filters and scope; O2; independent stop/start; pause vs capture plus O4; capture filter validate/apply; retention bound and drop; the socket flows (blocked, connect, hex error, template TX/RX, port mismatch, independent B, UDP SOME/IP, save/edit template, repeat plus O5 identity/Stop, disconnect, RST); follow; pcap structure (magic, linktype 1, count and lengths equal the displayed rows); CSV; storage prefix; reload without auto-connect or send; O1; close channel.
- Also: `node --check` passes on all four scripts, and there are no external URLs in the runtime files.

## Known limits
- **Display filter subset:** no `matches`, `in {}`, `xor`, slices or field-to-field comparisons. Address fields support only `==`/`!=`.
- **Capture filter:** BPF subset as documented in the channel dialog.
- **Follow Stream:** a snapshot when opened (as in Wireshark), refreshed manually. The client is the first SYN sender, or the first packet's sender for UDP. It joins the **retained** segments' payloads in capture order. It does **not** reassemble TCP by sequence number and does not handle retransmissions, out-of-order segments or gaps (including segments lost to the retention limit). It is not the full Wireshark stream engine.
- **pcap:** classic libpcap without channel or interface metadata (pcapng not implemented). CSV is a list-column export.
- **SIM peers:** fixed deterministic stubs (DoIP routing/ACK, a few UDS replies, SOME/IP GetVersion, echo 7, STAT 7000). This is DoIP layering only, not a UDS workflow. There are no TCP/UDP server listeners, no IPv6 and no VLAN.
- **Same-profile channels:** two channels on the same profile reuse the same SIM subnet. Their conversations are separated by `loom.ch` in Follow, but `tcp.stream` indices are shared by 5-tuple.
- **Pause:** while paused, packets evicted by retention can still show in the frozen view until Resume.
- **Row colours** are a light adaptation of the Wireshark default colouring rules, recalled rather than verified this session.
