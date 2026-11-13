# Ethernet workbench — Sol benchmark

Scope: one independent Ethernet proposal in the shared Bench Light shell. Read only the UART Opus shell conventions, the Ethernet request, and relevant Ethernet plan contracts. No Ethernet Opus creative files read. Runtime is local HTML/CSS/JS without dependencies, CDN, capture backend, OS sockets, or request APIs.

## Primary sources and observations

Read official sources on 2026-11-14:

- [Wireshark main window](https://www.wireshark.org/docs/wsug_html_chunked/ChUseMainWindowSection.html): packet selection controls details and bytes; field selection highlights bytes. Keyboard packet navigation is documented.
- [Capture interface selection](https://www.wireshark.org/docs/wsug_html_chunked/ChCapInterfaceSection.html): multiple interfaces can capture simultaneously; interface addresses and filters are associated with each interface.
- [Capture filters](https://www.wireshark.org/docs/wsug_html_chunked/ChCapCaptureFilterSection.html) and [display filters](https://www.wireshark.org/docs/wsug_html_chunked/ChWorkDisplayFilterSection.html): capture admission and displayed results are distinct. Display filtering preserves captured data.
- [Follow protocol streams](https://www.wireshark.org/docs/wsug_html_chunked/ChAdvFollowStreamSection.html): directional application data, ASCII/hex representations, and a conversation selected from a packet.
- [Packet Sender official site](https://packetsender.com/) and [upstream GUI documentation](https://github.com/dannagle/PacketSender/blob/master/README.md#gui): named packets associate endpoint, transport, and data; editable saved packets and traffic logs support repeated manual tests. Persistent TCP has its own window. Server states and bind addresses are explicit.

The Packet Sender documentation endpoint returned HTTP 500, so the upstream README supplied the detailed primary documentation. Hercules returned HTTP 403; no Hercules observations are claimed. Packet Sender fulfills the request’s “and/or” benchmark.

## Official screenshot inspection, without a browser

ROOT fetched the official images, then authorized reuse. Sol inspected the exact local files with the native image viewer:

- [Wireshark official main window](../qa/wireshark-official-main.png), upstream [ws-main.png](https://www.wireshark.org/docs/wsug_html_chunked/images/ws-main.png). Observed: dense protocol-colored rows, compact filter above, expanded protocol tree below, selected field visibly linked to hex and ASCII, counts on the status line.
- [Packet Sender official GUI](../qa/packetsender-official.png), official upstream `ps_GUI.png` illustrated by the [GUI documentation](https://github.com/dannagle/PacketSender/blob/master/README.md#gui). Observed: named packet/data/address composer, Send/Save actions, saved-packet rows, a separate lower traffic log, compact protocol/server status.

Sol’s attempted duplicate curl download failed sandbox DNS and the optional escalation was declined. No duplicate image was downloaded and no permission was changed. These existing QA files are evidence owned by ROOT; Sol did not modify them.

## Adopted / rejected

| Reference | Adopted | Rejected or narrowed | Proposal inference |
|---|---|---|---|
| Wireshark packet workspace | Dense list, expandable decode, matching hex/ASCII, protocol tint, arrow-key selection | Broad menus and full protocol dissector | List above; details and bytes side by side for desktop space |
| Wireshark interfaces | Independently started channels, alias/IP/interface/count, explicit filters | NIC discovery and native capture | Visible SIM rail with two actual open channels and four-channel cap |
| Wireshark filtering | Capture/display distinction, clear/apply, invalid-state feedback | Full BPF and full display-filter language | Capture admission select; documented small display grammar, retaining prior valid results |
| Wireshark Follow | Directional payload conversation | TCP reassembly or claiming a complete Follow implementation | Honest “Selected conversation,” retained frames and no reassembly noted |
| Packet Sender composer | HEX/text, endpoint/port, send once, named editable templates, RX/TX log | Automatic resends, cloud, real servers/sockets | Dedicated Socket Test tab, deterministic echo, independent per-channel simulated connection |
| Packet Sender persistence | Retain settings and templates | Restoring active traffic | Reload restores configuration only; capture and connections stay stopped |

The rail placement, tab separation, blue/green conversation direction, 240-frame retention, and compact light chrome are design choices, not claims about upstream products. Bench Light tokens follow the authorized UART shell reference. There are no dashboard cards, synthetic throughput KPIs, agent chat, or inactive fake modules.

## Deliberate fidelity boundaries

IPv4, UDP/TCP checksums, lengths, payloads, ARP fields, DoIP identification headers/VIN fields and SOME/IP notification fields are constructed coherently. Frames are padded to a 60-byte minimum; FCS is explicitly omitted. TCP fixture payload frames omit a handshake and are not a TCP stack. DoIP inspection is layering only. Timestamps are deterministic fixture time (`id × 0.137 s`), not wall time. RX is a byte-for-byte simulator echo, not a protocol-valid DoIP response to every user payload. CSV includes displayed row metadata and exact frame HEX; no PCAP claim.
