# ④ 탐색(Explore) R1 초안 — explore-opus

작성: explore-opus (Claude Opus 5.5), 2026-11-09
기준: `docs/plan/02-core-principle.md`(최우선) → `00-brief.md` §3 → `explore/brief.md` → `explore/tasks/r1-common.md`, `r1-opus.md`
상태: R1 독립 초안. 구현 코드 없음. 라이브러리 버전·날짜·라이선스는 **2026-11-09 PyPI JSON API(`https://pypi.org/pypi/<pkg>/json`) 직접 조회값**이다(표에 "PyPI" 로 표기). 그 밖에 확인하지 못한 것은 "미확인"으로 적는다. 심층 실측(지연·처리량)은 explore-sol 쪽 몫이라 수치 상당수가 "목표치(근거 미확인)"다.

---

## 0. 요약

**결론: 아래 5개 도메인을 추천하고, 시나리오 러너·통합 타임라인·녹화/재생은 새 도메인이 아니라 `loom.core`/`loom.viz` 기능으로 넣는다.**

| 순위 | 도메인 (CLI) | 총점 | 한 줄 이유 |
|---|---|---|---|
| 1 | **DLT 라이브 로그** (`loom dlt`) | 94 | 사용자가 이미 COVESA DLT 3.0.1을 쓴다(`dlt-wrapper`). dlt-daemon TCP(3490)에 붙어 로그를 실시간으로 읽고, **제어 메시지로 로그 레벨을 바꾸고 인젝션을 보내는** 쓰기가 성립. HW 불필요, 시뮬레이션은 실물 dlt-daemon을 macOS에서 그대로 돌리면 됨. |
| 2 | **벤치 전원공급기** (`loom psu`, 패키지 `loom.instr`) | 94 | 모든 벤치 작업의 출발점(전원 사이클, 소비전류·암전류 확인). SCPI는 표준이 단순하고 pyvisa가 성숙. 시나리오 러너의 "1단계"가 이것이다. |
| 3 | **타깃 셸** (`loom shell`: SSH / ADB / telnet / raw TCP) | 92 | 리눅스·QNX·안드로이드 기반 ECU(IVI, 도메인 컨트롤러)의 콘솔과 `journalctl`/`logcat`. UART 팀 콘솔 엔진을 **전송 계층만 바꿔** 재사용 → 비용 최소, 가치 큼. |
| 4 | **디버그 프로브** (`loom dbg`: pyOCD / J-Link / TRACE32 원격) | 88 | 부팅 디버깅의 핵심: 실행 중 변수 라이브 플롯, RTT 로그, halt/step/reset. 가치 최고지만 **위험 등급도 최고**이고, 사용자 MCU가 Cortex-M이 아니면(AURIX/RH850) 백엔드 선택이 바뀜 → 질문 필요. |
| 5 | **릴레이/IO 보드** (`loom io`) | 83 | KL15(점화)/KL30(상시전원)/버튼 신호를 에이전트가 직접 토글. PSU와 함께 "사람 손 없는 벤치"를 완성. 구현은 가장 쉬움. |

**다음 순위(추천 밖, 후보 유지)**: DMM/전자부하(81, PSU와 같은 `loom.instr`로 2단계 흡수), XCP/CCP(79, ECU 측 XCP 슬레이브·A2L 보유 여부에 달림 — 질문 Q6).

**교차 기능 판정**
- 시나리오/테스트 러너 → **`loom.core` 기능** (`loom run`). 데몬 안의 잡(job)으로 돌고, 각 단계는 CLI와 **같은 RPC**를 origin=`script:<run-id>`로 호출. 사용자가 뷰어에서 일시정지/중단/수동 단계 승인. UDS 팀의 "시나리오 YAML"은 이 러너의 UDS 단계로 흡수하자고 **공통 계약 변경 요청**(§8).
- 통합 타임라인 → **`loom.viz` 패널 타입**(레인=도메인/채널). 새 도메인은 이벤트만 내보내면 자동으로 레인이 생김.
- 세션 녹화·재생 → **`loom.core` 기능**. 재생은 "replay 백엔드"로 이벤트를 다시 흘려 모든 뷰어가 그대로 동작, **장비 쓰기는 절대 재현하지 않음**.

**착수 순서**: E0(세 팀 M1과 병행: 플러그인 계약 + SimWorld 명세) → E1 DLT → E2 PSU+IO → E3 시나리오 러너 → E4 타깃 셸 → E5 디버그 프로브(가장 위험·불확실, 단 E0에서 스파이크 선행). 이유는 §6.

---

## 1. 핵심 루프 적합성 기준표

### 1.1 4요소 판정 기준

| 요소 | ○ 성립 | △ 부분 | ✗ 불성립 |
|---|---|---|---|
| **직접 접근** | Python 데몬이 사람 손 없이 장비를 열고, 끊기면 다시 연다. 필요한 것은 드라이버 1회 설치 정도. 벤더 GUI 앱을 띄울 필요 없음. | 벤더 GUI 앱이 떠 있어야 하거나(예: Saleae Logic 2 앱의 Automation 서버), 한 OS에서만 되거나, ctypes 바인딩을 우리가 새로 써야 함. | 실시간 장비가 없음(파일 분석 도구), 또는 사람이 버튼을 눌러야만 연결됨. |
| **읽기** | 경계 있는 명령(`--tail`, `--timeout`, `wait-for`)으로 **해석된 값**(신호, 측정치, 로그 필드)을 얻고, 데몬은 연속 수집할 수 있음. | 스냅샷만 가능(수 초 간격), 원시 바이트만 나오고 해석 불가, 또는 수집 후 후처리가 필요. | 읽을 수 없음. |
| **쓰기** | **DUT 상태를 바꾸는** 입력을 보낼 수 있음: 전압·출력, 프레임, 메모리, 로그 레벨, 키 입력, 릴레이. | 장비 설정만 바뀌고 DUT에는 영향 없음(예: 로직 애널라이저 트리거 arm), 또는 극히 제한된 쓰기. | 쓰기 수단 없음. |
| **사용자 실시간 표시** | 장비 이벤트 → 사용자 화면이 도메인 목표 지연(최대 500 ms) 안. 사용자 뷰가 데몬 스트림을 구독하므로 에이전트와 같은 데이터·출처를 봄. 사용자가 뷰에서 직접 개입 가능. | 갱신이 1 s 이상 간격이거나, 라이브는 벤더 앱 화면에만 있고 Loom 뷰어엔 사후 반영. | 사후 파일로만 볼 수 있음. |

보조 판정: "에이전트 전용 숨은 연결"이 구조적으로 필요한 후보(예: 장비가 단일 클라이언트만 허용해서 데몬이 아닌 에이전트가 직접 열어야 함)는 실시간 표시를 △ 이하로 둔다.

### 1.2 점수화

각 항목을 다음과 같이 매긴 뒤 100점으로 환산한다.

| 항목 | 척도 | 가중치 | 비고 |
|---|---|---|---|
| 사용자 가치 V | 1–5 | 30 | 차량 임베디드 개발자의 하루(벤치 테스트, 부팅 디버깅, 진단, 캘리브레이션)에서 쓰는 빈도 × 시간 절약 |
| 핵심 루프 L | ○=2, △=1, ✗=0 → 4요소 합 0–8 | 30 | 원칙상 가장 중요하므로 가치와 같은 무게 |
| 구현 난이도 E | 1–5 (5=쉬움) | 10 | |
| 라이브러리 성숙도 M | 1–5 | 10 | 최근 릴리스, 1.x 이상, 라이선스 무난 |
| HW 확보 H | 1–5 (5=SW만/저가) | 5 | |
| OS 지원 O | 1–5 (5=macOS+Windows 모두) | 5 | 사용자: 개발 macOS, 실HW Windows 가능성 |
| loom.core 재사용 R | 1–5 | 10 | 데몬/IPC/이벤트/viz/기록을 그대로 쓰는 정도 |

`총점 = 30·V/5 + 30·L/8 + 10·E/5 + 10·M/5 + 5·H/5 + 5·O/5 + 10·R/5`

### 1.3 탈락·제한 규칙 (점수보다 우선)

1. **직접 접근 ✗ 또는 실시간 표시 ✗ → 탈락.** (파일 분석 도구가 여기 걸림)
2. **4요소 중 ✗가 하나라도 있으면 상위 추천 불가.** 보조 도구 / 후순위로만.
3. **△가 2개 이상이면 상위 추천 불가.**
4. **기존 세 팀 범위 안이면 "흡수"** — 점수는 매기되 이 팀 추천에서 제외하고 해당 팀/공통 계약 요청으로 넘김.
5. 라이선스가 배포를 막을 수 있으면(GPL 라이브러리를 데몬에 import) 탈락이 아니라 **리스크로 표시**하고 대안(별도 프로세스, 자체 구현)을 적는다.
6. 동점이면 "사용자 업무 빈도"가 높은 쪽을 위로.

---

## 2. 범위 / 비범위

### 이 팀(④)이 다루는 것
- UART/CAN/UDS 이외의 **장비·프로토콜 도메인 후보 평가**와 상위 5개 간이 계획.
- 새 도메인을 붙이기 위한 **도메인 플러그인 계약 제안**(§5) — 실제 소유는 `loom.core`/`loom.viz`이므로 "공통 계약 변경 요청"으로 제출.
- 교차 기능(시나리오 러너, 통합 타임라인, 녹화/재생)의 **위치 판정과 UX 제안**.
- 새 도메인용 위험 등급과 **벤치 프로파일**(장비 한계값 파일) 제안.

### 기존 세 팀에 남기는 것 (건드리지 않음)
| 항목 | 남기는 팀 | 이 문서의 태도 |
|---|---|---|
| UART 콘솔, TUI 터미널 뷰어, expect | UART | 타깃 셸(`loom shell`)은 UART 콘솔 엔진을 **재사용 요청**만 함 |
| CAN 트레이스/DBC/viz/CANoe 제어/CAN XL/ISO-TP | CAN | CAN XL·CANoe 원격제어는 "CAN 흡수" 판정 |
| UDS 서비스·DoIP·보안접근·플래시 | UDS | UDS 시나리오 YAML은 공통 러너로 통합 **제안**만 |
| `loom.viz` 렌더러 구현 | CAN | 패널 타입 추가 요청(타임라인, 상태 램프, 확인 배너) |

### 비범위 (이 팀이 추천하지 않음, 근거 §3)
- 파일 분석 전용 도구(MF4/BLF/A2L 오프라인 뷰어) — 핵심 루프 불성립.
- FlexRay — HW·라이브러리 부재.
- Modbus를 독립 도메인으로 — 릴레이/계측기의 **전송 계층**으로만 사용.
- 영상(HMI) 캡처 — 후보 유지, 이번 추천 밖.

---

## 3. 후보 전체 평가표

라이브러리 열 형식: `패키지 버전 (최근 릴리스일, 라이선스)` — 모두 PyPI 2026-11-09 조회. 루프 열 순서: 접근/읽기/쓰기/표시.

