# uds → core 요청

## 1. workspace 의존성 추가: `serde-saphyr` (2026-11-11)
- 용도: UDS 진단 DB YAML 로더(`crates/loom-uds-codec`). 근거: UDS plan.md §7.0·D29(조건부 채택, 크기·깊이·alias 상한 설정 가능), `docs/plan/uds/v2-sol.md`.
- 요청 판: `serde-saphyr = "2.1.1"`. 그 전까지 UDS DB 로더는 JSON만 지원한다(막힘 없음).
- 다른 팀(CAN DBC 외 YAML 설정 등)도 쓰면 공통으로 두는 게 좋다.

## 2. `WriteState::Confirmed` 문서 정정 (2026-11-11)
- 현재 `loom-proto` 주석: "Backend confirmed transmission". UDS는 `confirmed` = **ECU 긍정 응답**(마스터 판정 — `docs/plan/_xteam/can-uds-isotp.md` 정정 채택, UDS plan D2). CAN은 버스 ACK 확인이다.
- 요청: 주석·계약 02를 "backend가 정의한 확인(도메인별: CAN = 버스 송신 확인, UDS = ECU 긍정 응답)"처럼 도메인 정의로 적어 달라. enum 값 변경은 없다(비파괴).
- UDS 결정(참고, 계약과 충돌 없으면 그대로 감): SPRMIB 성공 = `sent` + `payload.ok_suppressed:true`, 송신 후 링크 끊김 — 읽기 = `failed`(`E_DISCONNECTED`), 쓰기 = `outcome_unknown`(재전송 없음).
