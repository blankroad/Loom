# UI workbench late-join — READY_FOR_REVIEW

Owner: foreground implementation, direct mode; no subagents or escalation. Main owns aggregate/fullcheck, browser rerun, build and commit. No commit/push performed. Shared worktree at `/Users/ctmctm/workspace/Loom`; baseline HEAD `ea49d607733e9452d20c753627255ba5228f3919`.

## Result

Shell discovery now expands native `core.channels` string/object entries and unwraps actual Rust reducer SnapshotItems: UART state, CAN fixed/state/payload, UDS state/payload (without outer channel), ETH capture and PSU state. It retains explicit connectivity and generation, silently, independently of mounted panels. Unknown/output-off does not imply open; existing Main unknown-state inference fix is preserved. Existing core.activity snapshot subscription already delivers discovery; no new subscriptions or connection/write calls.

UDS console hydrates native snapshot payloads with empty event backlog and no configured ghost target. It filters snapshot items by UDS domain/kind and configured targets, replaces targets on fresh snapshots, ignores older backlog state through snapshot H, and accepts newer live state. No synthetic events, sequence numbers, transaction rows or historical writes are created.

## Exact SDK API for Main / UART

`ctx.store(topicKey).snapshot` is a read-only `MirrorSnapshot | null`:

```ts
interface MirrorSnapshot {
  readonly items: readonly Readonly<SnapshotItem>[];
  readonly throughSeq: bigint;
  readonly version: number;
  readonly truncated: boolean;
}
```

Available after native snapshot_end; `throughSeq` is that native high water and `version` increases on each completed snapshot. Multipart items remain unpublished until completion. First part/resync clears previous retained snapshot; replacement does not append forever. Retention is limited to min(mirror capacity, 1024) items and 1,048,576 bytes estimated as JSON UTF-16 code units × 2. Oversize items are skipped and `truncated` is set. This estimates serialized content, not total JS heap overhead. The items array is frozen; readonly typing covers the API, with domain state retained as received. Snapshot state is separate from event ring/tail/after and has no invented per-item event seq. Context disposal/resubscribe clears old mirror state and event references.

UART may inspect `item.domain === 'uart'`, then native `item.state` (channel/generation/state/config). Do not replay these as events. Main's uartState.ts/terminalPanel.ts and empty-dock/save-empty changes were untouched.

## Files / change identification

- `ui/src/store/channels.ts`
- `ui/src/store/channels.test.ts`
- `ui/src/store/shellControl.ts`
- `ui/src/store/shellControl.test.ts`
- `ui/src/sdk/storeMirror.ts`
- `ui/src/sdk/storeMirror.test.ts`
- `ui/src/sdk/context.ts`
- `ui/src/panels/uds/consolePanel.ts`
- `ui/src/panels/uds/udsModel.ts`
- `ui/src/panels/uds/uds.test.ts`

Combined source/test content SHA-256 at handoff: `f9724fcf349e45638e4b8a4d2165faed9578b6745cb459142a99bb7b5316dd48`. Existing pre-task edits in channels.ts, channels.test.ts, shellControl.ts and shellControl.test.ts were preserved; HEAD diff includes those earlier edits. SDK/context ownership extension was explicitly released by Main within the authorized coding scope. No QA, shell CSS, ShellApp, InterfaceConnect, dock or UART source edits.

## Focused verification

- `pnpm --dir ui exec vitest run src/store/channels.test.ts src/store/shellControl.test.ts src/sdk/storeMirror.test.ts src/sdk/sdk.test.ts src/panels/uds/uds.test.ts` — PASS, 5 files / 65 tests.
- `pnpm --dir ui exec vitest run src/sdk/m1Sdk.test.ts src/sdk/p1.test.ts src/shell/m1b.test.tsx src/shell/channelExplorer.test.tsx` — PASS, 4 files / 47 tests.
- `pnpm --dir ui typecheck` — PASS after SDK/UDS changes.
- ESLint on all ten listed source/test files — PASS.
- `git diff --check` — PASS.

Total focused suite: 112 passing tests. JSDOM printed existing canvas getContext notices; no test failures. Native shapes were checked directly against loom-proto/src/stream.rs; loom-gateway/src/hub/mod.rs; loom-uart/src/mirror.rs; loom-can/src/gateway.rs FixedReducer; loom-uds/src/module.rs StateReducer; loom-eth/src/gateway/evidence.rs; loom-psu/src/gateway/evidence.rs. UDS tests also use Rust fixtures/state.json.

Regressions cover actual nested envelopes, string core state, generation, closed/unknown/output-off truthfulness, silent repeated discovery/reconnect, empty-backlog second viewer, configured target/domain isolation, snapshot vs older backlog/newer live precedence, multipart completion, item/byte bounds, replacement and disposal.

## Remaining Main checks / constraints

Self-reviewed; no independent reviewer claimed. Browser/fullcheck/build intentionally not run by this owner. Main must rerun actual two-viewer UART dut/aux + CAN ch0 + UDS ecu + ETH cap flow and aggregate/build before commit. No known implementation blocker remains. Return scope to Main after this report; stop edits.
