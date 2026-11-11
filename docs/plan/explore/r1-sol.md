# Explore R1 — Sol: 라이브러리·백엔드 실측 중심 추가 컴포넌트 계획

- 기준 문서: `docs/plan/02-core-principle.md` 최우선, `00-brief.md`, Explore 브리프 순.
- 조사일: 2026-11-09 KST. 원자료의 조회 시각은 UTC ISO 8601이다.
- 성격: 독립 계획 초안. Loom 제품 구현이나 다른 팀 계약 변경을 실행하지 않았다.
- 상대 Explore 워커의 R1은 읽지 않았다. UART/CAN/UDS 브리프·R1 범위/계약/안전 절과 `_xteam` 초안을 참고했다.
- 이 문서의 CLI·화면·수용 기준은 **제안**이며 현재 실행 가능한 Loom 기능을 뜻하지 않는다.
- `○`는 설계 가능한 도메인 능력이다. 실제 Loom 종단 간 검증 완료 표시는 아니다.

## 0. 요약

**추천: SCPI 전원공급기 → DLT 라이브 제어 → SSH 타깃 콘솔 → 디버그 프로브 → 조건부 XCP.**
에이전트의 조작, 장비 응답, 사용자 개입을 한 데몬 스트림에서 합치는 순서다.
기존 UART/CAN/UDS의 M1을 먼저 완성하고, 이 확장을 위해 기존 팀의 범위를 넓히지 않는다.

| 추천 | 한 줄 이유 | 초기 구현 경계 | 착수 조건 |
|---|---|---|---|
| 1. SCPI/VISA PSU | 전원 설정·실측·전원 중단을 함께 보므로 ECU 벤치 작업의 앞뒤를 연결한다 | PSU 한 모델 + `pyvisa-sim` | 공통 데몬·출처·기록·승인 계약 |
| 2. DLT | 이미 쓰는 dlt-wrapper의 APID/CTID 로그와 런타임 레벨 변경을 같은 화면에서 확인한다 | DLT V1 TCP + get-info/set-level | COVESA 3.0.1 호환 제어 왕복 스파이크 |
| 3. SSH | UART 없는 Linux/QNX 타깃에서도 로그와 명령을 같은 콘솔 흐름으로 다룬다 | SSH exec + 제한 PTY | UART 콘솔 렌더링 계약 재사용 |
| 4. Debug | RAM/레지스터 읽기·허용 RAM 쓰기·RTT 양방향 콘솔이 가능하다 | CMSIS-DAP/pyOCD 또는 QEMU GDB 중 하나 | 타깃 메모리 맵·프로브 독점 정책 |
| 5. XCP | ECU 변수 측정과 RAM 캘리브레이션 변경을 같은 플롯에서 확인한다 | Ethernet + 1개 변수, DAQ는 다음 | XCP 슬레이브·A2L·쓰기 권한 보유 |

SCPI의 오실로스코프/DMM/전자부하는 같은 장비 도메인의 후속 프로파일이다.
DLT 파일 분석만, 수동 pcap, 로직 애널라이저 수집만으로는 쓰기 루프가 없으므로 상위 추천하지 않는다.
Modbus는 백엔드 검증이 쉽지만 현재 차량 개발 맥락에서 실사용 근거가 약해 예비 후보로 둔다.

실측의 가장 큰 발견은 패키지 이름과 의존성 함정이다.
PyPI `python-dlt`는 automotive DLT가 아니라 데이터 적재 프로젝트의 이전 이름이다.
BMW automotive `python-dlt` 저장소는 별개이며 현재 프로젝트 메타데이터의 이름도 `dlt`여서 이름만으로 설치하면 안 된다.
pyOCD 0.46.1은 `pylink-square<2`를 요구한다. 최신 PyLink 2.1.0과 동시 고정 설치는 실제 resolver에서 실패했다.
성공한 공존 조합은 pyOCD 0.46.1 + PyLink 1.7.0이다.

짧은 검증은 macOS arm64 / Python 3.14.1 / `spikes/explore/.venv`에서만 수행했다.
6개 백엔드 검사와 14개 모듈 import가 최종 통과했다.
PSU 설정 왕복, Modbus TCP, virtual CAN, DLT V1 로그 codec, QEMU GDB SRAM, 로컬 SSH 왕복이다.
어느 것도 실제 HW 속도나 완성된 Loom 뷰어 지연을 검증한 것은 아니다.
DLT 제어 왕복·XCP DAQ·Windows 드라이버는 **미확인**이다.

원자료 위치:

- `spikes/explore/pypi-evidence.json`, `pypi-extra-evidence.json`: 패키지 메타데이터 요약.
- `spikes/explore/pypi-raw/`: 조회 당시 PyPI 원본 JSON, 버전별 파일 목록 포함.
- `spikes/explore/upstream-evidence.json`: GitHub 기본 브랜치 최신 commit Atom 및 이슈 화면 날짜.
- `spikes/explore/upstream-raw/`: GitHub HTML/Atom, 일부 README·라이선스·프로토콜 소스.
- `spikes/explore/github-evidence.json`: GitHub REST 조회 실패도 보존(대부분 403 rate limit).
- `spikes/explore/source-manifest.json`, `docs-raw/`: 공식 문서 URL·상태와 원문.
- `spikes/explore/backend-results.json`, `backend-run.log`: 최종 백엔드 결과.
- `spikes/explore/backend-results-initial.json`: 수정 전 Modbus 실패도 보존.
- `spikes/explore/import-results.json`, `install-imports.log`: import 및 설치 결과.
- `spikes/explore/requirements-observed.txt`: 실측 환경의 전체 의존성 버전.

## 1. 핵심 루프 적합성 기준표

| 요소 | ○ 성립 | △ 부분 | ✗ 불성립 |
|---|---|---|---|
| 직접 접근 A | 데몬이 장치/API/타깃 소켓을 열고 에이전트 명령으로 제어 | 별도 GUI가 이미 연 세션에 제한적으로 위임 | 사람이 먼저 덤프·파일을 만들어야 함 |
| 읽기 R | 시간·개수·바이트 상한으로 응답/관측을 읽고 원시값과 해석을 함께 제공 | 명령 캐시/설정값만 읽거나 캡처 종료 후에만 제공 | 결과를 기계적으로 회수할 수 없음 |
| 쓰기 W | 장비 입력·설정·프로토콜 요청이 실제 타깃 상태에 영향을 줌 | capture arm 등 관측기 설정만 바꾸거나 별도 도메인 자극이 필요 | 파일 decode·passive capture만 함 |
| 실시간 표시 V | 동일 session/event ID로 TX·RX·출처를 즉시 구독하고 사용자 개입 가능 | live export가 제한되거나 화면과 제어 세션이 분리됨 | 사후 파일만 열람 |

직접 접근은 Python이 USB를 직접 여는 경우뿐 아니라 데몬이 소유·감사하는 J-Link/GDB 서비스도 포함한다.
그 경우 장치를 실제 소유하는 vendor 프로세스는 데몬의 관리 대상이며, 다른 클라이언트가 우회 접속하지 않도록 한다.
읽기에서 설정값 readback과 물리 출력 측정은 서로 다른 quality로 기록한다.
쓰기에서 뷰어 필터 변경이나 로컬 파일 저장은 장비 쓰기로 세지 않는다.
실시간 표시는 에이전트 전용 연결을 만든 뒤 별도 사용자 연결을 만드는 방식으로 달성할 수 없다.

### 점수와 탈락 규칙

총점은 `10×(A+R+W+V) + 4U + 3F + 2L + H + O + C` (100점 만점)이다.
루프 기호는 ○=1, △=0.5, ✗=0으로 계산한다.
나머지 U/F/L/H/O/C는 0~5이며 아래 정의를 사용한다.

| 점수 축 | 가중치 | 5점 기준 | 0점 기준 |
|---|---:|---|---|
| U 사용자 가치 | 20 | 현재 차량 벤치 반복 업무와 직접 연결 | 현재 사용자 작업 근거 없음 |
| F 구현 가능성 | 15 | 표준 API·짧은 simulator 왕복 확인 | 사양·SDK·통신 경로 불명 |
| L 라이브러리 | 10 | 최근 활동·명시 라이선스·검증 가능한 API | 오인 패키지/출처 불명 |
| H HW 확보 | 5 | 기존 장비/표준 소켓 또는 저비용 범용 어댑터 | 특수 장비와 구입 경로 불명 |
| O OS | 5 | macOS·Windows 모두 실행 경로 명확 | 주 OS에서 실행 불가 |
| C core 재사용 | 5 | 소유권·이벤트·viz·정책 모두 그대로 활용 | 별도 제품급 런타임 필요 |

상위 추천은 네 루프 모두 ○이며 85점 이상인 후보 중 선택한다.
실제 소유 장비가 미확인인 경우 점수는 잠정치이며, 숫자를 성능 보증처럼 해석하지 않는다.
DLT는 제어 스파이크 통과를, XCP는 슬레이브·쓰기 권한 확보를 실 HW 착수 gate로 둔다.
네 요소 중 △/✗가 있으면 총점과 관계없이 상위 추천 불가다.
파일 전용 후보와 타팀 중복 후보는 점수가 높아도 독립 도메인 착수에서 제외한다.
도메인이 원리상 ○여도 M1에서 사용자 개입까지 시연하지 못하면 M1 실패다.

### 지연과 무손실의 용어

`device_ts`는 장비가 제공한 시간이며 clock domain과 동기 오차를 함께 보낸다.
`host_rx_mono`는 데몬 수신 시각, `render_mono`는 사용자 화면 반영 확인 시각이다.
장비 시각을 얻을 수 없으면 `host_rx→render`만 측정하고 `device→render 미확인`을 표시한다.
폴링 계측은 poll period + 장비 처리 + transport + IPC + render가 사건 지연의 상한을 만든다.
백분위 지연은 같은 장비·채널·출처·부하 profile에서 p50/p95/p99와 표본 수로 보고한다.
기록 무손실은 **수신한 이벤트**를 디스크에 빠뜨리지 않는 목표이며 target 내부 drop까지 없다는 뜻이 아니다.
디스크가 감당하지 못하면 gap/overflow를 남기고 세션을 실패 처리한다. 조용히 무손실을 주장하지 않는다.

## 2. 범위 / 비범위

Explore는 새 장비 backend와 domain adapter의 타당성을 제안한다.
도메인별 명령 의미·위험 분류·뷰어 패널 요구·시뮬레이션 정의까지가 범위다.
이번 라운드는 계획과 사실 확인용 짧은 코드만 만들며 제품 CLI/데몬/뷰어를 구현하지 않는다.

| 기존 소유자 | 그대로 남기는 범위 | Explore가 요청하는 접점 |
|---|---|---|
| UART | COM 포트, 입력 중재, ANSI/바이너리 콘솔, DTR/RTS | SSH·RTT가 재사용할 console panel과 bounded read 계약 |
| CAN | python-can 장치 소유, CAN FD, DBC, trace, loom.viz | XCP CAN transport lease, LIN LDF 범위 협의, 패널 확장 |
| UDS | DoIP/CAN-TP, 진단 세션·DID·DTC·flash 판단 | 전원 사이클 전 진단 작업 중지, scenario correlation |
| loom.core | 세션·IPC·기록·출처·안전 | §5의 capability·artifact·권한·gap 계약 |

CAN-TP는 기존 `_xteam/can-uds-isotp.md`의 CAN 데몬 내부 호스팅 안을 존중한다.
XCP는 UDS와 별개 프로토콜이다. XCP를 UDS raw 서비스로 흉내내지 않는다.
LIN은 CAN 프레임의 변형이 아니다. LDF parser만 CAN 팀에 흡수할지 협의한다.
CAN XL 장비 SDK 검토의 귀속은 CAN 팀에 변경 요청으로 남긴다. 현재 CAN R1의 비범위를 임의로 확대하지 않는다.

교차 시나리오 러너는 독립 HW 도메인 후보가 아니라 기존 명령을 조합하는 공통 기능이다.
통합 대시보드·녹화·재생은 loom.viz/core의 후속 기능이며 새 장비보다 먼저 만들지 않는다.
오프라인 A2L/pcap/DLT 파일 분석, ECU 펌웨어 작성, 상용 툴 대체 전체 기능은 후순위다.
실차 주행 제어, 자동 seed-key 우회, 보안 해제, 무감독 flash는 초기 비범위다.
Windows 원격 HW 호스트 지원은 별도 공통 계약 변경이다. 로컬 IPC에 임의 LAN 바인딩을 추가하지 않는다.

사용자 맥락은 dlt-wrapper README와 `docs/dltw-api-v6-guide.md`에서 확인했다.
현재 libdlt 3.0.1 위의 C/C++ 로깅, APID/CTID, 실행 중 레벨 변경, 매니페스트 기반 설정이 핵심이다.
console 백엔드도 있지만 이는 UART/SSH stderr 흐름으로 볼 수 있고 DLT TCP 제어와 구분해야 한다.
래퍼의 짧은 프로세스 종료 대기와 재연결 ring buffer는 Loom 수신기에서 완전 무손실을 가정할 수 없다는 근거다.
이 프로젝트는 읽기만 했으며 기존 설정·테스트·생성물을 수정하거나 재실행하지 않았다.

