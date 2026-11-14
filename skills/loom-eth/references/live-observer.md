# Live observer proof (agent + UI see the same packets)

Use this when you must show that packets arrive in real time, both for you and for the user's Loom UI.

1. Check `loom eth ifaces` first and capture on a returned name. No NIC: `capture start sim --backend sim --ch <ch>`.
   In sim, prefer `send raw` because it stays in-process. `send udp` opens a real OS socket, even on sim.
2. Run the observer under its own identity (`LOOM_CLIENT=<you>-observer`) and the sender under another. Bound it:
   `loom events --since now --channel <ch> --follow --timeout 25 --json --max 200`. Keep every follow at 30 s or less. Never renew
   automatically and never poll in a loop. Record the host receive time for each output line.
3. Before any send, run `send … --dry-run`. Then send once with `--idempotency-key`, and never retry automatically.
4. Correlate using fields, not prose:
   - the send's `request_id` must equal the eth.tx `request_id` (requested → accepted → sent);
   - eth.tx `observed_src_seq` must equal eth.pkt `layers._capture.source_seq`;
   - eth.pkt `link.request_id` must be the same request (`confidence: exact`);
   - the bytes must match the sent hex exactly.
   Quote REFs such as `eth:<ch>@seq:N`.
5. Device proof means packets with origin device that Loom did not send, for example the sim's native SOME/IP-SD offers at about 1 Hz.
   Your own echoed frame (`attributed_origin: agent`, ORG `AGT`; its event `origin` may read `device`) proves
   correlation only, never device behaviour.
6. A UI pass needs the user/ROOT viewer on the same daemon (`viewers N/N visible`, same `stream_epoch`). It also needs
   that viewer's native row (`data-seq`) to match your seq, request_id and bytes. A `no_viewer_attached` warning means agent-only:
   say so and do not claim a UI pass.
7. Stopping a capture keeps its history queryable (`trace`/`show` by seq). Stop only when the user/ROOT is done looking.
