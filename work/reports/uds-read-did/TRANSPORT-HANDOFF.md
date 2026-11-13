# UDS transport boundary — Opus TRANSPORT-HANDOFF

Status: **ACCEPTED**. ROOT final acceptance is recorded in [`qa/transport/REVIEW.md`](qa/transport/REVIEW.md). SOURCE_FREEZE (§5) is unchanged. READY_RETURN. The plan in §1–§3 was written before any source edit. Opus 5.5 w1F, base main 4fb848a. Opus owns `crates/loom-uds-runner/`, `crates/loom-uds/` and this file only. ROOT owns the task, QA and shared docs.

## 0. Preservation
- The accepted mock is read-only. Its SHA-256 values below were taken before the edit and are re-checked at freeze (`shasum -a 256 app/*.js app/*.html app/*.css`):
  - `11413b50abce503953560cc4005e8f384d94e403f0d0e0d1487bef28a97cfa4b  app/app.js`
  - `2b3592854ffe8cee8898c8733d46647f829b2bb6c2c365b8d64ef54adcd22231  app/config.js`
  - `2b0dd5cf3cafdfeb4ebc551f3d0880f7e75fc0dc523c72471a400faedfa5beb9  app/engine.js`
  - `db1b9ce474f5bc5d0619233dd565280236f0112549ef5fbd774640abd259acf5  app/tools.js`
  - `d7c5d8c35252ee536dc3b1f5bbb5a8bf0b03a9932db59b9cd490e6bfd46b6faa  app/index.html`
  - `1cc606571d961bef067aafe3ba8486ff488713cd00cc744abe0023f06aa38c45  app/styles.css`
- No changes to the root `Cargo.toml`, `Cargo.lock`, dependencies or crates.
- Builds use `/tmp/loom-uds-transport-check` only. The root `target/` is not touched.

## 1. Current boundary (audit of the actual Rust code)
```
 RPC/CLI handlers ──► UdsManager (loom-uds/manager.rs)
                        │  lease · lanes · STOP · rows/events · session belief
                        │  Target { la:u16, client_cfg:ClientConfig, Gate{client:DoipClient} }
                        │  connect():   DoipClient::connect + open_target(la)       ◄─ DoIP hardwired
                        │  reconnect(): DoipClient::connect + open_target(la)       ◄─ DoIP hardwired
                        │  watch_link(): mpsc::Receiver<loom_uds_doip::LinkEvent>   ◄─ DoIP hardwired
                        ▼
                      RunnerHandle / Runner (loom-uds-runner)   ── already transport-neutral:
                        P2/P2*/0x78/deadline/RxStart completion/U quarantine/generation/outcome_unknown
                        ▼  Box<dyn Transport>  (generation/submit/confirm/recv/try_recv)
                      DoipTransport (always compiled; loom-uds-doip is a hard dependency)
```
**Native vs mock:** the browser mock (`app/engine.js`) has a local SIM route selector for DoIP/CAN-TP/EthDiag. It does **not** connect to native transports. A selectable route there does not mean a native protocol exists. Natively there is only DoIP; `connect --sim` is DoIP over loopback to a `DoipEntity`.

## 2. Target boundary
```
 UdsManager ── connect(ConnectSpec)  ── builds DoipConnector (+ SimSide for --sim) ─┐
            └─ connect_link(LinkSpec{name, env, backend, auto_reconnect,           │
                                   connector: Arc<dyn Connector>}) ◄───────────────┘ (same path)
               Target { connector, Gate{ resources: Box<dyn Any+Send> } }
               initial connect / reconnect(gen+1) ─► connector.connect(generation) ─► Link
 loom-uds-runner::link (neutral, always built):
   trait Connector { endpoint() -> String; connect(generation) -> BoxFuture<Result<Link, ConnectError>> }
   struct Link { transport: Box<dyn Transport>, events: Box<dyn LinkEvents>, resources: Box<dyn Any+Send> }
   trait LinkEvents { next() -> BoxFuture<Option<LinkSignal>> }   LinkSignal::{Down{reason}, RxOverflow{dropped_total}}
   (EOF = None → manager treats it as link down, cause "link signal source ended", then reconnect policy)
 loom-uds-runner::doip (feature "doip", default on): DoipTransport + DoipConnector (LA/ClientConfig/client state live here)
```
- **UDS logic stays in the runner and manager, unchanged.** Every transport uses the same runner. No link framing is handled above the adapter.
- **The manager verifies the link generation.** It rejects (drops) a link whose `transport.generation()` is not the requested generation, so generation stays monotonic whatever the adapter does.
- **A failed connect must leave nothing behind.** On failure, `connect()` must have released everything it allocated. `resources` owns the background tasks and sockets, and is dropped when the link is replaced or the target goes away.
- **No backend enum and no EthDiag frame types.** A future adapter supplies its own `Connector` plus `Transport`.