| # | 후보 | V | 루프 | E | 라이브러리 성숙도 (PyPI 실측) | M | H | macOS·Win | O | R | 기존 3팀 중복 | **총점** |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | **DLT 수신·제어** (dlt-daemon TCP, 제어/인젝션) | 5 | ○○○○ | 4 | pydlt 0.3.5 (2025-03-07, MIT); 자체 코덱 권장. ※ PyPI `python-dlt` 0.2.1은 "now dlt" = dltHub 데이터로더로 **무관**, `dlt` 1.32.0도 무관 | 3 | 5 | ○·○ (TCP) | 5 | 5 | 없음 | **94** |
| 2 | **전원공급기 (SCPI/VISA)** | 5 | ○○○○ | 4 | pyvisa 1.16.2 (2026-02-27, MIT), pyvisa-py 0.8.1 (2025-09-04, MIT), pyvisa-sim 0.7.1 (2025-09-04, MIT) | 5 | 4 | ○(pyvisa-py)·○ | 4 | 4 | 없음 | **94** |
| 3 | **타깃 셸** (SSH/ADB/telnet/raw TCP) | 4 | ○○○○ | 4 | asyncssh 2.24.1 (2026-10-04, EPL-2.0 OR GPL-2.0+), paramiko 5.1.0 (2026-10-18, LGPL-2.1), adbutils 2.12.0 (2025-11-12, MIT), telnetlib3 5.0.2 (2026-11-04, ISC) | 5 | 5 | ○·○ | 5 | 5 | UART 콘솔 모델과 개념 중복 → 엔진 재사용 | **92** |
| 4 | **디버그 프로브** (SWD/JTAG 메모리, RTT, halt, flash) | 5 | ○○○○ | 2 | pyocd 0.46.1 (2026-11-02, Apache-2.0), pylink-square 2.1.0 (2026-11-06, Apache-2.0), lauterbach-trace32-rcl 1.1.6 (2026-07-02, MIT), pyelftools 0.33 (2026-05-29, Public domain), pygdbmi 0.11.0.0 (2023-01-29, MIT — 정체) | 4 | 4 | ○·○ (pyOCD/J-Link) | 4 | 4 | 없음 | **88** |
| 5 | **릴레이/IO 보드** (KL15/KL30/버튼) | 4 | ○△○○ | 5 | pyhid-usb-relay 1.4.0 (2026-09-23, 라이선스 표기 비표준 → 미확인), numato-gpio 0.14.0 (2025-04-29, MIT, **Python <3.14 제한**), pymodbus 3.16.2 (2026-11-03, BSD-3), pyserial 3.5 (2020-11-23, BSD) — 파편화 | 2 | 5 | ○·○ (HID/시리얼) | 4 | 5 | 없음 | **83** |
| 6 | DMM / 전자부하 (SCPI) | 3 | ○○○○ | 4 | pyvisa (위와 동일) | 4 | 3 | ○·○ | 4 | 5 | 없음 | 81 |
| 7 | XCP/CCP 측정·캘리브레이션 | 4 | ○○○○ | 2 | pyxcp 0.30.3 (2026-11-06, LGPLv3, Py 3.11–3.15), pya2ldb 1.0.355 (2026-10-16, **GPLv2**) | 3 | 3 | ○·○ (XCP on Eth); CAN은 CAN 데몬 의존 | 4 | 4 | XCP on CAN은 CAN 채널 공유 | 79 |
| 8 | I2C/SPI/GPIO 어댑터 (FT232H 등) | 2 | ○○○○ | 4 | pyftdi 0.57.2 (2026-06-02, BSD-3), pylibftdi 0.25.0 (2026-11-03, MIT); CH341·Bus Pirate PyPI 패키지 없음(조회 404) | 4 | 5 | ○(libusb)·△(Zadig 드라이버 교체) | 3 | 4 | 없음 | 74 |
| 9 | 오실로스코프 (SCPI 파형) | 3 | ○○○△ | 3 | pyvisa; 벤더별 파형 전송 명령 상이 | 4 | 3 | ○·○ | 4 | 4 | 없음 | 73 |
| 10 | HMI 영상 캡처 + 터치 주입 (UVC 캡처/ADB) | 3 | ○○△○ | 3 | opencv-python 5.0.0.93 (2026-07-02, Apache-2.0), adbutils | 4 | 4 | ○·○ | 4 | 2 | 없음 | 70 |
| 11 | Modbus (TCP/RTU) 일반 | 1 | ○○○○ | 4 | pymodbus 3.16.2 (2026-11-03, BSD-3) | 5 | 3 | ○·○ | 5 | 4 | 없음 | 70 |
| 12 | 차량 이더넷 캡처 / SOME/IP(-SD) | 3 | ○○○△ | 3 | scapy 2.8.0 (2026-10-02, **GPL-2.0-only**), someipy 2.1.2 (2026-01-10, 라이선스 PyPI 미표기), dpkt 1.9.8 (2022-08-18, BSD), pyshark 0.6 (2023-04-26, MIT) | 3 | 3 | △(BPF 권한)·△(Npcap) | 3 | 3 | DoIP는 UDS 팀 | 68 |
| 13 | LIN | 3 | △○○○ | 3 | ldfparser 0.26.0 (2025-01-27, MIT); python-can LIN 지원 없음(미확인 — CAN 팀 확인 요망), PLIN/Vector XL LIN은 ctypes 직접 | 2 | 3 | ✗·○ (사실상 Windows) | 2 | 4 | CAN 팀 버스 모델과 유사 | 67 |
| 14 | CAN XL | 2 | ○○○○ | 3 | python-can 4.6.1 (2025-08-12, LGPL-3.0) — XL 지원 여부 미확인 | 2 | 1 | 미확인 | 2 | 5 | **CAN 팀 흡수** | 65 |
| 15 | 로직 애널라이저 (sigrok/Saleae) | 3 | ○○△△ | 3 | logic2-automation 1.0.11 (2025-11-18, 라이선스 미표기), pysigrok 0.4.2 (2023-02-23, GPLv3+, libsigrok 바인딩이 아닌 순수 Python 재구현), saleae 0.12.0 (2021, 구 Logic 1 API) | 2 | 4 | ○·○ | 3 | 3 | 없음 | 64 |
| 16 | CANoe/CANalyzer 원격 제어 (COM) | 3 | ○○○△ | 3 | py-canoe 26.4.1 (2026-08-30, MIT) | 2 | 2 | ✗·○ | 1 | 3 | **CAN 팀 흡수**(CAN 브리프에 명시) | 63 |
| 17 | 파일 분석 도구 (MF4/BLF/A2L 오프라인) | 3 | ✗○✗✗ | 4 | asammdf 8.8.32 (2026-11-03, LGPLv3+), canmatrix 1.3.3 (2026-10-21, BSD-2) | 4 | 5 | ○·○ | 5 | 4 | CAN viz 파일 입력과 중복 | 60 → **탈락(규칙1)** |
| 18 | FlexRay | 2 | △○△○ | 1 | 사용 가능한 Python 드라이버 없음(Vector XL FlexRay ctypes 직접, 미확인) | 1 | 1 | ✗·△ | 1 | 3 | 없음 | 47 → **탈락** |

교차 기능(도메인이 아니므로 점수 대신 위치 판정):

| 기능 | 판정 | 근거 |
|---|---|---|
| 시나리오/테스트 러너 | `loom.core` (데몬 호스팅 잡) | 모든 도메인 RPC를 같은 경로로 호출해야 숨은 연결이 안 생김. 사용자가 진행을 라이브로 보고 개입하려면 데몬 안에서 상태를 가져야 함. 독립 컴포넌트로 두면 CLI 프로세스 수명에 묶임. |
| 통합 타임라인 | `loom.viz` 패널 타입 | 공통 이벤트 레코드(`ts`, `domain`, `channel`, `origin`)만으로 그릴 수 있음. 새 도메인은 추가 작업 없이 레인이 생겨야 함. |
| 세션 녹화·재생 | `loom.core` | 기록은 공통 계약 §3-6에 이미 있음. 재생은 "replay 백엔드"를 core가 제공하면 도메인마다 따로 만들 필요 없음. |
| 원격 벤치 접속 (macOS → Windows 벤치 PC 데몬) | `loom.core` (SSH 터널, 계약 유지) | 실HW가 Windows에 붙을 가능성이 높음. IPC는 로컬 전용 계약 그대로, `loom --remote bench-pc` 가 SSH 로컬 포워딩을 여는 방식 제안(§8). |

### 3.1 후보별 근거

1. **DLT** — 사용자가 직접 만든 `dlt-wrapper`(COVESA libdlt/dlt-daemon 3.0.1, 포트 3490, 실행 중 레벨을 DLT Viewer·`dlt-control`로 바꾼다고 README에 명시)로 보아 일상 도구. 읽기 = 데몬 TCP 스트림. 쓰기 = 제어 메시지(서비스 ID `0x01 SET_LOG_LEVEL`, `0x03 GET_LOG_INFO`, `0x11 SET_DEFAULT_LOG_LEVEL`, `0x13 GET_SOFTWARE_VERSION`, `0xF04 MARKER`, `0xF08 SET_ALL_LOG_LEVEL`)와 인젝션(`0xFFF` 이상, 앱이 등록한 콜백 호출) — `dlt-wrapper/third_party/dlt/orig/include/dlt/dlt_protocol.h`에서 확인. 라이브러리는 약하지만 프로토콜이 단순(표준 헤더 + 확장 헤더 + verbose 인자)해 자체 코덱이 현실적. 사용자가 이미 DLT Viewer를 쓰므로 Loom 기록을 **표준 `.dlt` 파일**로 내보내 DLT Viewer와 호환.
2. **전원공급기** — `VOLT`, `CURR`, `OUTP ON|OFF`, `MEAS:VOLT?`, `MEAS:CURR?`, `VOLT:PROT` 같은 SCPI 공통 명령으로 대부분 커버. 벤더 차이는 프로파일로 흡수(예: `INST:NSEL` 채널 선택, Korad 계열 비SCPI 시리얼은 별도 드라이버). 시뮬레이션은 pyvisa-sim 또는 자체 가짜 SCPI 서버(TCP 5025).
3. **타깃 셸** — "UART와 같은 콘솔 모델로 통일 가능한가?"에 대한 답: **가능하고, 그래야 한다**. 바이트 스트림 + expect + 사용자 TUI 공유는 UART와 완전히 같고, 다른 것은 전송(SSH 채널, `adb shell`, TCP)과 재연결 의미(로그인 인증)뿐. 단 `telnetlib`는 Python 3.13에서 표준 라이브러리에서 제거됨(PEP 594) → telnetlib3 또는 asyncio 스트림 사용.
4. **디버그 프로브** — Cortex-M은 실행 중에도 AHB-AP로 메모리를 읽을 수 있어 "halt 없이 변수 라이브 플롯"이 성립(pyOCD/J-Link 공통). 단 차량 MCU가 Infineon AURIX(TriCore)나 Renesas RH850이면 pyOCD는 지원하지 않음(pyOCD는 Cortex-M 전용, 공식 설명 "Cortex-M debugger for Python") → Lauterbach TRACE32 원격 API(lauterbach-trace32-rcl, MIT) 또는 벤더 툴 GDB 서버가 백엔드가 됨. 위험 등급이 가장 높은 도메인.
5. **릴레이/IO** — 기능은 단순(채널 on/off, 펄스, 입력 읽기)하지만 KL15/KL30 차단은 DUT에 즉시 영향. 저가 보드는 상태 되읽기가 없는 경우가 많아 읽기 △ (보드에 따라 ○). 라이브러리가 보드별로 파편화 → 드라이버 3종(USB HID, 시리얼 텍스트 프로토콜, Modbus RTU)만 공식 지원.
6. **DMM/전자부하** — 암전류(µA 단위) 측정은 PSU 되읽기 분해능으로 부족한 경우가 많아 가치 있음. PSU와 백엔드 공유 → `loom.instr` 2단계.
7. **XCP/CCP** — 루프는 완벽(DAQ 측정 = 읽기, 캘리브레이션 = 쓰기). 하지만 ECU 개발 빌드에 XCP 슬레이브와 A2L이 있어야 하고, A2L 파서 pya2ldb가 GPLv2라 데몬 import 시 라이선스 검토 필요. 사용자 업무에 캘리브레이션이 있는지 미확인 → 질문 Q6. XCP on CAN은 CAN 데몬의 채널을 공유해야 하므로 ISO-TP와 비슷한 "데몬 내 호스팅" 요청이 필요할 수 있음(DAQ 레이트가 높아 IPC 너머 프레임 단위 처리는 부담).
8. **I2C/SPI/GPIO** — 루프는 성립하지만 ECU 개발자의 일상 빈도가 낮음(센서 브링업·EEPROM 정도). Windows에서 FTDI 드라이버를 libusb로 교체해야 하는 마찰.
9. **오실로스코프** — 파형 전송이 수백 ms~수 s라 실시간 표시 △. 쓰기는 트리거/타임베이스 설정(장비 설정)이지만 "부팅 시 전원 레일 파형 캡처" 같은 시나리오 단계로서 가치. `loom.instr` 3단계.
10. **HMI 영상** — IVI 개발엔 유용. 쓰기(터치 주입)는 안드로이드 계열만 ADB로 가능 → △. 영상은 공통 이벤트 레코드에 맞지 않음(프레임은 별도 저장, 이벤트엔 참조만). 후보 유지.
11. **Modbus** — 차량 개발에서 독립 도메인 가치는 낮음. Modbus RTU 릴레이 보드·전자부하·환경 챔버의 **전송 계층**으로만 쓴다.
12. **차량 이더넷/SOME/IP** — 100BASE-T1 미디어 컨버터가 필요하고(가격 미확인), 캡처는 macOS BPF 권한/Windows Npcap 필요, 트래픽이 많아 디코드 표시가 무거움. scapy GPL-2.0-only. 가치는 있으나 이번 추천 밖. DoIP 자체는 UDS 팀.
13. **LIN** — Python에서 LIN HW 접근이 빈약(python-can은 LIN 비지원으로 알고 있으나 미확인). PLIN-USB/Vector LIN 모두 Windows DLL ctypes → 접근 △, macOS ✗. CAN 팀 버스 모델·트레이스를 재사용할 수 있으므로 CAN 팀 M2 이후 "LIN 백엔드" 확장으로 제안하는 편이 낫다.
14. **CAN XL** — CAN 팀 범위. HW 매우 희소.
15. **로직 애널라이저** — Saleae Logic 2 Automation API는 Logic 2 앱이 떠 있어야 하고(접근 △ 경계) 캡처 후 내보내기 방식이라 라이브 표시 △, DUT 쓰기 없음 → 규칙 3으로 상위 불가.
16. **CANoe 원격 제어** — CAN 브리프에 범위 판단이 이미 위임됨 → 흡수.
17. **파일 분석 도구** — 장비 접근이 없어 규칙 1로 탈락. 필요한 파일 입력은 `loom.viz`가 이미 다룸.
18. **FlexRay** — HW 고가, Python 드라이버 없음, 사용자 맥락에 흔적 없음 → 탈락.

---

## 4. 상위 5개 추천

공통 전제(모든 소절에 적용):
- 장치는 **Loom 데몬 하나**가 소유(계약 §3-2). CLI와 뷰어는 모두 데몬 클라이언트이며, 에이전트가 보낸 모든 쓰기는 이벤트 스트림에 `origin=agent`로 먼저 기록·방송된 다음 장비로 나간다(“기록 → 방송 → 송신” 순서. 송신 실패 시 같은 `corr` id로 실패 이벤트).
- 사용자 개입은 뷰어 → 데몬 RPC(같은 API) → `origin=user`. 에이전트는 `loom <domain> events --origin user --since <cursor>`로 읽을 수 있다.
- 위험 등급 R0–R5는 §5.6의 공통 레지스트리 정의를 쓴다. 장비 한계값은 사용자가 소유한 **벤치 프로파일 파일**(`loom.bench.toml`, §5.7)에 있고, 에이전트는 이 파일을 바꿀 수 없다(데몬이 로드 시 해시 기록, 변경은 사용자 뷰어/편집 후 `loom bench reload` 사용자 확인 필요).
- 지연 수치 중 "목표"는 설계 목표이고, 실측 근거는 아직 없다(Sol 스파이크/구현 M1에서 측정).

### 4.1 DLT 라이브 로그 — `loom dlt`

#### a. 핵심 루프 표

| 요소 | 명령 | 데몬 구성요소 | 사용자 화면 |
|---|---|---|---|
| 직접 접근 | `loom dlt connect ecu1 --tcp 192.168.0.10:3490` (기본 포트 3490), `--serial COM7 --baud 115200`(직렬 DLT, "DLS\x01" 동기 헤더) | `DltTcpBackend` / `DltSerialBackend`: 재연결(지수 백오프 최대 5 s), 연결 상태 이벤트 | 상단 상태줄 `ecu1 ● connected 192.168.0.10:3490  12.3k msg/s` |
| 읽기 | `loom dlt tail`, `loom dlt wait-for`, `loom dlt contexts`, `loom dlt stats` | DLT 코덱(표준 헤더/확장 헤더/verbose 인자 디코드) → 링버퍼(채널당 최근 200k건) → 구독자별 필터 푸시다운 → 기록기(`.dlt` 원본 + JSONL 인덱스) | DLT 트레이스 패널: 시각, ECU 타임스탬프, ECU/APID/CTID, 레벨, 페이로드, **출처 열** |
| 쓰기 | `loom dlt set-level`, `loom dlt inject`, `loom dlt marker`, `loom dlt get-log-info` | 제어 메시지 빌더(서비스 ID 0x01/0x03/0x11/0x13/0xF04/0xF08, 인젝션 ≥0xFFF), 응답 상관(요청 서비스 ID ↔ 응답 상태) | 제어 요청과 응답이 트레이스에 `CTRL` 행으로, origin 색(agent=청록, user=노랑)으로 표시 |
| 실시간 표시 | `loom dlt attach ecu1` (TUI, herdr 패인) / 웹 viz "DLT 트레이스" 패널 | 이벤트 스트림 `dlt.log`, `dlt.ctrl`, `dlt.gap`, `dlt.conn` | 행에서 `L` 키 = 그 컨텍스트 레벨 변경(사용자 개입), `/` 필터, `M` 마커 삽입 |

