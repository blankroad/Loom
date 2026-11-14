# READY_FOR_UI: uart native fixed (qa_uart_fixed)
- Backend: LOOM_HOME=/tmp/loom-uart-secret-native/home, port 53814, loomd pid 17384,
  `/tmp/loom-uart-secret-build/debug/loomd` SHA-256 06789da6fc4c890c9bec18e96f47993cbc593907a151b0833eb60e8a774dd4fa,
  stream_epoch 1794614502. Viewers at the secret login: subscribed 1 / visible 1.
- Channel: `qa_uart_fixed` (uart, sim://bootshell?seed=20261114, generation 1, logged in as `root@sim:~# `).
- The secret login is already done: request w3, `uart.data` tx seq 43 = `{len:13, secret:{len:13}}`, quality redacted, no bytes.
  UI check for ROOT: the DOM shows no `DummySecret1` and the panel keeps streaming.
- Fresh observer (30 s, client `uart-fixed-observer`):
  `LOOM_CLIENT=uart-fixed-observer target/debug/loom events --follow --since now --channel qa_uart_fixed --timeout 30 --max 1000 --json`
- Producer (one marker, client `uart-fixed-sender`):
  `LOOM_CLIENT=uart-fixed-sender target/debug/loom uart send qa_uart_fixed "echo QA-UART-UI-<nonce>" --expect "QA-UART-UI-<nonce>" --timeout 10 --json`
- Waiting for ROOT BEGIN. If ROOT sends the marker from the UI, I run the observer alone for 30 s.