## 3. Steps
1. Runner: add a `doip` feature (default) that makes `loom-uds-doip` optional. Add `link.rs` with the neutral contract and `DoipConnector` (in `doip.rs`). `doip_e2e` gets `required-features = ["doip"]`.
2. Manager: `Target` holds the connector. `connect`, `reconnect` and `watch_link` go through `Connector`/`LinkEvents`. `ConnectSpec` keeps the DoIP/SIM path and the same JSON, errors and events. Add the public `connect_link`.
3. Tests:
   - `loom-uds/tests/transport.rs`: an external fake connector (FakeTransport-based, no DoIP) covering exact DID/raw bytes, `0x78` pending, read timeout, a write in flight at link loss → `outcome_unknown`, no resend after reconnect, failed reconnect attempts that leak no resources, wrong-generation rejection, and generation monotonicity.
   - Runner `--no-default-features` tests.
   - Existing DoIP/SIM regressions.
4. Format, clippy and targeted tests, then freeze.

## 4. Progress log
1. Plan written. Bounded target dir confirmed unused; root `target/` is 92M.
2. Runner changes:
   - `link.rs`: the neutral contract.
   - `DoipConnector` and `DoipLinkEvents` added in `doip.rs`.
   - The `doip` feature (default) now gates `loom-uds-doip`. `doip_e2e` has `required-features`.
3. Manager: `Target` now holds `connector: Arc<dyn Connector>`, and `Gate.link` holds the link resources. The `la`, `ClientConfig` and `DoipClient` fields are gone from `Target`; the SIM ECU LA moved into `SimSide`.
   - `connect(ConnectSpec)` now builds a `DoipConnector` and goes through the private `attach()`, the same path as the new public `connect_link(LinkSpec)`.
   - `open_link()` enforces the generation contract.
   - `reconnect()` and `watch_link()` are transport-neutral.
4. ROOT review fixes, applied:
   - The CAN generation contract no longer claims that rebuilding the receive path is proof. It requires a verified adapter- or vendor-specific response-epoch fence or drain covering every outstanding transaction; otherwise the reconnect fails and the ambiguity is kept. It gives no prescriptive reset, session or P2* examples, and adds no automatic ECU actions.
   - LinkEvents EOF now means link down.
   - The `confirm`/P2-start doc now covers segmented TX.
5. Tests added (`crates/loom-uds/tests/transport.rs`):
   - First run: 1 failure, caused by test timing — P2 ends the read before the 300 ms total timeout — and a JSON path in the test (`bytes.data`).
   - Both fixed in the test only; production code unchanged. The failure output is kept in `qa/transport/test-transport-first-run-failure.txt`: a verbatim transcript, because the raw log was overwritten by the passing rerun.
   - Stable over 3 consecutive runs, then the full set below.

## 5. SOURCE_FREEZE
**Changed (Opus-owned only):**
- `crates/loom-uds-runner/{Cargo.toml, src/lib.rs, src/doip.rs, src/transport.rs (doc only)}`, plus the new `src/link.rs`.
- `crates/loom-uds/{Cargo.toml (runner dep `features = ["doip"]`), src/manager.rs}`, plus the new `tests/transport.rs`.

**Not touched:** root `Cargo.toml`, `Cargo.lock` (md5 identical before and after), other crates, UI and mock sources.
- Downstream users (`loom-gateway`, `loom-cli`) only use `UdsModule`, `sim_support` and `cli`, which are unchanged (checked by grep, not rebuilt).

**Diff identifier:** `git diff --binary -- crates/` (tracked files) sha256 `4dc1908344e6e7a08fd60ace8d69ca28f219f261a52d8649b56fff48f1549e51`, base 4fb848a. Untracked new files are covered by the source hashes. `git diff --check` is clean.

**Source hashes:** `qa/transport/source-sha256-freeze-transport.txt` (8 files). Verify from the repo root with `shasum -a 256 -c work/reports/uds-read-did/qa/transport/source-sha256-freeze-transport.txt`.

**Mock hashes:** all 6 identical to §0 (re-checked after the last edit).

**Commands** (repo root; `CARGO_TARGET_DIR=/tmp/loom-uds-transport-check CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`, `--offline -j2`). Logs are in `qa/transport/`.

