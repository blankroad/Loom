# ETH-EXPORT-FIX — STATUS: READY (2026-11-15)

Foreground Opus 5.5 worker, direct implementation (no subagents). I did not run a browser, fixture, production UI/daemon/app build, Git write or index change. I added no job RPC, native IPC, permission or capability.

## Actual contract (verified in source, not assumed)
- `eth.export` is **not** a write command. The registry `write_commands` are only `capture.start/stop` and `send`, so the result has no `request_id`, and `ctx.request_id()` can be absent.
- The handler (`handlers.rs Export`) returns `{path, packets, bytes, truncated, comments_dropped, gaps[]}`.
- The gateway `daemon.rs envelope()` wraps that as `{schema_version, ok, cmd, data, meta, others_since_last}`.
- `ActionGate.run` keeps the success shape and throws `DomainRpcError` on `ok:false`.
- No progress is published: export is synchronous and bounded, with no job/Activity feed.
- Failed write row from the daemon (`daemon.rs fail()`, and the final row when the worker gave none): `payload = {write_state:"failed", method, final:true, error:{code, message}, detail}`, with no `summary`/`to`.
  - RowStore replaces the row payload with this latest row.
  - That is why the old TX cell showed an empty summary with no reason.

## 1. Fresh default export file (Rust)
- `crates/loom-eth/src/gateway/export.rs`: new `create_default_file(dir, ch, tag)`.
  - It uses `OpenOptions::create_new` with bounded suffixes `-1…-63` (`DEFAULT_FILE_ATTEMPTS = 64`), so allocation is atomic even under concurrent exports and never overwrites.
  - `ch` and `tag` are reduced to `[A-Za-z0-9._-]` (max 64 chars, no leading dot), so a channel name cannot leave the dir.
- `handlers.rs Export`:
  - Default path is std temp dir, `loom-eth-<ch>-<request_id or wall-ns>[-n].pcapng`. The daemon's actual path is still the truth: it is returned in `path`.
  - Explicit `path` keeps its documented create/truncate semantics (CLI `-o FILE`).
  - A failed **default** export (scan error, writer error, flush error) removes its own half-written file. An explicit user path is never removed.
- No new dependencies. The destination (temp dir) is unchanged: not Downloads, not a home guess, and no filesystem access for the UI.

## 2. Export completion in the UI
- SDK (`ui/src/sdk/context.ts`, additive, ROOT-authorized): `WriteHandle.result?: unknown` is readonly and optional.
  - Its value is `unwrapDomainResult(spec.rpc, gateResult)`: a success envelope gives its `data`, and a bare result passes through unchanged.
  - `requestId`, `state()` and `onChange` are unchanged. Fake handles stay source compatible (tsc -b clean). Failures still throw before any handle exists.
- `ethPanel.ts`:
  - While the RPC runs, the export button is disabled with `aria-busy` (one export at a time).
  - On completion, a compact dialog appears using the existing `.eth-dialog`/`.eth-kv` classes: daemon path, packets, bytes, plus 잘림, 손실 구간 N건 and 주석 생략 N when present.
  - The dialog has a "경로 복사" button (clipboard, with a fallback toast) and says "파일은 Loom daemon이 실행 중인 머신(벤치)에 저장됩니다 — 브라우저 다운로드가 아닙니다".
  - A toast also shows the path. "진행률은 Activity" is removed from the toast and the tooltip.
  - Errors still go through the existing `act()` toast (`export 실패: eth.export: <daemon message>`), with no dialog.
  - Only one result dialog exists at a time, and it closes on panel dispose.
- `eth.css`: one line `.eth-export-path`, so long paths wrap.

## 3. Failed TX reason
- TX "요약" cell: when the state is `failed`/`outcome_unknown` and `payload.error` exists, it shows `"<summary> — E_LOCKED: <message>"`.
  - The cell has class `res-fail eth-tx-why`, and its title reads "송신 실패 사유 (daemon): …".
  - With no error field, no reason is invented.
- Lifecycle classes (`st-row-failed`, `data-state`) and the NIC column ("NIC 관찰 —", never ✓ without capture evidence) are unchanged. No retry or new send path was added.

## Checks
- `cargo test --locked -p loom-eth`: all pass (lib 34, cli 7, gateway 15). The new gateway tests are:
  - `default_exports_get_fresh_files_and_explicit_path_still_overwrites`:
    - Two default exports under the same request id ("rq-1", the worst case) get distinct temp-dir paths.
    - The first file's bytes are byte-identical after the second export, and both files are valid pcapng.
    - The counts are right (2 and 1 with the filter), and so is the packet data.
    - An explicit path over a stale file is replaced, giving a valid 2-packet pcapng.
  - `default_export_file_names_stay_in_the_dir_and_never_reuse_a_file`: with `../../etc/x`, the files stay in the dir, the name is sanitized, and the first file is intact.
