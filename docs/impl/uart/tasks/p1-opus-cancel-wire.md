docs/plan/02-core-principle.md 준수.
질문 창 금지: 대화형 질문을 띄우지 말고, 스스로 정해 보고서에 적거나 보고서에 질문을 쓰고 끝낸다(playbook §6, p0-common.md).

# P1 uart-opus: TX 취소 API를 worker에 연결 (T10 마무리)

공통 규칙: `docs/impl/uart/tasks/p0-common.md`. CARGO_TARGET_DIR = `$HOME/.cache/loom-target/uart-opus`.
보고서: 기존 `/Users/ctmctm/workspace/Loom/docs/impl/uart/reports/p1-opus-worker.md` 끝에 `## 추가: 취소 연결` 절.

## 수정 가능한 파일
- `crates/loom-uart-worker/src/worker.rs`, `src/worker/**`, `tests/worker_*.rs`

## 배경
uart-sol이 `TxHandle::cancel(request_id, cause) -> CancelResult`와 `cancel_lane(TxLane::Agent, cause) -> usize`를 만들었다(커밋 168ac1c, 보고 `/Users/ctmctm/workspace/Loom/docs/impl/uart/reports/p1-sol-tx-cancel.md`, `TxDone.cause` 추가).

## 할 일
1. **64 B 우회 제거**: arbiter `preempt_agent`가 오면 `cancel(진행 중 agent request_id, "interrupted_by_user")` 후 사용자 바이트는 크기와 무관하게 user lane(0x03은 Ctrl-C lane)으로.
2. **STOP latch**(`on_latch(true)`/`cx.latch()` 변화): `cancel_lane(Agent, "stopped")` — 대기 중 agent 요청은 `failed`·`E_STOPPED`, 진행 중은 `outcome_unknown`+정확한 `written_bytes`. 사용자 입력은 계속 송신(계약 04 §4.4).
3. **HOLD**(`uart.hold`): `cancel_lane(Agent, "held_by_user")` → `E_HELD_BY_USER`. 이미 들어간 대기 send도 즉시 거절(arbiter와 일관).
4. **close**: `cancel_lane(Agent, "closed")` 후 정지.
5. `uart.write` 종단 행의 `error.code`·`cause`를 TxDone.cause에서(거짓 성공 금지).
6. 시험: 4 KiB 사용자 붙여넣기가 에이전트 대용량 송신을 청크 경계에서 끊음(우회 제거 확인), STOP 중 대기 agent 요청 failed + 사용자 키는 장치 도달, HOLD 시 진행 중 송신 절단, close 시 미완료 `outcome_unknown`.
fmt·clippy·`cargo test -p loom-uart-worker` 녹색(3회 연속) 후 보고, 대기. 커밋 금지.
