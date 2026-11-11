BLOCKED: P1 게이트 — p1-sol-review P1 반례 6건 수정 중(tasks/p1-fix.md). 특히 #1 거짓 confirmed(송신 후 future drop → 이전 응답이 새 쓰기를 confirmed), #3 취소된 TX 송신 = 관문 차단. 해소 전 P1 MERGE READY 없음.
| # | 반례 | 담당 | 상태 |
|---|---|---|---|
| 1 | 송신 후 drop 시 U 누락 → 거짓 confirmed (runner) | opus | 배정·진행 |
| 2 | 늦은 응답 stamp를 deadline과 미대조 → 거짓 성공 (runner) | opus | 배정·진행 |
| 3 | 만료·취소 TX 송신 / write_all 무한 대기 (doip) | sol | 배정·진행 |
| 4 | reactivate 취소 후 ActivationBusy 고착 (doip) | sol | 배정·진행 |
| 5 | 다른 tester 주소 RA 응답으로 Active (doip) | sol | 배정·진행 |
| 6 | 큐 admission 바이트 상한 없음 (runner) | opus | 배정·진행 |
- P2 3건(ACK 상관 LA 단위, generation U 해제 범위, RX 바이트 예산)도 같은 배정에 포함.
- 미커밋 리뷰 완료분: uds-opus P1-b(loom-uds 핸들러↔runner·DoIP·sim·CallCtx, 시험 17 중 지연 시험 1개가 머신 부하(load avg 371/8코어)로 실패 — 수정 지시함), uds-sol loom-uds-load(시험 8, 5분 지연 p99 9.5 ms). 수정과 함께 커밋 예정.
- RSS 30분 판정 미완: sol 샌드박스에서 ps 불가 → 리드가 ps 가능한 환경에서 5분 실행 필요(미실행).
- 남은 P1: CLI 조각(`loom uds …`, loom-cli-kit DomainCli) 미배정, CORE R1 도메인 등록 대기 후 실 loomd sim·웹 E2E-1(U-11).
- 커밋된 것: 2633aaa runner 우선순위·abort, 68f9277 main 001fa1e 병합.
