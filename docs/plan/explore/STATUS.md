DONE v2.0
- 주 작업 TEST 기획 완료 → `docs/plan/test/plan.md`, `docs/plan/test/STATUS.md`.
- `explore/plan.md` v2.0: 새 §0.4가 본문보다 우선한다(결정 D34–D36). 도메인별 Rust 경로는 DLT 자체 codec(+dlt-core), SSH russh, **Saleae는 공개 gRPC proto → tonic/prost**(스파이크로 빌드와 연결 실패 처리 확인), Debug probe-rs(+TRACE32 C API FFI), SCPI raw TCP, IO hidapi·tokio-modbus. **Python 예외는 없다.**
- C2: 도메인별 큐·버퍼 상한, worker 메모리 예산(DLT 64 MiB, LA 256 MiB 등), soak 부하 패턴.
- C6: 사용자 화면은 웹 UI 패널이다. 명세는 `_xteam/explore-ui.md`(dlt.trace, la.capture, psu.gauge)에 있고, 러너 화면은 테스트 실행 화면을 재사용한다.
- C7·C8: 탐색 도메인도 결과에 증거 REF를 넣고, 스킬에 "TC 단계로 쓸 때" 절을 둔다. M1을 Rust + 웹 UI 기준으로 다시 정의했다.
- 재개 후 확인: 05 공지의 C4.1(DoIP = PC NIC)은 탐색 도메인에 영향이 없다. 재부팅 전 멈췄던 편집은 다시 적용했다.

---
(이전 상태 v1.2)
DONE v1.2
- 산출물: `docs/plan/explore/plan.md` v1.2 — 마스터 §2.2 B1–B7(사용자 답변, A·D보다 우선) 반영. 바뀐 절만 고쳤고 결정 D30(v1.2 사용자 답변 반영)·D31–D33을 추가했다. Saleae 사실 확인은 `v12-saleae-check.md`(explore-sol, 출처 URL 포함, 미확인 표기).
- **LA = Saleae Logic 2 자동화 1순위, P2로 앞당김**(새 §5.5 L). 캡처 중 데이터 스트리밍 API가 없고, 자동화 캡처 중 앱 창 파형 갱신도 미확인이다. 그래서 화면을 "캡처 중 = Loom에 상태·진행·자극 이벤트 실시간(파형 없음 배지) / 캡처 후 = Loom 레인·디코드 표 + 송신 이벤트 정렬"로 설계했다. 4요소는 모두 △로 정직하게 남기고, §2.3 규칙 8(보유 장비 예외)로 상위에 둔다(점수 67, 부풀리지 않음). CI는 앱 없는 SimLa, 개발은 시뮬레이션 장치 F4241, 실기는 보유 Saleae로 Windows·Linux. sigrok은 2순위(구형 Original Logic이면 sigrok로 실시간 스트리밍 가능).
- 순위·일정: DLT → LA(Saleae) → SSH → PSU(SimPsu 계약 검증 전용, 실장비 구매 시) → Debug(프로브 구매 시). 스코프 보류, 계측기 추가 구매 없음. P0 X0·DLT gate·SimPsu·Saleae gate / P1 PSU(sim) / P2 DLT + LA / P3 러너(la 단계 포함)·SSH / P4 Debug.
- B4: vehicle도 bench와 같은 등급·R2 경로(A10 기본 deny 폐기). `env=vehicle` 배지, R3/R4 뷰어 승인, `immutable_deny`는 유지.
- B1: 실HW 호스트 = Windows x64 + Linux x64. 실HW 체크리스트를 두 OS로 나눔(§5.6.1).
- 요청: CORE에 `job.progress` 이벤트·`source.gap` cause(`capture_gap`, `backend_down`) 추가(D-5). 남은 질문은 마스터 B7에 더해 Q13(Saleae 모델·Logic 2 버전)과 Q14(앱 자동 기동 vs attach)를 추가했다.