## 3. 후보 전체 평가표

기호 열은 `접근/읽기/쓰기/실시간` 순서다.
난이도는 쉬움/중간/높음이며 F 점수와 함께 읽는다.
라이브러리 칸은 `패키지 버전 · 마지막 릴리스(UTC 날짜) · 라이선스`다.
확인 시점의 최신 버전은 PyPI `info.version`이며 설치한 버전과 다를 수 있다.
`M/W`는 macOS/Windows이며 라이브러리 지원과 vendor driver 지원을 분리했다.
총점 뒤 괄호는 `U,F,L,H,O,C`의 원점수로 재계산할 수 있다.

| 후보 | 사용자 가치 | 핵심 루프 A/R/W/V | 구현 난이도 | Python 라이브러리 성숙도(실측) | HW 확보 난이도 | macOS·Windows | core 재사용 | 기존 3팀 중복 | 총점 |
|---|---|---|---|---|---|---|---|---|---:|
| SCPI PSU | DUT 전원·전류 확인 | ○/○/○/○ | 중간 | PyVISA 1.16.2 · 2026-02-27 · MIT; sim 0.7.1 · 2025-09-04 · MIT | 쉬움~중간 | M/W TCP 가능, USB backend별 | 매우 높음 | UART 전송만 공유 가능 | 96 (5,4,5,4,5,5) |
| SSH exec/PTY | 타깃 로그+입력 | ○/○/○/○ | 중간 | asyncssh 2.24.1 · 2026-10-04 · EPL-2.0 OR GPL-2.0-or-later | 기존 Ethernet | M/W 라이브러리 | 매우 높음 | UART console UX | 93 (4,4,5,5,5,5) |
| DLT live+control | 기존 로깅과 레벨 제어 | ○/○/○/○ | 중간~높음 | pydlt 0.3.5 · 2025-03-07 · MIT; BMW wrapper 별도 | 기존 IP 타깃 | pure codec M/W, libdlt Windows 미확인 | 매우 높음 | UART 로그 뷰 일부 | 89 (5,3,3,5,4,5) |
| Debug+RTT | RAM·변수·입력 콘솔 | ○/○/○/○ | 높음 | pyocd 0.46.1 · 2026-11-02 · Apache-2.0; PyLink 2.1.0 · 2026-11-06 · Apache-2.0 | 중간 | M/W 프로브·SDK별 | 높음 | UART RTT 콘솔 재사용 | 86 (5,2,4,4,4,4) |
| XCP | 측정+RAM 캘리브레이션 | ○/○/○/○ | 높음 | pyxcp 0.30.3 · 2026-11-06 · LGPLv3 계열; pya2ldb 1.0.355 · 2026-10-16 · GPLv2 메타데이터 | 중간~어려움 | Ethernet M/W; CAN driver별 | 높음 | CAN transport | 85 (5,2,4,3,4,4) |
| Modbus TCP/RTU | 벤치 IO·센서 | ○/○/○/○ | 쉬움~중간 | pymodbus 3.16.2 · 2026-11-03 · BSD-3-Clause | 쉬움 | TCP M/W, RTU serial driver별 | 매우 높음 | RTU 포트 UART 소유 중복 | 84 (2,4,5,4,5,5) |
| SOME/IP active | 서비스 호출·이벤트 자극 | ○/○/○/○ | 높음 | someipy 2.1.2 · 2026-01-10 · GPL-3.0 소스 확인; scapy 2.8.0 · 2026-10-02 · GPL-2.0-only | 일반 NIC, PHY 별도 | W 지원 명시, M 실행 미검증 | 높음 | DoIP와 NIC만 공유 | 81 (4,2,3,5,4,4) |
| LIN | 액추에이터·스케줄 제어 | ○/○/○/○ | 높음 | ldfparser 0.26.0 · 2025-01-27 · MIT; PLIN vendor SDK | 중간~어려움 | XL/PLIN W, M 경로 미확인 | 높음 | CAN LDF 선택범위 | 80 (5,2,3,2,2,4) |
| FT232H I2C/SPI/GPIO | 보드 센서·핀 제어 | ○/○/○/○ | 중간 | pyftdi 0.57.2 · 2026-06-02 · BSD-3-Clause | 쉬움 | M libusb; W 공식 미지원 | 높음 | FTDI UART 채널 충돌 | 80 (3,3,4,4,3,4) |
| USB 릴레이/IO | 점화·전원선 토글 | ○/△/○/○ | 중간 | hidapi 0.15.0 · 2025-12-09 · GPL-3.0/BSD-style/원본 중 선택; pyserial 3.5 · 2020-11-23 · BSD | 쉬움 | HID M/W, 보드별 driver | 매우 높음 | 시리얼 제어 UART | 79 (4,3,3,4,4,5) |
| ADB shell/logcat | Android ECU 로그+명령 | ○/○/○/○ | 중간 | adbutils 2.12.0 · 2025-11-12 · MIT | Android 타깃 필요 | M/W adb 실행파일·USB 권한 | 높음 | SSH/UART console | 79 (3,3,4,3,4,3) |
| Bus Pirate | 저속 버스 탐색 | ○/○/○/○ | 중간~높음 | pyBusPirateLite/PyPI 미발견; pyserial 3.5 BSD, 공식 binmode 별도 | 중간 | serial M/W, firmware별 | 높음 | UART 포트 점유 | 73 (3,2,2,3,4,4) |
| CH341 I2C/SPI | 저비용 EEPROM/센서 | ○/○/○/○ | 높음 | ch341/ch341a/pych341 PyPI 404; 검증된 공통 wrapper 미확인 | 쉬움 | W DLL/M libusb 경로 미검증 | 보통 | USB/UART mode 충돌 | 69 (3,2,1,4,2,3) |
| CCP | 구형 ECU 캘리브레이션 | ○/○/○/○ | 높음 | pyxcp를 CCP 구현으로 볼 근거 없음; 별도 최신 라이브러리 미확인 | 어려움 | CAN driver 의존 | 높음 | CAN transport | 67 (3,1,1,2,4,4) |
| FlexRay | 기존 ECU 네트워크 제어 | ○/○/○/○ | 매우 높음 | python-flexray PyPI 404; XL C SDK wrapper 필요 | 매우 어려움 | XL W, M 미확인 | 보통 | CAN vendor 소유권 | 64 (3,1,2,1,1,3) |
| CAN XL | 차세대 프레임 교환 | △/△/△/△ | 매우 높음 | python-can 4.6.1 · 2025-08-12 · LGPL-3.0-only는 XL API 아님 | 매우 어려움 | 새 HW+SDK 확인 필요 | 높음 | **CAN 팀 귀속 요청** | 45 (3,1,2,1,1,4) |
| Ethernet passive pcap | 네트워크 원인 관측 | ○/○/✗/○ | 중간 | scapy 2.8.0 GPL-2.0-only; dpkt 1.9.8 · 2022-08-18 BSD; pyshark 0.6 · 2023-04-26 MIT | 일반 NIC/TAP | M libpcap/W Npcap·tshark | 매우 높음 | viz trace 재사용 | 78 (3,4,5,4,5,5) |
| Saleae Logic 2 | 버스 타이밍 관측 | △/△/△/△ | 중간 | logic2-automation 1.0.11 · 2025-11-18 · Apache-2.0 | 중간~어려움 | Logic 2 M/W | 높음 | viz artifact | 59 (3,3,4,2,5,3) |
| sigrok logic capture | 저가 버스 관측 | ○/○/△/△ | 중간~높음 | libsigrok GPL-3.0-or-later; 공식 binding은 PyPI 단순 설치 아님 | 쉬움~중간 | M 빌드/W bindings 제약 | 보통 | viz waveform | 64 (3,2,3,4,3,3) |
| 오프라인 파일 분석 | 저장 trace 해석 | ✗/○/✗/✗ | 쉬움 | pydlt/dpkt/scapy 재사용 | HW 불필요 | M/W | 매우 높음 | viz export/import | 56 (2,5,4,5,5,5) |
| 교차 시나리오 러너 | 전원→부팅→CAN→UDS | △/△/△/△ | 높음 | 독립 장비 lib 없음, 기존 domain API 조합 | 기존 장비 합 | core 계약 의존 | 매우 높음 | **공통 기능 흡수** | 71 (5,3,4,4,5,5) |

### 후보별 근거와 탈락 이유