- `cargo clippy --locked -p loom-eth --all-targets -D warnings`: clean. `cargo fmt -p loom-eth --check`: clean (HEAD versions of these files were already clean).
- `npx vitest run src/sdk src/panels/eth`: 8 files, 100 tests passed. The new tests are:
  - sdk.test.ts: an envelope (eth.export-shaped data, no request_id) gives `result = data`, no envelope keys, `requestId` null, inert state/onChange.
    - A write envelope gives result plus working row tracking.
    - A bare result and a bare business `ok:false` pass through; envelope `data:null` gives null.
    - A failure envelope (E_LOCKED) rejects.
  - eth.test.ts, export completion:
    - The button is busy during the RPC, and a second click makes no second call.
    - The dialog shows the actual path, 17 packets and 1234 B, with the daemon-machine and not-a-download note. No "Activity" appears anywhere.
    - Copy writes the exact path.
    - A second export replaces the dialog with the new path, showing 잘림, 손실 구간 and 주석 생략. Escape closes it.
  - eth.test.ts, failed export envelope: the daemon message is toasted, no dialog or "완료" appears, and the button is re-enabled.
  - eth.test.ts, failed TX: the daemon-shaped row shows `E_LOCKED: <message>` with a title. `st-row-failed` and "NIC 관찰 —" are kept, and a failed row without an error shows no reason.
- Mutation checks:
  - allocator `create_new` → `create+truncate`: both new Rust tests fail.
  - SDK without unwrap: the SDK test and the ETH completion test fail.
  - TX reason removed: the TX test fails. Every source was restored (cmp verified).
- `npx tsc -b`: 0 errors. Scoped eslint on the 4 TS files: clean.
- I did not run the full UI suite or any staged build, per the task.

## Files and fingerprint (sha256)
```
b7092f4462115bc252698b81e5b761f7aec5c4cb025d221d626852497b68aacb  crates/loom-eth/src/gateway/export.rs
ca46028e17de30976bfab8892a1e2e9f2263f715ba738038f4f22d226ca428ee  crates/loom-eth/src/gateway/handlers.rs
d4ae3cb87a391bb0afd847c5f35a18f45607dd3613fb148fce562cbcb133e5a1  crates/loom-eth/tests/gateway.rs
31f368ddaeddc97bf86efc6ecd7ae92e487daf368741899c5a488d5221adf0aa  ui/src/sdk/context.ts
d73e4fd9af6ffce49fd8a14b6be307298c3dc17c740bae0de7f26370742bf16d  ui/src/sdk/sdk.test.ts
1b716a88daa3506b2ce1a80a227157412bb4f5f6ebc65cebd5ed166c9faab57a  ui/src/panels/eth/ethPanel.ts
da9e265e2b5c0d3a6d128608ec6e6088e221e1541f839e3e1d3ef4153e41578a  ui/src/panels/eth/eth.test.ts
92b6aa5e5af60258ac71c588993a3594c2b1f287884d378630dcac10d39aefdb  ui/src/panels/eth/eth.css
```
`ethPanel.ts`, `eth.test.ts` and `eth.css` already had uncommitted edits from the earlier ETH port. My changes are additive hunks only. I did not touch `dialogs.ts`, `ethModel.ts`, `filterBar.ts`, `socketDrawer.ts` or any startup/macro/dock patch.

## Exact limitations
- The path is a daemon-machine path. A remote UI cannot open it, and no download/raw HTTP route was added; schema-v1 `raw_url` is still future CORE work.
- There is no progress while exporting: export is synchronous and bounded (≤ 100 000 packets / 256 MiB / scan budget). The UI shows busy, then the result.
- Default files accumulate in the daemon temp dir (no cleanup policy). After 64 taken names for the same stem the export fails with an explicit error. The stem includes the request id or wall-ns, so this is practically unreachable.
- `gaps` is shown as a count of listed loss entries. `gaps_omitted` is not in the export result, so the count is a lower bound.
- A worker-only failed TX row (worker emitted `failed` without `error`, no daemon final row) still has no reason in the row. That send's RPC failure is toasted by the existing `act()` path. Adding a reason there would need a worker payload change, which is out of scope.
- The changed `eth.export` default behavior needs a daemon rebuild and stage, owned by finalQA.

## For finalQA (after the combined rebuild)
- Export twice from the UI with no path:
  - two distinct `…/loom-eth-<ch>-<tag>[-n].pcapng` paths are shown in the dialog,
  - the first file is unchanged (17 packets),
  - copy-path gives the exact path.
- Force an E_LOCKED send: the TX row shows `E_LOCKED: …` inline and in the title.
