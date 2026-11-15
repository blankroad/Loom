# Actual Ethernet export usability fix — native foreground Opus 5.5 worker

You implement directly. No Agent/Task/subagent/background LLM. ROOT orchestrates/reviews/Git. FinalQA w2R is exercising actual DoIP in the current app and owns all production UI build/daemon stage/app packaging. Do not touch its browser, fixtures or artifacts. No Git/index or production UI/daemon build. Read only necessary source.

Actual QA verified UI eth.export produces correct 17 packet bytes, but found: RPC no path => `daemon TMPDIR/loom-eth-<ch>-export.pcapng` (ctx.request_id is absent for this export). Repeated UI exports overwrite the same output. UI only toasts "진행률은 Activity", never shows the completed result/path. Failed TX rows show failure without daemon E_LOCKED reason. This is actual user-facing behavior from finalQA, not a hypothetical cleanup.

Own narrowly:
- `crates/loom-eth/src/gateway/handlers.rs` export default-file allocation and relevant existing export tests (no transport/decoder/core changes). May add a focused helper in existing export module if cleaner; don't move unrelated code.
- `ui/src/panels/eth/ethPanel.ts` + `eth.test.ts` for export completion/location and failed-TX explanation, plus minimal CSS only if necessary.
- **ROOT explicitly authorizes this additive SDK behavior change:** `ui/src/sdk/context.ts` + its tests only to expose the existing successful action result as an OPTIONAL readonly field on WriteHandle (e.g. result/data, unwrapped consistently via existing rpcEnvelope helper). Current handle requestId/state/onChange behavior must remain; no auth/native IPC/network capability expansion. Fake handles remain source compatible. Verify actual handler result/envelope rather than inventing a job RPC. There appears to be no generic jobs-get endpoint; don't build one.

Requirements:
1. Default export chooses a fresh file atomically, never silently overwrites previous output. Explicit user-supplied path retains documented semantics. Avoid new dependencies if std create_new + bounded collision allocation works. Don't change destination to a guessed user home/Downloads or grant filesystem access to remote UI. Actual daemon path is the truth.
2. After the real RPC completes, UI shows actual returned path, packet/byte count and truncated/gap information where supported, with a copy-path action. Say it is on the daemon/bench machine, not a browser download. Actual errors must remain errors. Don't falsely advertise progress in Activity if none is supplied. Keep canonical compact toolbar/panel density; a small completion detail/popover is enough, not a new design.
3. Failed TX has a visible actionable daemon reason when present (in details/title/selected row); keep receipt/NIC evidence honesty and existing lifecycle classes. No new retry or send path.
4. Meaningful tests: two default exports produce distinct paths and first bytes remain intact; explicit output still works; SDK bare+enveloped result and failure handling; ETH export actual completion data/path vs failure, failed TX reason. Test the actual contract, don't mirror implementation.

Run relevant focused Rust export tests with `--locked`, scoped UI tests/typecheck/lint. Do not run full UI suite or build staged daemon/UI/app; finalQA does combined final stage after READY. Return `ETH-EXPORT-FIX-STATUS.md` with files/checks/source hashes and exact limitations. Coordinate through ROOT. All previous startup/macro/dock patches are frozen; don't edit them.
