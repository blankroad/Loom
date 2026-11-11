# ② CAN + 시각화 브리프

목표: Python으로 **Vector(XL Driver, CANoe/CANalyzer와 같은 HW) / PEAK PCAN** 등 CAN 인터페이스에 접근. 에이전트는 읽고(신호 값 포함) 쓰고, 사용자는 **CANoe 수준**의 트레이스/플롯/송신 도구를 쓴다. 매번 플롯 코드를 새로 짜지 않도록 **데이터만 주면 렌더링되는 범용 뷰어(`loom.viz`)** 를 이 팀이 소유한다.

## 꼭 다룰 것

- 백엔드: python-can (vector, pcan, virtual, socketcan, kvaser 등), CAN FD 지원, 다채널, 비트레이트/FD 데이터 비트레이트, 하드웨어 타임스탬프. 실제 CANoe가 같은 채널을 쓰고 있을 때 공존 가능성(Vector XL 앱 채널 할당) 조사.
- (선택) CANoe 자체 제어: CANoe COM API / CANoe Python API(`canoe` / Vector의 공식 패키지 여부) / XIL API — 측정 시작/정지, 시스변수/신호 읽기쓰기, 테스트 실행. 범위 넣을지 판단하고 근거 제시.
- 데이터베이스: DBC (+ ARXML, LDF는 범위 판단). cantools 등. 신호 디코드/인코드, 멀티플렉스, 값 테이블.
- 에이전트 동작: 프레임 송신(1회/주기), 신호 단위 송신(DBC로 인코딩, 주기 메시지 내 신호만 바꾸기 = CANoe IG 유사), 트레이스 조회(필터, tail, since), 신호 값 조회/대기(`wait-for signal > x --timeout`), 버스 통계(부하, 에러프레임), 기록 시작/정지.
- 사용자 도구(CANoe 수준): Trace 창(심볼릭/원시, 고정/스크롤 모드, 필터), Graphics(다중 신호 시계열 플롯, 커서, 줌), Data/State 창, Interactive Generator(송신 패널), Statistics, 로그 재생(replay). 웹(예: FastAPI+WebSocket+고성능 플롯 라이브러리) vs 데스크톱(PySide/pyqtgraph) vs TUI — 비교하고 결정. herdr 안 터미널에서도 최소 트레이스는 볼 수 있으면 좋음.
- **`loom.viz` 범용성**: CSV/JSONL/Parquet/BLF/ASC/MF4/라이브 스트림 입력 → 자동 렌더. 에이전트가 `loom viz show data.csv --x ts --y a,b` 같이 한 줄로 사용자에게 플롯을 띄울 수 있어야 함. UART/UDS 팀 요구는 `docs/plan/_xteam/viz-requirements.md` 참고(그쪽이 작성). 대용량(수백만 프레임) 성능 고려(다운샘플링, LTTB 등).
- 기록 포맷: BLF/ASC/MF4 (python-can, asammdf) 입출력.
- **CAN-TP 호스팅**: UDS 팀과 `docs/plan/_xteam/can-uds-isotp.md`에서 인터페이스 합의 (브리프 §3-8). python-can-isotp 등 후보와 타이밍(STmin, FC, N_Bs/N_Cr) 검토.
- 시뮬레이션: virtual bus + 가짜 ECU 노드(DBC 기반 주기 송신)로 HW 없이 전체 데모/테스트.
- 플랫폼: Vector XL은 Windows 전용, PCAN macOS(PCBUSB) 가능 여부 확인.
