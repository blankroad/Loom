# External DoIP server smoke — 2026-11-14

Result: native TCP/DoIP interoperability PASS for one client, external ECU fixture. Browser workbench end-to-end connection is NOT implemented; its Connect button still controls engine.js local simulation.

## Independent server

- Upstream: https://github.com/Alonso07/doip_server / https://pypi.org/project/doip-server/
- Pinned package: doip-server 2026.6.14.post116, Python 3.14, temporary isolated venv.
- Unmodified upstream implementation; own three YAML fixtures alongside this report.
- Bound only to 127.0.0.1:52388 (TCP+UDP), ECU 0x1001, accepted tester 0x0E80.
- Entry point: `python -m doip_server.main --gateway-config /absolute/path/gateway1.yaml`.
- The console-script `doip_server --help` does not parse CLI flags in this release. Without an existing default config it creates config/gateway1.yaml then fails because `logger` has not been initialized. Use the explicit module/config command above; no upstream code was patched. Its generated workspace file was moved to the owned temp directory and the now-empty directory removed.

## Actual round trip

Native CLI and daemon were the existing 2026-11-12 built executables in target/debug. Their hashes are recorded in binary-sha256.txt. They were NOT rebuilt from the newly pushed transport refactor. Earlier current-source Rust tests remain separate evidence in ../transport/REVIEW.md.

A separate loomd ran foreground with its own temporary LOOM_HOME; existing user daemons were untouched.

```sh
LOOM_HOME=/tmp/loom-doip-interop-20261114/home target/debug/loomd run --port 0 --env sim --rights operate
LOOM_HOME=/tmp/loom-doip-interop-20261114/home target/debug/loom --json uds connect --host 127.0.0.1 --port 52388 --la 0x1001 --tester-la 0x0E80 --name interop --no-reconnect
LOOM_HOME=/tmp/loom-doip-interop-20261114/home target/debug/loom --json uds read-did F190 -t interop
LOOM_HOME=/tmp/loom-doip-interop-20261114/home target/debug/loom --json uds raw 22FFFF -t interop
```

- TCP connection and Routing Activation accepted; target link up/generation1.
- VIN: 22 F1 90 → 62 F1 90 + ASCII `LOOMINTEROP0000001`; native result confirmed, RTT0.412ms.
- Unsupported DID fixture: 22 FF FF → 7F 22 31; native E_NRC/requestOutOfRange, RTT0.332ms, no false success.
- Server Ctrl-C → native target link down, cause `early eof`; session/security confidence stale. after-server-stop.json records this.
- native-trace.json contains the actual journal TX/RX/results. server-transcript.txt contains upstream server output showing RA, source/target, DoIP ACK and response generation.

## Candidate limitation found

A second connection using tester0x0E81 while the first connection remained open timed out in Routing Activation. This does NOT establish that the client correctly decodes an explicit routing rejection. Installed server source calls blocking `handle_client(client_socket)` directly from its accept loop; recv remains blocking, so another TCP client is not serviced while the first is active. No claim of multi-client interoperability or explicit RA-denial coverage. Use this server for a single tester smoke, and use Loom's Rust DoipEntity or a suitably concurrent independent server for multi-client/fault tests.

## Browser integration still required

The current static workbench has IP/port/tester LA/ECU LA configuration but no backend transport. Next integration should use `workbench → loomd RPC/event stream → native UDS runner/DoIP client → external server`. Browser TCP sockets are not available to this ordinary web app; do not simulate success in the UI.

Acceptance for that integration:

1. Explicit local SIM vs backend DoIP mode; retain configured target/profile snapshots.
2. Show TCP connecting, routing activation, ready, failure and disconnected states; Ready only after RA succeeds.
3. Disable sending until backend ready; manual DID/Raw, Watch and sequences share the same real request path.
4. Show actual request/response/NRC/timing and route; rejected or late responses cannot update confirmed values.
5. Real browser test: configured external server → Connect → VIN read, raw NRC, disconnect and server-loss state. Add unavailable port, RA rejection and reconnect tests using an appropriate fixture.

No UI-to-real-server PASS is claimed. Standard local DoIP smoke does not prove OEM-specific Hyundai/Kia compatibility or hardware operation.

## Cleanup

External server and owned daemon stopped normally; foreground sessions exited0. Own temporary venv/config/daemon data removed after copying this evidence. No dependency, Cargo build or user daemon changes.
