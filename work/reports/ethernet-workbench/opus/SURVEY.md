# SURVEY — Opus Ethernet workbench benchmark

Model `claude-opus-5-5`, native Claude Code foreground, 2026-11-14. Narrow scope: Ethernet capture + TCP/UDP test only. The Sol proposal was not read.

## Primary sources (read this session)
| # | Source | What was used |
|---|---|---|
| W1 | Wireshark User's Guide — *The Packet List Pane* (wireshark.org/docs/wsug_html_chunked/ChUsePacketListPaneSection.html) | Columns No./Time/Source/Destination/Protocol/Length/Info. No. is stable under display filter. ↑↓ when focused, **Ctrl+↓/F8, Ctrl+↑/F7** work without focus. Conversation-related symbols in the first column. |
| W2 | WSUG — *Filtering Packets While Viewing* (ChWorkDisplayFilterSection.html) | A display filter changes only the view, "all packets remain in the capture file". Apply with Enter, Clear button. |
| W3 | WSUG — *Building Display Filter Expressions* (ChWorkBuildDisplayFilterSection.html) | Operators `== != > < >= <=` with English forms `eq ne gt lt ge le`. `!=` means all values differ. `contains`. `and/&& or/|| not/!`. Examples `ip.addr==…/16`, `frame.len > 10`. |
| W4 | WSUG — *The Filter Toolbar* (ChUseFilterToolbarSection.html) | The field turns **red for an incomplete/invalid string** and green when valid. Apply, Clear, Recent and Bookmarks controls. |
| W5 | WSUG — *Filtering while capturing* (ChCapCaptureFilterSection.html) | Capture filter = libpcap syntax, entered in Capture Options. Non-matching packets are never recorded. Primitives `[tcp|udp] [src|dst] port`, `host`, `ether host`, `less/greater`, and/or/not. |
| W6 | WSUG — *Following Protocol Streams* (ChAdvFollowStreamSection.html) | Client→server **red**, server→client **blue**. Direction selector. "Show data as" ASCII / Hex Dump / Raw / … Filter Out This Stream. **Not updated during live capture.** |
| W7 | `qa/wireshark-official-main.png` (official screenshot, root-provided, viewed locally) | Three stacked panes: dense list, tree, hex+ASCII. Selected field highlighted in the bytes pane. Status bar `Packets: N · Displayed: N (100%)`. Protocol row colouring. |
| PS1 | Packet Sender README (github.com/dannagle/PacketSender) | Saved packet = Name, ASCII/HEX, Address, Port, Resend Delay (0 = single shot), Protocol, Send. Mixed ASCII escapes `\XX \n \r \t`. Hex tolerates commas, colons, `0x`, odd-length input. Persistent TCP has Raw/ASCII views. |
| PS2 | `qa/packetsender-official.png` (viewed locally) | Composer on top (Name/ASCII/HEX/Address/Port/Resend/Proto/Send/Save). Saved-packets table with a per-row **Send**. Traffic log with Time/From/To/Method/Error/ASCII/Hex. |
| H1 | HW-group Hercules page (hw-group.com/software/hercules-setup-utility) | Separate TCP Client/Server/UDP tabs. Module IP + Port, **Connect/Disconnect**. Send fields with a **HEX checkbox**. Received/Sent data window. Escapes `$yy` / `#ddd`. |

Not fetched: packetsender.com/documentation returned HTTP 500. The README was used instead.

## Adopted vs rejected
| Decision | Basis | Status |
|---|---|---|
| Three panes: list / details tree / hex+ASCII, all derived from the same frame bytes. Field↔byte highlight in both directions. | W1, W7 | **Adopted** (documented) |
| List columns No./Time/Source/Destination/Protocol/Length/Info plus a **Ch** column for multi-channel | W1 + inferred | Adopted. Ch is a Loom addition. |
| Global frame numbers stay stable under filter and scope | W1 | Adopted |
| ↑↓/Home/End/PgUp/PgDn in the list. F7/F8 and Ctrl+↑/↓ global. | W1 | Adopted |
| Display filter red/green validation. Invalid input is never applied and the previous result is kept. Recent filters, examples and field suggestions. | W3, W4 | Adopted. Bookmarks rejected (scope). |
| Display filter is a **documented subset**: protocols, the fields listed in `pkt.js FIELDS`, ops `== != > < >= <=`/English forms, `contains` on string fields, `&& || !`/`and or not`, parentheses, `ip.addr == a.b.c.d/nn` | W3 | Adopted (subset). `matches`, `in {}`, `xor`, slices and field-to-field comparisons are rejected. |
| `loom.ch == "alias"` field | inferred | Adopted. It is labelled as a Loom extension because Wireshark has `frame.interface_name`, which is also supported. |
| Capture filter per channel, BPF subset, applied at capture time, locked while LIVE, with a count of excluded packets | W5 | Adopted (subset). The distinction from the display filter is shown in the UI. |
| Wireshark-style relative TCP seq/ack in Info and tree, with the raw values also shown | W7 (screenshot shows relative values) | Adopted |
| Row colouring (TCP lavender, UDP light blue, ARP cream, SYN/FIN grey, RST red) plus DoIP/SOME/IP tints | inferred from the W7 look; exact default colour rules **not verified this session** | Adopted as a light adaptation, with a toggle |
| Follow Stream: red client / blue server, direction selector, ASCII/Hex Dump/Raw, "only this stream" and "exclude" filters, snapshot without live update (with a refresh button) | W6 | Adopted. C Arrays, EBCDIC and YAML rejected. |
| Export: CSV of the displayed list columns, plus **classic libpcap `.pcap`** of the displayed frames (LINKTYPE_ETHERNET) | inferred (honest format) | Adopted. pcapng with per-interface blocks is rejected and stated as a limit. |
| Socket test as a **side drawer** (Packet Sender composer + saved list, Hercules Connect/Disconnect + HEX toggle) | PS1, PS2, H1 | Adopted. A drawer keeps the capture workspace primary. |
| HEX/Text input: hex tolerates spaces, commas, colons, `0x` and odd length. Text escapes are `\r \n \t \\ \xHH`. | PS1 | Adopted. Hercules `$yy/#ddd` rejected in favour of a single escape grammar. |
| Templates (Name, proto, port, fmt, data) with run/edit/delete. Run **never auto-connects** and is blocked on a proto/port mismatch. | PS1, PS2 (Save, per-row Send) | Adopted. A safety rule was added. |
| Resend delay → explicit count × interval repeat, stopped by Disconnect, never restored on reload | PS1 (Resend Delay) | Adopted with a bound of 1–1000 sends at 50–60000 ms |
| Packet Sender's TCP/UDP/SSL **server** listeners and multicast | PS2 | **Rejected**. The scope is a TCP client and a UDP socket against deterministic SIM peers. |
| Hercules Server echo, Ping and NVT | H1 | Rejected (scope) |
| Wireshark menus, I/O graphs, statistics, intelligent scrollbar | W7 | Rejected (scope, density budget) |
| Agent chat or KPI dashboard | REQUEST | Rejected |