#### b. 종단 간 시나리오 — "경고가 나는 컨텍스트를 디버그 레벨로 올렸다 내린다"

```
t=0     에이전트: loom dlt tail ecu1 --level warn --tail 20 --json
        → CAM 컨텍스트에서 "frame 812 late" 경고 3건을 읽음
t=+1.0s 에이전트: loom dlt set-level ecu1 --app CAMS --ctx CAM --level debug
        데몬: 이벤트 {kind:"dlt.ctrl", dir:"tx", origin:"agent", payload:{sid:0x01, app:"CAMS", ctx:"CAM", level:"debug"}} 기록·방송 → TCP 송신
        사용자 화면(≤100 ms 목표): 트레이스에 청록색 행  "▶ CTRL set_log_level CAMS/CAM → debug   [agent]"
        dlt-daemon 응답: 상태 OK → 행 끝에 "✓ ok 4 ms"
t=+1.1s ECU: CAM debug 로그 폭증 ("rx ca fe 00 01 ..." 초당 800건)
        사용자 화면(p95 ≤150 ms 목표): 회색 debug 행이 흐름. 상태줄 "12.3k → 13.1k msg/s"
t=+6s   사용자: 너무 많다고 판단, 트레이스에서 CAM 행에 커서 → L → info 선택
        데몬: {kind:"dlt.ctrl", origin:"user", sid:0x01, level:"info"} → 노란 행 "▶ CTRL set_log_level CAMS/CAM → info  [user]"
t=+8s   에이전트: loom dlt events ecu1 --kind ctrl --since @c41 --json
        → [{"origin":"user","sid":"set_log_level","ctx":"CAM","level":"info","ts":"…"}]
        에이전트는 사용자 개입을 인지하고 debug 로그 3초 분량만 분석(`loom dlt tail --ctx CAM --level debug --since -8s --until -2s --max-lines 200`)
```

#### c. 실시간성 목표와 고부하 동작

| 경로 | 목표(근거 미확인) |
|---|---|
| dlt-daemon TCP 수신 → Loom 데몬 디코드 | p95 ≤ 10 ms |
| 디코드 → TUI/웹 화면 | p95 ≤ 150 ms (렌더 배치 주기 50 ms) |
| 제어 메시지 송신 → 화면 `CTRL` 행 | ≤ 100 ms (송신 전 방송하므로 장비 응답과 무관) |

고부하(예: 2만 msg/s, 목표치 — 실제 상한은 미확인):
- **기록은 무손실**: 수신 바이트를 그대로 `.dlt`(storage header 포함 표준 형식)로 먼저 쓴다. 디코드·방송보다 앞.
- **화면은 정직하게 생략**: 뷰어는 초당 렌더 상한(TUI 2k행/s, 웹 5k행/s 목표)을 넘으면 묶어서 `⋯ 표시 생략 12,345건 (기록됨, 필터를 좁히세요) ⋯` 행을 넣는다. 스크롤 고정 모드에서는 생략 없이 링버퍼에서 페이지로 읽는다.
- **필터 푸시다운**: 구독자별 필터(APID/CTID/레벨/정규식)를 데몬이 적용 → 사용자가 필터를 좁히면 생략이 사라짐.
- **상류 손실 표시**: dlt-daemon이 `0x14 MESSAGE_BUFFER_OVERFLOW`를 보내거나 메시지 카운터(표준 헤더 MCNT) 불연속이 감지되면 빨간 `GAP` 행 + 이벤트 `dlt.gap {lost_est}`. "Loom이 못 받았다"와 "ECU가 못 보냈다"를 구분해 표시.

#### d. CLI 초안

```
loom dlt connect <name> (--tcp HOST[:PORT] | --serial PORT --baud N) [--ecu-id ID]
loom dlt disconnect <name>
loom dlt status [<name>] [--json]
loom dlt tail <name> [--app A] [--ctx C] [--level LVL] [--grep RE] [--tail N=50] [--since T] [--until T] [--max-lines N=200] [--json]
loom dlt wait-for <name> --grep RE [--app A] [--ctx C] [--level LVL] --timeout 30s [--json]
loom dlt contexts <name> [--refresh]            # GET_LOG_INFO(0x03) 결과 표
loom dlt set-level <name> --app A [--ctx C] --level off|fatal|error|warn|info|debug|verbose|default
loom dlt set-level <name> --all --level L       # 0xF08, R2
loom dlt inject <name> --app A --ctx C --sid 0x1000 --data 'hex:0102' | --text "..."
loom dlt marker <name> [--label TEXT]            # 0xF04, 타임라인 정렬용
loom dlt events <name> --kind ctrl|gap|conn [--origin agent|user|script] [--since CURSOR] [--json]
loom dlt stats <name> [--by app|ctx|level] [--window 10s]
loom dlt export <name> --since T --until T --out file.dlt
```

출력 예시(사람용, 상한 200줄, 넘으면 마지막 줄에 안내):
```
$ loom dlt tail ecu1 --level warn --tail 3
14:02:11.482  +812.3021s  ECU1 CAMS CAM  warn  frame 812 late (lat=41.20ms)        [device]
14:02:11.997  +812.8170s  ECU1 CAMS NET  warn  connect 10.0.0.2:13400 failed      [device]
14:02:12.103  +812.9234s  ECU1 CAMS CAM  warn  frame 813 late (lat=39.75ms)        [device]
(3 lines, cursor=@c41  · next: --since @c41)
```
`--json`: 한 줄에 하나, `{"seq":..., "ts":{"mono_ns":...,"wall":"..."}, "domain":"dlt", "channel":"ecu1", "dir":"rx", "origin":"device", "kind":"dlt.log", "payload":{"ecu":"ECU1","apid":"CAMS","ctid":"CAM","level":"warn","tmsp_01ms":8123021,"mcnt":17,"args":["frame 812 late (lat=41.20ms)"],"text":"frame 812 late (lat=41.20ms)"}}` + 마지막 줄 `{"cursor":"@c41","truncated":false}`.

#### e. 사용자 뷰

```
┌ loom dlt attach ecu1 ─────────────────────────────── ● 192.168.0.10:3490  13.1k msg/s  rec ● ┐
│ filter: level>=info  app=*  ctx=*                                  [F]ilter [L]evel [M]arker │
├──────────────┬────────────┬──────┬──────┬───────┬──────────────────────────────────┬────────┤
│ host time    │ ECU tmsp   │ APID │ CTID │ level │ payload                          │ origin │
│ 14:02:12.103 │ +812.9234  │ CAMS │ CAM  │ warn  │ frame 813 late (lat=39.75ms)     │ device │
│ 14:02:13.000 │            │      │      │ CTRL  │ ▶ set_log_level CAMS/CAM→debug ✓ │ agent  │
│ 14:02:13.004 │ +813.8211  │ CAMS │ CAM  │ debug │ rx ca fe 00 01                   │ device │
│ ⋯ 표시 생략 3,812건 (기록됨) ⋯                                                                 │
│ 14:02:19.020 │            │      │      │ CTRL  │ ▶ set_log_level CAMS/CAM→info ✓  │ user   │
└──────────────┴────────────┴──────┴──────┴───────┴──────────────────────────────────┴────────┘
```
웹 패널은 같은 열 + 레벨별 메시지율 미니 플롯(viz 시계열) + 컨텍스트 트리(체크박스=필터, 우클릭=레벨 변경).

#### f. 주 라이브러리

| 용도 | 선택 | 근거 |
|---|---|---|
| 코덱 | **자체 구현**(표준/확장 헤더, verbose 인자 타입 BOOL/SINT/UINT/FLOA/STRG/RAWD, 제어 메시지) | 프로토콜이 작고, 대안 pydlt 0.3.5(MIT, 2025-03-07)는 0.x·단일 유지자(추정). pydlt는 테스트 교차검증용 dev 의존성으로만. |
| 시뮬레이터 | COVESA dlt-daemon 3.0.1 (사용자 `dlt-wrapper`가 macOS arm64에서 빌드 확인, `scripts/build_dlt.sh`) | 실물 데몬과 같은 동작. 라이선스 MPL-2.0으로 알고 있음(미확인) — 별도 프로세스로만 쓰므로 영향 없음. |
| 비verbose 모드 | 비범위(M3 후보) | FIBEX/ARXML 필요. 질문 Q4. |

#### g. 시뮬레이션 백엔드
1. **CI용 순수 Python `SimDltServer`**: TCP 서버로 verbose 메시지를 생성(SimWorld의 가상 DUT 상태에 따라 부팅 로그 → 주기 로그), `SET_LOG_LEVEL` 수신 시 내부 레벨을 바꿔 **쓰기 효과가 읽기에 보이게** 함, `GET_LOG_INFO` 응답, 인젝션 수신 시 "injection 0x1000 received" 로그 송출.
2. **통합용 실물**: dlt-daemon 3.0.1 + `dltw` 예제 앱(`DLT_PIPE_DIR`, 임시 포트 34910 — dlt-wrapper README와 같은 방식). 레벨 변경·인젝션 콜백의 실제 동작을 검증.

#### h. M1 걷는 뼈대 (SimDltServer 위)
- `loom dlt connect sim1 --tcp 127.0.0.1:<port>` → `tail`/`wait-for`가 경계 있게 동작(타임아웃·상한).
- `set-level` 송신 → 다음 로그부터 레벨 변화가 읽기 결과에 반영.
- `loom dlt attach sim1` TUI에 device 로그 + agent `CTRL` 행이 보이고, TUI에서 `L`로 사용자 레벨 변경 → `loom dlt events --origin user`로 에이전트가 읽음.
- 수용 기준: CI 자동 테스트 4개(접근/읽기/쓰기/표시+개입), 1k msg/s에서 화면 지연 p95 ≤ 150 ms 측정 로그, 1만 msg/s 주입 시 `.dlt` 기록 건수 = 송신 건수(무손실)와 생략 행 표시 확인.

#### i. 안전/가드레일
| 동작 | 등급 | 규칙 |
|---|---|---|
| tail/wait-for/contexts/stats | R0 | 자동 |
| set-level 단일 컨텍스트, marker | R1 | 자동, 감사 로그. 레이트 ≤ 5 req/s |
| set-level `--all` 또는 verbose 전체 | R2 | 자동 허용하되 화면 배너 + 30 s 후 원복 제안. ECU 대역폭 폭주 경고 |
| inject (벤치 프로파일 화이트리스트 SID) | R2 | 자동, 페이로드 hex 기록 |
| inject (화이트리스트 밖) | R3 | 사용자 확인(인젝션 의미는 앱이 정의하므로 미지 SID는 위험) |
| STORE_CONFIG(0x05), RESET_TO_FACTORY_DEFAULT(0x06), SET_COM_INTERFACE_*(0x07/0x08) | R3 | 사용자 확인 + `--i-know-persistent` (영속 변경·통신 차단 가능) |

#### j. 리스크
- 양산/통합 ECU는 비verbose DLT가 흔함 → FIBEX 없이 페이로드 해석 불가(원시 hex로만 표시).
- dlt-daemon이 다중 TCP 클라이언트를 받는지(DLT Viewer와 Loom 동시 접속) — 받는 것으로 알고 있으나 **미확인**. 안 되면 Loom이 단일 소유자가 되고 DLT Viewer는 Loom의 재송출 포트에 붙게 해야 함(§8 요청).
- ECU 타임스탬프(0.1 ms 틱, ECU 기동 기준)와 호스트 시각 정렬: 둘 다 저장하고 타임라인은 호스트 수신 시각 기준, 오프셋 추정은 M2.

### 4.2 벤치 전원공급기 — `loom psu` (패키지 `loom.instr`)

#### a. 핵심 루프 표

| 요소 | 명령 | 데몬 구성요소 | 사용자 화면 |
|---|---|---|---|
| 직접 접근 | `loom psu connect bench --visa "TCPIP::192.168.0.50::5025::SOCKET"` 또는 `--visa "USB0::0x1AB1::…::INSTR"`, `--serial COM9 --driver korad` | `VisaBackend`(pyvisa + pyvisa-py 기본, NI-VISA 있으면 선택), 드라이버 프로파일(SCPI 방언), `*IDN?` 검증, 재연결 | 상태줄 `bench ● RIGOL DP832 (IDN 확인) ch1` |
| 읽기 | `loom psu meas`, `loom psu watch --for 10s`, `loom psu wait-for --curr-lt 0.005 --hold 2s --timeout 120s` | 폴러(기본 10 Hz, `MEAS:VOLT?`/`MEAS:CURR?`), 출력·보호 상태 폴링(1 Hz) | 큰 숫자 V/A/W, 출력 램프, 전류 시계열 플롯(viz) |
| 쓰기 | `loom psu set --volt 13.5 --curr 3.0`, `loom psu out on|off`, `loom psu cycle --off 2s` | 명령 게이트(벤치 프로파일 한계 클램프 → 위험 등급 → 확인), SCPI 송신 후 되읽기 검증 | 쓰기 행이 이벤트 목록과 플롯 마커(▲agent ▲user)로 표시 |
| 실시간 표시 | `loom psu attach bench` (TUI) / 웹 "계기판" 패널 | 이벤트 `psu.meas`, `psu.state`, `psu.cmd`, `psu.prot`(OVP/OCP 트립) | 키: `o` 출력 토글(확인), `v`/`c` 설정 입력, `Esc` 비상 OFF(확인 없음) |