| Check | Result | Log |
|---|---|---|
| `rustfmt --edition 2024 --check` (changed files) | exit 0 | `fmt.log` |
| `cargo clippy -p loom-uds-runner -p loom-uds --all-targets -- -D warnings` | exit 0 | `clippy-default.log` |
| `cargo clippy -p loom-uds-runner --no-default-features --all-targets -- -D warnings` | exit 0 | `clippy-runner-nodefault.log` |
| `cargo tree -p loom-uds-runner --no-default-features -e normal` | `loom-uds-doip` absent (present with default features) | `tree-runner-nodefault.txt` |
| `cargo doc -p loom-uds-runner --no-deps` | exit 0, 0 warnings | `doc-runner.log` |
| `cargo test -p loom-uds-runner --no-default-features` | 50 passed (runner 30, priority_abort 10, gate_fixes 10) | `test-runner-nodefault.log` |
| `cargo test -p loom-uds-runner` | 54 passed (adds `doip_e2e` 4: real loopback DoIP, including reconnect) | `test-runner-default.log` |
| `cargo test -p loom-uds` | 31 passed (`e2e` 7 over real SIM DoIP, including link loss → reconnect; `transport` 4; cli 7; uds 7; …) | `test-uds.log`, `test-transport.log` |

**New tests in `tests/transport.rs`.** All four use an external `FakeLinks: Connector` over `FakeTransport`; no DoIP is involved.
- `uds_logic_runs_unchanged_over_an_injected_connector`:
  - exact bytes for DID `22 F1 90`, decoded as F190;
  - raw `19 02 FF`, with the DTC parsed;
  - `0x78` → pending_count 1 → positive;
  - read timeout → `failed` + timeout, with SID 0x22 quarantined;
  - the next 0x22 is refused (`quarantine`) and never sent;
  - the late `62 F1 92 AA` is attributed to the timed-out request and releases U;
  - the link's sent log contains exactly the five UDS PDUs.
- `reconnect_through_connector_keeps_generation_and_never_resends`:
  - a write in flight when the link is cut ends `outcome_unknown`;
  - the reconnect attempts run 2 refused + 1 wrong generation (dropped) + 1 accepted, all requesting generation 2;
  - the session generation goes 1 → 2;
  - exactly 1 live link resource remains;
  - quarantine is cleared;
  - the new link's sent log is empty, so nothing is resent;
  - the next read runs on generation 2.
- `link_events_eof_is_link_loss`: when the signal source ends without `Down`:
  - with `auto_reconnect=false`, the link goes down with cause `link signal source ended` and generation stays 1;
  - with `auto_reconnect=true`, it reconnects to generation 2 with 1 live resource.
- `failed_first_connect_leaves_nothing`:
  - a refused connect and a wrong-generation first connect both give `E_NOT_OPEN`, register no target and leave 0 live resources;
  - a later connect works, and a repeat connect returns `existing` without a new attempt.

**Build cache:** `/tmp/loom-uds-transport-check` was created by this task (it did not exist before), measured 314M, and was removed after the pass (`rm -rf /tmp/loom-uds-transport-check`; absent afterwards). The root `target/` stays at 92M and the cargo home is not touched.

## 6. Future adapter recipe (CAN-TP / vendor Ethernet diagnostics)
1. **Implement `loom_uds_runner::Transport`** for one target.
   - It carries UDS PDUs only. Framing (ISO-TP PCI, flow control, vendor Ethernet headers) stays inside the adapter.
   - `confirm` resolves when the whole request has been handed over (ISO-TP: last CF sent; not the FF or the enqueue).
   - Segmented responses emit `Incoming::RxStart` at the first frame, then `Incoming::Pdu` with the reassembled PDU.
   - Map failures to `TransportError`/`RecvError`, and keep `nothing_sent` honest.
2. **Implement `Connector`** (owning addressing, channel config and client state) and `LinkEvents`. An `mpsc::Receiver<LinkSignal>` already works as `LinkEvents`.
   - Return a link only at the requested generation, and only with a verified response-epoch fence covering all outstanding transactions. Otherwise return `ConnectError`.
   - Release everything on error. `Link::resources` must close the link when dropped.
3. **Attach with `UdsManager::connect_link(ctx, LinkSpec { name, backend, env, auto_reconnect, connector })`.** No runner, request, lease, lane or STOP code changes.
   - An RPC/CLI selector for a new backend is a separate task: no backend enum is advertised now.
4. **Tests:**
   - reuse the `tests/transport.rs` pattern, with the adapter in place of `FakeLinks`;
   - add the adapter's own framing tests in its crate;
   - add a fence/drain test proving an old-generation response can't be accepted.
