# READY_UI: uart FINAL fixture (qa_uart_final)
- Backend: LOOM_HOME=/tmp/loom-uart-audit-fixed/home, port 54139, loomd pid 23504 (started 09:08:45),
  `/tmp/loom-uart-secret-build/debug/loomd` SHA-256 4ac3ee715f97462ec2b7c1726cb9689b4e312ac2d838a7669b18d1c793e418ed (uart worker fix + gateway audit fix).
  CLI `target/debug/loom` SHA-256 22077e2af3e6bd448efc87561bffe299f384d79905f66b069f9a127a90ab824a. stream_epoch **1794614926**. Viewer 1/1 at login.
- Channel `qa_uart_final`: uart worker pid 24032, sim://bootshell?seed=20261114, generation 1, open w1, logged in as `root@sim:~# `.
- Secret login (dummy DummySecret1): request **w3**, written_bytes 13. `uart.data` tx **seq 42** = `{len:13, secret:{len:13}}`, quality ["redacted"], no bytes.
  Audit `write.intent` seq 5: `args.data = {"secret":{"len":12}}` (12 = secret without the CR the worker appends).
  Leak scan (`10-leak-scan.txt`, needles plain/base64 at 3 alignments/hex/HEX/spaced hex) over events, read, audit, all journals and manifest: **TOTAL_HITS 0**.
  UI check for ROOT: the DOM has no `DummySecret1`, and the panel shows `[secret 13 B]` and keeps streaming.
- Fresh observer (30 s, client `uart-final-observer`). It starts when ROOT says GO; send the marker within 30 s after that:
  `LOOM_CLIENT=uart-final-observer target/debug/loom events --follow --since now --channel qa_uart_final --timeout 30 --max 1000 --json`
- Producer: ROOT sends the fresh marker (I do not auto-send). Suggested: `echo QA-UART-UI-<nonce>` typed in the UART panel, or
  `LOOM_CLIENT=<root> target/debug/loom uart send qa_uart_final "echo QA-UART-UI-<nonce>" --expect "QA-UART-UI-<nonce>" --timeout 10 --json`.
- Compare: epoch 1794614926 seq / request_id / exact marker bytes between `21-observer.raw.ndjson` and the native UI DOM.