#### b. 종단 간 시나리오 — "DUT 전원 인가 후 이상 전류를 사용자가 끊는다"

```
t=0     에이전트: loom psu set bench --volt 13.5 --curr 3.0
        데몬: 프로파일(vmax 16.0, imax 5.0) 이내 → R2 자동 → {kind:"psu.cmd", origin:"agent", VOLT 13.5, CURR 3.0} 방송
        → SCPI "VOLT 13.5;CURR 3.0" → 되읽기 "VOLT? 13.500" 일치 → ✓
        화면(≤300 ms 목표): 설정값 13.50 V / 3.00 A, 이벤트 줄 "agent set 13.5V 3.0A ✓"
t=+2s   에이전트: loom psu out bench on        (R2: 프로파일 허용 시 자동, 아니면 확인)
        화면: 출력 램프 ● ON(agent), 전류 플롯이 0 → 0.35 A (10 Hz 샘플, 실측 주기 표시 "Δ 101 ms")
t=+3s   에이전트: loom psu wait-for bench --curr-between 0.2,0.6 --hold 3s --timeout 20s
t=+5s   전류가 1.8 A로 상승(DUT 이상). 사용자 화면: 플롯이 주황 경고선(프로파일 warn_curr 1.5 A) 넘음, 배너 "전류 1.80 A > warn 1.50 A"
t=+5.4s 사용자: Esc (비상 OFF, 확인 없이 즉시)
        데몬: {kind:"psu.cmd", origin:"user", OUTP OFF} → 램프 ○ OFF(user)
t=+5.5s 에이전트의 wait-for 반환(exit 3):
        "aborted: output turned OFF by user at 14:10:05.412 (last 1.80 A)"
        --json: {"result":"interrupted","by":{"origin":"user","kind":"psu.cmd","cmd":"out off"},"last":{"v":13.49,"i":1.80}}
```

#### c. 실시간성 목표와 고부하 동작
- 측정 → 화면 p95 ≤ 300 ms(폴링 10 Hz 기준). 상태 변경(출력 ON/OFF, 보호 트립) → 화면 ≤ 500 ms(1 Hz 상태 폴링 + 쓰기 직후 즉시 폴링).
- SCPI 왕복 시간은 장비·연결(LAN/USBTMC/시리얼)마다 다름 — 수치 미확인(Sol 조사). 데몬은 **실측 주기와 지터를 항상 화면에 표시**하고, 장비가 느리면 폴링 주기를 자동으로 늘리며 "요청 10 Hz → 실제 4.2 Hz"를 명시.
- 응답 타임아웃 샘플은 보간하지 않고 플롯에 **끊김(gap)** 으로 그림.
- 정직성 문구: PSU 되읽기는 장비 내부 평균(수십~수백 ms)이라 돌입전류·ms 단위 리셋 원인은 잡을 수 없다 → 패널에 "되읽기 해상도 ≈ 100 ms, 과도현상은 스코프/DMM 필요" 고정 표기.
- 기록: 모든 샘플과 명령을 JSONL로 무손실, CSV export.

#### d. CLI 초안

```
loom psu connect <name> (--visa RESOURCE | --serial PORT --driver korad|…) [--channel 1] [--profile bench.toml]
loom psu status <name> [--json]                       # IDN, 설정값, 출력, 보호 상태, 폴링 실측 주기
loom psu meas <name> [--n 1] [--json]
loom psu watch <name> --for 10s [--rate 10Hz] [--summary] [--json]   # 기본 요약(min/max/mean/마지막), --raw 시 최대 500샘플
loom psu wait-for <name> (--curr-lt A | --curr-gt A | --curr-between A,B | --volt-…) [--hold 2s] --timeout 60s
loom psu set <name> [--volt V] [--curr A] [--ovp V] [--ocp A]
loom psu out <name> on|off
loom psu cycle <name> --off 2s [--on-delay 0]
loom psu events <name> [--origin …] [--since CURSOR] [--json]
```

예시:
```
$ loom psu watch bench --for 5s
bench ch1  13.49 V  (set 13.50)  output ON(agent 14:10:02)
current  min 0.312  max 0.388  mean 0.351 A   samples 49/50 (1 timeout)  rate 9.8 Hz
```

#### e. 사용자 뷰

```
┌ loom psu attach bench ── RIGOL DP832 ch1 ─ poll 9.8 Hz (Δ101 ms) ─ rec ● ───────────┐
│   SET 13.50 V  3.00 A         MEAS  13.49 V   0.351 A   4.73 W     OUTPUT ● ON (agent) │
│   OVP 16.0 V  OCP 5.0 A       profile: vmax 16.0  imax 5.0  warn_curr 1.5            │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ A 2.0┤                                    ╭──── warn 1.5 ───────────────            │
│   1.0┤                 ▲agent on                                                    │
│   0.0┼─────────────────┴──╯‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾                                    │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ 14:10:00.102 agent set 13.5V 3.0A ✓   14:10:02.010 agent out on ✓                   │
│ [o] output  [v] volt  [c] curr  [Esc] 비상 OFF  [p] 프로파일 보기                     │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

#### f. 주 라이브러리
- pyvisa 1.16.2 (MIT, 2026-02-27, Py ≥3.10), pyvisa-py 0.8.1 (MIT, 2025-09-04) — TCPIP SOCKET/INSTR, USBTMC(pyusb 1.3.1 + libusb 필요; libusb-package 1.0.30.1(Apache-2.0)로 번들 가능), 시리얼. 근거: PyPI.
- NI-VISA는 선택(Windows에서 이미 깔려 있으면 사용). macOS NI-VISA 지원 현황 미확인 → macOS 기본은 pyvisa-py.
- 비SCPI 저가 PSU(Korad/RD6006 등)는 pyserial/pymodbus 드라이버 — M2 이후, 보유 장비 답변(Q1) 보고 결정.

#### g. 시뮬레이션 백엔드
- **`SimPsu`(in-process)**: 설정값·출력·보호 상태를 가진 상태기계 + **SimWorld 부하 모델**(DUT 상태: off 0 A / boot 0.6 A / run 0.35 A / sleep 0.004 A, 잡음). 출력 OFF → SimWorld DUT 전원 끊김 → SimDlt·UART sim·CAN sim이 함께 멈춤(교차 도메인 시나리오 테스트의 기반).
- **`SimScpiServer`(TCP 5025)**: 같은 상태기계를 SCPI 텍스트로 노출 → pyvisa-py `TCPIP::127.0.0.1::5025::SOCKET` 경로까지 CI에서 검증.
- pyvisa-sim 0.7.1: YAML 기반이라 동적 부하 모델 표현이 제한적(속성 get/set은 가능한 것으로 앎, 미확인) → 드라이버 방언 단위 테스트용 보조.

#### h. M1 걷는 뼈대 (SimScpiServer 위)
- connect(IDN 확인) → meas/watch/wait-for(경계 있음) → set/out(프로파일 클램프, 되읽기 검증) → TUI attach에 계기판+플롯+agent 표시 → 사용자 Esc OFF → 에이전트 wait-for가 `interrupted by user`로 반환.
- 수용 기준: 위 시나리오 자동 테스트, 프로파일 초과 `set --volt 30` 거부(exit 4, 사유 출력), 응답 지연 주입 시 화면에 실측 주기·gap 표시.

#### i. 안전/가드레일
| 동작 | 등급 | 규칙 |
|---|---|---|
| meas/watch/status | R0 | |
| set (프로파일 한계 이내) | R2 | 자동. 한계 초과는 **어떤 확인으로도 불가**(프로파일 변경은 사용자만) |
| out on | R2 | 프로파일 `agent_power_on = true`일 때만 자동, 기본은 R3(확인) |
| out off / cycle | R3 | 확인 필요. 단 **인터록**: UDS 플래시 세션·디버그 프로브 flash 진행 중이면 R4로 승격(확인 토큰 + 사유) |
| set --ovp/--ocp | R3 | 프로파일 범위 안에서만 |
| 사용자 Esc 비상 OFF | — | 사용자 origin, 확인 없음(안전 방향 동작은 막지 않음) |
- 레이트 제한: 출력 토글 ≤ 1회/s(릴레이·DUT 보호). dry-run: `--dry-run`은 SCPI 문자열과 판정만 출력.

#### j. 리스크
- SCPI 방언(채널 선택, 응답 형식) 차이 → 장비별 드라이버 프로파일 필요. 보유 장비 목록(Q1)이 없으면 범용 SCPI로 시작.
- USBTMC on macOS: libusb 접근 권한/드라이버 충돌 가능(미확인). LAN 연결을 기본 권장.
- 차량 12 V 벤치는 대전류(수십 A) PSU를 쓰기도 함 → 프로파일 한계 필수.

### 4.3 타깃 셸 — `loom shell` (SSH / ADB / telnet / raw TCP)

#### a. 핵심 루프 표

| 요소 | 명령 | 데몬 구성요소 | 사용자 화면 |
|---|---|---|---|
| 직접 접근 | `loom shell open ivi --ssh root@192.168.0.20 [--key ~/.ssh/id_ed25519]`, `--adb <serial>`, `--telnet host:23`, `--tcp host:port` | **UART 팀 콘솔 엔진**(바이트 버퍼·커서·expect·출처 태깅) + 전송 어댑터(`SshTransport` asyncssh PTY, `AdbTransport`, `TcpTransport`) | 상태줄 `ivi ● ssh root@192.168.0.20 (pty 120x40)` |
| 읽기 | `loom shell read ivi --tail 50 / --since @cur`, `loom shell expect ivi 'login:|# $' --timeout 20s`, 로그 채널: `loom shell log open ivi-j --from ivi --cmd "journalctl -f -o short-monotonic"` | 콘솔 링버퍼 + 로그 채널(별도 채널, 같은 이벤트 스트림) | UART와 동일한 TUI 터미널 + 로그 채널 탭 |
| 쓰기 | `loom shell send ivi 'systemctl restart camd\n'`, `loom shell exec ivi "cat /proc/meminfo" --timeout 5s` | `exec`는 **공유 PTY 안에서** 센티넬로 감싸 실행(`cmd; echo "__LOOM_RC_42=$?"`) — 사용자가 같은 화면에서 봄 | 에이전트 입력 강조색 + `[agent]` 여백 표시 |
| 실시간 표시 | `loom shell attach ivi` (UART `attach`와 같은 TUI 바이너리) | 이벤트 `shell.rx`, `shell.tx`, `shell.conn` | 사용자가 그대로 타이핑(입력 규칙은 UART 팀 결정을 따름) |

숨은 연결 금지 규칙: `exec`를 별도 SSH exec 채널로 돌리면 사용자에게 안 보이므로, **기본은 공유 PTY**. 출력이 큰 명령용 `--side-channel` 옵션은 허용하되, 그 채널도 `ivi/exec#N` 이름으로 이벤트 스트림에 올라가 뷰어 탭에 자동으로 뜬다.

#### b. 종단 간 시나리오 — "서비스 재시작 후 로그 확인, 사용자가 중간에 끼어든다"

```
에이전트: loom shell exec ivi "systemctl restart camd" --timeout 10s
  화면(≤100 ms 목표): 터미널에 청록색 "# systemctl restart camd" [agent] 표시, 프롬프트 복귀 → rc=0
에이전트: loom shell expect ivi-j 'camd.*(started|failed)' --timeout 15s
  journal 채널 탭: "camd[912]: failed to open /dev/video2" (device) → expect 매치 반환
사용자: 터미널 탭에서 직접 "ls -l /dev/video*" 입력 (노란 [user])
에이전트: loom shell read ivi --since @c88 --origin all
  → user가 입력한 명령과 출력(video0, video1만 존재)을 읽고, 장치 노드 문제로 결론
```

#### c. 실시간성 목표
- 수신 바이트 → 화면 p95 ≤ 100 ms(UART와 동일 목표). SSH 자체 왕복은 네트워크에 의존(LAN 수 ms, 미확인).
- 고부하(logcat/journal 폭주): UART 팀 정책 그대로(기록 무손실, 화면 렌더 상한 + 생략 표시). 로그 채널은 콘솔과 분리되어 있어 콘솔 상호작용을 막지 않음.

#### d. CLI 초안
UART CLI와 **동사 동일**(`open/close/status/send/read/expect/attach/events`) + 셸 전용 `exec`, `log open|close`, `push/pull`(파일 전송, M2, R2/R3).
```
$ loom shell exec ivi "uptime" --timeout 5s
 14:21:07 up 3 min,  load average: 1.12, 0.80, 0.33
(rc=0, 0.21 s, ran in shared pty ivi)
```

#### e. 사용자 뷰
UART 뷰어 재사용. 탭: `ivi (ssh)` | `ivi-j (journalctl)` | `ivi/exec#3`. 상단에 재연결 상태, 인증 방식 표시.

#### f. 주 라이브러리
- asyncssh 2.24.1 (EPL-2.0 OR GPL-2.0-or-later, 2026-10-04, Py ≥3.10) — asyncio 네이티브, 서버 기능 포함(시뮬레이터에 유리). paramiko 5.1.0(LGPL-2.1)은 대안.
- adbutils 2.12.0 (MIT, 2025-11-12) — adb 서버 프로토콜 클라이언트. `adb` 바이너리/서버 필요(미확인: adbutils 단독 동작 범위).
- telnetlib3 5.0.2 (ISC, 2026-11-04) — 표준 `telnetlib`는 Python 3.13에서 제거(PEP 594).

#### g. 시뮬레이션 백엔드
- asyncssh 서버로 `SimSshShell` — UART 팀의 가짜 디바이스 스크립트(부팅 로그, 프롬프트)를 그대로 셸로 노출. `journalctl -f` 흉내 명령 포함.
- raw TCP 에코/셸 서버(가장 단순, M1 첫 테스트).
- ADB 시뮬레이션은 어려움 → M2에 실기기 또는 에뮬레이터(Android emulator)로 수동 검증.

#### h. M1 걷는 뼈대
- 전제: UART 팀 콘솔 엔진이 전송 추상화(`Transport` 인터페이스)를 노출(§8 요청). 
- SimSshShell 위에서 open → exec(공유 PTY, rc 회수) → expect → attach TUI에 agent/user/device 구분 → 사용자 입력을 에이전트가 `--origin user`로 읽음.
- 수용: CI 테스트, 재연결(서버 재시작) 후 상태 이벤트, 출력 상한 동작.

