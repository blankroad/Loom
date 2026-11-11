# ③ UDS (DoIP + CAN-TP) 브리프

목표: 에이전트가 **이더넷(DoIP, ISO 13400)** 또는 **CAN(CAN-TP, ISO 15765-2)** 으로 ECU에 UDS(ISO 14229) 진단을 수행해 사용자가 원하는 작업을 한다. 사용자는 진행 상황과 요청/응답을 실시간으로 본다.

## 꼭 다룰 것

- 전송 계층:
  - DoIP: 차량 탐색(UDP vehicle identification / announcement), 라우팅 활성화, 논리 주소, TLS(ISO 13400-2:2019+, 포트 3496) 여부, 다중 ECU 게이트웨이.
  - CAN-TP: **CAN 데몬 위에서 동작** (브리프 §3-8). CAN 팀과 `docs/plan/_xteam/can-uds-isotp.md`에서 인터페이스 합의. 주소 방식(normal/extended/mixed, 11/29bit), 기능 주소(functional) 브로드캐스트, CAN FD.
- UDS 서비스: 세션 제어(0x10), ECU 리셋(0x11), Security Access(0x27, seed-key 알고리즘은 사용자 플러그인), TesterPresent 자동 유지, Read/Write DID(0x22/0x2E), DTC 읽기/삭제(0x19/0x14), Routine(0x31), IO 제어(0x2F), 다운로드/플래시(0x34/0x36/0x37) — 플래시는 범위 판단, 응답 대기(NRC 0x78) 처리, 타이밍(P2/P2*).
- 진단 DB: ODX/PDX, CDD(Vector CANdela) 등 지원 여부 판단. 최소한 사용자 정의 YAML/JSON DID 정의(이름, 스케일, 단위)로 디코드. odxtools 등 검토.
- 에이전트 동작: `loom uds connect --doip <ip> --la 0x...` / `--can ch0 --tx 0x7E0 --rx 0x7E8`, `loom uds read-did F190`, `loom uds dtc list`, 원시 요청 `loom uds raw 22F190`, 시나리오 스크립트(YAML 시퀀스) 실행. 결과는 디코드된 사람이 읽는 형태 + `--json`.
- 사용자 가시성: 요청/응답 트레이스 뷰(서비스명, NRC 해석, 타이밍) — 터미널 및/또는 viz 웹. DID 값 주기 폴링 → 플롯 (viz 요구사항은 `docs/plan/_xteam/viz-requirements.md`에 작성).
- **안전 (매우 중요)**: 서비스별 위험 등급 (읽기 < 세션 변경 < 쓰기/IO 제어/리셋 < 보안접근/플래시). 위험 동작은 사용자 명시 확인, dry-run, 대상 ECU 화이트리스트, 감사 로그. 에이전트 스킬 문서에 규칙 명시.
- 후보 라이브러리 예: udsoncan, doipclient, python-can-isotp, odxtools. 실제 유지보수 상태/라이선스 확인.
- 시뮬레이션: 가상 ECU (UDS 서버 시뮬레이터) — DoIP 엔티티 + CAN-TP 노드, HW 없이 CI 테스트.
