# UART operator upgrade — STATUS

**READY_TO_INTEGRATE — all batches B1–B8 delivered and lead-reviewed** · 2026-11-16 · lead Opus 5.5 · worker Sonnet (bodies only)
Worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-uart` · branch `operator/uart` · baseline `610029e` · no Git action
by the lead or the worker (ROOT owns index/commit/push; the user authorized the push after integration).

## Coverage (RETURN-uart U1–U8)
| Finding | Batch | Delivered | Review |
|---|---|---|---|
| U1-A continuous native log (opt-in, bounded, never deletes, no stream seq) | B4, CLI B7, UI B6 | `logsink.rs`, `uart.log.start/stop`, `uart.open.log`, REC dialog controls | B4/B7/B6-REVIEW.md |
| U1-B past-range journal export (peek, truthful header/gaps/caps) | B5 | `journalExport.ts`, REC dialog section | B5-REVIEW.md |
| U2 opt-in bounded reconnect (same identity, new generation, STOP/close end it, no resend) | B3, CLI B7, UI B6 | `worker/reconnect.rs`, `uart.open.reconnect` | B3-REVIEW.md |
| U3 transmit pacing (no burst; STOP/HOLD/close/Ctrl-C leave no delayed write) | B2, CLI B7, UI B6 | TX actor pacing, `uart.pacing`, per-send overrides | B2-REVIEW.md |
| U4 last-view-close toast | B5 | `[포트 닫기]` / `[다시 보기]` | B5-REVIEW.md |
| U5 search (B1) + highlight rules / scrollback (B6) | B1, B6 | `scrollSearch.ts`, `highlight.ts` | B1/B6-REVIEW.md |
| U6 `uart.signal` (break/DTR/RTS; release always) + UI pulses with confirm card | B3, B6 | `worker/signal.rs`, M1 | B3/B6-REVIEW.md |
| U7 narrow-pane identity | B1 | `paneLayout.ts` | B1-REVIEW.md |
| U8 open-toast wording / [패널 열기] (ROOT R4) | B8 | shellControl/channels/services narrow edits | B8-REVIEW.md |

## Last lead verification (per batch, targeted; see each review for exact output)
- Rust: `cargo test -p loom-uart-worker` all 18 suites ok; `cargo test -p loom-uart` all ok (cli_b7 8/8); clippy 0 warnings for
  both crates (CARGO_BUILD_JOBS=2, no full-workspace build).
- UI (from ui/, `--maxWorkers=2`): `src/panels/uart src/workbench/uart` + B8 shell tests: 19 files / 196 tests pass; isolated tsc
  app/node exit 0; eslint exit 0. The full UI suite last ran once at B8 (74 / 817 pass); from then on only ROOT runs it, at the final freeze.

## Process deviations on record (ROOT correction 2026-11-16 — static lines are lead-only)
- E1 (B5): the `decodePayloadBytes` import in `journalExport.ts` was worker-written; also the unused scaffold `todo` there was deleted by the worker.
- E2 (B4): the `Port::producer()` accessor signature in `worker/port.rs` was worker-written.
- E3 (B6): the `workbench.ts` line-16 import was worker-written before the revocation. E4 was revoked unused.
All are kept and their static deltas reviewed in B4/B5/B6-REVIEW.md. From now on: return the lease → lead edit → re-freeze → RESUME (done for `PORT_KINDS`).

## For ROOT at integration
- R1: regenerate the schema/catalog for the new verbs (`uart.pacing`, `uart.log.start`, `uart.log.stop`, `uart.signal` now M1), the new `uart.open` params (`pacing`, `reconnect`, `log`), the send/run params, the event kind `uart.log`, and the `uart.state` / `uart.signal` / `uart.input_ack` / `uart.config` payload fields.
- `work/typecheck/` is untracked and not ignored: exclude it. `ui/node_modules` is untracked: not ours.
- Baseline rustfmt differences in `worker/port.rs` and `tests/registry.rs` are left for ROOT's final pass.
- Load-sensitive tests seen under load ≈30–80: `defaultLayout.test.tsx` (5 s), `registry::hooks_classify_fast` (1 ms), and pty spawn (`Result too large`).
  All passed on rerun; no timeout or assertion was changed.
- R6: native/WKWebView download behaviour (REC and journal export) is ROOT's check. GUI checks are ROOT's.
- Phase-2 PTY simulator validation is planned in SIM-VALIDATION-PLAN.md and NOT run; it is blocked on ROOT's metadata (an isolated daemon/LOOM_HOME, the sim device path, unplug/replug).
- Known limits: reconnect is tested with fakes only (no real USB identity; no path change on replug); late-join snapshot channels are
  not "seen open" for "다시" wording; highlights match per buffer row.

## Final source files (51, sha256 at handoff)
```
9990222000022bb3d242daea6e0fa62d24abaae773eb75f96d681614f50582a1  crates/loom-uart-worker/src/lib.rs
8d19f735ed2aadccb6f548889d25336ea4bb0c475fe5fb2455572c4be359628c  crates/loom-uart-worker/src/logsink.rs
bc3d6b2822f69989168553af9dd9ab8d02eafa92d57bd5a3c52ec67fcaff6575  crates/loom-uart-worker/src/tx.rs
7dc25e51af01ca7790acb6477a90358e012973232914d1ca40f99b87f0c54e42  crates/loom-uart-worker/src/worker.rs
80426121e0dcbd9c880c41fbfdf9be0ad596f411b0a288d82e2a5f4104f76ed6  crates/loom-uart-worker/src/worker/port.rs
5f4326eecf07f29d8ce4fce62367efa3de47a295c69780669d47032d43db8f72  crates/loom-uart-worker/src/worker/reconnect.rs
4405e274d192e7590db3d1d35a149947d0df18901130a18683187d3498fc489f  crates/loom-uart-worker/src/worker/send.rs
170c433de4f973bb3c0b43f06432e39ce5d6bea972074eaa6cd832615bcba9d0  crates/loom-uart-worker/src/worker/signal.rs
b4638f982b4d7df9091f6c7b34d3982e24c064f2fd3b6d1939de8687d2198d15  crates/loom-uart-worker/src/worker/util.rs
5c008e747f981fbce6213c19da49132172c30194ec98a6dd184716c5d16d1741  crates/loom-uart-worker/tests/common/mod.rs
0cd99b3bca8380c05f3b92d9987a5b49c8f906021bb64110d9a7e69434b8b47b  crates/loom-uart-worker/tests/logsink.rs
55ed3ea9b6d62ff2fab6b3eda23632330c6581ecd672ebbae3185ee4af21cc5c  crates/loom-uart-worker/tests/tx_cancel.rs
e02d15a8a5797f288bb1d354bd596807d5f61b3ed8582c6cb22979c0ed838cf3  crates/loom-uart-worker/tests/tx_pacing.rs
07d211a898405462f2aab596e8799e7d430cc65466d558bc254860136f305f04  crates/loom-uart-worker/tests/tx.rs
c32e952985e631ce43dfde184c07a58ca1fe1d253cec992fcce9566b83670f4f  crates/loom-uart-worker/tests/worker_log.rs
1ed11eba818f40c0a89e362518be5a40b41ab181c37fff3a82b4584f5146516a  crates/loom-uart-worker/tests/worker_pacing.rs
f6cb7378bfe710dc02350d01f293d0222bb480677b349ea61e69ac9716959764  crates/loom-uart-worker/tests/worker_reconnect.rs
4032351ef7b85944f00ee0a1a28a9ecd86b9de58518e80b878a442bba1576874  crates/loom-uart-worker/tests/worker_signal.rs
87da94bcf30490a6b8e0d8cbbc7a9a682ea96484f7eb94cc8134bc9b80dcb7dd  crates/loom-uart/src/cli.rs
2dd7f6fa6d8cfbde18697dd5a5795730e220441120c82c52de831fd270499ad3  crates/loom-uart/src/commands.rs
d9ca5cf5e524d350d614999dd109ea1e7792c239b0a3caa660b819f9d90c104e  crates/loom-uart/src/kinds.rs
d2aa6a3ecdf674f1daff70e1d5d44e11c7cb03ccec3c6d2550c5067a4a7103ec  crates/loom-uart/tests/cli_b7.rs
829a2546651a9ffd292c2e6ceccfed75a350a30e56aed8e8cee86c24dc9a29f9  crates/loom-uart/tests/registry.rs
68c5847f474c30d8048684817b8432f48904d28e164afe3ea0feecc501bc445b  skills/loom-uart/SKILL.md
9c26c955c340c5fe65c9558e3bab54914d8e21cd59907658c014fcf24d2b9399  ui/src/panels/uart/batch1.panel.test.ts
e671907d7c00743a67d97b8f130edda7f7948877acf526bfb72df4645637f7b8  ui/src/panels/uart/batch5.panel.test.ts
ff64abd9bf2708c0f6a1ae0a26d710c94533273ad860446039633fc838c64524  ui/src/panels/uart/batch6.panel.test.ts
1759ed786802ca30bc54e7d30d4f7ebc583c40e0113275d46af129bb53077310  ui/src/panels/uart/highlight.test.ts
b7c82388a7c6e68d834a2bfb9e2796b2be99ead08015c952693641d3861928aa  ui/src/panels/uart/highlight.ts
a4daa9954e758b80faaad48ebf80d0667eb9ee4509a8d8080b4b816ab710a667  ui/src/panels/uart/journalExport.test.ts
e0ae1df5498147712669508342633f45f36580149a2e562a32523d8aaa2ccd59  ui/src/panels/uart/journalExport.ts
9c9f391ff5edddf7695bfe601cbaded70a76976f7c3159ea7619fcad86efe36f  ui/src/panels/uart/paneLayout.test.ts
d11390b64725f8c7306092d6c6313fe87ef8008d50e056d83a5870c3132cb07a  ui/src/panels/uart/paneLayout.ts
3c4a8b68033548392521c19e3a1419443b0b31f4109407d5aea162aaaec5a1d5  ui/src/panels/uart/scrollSearch.test.ts
1637a616366ab12362bae29827fae7c629a6e28bd99ddd88aac9ac0a40defee8  ui/src/panels/uart/scrollSearch.ts
74361d3db612ca720ae9417356993d5c9d330cb77ce0db7b64e3c30968e7063f  ui/src/panels/uart/terminalPanel.ts
db8d46e935c39e02a42e7e8a239493f43ba806e66ee3bf2edbaa82f82efd23a5  ui/src/panels/uart/uart.css
2bc47e02212a1a0fcb283b09dfe1bf08cb9a4caa7b8f400383598e0736ecada7  ui/src/panels/uart/uartState.b6.test.ts
a3590091daf65191ebce9c18ef4d806cb3a3b128f6995c94a8b5328c893d34f2  ui/src/panels/uart/uartState.ts
e524b93de30ba9ef551b8fbb413e24257f5ac9a582d1fb93ed198c47ade36b21  ui/src/shell/m1b.test.tsx
915855017b06033007fc6098f841e4fcdf1613b792fdca35e36ec00971e8c8ac  ui/src/shell/services.ts
cd5973d40b2fc25258a942c16392b8ef439ef851b41cdc4da244f342d46f0024  ui/src/store/channels.ts
0455b9681d403202979bcba2315f599f6a1098884d79c35f3bf32195f2922f7a  ui/src/store/shellControl.b8.test.ts
e972793f559d794d4f02723f9aac2a5c33e501544836730d203d70cf62fbaeb6  ui/src/store/shellControl.ts
6f8d566ed787589aaaca9f35fe6db947af01a6dedfc58e109fe1c122ea41d607  ui/src/workbench/uart/bench.css
0ecf86c6c5d9380543d26be804874eb3ff448a99345839e1a0665a2fe4e9ee05  ui/src/workbench/uart/icons.ts
1448285de0109d46a2347ab6727782c57b4d376d82ea401e51a3d6c8dcabd86c  ui/src/workbench/uart/UartWorkbench.tsx
df8063662b88e5e0fd8c79068a9e6233c39f7f1c5ac881bfe6a6d498ecd4c4d6  ui/src/workbench/uart/workbench.b5.test.ts
23b81664f1299b2922d72a9169d9970a7070c4072e0548631af0650f868d7891  ui/src/workbench/uart/workbench.b6.test.ts
42d1bc3ffd5600f5c76ba576917758edb6d5c4e6ef7af9b393fa44d5d5fc136a  ui/src/workbench/uart/workbench.b8.test.ts
e82b23b255918b8cd35e2cd27dcd1bf8909fba4b5018702af9973e324e467490  ui/src/workbench/uart/workbench.ts
```
Reports: PLAN.md, FUNCTIONS.md, B1–B8 -WORKER.md / -REVIEW.md, SIM-VALIDATION-PLAN.md, this STATUS.md (canonical
`/Users/ctmctm/workspace/Loom/work/reports/operator-upgrade/uart/`, identical copies in the worktree).

## Freeze (2026-11-16)
ROOT imported all accepted UART scopes plus the shared-shell narrow changes. The source/test lease is now ROOT's and the worktree is frozen. Lead and worker make no further source edits, builds or test runs. The phase-2 PTY validation waits for ROOT's FINAL fresh daemon metadata (no IO until then). ROOT runs the final combined schema/tsc/lint/UI/Rust/fmt and owns the index and the push.

## Correction batch B9 (ROOT-accepted F1/F2, 2026-11-16) — leased to the worker
- Baseline = ROOT's imported UART files (`b9-baseline-hashes.txt`, 51 files). Versus my handoff (`final-hashes.txt`) only 5 files differ, all
  through ROOT's mechanical rustfmt (whitespace, plus import order in `worker.rs` / `port.rs`); the worktree was synced to ROOT's versions.
- F1: the pacing budget = min(RPC deadline, caller window = defer + timeout), checked before admission and again before submit; the TX
  deadline and the expect wait are capped by it; unpaced requests are unchanged. F2: ENOTTY / ENOTSUP on DTR/RTS/break → `Unsupported` → `E_UNSUPPORTED`.
- The lead wrote 3 static signatures (`send.rs` ×2, `serial.rs` ×1), `tests/b9_regression.rs` (8: 5 slow-transport + 3 real-PTY on macOS)
  and the SKILL note. Bodies: worker.
- w11 (PRE_FINAL7) is known `sent 200` and is **not retried**. The runtime stays PAUSED; there is no IO until ROOT's NEXT metadata. The STOP test remains LAST.
- 2026-11-16 B9 accepted (B9-REVIEW.md): worker 19 suites incl. b9_regression 8/8 (real PTY), loom-uart ok, clippy 0. Incremental files
  vs ROOT baseline: send.rs, serial.rs, SKILL.md, tests/b9_regression.rs (new). READY_TO_INTEGRATE (B9). Runtime still PAUSED.