#### i. 안전/가드레일
| 동작 | 등급 |
|---|---|
| read/expect/status | R0 |
| send/exec 일반 명령 | R1 |
| 위험 패턴 명령(`reboot`, `poweroff`, `rm -rf`, `dd of=`, `mkfs`, `flash_erase`, `fw_setenv`, `mount -o remount,rw`, `echo … > /sys/…`) | R3 확인 |
| push(파일 쓰기) | R2, 시스템 경로(`/`, `/etc`, `/usr`, `/boot`)는 R3 |
- 패턴 분류는 최선 노력(셸은 우회 가능) → 스킬 문서에 "의도적 우회 금지"를 규칙으로 명시하고, 벤치 프로파일에서 대상별 `shell_policy = "readonly"|"normal"` 지정. `readonly`면 send 자체를 R3로.
- 자격증명: 비밀번호는 CLI 인자로 받지 않음(키/ssh-agent/OS 키체인). 로그에 비밀번호 프롬프트 응답 마스킹.

#### j. 리스크
- 전체화면 프로그램(vi, top)이 공유 PTY를 점유 → 에이전트 exec 실패. 규칙: exec 전에 프롬프트 상태 확인, 아니면 `--side-channel` 제안.
- UART 팀 콘솔 엔진이 전송 추상화를 안 하면 재사용 불가 → 코드 중복.
- QNX/임베디드 리눅스의 sshd 유무 다양.

### 4.4 디버그 프로브 — `loom dbg` (pyOCD / J-Link / TRACE32 원격)

#### a. 핵심 루프 표

| 요소 | 명령 | 데몬 구성요소 | 사용자 화면 |
|---|---|---|---|
| 직접 접근 | `loom dbg connect mcu --probe pyocd:<uid> --target stm32h743 --elf build/app.elf` (기본 **attach, halt·reset 없음**), `--probe jlink:<sn> --device S32K344`, `--probe t32:localhost:20000` | `ProbeBackend` (pyOCD / pylink / TRACE32 RCL / OpenOCD TCL) + `SymbolTable`(pyelftools로 ELF/DWARF → 이름·주소·타입·크기) | 상태줄 `mcu ● RUNNING  J-Link 4000 kHz  app.elf (sha 3f2a…)` |
| 읽기 | `loom dbg read mcu g_state.speed`, `loom dbg mem read mcu 0x20000000 64`, `loom dbg watch mcu g_boot_stage,g_err_cnt --rate 50Hz --for 5s`, `loom dbg rtt read mcu --tail 50` | 워치 샘플러(실행 중 비침투 메모리 읽기), RTT 폴러(10 ms), 코어 상태 폴러(100 ms) | 라이브 워치 표+플롯, RTT 콘솔, 코어 상태 램프 |
| 쓰기 | `loom dbg write mcu g_cfg.mode 2`, `loom dbg rtt send mcu "help\n"`, `halt/resume/step/reset`, `flash` | 위험 게이트(심볼 화이트리스트, 메모리 영역 맵), 쓰기 후 되읽기 검증 | 쓰기 이벤트 표시, halt 시 레지스터/콜스택 패널 자동 표시 |
| 실시간 표시 | `loom dbg attach mcu` (TUI) / 웹 "Watch + RTT" 패널 | 이벤트 `dbg.sample`, `dbg.rtt`, `dbg.core`, `dbg.cmd` | 키: `h` halt(확인), `r` resume, `w` 변수 추가, `e` 값 편집(확인) |

#### b. 종단 간 시나리오 — "부팅이 특정 단계에서 멈춘다"

```
에이전트: loom dbg connect mcu --probe jlink:600112345 --device STM32H743ZI --elf build/app.elf
         → RUNNING, attach만(코어 영향 없음)
에이전트: loom dbg watch mcu g_boot_stage,g_can_init_err --rate 20Hz --for 3s --summary
         화면(샘플→표시 ≤250 ms 목표): 플롯 g_boot_stage가 4에서 평평, g_can_init_err=0x11
에이전트: loom dbg rtt read mcu --tail 20
         → "[boot] stage 4: can init ... timeout waiting for PLL2"
에이전트: loom dbg halt mcu --reason "inspect PLL2 config"
         데몬: R3 → 확인 대기 이벤트. 사용자 화면에 배너
           "에이전트가 코어 HALT를 요청: inspect PLL2 config  [y] 허용  [n] 거절  (60 s 후 자동 거절)"
         CLI 출력: "waiting for user confirmation (id cf-17)…"
사용자: y  → {origin:"user", kind:"dbg.confirm", id:"cf-17", answer:"allow"}
         코어 HALTED, 화면에 레지스터·콜스택(pc= pll_wait_lock+0x1c) 자동 표시, 플롯에 회색 "HALTED" 구간
에이전트: loom dbg mem read mcu RCC.PLLCFGR --fmt fields   (SVD 있으면 필드 해석, M2)
사용자: 원인 확인 후 뷰어에서 r (resume)  → origin=user resume
에이전트: loom dbg events mcu --kind core --since @c9 → resume by user 확인, 이후 분석 계속
```

#### c. 실시간성 목표와 고부하 동작
- 워치 샘플 → 화면 p95 ≤ 250 ms, RTT 텍스트 → 화면 ≤ 100 ms(폴링 10 ms 가정).
- 달성 가능 샘플링 레이트는 프로브·SWD 클럭·변수 수에 달림 — 근거 수치 미확인(Sol). 목표: J-Link에서 변수 8개 100 Hz, CMSIS-DAP/ST-Link에서 20 Hz(추정).
- 정직성: (1) 실측 샘플 주기·지터 표시, (2) 요청 레이트를 못 맞추면 "요청 100 Hz → 실제 37 Hz", (3) 다중 워드 변수는 찢긴 읽기 가능성 표시(`⚠ non-atomic 64-bit`), (4) HALTED 동안 선을 잇지 않고 회색 구간, (5) RTT 버퍼 오버플로(타깃 쪽 모드에 따라 드롭)는 감지 가능한 범위에서 `RTT overflow?` 표시(정확한 감지 가능 여부 미확인).

#### d. CLI 초안
```
loom dbg connect <name> --probe (pyocd:UID|jlink:SN|t32:HOST:PORT|openocd:HOST:PORT) [--target T|--device D] [--elf F] [--svd F] [--speed 4000kHz]
loom dbg status <name> [--json]                     # 코어 상태, 프로브, ELF 해시, 활성 워치
loom dbg symbols <name> --grep RE [--max 50]
loom dbg read <name> <expr>                          # 심볼, 구조체 필드, 배열 인덱스, 주소
loom dbg mem read <name> <addr|sym> <len> [--fmt hex|u32|fields]
loom dbg watch <name> <expr,…> --rate 20Hz --for 5s [--summary|--raw --max 2000] [--json]
loom dbg rtt read <name> [--chan 0] [--tail N] [--since CUR] [--grep RE] [--timeout]
loom dbg rtt send <name> [--chan 0] TEXT
loom dbg rtt wait-for <name> --grep RE --timeout 30s
loom dbg write <name> <expr> <value>                 # R2/R3
loom dbg halt|resume|step|reset <name> [--reason TEXT]   # R3
loom dbg flash <name> <image.hex|elf> [--verify] [--dry-run]  # R4
loom dbg events <name> [--kind core|cmd|confirm] [--origin …] [--since CUR]
```

#### e. 사용자 뷰
```
┌ loom dbg attach mcu ─ STM32H743 ● RUNNING ─ J-Link 4 MHz ─ watch 19.6 Hz (req 20) ─┐
│ WATCH                         value      Δ/s     │ RTT ch0                           │
│ g_boot_stage      u8          4          0       │ [boot] stage 3 ok                 │
│ g_can_init_err    u32         0x00000011 0       │ [boot] stage 4: can init ...      │
│ g_tick_ms         u32         51234      1000    │ timeout waiting for PLL2  [device]│
├─────────────────────────────────────────────────┤ > help                    [agent] │
│ plot g_boot_stage  ▁▂▃▄▄▄▄▄▄▄▄▄▄▄░░░░(HALTED)   │                                   │
├─────────────────────────────────────────────────┴───────────────────────────────────┤
│ ⚠ 확인 요청 cf-17: agent → HALT core ("inspect PLL2 config")   [y]허용 [n]거절 58s   │
│ [h]alt [r]esume [w]atch+ [e]dit [Tab]RTT 입력                                         │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

#### f. 주 라이브러리
- pyocd 0.46.1 (Apache-2.0, 2026-11-02, Py ≥3.9): Cortex-M 전용, CMSIS-DAP/ST-Link/J-Link 프로브. RTT 지원 여부: pyOCD에 `rtt` 하위명령이 있는 것으로 알고 있음(도입 버전 미확인).
- pylink-square 2.1.0 (Apache-2.0, 2026-11-06): SEGGER J-Link SDK 래퍼, RTT API 포함(SEGGER J-Link 소프트웨어 설치 필요, macOS/Windows 모두 SEGGER 제공).
- lauterbach-trace32-rcl 1.1.6 (MIT, 2026-07-02): TRACE32 원격 API — AURIX/RH850 등 비Cortex 차량 MCU 경로. TRACE32 PowerView가 떠 있어야 함(접근 △, 단 차량 업계 표준 도구).
- pyelftools 0.33 (Public domain, 2026-05-29): DWARF 타입 해석.
- OpenOCD는 TCL 포트(기본 6666, 미확인)로 텍스트 명령 — 별도 패키지 불필요. pygdbmi 0.11.0.0은 2023 이후 릴리스 없음 → GDB/MI 경로는 후순위.

#### g. 시뮬레이션 백엔드
- **M1: `SimTarget`(순수 Python)**: 메모리 맵(dict 기반 RAM/Flash/주변장치 영역), ELF 대신 **심볼 JSON**(이름·주소·타입) — CI에 툴체인 불필요. SimWorld가 변수(g_boot_stage, g_tick_ms 등)를 진화시키고, RTT 링버퍼(SEGGER RTT 컨트롤 블록 형식 흉내)에 로그를 씀. halt 시 변수 진화 정지.
- **M2: Renode 또는 QEMU + GDB 원격 프로토콜** 백엔드로 실제 펌웨어 ELF를 돌려 통합 테스트(Renode GDB 서버 지원 — 사실 확인 필요, 미확인).

#### h. M1 걷는 뼈대
- SimTarget에서 connect(attach) → read/watch(경계, 요약) → rtt read/send → write(화이트리스트 변수) → halt 요청 시 뷰어 확인 배너 → 사용자 허용 → 사용자 resume → 에이전트가 origin=user 읽음.
- 수용: CI 테스트, 확인 거절/시간초과 경로, HALTED 구간 플롯 표시, 주변장치 영역 읽기 기본 거부 확인.

#### i. 안전/가드레일 (가장 엄격)
| 동작 | 등급 | 규칙 |
|---|---|---|
| connect(attach), status, symbols | R0 | **기본 attach: halt/reset 하지 않음.** `--halt-on-connect`는 R3 |
| RAM/Flash 영역 읽기, watch, rtt read | R0 | 메모리 맵 기준 |
| **주변장치 레지스터 영역 읽기** | R2 | 읽기 부작용(상태 플래그 클리어, FIFO pop) 가능 → 기본 거부, SVD에서 부작용 없음 표시된 레지스터만 허용 |
| rtt send | R1 | 펌웨어가 명령으로 해석하므로 대상별 `rtt_policy` |
| RAM 변수 쓰기(프로파일 화이트리스트 심볼) | R2 | 되읽기 검증 |
| RAM 쓰기(화이트리스트 밖) | R3 | 확인 |
| halt / step / reset / 브레이크포인트 | R3 | 확인. 벤치에 액추에이터(모터, 밸브, 고전압)가 있다고 프로파일에 적혀 있으면 R4 |
| 주변장치 레지스터 쓰기 | R4 | 확인 토큰 + 사유 |
| flash erase/program | R4 | 이미지 해시·크기·대상 영역을 배너에 표시, 확인 토큰. PSU 인터록(전원 OFF 금지) 자동 설정 |
| 옵션 바이트, 읽기 보호(RDP)/락 비트, OTP, 보안 디버그 잠금 | **R5 금지** | 어떤 확인으로도 에이전트 불가(벽돌 위험) — 사용자가 직접 벤더 도구 사용 |

#### j. 리스크
- **MCU 계열**: AURIX/RH850이면 pyOCD 불가 → TRACE32/벤더 GDB 서버 백엔드가 주력이 되어 난이도·라이선스 비용 상승(질문 Q3).
- 프로브 점유: IDE 디버거가 프로브를 잡고 있으면 Loom이 못 엶. 대안: Loom 데몬이 GDB 서버를 재노출(pyOCD 내장 gdbserver 활용)해 IDE가 Loom을 통해 디버깅 → 모든 동작이 이벤트로 보임(M3 후보).
- 비침투 읽기도 버스 경합으로 타이밍 민감 코드에 미세 영향 가능.
- 보안 디버그(인증 필요) 잠긴 양산 샘플은 접근 불가.

### 4.5 릴레이/IO 보드 — `loom io`

#### a. 핵심 루프 표

| 요소 | 명령 | 데몬 구성요소 | 사용자 화면 |
|---|---|---|---|
| 직접 접근 | `loom io connect relays --driver hid-relay [--serial ABCDE]`, `--driver numato --port COM12`, `--driver modbus-rtu --port COM13 --unit 1`, `--driver ftdi-gpio` | `IoBackend` 드라이버 3–4종, 채널 이름 매핑(벤치 프로파일: `ch1=kl30, ch2=kl15, ch3=btn_wake`) | 상태줄 `relays ● USB-HID 8ch (readback ✓)` |
| 읽기 | `loom io state relays`, `loom io wait-for relays kl15=on --timeout 10s`, 입력핀 읽기 | 상태 폴러(되읽기 지원 시 5 Hz, 미지원 시 "명령 기준 추정 상태" 표시) | 채널 램프 행(이름, 상태, 마지막 변경자) |
| 쓰기 | `loom io set relays kl15 off`, `loom io pulse relays btn_wake 200ms`, `loom io seq relays kl30:on,+500ms,kl15:on` | 위험 게이트 + 채널별 레이트 제한 + 펄스 타이머(데몬 내) | 램프가 바뀌고 변경자(agent/user/script) 표시 |
| 실시간 표시 | `loom io attach relays` / 웹 "IO 패널" + 통합 타임라인 레인 | 이벤트 `io.state`, `io.cmd` | 키 `1`–`8` 토글(위험 채널은 확인), 웹에선 스위치 클릭 |

#### b. 종단 간 시나리오 — "웨이크업 버튼 펄스 후 사용자가 점화를 끈다"
```
에이전트: loom io pulse relays btn_wake 200ms
  화면(≤100 ms 목표): btn_wake 램프 ● (agent) → 200 ms 후 ○, 타임라인에 폭 200 ms 막대
  (SimWorld/실 DUT가 깨어나 DLT/UART 로그 시작 — 같은 타임라인의 다른 레인에 보임)
