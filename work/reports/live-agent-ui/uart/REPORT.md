# UART live agent/UI check — REPORT

Owner: uart-live-check-opus (w1N:p1). Date 2026-11-14. Initial baseline used the shared isolated daemon; later fixed runtimes and ROOT cleanup are recorded below.
Backend: `LOOM_HOME=/tmp/loom-uds-live-20261114/home`, port 52575, loomd pid 95239, stream_epoch `1794612979`,
CLI `target/debug/loom` (realpath `/Users/ctmctm/workspace/Loom/target/debug/loom`, 9776424 B, mtime Nov 14 08:41,
SHA-256 `d54e4134f10b84fe3ce4c1e5e0e01cdf677cd317677b1c22c92143a6430a3689`, matching ROOT. The earlier `fd60a9eddd79…` was
the SHA-1 from `shasum`'s default algorithm; it is the same file, so the earlier value was a reporting error),
daemon `target/debug/loomd` (pid 95239 started 08:36:19; uart worker pid 5605 `loomd worker uart --instance uart.qa_uart_live`;
on-disk loomd mtime Nov 12 21:48, SHA-256 `9f6ab4b8059536cd352cb9c656455d75281b563636534cba7a6bfa28539d218d`),
`LOOM_NO_AUTOSTART=1`. No code/skill/CLI default files changed.

## Status
| Gate | Result |
|---|---|
| AGENT_PASS | **PASS**: an independent observer received live native sim device traffic in real time |
| UI_WAIT | **WAIT**: viewers subscribed=0 visible=0 (`no_viewer_attached`). ROOT browser auth is pending. No UI claim made |
| UI_FAIL | none |
| DONE | **no**: channel left open for the ROOT BEGIN_UI phase |
| PRODUCT BUG | **found**: secret bytes are not redacted in the `uart.data` tx event (see below) |

## Setup (CLI only)
1. `loom uart help --json` → `raw/00-uart-help.json` (`loom help --json` → `raw/00-loom-help.json`).
2. `LOOM_CLIENT=uart-live-sender loom uart open 'sim://bootshell?seed=20261114' --as qa_uart_live --env sim --json`
   → request_id w17, generation 1 (`raw/02-open.json`).
3. Login: `expect 'login: '` → `send root --expect 'Password: '` → `send --secret-env QA_UART_PW --expect-prompt`
   (w20) → `root@sim:~# ` (`raw/03..06`).

## Agent phase (bounded, one shot)
- Observer, run first as a bounded child (20 s), client `uart-live-observer`:
  `loom events --follow --since now --channel qa_uart_live --timeout 20 --max 1000 --json`
  Each NDJSON line is saved raw (`raw/11-observer.raw.ndjson`) and with its receive wall time (`raw/11-observer.stamped.jsonl`).
- Producer, client `uart-live-sender`:
  `loom uart send qa_uart_live "echo QA-UART-LIVE-1794613693-27356" --expect QA-UART-LIVE-1794613693-27356 --timeout 10 --json`
  → ok, request_id **w21**, written_bytes 35 (`raw/12-send-marker.json`).

Correlation (epoch 1794612979):
| seq | kind | origin/dir | request_id | event wall_ns |
|---|---|---|---|---|
| 331 | uart.write | agent tx | w21 | 1794613696136634000 (text `echo <MARK>\r`) |
| 334 | uart.data | agent tx | w21 | 1794613696136916000 (bytes `echo <MARK>\r`) |
| 338 | uart.data | **device rx** | — | 1794613696136936000 (`echo <MARK>\r\n<MARK>\r\n…`) |
| 340 | uart.expect_result | agent | w21 | 1794613696149999000 matched |

- The sender CLI ran from 1794613696124383000 to 1794613696260326000.
- The observer received the batch with seq 329–340 at **1794613696249187000**. That is about 112 ms after the device rx event, and before the sender CLI returned. This is live bounded follow (200 ms polling), not a historical-only replay.
- Note: the `--expect` literal matched the echoed command line (rx 13136–13165), not the output line. The output line is present in the same rx chunk (seq 338).

## READY_FOR_UI
- Channel: `qa_uart_live` (uart, `sim://bootshell?seed=20261114`, generation 1, logged in at `root@sim:~# `)
- Env for both commands: `LOOM_HOME=/tmp/loom-uds-live-20261114/home LOOM_NO_AUTOSTART=1`
- Observer: `LOOM_CLIENT=uart-live-observer target/debug/loom events --follow --since now --channel qa_uart_live --timeout 30 --max 1000 --json`
- Producer: `LOOM_CLIENT=uart-live-sender target/debug/loom uart send qa_uart_live "echo QA-UART-UI-<nonce>" --expect "QA-UART-UI-<nonce>" --timeout 10 --json`
- UI comparison keys: epoch/seq of the `uart.data` device rx event, request_id of the producer write, and exact marker bytes in the UART panel.
- I will wait for ROOT BEGIN_UI before running a new cross-consumer phase. There is no auto renew and no polling.

## PRODUCT BUG — secret not redacted in uart.data tx (repro, not fixed)
- `loom uart send qa_uart_live --secret-env QA_UART_PW --expect-prompt` with `QA_UART_PW=sim` (request w20).
- `uart.write` seq 250 shows `text: "[secret 4 B]"` (redacted), but `uart.data` tx seq 253 carries `bytes.data: "c2ltDQ=="` (= `sim\r`, plaintext).
- Evidence: `raw/20-secret-w20-events.json`. Any `loom events` reader (and probably the recording) can see the secret.
  This conflicts with the skill rule "passwords: `--secret-env VAR` only". The secret here is a dummy sim value.
- I made no shared edits. ROOT should route this to the uart worker/gateway owner.

### Current-source check (narrow, read-only)
`crates/loom-uart-worker` and `crates/loom-console` have no working-tree diff. The last commit touching
`worker/{port,util,send}.rs` is 1a2e817 (2026-11-11), which is before the loomd build (11-12 21:48).
**The leak path is present in current HEAD source.** I did no runtime repro on a fresh build (no builds were allowed).
1. `worker/send.rs:237,272`: `op.secret` is parsed, but it is used only for `uart.write` previews/labels (`:337-345`, `:368`, `:566`).
2. `tx.rs:174` `TxChunk`: has no `secret` field, so the flag is lost when the request reaches the TX actor.
3. `worker/port.rs:445-446` `tx_loop` → `channel.rs:388-412` `ChannelCore::on_tx`: `data_payload(&t.bytes, None)` emits the raw bytes
   in `uart.data` tx. It also pushes the plaintext bytes into the console ring (`Entry::Data{bytes}`), so `uart read/lines` and
   UI tx-echo rendering can probably expose them too (not verified).
4. The consumer contract already exists. `crates/loom-uart/src/evidence.rs:159` treats `payload.secret` or quality `redacted` as
   undecodable, and the `uart.write` schema has `secret{len,sha256}` (`kinds.rs:123`). The producer never sets this on `uart.data`.
   The journal/recording is written from the same emitted event, so it most likely holds the plaintext as well.

### Fix PLAN (for ROOT decision; not started)
- Files: `crates/loom-uart-worker/src/tx.rs` (add `secret: bool` to `TxChunk` and to the submit request that creates it),
  `worker/send.rs` (pass `op.secret` through), `channel.rs` `on_tx` (if secret: emit `{len, secret:{len}}` with no `bytes`, add quality
  `redacted`, and store a placeholder or length only in the ring entry), `worker/port.rs` (pass-through only),
  and one worker unit test (a secret send must leave no bytes in `uart.data` or the ring).
- Possible schema touch: `crates/loom-uart/src/kinds.rs` `uart.data` (allow an optional `secret`).
- Out of my scope; ROOT must confirm: UI rendering of tx data without `bytes`, and the gateway/recording (they need no change if they
  just pass the event through). No other modules.
- Verification afterwards: rebuild once (ROOT decides), rerun the w20-style secret send, and grep the events and journal for the base64/hex of the secret.

## Cleanup (pending UI acceptance)
After acceptance: `loom uart close qa_uart_live`. Do not stop the daemon or other channels.

## FIX PLAN (approved 2026-11-14; implementing)
Goal: the real secret bytes still go to the device. Shared `uart.data` tx, the console ring, read/lines, UI, log and journal carry only `secret{len}` and a mask, never the original bytes.
1. `tx.rs`: add `secret: bool` to the TX request and to `TxChunk`, carried through to each chunk.
2. `worker/send.rs`: set the request's `secret` from `op.secret`.
3. `channel.rs on_tx`: if `secret`, the payload is `{len, secret:{len}}` with no `bytes`, and the ring gets a fixed mask placeholder (`*` × len) instead of the bytes.
4. `worker/port.rs`: mark the tx data draft with quality `redacted` when secret.
5. `loom-uart/src/kinds.rs`: `uart.data` schema allows `secret`. `evidence.rs` already skips redacted data; confirm it.
6. Tests: worker unit/regression test (secret send → device got the bytes; event/ring hold no bytes); evidence handling of redacted data without bytes.
7. Verify: `cargo test -p loom-uart-worker -p loom-uart` and clippy for those crates in `/tmp/loom-uart-secret-build` (CARGO_INCREMENTAL=0, debuginfo 0). Then build the worker binary only and return its path/hash. ROOT coordinates the restart.

## FIX DONE: source + unit verified; native verification pending (ROOT)
Scope: 11 files from the approved list, plus `loom-uart/tests/registry.rs` (schema regression):
`loom-uart-worker/src/{tx.rs,channel.rs,worker/send.rs,worker/port.rs}`, `loom-uart/src/kinds.rs`; tests `loom-uart-worker/tests/{channel_core,tx,worker_e2e,channel_perf,common/mod,tx_cancel}.rs` (the last 3 only gain a `secret: false` literal), `loom-uart/tests/registry.rs`.
- Device path: `TxRequest.secret` → `TxChunk.secret`. The transport still writes the real bytes (test `secret_request_writes_real_bytes_and_flags_every_chunk`).
- `uart.data` tx for secret: payload `{len, secret:{len}}` with no `bytes`, quality `["redacted"]` (port.rs). The ring stores `*`×len (`SECRET_MASK`), the same length as the TX it replaces. Ring TX entries carry no rx_offset, so RX offsets are unchanged.
- Schema `uart.data`: `required:[len]` + `anyOf:[{required:[bytes]},{required:[secret]}]`, and `secret.required:[len]`. A non-secret payload without bytes stays invalid (test `uart_data_schema_requires_bytes_unless_secret`). The schema is descriptive (registry/help); no runtime validator was found on the event path.
- Consumers (read-only check): UI `panels/uart/terminalPanel.ts:367` gets null from `decodePayloadBytes` and returns for that event only, so the stream continues. The UI never reads `quality` at runtime. `loom-uart/src/evidence.rs:311` skips TX before any bytes access (`tx_skipped`). The gateway passes payloads through. The `ui/src/codec/columns.ts` columnar path has no producer.
- Tests (`/tmp/loom-uart-secret-build`, incremental 0, debuginfo 0): `cargo test -p loom-uart-worker -p loom-uart` all ok (`raw/30-unit-test-summary.txt`); targeted new tests ok (`raw/31`, `raw/33`); e2e `core_loop_open_expect_send_run_read` now also asserts that `l-pass` `uart.data` has no bytes, has `secret.len` and quality `redacted`, and that no published record contains the base64 or hex of the password. Clippy `-p loom-uart-worker -p loom-uart --all-targets -D warnings` clean (`raw/32`, `raw/34`).

### New binary (HANDOFF)
- `/tmp/loom-uart-secret-build/debug/loomd`, 62415648 B, 2026-11-14 09:00,
  SHA-256 `06789da6fc4c890c9bec18e96f47993cbc593907a151b0833eb60e8a774dd4fa`.
  Command: `CARGO_TARGET_DIR=/tmp/loom-uart-secret-build CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build -p loom-gateway --bin loomd` (default features).
- Source: HEAD `47db65d0c2d3` + uncommitted uart diff (`git diff -- crates/loom-uart-worker crates/loom-uart | shasum -a 256` = `d53fe902f18b…dbd990`).
  Other dirty files at build time: `crates/loom-cli/src/{lib,skills}.rs` (not linked into loomd) and `ui/src/shell/dock/activeWorkspace.ts` (ROOT ETH work; loomd serves `ui/dist` via rust-embed, not ui/src).
- **HEAD vs native difference**: the running isolated daemon (pid 95239, `target/debug/loomd` SHA-256 `9f6ab4b8…218d`, built 11-12) does NOT have this fix. HEAD alone does not have it either; it is uncommitted. ROOT owns Git.
- I did not stop or restart anything. `qa_uart_live` (worker pid 5605, old binary) is still open.

### Native re-verification for ROOT after the swap (dummy secret only)
1. After the old isolated daemon has stopped normally, start the new loomd on the isolated LOOM_HOME. Then: `loom uart open 'sim://bootshell?seed=20261114' --as qa_uart_live --env sim`.
2. `expect 'login: '` → `send root --expect 'Password: '` → `QA_UART_PW=DummySecret1 loom uart send qa_uart_live --secret-env QA_UART_PW --expect-prompt` (the login must reach `root@sim:~# `).
3. In `loom events --channel qa_uart_live --since -60s --json`, the `uart.data` tx for that request_id has no `bytes`, has `secret.len=13` and `quality:["redacted"]`. grep for `DummySecret1`, `RHVtbXlTZWNyZXQx` and `44756d6d79536563726574` in events, `uart read`/`lines` (TX row `*************`), the journal (`records/<date>/<session>/`) and the UI DOM: 0 hits.
4. In the UI, the channel keeps streaming after the redacted TX (send `echo QA-UART-UI-<nonce>` and check that it appears).

## NATIVE RE-VERIFICATION (new binary, new home): AGENT_PASS for the worker path; NEW FINDING in the gateway audit
- Binary: `/tmp/loom-uart-secret-build/debug/loomd` SHA-256 `06789da6…dd4fa` (pid 17384, port 53814, `--rights watch --ui-dir ui/dist`), epoch 1794614502.
  CLI `target/debug/loom` d54e4134…. Old baseline: pid 95239 `target/debug/loomd` 9f6ab4b8… leaked at w20 seq 253.
- Channel `qa_uart_fixed` opened (w1, gen 1). Login: root (w2) → `--secret-env QA_UART_PW` = DummySecret1 (**w3**, written_bytes 13, `sent_escaped "[secret 13 B]"`)
  → `root@sim:~# `, so the real bytes reached the device. Viewer attached (1/1) throughout.
- w3 events (`native/09-w3-events.json`): `uart.write` seq 40 `{len:13, secret:{len:13}, text:"[secret 13 B]"}`; **`uart.data` tx seq 43 `{len:13, secret:{len:13}}`, quality `["redacted"]`, no bytes**.
- Leak scan (`native/10-leak-scan.txt`; needles: plain, plain+CR, base64 at all alignments, hex, spaced hex):
  events, read, send result, worker journal `uart.qa_uart_fixed/1/000001.lj.open`, gateway/_mapping journals and manifest: **0 hits**.
- **FINDING 2 (gateway, shared, not my scope; not fixed):** `audit/audit-2026-11.jsonl` seq 5 `write.intent` (request w3) stores the full request
  `args` including `"data": "<hex of DummySecret1\r>"` next to `"secret": true`. Source: `crates/loom-gateway/src/daemon.rs:2043`
  `json!({... "args": args})`. No other gateway site records `args`.
  PLAN for ROOT (gateway owner): before `audit.append("write.intent", …)`, if `args.secret == true`, replace `args.data` with `{"secret":{"len":<decoded byte len>}}`
  (or a generic domain-declared redacted-arg list). Add a gateway unit test (secret send → the audit line has no data hex).
  The hash chain is computed over the redacted record, so `verify_file` is unaffected.
- `uart read` is RX text only, so the TX mask is not visible there. The ring/lines mask is covered by the unit test (the CLI has no `lines` verb).
- READY file: `native/READY_FOR_UI.md`. The cross phase waits for ROOT BEGIN.

## FIX 2: gateway audit secret redaction (ownership extended: `crates/loom-gateway/src/daemon.rs` only)
- Finding (native, binary 06789d…): `audit/audit-2026-11.jsonl` seq 5 `write.intent` w3 stored `args.data` = hex of DummySecret1\r next to `secret:true`.
- Fix: `daemon.rs` adds the helper `audit_args(&Value) -> Value`. When `args.secret == true` it replaces only `data` with `{"secret":{"len":N}}`
  (N = hex digits / 2). The intent uses `"args": audit_args(&args)`. The original `args` is still what goes to the worker/device
  (`json!` borrows it; the helper clones). Non-secret and `secret:false` args are recorded verbatim.
- Tests (`daemon::audit_secret_tests`): `secret_args_lose_only_their_data` (only data is redacted, other fields kept, original untouched,
  plain unchanged); `audit_file_has_no_secret_bytes_and_chain_verifies` (real `Audit` writer in a tempdir: the line has `len:13`, no hex,
  base64 or plaintext, and `verify_file` passes). `cargo test -p loom-gateway --lib audit`: 4/4 ok (`raw/40`).
  `cargo clippy -p loom-gateway --lib --tests -D warnings` clean (`raw/41`).
- Rebuilt in the same cache: `/tmp/loom-uart-secret-build/debug/loomd` 62422560 B 09:07,
  **SHA-256 `4ac3ee715f97462ec2b7c1726cb9689b4e312ac2d838a7669b18d1c793e418ed`** (`raw/42`).
  Source = HEAD 47db65d + uart diff sha256 d53fe902… + gateway daemon.rs diff sha256 5dc30fa3… (+73/−1).
- **Caution for ROOT:** the rebuild replaced the file at the path the running fixture uses. pid 17384 and its workers
  (echo, can_sim, uart.qa_uart_fixed 18048, eth.qa_eth_fixed 19903) keep executing the old 06789d… image. Any worker that pid 17384
  spawns from now on execs the NEW 4ac3ee… file (`current_exe().with_file_name`). That would be a mixed fixture, so restart before the final UI.
- Native verification of FIX 2 is pending ROOT's restart of the fixture. Then rerun the w3-style secret login and run the leak scan over audit/events/journals.

## HOLD (2026-11-14, ROOT): audit approach accepted. Native phase waits for ROOT's fixture restart and the new epoch
- Build: `/tmp/loom-uart-secret-build/debug/loomd` SHA-256 `4ac3ee715f97462ec2b7c1726cb9689b4e312ac2d838a7669b18d1c793e418ed`.
- Source: HEAD `47db65d0c2d3c9766cdd795a2cb9fe7468d5b303` + uncommitted diffs: uart `d53fe902f18b…dbd990`, gateway daemon.rs `5dc30fa318d6…f72d`.
  Per-file SHA-256: daemon.rs `911d4349…4767`, tx.rs `d0e8e4b3…3252`, channel.rs `2652873d…959b`, send.rs `8d17e390…9b2c`,
  port.rs `971fe2c5…80db`, kinds.rs `9c4d582d…f1e1`.
- Test/clippy/build logs: `raw/30-unit-test-summary.txt`, `raw/30-unit-test.log`, `raw/31-secret-tests.txt`, `raw/32-clippy.txt`, `raw/33-schema-e2e-tests.txt`,
  `raw/34-clippy-after-schema.txt`, `raw/35-loomd-build.txt`, `raw/40-gateway-audit-tests.txt`, `raw/41-gateway-clippy.txt`, `raw/42-loomd-rebuild.txt`.
- I will not restart anything myself. After ROOT gives the new epoch: secret login (dummy) → leak scan of audit, events and journals → fresh 30 s observer READY → marker on ROOT BEGIN.
- CLI for the final native phase: `target/debug/loom` SHA-256 `22077e2af3e6bd448efc87561bffe299f384d79905f66b069f9a127a90ab824a`
  (9682480 B, 09:08, verified locally). Replaces d54e4134… used in earlier phases. Still waiting for ETH UI acceptance and the gateway restart.

## FINAL FIXTURE (ROOT, /tmp/loom-uart-audit-fixed/home :54139, loomd 4ac3ee…, epoch 1794614926): AGENT_PASS, secret leak fixed natively
- `qa_uart_final` w1 open, w2 root, **w3 secret** (13 B, login reached the shell). `uart.data` tx seq 42 has no bytes, `secret.len 13`, quality redacted.
  Audit write.intent seq 5: `args.data={"secret":{"len":12}}`. Leak scan TOTAL_HITS 0 over events, read, audit, journals and manifest (`final/10-leak-scan.txt`).
- READY_UI: `final/READY_UI.md`. Observer prepared (starts on ROOT GO, 30 s). ROOT sends the marker. UI_WAIT until then.

## FINAL UI PASS + CLEANUP: READY_TO_INTEGRATE
| Gate | Result |
|---|---|
| AGENT_PASS | PASS (old home w21; final fixture w3 secret + observer) |
| UI | **PASS** (ROOT UI_SCREENSHOT_PASS: `final/ui-marker-terminal.png`, `final/ui-marker-dom.txt`, `final/root-agent-marker-compare.json`) |
| Secret leak | **FIXED natively**: TOTAL_HITS 0 over events, read, audit, journals and manifest; ROOT DOM secret 0 hits |
| DONE | own channels closed; shared daemons untouched |

- Cross phase (epoch 1794614926): observer `uart-final-observer` was READY at 09:10:08.110 (first snapshot), expiring 09:10:37.955 (`final/observer-ready.json`).
  ROOT sent one marker, `QA-UART-DUAL-1794615008190535000` (request **w4**, client `uart-ui-root-final#24948`).
  The observer independently got TX `uart.data` **seq 62** (`echo <MARK>\r`) and device RX **seq 65** (`echo <MARK>\r\n<MARK>\r\nroot@sim:~# `),
  recv 1794615008394945000, about 193 ms after the RX event. This matches ROOT's TX62/RX65 (`final/23-observer-correlation.txt`, raw `final/21-observer.raw.ndjson`).
  The observer ended normally at 30 s. I sent no marker of my own.
- Cleanup, own channels only (`cleanup/close-*.json`): qa_uart_live (52575) w29, qa_uart_fixed (53814) w8, qa_uart_final (54139) w5, all
  `closed:true refs_left:0 threads_left:0`; status afterwards `open:false`. The worker processes (5605, 18048, 24032) remain, serving the retained
  history ring for closed channels. Observation only, no action taken. No daemon was stopped (95239, 17384, 23504 all running).
  ROOT reports that closing the last active UART exposed an ETH shell bug (a retained closed ETH panel is hidden under the global empty welcome); the ETH owner is fixing it.
- An earlier cleanup attempt with `rm -f` on a variable path was rejected; nothing was deleted. The retry used literal commands with no rm.
- Final build: `cargo build -p loom-gateway --bin loomd` (same cache, default features) **ran before ROOT's HOLD arrived**. No source changes.
  `/tmp/loom-uart-secret-build/debug/loomd` 09:12, **SHA-256 `75ca48753a20fbe4e3778e750818880d0bed236a63d14b3d212401a0b40c518d`**, embeds `index-zZUL1YL_.js`
  (`raw/43-loomd-final-build.txt`). It is not final with respect to the pending ETH bundle: per HOLD, no further rebuild until the ETH final bundle signal.
  The binary file at this path was replaced again. Running pids 17384 and 23504 keep the 06789d… and 4ac3ee… images; workers they spawn from now on exec 75ca48….
- Code fixes (source = HEAD 47db65d + uncommitted): uart worker/kinds diff `d53fe902…`, gateway daemon.rs diff `5dc30fa3…`. ROOT owns Git and promotion to target/debug/loomd.
