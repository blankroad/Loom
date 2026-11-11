docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선함)

# R1 지시 — can-sol

1. 먼저 `docs/plan/can/tasks/r1-common.md`를 읽고 따른다.
2. 출력: `docs/plan/can/r1-sol.md`

## 당신의 관점 (무게를 더 둘 것)
- **라이브러리·프로토콜·드라이버 조사 (실제 문서/소스 확인)**: python-can(최신 버전, 백엔드별 FD/HW 타임스탬프/다채널 지원, BLF/ASC/MF4 지원 여부), Vector XL Driver(앱 채널 할당, CANoe와 동시 사용 시 init access/비-init access 동작), PCAN-Basic + macOS PCBUSB/MacCAN 상태, cantools(DBC/ARXML/멀티플렉스), canmatrix, asammdf(MF4 라이선스 확인 — LGPL?), python-can-isotp(최신 버전, python-can 4.x 호환, 타이밍 파라미터, 실시간성), CANoe COM API / Vector 공식 Python 패키지 존재 여부, 웹 플롯 라이브러리(uPlot, Plotly, ECharts 등) vs pyqtgraph 성능, LTTB 구현. 각 항목에 버전/라이선스/플랫폼/유지보수 상태와 출처.
- **실현 가능성 & 타이밍/성능**: Python GIL 하에서 ISO-TP FC 응답 지연, 주기 송신 지터(python-can `send_periodic` / HW 주기 송신 지원 백엔드), 수천 fps 버스에서 데몬 → 웹 이벤트 전달 부하(배칭), 수백만 프레임 플롯.
- **테스트·시뮬레이션**: virtual bus, socketcan vcan(Linux CI), 가짜 ECU, 기록 재생, CI 구성.
- 필요하면 `spikes/can/`에서 짧은 검증 (예: python-can virtual bus + python-can-isotp 왕복 지연, send_periodic 지터 측정, cantools 디코드 속도). venv는 `spikes/can/.venv`. 측정 결과는 숫자로 초안에 인용.
- 아키텍처/UX/스킬/안전 절도 빠짐없이 쓰되, 그쪽은 Opus가 무게를 두니 간결해도 된다.