에이전트: loom dlt wait-for ecu1 --grep 'wakeup reason: BTN' --timeout 5s → 매치
사용자: IO 패널에서 kl15 스위치 OFF 클릭 → 확인 대화상자(R3, 사용자도 확인 대상) → OK
  {origin:"user", kind:"io.cmd", ch:"kl15", value:"off"} → 램프 ○ (user)
에이전트: loom io events relays --origin user --since @c3 → kl15 off by user 14:31:10.204
         → 에이전트는 다음 단계(슬립 전류 측정: loom psu wait-for --curr-lt 0.005 --hold 5s)로 진행
```

#### c. 실시간성 목표
- 명령 → 화면 ≤ 100 ms(방송이 먼저), 되읽기 확인 → ≤ 300 ms. 기계식 릴레이 동작 시간(수~수십 ms, 보드마다 다름, 미확인)은 되읽기로만 확정.
- 되읽기 없는 보드: 램프를 실선이 아닌 **테두리만**(추정 상태)으로 그리고 "readback 없음" 표기 — 화면이 거짓말하지 않게.

#### d. CLI 초안
```
loom io connect <name> --driver hid-relay|numato|modbus-rtu|ftdi-gpio [...]
loom io status|state <name> [--json]
loom io set <name> <ch|alias> on|off
loom io pulse <name> <ch> <dur> [--count N --gap DUR]
loom io seq <name> "kl30:on,+500ms,kl15:on"        # 데몬 내 타이밍, 전체를 한 위험 판정
loom io wait-for <name> <ch>=on|off --timeout T     # 입력핀/되읽기
loom io events <name> [--origin …] [--since CUR]
```

#### e. 사용자 뷰
```
┌ loom io attach relays ─ USB-HID 8ch ─ readback ✓ ──────────────────────────┐
│ 1 kl30     ● ON   agent 14:30:01   [R3]   5 btn_wake ○ off  agent 14:31:02  │
│ 2 kl15     ○ OFF  user  14:31:10   [R3]   6 (unused)                         │
│ 3 can_term ● ON   user  13:58:44   [R1]   7 ...                              │
│ [1-8] 토글  [p] 펄스  위험 채널은 확인 대화상자                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

#### f. 주 라이브러리
- USB HID 릴레이: pyhid-usb-relay 1.4.0 (2026-09-23, 라이선스 표기 비표준 — 미확인) 또는 hidapi 0.15.0(2025-12-09) 위 자체 20줄 드라이버.
- Numato: numato-gpio 0.14.0 (MIT, 2025-04-29, **Python <3.14** 제약 — 장기 리스크) → pyserial 직접 텍스트 프로토콜이 안전.
- Modbus RTU 릴레이: pymodbus 3.16.2 (BSD-3, 2026-11-03).
- FTDI GPIO: pyftdi 0.57.2 (BSD-3, 2026-06-02).

#### g. 시뮬레이션 백엔드
- `SimRelay`: 채널 상태 + SimWorld 연동(kl30 off → DUT 전원 없음, kl15 off → DUT 슬립 진입 → SimPsu 전류 4 mA, DLT/UART sim "entering sleep" 로그). 되읽기 있음/없음 두 모드.
- Modbus RTU 보드 경로는 pymodbus 서버로 CI 검증 가능.

#### h. M1 걷는 뼈대
- SimRelay에서 connect → state → set/pulse(데몬 타이머) → TUI 램프 + agent 표시 → 사용자 토글(위험 채널 확인) → 에이전트 events로 읽음.
- 수용: 위 4요소 자동 테스트, 레이트 제한 동작, 되읽기 없음 모드의 추정 표시.

#### i. 안전/가드레일
- 채널별 등급은 **벤치 프로파일에서 사용자가 지정**(기본: 이름이 없는 채널 = R3, kl30/kl15/전원 계열 = R3, 버튼/종단저항 = R1).
- 인터록: 플래시 중(UDS 0x34–0x37 진행, dbg flash) kl30/kl15 OFF → R4.
- 레이트 제한: 채널당 ≤ 2 토글/s, 펄스 최소 20 ms(보드 사양 확인 필요).
- `safe_state` 정의(예: `kl15=off, kl30=on`) — 시나리오 중단·데몬 종료 시 적용 여부를 프로파일에서 선택(기본: **현 상태 유지**, 의도치 않은 전원 차단 방지).

#### j. 리스크
- 보드 파편화(동일 외형 다른 프로토콜). 지원 보드 목록 명시 필요(Q1).
- 되읽기 없는 보드는 상태가 거짓일 수 있음(전원 재투입 시 초기화).
- 유도성 부하/고전류는 릴레이 보드 정격 확인 필요 — Loom이 알 수 없음 → 프로파일에 정격 기록, 스킬에서 경고.

---

## 5. 공통 계약(loom.core, loom.viz)에 요구할 것 — 도메인 플러그인 구조

### 5.1 목표와 원칙
- **새 도메인 추가 = 패키지 하나 + entry point 한 줄, core 코드 수정 0.** UART/CAN/UDS도 같은 구조로 등록되면(권장) core가 특정 도메인을 모르게 됨.
- 플러그인은 "장비를 어떻게 여는가/무엇을 읽고 쓰는가/어떻게 위험한가/어떻게 보이는가"만 선언하고, **데몬 수명·IPC·기록·방송·확인·뷰어 연결은 core가 전부 소유**. 그래야 어떤 도메인도 숨은 연결을 만들 수 없음(모든 쓰기가 core 게이트를 지남).
- 아래 시그니처는 **초안**(이름·형태 합의용)이며 구현이 아니다.

### 5.2 도메인 플러그인 인터페이스

```python
# loom/core/plugin.py — 초안(시그니처만)
from enum import IntEnum
from typing import Protocol, ClassVar, Any, Mapping, AsyncIterator, Callable, Awaitable

class Risk(IntEnum):
    R0_READ = 0        # 읽기, 장비 상태 불변
    R1_BENIGN = 1      # 되돌리기 쉬운 경미한 쓰기 (로그 레벨, RTT 텍스트, 콘솔 일반 입력)
    R2_STATEFUL = 2    # DUT 상태 변경, 한계 안 (전압 설정, RAM 변수, 버튼 릴레이)
    R3_DISRUPTIVE = 3  # 동작 중단/전원/리셋/halt — 사용자 확인 필요
    R4_DESTRUCTIVE = 4 # 영속·비가역 (flash, 영속 설정) — 확인 토큰 필요
    R5_FORBIDDEN = 5   # 에이전트 금지 (옵션 바이트, 보안 잠금)

class DomainPlugin(Protocol):
    name: ClassVar[str]          # "dlt" — CLI 그룹명, 이벤트 domain 값, 프로파일 섹션명
    api_version: ClassVar[int]   # core 플러그인 API 주버전. 불일치 시 로드 거부 + 경고
    def register(self, reg: "Registry") -> None: ...

class Registry(Protocol):
    def backend(self, kind: str, factory: "BackendFactory", *,
                sim: bool = False, platforms: frozenset[str] = frozenset({"darwin","win32","linux"})) -> None: ...
    def rpc(self, method: str, handler: "RpcHandler", *,
            writes: bool, risk: "RiskClassifier") -> None: ...        # method = "dlt.set_level"
    def event_kind(self, kind: str, schema: Mapping[str, Any], *,
                   version: int, metrics: "MetricMap | None" = None) -> None: ...  # kind = "dlt.log"
    def cli(self, app: Any) -> None: ...                             # typer/click 그룹 → `loom dlt …`
    def panel(self, spec: "PanelSpec") -> None: ...                   # 선언형 뷰어 패널 (§5.8)
    def sim_actor(self, name: str, factory: "SimActorFactory") -> None: ...  # SimWorld 참가자 (§5.5)
    def profile_section(self, schema: Mapping[str, Any]) -> None: ... # loom.bench.toml [dlt] 검증 스키마
    def interlock(self, name: str, active: "Callable[[], bool]") -> None: ...  # 예: "dbg.flashing"
    def skill(self, path: Any) -> None: ...                           # importlib.resources 경로 (§5.10)

RiskClassifier = Callable[[Mapping[str, Any], "Context"], "RiskDecision"]
# RiskDecision = (risk: Risk, reason: str, summary: str) — 인자를 보고 판정 (예: raw UDS 바이트 → SID 분류,
# 셸 명령 → 패턴, 메모리 주소 → 영역 맵). 플러그인은 등급만 판정, 허용/확인/거부 결정은 core 게이트.
```

### 5.3 장치 백엔드 등록과 채널 수명주기

```python
class DeviceBackend(Protocol):
    mode: ClassVar[str]                                  # "push"(DLT, 셸) | "poll"(PSU, dbg watch)
    async def open(self, cfg: Mapping[str, Any]) -> "DeviceInfo": ...   # IDN, 펌웨어, 기능(readback 등)
    async def close(self) -> None: ...
    def rx(self) -> AsyncIterator["RawRx"]: ...          # push형: 장비→데몬
    async def poll(self, what: str) -> list["RawRx"]: ...# poll형: core 스케줄러가 주기 호출 (주기·지터 계측은 core)
    async def write(self, op: "WriteOp") -> "WriteResult": ...
    def health(self) -> "Health": ...                    # connected | degraded | lost + 카운터

BackendFactory = Callable[["Context"], DeviceBackend]
```

core가 제공하는 **Channel**(도메인 무관): 이름, 소유 백엔드, 재연결 정책, 채널별 `seq`, 링버퍼, 기록기, 구독자별 필터 팬아웃, 폴링 스케줄러(실측 주기·지터를 `core.poll_stats` 이벤트로 발행), 쓰기 직렬화 큐.

**쓰기 파이프라인(모든 도메인 공통, 우회 불가)**:
```
RPC(method,args,origin) → RiskClassifier → 프로파일 한계(클램프/거부) → 인터록 확인
  → 게이트: R0–R2 자동 | R3 확인 | R4 확인 토큰 | R5 거부
  → 이벤트 발행 {dir:"tx", origin, corr, risk}  (먼저 방송 = 사용자가 송신 전에 봄)
  → backend.write → 결과 이벤트 {kind:"<d>.result", corr, ok, detail}
  → 감사 로그(audit.jsonl: who/what/risk/decision/approver)
```

### 5.4 공통 이벤트 레코드 확장 규칙

```json
{"v":1, "seq":1042,
 "ts":{"mono_ns":81234567890, "wall":"2026-11-09T14:02:13.000123+09:00",
       "dev":{"src":"dlt.tmsp","value":8138211,"unit":"100us"}},
 "domain":"dlt", "channel":"ecu1", "dir":"tx", "origin":"agent",
 "kind":"dlt.ctrl", "corr":"rq-77", "step":null, "risk":1,
 "payload":{"sid":1,"app":"CAMS","ctx":"CAM","level":"debug"},
 "metrics":null}
```

규칙:
1. **최상위 필드는 core 소유.** 계약 §3-6의 `ts/domain/channel/dir/origin/payload`에 `v, seq, kind, corr, step, risk, metrics`를 추가 요청(§8 CR-2). 도메인은 `payload` 안만 정의.
2. `kind = "<domain>.<type>"`, `event_kind()`로 JSON 스키마와 버전을 등록. 필드 추가는 하위 호환으로만, 깨는 변경은 `payload.pv` 증가.
3. `dir ∈ {rx, tx, none}` — 상태/메타 이벤트는 `none`.
4. `origin ∈ {agent, user, device, script:<run-id>, core}`. `script:` 접두로 시나리오 실행을 구분. 재생 이벤트는 원래 origin 유지 + 채널 네임스페이스 `replay:<session>/…`(§5.14).
5. `seq`는 채널별 단조 증가. 데몬 내부 드롭은 `core.gap {channel, from_seq, to_seq, reason}` 이벤트로 반드시 기록. 뷰어는 seq 불연속을 그대로 "생략" 표시에 사용.
6. `ts.dev`는 선택: 장비 자체 시각(DLT 0.1 ms 틱, CAN HW 타임스탬프, PSU 없음).
7. 바이너리: ≤256 B는 `*_hex`, 그 이상은 `*_b64`, 64 KB 초과(파형, 영상 프레임, 메모리 덤프)는 `blob_ref`(기록 디렉터리의 사이드 파일)로만.
8. **`metrics`**: 선택적 평평한 숫자 맵(`{"current_a":0.351,"voltage_v":13.49}`) + 등록 시 단위. `loom.viz`는 도메인을 몰라도 모든 `metrics`를 시계열로 그릴 수 있음 → "데이터만 주면 렌더링"을 새 도메인에도 자동 적용. 시리즈 이름 = `<domain>.<channel>.<key>`.
9. `corr`는 요청↔결과↔확인을 묶고, `step`은 시나리오 단계 id.
10. 이벤트 1건 상한 64 KB.

### 5.5 시뮬레이션 백엔드 등록과 SimWorld
- 각 플러그인은 `backend(kind, factory, sim=True)`로 sim 백엔드를 등록. `loom <d> connect x --sim` 또는 프로파일 `backend="sim"`.
- **SimWorld(요청: core 소유)**: 하나의 가상 DUT 모델(전원 상태: off/boot/run/sleep, 시각, 공유 변수)을 여러 도메인 sim이 공유. 플러그인은 `sim_actor()`로 참가자 등록:
  - SimPsu/SimRelay가 DUT 전원을 바꿈 → SimWorld 상태 전이 → SimDlt/UART sim/CAN sim/UDS 가상 ECU/SimTarget이 각자 반응(부팅 로그, 주기 프레임 시작, 변수 진화).
  - 이게 있어야 **교차 도메인 시나리오(전원 사이클 → 부팅 로그 → CAN 신호 → DTC)를 HW 없이 CI에서** 돌릴 수 있음.
