docs/plan/02-core-principle.md 준수

# v1.2 사실 확인 — uds-sol (짧게, plan.md 수정 금지)

배경: 마스터 `00-master-plan.md` §2.2 B3. 사용자는 진단을 CANoe로 CAN·이더넷 모두 수행한다. UDS 팀은 Windows 전용 백엔드 `canoe`를 추가한다: Loom이 요청 **바이트**를 분류·승인한 뒤 CANoe COM 자동화로 송신하고, 응답 바이트를 받아 Loom 이벤트로 발행한다. 브리지 코어(COM 세션·측정 상태·재연결)는 CAN 팀 소유.

공개 자료만으로 아래를 확인한다. 1순위 근거: Vector 공개 도움말(CANoe COM Server / Diagnostics 객체), `py_canoe` GitHub 소스(진단 래퍼), 그 외 공개 예제. 설치·실행 금지, 웹 조회만. 확인 못 하면 "미확인"이라고 쓰고 추정과 구분한다.

1. COM으로 **임의 요청 바이트**를 보내고 응답 바이트를 받는 방법(예: `Diagnostic.CreateRequestFromStream`, `DiagRequest.Send`, `Pending`, `Responses`, 응답 `Stream`/`Positive`/`ResponseCode`). 정확한 객체 경로(Networks → Devices → Diagnostic 등)와 블로킹/폴링 여부.
2. 0x78 응답 대기·P2/P2* 타임아웃을 누가 처리하는가(CANoe 진단 설정 vs 호출자). 응답 타임스탬프를 얻을 수 있는가.
3. CANoe에 설정된 **seed-key DLL**을 COM에서 쓰는 방법이 있는가(예: 보안 접근 기본 함수, CAPL `DiagGenerateKeyFromSeed`를 COM에서 호출하는 경로). 없으면 미확인/불가.
4. TesterPresent: CANoe 진단 설정의 자동 TP를 COM에서 켜고 끄거나 상태를 읽을 수 있는가.
5. 진단 기술(CDD/ODX)의 **DID·루틴 이름 목록**을 COM으로 열거할 수 있는가. 아니면 CDD 파일(XML) 직접 읽기가 가능한 형식인가.
6. CANoe DoIP가 Vector 이더넷 인터페이스(VN56xx 등)를 쓸 때와 Windows NIC(TCP/IP stack)를 쓸 때의 구분, 그리고 같은 ECU에 다른 테스터(Loom 자체 DoIP)가 같은/다른 테스터 SA로 라우팅 활성화하면 생기는 일(ISO 13400 응답 코드 0x02/0x03 등, alive check).

출력: `docs/plan/uds/v12-canoe-facts.md`, 60줄 이하, 한국어. 항목마다 "확인 / 미확인 / 추정" 표시와 근거 URL. 끝나면 멈춘다.
