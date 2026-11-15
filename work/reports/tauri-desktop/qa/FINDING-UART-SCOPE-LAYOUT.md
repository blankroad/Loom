# FINDING (UART workbench/shell, A-owned → ROOT) — recipients of hidden-tab sessions + split lost after a deep link. 2026-11-15 11:57

Setup: the current final build, native WK e2e app, real daemon. dut (/dev/ttys000) and dut2 (/dev/ttys007, opened from the UI via 수동 입력) are both connected.

## 1. 연결된 모두 / 선택 exclude a connected session whose tab is not visible (correctness of recipient resolution)
- The user split dut | dut2 (2 panes) → scopes worked: 연결된 모두 → dut, dut2 (2) (device counts +1/+1).
- After the panes became tabs of one pane (dut active, dut2 a background tab), the macro scope showed
  **"연결된 모두 → dut · 제외 dut2(상태 확인 중)"**, and in 선택 "dut2 · 상태 확인 중 — 제외". It stayed that way for more than 6 s.
- At the same moment the daemon reported `uart.status dut2` → connected:true, open:true.
- The exclusion is labelled (no silent wrong send), but an actually connected session is dropped from 연결된 모두. The recipient state seems to
  come from the mounted/visible pane rather than the shell/daemon channel state.
- Evidence (opened and assessed): wk/uart-key-broadcast.png, wk/uart-scope-hidden-tab.png.

## 2. User split layout is collapsed after an agent deep link round-trip
- 2-pane split (dut | dut2) → `#/p/uart/terminal?channel=dut` (agent deep link) → back to `#/s/uart`.
- Result: one pane with tabs dut, dut2. The split was not retained. 키 입력 대상 was also back to 현재 세션.
- PARITY-PLAN requires deep links to keep the workbench, and FINAL-PREP requires retained layout. Owner to decide whether a deep link may replace the layout.

## Note
Both are independent of the macro expect fix (FINDING-MACRO-EXPECT.md). Same owner area (w2T scope or ROOT assignment).
