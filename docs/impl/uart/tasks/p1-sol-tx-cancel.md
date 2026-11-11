docs/plan/02-core-principle.md 준수.
질문 창 금지: 대화형 질문을 띄우지 말고, 스스로 정해 보고서에 적거나 보고서에 질문을 쓰고 끝낸다(playbook §6, p0-common.md).

# P1 uart-sol: TX actor 취소 API (T10 연결 보강)

공통 규칙: `docs/impl/uart/tasks/p0-common.md`. CARGO_TARGET_DIR 지정 없이 worktree 기본 `target/`. 경로는 `/Users/ctmctm/workspace/Loom/...`만.
보고서: `/Users/ctmctm/workspace/Loom/docs/impl/uart/reports/p1-sol-tx-cancel.md`.
(직전 작업 p1-classify는 리드가 proptest 의존성을 넣어 관문 전부 녹색으로 커밋했다 — 86f297c.)

## 수정 가능한 파일 (이것만)
- `crates/loom-uart-worker/src/tx.rs` (하위 `src/tx/` 허용)
- `crates/loom-uart-worker/tests/tx_*.rs`

`worker.rs`·`worker/`·`channel*`는 uart-opus 소유(읽기만). 공개 API는 **추가만**(기존 시그니처 유지).

## 배경
uart-opus 보고(`/Users/ctmctm/workspace/Loom/docs/impl/uart/reports/p1-opus-worker.md` §0-3): TX actor에 요청 id로 취소하는 API가 없어서, InputArbiter의 `preempt_agent`(사용자 키가 에이전트 전송을 선점, plan §3.7 표 6행)를 "64 B 이하 사용자 바이트는 Ctrl-C lane으로 보내기"로 우회하고 있다. STOP latch가 걸려도 이미 큐에 들어간 에이전트 바이트를 거둘 수 없다.

## 할 일
1. `TxHandle::cancel(request_id, cause) -> CancelResult`: 대기 중이면 큐에서 빼고 `TxDone{state: Failed, written_bytes: 0, cause}`; 진행 중이면 **다음 ≤ 1 KiB 청크 경계**에서 멈추고 `OutcomeUnknown{written_bytes 정확}`(이미 드라이버에 넘긴 바이트는 되돌릴 수 없음); 이미 끝났거나 모르는 id면 `NotFound`. cause 문자열: `interrupted_by_user`, `stopped`(STOP latch), `held_by_user`(HOLD), `closed`.
2. `TxHandle::cancel_lane(TxLane::Agent, cause) -> usize`: agent lane 대기·진행 전부 취소(STOP latch·HOLD·close용). user·Ctrl-C lane은 건드리지 않는다(STOP은 사용자 입력을 막지 않는다 — 계약 04 §4.4).
3. 취소 요청 자체도 상한 있는 제어 큐로(기존 control queue 재사용 가능), Ctrl-C 처리 지연을 늘리지 않을 것(Ctrl-C lane이 여전히 먼저).
4. 시험: 대기 중 취소, 진행 중 취소의 written_bytes 정확성(느린 가짜 transport), 끝난 요청 취소 = NotFound, cancel_lane이 user 키를 건드리지 않음, 동시 Ctrl-C와 취소 경쟁에서 결과 중복 없음(TxDone 정확히 1개/요청).

## 끝나면
fmt·clippy(--all-targets -D warnings)·`cargo test -p loom-uart-worker` 녹색 확인, 보고서(새 API 시그니처와 worker 연결 방법 예시 포함), 대기. 커밋 금지.