- `loom sim up bench-demo`(프로파일의 모든 장비를 sim으로 띄움), `loom sim inject fault=can_init_timeout`(결함 주입).

### 5.6 안전 등급 레지스트리와 확인 프로토콜
- 등급 정의는 §5.2의 `Risk`. 도메인별 등급표는 플러그인이 classifier로 제공하고, 실행 시 `loom safety table [--domain d]`로 사람이 읽는 표를 출력(스킬 references 자동 생성 원천).
- **확인 프로토콜(core)**:
  1. 게이트가 `core.confirm.request {id, method, summary, risk, reason, expires_s=60}` 발행 → 모든 뷰어에 배너.
  2. 응답은 **뷰어(사용자)만** 가능: `core.confirm.answer {id, allow|deny}` origin=user. CLI(에이전트 클라이언트)는 answer RPC 호출 권한 없음(클라이언트 종류는 연결 핸드셰이크에서 선언, CLI 바이너리는 answer 미구현 — 로컬 신뢰 모델의 최선).
  3. **뷰어가 하나도 붙어 있지 않으면 R3 이상은 즉시 실패**(exit 12 "no viewer attached — ask the user to run `loom view`"). → R3 이상 동작은 사용자가 보고 있을 때만 일어남. 핵심 원칙의 "같은 순간에 본다"를 안전 장치로 강제.
  4. R4는 배너에 4자리 토큰 표시, 사용자가 뷰어에 토큰을 입력해야 허용(실수 y 방지).
  5. 반복 디버깅용 **세션 사전 허용**: 사용자가 뷰어에서 `grant dbg.halt 10m` → 기간 내 같은 메서드 R3 자동. 감사 로그에 grant 기록. R4/R5는 grant 불가.
- **인터록 레지스트리**: 플러그인이 `interlock("uds.flashing", fn)`, `interlock("dbg.flashing", fn)` 등록 → PSU/IO 플러그인의 classifier가 `ctx.interlocks.active()`를 보고 등급 승격.
- 레이트 제한: 메서드별 토큰 버킷, 프로파일에서 조정(상한은 플러그인 기본값 이하로만).

### 5.7 벤치 프로파일 (`loom.bench.toml`, 사용자 소유)

```toml
[bench]
name = "desk-bench-1"
actuators = false            # true면 dbg halt/reset 등 R4로 승격

[psu.bench]
resource = "TCPIP::192.168.0.50::5025::SOCKET"
channel = 1
vmax = 16.0   imax = 5.0   warn_curr = 1.5
agent_power_on = false       # true면 out on 자동(R2)

[io.relays]
driver = "hid-relay"
channels = { 1 = {alias="kl30", risk=3}, 2 = {alias="kl15", risk=3}, 5 = {alias="btn_wake", risk=1} }
safe_state = "keep"          # 중단 시 상태 유지 (또는 {kl15="off"})

[dlt.ecu1]
tcp = "192.168.0.10:3490"
inject_allow = [0x1000, 0x1001]

[dbg.mcu]
probe = "jlink:600112345"   device = "STM32H743ZI"   elf = "build/app.elf"
write_allow = ["g_cfg.*", "g_test_*"]

[shell.ivi]
ssh = "root@192.168.0.20"   policy = "normal"
```
- 데몬은 로드 시 해시를 기록하고 모든 이벤트 세션 manifest에 넣음. 에이전트는 이 파일을 수정하지 않는다(스킬 규칙 + 변경 감지 시 `core.profile.changed` 이벤트와 사용자 확인 후 반영).

### 5.8 뷰어 패널 등록 (loom.viz에 요청)
- **선언형 패널 스펙 우선**: 대부분 도메인은 공통 패널 타입 조합으로 끝나야 함.

| 패널 타입(viz 제공 요청) | 쓰는 도메인 |
|---|---|
| `trace_table` (열 매핑, origin 색, 생략 행, 행 펼치기) | DLT, CAN, UDS, IO 이벤트 |
| `timeseries` (`metrics` 자동, gap 표시, 실측 주기 표시) | PSU, dbg watch, DLT 메시지율 |
| `gauge` (큰 숫자 + 설정값 + 한계선) | PSU, DMM |
| `state_lamps` (채널 상태, 변경자, 추정 상태 표시) | IO, PSU 출력, dbg 코어 상태 |
| `console` (UART 팀 터미널 위젯) | UART, shell, RTT |
| `timeline_lane` (§5.13) | 전부 자동 |
| `confirm_banner`, `run_panel` (core) | 전부 |

```python
@dataclass
class PanelSpec:
    id: str                      # "dlt.trace"
    type: str                    # "trace_table"
    title: str
    source: Mapping[str, Any]    # {"kinds": ["dlt.log","dlt.ctrl","dlt.gap"], "channel": "*"}
    columns: list[Mapping] | None = None   # [{"name":"APID","path":"payload.apid"}, …]
    actions: list["PanelAction"] = ()      # 키/버튼 → RPC 매핑 (예: L → dlt.set_level, 인자 프롬프트)
```
- `actions`가 사용자 개입 수단의 표준 형태: 뷰어는 액션을 RPC로 호출하고 origin=user가 자동으로 붙음. 도메인이 뷰어 코드를 안 짜도 개입 UI가 생김.
- 탈출구: 복잡한 화면(dbg 레지스터 뷰)은 Textual 위젯 클래스/웹 JS 모듈을 `panel(spec, custom=…)`로 등록 가능(후순위).

### 5.9 CLI 규약 (도메인 공통)
- 공통 동사: `connect/disconnect/status/events/attach` 필수, 해당 시 `tail|read`, `wait-for`, `watch --for`, `export`.
- 무한 스트림 금지: `watch`/`tail`은 `--for`/`--tail` 필수 기본값, `wait-for`는 `--timeout` 필수.
- 출력 상한: 기본 200줄 / 32 KB, 넘으면 잘라내고 마지막 줄에 `(truncated: N more; use --since CURSOR)`. 모든 읽기 명령은 마지막에 cursor 출력.
- 종료 코드(제안): 0 성공, 2 사용법, 3 사용자 개입으로 중단(interrupted by user), 4 가드 거부(한계/R5), 5 타임아웃, 10 확인 거절, 11 확인 시간초과, 12 확인할 뷰어 없음, 20 장비 오류, 21 연결 끊김.
- `--json`: JSONL(이벤트당 1줄) + 마지막 요약 객체. `--dry-run`: 쓰기 명령 전부 지원(판정 등급, 실제 송신 바이트/명령 문자열 출력).

### 5.10 스킬 패키징 규약과 도메인별 SKILL.md 초안
- 위치: `.claude/skills/loom-<domain>/SKILL.md` + `references/`. 플러그인 패키지에 원본을 두고 `loom skills sync`가 저장소의 `.claude/skills/`로 복사(사용자 전역 `~/.claude`는 건드리지 않음).
- SKILL.md는 60줄 이하: 언제 쓰나 → 시작 절차(status→connect→읽기 우선) → 핵심 명령 10개 → 안전 규칙 요약 → references 링크.
- Codex 호환: 저장소 `AGENTS.md`에 "장비 작업 전 `.claude/skills/loom-*/SKILL.md`를 읽을 것" 한 단락 + 동일 references 경로(파일 공유, 복제 없음).
- 공통 절차(모든 loom 스킬 첫 줄): "사용자가 `loom view`를 열어 두었는지 `loom status --viewers`로 확인하고, 없으면 열어 달라고 요청한다."

| 스킬 | SKILL.md 핵심 내용 | references/ |
|---|---|---|
| `loom-dlt` | 연결 → `contexts`로 앱/컨텍스트 파악 → `tail --level warn`부터 → 레벨 올리기는 컨텍스트 단위·작업 후 원복 → `marker`로 실험 구간 표시 | `cli.md`, `control-messages.md`(SID 표, 등급), `filters.md`, `export-dlt-viewer.md` |
| `loom-psu` | 프로파일 확인(`status`) → 설정 → 출력 ON은 확인 대상일 수 있음 → `watch --summary`로 읽기 → 이상 전류 시 즉시 OFF 제안(직접 OFF는 확인) → 되읽기 해상도 한계 명시 | `cli.md`, `safety.md`, `scpi-dialects.md`, `sleep-current-howto.md` |
| `loom-shell` | 공유 PTY 원칙 → `exec`는 짧은 명령만 → 로그는 `log open` 채널로 → 위험 명령 목록 → 비밀번호 입력 금지 | `cli.md`, `dangerous-commands.md`, `log-channels.md` |
| `loom-dbg` | **attach만, halt 금지가 기본** → 심볼 확인 → watch/RTT로 관찰 → halt/reset/write는 이유를 적고 사용자 확인 → flash는 사용자가 명시 요청한 경우만 → R5 목록 | `cli.md`, `safety.md`(등급표), `backends.md`(pyOCD/J-Link/TRACE32), `watch-rates.md`, `rtt.md` |
| `loom-io` | 채널 별칭으로만 조작 → 전원 계열은 확인 → 펄스/시퀀스 → 인터록 설명 | `cli.md`, `safety.md` |
| `loom-run` (core) | 시나리오 작성 규칙 → `run plan`(사전 위험 요약)으로 사용자에게 보여주기 → `start` → `wait` 경계 → 중단 시 처리 | `scenario-schema.md`, `step-types.md`, `examples/*.yaml` |

### 5.11 패키지 배포
```toml
# 각 도메인 패키지 pyproject.toml
[project.entry-points."loom.domains"]
dlt = "loom_dlt.plugin:DltPlugin"
```
- 모노레포 안에서는 `loom.dlt`, `loom.instr`(psu/dmm/scope), `loom.shell`, `loom.dbg`, `loom.io`를 같은 entry point 방식으로 등록 → 외부(사내 전용 장비, seed-key 등) 플러그인도 동일 경로.
- 무거운 의존성은 extras로 분리: `loom[dbg]`(pyocd, pylink-square, pyelftools), `loom[instr]`(pyvisa, pyvisa-py, pyusb), `loom[shell]`(asyncssh, adbutils), `loom[io]`(hidapi, pymodbus, pyftdi). 기본 설치는 core + UART/CAN/UDS + DLT(의존성 없음).
- 데몬 시작 시 `importlib.metadata.entry_points(group="loom.domains")` 로드, `api_version` 불일치·import 실패는 해당 도메인만 비활성 + `loom status`에 사유.

### 5.12 교차 기능 ① 시나리오/테스트 러너 — `loom run` (core)

**판정: loom.core 기능.** 데몬 안의 잡으로 실행 → CLI가 끝나도 계속 돌고, 사용자 뷰어가 진행을 라이브로 보며 개입 가능. 각 단계는 CLI와 **같은 RPC**를 `origin=script:<run-id>`, `step=<id>`로 호출하므로 별도 경로가 없음.

예시 (작업 지시의 PSU → UART → CAN → UDS):
```yaml
name: cold-boot-smoke
profile: desk-bench-1
vars: { boot_timeout: 30s }
steps:
  - id: power_off
    call: psu.out { name: bench, state: off }          # R3 → 시작 시 일괄 승인 대상
  - id: wait_off
    sleep: 2s
  - id: power_on
    call: psu.out { name: bench, state: on }
  - id: boot_log
    wait_for: uart.match { port: dut, regex: "login:", timeout: ${boot_timeout} }
  - id: can_alive
    wait_for: can.signal { channel: ch0, signal: "VehState.PowerMode", op: "==", value: 2, timeout: 5s }
  - id: check_current
    assert: psu.meas { name: bench, curr_between: [0.2, 0.6] }
  - id: operator_check
    manual: "디스플레이에 부팅 로고가 보이면 PASS를 누르세요"   # 사용자 수동 단계
    timeout: 120s
  - id: dtc
    call: uds.dtc_list { ecu: body, mask: 0x09 }
    expect: { count: 0 }
finally:
  - call: dlt.marker { name: ecu1, label: "run end" }
on_fail: pause            # pause | abort | continue
```

- 단계 타입: `call`(도메인 RPC), `wait_for`(이벤트 조건 + 필수 timeout), `assert`, `sleep`, `manual`(사용자만 완료 가능), `parallel`(M2), `finally`, `on_fail`.
- **사전 위험 요약**: `loom run plan file.yaml` → 단계별 등급·인터록·대상 장비 표. `start` 시 R3 단계 목록을 한 번의 확인 배너로 일괄 승인(“이 실행에서 psu.out off/on 2회 허용”), R4 단계는 단계마다 토큰 확인.
- **개입 UX**(뷰어 run 패널): `p` 일시정지(현재 `wait_for`의 타이머도 정지), `c` 계속, `s` 현재 단계 건너뛰기(결과 SKIPPED, 기록), `a` 중단(→ `finally` 실행, 프로파일 `safe_state` 적용 여부 표시), `m` 수동 단계 PASS/FAIL + 메모, `r` 실패 단계 재시도. 일시정지 중 사용자가 직접 장비를 조작하면 그 이벤트(origin=user)가 run 로그에 "operator action"으로 함께 기록.
- 에이전트 CLI:
```
loom run plan cold-boot.yaml
loom run start cold-boot.yaml [--var boot_timeout=60s] --json   → {"run":"r-0012"}
loom run wait r-0012 --timeout 300s         # 경계 있는 대기, 종료/일시정지/수동단계 대기 시 반환
loom run status r-0012                       # 단계 표
loom run log r-0012 --tail 50
loom run pause|resume|abort r-0012           # 에이전트도 가능(origin=agent)
```
```
$ loom run status r-0012
cold-boot-smoke  r-0012  RUNNING  4/8  elapsed 00:41
 ✓ power_off      psu.out off            0.12 s  (approved by user 14:40:01)
 ✓ wait_off       sleep 2s
 ✓ power_on       psu.out on             0.10 s
 ▶ boot_log       uart.match "login:"    18.2 s / 30 s
 · can_alive · check_current · operator_check(manual) · dtc
```
- 결과 기록: run = 세션 기록 안의 구간 + 단계 경계 마커. JUnit XML export는 후순위.
- UDS 팀의 "시나리오 YAML"은 이 스키마의 `uds.*` 단계로 표현 → 공통 계약 변경 요청 CR-5.