**SCPI PSU:** [PyVISA 공식 문서](https://pyvisa.readthedocs.io/en/latest/introduction/communication.html)는 resource open/write/query 경로를 제공한다.
PSU는 VOLT/CURR 설정과 OUTP 조작, 측정 응답이 분명해 네 요소의 최소 데모가 작다.
오실로스코프는 acquisition·binary waveform 지연이 달라 같은 latency 약속을 쓰지 않는다.

**SSH:** [AsyncSSH](https://asyncssh.readthedocs.io/)로 상주 연결과 exec/PTY를 호스팅할 수 있다.
로컬 서버에 host key를 검증하며 `status`를 보낸 왕복을 확인했다.
사용자 PTY 입력과 agent exec의 논리 순서를 묶는 console 중재가 핵심이다.

**DLT:** [BMW wrapper](https://github.com/bmwcarit/python-dlt)는 ctypes/libdlt이며 README의 검증 대상은 2.18.8/2.18.10이다.
현재 사용자의 libdlt 3.0.1과 자동 호환된다고 가정할 수 없다.
[pydlt](https://github.com/mikiepure/pydlt)는 pure Python V1 codec 후보이며 런타임 제어 client 완제품으로 간주하지 않는다.

**Debug+RTT:** [pyOCD](https://pyocd.io/docs/)는 Cortex-M 디버그 세션, [PyLink](https://github.com/square/pylink)는 J-Link SDK 경로다.
[OpenOCD](https://openocd.org/doc/html/Server-Configuration.html)는 Tcl RPC 기본 6666과 GDB 기본 3333을 문서화한다.
SWD clock과 변수 샘플률은 같지 않으며 halt·읽기 부작용을 별도 정책으로 다뤄야 한다.

**XCP:** [pyXCP quickstart](https://pyxcp.readthedocs.io/en/latest/quickstart.html)는 measurement/calibration 연결 예를 제공한다.
페이지의 Python 3.8 표기보다 조회한 0.30.3 wheel 메타데이터의 Python ≥3.11,<3.16을 우선한다.
DAQ는 ECU event channel·transport·DTO 길이의 함수이며 일반적인 1 kHz 보장을 제안하지 않는다.

**Modbus:** [pymodbus](https://github.com/pymodbus-dev/pymodbus)는 TCP server/client 양쪽을 제공한다.
로컬 TCP holding register 1234 쓰기·읽기에 성공했다. legacy datastore는 v4 제거 예정 경고가 있었다.
현재 사용자 벤치에 Modbus 장비가 있다는 근거가 없어 우선순위를 낮췄다.

**SOME/IP:** [someipy](https://github.com/chrizog/someipy)는 SD와 service client/server를 제공하나 사양 전체 지원은 아니라고 명시한다.
Scapy SOME/IP contrib import를 확인했지만 SD 가입·RPC 왕복은 미검증이다.
macOS multicast routing, Windows firewall, ECU interface schema 확보가 먼저다.

**LIN:** [PLIN-API](https://www.peak-system.com/products/software/development-packages/plin-api/?L=1)는 Windows DLL과 Python 3 예제를 제공한다.
[Vector XL](https://www.vector.com/en/business-unit/testing-validation-diagnostics/products/network-interfaces/xl-driver-library/)도 LIN/FlexRay API를 제공하지만 python-can의 LIN 구현을 뜻하지 않는다.
LDF는 [ldfparser](https://github.com/c4deszes/ldfparser)가 해석하며 송수신은 별도 backend가 필요하다.

**FT232H:** [PyFtdi 설치 문서](https://eblot.github.io/pyftdi/installation.html)는 macOS/libusb와 Windows 공식 미지원 상태를 명시한다.
I2C repeated-start·SPI mode·핀 전압이 보드별로 중요하다. import 성공은 USB 연결 성공이 아니다.
FT232R과 FT232H를 혼동하거나 VCP와 MPSSE를 동시에 열지 않는다.

**릴레이:** [Numato 공식 목록](https://numato.com/product-category/usb-relay-modules)에 USB relay 제품이 존재한다.
HID/serial 명령 readback만 있는 보드는 접점의 실제 닫힘을 보증하지 못해 읽기를 △로 보수 평가했다.
물리 feedback 입력이 확인된 모델은 ○로 재평가하고 PSU/IO 하위 backend에 넣을 수 있다.

**ADB:** [adbutils](https://github.com/openatx/adbutils)는 adb server를 경유한다.
`adb` device authorization과 Android build 정책은 라이브러리 밖의 의존성이다.
SSH console 확장이 안정화되고 실제 Android 타깃이 있을 때 붙인다.

**Bus Pirate:** [공식 binary mode 문서](https://docs.buspirate.com/docs/binmode-reference/)를 기준으로 firmware 세대별 프로토콜을 고정해야 한다.
예전 pyBusPirateLite 이름을 그대로 pip 설치하는 경로는 확인하지 못했다.
serial CLI를 통합할 수 있지만 잘못된 mode 진입·전압 인가 위험 때문에 FT232H 이후다.

**CH341:** 조회한 세 PyPI 이름이 모두 404였다. 이것이 CH341 Python 프로젝트 전체 부재의 증거는 아니다.
Windows vendor DLL과 libusb 기반 프로젝트를 모델·핀 기능까지 검증해야 한다.
저렴함만으로 안전한 I2C/SPI backend를 보장하지 못하므로 채택 보류다.

**CCP:** pyXCP의 이름이나 CAN 지원만으로 CCP 명령 집합 지원을 추론하지 않는다.
현재 검증한 최신 CCP 라이브러리가 없고 타깃의 CCP 요구도 미확인이다.
기존 CAN transport를 재사용할 수 있어도 별도 protocol adapter 검토가 필요하다.

**FlexRay:** XL C API는 존재하지만 cluster startup·cycle·slot 구성과 지원 HW가 필요하다.
[Vector 공식 XL 페이지](https://www.vector.com/en/business-unit/testing-validation-diagnostics/products/network-interfaces/xl-driver-library/)에서 Windows 배포를 확인했다.
Python wrapper·macOS 경로·보유 cluster 파일이 미확인이라 독립 착수하지 않는다.

**CAN XL:** 설치한 python-can 4.6.1의 Message에는 CAN/FD 속성만 있고 검증 시 FD DLC 상한은 64다.
Vector 상수의 XL 이름은 XL Driver Library이며 CAN XL 지원의 증거가 아니다.
최신 vendor HW/SDK가 CAN XL을 지원해도 python-can 현재 API에서 그대로 송수신된다는 뜻은 아니다.

**pcap:** [Scapy](https://scapy.readthedocs.io/), [dpkt](https://github.com/kbandla/dpkt), [PyShark](https://github.com/KimiNewt/pyshark)는 역할이 다르다.
dpkt는 주로 parsing, PyShark는 tshark 실행 의존, Scapy는 capture/send가 가능하다.
이 행은 passive capture로 평가했으며 active SOME/IP 주입은 별도 후보의 정책 아래 둔다.

**Saleae:** [공식 API](https://docs.saleae.com/automation/guides/getting-started)는 Logic 2 gRPC server를 경유한다.
설정·capture·export는 가능하지만 DUT에 능동 bus 명령을 쓰는 기능이 아니다.
Loom live waveform을 export 없이 지속 수신할 수 있는지는 미확인이라 네 요소를 부분 판정했다.

**sigrok:** [libsigrok](https://sigrok.org/wiki/Libsigrok)은 SWIG Python binding을 제공한다.
프로토콜 decoder가 Python이라는 사실과 하드웨어 Python binding 설치 가능성은 구분한다.
[Windows 문서](https://sigrok.org/wiki/Windows)의 binding 빌드 제약 때문에 우선 `sigrok-cli` subprocess를 검토한다.

**오프라인 파일:** 파일에서 읽기·분석은 가능하지만 실제 장비 접근·쓰기·동시 개입이 없다.
좋은 export/import 부가기능이며 새 컴포넌트 상위 추천에서 탈락한다.

**시나리오 러너:** 전원→UART expect→CAN wait→UDS DTC가 연결되면 가치가 크다.
각 장치의 네 요소를 대신 제공하지 않으므로 core가 있는 다음 단계다.
상태 표시·사용자 abort·출처를 공유하는 bounded job으로 제안하며 독립 장비로 점수를 경쟁시키지 않는다.

### PyPI 버전·지원 Python 상세

조회 URL은 각 패키지 링크의 `/json` API다. `requires_python` 공란은 무제한 호환 보장이 아니다.
BSD/GPL 계열의 정확한 배포 의무는 채택할 wheel/sdist의 LICENSE까지 고정해야 한다.
pya2ldb README badge와 license metadata가 일관되지 않아 허용 라이선스로 완화 해석하지 않았다.

| 패키지 | 최신 버전 | 마지막 릴리스 UTC | Requires-Python | 라이선스/주의 |
|---|---|---|---|---|
| [pyocd](https://pypi.org/project/pyocd/) | 0.46.1 | 2026-11-02 | `>=3.9.0` | Apache-2.0 |
| [pylink-square](https://pypi.org/project/pylink-square/) | 2.1.0 | 2026-11-06 | `>=3.0` | Apache-2.0 |
| [pygdbmi](https://pypi.org/project/pygdbmi/) | 0.11.0.0 | 2023-01-29 | `공란` | MIT |
| [pyvisa](https://pypi.org/project/pyvisa/) | 1.16.2 | 2026-02-27 | `>=3.10` | MIT |
| [pyvisa-py](https://pypi.org/project/pyvisa-py/) | 0.8.1 | 2025-09-04 | `>=3.10` | MIT |
| [pyvisa-sim](https://pypi.org/project/pyvisa-sim/) | 0.7.1 | 2025-09-04 | `>=3.10` | MIT |
| [python-can](https://pypi.org/project/python-can/) | 4.6.1 | 2025-08-12 | `>=3.9` | LGPL-3.0-only |
| [ldfparser](https://pypi.org/project/ldfparser/) | 0.26.0 | 2025-01-27 | `!=3.0.*,!=3.1.*,!=3.2.*,!=3.3.*,!=3.4.*,!=3.5.*,<4` | MIT |
| [someipy](https://pypi.org/project/someipy/) | 2.1.2 | 2026-01-10 | `>=3.8` | 메타데이터 공란; upstream GPL-3.0 |
| [scapy](https://pypi.org/project/scapy/) | 2.8.0 | 2026-10-02 | `<4,>=3.7` | GPL-2.0-only |
| [dpkt](https://pypi.org/project/dpkt/) | 1.9.8 | 2022-08-18 | `공란` | BSD |
| [pyshark](https://pypi.org/project/pyshark/) | 0.6 | 2023-04-26 | `공란` | MIT |
| [python-dlt](https://pypi.org/project/python-dlt/) | 0.2.1 | 2023-04-18 | `공란` | 공란/다른 프로젝트 |
| [pydlt](https://pypi.org/project/pydlt/) | 0.3.5 | 2025-03-07 | `>=3.8` | MIT |
| [pyxcp](https://pypi.org/project/pyxcp/) | 0.30.3 | 2026-11-06 | `<3.16,>=3.11` | LGPLv3 |
| [pya2ldb](https://pypi.org/project/pya2ldb/) | 1.0.355 | 2026-10-16 | `<3.16,>=3.11` | GPLv2 metadata; source GPLv2, SPDX 범위 재확인 |
| [pya2l](https://pypi.org/project/pya2l/) | 0.4.0 | 2026-09-04 | `>=3.9` | BSD metadata |
| [pyftdi](https://pypi.org/project/pyftdi/) | 0.57.2 | 2026-06-02 | `>=3.9` | BSD-3-Clause |
| [pymodbus](https://pypi.org/project/pymodbus/) | 3.16.2 | 2026-11-03 | `>=3.10.0` | BSD-3-Clause |
| [hidapi](https://pypi.org/project/hidapi/) | 0.15.0 | 2025-12-09 | `공란` | upstream GPL-3.0/BSD-style/원본 중 선택 |
| [asyncssh](https://pypi.org/project/asyncssh/) | 2.24.1 | 2026-10-04 | `>=3.10` | EPL-2.0 OR GPL-2.0-or-later |
| [paramiko](https://pypi.org/project/paramiko/) | 5.1.0 | 2026-10-18 | `>=3.9` | LGPL-2.1 |
| [adbutils](https://pypi.org/project/adbutils/) | 2.12.0 | 2025-11-12 | `>=3.8` | MIT |
| [python-lin](https://pypi.org/project/python-lin/) | 0.1.1 | 2019-03-14 | `공란` | MIT |
| [pyplin](https://pypi.org/project/pyplin/) | 0.13 | 2017-08-26 | `공란` | MIT |
| [logic2-automation](https://pypi.org/project/logic2-automation/) | 1.0.11 | 2025-11-18 | `>=3.7` | 메타데이터 공란; upstream Apache-2.0 |
| [python-periphery](https://pypi.org/project/python-periphery/) | 2.5.0 | 2026-10-29 | `공란` | MIT |
| [pyserial](https://pypi.org/project/pyserial/) | 3.5 | 2020-11-23 | `공란` | BSD |
| [dlt](https://pypi.org/project/dlt/) | 1.32.0 | 2026-10-29 | `<3.15,>=3.10` | Apache-2.0 |
| [sigrok](https://pypi.org/project/sigrok/) | 0.1.2 | 2025-05-06 | `>=3.10` | 미확인 |

`python-dlt` 0.2.1은 이름 변경 안내용이며 automotive 후보로 사용하지 않는다.
현재 PyPI `dlt` 1.32.0의 summary도 data loading library다. BMW 저장소와 혼동 금지다.
공식 automotive wrapper는 검증한 commit hash를 source URL로 고정하는 방식부터 검토한다.
PyPI `sigrok`는 stefanhoelzl/python-sigrok 별도 프로젝트이며 libsigrok 공식 SWIG binding 그 자체가 아니다.
404 후보는 `ch341`, `ch341a`, `pych341`, `pyBusPirateLite`, `pyBusPirate`, `saleae-automation`, `python-flexray`다.
Saleae의 실제 패키지명은 `logic2-automation`이며 import 경로는 `saleae.automation`이다.

### GitHub 최근 활동 확인

REST API는 rate limit로 대부분 403을 반환했다. 이 실패를 숨기지 않고 별도 JSON에 보존했다.
대신 공개 기본 브랜치 commit Atom과 이슈 목록 HTML을 읽어 아래 날짜를 얻었다.
커밋은 Atom 첫 entry 시각이고, 이슈는 정렬된 목록 HTML에서 관측한 최신 날짜다.
이슈 날짜는 사용자 댓글·생성·업데이트 표시에 대한 근사 활동 지표이며 maintainer 응답 SLA가 아니다.
라이브러리 릴리스가 오래됐어도 최근 소스 활동이 있을 수 있으므로 두 값을 함께 판단한다.

| 저장소 | 기본 브랜치 최신 commit UTC | 이슈 목록 관측 최신 날짜 | 해석 |
|---|---|---|---|
| [pyocd/pyOCD](https://github.com/pyocd/pyOCD) | [2026-11-02](https://github.com/pyocd/pyOCD/commit/0ce0fbd201010d617d7a4e3582c39b8e8561e33d) | 2026-11-09 | 2026년 커밋 관측 |
| [square/pylink](https://github.com/square/pylink) | [2026-11-06](https://github.com/square/pylink/commit/1904c4775d6b5606a21a5d9d975cef2ba78074bc) | 2026-11-04 | 2026년 커밋 관측 |
| [cs01/pygdbmi](https://github.com/cs01/pygdbmi) | [2023-07-05](https://github.com/cs01/pygdbmi/commit/d570d7157f6fa081208bc82269a8efff87505708) | 2025-12-11 | 활동 간격 주의 |
| [pyvisa/pyvisa](https://github.com/pyvisa/pyvisa) | [2026-10-26](https://github.com/pyvisa/pyvisa/commit/a5b56ab857115e44e59f240f99207b305f77c459) | 2026-10-13 | 2026년 커밋 관측 |
| [pyvisa/pyvisa-py](https://github.com/pyvisa/pyvisa-py) | [2026-11-02](https://github.com/pyvisa/pyvisa-py/commit/f38042328c4701c4720a383d74174a9093a3c03a) | 2026-11-07 | 2026년 커밋 관측 |
| [pyvisa/pyvisa-sim](https://github.com/pyvisa/pyvisa-sim) | [2026-11-02](https://github.com/pyvisa/pyvisa-sim/commit/a7d335c23236424f6747ed153d8d1efc83fa72b0) | 2026-09-28 | 2026년 커밋 관측 |
| [hardbyte/python-can](https://github.com/hardbyte/python-can) | [2026-11-08](https://github.com/hardbyte/python-can/commit/2a45350e3c23cd6ddeb71fc1d1c58dbaf311f263) | 2026-11-05 | 2026년 커밋 관측 |
| [c4deszes/ldfparser](https://github.com/c4deszes/ldfparser) | [2025-03-11](https://github.com/c4deszes/ldfparser/commit/2de76f53d1d9c3be536f24d4239f6cd476e3d87a) | 2025-04-23 | 활동 간격 주의 |
| [chrizog/someipy](https://github.com/chrizog/someipy) | [2026-02-26](https://github.com/chrizog/someipy/commit/fd6bf2cc78fce4e988354fd2d1de28ae584cdb64) | 2026-06-22 | 2026년 커밋 관측 |
| [secdev/scapy](https://github.com/secdev/scapy) | [2026-11-08](https://github.com/secdev/scapy/commit/d2a43b525e145969fae36e42adfb4de8341fbcb8) | 2026-11-08 | 2026년 커밋 관측 |
| [kbandla/dpkt](https://github.com/kbandla/dpkt) | [2024-05-05](https://github.com/kbandla/dpkt/commit/4f8958eb4b375b38d0ab6ffadf2da9b24395394d) | 2026-10-10 | 활동 간격 주의 |
| [KimiNewt/pyshark](https://github.com/KimiNewt/pyshark) | [2026-03-22](https://github.com/KimiNewt/pyshark/commit/91441d3eee47e50e552ac2436df2ec89abad07b0) | 2026-09-19 | 2026년 커밋 관측 |
| [bmwcarit/python-dlt](https://github.com/bmwcarit/python-dlt) | [2026-09-07](https://github.com/bmwcarit/python-dlt/commit/323a8f252488c82ece6191909aa697cfff948e4e) | 2026-05-08 | 2026년 커밋 관측 |
| [COVESA/dlt-daemon](https://github.com/COVESA/dlt-daemon) | [2026-11-05](https://github.com/COVESA/dlt-daemon/commit/144703c39f2031e8e38ee32fc3a9d2e3cb097b28) | 2026-11-05 | 2026년 커밋 관측 |
| [mikiepure/pydlt](https://github.com/mikiepure/pydlt) | [2025-03-07](https://github.com/mikiepure/pydlt/commit/1bf71c290c4de19299763c953e5196350b10e0b6) | 2026-03-02 | 활동 간격 주의 |
| [christoph2/pyxcp](https://github.com/christoph2/pyxcp) | [2026-11-06](https://github.com/christoph2/pyxcp/commit/049d68a13572e40dfafeddc960786f1491996dcd) | 2026-11-09 | 2026년 커밋 관측 |
| [christoph2/pyA2L](https://github.com/christoph2/pyA2L) | [2026-11-03](https://github.com/christoph2/pyA2L/commit/3ef9a67111c4de1f9129a7a190593a24c3dcfc74) | 2026-09-03 | 2026년 커밋 관측 |
| [eblot/pyftdi](https://github.com/eblot/pyftdi) | [2026-06-02](https://github.com/eblot/pyftdi/commit/25d77b02960dd42c018ac7c463b6a5500b95b5b6) | 2026-10-20 | 2026년 커밋 관측 |
| [saleae/logic2-automation](https://github.com/saleae/logic2-automation) | [2026-05-04](https://github.com/saleae/logic2-automation/commit/ee5e1ceebfb63c285828eb337f597f133f7125e4) | 2026-05-20 | 2026년 커밋 관측 |
| [pymodbus-dev/pymodbus](https://github.com/pymodbus-dev/pymodbus) | [2026-11-08](https://github.com/pymodbus-dev/pymodbus/commit/c50599b80752255517c2370cf3b62b0cb8c61f32) | 2026-11-08 | 2026년 커밋 관측 |
| [trezor/cython-hidapi](https://github.com/trezor/cython-hidapi) | [2026-03-15](https://github.com/trezor/cython-hidapi/commit/2924f2c76a6b34ecfa45818d3c2cb5087e1c7a39) | 2026-11-02 | 2026년 커밋 관측 |
| [ronf/asyncssh](https://github.com/ronf/asyncssh) | [2026-11-07](https://github.com/ronf/asyncssh/commit/f1d1c7505468e3ebfd4eda90d831934e20f69bdf) | 2026-11-08 | 2026년 커밋 관측 |
| [paramiko/paramiko](https://github.com/paramiko/paramiko) | [2026-10-18](https://github.com/paramiko/paramiko/commit/0e9ccb91e0f9a68f2587233d523ad7fe46e730fe) | 2026-11-07 | 2026년 커밋 관측 |
| [openatx/adbutils](https://github.com/openatx/adbutils) | [2026-08-10](https://github.com/openatx/adbutils/commit/2382f08ba9394c6a3410ed25ffb89c02801850ec) | 2026-02-15 | 2026년 커밋 관측 |
| [willhyper/pyplin](https://github.com/willhyper/pyplin) | [2018-03-13](https://github.com/willhyper/pyplin/commit/bf0046809202d55618eff5b79175e78b56cde9e0) | 날짜 추출 없음/미확인 | 활동 간격 주의 |
| [richClubb/python-lin](https://github.com/richClubb/python-lin) | [2026-06-02](https://github.com/richClubb/python-lin/commit/72cdbf8abd85567e36e79b66cab9de6321d97374) | 2026-02-13 | 2026년 커밋 관측 |

### HW·OS·가격 현실성

가격은 2026-11-09에 확인한 공식 페이지의 표시값 또는 명시한 예산 가정이다.
한국 배송·VAT·환율·옵션을 포함한 구매 견적이 아니며 재고는 미확인이다.
모델 미정 후보에 근거 없는 정확한 가격을 붙이지 않았다.

| 후보/장비 | 가격대·조달 | macOS 경로 | Windows 경로 | 남은 검증 |
|---|---|---|---|---|
| SCPI PSU | [RIGOL DP832 표시 $449](https://www.rigolna.com/products/dc-power-loads/dp800/dp821a/); 기존 PSU 우선 | LAN+pyvisa-py 우선, USB는 libusb | NI-VISA 또는 pyvisa-py | 보유 모델·옵션별 SCPI/USB 지원 |
| DLT | 기존 ECU IP 연결이면 추가 HW 없음 | pure Python TCP; native libdlt 별도 | pure Python TCP 우선 | 3.0.1 제어·V1/V2 협상 여부 |
| SSH/ADB | 기존 NIC/USB 케이블; 추가 비용 0 가정 | OpenSSH 타깃/adb server | 동일, adb OEM USB driver 가능 | shell 권한·host key·장치 인증 |
| Debug CMSIS-DAP/ST-Link | 보유 dev board 우선; 신규 $20~100는 예산 가정/미확인 | USB backend+권한 | WinUSB/HID 또는 vendor driver | probe/MCU 조합별 지원 |
| J-Link BASE | [SEGGER USD 가격표 $598](https://www.segger.com/purchase/pricing/jlink-related/) | [J-Link package](https://www.segger.com/downloads/jlink/) macOS | 동일 Windows package | DLL/dylib ABI·SDK 배포 조건 |
| XCP Ethernet | 기존 NIC+XCP ECU면 추가 HW 없음 | native wheel/빌드 확인 필요 | wheel+Ethernet | ECU access·A2L·보안 모듈 |
| XCP CAN/CCP | CAN 팀 보유 장비 재사용; 신규 가격 미확인 | 기존 CAN 지원 경로 | Vector/PCAN | transport shim과 bus lease |
| LIN/PLIN | vendor 견적 필요; 보유 PCAN Pro 여부 확인 | 공식 PLIN 경로 미확인 | PLIN API Windows 10/11 | transceiver·master schedule |
| FlexRay/CAN XL | 전문 vendor 견적, 구입 난도 높음 | 지원 미확인 | Vector HW/SDK별 | 모델·라이선스·piggyback |
| FT232H | [Adafruit 표시 $14.95](https://www.adafruit.com/product/2264) | libusb 필요 | PyFtdi 공식 미지원, libusb driver 검토 | 전압·USB driver 공존 |
| CH341 | $5~30는 예산 가정/미확인 | libusb 후보 미검증 | vendor DLL 후보 미검증 | exact chip·기능 mode·전압 |
| Bus Pirate | firmware/세대 미정, 가격 미확인 | USB serial | USB serial | binmode와 전압 출력 |
| USB relay | [Numato 1ch $34.99, 8ch $79.99](https://numato.com/product-category/usb-relay-modules) | 모델별 CDC/HID | 모델별 CDC/HID | 접점 feedback·정격·절연 |
| Modbus | TCP simulator 0; physical IO 가격 미확인 | TCP, RTU USB driver | TCP, RTU USB driver | register map·endianness |
| Saleae | 기존 Logic 8/Pro 보유 우선; 신규 가격 미확인 | Logic 2 macOS 12+ | Logic 2 Win10/11 x64 | Automation API live export |
| sigrok/pcap | 지원 분석기/TAP별, 가격 미확인 | libsigrok 빌드/libpcap | CLI installer/Npcap+tshark | capture 권한·drop 통계 |

[PyVISA-py 설치 문서](https://pyvisa-py.readthedocs.io/en/latest/installation.html)의 transport별 의존성을 확인해야 한다.
TCP socket은 native VISA를 피할 수 있지만 모든 장비가 raw socket/VXI-11/HiSLIP을 동일하게 제공하지 않는다.
NI-VISA의 최신 macOS 버전·Apple silicon 조합은 이번 조사에서 **미확인**이다. 초기 macOS 계약은 LAN+pyvisa-py다.
Vector XL/PLIN Windows 경로를 macOS Python의 ctypes만으로 해결할 수 있다고 주장하지 않는다.
Windows HW 호스트와 macOS 뷰어를 연결하려면 SSH tunnel 등 별도 보안·시계 계약이 필요하다(§8).

### 타이밍 근거와 미확인 분리

| 관측/목표 | 확인된 근거 | 해석·한계 |
|---|---|---|
| VISA sim query median 0.009875 ms, p95 0.010625 ms / 100회 | `backend-results.json` | 메모리 안 simulator 속도; 실 SCPI RTT로 사용 금지 |
| SCPI scalar poll 5 Hz, UI p95 300 ms | 제안 목표 | 실제 query RTT·장비 측정 갱신률 미확인; 측정 후 poll 조정 |
| RTT 최대 약 3.5 MB/s background | [SEGGER KB](https://kb.segger.com/RTT) | 특정 mode의 vendor 상한, Python·사용자 HW 보장 아님 |
| RTT 10 ms poll, UI p95 100 ms | 제안 목표 | probe/USB/firmware buffer·poll cost 실측 필요 |
| RAM 변수 4개×20 Hz부터 | 제안 초기 profile | SWD MHz를 sample/s로 변환할 근거 없음; 실제 batch read RTT 미확인 |
| DLT UI p95 100 ms, 10k msg/s 시험 | 제안 목표/부하 profile | 실 daemon 처리량 벤치 아님; target drops 별도 |
| XCP scalar 20 Hz, DAQ 1 kHz×8 예비 시험 | 제안 시험 profile | 가능한 ECU event/transport인지 미확인; DAQ 성능 주장 아님 |
| SSH UI p95 100 ms / loopback | 제안 목표 | exec 완료 시간·PTY buffering·원격망 지연은 별도 |
| QEMU GDB 4-byte SRAM write/read | 실제 왕복 통과 | 정지 CPU, 실제 SWD·실행 중 일관성·RTT는 검증 안 함 |

공통 UI는 20 Hz(50 ms) batch부터 시작하고 원시 기록 속도와 화면 갱신률을 분리한다.
고속 파형·DAQ는 장비 sample clock과 화면 display clock을 별도 표시한다.
UI 지연 목표는 계획상 SLO이며 현재 측정된 완성품 수치가 아니다.

## 4. 상위 3~5개 추천

아래 다섯 추천의 M1은 모두 simulation 위의 네 요소를 포함한다.
각 M1의 마지막 검증은 agent 조작 중 새 viewer가 붙어 같은 seq를 보고 사용자가 개입하는 것이다.
백엔드만 통과한 현재 스파이크를 M1 완료로 계산하지 않는다.

### 4.1 SCPI/VISA — PSU 먼저, scope/DMM/load 확장

#### a. 핵심 루프

| 요소 | 구현 의미 | 데몬·화면 |
|---|---|---|
| 직접 접근 | VISA resource open, LAN/USBTMC/serial backend 선택 | `scpi` session worker가 단독 소유 |
| 읽기 | `*IDN?`, `MEAS:VOLT?`, `MEAS:CURR?`, `OUTP?` | 명령/응답 trace + 측정값·freshness |
| 쓰기 | VOLT/CURR 한계 설정, OUTP 전환 | 정책 검증 뒤 queue, origin 표시 |
| 실시간 표시 | 동일 TX/RX와 V/I 플롯, 사용자 output off | web PSU panel + TUI trace |

#### b. 종단 간 시나리오

에이전트가 `loom scpi set psu1 --voltage 12 --current-limit 0.2 --timeout 2s --json`을 보낸다.
데몬은 output off 상태에서 프로파일에 맞는 VOLT/CURR 명령을 쓰고 readback한다.
출력 인가 승인을 받은 뒤 `loom scpi output psu1 on --timeout 2s --json`을 보낸다.
장비가 OUTP readback과 측정 11.998 V / 0.083 A를 반환하면 사용자 trace와 V/I 플롯에 함께 뜬다.
장비 사건→화면 목표 p95 300 ms, 수신→화면 목표 p95 100 ms다.
사용자가 패널의 Output OFF를 누르면 동일 queue에서 origin=user로 처리한다.
에이전트의 `loom scpi events psu1 --tail 20 --timeout 1s --json`은 그 중단과 결과를 읽는다.
사용자 변경 뒤 기존 자동 ON 작업을 취소하고 재개에는 새 명시적 요청을 요구한다.

#### c. 실시간성·고부하

scalar 기본 5 Hz, 한 장비에서 미완료 query는 하나만 허용한다.
장비 refresh≥5 Hz·query RTT≤50 ms·render≤50 ms인 profile을 300 ms 사건 지연 목표의 전제로 두며, 미달이면 측정한 지연으로 목표를 재협의한다.
p95 query RTT가 150 ms라면 200 ms poll을 유지하지 않고 poll 저하·stale을 알린다.
장비 내부 measurement refresh가 1 Hz면 5 Hz 쿼리도 5 Hz 실제 측정으로 표시하지 않는다.
UI는 min/max/last 집계를 표시하고 빠진 표본 수·집계 간격을 함께 보낸다.
모든 수신 응답은 원시 기록하며 disk stall이면 새 poll을 줄이고 loss 상태를 기록한다.
scope waveform은 raw binary block을 artifact로 저장하고 trace에는 byte 수·hash·진행률만 둔다.

#### d. CLI 초안·출력

```text
loom scpi discover --timeout 2s --limit 20 --json
loom scpi open --resource TCPIP0::192.0.2.10::inst0::INSTR --profile psu-model-a --timeout 3s --json
loom scpi query psu1 '*IDN?' --timeout 1s --max-bytes 4096 --json
loom scpi set psu1 --voltage 12 --current-limit 0.2 --dry-run --timeout 1s --json
loom scpi poll psu1 --fields voltage,current --period 200ms --duration 10s --max-samples 100 --json
loom scpi wait-for psu1 'current < 0.1A' --timeout 5s --json
loom scpi events psu1 --tail 20 --max-bytes 65536 --timeout 1s --json
{"request_id":"r12","origin":"agent","setpoint":{"V":12,"A_limit":0.2},"confirmed":true,"event_seq":42}
{"event_seq":49,"origin":"user","dir":"tx","command":"output.off","status":"sent"}
```

임의 raw `query`도 부작용이 있을 수 있다. 허용 목록 밖은 write와 같은 정책 심사 대상으로 둔다.
raw binary는 CLI stdout 대신 `artifact_id`로 반환한다.

#### e. 사용자 뷰

```text
[psu1 CONNECTED] [agent/user origin] [age 84ms] [OUTPUT OFF]
Set: 12.0V / limit .2A   Actual: 11.998V .083A   CV/CC
[V/I plot, min/max, stale marker] [SCPI TX/RX trace]
[Edit V] [Edit I limit] [Output ON] [Output OFF] [Stop polling]
```

숫자 편집은 즉시 장비에 쓰지 않고 적용 버튼으로 한 transaction을 만든다.
Output OFF도 DUT reset을 유발할 수 있어 사전 bench policy를 따르되, 승인된 비상 중단은 agent lease보다 우선한다.

#### f. 라이브러리·근거

PyVISA 1.16.2/MIT/Python≥3.10, pyvisa-py 0.8.1/MIT/≥3.10을 기본 검토한다.
[공식 binary values 문서](https://pyvisa.readthedocs.io/en/latest/introduction/rvalues.html)는 binary waveform 전송 API 근거다.
PSU 명령은 [Keysight programming guide](https://www.keysight.com/il/en/assets/9018-04577/programming-guides/9018-04577.pdf)처럼 모델 매뉴얼에서 고정한다.
`VOLT/CURR/OUTP`는 대표 어휘이며 channel selector·측정 명령은 기종마다 다르다.
`VOLT?` 설정값을 `MEAS:VOLT?` 물리 측정처럼 라벨링하지 않는다.

#### g. 시뮬레이션

pyvisa-sim 0.7.1 YAML `psu.yaml`로 IDN/VOLT/CURR/OUTP write/query 왕복을 실제 확인했다.
현재 YAML은 setpoint 기반이며 물리 전압 ramp·전자부하·과전류를 시뮬레이트하지 않는다.
M1에는 stateful simulated PSU의 actual/readback 구분·delay·timeout·user off를 추가해야 한다.
현재 simulator는 네 요소 중 backend 읽기/쓰기만 검증했으며 viewer·origin은 아직 없다.

#### h. M1 걷는 뼈대

단일 simulated PSU를 데몬이 열고 agent set/output, viewer 구독, user off, agent read를 시연한다.
agent와 user 두 클라이언트가 받은 request/event seq가 일치해야 한다.
한계 초과 20 V 요청은 tx 없이 거절되고 거절 사유가 화면에 보여야 한다.
200 ms delay와 timeout을 주입하면 실제 측정값이 stale로 바뀌고 이전 값을 새 값으로 그리지 않는다.
추가 viewer를 늦게 열어도 같은 session의 snapshot+live cursor를 받으면 통과다.

#### i. 안전/가드레일

R0 식별/읽기, R1 poll 설정, R2 출력·전압·전류 변경으로 나눈다.
DUT profile의 max V/I, slew, 최소 off dwell, 채널 고정을 데몬에서 검증한다.
R2 승인은 target/channel/profile hash·최대값·유효시간에 묶는다.
dry-run은 실제 송신 없이 해석된 명령·예상 state transition을 보여준다.
클라이언트 종료 때 무조건 OFF가 안전하다고 가정하지 않고 bench별 fail policy를 명시한다.

#### j. 리스크

SCPI error queue·channel selection·ASCII termination·binary length가 기종별이다.
장비 front panel 조작은 origin=device로 관측하되 사용자 신원을 추측하지 않는다.
고속 scope export가 PSU off 명령을 막지 않도록 장비별 작업 lane을 분리한다.
첫 확장은 한 PSU 모델만 지원하고 범용 raw SCPI 안전성을 과장하지 않는다.

### 4.2 DLT — 라이브 로그 + 컨텍스트 레벨 제어

#### a. 핵심 루프

| 요소 | 구체 동작 | 데몬·화면 |
|---|---|---|
| 직접 접근 | ECU `host:3490` TCP 접속 | 하나의 DLT connection owner + stream parser |
| 읽기 | 로그·control response·APID/CTID 조회 | log table, payload, level state |
| 쓰기 | get-log-info / set-log-level | 제어 transaction과 결과, target context 고정 |
| 실시간 표시 | 같은 socket의 TX/RX 구독, 사용자 level 변경 | TUI/web log+context panel |

#### b. 종단 간 시나리오

에이전트가 `loom dlt open --host 192.0.2.20 --port 3490 --protocol v1 --timeout 3s --json`으로 연결한다.
`loom dlt contexts dlt1 --timeout 2s --limit 100 --json`으로 CAMS/CAM을 확인한다.
`loom dlt level dlt1 --apid CAMS --ctid CAM --level debug --timeout 2s --json`을 보낸다.
데몬은 set-log-level 요청과 응답을 기록하고 log panel에 agent TX / device RX를 표시한다.
타깃 CAMS/CAM debug 로그가 들어오면 수신→표시 p95 100 ms를 목표로 같은 스트림에 나타낸다.
사용자가 context panel에서 info로 바꾸면 origin=user transaction이 생성된다.
에이전트가 `loom dlt events dlt1 --tail 30 --timeout 1s --json`으로 그 개입을 읽고 verbose 조사 작업을 멈춘다.
제어 ACK만으로 실제 앱 레벨 반영을 확정하지 않고 후속 조회 또는 앱 관측까지 상태를 나눈다.

#### c. 실시간성·고부하

로그 host receive→render p95 100 ms, device timestamp가 동기화된 경우에만 device→render를 보고한다.
레벨 변경 ACK 목표 500 ms는 제안이며 앱 반영은 별도 최대 2 s 관측창으로 둔다.
dlt-wrapper README의 libdlt 응답 처리 주기 약 0.5 s 사례 때문에 100 ms 반영을 보장하지 않는다.
M1 부하 profile은 10k msg/s×평균 256 B×60 s이며 실제 target throughput 측정치가 아니다.
UI는 최신 행 제한·APID 집계·오류 우선 표시, 생략 행 수와 렌더 lag를 표시한다.
원시 wire bytes와 decoded records를 저장하고 MCNT gap·target overflow·socket reconnect를 별도 이벤트로 남긴다.
MCNT 순환과 context/session 경계를 고려하며 모든 gap을 네트워크 loss로 단정하지 않는다.
TCP backpressure가 target buffer drop을 유발할 수 있어 디스크 무손실과 target 무손실을 구분한다.

#### d. CLI 초안·출력

```text
loom dlt open --host 192.0.2.20 --port 3490 --protocol v1 --timeout 3s --json
loom dlt read dlt1 --apid CAMS --ctid CAM --level info --tail 50 --max-bytes 65536 --timeout 1s --json
loom dlt contexts dlt1 --limit 100 --timeout 2s --json
loom dlt level dlt1 --apid CAMS --ctid CAM --level debug --dry-run --timeout 1s --json
loom dlt wait-for dlt1 --apid CAMS --pattern 'camera ready' --timeout 10s --max-bytes 16384 --json
loom dlt events dlt1 --tail 30 --timeout 1s --json
{"seq":120,"dir":"tx","origin":"agent","service_id":1,"apid":"CAMS","ctid":"CAM","level":"debug"}
{"seq":121,"dir":"rx","origin":"device","request_id":"r20","status":"acknowledged","effective":"unverified"}
{"seq":130,"dir":"tx","origin":"user","service_id":1,"level":"info"}
```

#### e. 사용자 뷰

```text
[dlt1 / ECU1 / V1 / TCP] [recording] [lag 24ms] [gaps 0]
Context tree: CAMS/CAM [desired DEBUG] [reported INFO] [pending]
Time | origin | ECU | APID | CTID | level | text | raw/control
[Filter] [Level selector + Apply] [Pause display] [Disconnect]
```

Pause는 화면만 멈추고 수신·기록은 계속한다. 재개 때 생략 수와 cursor 이동을 표시한다.
뷰 필터는 장비 log-level 변경과 별도 버튼이며 동일 의미처럼 보이지 않게 한다.
late viewer는 context snapshot과 마지막 seq를 먼저 받고 그 이후 stream에 붙는다.

#### f. 라이브러리·프로토콜 근거

pydlt 0.3.5/MIT/Python≥3.8의 V1 log codec encode/decode를 확인했다.
자동차용 [BMW python-dlt](https://github.com/bmwcarit/python-dlt)는 MPL-2.0이며 native libdlt 의존이다.
최신 PyPI release로 이 wrapper 버전을 확정할 수 없어 commit 고정 소스 후보로만 둔다.
[COVESA protocol header](https://github.com/COVESA/dlt-daemon/blob/master/include/dlt/dlt_protocol.h)의 set-log-level=0x01, get-log-info=0x03을 확인했다.
[daemon 설정](https://github.com/COVESA/dlt-daemon/blob/master/src/daemon/dlt.conf)의 injection 정책과 TCP control 노출도 확인해야 한다.
M1은 V1만 지원하고, 사용자 libdlt 3.0.1의 V2를 자동 지원한다고 표시하지 않는다.

#### g. 시뮬레이션

M1은 격리 포트의 로컬 COVESA dlt-daemon + 작은 APID/CTID 생성 앱으로 한다.
macOS native build 또는 Linux container가 가능 후보이며 Windows는 container/Linux simulation 경로를 구분한다.
이번 스파이크는 pydlt의 CAMS/CAM 로그 44 byte round trip만 통과했다.
실제 daemon 실행·control response·레벨 callback 왕복은 **미검증**이며 M0 gate다.
protocol fixture는 parser fragmentation·잘못된 길이·reconnect 검사에 쓰되 native daemon 검증을 대체하지 않는다.

#### h. M1 걷는 뼈대

가짜 ECU 앱이 info/debug 로그를 내며 agent가 debug enable, user가 info 복귀를 수행한다.
두 조작과 실제 로그 변화가 같은 뷰어에 표시되고 agent bounded read에도 user origin이 남아야 한다.
agent 클라이언트 종료 후에도 viewer와 daemon 기록이 유지되어야 한다.
fragmented TCP·reconnect를 주입해 parser가 새 세션 경계를 표시하고 command 재송신을 자동 반복하지 않아야 한다.
소켓에 보낸 제어가 ACK·효력 관측까지 구분되지 않으면 M1 불합격이다.

#### i. 안전/가드레일

R0 로그/get-info, R1 제한된 verbosity 변경, R2 injection으로 나눈다.
장비별 load 영향에 따라 debug/verbose 변경도 R2로 승격 가능하다.
기본 injection 금지, persist 설정·factory reset·임의 control은 초기 비범위다.
APID/CTID allowlist, verbose TTL, 최대 msg/s 관측 한계, 사용자 revert를 제공한다.
복귀는 변경 전 level이 확인되고 타깃 세션이 같을 때만 시도하며 사용자 변경을 덮어쓰지 않는다.

#### j. 리스크

verbose DLT가 아니면 payload 해석용 metadata가 필요하고 V2·endianness·header 옵션도 고려해야 한다.
control response에 전역 고유 request ID가 없을 수 있어 ECU/서비스/APID/CTID별 요청 직렬화가 필요하다.
정확히 결합 못한 응답은 unsolicited/ambiguous로 남기고 성공으로 추정하지 않는다.
Windows native wrapper 빌드와 libdlt ABI 문제는 pure TCP codec 경로와 분리해 검증한다.

### 4.3 SSH — 타깃 OS 콘솔과 로그

#### a. 핵심 루프

| 요소 | 구체 동작 | 데몬·화면 |
|---|---|---|
| 직접 접근 | 고정 host key로 SSH 연결 | target session owner, 비밀정보는 daemon 보관 |
| 읽기 | exec stdout/stderr, PTY, journal follow | console bytes + stream/channel 식별 |
| 쓰기 | exec 요청, PTY 입력·signal | 동일 입력 중재/승인 queue |
| 실시간 표시 | user terminal attach + agent event read | UART와 같은 console panel 계약 |

#### b. 종단 간 시나리오

에이전트가 `loom ssh open --target bench-ecu --timeout 3s --json`으로 승인된 타깃을 연다.
`loom ssh exec ssh1 --argv '["systemctl","status","camera"]' --timeout 3s --max-bytes 32768 --json`을 실행한다.
장비 stdout/stderr가 exit보다 먼저 live console에 origin=device로 표시되며 수신→표시 p95 100 ms를 목표로 한다.
에이전트가 승인된 `camera-cli status` 입력을 PTY에 보내면 TX도 origin=agent로 보인다.
사용자가 같은 PTY에서 `help`를 입력하면 user origin이 찍힌다.
agent의 `loom ssh read ssh1 --since cursor-10 --tail 30 --timeout 1s --json`에 그 입력과 응답이 같이 나온다.
장시간 journal follow는 daemon job이 소유하고 CLI는 job ID만 반환한다.

#### c. 실시간성·고부하

원격 프로그램의 line/block buffering은 네트워크 수신 이전 지연이므로 UI SLO와 분리한다.
PTY는 터미널 echo가 있을 수 있어 TX와 echo RX를 중복 제거하지 않고 의미를 표시한다.
기본 session 수 4, 동시에 실행하는 write job 1, exec 출력 상한 64 KiB다.
폭주 로그는 raw bytes 기록·화면 행 제한·숨긴 byte 수·stderr 손실 여부를 표시한다.
로그를 drop 없이 저장할 수 없으면 gap+job failure로 알리고 자동 재실행하지 않는다.

#### d. CLI 초안·출력

```text
loom ssh open --target bench-ecu --timeout 3s --json
loom ssh exec ssh1 --argv '["uname","-a"]' --timeout 2s --max-bytes 8192 --json
loom ssh pty ssh1 --command camera-cli --timeout 3s --json
loom ssh send ssh1 --text 'status\n' --timeout 1s --max-bytes 1024 --json
loom ssh wait-for ssh1 --pattern 'ready' --timeout 10s --max-bytes 16384 --json
loom ssh job-start ssh1 --profile journal-camera --ttl 60s --timeout 1s --json
loom ssh read ssh1 --tail 30 --max-bytes 32768 --timeout 1s --json
{"request_id":"r32","origin":"agent","stream":"stdout","exit_status":0,"truncated":false,"cursor":"c32"}
{"seq":208,"origin":"user","dir":"tx","stream":"pty","payload":"help\n"}
```

#### e. 사용자 뷰

```text
[bench-ecu / fingerprint SHA256:...] [ssh1] [jobs 1] [input user]
[PTY console with origin marks] [stdout/stderr exec tabs]
[Take input] [Send Ctrl-C] [Cancel job] [Read-only attach]
```

user가 입력을 잡으면 agent의 후속 PTY send를 잠시 거절한다. 로그 구독은 유지한다.
백그라운드 exec와 PTY는 별도 channel ID로 표시하고 한 셸의 연속 출력처럼 섞지 않는다.

#### f. 라이브러리·근거

[AsyncSSH](https://asyncssh.readthedocs.io/) 2.24.1, Python≥3.10, EPL-2.0 OR GPL-2.0-or-later를 우선한다.
Paramiko 5.1.0/Python≥3.9/LGPL-2.1은 동기 worker 대안이며 두 라이브러리를 동시에 요구하지 않는다.
두 프로젝트의 최근 commit·이슈 활동은 §3 표와 저장한 upstream 자료에서 확인했다.
M/W Python 경로가 있어도 타깃의 SSH 서버·PTY/signal 동작은 target OS별 검증이 필요하다.

#### g. 시뮬레이션

AsyncSSH 임시 local server + 생성 host key 검증으로 `status` 왕복·exit=0을 확인했다.
무인증 simulator는 loopback에만 바인딩하고 제품의 인증 기본값으로 복사하지 않는다.
M1 simulator는 상태가 바뀌는 command loop·PTY echo·delay·stderr·signal 응답을 제공해야 한다.
실제 QNX 셸·sudo·journalctl은 현재 미검증이다.

#### h. M1 걷는 뼈대

한 local server를 daemon이 열고 agent status→user help→agent cursor read를 같은 console에서 시연한다.
host key mismatch는 연결 전에 실패하고 viewer에 이유를 표시해야 한다.
출력 폭주·멀티바이트 분할·stderr·timeout·연결 끊김이 구독자 모두에게 같은 seq로 전달되어야 한다.
agent CLI 종료로 PTY/기록이 사라지거나 재접속 시 hidden shell이 생기면 실패다.

#### i. 안전/가드레일

허용된 정보 조회 R0, 일반 콘솔 입력 R1, 파일/서비스/네트워크 변경 R2, reboot/firmware R3다.
명령 이름만으로 읽기라고 자동 판정하지 않고 profile allowlist와 argv·환경·cwd를 승인에 묶는다.
host key 검증·credential reference·비밀 마스킹을 기본으로 하고 key 자체를 trace에 넣지 않는다.
Cancel은 channel 종료이며 원격 자식 프로세스가 실제 종료됐는지 별도 확인한다.

#### j. 리스크

PTY shell 문자열은 로컬 argv와 달리 target shell escaping이 필요하다. exec도 서버 구현에 따라 command string으로 전달된다.
agent가 로그의 명령문을 그대로 실행하지 않도록 장비 출력과 사용자 지시를 스킬에서 구분한다.
ADB는 이 console 계약을 재사용하되 Android 승인·adb server owner 검증 후 별도 backend로 둔다.

### 4.4 Debug — CMSIS-DAP/pyOCD, J-Link, GDB/RTT

#### a. 핵심 루프

| 요소 | 구체 동작 | 데몬·화면 |
|---|---|---|
| 직접 접근 | probe USB 또는 daemon-managed GDB server | backend별 독점 lease |
| 읽기 | 제한 RAM/레지스터 read, RTT up | memory/state table + 변수 plot + console |
| 쓰기 | 허용 RAM write, halt/resume, RTT down | 승인된 control queue + before/after |
| 실시간 표시 | 모든 디버그 TX/RX와 user 조작 | state/console/plot 공유 session |

#### b. 종단 간 시나리오

에이전트가 `loom debug open --backend qemu-gdb --target sim-mcu --timeout 3s --json`을 보낸다.
`loom debug read dbg1 --address 0x20000000 --length 4 --timeout 1s --json`으로 SRAM을 읽는다.
허용 simulation map에서 `loom debug write dbg1 --address 0x20000000 --hex 78563412 --timeout 1s --json`을 보낸다.
장비 ACK와 readback이 memory panel에 수신→표시 p95 100 ms 목표로 뜬다.
사용자가 memory panel에서 0x00000000으로 바꾸면 origin=user write/readback이 남는다.
agent는 `loom debug events dbg1 --tail 20 --timeout 1s --json`에서 개입을 확인하고 오래된 write 계획을 취소한다.
RTT target에서는 agent down-channel `status\n`→target up-channel 응답→user `help\n`도 같은 session으로 시연한다.

#### c. 실시간성·타이밍

RTT poll 기본안은 10 ms, device→UI p95 100 ms이며 실제 poll cost와 target buffer를 먼저 측정한다.
SEGGER의 [RTT KB](https://kb.segger.com/RTT)는 background 최대 약 3.5 MB/s를 제시하지만 보유 probe 보장치는 아니다.
firmware memcpy 시간, probe 전송량, Python read 처리량, UI latency를 서로 대체하지 않는다.
변수 watch는 4개 scalar×20 Hz부터 시작하고 인접 RAM을 batch로 읽는다.
SWD 4 MHz 설정만으로 1 kHz 변수 그래프를 약속하지 않는다. access RTT·target contention·halt 필요 여부는 미확인이다.
실행 중 multiword read는 원자적 snapshot이 아닐 수 있어 `coherent=false`와 read interval을 표시한다.
RTT nonblocking target drop은 target counter가 있어야 셀 수 있다. 없으면 `target_loss=unknown`이다.

#### d. CLI 초안·출력

```text
loom debug probes --timeout 2s --limit 20 --json
loom debug open --backend pyocd --probe serial:ABC --target cortex_m_model --timeout 3s --json
loom debug read dbg1 --address 0x20000000 --length 16 --max-bytes 4096 --timeout 1s --json
loom debug write dbg1 --address 0x20000000 --hex 78563412 --dry-run --timeout 1s --json
loom debug watch dbg1 --symbols speed,temp --period 50ms --duration 10s --max-samples 400 --json
loom debug rtt-read dbg1 --channel 0 --tail 30 --max-bytes 16384 --timeout 1s --json
loom debug rtt-send dbg1 --channel 0 --text 'status\n' --timeout 1s --json
{"seq":301,"origin":"agent","address":"0x20000000","before":"00000000","after":"78563412","readback":true}
{"seq":304,"origin":"user","operation":"memory.write","policy":"sim-sram-only"}
```

#### e. 사용자 뷰

```text
[probe ABC / target] [RUNNING|HALTED|UNKNOWN] [owner daemon]
[Memory + register table] [watch plot + coherent/age]
[RTT console + origin] [raw command/response trace]
[Halt] [Resume] [Allowed RAM edit] [Stop watch] [Take input]
```

run/halt 상태를 읽기 결과와 함께 표시하고 watch가 CPU를 halt했다면 숨기지 않는다.
사용자 편집도 동일 allowlist·정책·기록을 통과하며 GUI가 probe를 직접 열지 않는다.

#### f. 라이브러리·플랫폼

pyOCD 0.46.1/Apache-2.0/Python≥3.9와 CMSIS-DAP를 첫 물리 backend로 검토한다.
PyLink 최신 2.1.0/Apache-2.0는 pyOCD의 `<2` 제약과 충돌하므로 공존 baseline은 1.7.0이다.
J-Link SDK는 별도 proprietary 배포·사용 조건이며 Python wrapper 라이선스와 구분한다.
pygdbmi 0.11.0.0/MIT는 GDB/MI 파서·프로세스 제어용이지 probe driver나 GDB 실행파일이 아니다.
OpenOCD는 subprocess+Tcl/GDB backend 대안이며 버전·라이선스는 설치할 배포본을 별도 고정한다.
[OpenOCD RPC](https://openocd.org/doc/html/Server-Configuration.html)는 loopback에만 노출하고 vendor service도 daemon이 관리한다.

#### g. 시뮬레이션

설치된 QEMU `lm3s6965evb` GDB stub에 raw RSP로 SRAM 4 byte write/read를 실제 확인했다.
[QEMU GDB 문서](https://www.qemu.org/docs/master/system/gdb.html)와 [Renode GDB 문서](https://renode.readthedocs.io/en/latest/debugging/gdb.html)를 대체 경로로 확인했다.
이번 QEMU는 reset 상태 CPU를 정지시켰고 target firmware 실행·RTT는 검증하지 않았다.
M1에는 증가 counter와 writable command 변수의 작은 firmware 또는 동등한 simulator가 필요하다.
GDB memory loop의 통과로 pyOCD USB/RTT backend까지 통과했다고 쓰지 않는다.

#### h. M1 걷는 뼈대

QEMU 하나를 daemon이 소유하고 agent RAM read/write→viewer memory/plot→user write→agent event read를 시연한다.
실시간 counter plot은 raw reads와 같은 seq를 참조하고 downsample 표시를 가져야 한다.
금지 MMIO/flash 주소는 probe로 전달되기 전에 거절되어야 한다.
agent 중단 후 user resume·watch stop이 같은 session에 작동해야 한다.
첫 physical gate는 CMSIS-DAP 한 모델+허용 SRAM, RTT는 firmware·probe 지원을 확인한 다음이다.

#### i. 안전/가드레일

부작용 없는 허용 SRAM read R0, halt/resume R2, RAM write R2, reset/flash/unlock R3다.
MMIO read도 clear-on-read 부작용이 있을 수 있어 주소별로 R2 이상이 될 수 있다.
M1에서 flash·option bytes·readout unprotect·mass erase·임의 reset은 금지한다.
쓰기 token은 MCU ID·probe serial·메모리 map hash·주소 범위·길이에 묶는다.
dry-run에 주소·길이·byte order·target state·예상 halt 여부를 표시한다.

#### j. 리스크

IDE/RTT Viewer와 동시 probe 소유는 기본 거절하고 vendor별 sharing이 검증된 경우만 예외로 둔다.
디버거가 남긴 halt·breakpoint·RAM 변경은 연결 종료 시 자동 복구가 안전한지 판단해야 한다.
pyOCD·J-Link·OpenOCD를 한 번에 구현하지 않고 capability별 backend 하나로 시작한다.

### 4.5 XCP — 측정·RAM 캘리브레이션, 조건부 착수

#### a. 핵심 루프

| 요소 | 구체 동작 | 데몬·화면 |
|---|---|---|
| 직접 접근 | XCP Ethernet slave connect, CAN은 기존 bus lease 경유 | XCP master worker 단독 세션 |
| 읽기 | 허용 symbol upload/poll, 후속 DAQ DTO | scalar+unit+quality plot, raw CTO/DTO |
| 쓰기 | 허용 RAM characteristic download | before/after/readback, 승인 transaction |
| 실시간 표시 | 동일 측정값·쓰기·user 변경 공유 | calibration table + trace + graphics |

#### b. 종단 간 시나리오

XCP slave와 A2L·쓰기 권한이 있는 개발 ECU라는 조건에서만 시작한다.
에이전트가 `loom xcp open --target bench-xcp --transport eth --timeout 3s --json`으로 연다.
`loom xcp poll xcp1 --symbols SimSpeed --period 50ms --duration 10s --max-samples 200 --json`으로 읽는다.
승인된 `loom xcp set xcp1 SimGain 1.1 --timeout 2s --json`은 RAM 위치만 바꾼 뒤 readback한다.
새 SimGain과 그에 따른 SimSpeed가 trace/표/플롯에 수신→표시 p95 100 ms 목표로 뜬다.
사용자가 SimGain=1.0을 적용하면 동일 정책 경로로 origin=user 이벤트가 생긴다.
agent가 `loom xcp events xcp1 --tail 20 --timeout 1s --json`을 읽고 자신의 이후 sweep을 취소한다.
실 장비 사건→표시 목표는 scalar poll에서 p95 200 ms로 잡되 target timestamp 확보 전에는 미확인이다.

#### c. 실시간성·부하

초기는 scalar 20 Hz×소수 symbol이며 DAQ는 ECU event·ODT 구성을 읽은 다음 단계다.
DAQ 1 kHz×8×4 B는 32 kB/s의 **payload 계산값**이지 처리량 벤치나 CAN wire rate가 아니다.
DTO header·timestamp·framing·CAN arbitration·ECU DAQ capacity가 추가된다.
XCP DAQ에는 모든 구성에서 독립 sample sequence가 있는 것이 아니므로 loss 검출 가능 범위를 기록한다.
UI는 min/max envelope와 원본 sample 수를 보내고 DAQ 원시 DTO를 먼저 디스크에 쓴다.
기록 backlog가 한계를 넘으면 DAQ stop을 시도하고 overflow·잔여 손실·stop 결과를 남긴다.
logger가 못 따라가는데 graph만 부드럽게 보이는 상태는 수용하지 않는다.

#### d. CLI 초안·출력

```text
loom xcp open --target bench-xcp --transport eth --timeout 3s --json
loom xcp inspect xcp1 --timeout 2s --limit 100 --json
loom xcp read xcp1 SimSpeed --timeout 1s --max-bytes 8192 --json
loom xcp set xcp1 SimGain 1.1 --dry-run --timeout 1s --json
loom xcp poll xcp1 --symbols SimSpeed --period 50ms --duration 10s --max-samples 200 --json
loom xcp daq-start xcp1 --profile bench-8vars --ttl 30s --timeout 2s --json
loom xcp wait-for xcp1 'SimSpeed > 10' --timeout 5s --json
{"request_id":"r55","symbol":"SimGain","origin":"agent","old":1.0,"new":1.1,"storage":"RAM","verified":true}
{"seq":904,"origin":"user","symbol":"SimGain","new":1.0,"correlation_id":"cal-1"}
```

DAQ start는 유한 TTL job이며 CLI는 즉시 job ID를 반환한다. TTL 연장도 명시적으로 요청한다.
scalar read는 A2L hash와 유효 decode 조건을 반환한다. 초기 raw address write는 금지한다.

#### e. 사용자 뷰

```text
[xcp1 / ECU build-id / A2L hash] [DAQ state] [clock quality]
Calibration: symbol | RAM/page | old | requested | readback | origin
Graphics: value + unit + age + min/max aggregation + gaps
[Apply allowed value] [Stop DAQ] [Cancel sweep] [CTO/DTO trace]
```

user가 calibration을 바꾸면 해당 symbol의 agent lease를 취소하고 UI가 충돌을 알린다.
page switching·reset으로 주소 의미가 달라지면 기존 편집 폼과 승인 token을 무효화한다.

#### f. 라이브러리·근거

pyXCP 0.30.3, Python≥3.11,<3.16, LGPLv3 계열이며 native 확장·transport별 의존성이 있다.
PyPI 원자료에서 macOS arm64 CPython 3.14 wheel 존재를 확인했지만 이번 환경에서 설치/import는 하지 않았다.
pya2ldb 1.0.355는 christoph2/pyA2L 배포이며 같은 이름의 pya2l 0.4.0/Sauci 프로젝트와 구분한다.
pya2ldb metadata는 GPLv2이고 저장한 LICENSE도 GPL v2 텍스트다. README badge를 근거로 LGPL로 분류하지 않는다.
작은 M1에서는 검증된 symbol map을 써 A2L 도입·배포 의무 결정을 분리한다.
[pyXCP 문서](https://pyxcp.readthedocs.io/en/latest/quickstart.html)의 Ethernet 경로와 [XCPlite](https://github.com/vectorgrp/XCPlite)를 참고한다.

#### g. 시뮬레이션

XCPlite 기반 local Ethernet slave 또는 pyXCP와 왕복 가능한 작은 허용 RAM slave가 후보다.
현재 실제 XCP slave 실행·CTO 왕복·DAQ는 **미검증**이며 M0 gate로 둔다.
Linux vcan은 Linux 전용이고 macOS의 python-can virtual은 프로세스 내부 공유 범위다.
virtual CAN 송수신 성공을 XCP slave 시뮬레이터 성공으로 간주하지 않는다.
M1은 Ethernet을 먼저 택해 CAN 소유권 계약이 끝나지 않아도 독립 검증할 수 있게 한다.

#### h. M1 걷는 뼈대

local slave의 read-only SimSpeed와 writable RAM SimGain 두 symbol을 정의한다.
agent read/set→viewer plot→user set→agent event read가 모두 같은 session/seq에 남아야 한다.
min/max 초과·wrong build ID·A2L hash mismatch는 실제 download 없이 거절되어야 한다.
DAQ 없이 polling만으로 네 요소를 완성하고 DAQ 고속 측정은 M2로 분리한다.
물리 ECU의 access·페이지·보호 정책이 미확인인 동안에는 simulation 전용 표시를 유지한다.

#### i. 안전/가드레일

허용 upload R0, DAQ 구성 R1, 허용 RAM calibration R2, programming/erase/unlock R3다.
R3는 초기 비범위이며 seed-key 알고리즘은 사용자 제공 플러그인과 별도 승인 없이는 호출하지 않는다.
A2L 범위·단위·변환식·datatype·page·ECU build ID를 검증하고 daemon에서 한계를 강제한다.
변경 전 값을 기록해도 자동 rollback을 보장하지 않는다. ECU 상태가 같고 user 변경이 없을 때만 복귀를 제안한다.

#### j. 리스크

A2L·실 firmware 불일치, protected memory, endian/alignment, event timestamp wrap, CAN bus 부하가 크다.
pyXCP의 CAN backend가 기존 CAN owner를 우회해 두 번째 장치를 열면 채택하지 않는다.
필요하면 transport adapter 개발 비용을 별도 추정하며 CCP 지원을 pyXCP에서 추론하지 않는다.

## 5. 공통 계약(loom.core, loom.viz)에 요구할 것 — 새 도메인을 쉽게 붙이는 플러그인 구조 제안

### 5.1 등록과 배포

`loom` 공통 런타임과 domain extras를 나누고 PyPI 배포 시 Python entry points를 사용한다.
예시 그룹은 `loom.domains`, `loom.backends`, `loom.panels`, `loom.simulators`, `loom.policies`다.
이 이름과 ABI는 제안이며 기존 팀 구현을 즉시 변경하라는 확정 계약이 아니다.
`loom[scpi]`, `loom[ssh]` 또는 별도 `loom-scpi` wheel은 lockfile·버전 협상을 같이 배포한다.
기본 설치에 vendor SDK·GUI·native A2L parser를 전부 넣지 않는다.
pyOCD/PyLink 충돌처럼 공존 불가능한 경우 backend worker 환경을 분리하거나 호환 버전을 고정한다.
단순 subprocess 격리만으로 장치 소유권을 두 개 만드는 것은 허용하지 않는다.

| 인터페이스(초안) | 책임 | 금지/요구 |
|---|---|---|
| DomainSpec | domain ID, command schema, capability 목록, schema version | CLI와 GUI의 command 의미가 달라지면 안 됨 |
| BackendFactory | discover/open/close, driver 진단, resource identity | UI·CLI가 직접 backend 인스턴스를 소유하지 않음 |
| SessionBackend | execute(command, deadline, cancel), subscribe, snapshot | 모든 IO가 daemon sequence·기록을 경유 |
| Capability | memory-read, output-control, log-level, console 등 | 지원 안 하는 기능은 명시적 unsupported |
| PolicySpec | 위험 등급·주소/채널/단위 한계·승인 schema | 스킬 경고만으로 안전 검증 대체 금지 |
| SimulatorFactory | 고정 seed·fault profile·시간 진행·resource 명명 | real/sim을 UI와 기록에서 명백히 구분 |
| PanelSpec | trace/scalar/console/waveform/calibration schema | 실행 코드나 임의 HTML을 이벤트 payload로 전달 금지 |

### 5.2 소유권과 중재

resource identity는 URI뿐 아니라 serial·channel·backend·target fingerprint를 정규화한다.
다른 URI로 같은 probe/VISA 장치를 중복 open하는 alias를 감지해야 한다.
상주 daemon이 실제 owner이며 vendor server 프로세스는 그 아래 device worker로 관리한다.
agent/user/script 모두 같은 policy·queue를 거치고 인증된 client role에서 origin을 부여한다.
클라이언트가 payload의 origin을 임의로 user로 위조해 승인 우선권을 얻지 못하게 한다.
read 구독은 다중 허용하되 write는 session/target/symbol 범위 lease로 직렬화한다.
user takeover가 pending agent write를 취소하고, 이미 장비에 보낸 작업은 취소 가능 여부를 별도로 반환한다.
queued/approved/sent/acknowledged/effective/cancelled/failed를 구분해 화면에 거짓 성공을 만들지 않는다.
request ID 중복 수신은 멱등 처리하되 장비 쓰기를 무조건 재실행하지 않는다.

### 5.3 공통 이벤트 확장

기본 `ts,domain,channel,dir,origin,payload`는 보존하고 optional 필드를 versioned envelope로 추가한다.
제안 필드는 `schema_version,session_id,event_seq,request_id,correlation_id,backend_id,simulated,quality`다.
`ts`는 `host_rx_mono_ns,wall_utc,device_ts,device_clock_id,uncertainty_ns`를 구분한다.
초기 계약의 monotonic+wall 시간과 하위 호환 가능한 구체 encoding은 core가 결정한다.
`dir`는 wire IO에 rx/tx, 상태·거절·policy 이벤트에는 null 또는 별도 kind를 허용하도록 협의한다.
TX 사건의 origin=agent/user/script, 자발 RX= device이며 응답의 부모 request origin은 별도 필드로 잇는다.
사용자의 명령에 응답한 장비 RX를 origin=user로 바꾸지 않는다.
대형 raw data는 `artifact_id,offset,length,sha256,format` 참조로 보내고 CLI·WebSocket payload 상한을 유지한다.
단위·스케일·enum·raw bytes·physical value·decode version을 분리해 저장한다.
`quality`는 stale, gap, aggregated, readback-only, coherent, target-loss-unknown 등을 담는다.

### 5.4 기록과 backpressure

acquisition→durable writer→subscriber fanout 구조를 기본으로 하고 UI 지연이 장치 수신을 막지 않게 한다.
내구성 ACK 이전과 이후를 구분하고 disk-full 시 write 제한·poll/DAQ 정지 정책을 session에 공지한다.
단일 무한 queue 대신 byte/event budget과 high-water mark를 둔다.
UI가 늦으면 aggregate/latest view로 전환하며 원본 cursor로 다시 읽을 수 있어야 한다.
`gap`에는 source=device/transport/daemon/recorder/viewer, 알 수 있는 count와 unknown을 구분한다.
raw log loss를 UI 생략 수와 합쳐 하나의 dropped 숫자로 숨기지 않는다.
reconnect 시 새 epoch·snapshot cursor를 주고 오래된 승인·pending write를 자동 복원하지 않는다.

### 5.5 뷰어 패널 등록

CAN 팀의 loom.viz가 기본 trace·scalar plot·state·artifact waveform 패널을 소유한다.
SSH/RTT console은 UART console 계약을 재사용하고 DLT text/structured payload를 추가한다.
SCPI setpoint vs measurement, debug coherent state, XCP calibration table을 metadata로 표현한다.
viewer는 snapshot at seq N + subscribe after N을 원자적으로 받아 늦은 접속에서도 gap/중복을 판단한다.
패널의 user action은 동일 domain command schema를 호출하며 backend로 직접 쓰지 않는다.
TUI는 trace·last scalar·action 최소 기능, web은 plot/calibration/waveform을 우선한다.
M1에 사용자 입력이 빠진 read-only dashboard를 완성품처럼 제출하지 않는다.

### 5.6 안전 등급 레지스트리와 스킬 패키징

R0/R1/R2/R3는 Explore 제안 등급이다. UART/CAN의 일부 R1은 R4도 사용하므로 번호만으로 호환되지 않는다.
장비별 정책은 기본 등급을 올릴 수 있으며 임의 plugin이 위험 동작을 R0로 낮추면 안 된다.
R0은 허용된 비침습 읽기, R1은 제한 설정, R2는 상태·출력 변경, R3는 파괴/복구 곤란 작업이다.
승인은 action hash·resource identity·한계·시간·횟수에 묶고 GUI와 CLI에서 같은 token을 사용한다.
dry-run은 실제 송신 없이 정책 판정과 해석된 명령을 반환한다. 성공할 것이라는 보장은 아니다.
`.claude/skills/loom-scpi`, `loom-dlt`, `loom-ssh`, `loom-debug`, `loom-xcp`를 제안한다.
SKILL.md에는 discover→open→viewer→bounded read→승인된 write→user intervention 확인 순서만 짧게 둔다.
references에는 모델별 command·한계·예시·실패 처리·sim demo를 넣고 SDK를 프롬프트로 설치하지 않는다.
Codex용은 같은 문서 source를 설치 대상 skill path에 패키징하고 사용자 전역 설정을 자동 수정하지 않는다.
비밀정보·로그 내부 지시·원격 출력은 승인 근거가 아니며 target data로 취급하도록 스킬에 명시한다.

## 6. 추천 착수 순서와 이유

기존 세 팀 M1 이전에는 metadata·simulator 사실 확인만 진행한다.
UART/CAN/UDS의 읽기·쓰기·동일 스트림·user intervention이 먼저 작동해야 확장 비용을 측정할 수 있다.
추천 순서는 총점 내림차순과 완전히 같지 않다. DLT는 사용자의 현재 작업 근거가 명확해 SSH보다 먼저다.

| 순서 | 시작 조건 | 할 일 | 종료 기준 | 다음 의존성 |
|---|---|---|---|---|
| E0 | 기존 M1 완료 | core envelope/lease/policy/panel 최소 계약 합의 | 신규 sim backend 하나가 공통 session에 등록 | core·CAN viz·UART console |
| E1 | E0 | SCPI sim PSU 걷는 뼈대 | §4.1 h 다섯 조건 통과 | 실제 PSU 한 모델 |
| E2 | E0 병행 가능 | DLT 3.0.1 V1 제어 M0 gate | native daemon get-info/set-level·효력 확인 | 미통과 시 backend 재선정 |
| E3 | E1, DLT gate | DLT M1 | §4.2 h 통과 | APID/CTID 프로파일 |
| E4 | UART console 계약 | SSH M1 | §4.3 h 통과 | 실제 Linux/QNX 타깃 |
| E5 | memory map·lease 확정 | QEMU debug M1→probe 1종 | §4.4 h 통과 | RTT firmware·driver |
| E6 | XCP slave·쓰기 권한·symbol map | Ethernet XCP M0→M1 | §4.5 h 통과 | DAQ/A2L→CAN transport |
| E7 | 2개 이상 도메인 M1 | bounded scenario job | user abort와 공통 timeline | 별도 core 계획 |

기간은 팀 인원·보유 HW가 미확인이라 확정하지 않는다.
작업 크기 가정으로 PSU sim/SSH는 소형, DLT control/debug backend는 중형, XCP DAQ/A2L은 대형이다.
M0는 라이브러리/driver/프로토콜 gate, M1은 네 요소 전체, M2는 실제 HW 한 모델+부하/복구 검증이다.
멋진 대시보드·광범위 파일 포맷·flash·고속 DAQ는 각 도메인 M1 뒤로 보낸다.

## 7. 사용자에게 물어볼 질문

질문은 후속 계획 확인용이며 현재 조사·파일 작성의 답변 대기 조건이 아니다.
응답이 없을 때는 아래 기본값으로 simulation·벤치 범위를 유지한다.

| 질문 | 답이 없을 때 기본값 | 결정을 바꾸는 이유 |
|---|---|---|
| 보유 PSU/scope/DMM/전자부하 모델과 인터페이스는? | sim PSU + LAN backend, 실제 구매 없음 | SCPI 프로파일·query RTT·driver |
| J-Link/ST-Link/CMSIS-DAP와 MCU 종류는? | QEMU 먼저, physical probe 한 종류만 | vendor SDK·주소 map·RTT 지원 |
| DLT 타깃은 libdlt 3.0.1/V1인가, TCP control을 허용하는가? | 로컬 V1 sim, injection 금지 | native ABI·level control gate |
| 주 사용 환경은 macOS 단독인가 Windows HW host인가? | macOS 개발/시뮬레이션, W HW는 별도 검증 | XL/PLIN driver와 원격 계약 |
| 실차/벤치/개발 보드 중 어디에 연결하는가? | 전원 제한된 벤치, 실차 쓰기 금지 | 위험 등급·승인 범위 |
| XCP slave·A2L·ELF와 RAM calibration 권한이 있는가? | 없다고 보고 XCP simulation만 | 착수 5순위 유지 여부 |
| SSH/ADB 타깃 종류와 허용 명령은? | local SSH sim, sudo/reboot 금지 | 콘솔 backend·인증·PTY |
| LIN/PLIN/Vector FlexRay/CAN XL 장비가 있는가? | 신규 구입 없이 보류 | 전문 HW 비용·현재 가치 |
| relay 접점 실제 feedback과 허용 전압/전류가 있는가? | 물리 접점 확인 불가로 read △ | 읽기 루프 승격 가능성 |
| 로그량·허용 보존 기간·민감정보 정책은? | 로컬 기록, 용량 quota 설정, 비밀정보 제외 | recorder 부하·retention |
| 필요한 live 반응속도와 최대 채널 수는? | scalar 5~20 Hz, UI 20 Hz 목표 | poll/DAQ·event rate·부하 |

## 8. 공통 계약 변경 요청 / 타팀 의존성

이 절은 요청 목록이며 `_xteam`이나 다른 팀 파일을 수정하지 않았다.
현재 `_xteam`은 초안이므로 아래 내용을 이미 합의된 API처럼 사용하지 않는다.

| ID | 대상 | 요청 | 이유·실패 시 처리 |
|---|---|---|---|
| EXP-C1 | core | resource identity+capability+write lease+user takeover | 임의 새 plugin이 숨은 두 번째 연결을 만들지 않게 함 |
| EXP-C2 | core | session/seq/correlation, device clock/uncertainty, quality | 동일 순간·동일 데이터와 지연 검증 |
| EXP-C3 | core | artifact 참조·cursor replay·gap source | scope/DAQ가 IPC 상한을 넘지 않게 함 |
| EXP-C4 | core/UDS | 위험 registry·승인 token·dry-run 공통 schema | 도메인마다 다른 우회 안전 규칙 방지 |
| EXP-C5 | CAN/viz | console/state/calibration/waveform panel 등록 | 새 프로토콜마다 UI를 새로 만들지 않음 |
| EXP-U1 | UART | console bytes·input arbitration 재사용 계약 | SSH/RTT가 COM 포트 코드를 복제하지 않음 |
| EXP-K1 | CAN | XCP frame transport adapter, CAN owner 우회 금지 | 불가하면 XCP Ethernet만 지원 |
| EXP-K2 | CAN | LIN LDF 해석 범위와 CAN XL 조사 귀속 확인 | CAN 팀의 선택 범위를 Explore가 바꾸지 않음 |
| EXP-D1 | UDS | power-cycle 전 pending 진단/TesterPresent 중지 정책 | ECU reset 뒤 옛 세션·승인 재사용 방지 |
| EXP-R1 | core | Windows device host와 macOS viewer의 안전한 원격 접속 별도 ADR | 기본 localhost IPC는 유지, 원격 지원은 미확정 |
| EXP-S1 | core | simulator manifest·deterministic fault profile·late viewer test | 장비 없이 네 요소 CI가 도메인마다 일관됨 |

### 진행 중 R1 초안과의 정합성 확인

- UART [Sol §3/§15](../uart/r1-sol.md), [Opus §3/§15](../uart/r1-opus.md)는 단일 통합 daemon과 도메인 daemon 배치 의견이 다르다.
- Explore는 소유권·stream·policy의 단일 논리 계약만 요구하고 프로세스 배치를 선결 확정하지 않는다.
- UART Sol은 한 viewer의 입력권을 제안하지만 Opus는 여러 viewer 입력을 허용한다. SSH/RTT는 단일 writer lease를 제안하되 core 결정 요청이다.
- CAN [Sol §1/§7.5](../can/r1-sol.md)과 [Opus §1](../can/r1-opus.md)는 LIN runtime을 비범위로 둔다. LDF parser 검토만 접점이다.
- CAN Sol의 §5.4는 raw terminal이 UART 소유라고 명시한다. Explore의 console panel 등록은 이 소유권을 바꾸지 않는다.
- UDS [Sol §15](../uds/r1-sol.md), [Opus §3/§15](../uds/r1-opus.md)는 상주 manager와 CAN ISO-TP 접점을 요구한다.
- CAN ISO-TP 제공 시점과 UDS M1은 초안 간 차이가 있으므로 “기존 세 팀 M1 이후”의 실제 완료 범위는 리드가 먼저 정해야 한다.
- seq의 숫자/문자열, epoch/generation 명명, R0~R3/R4 등급, user 직접 입력 정책도 아직 공통 wire 계약이 아니다.
- §5 인터페이스와 §4 CLI 예시는 이 차이를 해결한 합의가 아니라 Explore의 요구 초안이다.

원격 host가 필요하면 로컬 daemon은 Windows에 있고 macOS는 승인된 tunnel로 같은 stream을 구독한다.
인증·권한·재연결·두 host monotonic clock 차이·renderer ack를 정한 뒤에만 지원한다고 표시한다.
운영망으로 `0.0.0.0` WebSocket을 열거나 두 daemon이 같은 HW를 소유하는 방식은 기본안이 아니다.
공통 event envelope가 아직 확정되지 않아도 M0 codec/백엔드 스파이크는 독립 진행 가능하다.
M1 통합은 같은 session/seq와 user action 계약이 확정된 뒤 수행한다.

## 9. 핵심 원칙 체크리스트 자기점검

아래 [x]는 **계획에 포함됨**을 뜻한다. 제품 구현·실 HW 수용시험 완료 표시가 아니다.
최종 `plan.md`의 맨 앞에는 각 추천의 §4 a 표를 합쳐 별도 “핵심 루프” 절을 넣어야 한다.
현재 산출물은 지시된 독립 `r1-sol.md`이며 최종 plan.md/STATUS를 대신 완료 처리하지 않는다.

| 02-core-principle의 검증 항목 | SCPI | DLT | SSH | Debug | XCP |
|---|---|---|---|---|---|
| [x] 첫 핵심 루프 표에 직접 접근·읽기·쓰기·실시간 표시 정의 | [x] §4.1 a | [x] §4.2 a | [x] §4.3 a | [x] §4.4 a | [x] §4.5 a |
| [x] agent TX→device RX→화면→user 개입→agent 확인 시나리오 | [x] §4.1 b | [x] §4.2 b | [x] §4.3 b | [x] §4.4 b | [x] §4.5 b |
| [x] 지연 목표·고부하·drop/downsampling·기록 한계 명시 | [x] §4.1 c | [x] §4.2 c | [x] §4.3 c | [x] §4.4 c | [x] §4.5 c |
| [x] M1은 simulation 위 네 요소 전체 | [x] §4.1 g/h | [x] §4.2 g/h | [x] §4.3 g/h | [x] §4.4 g/h | [x] §4.5 g/h |
| [x] viewer와 agent가 같은 session, late attach, user intervention | [x] §4.1 e/h | [x] §4.2 e/h | [x] §4.3 e/h | [x] §4.4 e/h | [x] §4.5 e/h |
| [x] 루프 기여 없는 기능은 후순위/비범위 | [x] §2 scope 후속 | [x] §2 파일 분석 | [x] §2 범용 OS 관리 | [x] §4.4 i flash 제외 | [x] §4.5 h/i DAQ·programming 후속 |

추가 조사 자기점검:

- [x] 후보 21개, 네 요소를 각각 평가하고 탈락 규칙을 명시했다(§1/§3).
- [x] 최신 버전·release date·license·Python metadata를 실제 PyPI JSON으로 남겼다(§3).
- [x] GitHub commit·issue activity를 조회했고 REST 실패와 대체 자료를 모두 보존했다(§3).
- [x] 라이브러리의 플랫폼 지원과 HW driver 지원을 분리했다(§3 HW 표).
- [x] 숫자는 실측·vendor 문서·제안 목표·미확인으로 구분했다(§3 타이밍/§4).
- [x] 여섯 backend 왕복/codec 검증과 열네 import 검증의 범위를 기록했다(§0 원자료).
- [x] DLT native control과 XCP DAQ는 검증하지 않았다고 명시했다(§4.2 g/§4.5 g).
- [x] 무손실 기록의 범위와 물리/target drop의 검출 한계를 명시했다(§1/§5.4).
- [x] 다른 팀·dlt-wrapper·사용자 전역 설정은 수정하지 않았다(§2/§8).

<!-- END R1 -->
