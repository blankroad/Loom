docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선함)

# R1 지시 — explore-sol

1. 먼저 `docs/plan/explore/tasks/r1-common.md`를 읽고 따른다.
2. 출력: `docs/plan/explore/r1-sol.md`
3. 조사 근거 원자료는 `spikes/explore/` 아래 JSON/텍스트로 남긴다 (예: `spikes/explore/pypi-evidence.json`).

## 당신의 관점 (무게를 더 둘 것)
- **라이브러리 실측 조사**: 후보마다 Python 라이브러리를 실제로 확인 — PyPI 최신 버전·릴리스 날짜·라이선스·지원 Python, GitHub 최근 커밋/이슈 활동, Windows/macOS 지원. 예시(빼거나 더할 것):
  - 디버그: pyocd, pylink-square(J-Link), OpenOCD Tcl/GDB 원격, pygdbmi, SEGGER RTT 접근 방법
  - 계측기: pyvisa, pyvisa-py, pyvisa-sim, 대표 SCPI 장비 명령(전원공급기 VOLT/CURR/OUTP, 오실로 파형 전송)
  - 차량 네트워크: LIN(python-can의 LIN 지원 여부, ldfparser, Vector XL LIN, PLIN), FlexRay, CAN XL(python-can 지원 현황), SOME/IP(scapy contrib, someipy 등), pcap(scapy, dpkt, pyshark), DLT(python-dlt / pydlt, dlt-daemon TCP 3490 프로토콜, 제어 메시지), XCP/CCP(pyxcp, A2L 파서 pya2ldb 등)
  - 저수준 버스: pyftdi(FT232H I2C/SPI/GPIO), CH341 라이브러리, Bus Pirate, sigrok(libsigrok Python 바인딩/ sigrok-cli), Saleae Logic 2 Automation API
  - 기타: pymodbus, 릴레이 보드(USB HID/시리얼/Numato/Denkovi 등), asyncssh/paramiko, adbutils(logcat)
- **실현 가능성·타이밍**: 각 후보에서 장비 이벤트 → 화면 지연 목표가 현실적인지 (예: J-Link RTT 폴링 주기·처리량, SWD 메모리 읽기 속도로 가능한 변수 플롯 샘플링 레이트, SCPI 쿼리 왕복 지연, DLT 메시지 처리량, XCP DAQ 레이트). 근거 있는 수치(문서/벤치마크)와 "미확인" 구분.
- **HW/OS**: 후보별 HW 가격대·구하기 쉬움, macOS/Windows 드라이버 상황(예: J-Link macOS 지원, NI-VISA vs pyvisa-py, Vector XL Windows 전용 영향).
- **시뮬레이션**: HW 없이 4요소를 돌릴 수 있는 백엔드가 있는지 (pyvisa-sim, dlt-daemon 로컬 실행, vcan/virtual bus 위 XCP 슬레이브 시뮬, QEMU/Renode + GDB 서버로 디버그 프로브 대체, pymodbus 서버 등). 가능하면 `spikes/explore/.venv`에서 짧게 설치·import·왕복을 검증하고 결과를 기록.
- 아키텍처·UX·스킬도 절 구조대로 빠짐없이 쓰되, 확신 없는 건 "미확인"으로 남겨도 된다.
