docs/plan/02-core-principle.md 준수 (먼저 읽고 r1-addendum-core.md 도 따를 것)

# R1 지시 — uds-sol (Codex Sol 6.1)

당신은 Loom 프로젝트 UDS 컴포넌트의 워커다. 리드는 uds-lead. **계획 단계이며 구현 코드는 쓰지 않는다** (아래 스파이크 예외). 한국어로 쓴다.

## 먼저 읽을 것
1. `docs/plan/00-brief.md` (공통 브리프, 특히 §3 공통 계약, §4 산출물 15개 절, §5 규칙)
2. `docs/plan/uds/brief.md` (UDS 브리프)
3. `docs/plan/can/brief.md` (CAN 팀 브리프 — CAN-TP 호스팅 담당)
4. 있으면 참고: `docs/plan/_xteam/`, `docs/plan/can/` 진행 중 초안 (읽기만)

## 할 일
`docs/plan/uds/r1-sol.md`에 `00-brief.md` §4의 **15개 절 전체** 초안을 쓴다. 다른 워커(uds-opus)가 아키텍처·UX·안전 쪽에 무게를 둔 독립 초안을 쓰고 있으니, 당신은 아래에 **무게**를 둔다(다른 절도 빠짐없이 쓴다):

- **라이브러리 실제 조사** (PyPI/GitHub/공식 문서를 실제로 확인하고 출처 URL, 최신 버전, 릴리스 날짜, 라이선스, Python 지원 버전, 마지막 커밋 시점 기록):
  - udsoncan, doipclient(python-doipclient), python-can-isotp (can-isotp), python-can, odxtools, scapy(UDS/DoIP 계층), 기타 후보(예: py-uds, pyvit, aioisotp 등 있으면).
  - 각 라이브러리의 핵심 기능 한계: udsoncan의 Connection 추상화로 "CAN 데몬 IPC 위의 ISO-TP"를 끼울 수 있는지(커스텀 BaseConnection), NRC 0x78 처리·P2/P2* 타이머 지원, TesterPresent 자동 유지 지원 여부, DID codec 체계. doipclient의 TLS 지원, 차량 탐색(UDP 13400 broadcast), 라우팅 활성화 타입, 게이트웨이 다중 ECU 처리, udsoncan 연결 어댑터. odxtools의 PDX 로드/디코드 수준과 라이선스. CDD(CANdela) 파싱 가능한 공개 도구 존재 여부.
  - UDS 서버(가상 ECU) 시뮬레이터 구현 수단: 기존 라이브러리(예: udsoncan 서버 측 없음? 다른 후보?), DoIP 엔티티 서버 구현 후보.
- **실현 가능성 / 타이밍**: CAN-TP를 CAN 데몬 안에서 돌리고 UDS는 IPC로 PDU 단위 송수신할 때의 지연 예산(P2 50ms / P2* 5000ms 대비 IPC 왕복), FC/STmin 타이밍, TesterPresent 주기(S3 5s) 유지 위치. DoIP 타이밍(A_DoIP_Ctrl, 라우팅 활성화 응답, alive check), TLS(포트 3496) Python ssl로 가능한지.
- **테스트·시뮬레이션**: 가상 ECU 설계(DoIP 엔티티 + CAN-TP 노드, 설정 가능한 DID/DTC/세션/보안/NRC 주입/지연 주입), python-can virtual bus 위 ISO-TP 루프백, HW 없는 CI 매트릭스(macOS/Linux/Windows), 결함 주입 시나리오 목록.
- HW/OS 지원 매트릭스, 플래시(0x34/36/37) 범위 판단 근거(블록 크기, 압축/암호화 포맷, 체크섬은 OEM별).
- **스파이크 (선택, 권장)**: `spikes/uds/` 안에서만, `spikes/uds/.venv`에 udsoncan/doipclient/can-isotp/python-can을 설치해 (1) virtual bus 위 ISO-TP 루프백으로 22F190 왕복, (2) 로컬 TCP로 가짜 DoIP 엔티티에 라우팅 활성화+UDS 1회 등을 짧게 검증하고 결과(성공/실패, 측정 지연)를 초안에 근거로 인용. 전역 설치 금지.

## 규칙
- 파일 쓰기는 `docs/plan/uds/`와 `spikes/uds/`만. `_xteam/`은 쓰지 말 것 — 리드가 한다.
- 확인한 사실은 출처를, 추정은 "추정"이라고 표시.
- 결정이 필요한 곳은 근거와 함께 하나를 골라 제안하고 대안을 짧게 남긴다.
- 완료되면 응답으로 "DONE r1-sol" 한 줄만 남긴다.
