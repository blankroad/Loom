# P1 관문 수정 — uds-sol

범위: `/Users/ctmctm/workspace/Loom/.worktrees/uds/crates/loom-uds-doip`만 수정. 커밋·질문 창·새 의존성·전역 설치 없음. F1 기본 자동 정책 변경 없음.
결과: DoIP 관문 3개 PASS, **46 tests passed**(기존 29 + 추가 17). runner 하위 E2E는 3 PASS / 1 인계 필요(아래).

## 반례별 해소

| 반례 | 수정 | 단언 시험·결과 |
|---|---|---|
| #3 만료·취소된 queued TX가 나중에 송신 | TxItem deadline·Queued/Writing/Handed/Cancelled 상태, write 직전 검사, drop guard로 취소·ACK 슬롯 정리 | `expired_submit_never_reaches_entity`, `cancelled_queued_submit_never_reaches_entity_and_slot_is_reusable` **수정 전 FAIL → PASS**; `submit_expiring_while_queued_is_rejected_before_write` PASS |
| #3 write_all 무한 대기·부분 frame 뒤 이어쓰기 | client/entity 공통 bounded write, 2초 또는 더 이른 요청 deadline; close/cancel 우선, 실패 시 소켓 폐기. entity ACK/alive/지연 응답에도 동일 적용 | `client_stalled_write_has_independent_socket_timeout`, `cancelled_partial_client_write_closes_connection_and_never_appends_next_frame`, `client_close_interrupts_in_progress_write`, `entity_close_interrupts_stalled_write`, `entity_stalled_reply_is_bounded_and_partial_stream_is_discarded`, io 부분쓰기 시험 2개 PASS(정지 시계·1 B duplex, prefix 뒤 EOF) |
| #4 reactivate 취소 후 ActivationBusy 고착·늦은 RA | u64 시도 ID로 cleanup 결속. queued 취소는 해당 시도만 제거, 송신 시작 후 취소/불확실은 연결 폐기. RA에 wire 시도 ID가 없으므로 같은 소켓에서 새 시도로 재귀속하지 않음 | `cancelled_reactivate_closes_ambiguous_attempt_instead_of_sticking_busy` **수정 전 FAIL → PASS**; `cancelled_queued_reactivation_removes_only_its_own_attempt`, `old_completed_ack_guard_cannot_remove_a_new_attempt_waiter` PASS |
| #5 다른 tester RA 응답으로 Active | tester SA 불일치 = Error + 연결 종료, 활성화 deadline 검사. close/Active 발행 순서도 같은 상태 lock으로 직렬화 | `wrong_tester_routing_activation_is_rejected` **수정 전 FAIL → PASS**; 기존 활성화 거부 코드·0xE0·재활성화 시험 유지 PASS |
| P2-1 늦은 ACK가 다른 SID의 새 waiter를 완료 | ACK timeout/drop의 불확실 LA를 연결 수명 동안 poison. 늦은 ACK/NACK·동일 소켓 RA·target drop/reopen으로 해제 불가; 다른 LA는 계속 사용 | `late_ack_cannot_confirm_a_different_sid_after_ack_timeout`, `dropping_handed_ack_wait_poisoned_la_cannot_be_reopened` **수정 전 FAIL → PASS**. 기존 `nack_and_missing_ack`는 NACK 확인을 유지하고 새 소켓 RA 후 정상 복구 검증으로 보강 PASS |
| P2-3 RX byte quota·손실 관측 | 대상/연결 byte 예약을 enqueue 전 적용, receive/drop 시 RAII 반환. 기존 RxOverflow 링크 이벤트 + sticky health watch·누적 counter 제공 | `rx_byte_quotas_release_on_receive_and_drop_and_health_survives_event_overflow` PASS: 3 LA·최대 PDU, 두 quota·용량 재사용·drop 반환·alive 응답·이벤트 큐 손실 후 health 보존 검증 |

## C2·공개 API

| 상한 | 정책 |
|---|---|
| `MAX_FRAME_WRITE_TIME=2 s` | 요청 deadline과 먼저 닿는 때 종료; 부분 전송 실패 뒤 같은 소켓에 다음 frame을 쓰지 않음 |
| `MAX_RX_BYTES_PER_TARGET=32 KiB`, `MAX_RX_BYTES_PER_CONNECTION=64 KiB` | 기존 건수 cap 32와 먼저 닿는 상한; drop/count + RxOverflow + `ClientHealth::RxOverflowed`, 수신 task는 대기하지 않음 |
| `MAX_POISONED_LAS=64` | tombstone eviction 없음; 용량 소진 시 연결 종료. 복구는 **구 소켓 폐기 + 새 connect/RA** |
| fatal NACK | 단일 bounded 슬롯(9 B), idle writer close 때만 제한 시간 내 flush; 부분 write 실패 뒤에는 append 금지 |

- 추가 API: `ClientHealth`, `DoipClient::health()`, `ClientStats::{rx_buffered_bytes,rx_dropped_total}`, `DoipTarget::buffered_bytes()`, `MAX_FRAME_WRITE_TIME`. 기존 send/recv 시그니처 유지.
- ACK 불확실 LA의 submit/open_target은 `DoipError::AckPending(la)`; `reactivate()`는 해당 poison을 해제하지 않는다. 확정된 ACK의 이전 guard는 ID가 다른 새 waiter를 지우지 않는다.

## 검증·통합 인계

- worktree에서 `CARGO_HOME=.cargo-home CARGO_TARGET_DIR=target` + `--offline`. `cargo fmt -p loom-uds-doip --check` PASS, `cargo clippy -p loom-uds-doip --all-targets --offline -- -D warnings` PASS, `cargo test -p loom-uds-doip --offline` **46 PASS**. 기존 10만 입력 fuzz·분할/연속·fatal NACK·alive·T_TCP 시험 유지.
- 수정 전 `--test gate_fixes`의 최초 6개 단언이 모두 FAIL임을 실제 확인한 후 수정했다. 이후 queued expiry/시도 ID/부분쓰기/byte quota 시험을 추가했다.
- 읽기 전용 통합 확인: `cargo test -p loom-uds-runner --offline --test doip_e2e` → **3 PASS / 1 FAIL**. `nack_ack_loss_and_link_loss_over_doip`(당시 140행)이 ACK timeout 뒤 같은 LA 재사용을 기대해, 실제 `Rejected(NotSent(Busy))`와 기존 기대 `LinkLost`가 충돌한다.
- **uds-opus 인계**: 위 시험에서 ACK timeout 뒤 구 client 폐기·새 connect/RA·generation 증가·replace_transport 후 CloseOnNextDiag 링크 손실을 검증해야 한다. ACK 불확실 재사용을 허용해 기존 기대값을 맞추면 늦은 ACK 반례가 재발하므로 허용하지 않았다. runner/loom-uds/load는 수정하지 않았다.
- CORE 추가 요청·DoIP 범위 미해결 없음. 전체 P1 관문 완료 선언은 runner 담당의 인계 반영·합동 재검증 후에 해야 한다.