### 5.13 교차 기능 ② 통합 타임라인 (loom.viz 패널)
- 레인 = (domain, channel). 이벤트는 점/막대(IO 펄스, PSU 출력 ON 구간, dbg HALTED 구간, run 단계), `metrics`는 미니 시계열.
- 시각 기준 = 호스트 `mono_ns`(단일 데몬이면 모든 도메인이 같은 시계). 장비 시각(`ts.dev`)은 툴팁으로, 오프셋 추정은 M2.
- 커서 동기화: 타임라인에서 시점을 클릭하면 모든 패널(DLT 트레이스, CAN 트레이스, UDS 표)이 그 시점으로 스크롤.
- 라이브 모드(오른쪽 끝 따라감) / 고정 모드. 렌더 상한 초과 시 레인별 "밀도 막대"로 바꾸고 표시.

### 5.14 교차 기능 ③ 세션 녹화·재생 (core)
- 녹화: 데몬 시작부터 자동. `sessions/<날짜시각>/`에 `events.jsonl`(모든 도메인), 도메인 원본 포맷(`ecu1.dlt`, `ch0.blf`, `dut.uart.raw`), `blobs/`, `manifest.json`(프로파일 해시, 플러그인 버전, run 목록), `audit.jsonl`.
- 재생: `loom replay <session> [--speed 1x|max] [--from T]` → core의 replay 백엔드가 이벤트를 `replay:<session>/<domain>/<channel>` 네임스페이스로 다시 흘림 → **모든 뷰어·`tail`/`wait-for` CLI가 그대로 동작**(에이전트가 과거 세션을 같은 명령으로 분석).
- 재생 채널의 쓰기 RPC는 전부 거부(장비 쓰기 재현 금지). "재생에서 본 송신을 실제로 다시 보내기"는 사용자가 명시적으로 run 시나리오로 옮겨야 함.

### 5.15 화면 레이아웃 스케치 (herdr + 웹)

herdr 탭 구성 제안:
```
┌──────────────── herdr ───────────────────────────────────────────────────────────────┐
│ [pane 1] Claude Code (agent)        │ [pane 2] loom view (TUI, 멀티 패널)            │
│ > loom run start cold-boot.yaml     │ ┌ run r-0012 ▶ boot_log 18/30s  [p][a][s] ┐    │
│ {"run":"r-0012"}                    │ ├ psu bench  13.49V 0.35A ● ON(script)     ┤    │
│ > loom run wait r-0012 --timeout 300│ ├ io  kl30● kl15● btn○                      ┤    │
│                                     │ ├ uart dut  [boot] U-Boot 2024.04 …        ┤    │
│                                     │ ├ dlt ecu1  CAMS CAM info frame 1 …        ┤    │
│                                     │ └ ⚠ confirm cf-21: run r-0012 R3×2 [y][n]  ┘    │
└──────────────────────────────────────┴──────────────────────────────────────────────┘
```
웹 대시보드(`loom view --web`, 127.0.0.1): 상단 run 패널 + 확인 배너, 중앙 좌 DLT/UART 트레이스 탭, 중앙 우 PSU 계기판 + dbg watch 플롯, 하단 통합 타임라인(전 폭). 모든 패널 행/점에 origin 색(agent 청록, user 노랑, device 회색/흰색, script 보라).

---

## 6. 추천 착수 순서와 이유

| 단계 | 내용 | 선행 조건 | 이유 |
|---|---|---|---|
| **E0** (세 팀 M1과 병행, 문서 위주) | 플러그인 계약(§5.2–5.4) 합의안, SimWorld 명세, 벤치 프로파일 스키마, 디버그 프로브 스파이크(pyOCD/J-Link 실행 중 메모리 읽기 레이트, RTT 폴링 — Sol 담당 권장) | 없음 | core가 굳기 전에 확장 지점을 넣어야 비용이 낮음. 디버그 프로브는 불확실성이 가장 커서 일찍 스파이크 |
| **E1** DLT | §4.1 M1 | core 데몬 M1(채널·이벤트·뷰어 구독), UART/CAN M1의 TUI 트레이스 위젯 | 의존성 없음·HW 없음·사용자가 이미 씀. **플러그인 계약을 처음 검증하는 도메인**으로 스트림형(고부하) 경로를 시험하기 좋음 |
| **E2** PSU + IO | §4.2, §4.5 M1, SimWorld 1차 | E1의 플러그인 계약 검증, 확인 프로토콜(§5.6) | 폴링형 도메인 + 위험 등급 R3 경로(확인 배너)를 처음 구현. 벤치 자동화의 물리적 기반 |
| **E3** 시나리오 러너 | §5.12 M1: call/wait_for/assert/sleep/manual, pause/abort, run 패널 | 도메인 3개 이상 M1(UART + PSU + IO 또는 DLT), SimWorld | 교차 도메인 가치가 처음 드러나는 시점. 이보다 먼저 만들면 단계 타입이 한 도메인에 치우침 |
| **E4** 타깃 셸 | §4.3 M1 | UART 팀 콘솔 엔진의 전송 추상화(§8 D-1) | 비용이 가장 작지만 UART 팀 일정에 의존 → 그 팀 M2 이후 |
| **E5** 디버그 프로브 | §4.4 M1(SimTarget) → M2 실프로브 | 확인 프로토콜, R4 토큰, 인터록, Q3 답(MCU 계열) | 가치 최상이나 위험·불확실성 최대. 안전 장치가 모두 검증된 뒤에 |
| 이후 | DMM/전자부하/스코프(`loom.instr` 2–3단계), XCP(Q6 답에 따라), HMI 영상, LIN(CAN 팀 협의) | | |

세 팀 M1과의 관계: E0는 지금, E1은 core 데몬 M1 직후(UART/CAN M1과 겹쳐도 됨), E2 이후는 세 팀 M2와 병렬. 이 팀 작업이 세 팀 경로를 막지 않도록 **core 변경은 E0에서 요청만 하고 구현은 core 소유자**가 한다.

---

## 7. 사용자에게 물어볼 질문 (기본값 제안)

| # | 질문 | 기본값(답 없을 때) |
|---|---|---|
| Q1 | 보유 장비 목록: 전원공급기(모델/연결 LAN·USB·시리얼), 릴레이/IO 보드, DMM/전자부하/스코프 모델 | 범용 SCPI over LAN PSU 1대, USB HID 8채널 릴레이를 가정 |
| Q2 | 실차 작업인가 벤치 작업인가? 벤치에 액추에이터(모터, 밸브, 고전압)가 있나? | 벤치, 액추에이터 없음(`actuators=false`) |
| Q3 | 디버그 대상 MCU 계열과 프로브: Cortex-M(STM32/S32K/NXP)? AURIX/RH850? 프로브는 J-Link/ST-Link/Lauterbach? | Cortex-M + J-Link. AURIX면 TRACE32 백엔드로 전환하고 E5를 뒤로 미룸 |
| Q4 | DLT: 대상이 verbose인가 non-verbose(FIBEX/ARXML 필요)인가? 이더넷(TCP 3490)인가 직렬 DLT인가? DLT Viewer와 동시 접속이 필요한가? | verbose, TCP 3490, DLT Viewer 동시 사용 |
| Q5 | 리눅스/QNX/안드로이드 기반 ECU에 SSH/ADB 접근이 있나? | SSH 있음, ADB는 후순위 |
| Q6 | 업무에 XCP 측정·캘리브레이션(A2L)이 있나? | 없음 → XCP는 후보 유지만 |
| Q7 | 실HW가 붙는 PC: macOS 개발 머신에 직접? 별도 Windows 벤치 PC? | Windows 벤치 PC + macOS에서 원격 접속(§8 CR-6) |
| Q8 | 위험 동작 확인 방식: 매번 뷰어 확인 vs 세션 사전 허용(grant) 허용 여부 | R3 매번 확인 + grant 허용(10분 상한), R4는 항상 토큰 |
| Q9 | 시나리오 결과를 CI/테스트 관리 도구(JUnit 등)로 내보낼 필요? | 불필요(후순위) |
| Q10 | 전원 제어 위험 기본값: 에이전트가 출력 ON까지 자동으로 해도 되나? | 아니오(`agent_power_on=false`, 확인 필요) |

---

## 8. 공통 계약 변경 요청 / 타팀 의존성

### 공통 계약 변경 요청 (메인 오케스트레이터 결정 요청)
| ID | 요청 | 근거 |
|---|---|---|
| CR-1 | **도메인 플러그인 구조**(§5.2–5.3, entry point `loom.domains`)를 core 계약으로 채택. UART/CAN/UDS도 같은 방식 등록 권장 | 새 도메인이 core 수정 없이 붙고, 모든 쓰기가 core 게이트를 지나 숨은 연결 불가 |
| CR-2 | 공통 이벤트 레코드에 `v, seq, kind, corr, step, risk, metrics`, `ts.dev` 추가와 §5.4 규칙 | 드롭 정직 표시(seq), 요청↔응답·시나리오 상관, 도메인 무관 플롯(metrics) |
| CR-3 | `origin`에 `script:<run-id>`, `core` 값 허용 | 시나리오 실행 구분, 데몬 자체 이벤트 |
| CR-4 | 위험 등급 R0–R5 공통 정의 + **확인 프로토콜**(뷰어만 응답, 뷰어 없으면 R3+ 실패, R4 토큰, grant) + 인터록 레지스트리를 core로 | UDS 팀의 위험 등급과 같은 척도를 써야 시나리오 러너가 도메인 섞어 사전 요약 가능. 브리프 §3-10은 "스킬 레벨 가드레일"인데 **데몬 강제**로 격상 제안 |
| CR-5 | 시나리오 러너를 core(`loom run`)로, UDS 시나리오 YAML은 그 스키마의 `uds.*` 단계로 | 도메인별 러너가 생기면 교차 시나리오 불가 |
| CR-6 | 원격 벤치: IPC는 로컬 전용 유지하되 `loom --remote <ssh-host>`가 SSH 로컬 포워딩으로 원격 데몬에 붙는 공식 경로 | 개발 macOS + HW Windows(브리프 §3-9). 계약 위반 없이 해결 |
| CR-7 | SimWorld(공유 가상 DUT)를 core 시뮬레이션 기반으로 | 교차 도메인 시나리오의 HW 없는 CI |
| CR-8 | 벤치 프로파일(`loom.bench.toml`)을 공통 설정 파일로, 장비 한계값은 에이전트 수정 불가 | PSU/IO/dbg 한계를 스킬 문서가 아닌 데몬이 강제 |
| CR-9 | CLI 종료 코드·출력 상한·cursor 규약(§5.9) 공통화 | 에이전트가 모든 도메인을 같은 방식으로 다룸 |

### 타팀 의존성
| ID | 대상 팀 | 요청 |
|---|---|---|
| D-1 | UART | 콘솔 엔진을 `Transport` 추상화(바이트 송수신 + 연결 상태)로 분리 → `loom shell`이 SSH/ADB/TCP 전송만 추가. TUI 터미널 위젯도 재사용 가능하게 |
| D-2 | CAN (viz 소유) | 패널 타입 `gauge`, `state_lamps`, `timeline_lane`, `confirm_banner`/`run_panel` 호스팅, `metrics` 자동 시계열, gap/실측 주기 표시 규칙(§5.8, §5.13) |
| D-3 | CAN | (후순위) XCP on CAN을 붙일 경우 ISO-TP처럼 데몬 내 호스팅 또는 고속 프레임 구독 API. LIN 확장 의향 확인 |
| D-4 | UDS | 플래시 진행 상태를 인터록(`uds.flashing`)으로 노출 → PSU/IO 전원 차단 승격 |
| D-5 | UDS | 위험 등급 척도를 CR-4와 맞춤(UDS 팀 등급표를 R0–R5에 매핑) |

---

## 9. 핵심 원칙 체크리스트 자기점검

02-core-principle.md 6개 항목을 상위 추천 5개 각각에 대해 점검. (이 문서는 R1 탐색 초안이므로 "plan.md 맨 앞 핵심 루프 절"은 각 도메인 소절 a에 해당.)

| 항목 | DLT §4.1 | PSU §4.2 | Shell §4.3 | Dbg §4.4 | IO §4.5 |
|---|---|---|---|---|---|
| 1. 핵심 루프 표(4요소) | [x] 4.1a | [x] 4.2a | [x] 4.3a | [x] 4.4a | [x] 4.5a |
| 2. 종단 간 시나리오(사용자 개입 포함) | [x] 4.1b | [x] 4.2b | [x] 4.3b | [x] 4.4b | [x] 4.5b |
| 3. 실시간성 목표 + 고부하 정직성 | [x] 4.1c | [x] 4.2c | [x] 4.3c (UART 정책 준용) | [x] 4.4c | [x] 4.5c |
| 4. M1 = 시뮬레이션 위 걷는 뼈대 | [x] 4.1h | [x] 4.2h | [x] 4.3h (UART D-1 전제) | [x] 4.4h | [x] 4.5h |
| 5. 사용자 화면이 에이전트 세션과 분리 안 됨 | [x] §4 공통 전제, 4.1e | [x] 4.2e | [x] 4.3a 숨은 exec 금지 규칙 | [x] 4.4e, §5.6 확인은 뷰어에서만 | [x] 4.5e |
| 6. 원칙에 기여 안 하는 기능은 후순위/비범위 | [x] 4.1f 비verbose M3 | [x] 4.2f 비SCPI M2 | [x] 4.3d push/pull M2, ADB sim M2 | [x] 4.4j GDB 재노출 M3, SVD M2 | [x] 4.5 드라이버 3–4종 한정 |

공통 보강: 실시간 지연 수치는 모두 **목표치이며 실측 근거 없음**(§4 공통 전제). R2에서 Sol 실측과 대조해 조정 필요. 교차 기능(§5.12–5.14)은 core 기능으로 판정했으므로 이 표에서 제외했으나, 시나리오 러너도 run 패널 라이브 표시·사용자 개입(§5.12)·SimWorld 기반 M1(E3)을 갖춘다.

<!-- END R1 -->
