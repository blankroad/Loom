docs/plan/02-core-principle.md 준수

# R4 지시 — uds-sol: 최종 점검 (plan.md 수정 금지, 보고서만)

리드가 R3에서 `docs/plan/uds/plan.md`를 통합했다. 이 파일을 아래 기준으로 점검해 `docs/plan/uds/r4-check.md`에 쓴다. plan.md는 직접 고치지 않는다(리드가 반영).

## 점검 기준
1. **02 핵심 원칙 체크리스트 6개 항목** (`docs/plan/02-core-principle.md`): 항목별 PASS/FAIL + plan.md 절 위치 + FAIL이면 고칠 문장 제안. 특히 M1 수용 기준이 실제 사용자 화면·개입을 요구하는지, 숨은 연결 경로가 남아 있는지.
2. **`00-brief.md` §4의 15개 절**이 모두 있고 내용이 비어 있지 않은지 (절 번호 매핑표).
3. **UDS 브리프 "꼭 다룰 것"** 누락 여부 (DoIP 탐색/라우팅/TLS/다중 ECU, CAN-TP 주소 방식/기능 주소/FD, 서비스 목록, 0x78/P2/P2*, 진단 DB, 에이전트 동작 예, 사용자 가시성, 안전, 라이브러리, 시뮬레이션).
4. **사실 확인** (출처 URL 또는 로컬 소스 경로와 함께):
   - DoIP 라우팅 활성화 타입: ISO 13400-2에서 central security가 0xE0인가 0xE1인가 (doipclient enum은 0xE1). 판본(2012/2019) 차이인지. 원문 접근이 안 되면 AUTOSAR SWS DoIP 등 공개 문서로 확인하고, 확인 불가면 그렇게 적는다.
   - plan §3.5의 "같은 SA로 두 번째 소켓 활성화 → 0x03 거부" 근거(AUTOSAR R24-11 SWS_DoIP_00105)가 맞는지.
   - plan §7 라이브러리 표의 버전·라이선스·날짜가 `spikes/uds/evidence/*.json`과 일치하는지.
   - plan §9.2 측정값이 `spikes/uds/evidence/roundtrip.json`과 일치하는지.
   - 기능 주소 SF 길이, NRC 0x37에 남은 시간 필드 없음, udsoncan 기본 타이밍 등 plan에 단정한 사실.
   - Textual 라이선스/최신 버전(PyPI) — plan에 "추정"으로 남아 있음. 확인되면 값 제시.
5. **내부 일관성**: §4 명령표의 M 표시 ↔ §11 마일스톤 ↔ §12 작업 분해 ↔ §1 범위가 서로 맞는지. 종료 코드·등급표가 §4/§10/스킬 본문에서 같은지.
6. 결정 기록(§16)에 근거가 약하거나 R2 리뷰의 P1 지적(`r2-sol-review.md` C1–C6)이 반영되지 않은 곳.

## 출력 형식 (`docs/plan/uds/r4-check.md`)
- 맨 위: 종합 판정 1줄 (PASS / PASS with fixes / FAIL)
- A. 02 체크리스트 표, B. 15개 절 매핑, C. UDS 브리프 커버리지, D. 사실 확인 결과 표, E. 일관성 문제, F. 반드시 고칠 것(P1) / 고치면 좋은 것(P2) 목록 — 각각 plan.md 절과 대체 문장 제안.
- 간결하게. 파일 쓰기는 `docs/plan/uds/r4-check.md`만 (필요하면 `spikes/uds/evidence/`에 확인 근거 저장 가능).
- 완료 응답: "DONE r4-sol" 한 줄.
