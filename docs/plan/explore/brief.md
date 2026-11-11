# ④ 탐색(Explore) 브리프 — 추가로 만들 만한 것 찾기

목표: UART / CAN / UDS 외에 Loom에 넣을 만한 **추가 장비·프로토콜 컴포넌트**를 찾아 평가하고, 상위 후보에 대해 간이 계획을 세운다.

**판단 기준은 `docs/plan/02-core-principle.md`의 핵심 루프다.** 에이전트가 장비에 직접 접근해 읽고 쓰고, 사용자가 실시간으로 보는 구조가 성립하고 가치가 큰 것만 추천한다. 단순 파일 분석 도구 같은 건 낮은 순위.

## 사용자 맥락

- 차량용 임베디드/진단 개발자로 보인다 (UART 콘솔, Vector/PCAN CAN, DoIP/CAN-TP UDS).
- 관련 기존 프로젝트: `~/Desktop/01_PROJECT/dlt-wrapper` (COVESA DLT 로깅 C 래퍼, 읽기 전용 참고).
- 주 개발 머신 macOS, 실제 HW는 Windows 가능성.

## 후보 씨앗 (예시일 뿐, 빼거나 더하라)

- 차량 네트워크: LIN, FlexRay, CAN XL, Automotive Ethernet 캡처(pcap, SOME/IP, SOME/IP-SD), DLT 수신/전송(dlt-daemon 접속, 제어 메시지), XCP/CCP(측정·캘리브레이션, A2L).
- 디버그/플래시: J-Link / ST-Link / CMSIS-DAP (pyOCD, OpenOCD, GDB) — 메모리/레지스터 읽기쓰기, 변수 라이브 플롯, 플래시, RTT/SWO 로그.
- 계측기: SCPI/VISA(PyVISA) — 전원공급기(전압·전류 설정, DUT 전원 사이클), 오실로스코프(파형 캡처), DMM, 전자부하.
- 저수준 버스: I2C/SPI/GPIO 어댑터 (FT232H, CH341, Bus Pirate, Raspberry Pi), 로직 애널라이저 (sigrok, Saleae).
- 제어 대상 보드: 릴레이/IO 보드 (DUT 전원/점화 신호 제어), Modbus.
- 타깃 OS 접근: SSH/ADB 셸·로그(logcat, journalctl) — UART와 같은 "콘솔" 모델로 통일 가능한가?
- 교차 기능: 여러 장비를 묶는 **시나리오/테스트 러너**(예: PSU로 전원 사이클 → UART 부팅 로그 대기 → CAN 신호 확인 → UDS DTC 읽기), 모든 도메인을 한 화면 타임라인에 동기 표시하는 **통합 대시보드**, 세션 녹화·재생.

## 산출물 `docs/plan/explore/plan.md`

1. **핵심 루프 적합성** 기준표 정의 (직접 접근 / 읽기 / 쓰기 / 실시간 표시 각각 성립 여부).
2. 후보 전체 평가표: 사용자 가치, 핵심 루프 적합성, 구현 난이도, Python 라이브러리 성숙도(실제 확인), HW 확보 난이도, macOS/Windows 지원, 기존 `loom.core`(00-brief §3) 재사용도.
3. **상위 3~5개 추천** 각각에 간이 계획: 핵심 루프 표, 종단 간 시나리오, CLI 초안, 사용자 뷰 형태, 주 라이브러리, M1 걷는 뼈대 정의, 리스크.
4. 공통 계약(`loom.core`, `loom.viz`)에 요구할 것 — 새 도메인을 쉽게 붙이기 위한 플러그인 구조 제안.
5. 추천 착수 순서와 이유, 사용자에게 물어볼 질문(보유 장비 목록 등).

다른 세 팀 폴더를 읽고 중복·충돌을 피할 것. 이 팀은 기존 세 팀의 범위를 바꾸지 않는다 (제안은 "공통 계약 변경 요청" 절로).
