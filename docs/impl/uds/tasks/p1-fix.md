docs/plan/02-core-principle.md 준수

# UDS P1 관문 차단 수정 — p1-sol-review.md P1 6건 (최우선)

공통 규칙 `tasks/p0-common.md`(worktree, 질문 창 금지, 커밋 금지, C2, 보고). 근거·재현: `reports/p1-sol-review.md`, `reports/p1-sol-review-repro.json`, 예제 `crates/loom-uds-load/examples/review_counterexamples.rs`.
**각 반례를 해당 crate의 단언 시험으로 바꾼다(수정 전 실패 → 수정 후 통과).** 기존 시험은 계속 통과.

## uds-sol — `crates/loom-uds-doip`만
- #3 만료·취소된 TX 송신 금지: TxItem에 deadline·취소 상태, 쓰기 직전 재검사, write_all 시간 상한·close 관측, 부분 frame 실패 시 연결 폐기(entity 포함).
- #4 reactivate 취소 후 ActivationBusy 고착: drop guard 또는 activation actor, 시도 id로 늦은 RA 구분.
- #5 RA 응답 tester 주소 검증, 불일치 = 프로토콜 오류·연결 종료.
- P2-1 ACK 상관: ACK 불확실 LA는 재연결까지 잠그거나 연결 폐기 + 늦은 ACK 교차 시험.
- P2-3 RX 바이트 상한(연결·대상), RxOverflow를 링크/health 이벤트로.

## uds-opus — `crates/loom-uds-runner` + `crates/loom-uds`(맞춤)
- #1 **거짓 confirmed 차단(관문)**: 송신 가능 시점부터 in-flight를 보존하는 취소 guard — future drop 시에도 U 등록. 결정: 외부 공개 실행 경로는 `RunnerHandle`만, `Runner::execute`는 crate 내부(또는 drop 안전 계약 + 시험). loom-uds-load는 Handle로 바꾸는 것을 sol과 보고서로 조율(load crate는 sol 소유 — 컴파일이 깨지면 sol에게 알림만).
- #2 응답 `at` stamp를 P2/P2*/완료/총 deadline과 대조 — 초과면 timeout + late/U. 제시간 stamp는 executor 지연으로 실패시키지 않음.
- #6 admission 전 PDU ≤ 4095·request_id 길이 검사, lane/대상 바이트 quota, probe_candidates 크기 상한.
- 네 P1-b runner 요청(RunnerEvent request 태그 → PriorityGate 제거 가능, Handle의 set_timing/replace_transport/unresolved 스냅샷, MonoClock::from_clock)도 이때.
- P2-2 기록만: generation 변경 U 해제는 DoIP(구 소켓 폐기 + RA)에서만 회복 증거, CAN(U-13)은 별도 증거 필요 — 코드 주석·보고서에.
- `tests/e2e.rs` 지연 시험: 전체 시간 단언(60 s)은 부하에 민감 — harness O(n²) 복제를 없애고, 시간 단언은 p95 기준만 유지.

보고: `reports/p1-fix-sol.md`, `reports/p1-fix-opus.md`(반례별 해소 표: 반례·수정·시험 이름·결과). 끝나면 멈춘다.
