# FINDING (UART workbench, A-owned; A is code-frozen → ROOT) — every macro `wait` step fails on the real engine. 2026-11-15 11:54

## Repro (current final build, native WK e2e app, real daemon, pty fixtures dut + dut2)
A macro `mping` = 1. send "mping" → 2. wait "ack:mping" ≤2000ms, scope 활성 (= dut2), ▶.
- The send is correct: device rx.log counts are dut2 = 1, dut = 0. The fixture answered `ack:mping`, which is visible in the dut2 pane.
- The wait fails immediately: `uart.expect: bad patterns: unknown variant \`text\`, expected …` (run strip "실패").
- Screenshot (opened and assessed): evidence/native-current/wk/uart-macro-active-run.png.

## Cause
- `ui/src/workbench/uart/workbench.ts:482` sends `patterns: [{ kind: 'text', pattern: text }]`.
- The daemon's pattern kinds are `literal | regex | prompt`. The CLI uses `{"kind":"literal","pattern":…}` (`crates/loom-uart/src/cli.rs` `patterns_json`).
- The unit tests mock `net.call`, so the real contract was never exercised.

## Minimal fix (owner/ROOT)
`kind: 'text'` → `kind: 'literal'`. Add a test asserting the exact request shape (`kind:'literal'`).
The verdict must still come only from `matched`, with no retry.

## Secondary observation (same screenshot, needs owner check)
After the failure, the pane run strip still shows the first entry as **"mping → dut2 · 실행 중 · 1/2 ■ 중지"** above the 실패 entry,
while the status bar says "매크로 실행 0". This looks like a stale running row after an expect rejection. Please verify that the failure path clears it.
**Confirmed after 4 runs** (evidence/native-current/wk/uart-macro-selected-none.png): every failed run leaves a stale
"실행 중 · 1/2 ■ 중지" row **plus** its 실패 row. The rows accumulate in each pane (dut has 2+2, dut2 has 2+2), while the status bar says "매크로 실행 0".

## QA plan
Recipient-scope checks (활성 / 연결된 모두 / 선택) continue on the current build: each run's send happens before its failing wait, and device rx.log is the ground truth.
The wait/verdict path and the STOP-mid-run test (done last) are re-run after the fixed build.
