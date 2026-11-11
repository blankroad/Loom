# R2b — 저가 오실로스코프 / 로직 애널라이저 재평가 (explore-opus)

작성: explore-opus, 2026-11-09
기준: `02-core-principle.md`(최우선), `explore/user-notes.md`(사용자 관심: 알리 등 저가 스코프·디지털 채널, PC 제어 필수, FNIRSI 같은 PC 연결 불가 휴대용 제외)
조사 방법: sigrok wiki, 벤더 제품·문서 페이지, PyPI, Homebrew formula 페이지, 커뮤니티 저장소를 웹으로 확인. **스파이크는 하지 않음**: 이 머신에 `sigrok-cli`/PulseView가 없고(`which sigrok-cli` → not found), 전역 설치(brew)가 금지라서다. sigrok 실행 확인은 전부 "미확인"이다.
가격 표기: 공식 사이트 표시가는 출처를 달았다. 알리 가격은 확인하지 못했으므로 모두 **"알리 추정, 미확인"**이다.

---

## 0. 결론 (먼저)

1. **이 장비군은 단독으로는 핵심 루프가 반쪽이다.** 대부분의 저가 스코프와 LA에서 "쓰기"는 장비 설정(트리거, 타임베이스, arm)뿐이다. DUT에 자극을 주는 출력(AWG, 디지털 패턴)이 있는 제품은 **Digilent Analog Discovery 3**와 **Hantek DSO2D15(AWG 1채널)** 정도다. 그러나 Loom의 다른 도메인(PSU, IO, UART, CAN, UDS)이 자극을 주고 스코프/LA가 관측하면 루프가 닫힌다: **에이전트가 자극 → 스코프/LA가 캡처 → 사용자가 같은 화면에서 봄**. 차량 벤치에서 가치가 큰 쪽은 오히려 이 교차 사용이다(전원 인가 시 레일·리셋 타이밍, CAN 물리층, MCU↔SBC SPI).
2. **추천 구조: 두 갈래로 나눈다.**
   - **스코프 = `loom.instr` 확장 (`loom scope`)**. PSU와 같은 pyvisa 백엔드·SCPI 드라이버 프로파일을 공유한다. LAN SCPI 스코프가 macOS에서 가장 확실한 경로다.
   - **LA = 별도 도메인 `loom la`**. sigrok-cli를 **subprocess**로 실행한다(libsigrok는 GPL-3.0이라 데몬에 import하지 않는다). 연속 스트리밍과 sigrok 프로토콜 디코더(UART/SPI/I2C/CAN/LIN)가 핵심이다.
3. **상위 추천 위치: 5위 "관측 계측(스코프/LA)"로 넣고, XCP는 조건부 예비로 내린다.** XCP는 원래 조건부(사용자 업무에 캘리브레이션이 있는지 미확인)였고, 스코프/LA는 사용자가 직접 관심을 밝혔다. 착수는 PSU 다음이 가장 싸다(백엔드 공유, PSU가 자극을 제공). 1–4위(PSU+IO, DLT, Shell, Debug)는 그대로 둔다. 근거는 §5.
4. **저가 조합 3세트** (상세 §3):
   - **A. 최저가**: fx2 8ch 클론 + Hantek 6022BE. 약 $70–100(알리 추정, 미확인). sigrok 정식 드라이버 두 개만 쓰고 자극 출력은 없다.
   - **B. macOS 확실 + 차량 디버깅**: LAN SCPI 스코프(Siglent SDS804X HD $461 / Rigol DHO804 $459) + fx2 클론. 약 $470.
   - **C. 올인원 + 자극**: Digilent Analog Discovery 3. $379. 단독으로 4요소가 모두 성립하는 유일한 후보지만, macOS는 Rosetta 경유이고 런타임이 독점이다.

---

## 1. 후보 제품 평가표

판정 기호는 접근/읽기/쓰기/실시간(○△✗) 순서다. 기준은 r1-opus §1.1이다. "쓰기"의 (a)/(b)/(c) 구분은 §2.

| 제품 | 가격대 | 아날로그 ch·대역 | 디지털 ch | PC 연결 | 제어 경로 | macOS 근거 | Windows | 라이브 방식 | 쓰기 | 디코드 | 루프 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **fx2(FX2LP) 8ch 24MHz LA 클론** ("Saleae Logic 클론") | $5–15 (알리 추정, 미확인) | — | 8 | USB2 | sigrok `fx2lafw`. wiki "Supported hardware"에 클론류 지원(MCU123 클론, ESLA100 등 100%) | sigrok-cli Homebrew **arm64 bottle 있음**(0.7.2, libsigrok 0.5.2) — 실기 동작 미확인 | sigrok-cli 설치본 + Zadig(WinUSB) | **연속 스트리밍**(장비 메모리 없음, SW 트리거) — 설계상 추정, 상한은 USB2 대역 | (a)만 | sigrok 디코더(UART/SPI/I2C/CAN/LIN 등) | ○○△○ |
| **Hantek 6022BE** | $60–80 (알리 추정, 미확인) | 2ch 20MHz, 48MS/s, 8bit | — | USB2 | sigrok `hantek-6xxx`(**supported**), 장착할 때마다 오픈 fx2lafw 펌웨어 업로드(영구 플래시 안 함) | sigrok-cli(Homebrew) 경유 가능성. 실기 미확인. OpenHantek6022 macOS는 "completely untested"(MacPorts) | Zadig 두 번 필요(펌웨어 업로드 전·후 VID/PID) | USB 스트리밍, **HW 트리거 없음(SW-only)**, 장비 메모리 없음 | (a)만 | 호스트 쪽 sigrok 디코더 | ○○△○ |
| **Hantek 6022BL** | $70–100 (알리 추정, 미확인) | 2ch 20MHz(스코프 모드) | 8(sigrok LA 모드 24MHz) | USB2 | **H/P 버튼으로 모드 전환** — LA 모드는 Saleae VID/PID로 잡혀 `fx2lafw`, 스코프 모드는 sigrok 6022BL 페이지 기준 **미지원**("only the 8-channel logic analyzer mode is supported"). MSO가 아님(동시 사용 불가) | 위와 같음 | 위와 같음 | 스트리밍 | (a)만 | sigrok 디코더 | **△**○△○ (모드 전환이 물리 버튼이라 에이전트 단독 불가) |
| **Kingst LA2016** / LA1016 | $90–130 (알리 추정, 미확인) | — | 16, 200MHz(LA2016) / 100MHz(LA1016) | USB2 | sigrok `kingst-la2016`: LA2016·LA1016·LA5016 **tested**. **FPGA 비트스트림을 KingstVIS에서 추출해야 함** | 드라이버가 2021-11~2022-02 이후 코드 권장 → **libsigrok 0.5.2(Homebrew stable)에는 없을 가능성 큼(추정)**. nightly는 macOS x86 DMG(Rosetta) 또는 직접 빌드 — 미확인 | nightly 설치본 | **버퍼 캡처**(SDRAM). wiki: "can stream direct to USB but we don't implement that method" | (a). 하드웨어 PWM 출력은 있으나 sigrok에서 노출 안 됨(디버그 로그의 PWM 레지스터만 확인) | sigrok 디코더 | ○○△△ |
| **Kingst LA1010** | $60–90 (알리 추정, 미확인) | — | 16, 100MHz | USB2 | 같은 드라이버지만 wiki 표에 **"untested"** | 위와 같음 | 위와 같음 | 버퍼 | (a) | sigrok 디코더 | △○△△ |
| DreamSourceLab DSLogic Plus / **U2Basic** | Plus $100–150, U2Basic ~$70 (알리 추정, 미확인) | — | 16 (400MHz@4ch … 100MHz@16ch) | USB2 | sigrok `dreamsourcelab-dslogic`: Basic/Plus/Pro **supported**, **U2Basic·U2Pro16·U3Pro16·pgl12 리비전은 미지원**. 펌웨어는 DSView에서 가져옴 | DSView(자체 GUI)의 macOS판 존재 여부·자동화 API 미확인 | 미확인 | 32MB 버퍼 + `continuous=on` 모드 | (a) | sigrok 디코더 | ○○△○ (Plus) / ✗ (U2Basic via sigrok) |
| Sipeed SLogic Combo 8 / SLogic16U3 | 16U3 $69 (cnx-software 기사 제목) | — | 8 / 16 | USB2 / USB3 | sigrok mainline 목록에 없음. Sipeed의 libsigrok fork(`slogic-dev`)와 자체 PulseView·SLogic 앱 | Sipeed 자료끼리 충돌: Combo 8 wiki "no MacOS environment" vs 최신 SLogic 앱 "macOS 15+, Apple Silicon"(제3자 미러) → 미확인 | 자체 앱 | 스트리밍(Linux 320Mb/s, Windows 160Mb/s 표기) | (a) | sigrok 디코더(fork) | △○△○ |
| **Rigol DHO804** (DHO802/812/814) | DHO804 **$459** (DHO802 $349, 할인 $314.10; DHO814 $549) — [rigolna](https://www.rigolna.com/products/digital-oscilloscopes/dho800/) | 4ch 70MHz, 12bit, 1.25GSa/s, 25Mpts | **없음** | USB, LAN, HDMI | SCPI(DHO800/DHO900 Programming Guide). sigrok mainline 미지원. 커뮤니티 fork(fredzo/libsigrok, **TCP/IP 전용**)와 upstream PR #245/#246(2024-08, 병합 여부 미확인) | **LAN + pyvisa-py → OS 무관, 가장 확실**(드라이버 불필요). 실기 미확인 | 동일 | **캡처 후 전송**. 화면 데이터 반복 조회로 준라이브 | (a)만 (AWG 없음) | 장비 내장 디코드 여부·옵션 미확인 | ○○△△ |
| Rigol DHO914S (+PLA2216) | **$769** + 로직 프로브 PLA2216 **$399** — [rigolna](https://www.rigolna.com/products/digital-oscilloscopes/dho900/) | 4ch 125MHz, 12bit | 16(프로브 필요) | USB(+LAN 추정) | SCPI. 디지털 채널은 BYTE 형식 전송 추정(DS7000 가이드 유추, 미확인) | LAN pyvisa-py | 동일 | 캡처 후 전송 | (a) + "S" 모델 내장 신호원(Bode용, 사양 미확인) | 내장 "serial and parallel bus analysis" | ○○○△ — 단 총 $1,168로 "저가" 밖 |
| **Hantek DSO2D15** (DSO2C10) | DSO2D15: EUR 231.42 net (TME), $359–380 (미국 소매) — 알리가 미확인 | 2ch 150MHz, 1GSa/s, 4–8Mpts | 없음 | **USB device**(LAN 없음으로 추정, 미확인) | SCPI(DSO2000 SCPI Programmers Manual). 파형은 비표준 `PRIVate:WAVeform:DATA:ALL?`(커뮤니티 `dso2d15-mcp`, pyvisa-py, 자체 표기 "not tested") | pyvisa-py USBTMC(libusb) — macOS 실기 미확인 | NI-VISA / pyvisa-py | 캡처 후 전송 | (a) + **(b) 내장 25MHz AWG 1채널**(임의파형·버스트·변조). DSO2C10은 AWG 없음 | 장비 내장(종류 미확인) | ○○○△ |
| Siglent SDS804X HD (참고) | **$461** (SDS802X HD $356, SDS814X HD $587) — [siglentna](https://www.siglentna.com/digital-oscilloscopes/sds800x-hd/) | 4ch 70MHz, 12bit, 2GSa/s, 50Mpts | SLA1016($489) 호환 **미확인** | USB device(USBTMC), **LAN(VXI-11, Telnet, Socket)**, 웹 서버 | SCPI. sigrok mainline 목록엔 SDS1000X만 있음, SDS HD는 위 fork | LAN → OS 무관 | 동일 | 캡처 후 전송 | (a) + (b) 옵션 25MHz AWG | **I2C/SPI/UART/CAN/LIN 내장** | ○○△(옵션 시 ○)△ |
| **Digilent Analog Discovery 3** | **$379** MSRP, 학생가 $249 ([Digilent blog](https://digilent.com/blog/wp-json/wp/v2/posts/29912), RS DesignSpark) | 2ch, 125MS/s (분해능 14bit는 미확인) | 16 DIO(LA + 패턴 발생, 미확인 세부) | USB-C | WaveForms SDK(dwf C 라이브러리) + **dwfpy 1.2.0** (MIT, 2025-01-10, Py≥3.8, PyPI). sigrok "planned" | Digilent 가이드: WaveForms는 Apple Silicon 미포팅, **Rosetta 2 권장**(최신판에서 바뀌었는지 미확인). arm64 Python과 x86 dwf 라이브러리 아키텍처 불일치 주의(추정) | 공식 | SDK record 모드로 연속 수집 가능(상한 미확인) | (a) + **(b) AWG 2채널 + 디지털 패턴 출력 + 가변 전원** | WaveForms 앱 프로토콜 도구. SDK의 UART/SPI/I2C/CAN 함수 유무 미확인 | **○○○○** |
| OWON VDS1022I | $100–150 (알리 추정, 미확인) | 2ch 25MHz, **절연형(I)** | — | USB | 공식 SDK 없음. 비공식 florentbr/OWON-VDS1022에 Python API. 유지자 이탈 보고 있음 | 비공식 프로젝트가 macOS Intel·ARM 지원 표기 | 비공식 | 캡처 반복 | (a) | 없음 | △○△△ |

출처(표 공통): [sigrok Supported hardware](https://sigrok.org/wiki/Supported_hardware), [Hantek 6022BE](https://sigrok.org/wiki/Hantek_6022BE), [Hantek 6022BL](https://sigrok.org/wiki/Hantek_6022BL), [fx2lafw](https://sigrok.org/wiki/Fx2lafw), [Kingst LA2016](https://sigrok.org/wiki/Kingst_LA2016), [Kingst LA Series](https://sigrok.org/wiki/Kingst_LA_Series), [DSLogic](https://sigrok.org/wiki/DreamSourceLab_DSLogic), [sigrok Downloads](https://sigrok.org/wiki/Downloads), [Homebrew sigrok-cli](https://formulae.brew.sh/formula/sigrok-cli), [EEVblog: DHO800/SDS HD sigrok fork](https://eevblog.com/forum/testgear/new-sigrokpulseview-hardware-support-(siglent-sds-hd-rigol-dho800-)/msg5556043), [dso2d15-mcp](https://glama.ai/mcp/servers/i2zd5uw993), [TME DSO2D15](https://www.tme.eu/en/details/dso2d15/digital-oscilloscopes/hantek/), [Sipeed Combo 8 wiki](https://wiki.sipeed.com/hardware/en/logic_analyzer/combo8/), [cnx-software SLogic16U3](https://www.cnx-software.com/2025/11/12/69-sipeed-slogic16u3-low-cost-logic-analyzer-supports-3-2-gbps-bandwidth-150-protocols/?amp=1), [Digilent WaveForms Getting Started](https://reference.digilentinc.com/software:waveforms:waveforms-3:getting-started-guide), [dwfpy PyPI](https://pypi.org/project/dwfpy), [florentbr/OWON-VDS1022](https://github.kxpsado.workers.dev/florentbr/OWON-VDS1022).

### 1.1 sigrok 공통 사실 (경로 판단에 중요)
- 최신 **정식 릴리스**: libsigrok 0.5.2, libsigrokdecode 0.5.3, sigrok-cli 0.7.2, PulseView 0.4.2. 이후는 nightly 빌드만 있다([Downloads](https://sigrok.org/wiki/Downloads)). 정식 릴리스가 오래돼 **Kingst 같은 신규 드라이버는 정식판에 없을 가능성이 크다**(Kingst 페이지가 2021-11 이후 코드를 권장하므로 추정).
- macOS 배포: nightly DMG는 **"64 bit x86 only"**(Apple Silicon은 Rosetta 경유, 실기 미확인). 정식 DMG는 PulseView 0.4.2뿐이고 sigrok-cli 정식 DMG는 없다. Homebrew `sigrok-cli`는 0.7.2 arm64 bottle이 있지만 libsigrok 0.5.2 기반이다.
- 라이선스: sigrok-cli·libsigrok는 **GPL-3.0-or-later**(Homebrew formula 표기), fx2lafw 펌웨어는 GPL-2.0+([fx2lafw](https://sigrok.org/wiki/Fx2lafw)).
- 시뮬레이션: **`demo` 드라이버가 있다.** sigrok-cli man page 예시에 "Demo device with 12 channels: D0–D7, A0–A3"가 나오고, 로직 패턴(`pattern=graycode|random …`)을 채널 그룹 단위로 설정한다([man page](https://www.mankier.com/1/sigrok-cli)). 아날로그 채널 파형 옵션은 미확인이다.
- Python 바인딩(SWIG)은 PyPI 패키지가 아니다. PyPI `sigrok` 0.1.2는 별개 프로젝트다(Sol R1 §3).

---

## 2. 핵심 루프 재판정 — 이 장비군에서 "쓰기"와 "실시간"은 무엇인가

### 2.1 쓰기의 세 층

| 층 | 정의 | 해당 제품 | 루프 판정 |
|---|---|---|---|
| (a) 장비 설정 | 트리거 소스·레벨·엣지, 타임베이스, 샘플레이트, arm/single/stop, 채널 on/off. **DUT 상태는 바뀌지 않는다** | 모든 제품 | 쓰기 **△** (r1 기준표 그대로) |
| (b) DUT 자극 출력 | AWG/신호발생기, 디지털 패턴 출력, 가변 전원 출력이 DUT 핀에 연결됨 | AD3(AWG 2 + DIO 16 + 전원), DSO2D15(AWG 1), SDS800X HD(옵션 AWG), DHO9xxS(내장 신호원, 사양 미확인). Kingst PWM은 sigrok 경로에서 불가 | 쓰기 **○** |
| (c) 교차 도메인 자극 | 다른 Loom 도메인의 쓰기(PSU 출력, IO 릴레이 KL15, UART 송신, CAN 송신, UDS 리셋)가 자극이고, 스코프/LA는 그 결과를 관측 | 모든 제품 + PSU/IO/UART/CAN/UDS 중 하나 | **조합 단위로 ○**. 단 스코프/LA 단독 도메인 판정은 △ 유지 |

**판정 원칙 제안**: 스코프/LA는 "관측기 도메인"으로 분류한다. 단독 쓰기는 △로 정직하게 두되, **M1 수용 기준에 (c) 교차 자극 시나리오를 필수로** 넣어 루프 성립을 시연한다. "에이전트가 자극 → 장비가 관측 → 사용자가 같은 화면에서 봄 → 사용자가 트리거 조건을 바꾸거나 Single을 눌러 개입"까지 가야 M1이다.

차량 벤치에서 (c)의 가치(사용자 하루 업무 기준):
- **전원 인가·크랭킹 파형**: `psu cycle` 또는 `io set kl30 on` → 12 V/5 V/3.3 V 레일 상승과 RESET 해제 타이밍 캡처. 부팅 실패 원인 분석의 1순위.
- **CAN 물리층**: `can send`로 프레임을 주입하고 스코프로 CAN_H/CAN_L 차동·비트 타이밍·종단을 확인. LA로는 버스선을 직접 보지 못하고 **트랜시버 TTL 쪽(TXD/RXD)만** 디코드할 수 있다(차동 12 V 계열 직결 불가).
- **MCU↔SBC/PMIC SPI**: 워치독 서비스 SPI를 LA로 디코드하고, `uds reset` 직후 재초기화 시퀀스를 확인.
- **UART 부팅 핀·타이밍**: UART 팀 도메인과 같은 바이트를 LA가 물리 신호로도 보여준다(전기적 문제와 로직 문제 구분).

### 2.2 실시간 표시의 두 종류

| 종류 | 장비 | 화면 갱신 목표(설계 목표, 근거 미확인) | 정직성 규칙 |
|---|---|---|---|
| **연속 스트리밍** | fx2 클론, 6022BE/BL(USB 스트리밍), DSLogic continuous, AD3 record | 디코드 주석(UART 바이트, SPI 트랜잭션): 장비 → 화면 **p95 ≤ 300 ms**. 원시 디지털 파형: 화면 1–2 Hz 갱신, min/max 데시메이션 | 샘플 드롭이 있으면 해당 구간을 빨간 "GAP"으로 표시. 데시메이션 배율("1:4096 표시")을 항상 표시. 원시 캡처는 `.sr` 파일로 무손실 저장. 저장이 못 따라가면 세션 실패로 처리 |
| **단발 캡처 반복** | SCPI 스코프(DHO, SDS, DSO2D15), Kingst(버퍼) | 화면 데이터(1k–10k점) 조회 **1–5 Hz**, 트리거 → 화면 **≤ 1 s**. 전체 메모리(수 Mpts) 전송은 수 초 이상 걸리는 **아티팩트 작업**(진행률 표시) | 각 캡처에 "캡처 #N, 장비 트리거 시각(추정), 전송 n ms, 점 m개 / 장비 메모리 M개" 배지를 붙인다. 캡처 사이 공백은 "관측 안 됨"으로 표시하고 이어 그리지 않는다. **스코프 화면이 더 빨라도 Loom 화면은 실제 수집 주기만 보여 준다** |

전송 시간은 장비·연결별 실측이 없다(미확인). M0에서 LAN SCPI 화면 데이터 왕복(예: 1,000점 `:WAV:DATA?`)과 USBTMC를 각각 측정해 목표를 확정한다.

---

## 3. 저가 조합 추천

| 세트 | 구성 | 대략 총액 | 되는 것 | 안 되는 것 / 리스크 | 백엔드 경로 |
|---|---|---|---|---|---|
| **A. 최저가** | fx2 8ch 24MHz 클론 + Hantek 6022BE | $65–95 (알리 추정, 미확인) | LA 연속 스트리밍 + sigrok 디코더(UART/SPI/I2C, 트랜시버 측 CAN/LIN) 라이브 표시. 2ch 20MHz 스코프(SW 트리거). 두 드라이버 모두 sigrok **정식판(0.5.2)** 에 있어 Homebrew arm64 sigrok-cli로 macOS 경로가 가장 단순(실기 미확인) | 자극 출력 없음 → (c) 교차 자극만. HW 트리거 없음, 20MHz·8bit. **비절연 USB** — 차량 배터리 접지와 노트북 접지 루프 주의. LA 입력 0–5 V급이라 12 V 직결 금지 | `loom la`(sigrok-cli subprocess)가 두 장비 모두 담당. 6022BE 아날로그도 sigrok `hantek-6xxx` |
| **B. macOS 확실 + 차량 디버깅** (추천 기본값) | LAN SCPI 스코프 1대(**Siglent SDS804X HD $461**: CAN/LIN 디코드 내장, LAN socket/VXI-11 / 또는 **Rigol DHO804 $459**) + fx2 클론 | 약 $470 (스코프는 공식가, 클론은 알리 추정) | LAN SCPI는 드라이버 없이 macOS/Windows 같은 경로(pyvisa-py TCPIP). 12bit 4ch로 레일·리셋·CAN 차동 관측. 스코프 내장 CAN 디코드(SDS HD) + LA 디코드. 자극은 PSU/IO/CAN/UART 도메인이 제공 | AWG는 SDS HD 옵션(가격 미확인), DHO804는 없음. 디지털 채널은 SDS HD+SLA1016 호환 미확인 → LA가 대신. 화면 갱신은 단발 캡처 반복(1–5 Hz) | `loom scope` = `loom.instr` pyvisa(PSU와 공유), `loom la` = sigrok-cli |
| **C. 올인원 + 자극** | Digilent Analog Discovery 3 | $379 (학생가 $249) | 단독으로 4요소 모두 ○: AWG 2ch로 DUT 입력 자극, 16 DIO로 디지털 패턴 출력·LA, 2ch 스코프. **dwfpy(MIT)** 로 Python 제어. 공식 macOS 앱 존재 | macOS는 **Rosetta 2 경유**(Digilent 가이드 기준). WaveForms 런타임은 독점이고 사용자가 따로 설치해야 함. arm64 Python ↔ x86 라이브러리 혼용 문제(추정). 125MS/s·2ch, 입력 범위 밖 차량 과도전압은 프로브·감쇠 필요(입력 범위 미확인). sigrok 미지원(planned) | 별도 백엔드 `dwf`(dwfpy). `loom scope`/`loom la` 둘 다의 백엔드로 등록 |

참고 대안:
- **Hantek DSO2D15**(EUR 231 net~$380)는 "B인데 AWG 포함 + 더 쌈"이다. 하지만 USB 전용이고 파형 조회가 비표준 명령(`PRIVate:…`)이며 macOS USBTMC 동작도 미확인이라, B보다 리스크가 크다. 사용자가 이미 가지고 있으면 2순위 드라이버로 지원한다.
- **Kingst LA2016**은 16ch·200MHz가 필요할 때 fx2 대신 쓴다. 단 비트스트림을 직접 추출해야 하고, 버퍼 캡처만 되며, 정식 libsigrok에 없는(추정) 3중 마찰이 있다. LA1010은 sigrok 표에 "untested"다.
- 차량 접지 안전이 중요하면 **절연형 USB 스코프(OWON VDS1022I)** 가 있다. 다만 제어 경로가 비공식이라 추천에서 뺐다. 비절연 장비는 스킬에 접지 경고를 명시한다(§4 i).

---

## 4. 간이 계획 (r1-common §4 a–j 형식)

이름: 스코프 `loom scope`(패키지 `loom.instr`, PSU와 공유), 로직 `loom la`(패키지 `loom.la`). 공통 동사는 `connect/status/arm/capture/wait/measure/decode/events/attach/export`로 맞춘다. 통합 이름(`loom wave`)도 검토했지만, 에이전트·사용자가 "스코프", "로직"으로 생각하고 백엔드·라이선스 경계도 달라 두 이름으로 나눈다. MSO의 디지털 채널(DHO914S)은 `loom scope --digital`로 다룬다.

### a. 핵심 루프 표

| 요소 | 스코프 (`loom scope`) | LA (`loom la`) | 데몬 구성요소 | 사용자 화면 |
|---|---|---|---|---|
| 직접 접근 | `connect bench-scope --visa TCPIP::192.168.0.60::INSTR --driver siglent-sdshd` | `connect la1 --sigrok fx2lafw [--conn …]` / `--sigrok demo` | `VisaBackend` + 스코프 드라이버 프로파일 / `SigrokCliBackend`(sigrok-cli 프로세스 관리, 버전·드라이버 목록 확인) | 상태줄: 장비 IDN, 연결, 수집 모드(single/normal/stream) |
| 읽기 | `capture`, `measure vpp,freq,rise --ch 1`, `wait --trigger --timeout` | `capture --for 2s`, `stream --for 10s --decode uart:baud=115200`, `decode` | 캡처 큐(장비당 미완료 1개), 측정 계산, sigrok 디코더 주석 파서(`-A`), 아티팩트 저장(`.sr`, `.npz`/CSV) | 파형 패널, 디지털 타이밍 레인, 디코드 주석 행·표 |
| 쓰기 | (a) `arm --trigger ch1:fall:2.5V --timebase 1ms`, `run/stop/single` · (b) `awg on --shape square --freq 1k --amp 3.3 --offset 1.65` | (a) `config samplerate=4m` · (b) AD3 `pattern d0:clock:10k` | 위험 게이트(§i), 설정 되읽기 검증 | 설정 변경 행에 origin 색. AWG/패턴 출력 램프 |
| 실시간 표시 | `attach bench-scope` / 웹 viz "Waveform" | `attach la1` / 웹 viz "Logic + Decode" | 이벤트 `scope.capture`(메타 + 화면 데이터 ≤10k점 또는 `blob_ref`), `la.annot`(디코드 주석), `la.chunk`(데시메이트 요약), `*.gap` | 사용자 개입: Single/Run/Stop, 트리거 레벨 드래그, 디코더 추가, AWG 끄기 |

### b. 종단 간 시나리오 — "전원 인가 시 리셋 해제 타이밍"(교차 자극, 세트 B)

```
에이전트: loom scope arm bench-scope --trigger ch1:rise:6V --timebase 2ms --pre 20% --mode single
          (ch1 = 12V 레일 x10 프로브, ch2 = MCU RESET, 벤치 프로파일의 probe_map에 정의)
  화면(≤300 ms 목표): 파형 패널 상단 "ARMED single  ch1 ↑6.0V  2ms/div  [agent]"
에이전트: loom psu out bench on            (R3 → 사용자 뷰어 확인 배너 → 사용자 y)
  장비: 레일 상승 → 트리거 → 캡처
  화면(트리거 → 표시 ≤1 s 목표): 파형 2개 + 타임라인 레인에 psu.cmd(▲user 승인/agent 실행)와 scope.capture 마커가 같은 축에
에이전트: loom scope measure bench-scope --delay ch1:rise:6V,ch2:rise:1.6V --json
  → {"delay_ms": 18.4, "capture": 7, "points": 10000, "source": "screen", "quality": ["single-shot"]}
사용자: 트리거 레벨을 9V로 드래그하고 Single을 누름(개입)
  → {kind:"scope.cfg", origin:"user", trigger_level:9.0} , {kind:"scope.arm", origin:"user"}
에이전트: loom scope events bench-scope --origin user --since @c12
  → 사용자가 트리거를 바꿨음을 인지. 다음 psu cycle 전에 사용자 설정을 덮어쓰지 않고 그대로 사용
```

LA 변형(세트 A, 교차 자극 UART): `loom la stream la1 --for 5s --decode uart:rx=D0:baud=115200` 실행 중 에이전트가 `loom uart send dut "help\r"` → LA 디코드 행에 "68 65 6C 70 0D"가 UART 도메인의 tx 이벤트와 같은 타임라인에 나란히 표시. 사용자가 뷰어에서 디코더 baud를 바꾸면(origin=user) 에이전트가 읽는다.

### c. 실시간성 목표와 고부하
§2.2 표 그대로. 추가 규칙:
- 스트리밍 LA는 sigrok-cli 출력을 데몬이 받아서 (1) 원시를 `.sr`(srzip)로 저장하고, (2) 디코드 주석만 이벤트로 방송하고, (3) 파형은 min/max 데시메이션 요약만 보낸다. 사용자가 확대하면 저장 파일에서 다시 읽는다(LTTB/min-max, viz 요구).
- sigrok-cli가 샘플 오버런으로 실패·중단하면 그 사실을 `la.gap {reason:"overrun"}`로 남기고, 자동 재시작은 사용자에게 보인 다음에만 한다.
- 스코프는 장비당 미완료 질의 1개(Sol R1 규칙 채택). 전체 메모리 전송 중에도 PSU OFF 같은 다른 장비 명령은 막지 않는다(장비별 작업 레인).

### d. CLI 초안

```
loom scope connect <name> --visa RESOURCE --driver rigol-dho|siglent-sdshd|hantek-dso2000|generic
loom scope status <name> [--json]                       # IDN, 채널 설정, 트리거, AWG 상태
loom scope arm <name> --trigger CH:EDGE:LEVEL [--timebase T] [--pre PCT] [--mode single|normal]
loom scope wait <name> --trigger --timeout 10s          # 트리거 될 때까지 경계 대기
loom scope capture <name> [--ch 1,2] [--points 10000|max] [--out cap.npz]   # 화면/메모리, max는 아티팩트
loom scope measure <name> --vpp CH | --freq CH | --delay CHa:EDGE:LVL,CHb:EDGE:LVL | --rise CH
loom scope awg <name> on|off [--shape sine|square|arb] [--freq F] [--amp Vpp] [--offset V]   # (b) 쓰기
loom scope run|stop|single <name>
loom la connect <name> --sigrok DRIVER [--conn SPEC] [--channels D0-D7]
loom la capture <name> --for 2s|--samples N [--samplerate 4m] [--trigger D2=r] [--out cap.sr]
loom la stream <name> --for 10s --decode uart:rx=D0:baud=115200 [--decode spi:clk=D1,mosi=D2,miso=D3,cs=D4] [--max-lines 200]
loom la decode <file.sr|@capture> --decode PD[:opts] [--annot rx-data] [--tail 100]
loom la pattern <name> D0:clock:10k|D1:seq:0101 --for 1s   # AD3 등 패턴 출력 장비만, (b) 쓰기
loom scope|la events <name> [--origin …] [--since CURSOR]
loom scope|la export <name> --capture N --format csv|sr|npz
```

출력 예시(에이전트 컨텍스트 보호: 샘플 원본을 stdout에 쏟지 않는다. 측정치·주석·요약만 내고 원시는 파일 경로로):
```
$ loom la stream la1 --for 3s --decode uart:rx=D0:baud=115200 --max-lines 5
la1 fx2lafw 8ch @4MHz  3.00 s  12.0 Msamples  gaps 0  saved: sessions/…/la1-0007.sr
UART D0 115200 8N1
 14:52:01.120412  rx  "U-Boot 2024.04 (Nov 09 2026)"
 14:52:01.133980  rx  "DRAM:  512 MiB"
 14:52:01.201551  rx  [frame error @ 0x0d]
 … (3 more lines; use --since @a41)
```

### e. 사용자 뷰 (loom.viz 요구)

```
┌ Waveform: bench-scope  SDS804X HD ─ capture #7 single ─ 10k pts (screen) of 25 Mpts ─ xfer 180 ms ─┐
│ ch1 12V  ──────────╱‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾   trig ↑6.0V [user 14:51:40]                                │
│ ch2 RST  ________________________╱‾‾‾‾‾‾‾‾   Δt cursor 18.4 ms                                         │
│ D0 UART  ▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▌▌▌▌▌▌                                                          │
│ decode   ............................[55][2D][42][6F]…                                                │
├──────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ timeline: psu.out on ▲agent(approved by user) │ scope.capture #7 │ uart.rx "U-Boot…"                │
│ [Single] [Run] [Stop] [Trig level ↕] [Decoder +] [AWG: OFF] [Full memory → artifact]               │
└──────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
viz에 요청할 패널 타입:
- `waveform`: 아날로그 다채널, 캡처 배지, 데시메이션 배율, 캡처 사이 공백 표시.
- `logic_lanes`: 디지털 레인과 디코드 주석 행. sigrok 주석 클래스를 행으로 쓴다.
- `decode_table`: 시각·주석·origin.
- 통합 타임라인에 캡처 구간 막대를 넣는다.

TUI(herdr 패인)에는 파형 대신 캡처 요약, 측정치, 디코드 표를 둔다(스파크라인 정도만). 파형은 웹 뷰어가 기본이다.

### f. 주 라이브러리·경로 선택

| 경로 | 장점 | 단점 | 결정 |
|---|---|---|---|
| **sigrok-cli subprocess** | GPL 경계 명확(별도 프로그램, 출력 파싱). 드라이버 수 최다. 디코더 포함. Homebrew/Windows 설치본/Linux 패키지 있음 | 정식 릴리스가 오래됨(0.5.2/0.7.2). 신규 드라이버는 nightly 필요. 출력 파싱 의존. 스트리밍 stdout 동작은 확인 필요(미확인) | **LA 기본 경로** |
| libsigrok Python(SWIG) 바인딩 | 프로세스 내 제어 | PyPI 없음, 빌드 난도, **데몬 import 시 GPL-3.0 결합** | 채택 안 함 |
| **pyvisa + pyvisa-py** (PSU와 공유) | 스코프 SCPI 표준 경로, LAN은 드라이버 불필요, MIT | 벤더 방언(파형 포맷·채널 선택), Hantek 비표준 명령 | **스코프 기본 경로** |
| dwfpy 1.2.0 (MIT) + WaveForms 런타임 | AD3 전 기능(AWG/패턴/전원) | 런타임 독점·별도 설치, macOS Rosetta | AD3 전용 백엔드(`dwf`) |
| sigrok의 스코프 드라이버(rigol-ds, siglent-sds) | 이미 있는 SCPI 구현 재사용 | DHO800/SDS HD는 fork 단계, GPL | 직접 pyvisa 드라이버. sigrok 스코프는 6022BE 같은 USB 스코프만 |

### g. 시뮬레이션 백엔드
- **LA: sigrok `demo` 드라이버**(존재 확인, 8 logic + 4 analog, 패턴 설정). CI에서 sigrok-cli 설치가 필요하다(Linux CI는 배포판 패키지, macOS CI는 Homebrew — 개발 머신 전역 설치는 사용자 결정).
- **LA 순수 Python `SimLa`**(sigrok 없이 CI 가능): SimWorld의 UART/SPI sim 바이트를 비트열 파형으로 합성해 같은 이벤트(`la.annot`)를 낸다. 에이전트가 UART sim에 `send`하면 SimLa 디코드에 같은 바이트가 보인다(교차 자극 시나리오 CI).
- **스코프 `SimScope`**: r1-opus의 SimScpiServer(TCP 5025)에 `:WAV:DATA?`·트리거 상태기계를 추가한다. 파형은 SimWorld PSU 상태로 생성(전원 ON 시 레일 상승 곡선 + RESET 지연). pyvisa-sim은 정적 응답이라 드라이버 방언 단위 테스트용 보조로만 쓴다.

### h. M1 걷는 뼈대 (시뮬레이션 위)
1. `loom scope connect sim --visa TCPIP::127.0.0.1::5025::SOCKET`, `arm` → 뷰어에 ARMED(agent).
2. `loom psu out sim-psu on`(SimPsu, 사용자 확인 경유) → SimScope 트리거 → 캡처 이벤트 → 웹 waveform 패널 표시 + 타임라인에 psu.cmd와 같은 축.
3. `loom scope measure --delay …` 경계 있는 측정 반환.
4. 사용자가 뷰어에서 트리거 레벨 변경 + Single → 에이전트 `events --origin user`로 확인.
5. `loom la stream sim-la --decode uart…`(SimLa) 중 `loom uart send` → 디코드 행 표시 → 사용자가 디코더 baud 변경 → 에이전트가 읽음.

수용 기준: 위 5단계 자동 테스트가 통과한다. 캡처 배지(점 수·전송 시간)가 표시되고 캡처 사이를 이어 그리지 않는다. 스트림 중 인위적 드롭을 주입하면 GAP이 표시된다. 원시 `.sr`/`.npz` 파일 샘플 수가 생성 수와 같다(무손실).

### i. 안전/가드레일

| 동작 | 등급(R0–R5, r1-opus §5.2) | 규칙 |
|---|---|---|
| capture/measure/decode/status | R0 | |
| 트리거·타임베이스·샘플레이트 설정, run/stop/single | R1 | 사용자가 같은 장비 앞패널·뷰어로 조작 중이면(최근 30 s 내 origin=user 변경) 에이전트 설정 변경은 확인 요청 — 사용자 설정을 덮어쓰지 않기 |
| 전체 메모리 전송 | R1 | 아티팩트 작업, 크기 상한 |
| **AWG on / 파라미터 변경** | **R2**(프로파일 한계 안) / **R3**(프로파일이 해당 출력이 연결된 넷을 `sensitive`로 표시하거나 한계 밖이면 거부) | 연결 시 AWG 기본 OFF. 진폭·오프셋·주파수 한계는 벤치 프로파일. **출력 임피던스 50 Ω 설정과 Hi-Z 부하 불일치로 진폭이 2배**가 되는 문제 → 드라이버가 부하 설정을 명시적으로 확인 |
| **디지털 패턴 출력(AD3 DIO)** | R2(프로파일 `dio_out` 허용 핀) / 그 외 R3 | **DUT가 이미 구동하는 핀에 출력하면 버스 충돌·손상** → 프로파일에 핀별 방향 선언. 기본은 전 핀 입력 |
| 가변 전원 출력(AD3 ±5 V) | R3 | PSU 도메인 규칙 준용 |
| 하드웨어 안전(소프트웨어로 못 막음) | — | 스킬과 뷰어 상시 배너에 경고를 둔다: **비절연 USB 스코프의 접지 클립은 노트북 USB 접지와 연결됨**(차량·PSU 음극 외 지점에 물리면 단락). LA 입력 전압 범위(fx2 클론 0–5 V급) 밖의 12 V 넷 직결 금지 → `probe_map`에 넷 전압을 적고, 넷 정격이 장비 정격을 넘으면 `connect` 때 경고 |

dry-run은 `awg`/`pattern`/설정 명령에서 SCPI 문자열·sigrok 옵션과 판정 등급을 출력한다.

### j. 리스크

| 리스크 | 영향 | 대응 |
|---|---|---|
| sigrok 정식 릴리스 정체(0.5.2) — Kingst/DHO800/SDS HD는 nightly나 fork에만 있음 | 지원 장비가 설치 경로에 따라 달라짐 | 공식 지원 목록을 "정식 sigrok-cli 0.7.2 + libsigrok 0.5.2에서 되는 드라이버"(fx2lafw, hantek-6xxx, dslogic, demo)로 시작. nightly는 사용자 선택 |
| macOS sigrok nightly x86 전용(Rosetta), Homebrew는 구버전 | Apple Silicon 실기 동작 미확인 | M0에서 사용자 승인 하에 Homebrew 또는 nightly로 fx2 클론 실측(이번 라운드는 전역 설치 금지로 미실행) |
| **GPL-3.0** (libsigrok, sigrok-cli) | 데몬에 import하면 Loom 배포 조건 오염 | subprocess 경계만 사용. 바인딩 import 금지를 계약에 명시. 펌웨어(fx2lafw GPL-2+, Kingst/DSLogic 추출 비트스트림)는 **Loom이 재배포하지 않음** — 사용자가 설치·추출 |
| 벤더 펌웨어 추출(Kingst, DSLogic) 라이선스 회색지대 | 배포·지원 부담 | 지원 등급을 "사용자 추출 시 동작"으로 표기 |
| Hantek DSO2000 비표준 SCPI, USB 전용 | 품질·macOS USBTMC 불확실 | 2순위 드라이버. 보유자만 대상 |
| 스코프 SCPI 파형 전송 속도 미확인 | 갱신 1–5 Hz 목표 미달 가능 | M0 실측 후 목표 조정, 화면에 실측 표시 |
| 6022BL 모드 전환이 물리 버튼 | 에이전트 단독 전환 불가 | 사용자 수동 단계로 처리(시나리오 `manual`), 또는 6022BE + fx2 클론 분리 구성 권장 |
| AD3 런타임 독점·Rosetta | 장기 유지, arm64 Python 혼용 문제 | `dwf` 백엔드는 선택 extra. 아키텍처 일치 검사를 `connect` 때 수행 |
| 접지·과전압으로 인한 HW 손상 | 장비·DUT 파손 | §i 경고, 프로파일 `probe_map`, 절연형 장비 권장 문구 |

---

## 5. 최종 판단

**결론: 기존 추천 1–4위는 유지하고, 5위를 "관측 계측(스코프/LA)"로 넣는다. XCP는 조건부 예비로 내린다. 구조는 `loom.instr` 확장(`loom scope`) + 별도 `loom la`(sigrok)로 나눈다.**

| 순위 | 도메인 | 변경 |
|---|---|---|
| 1 | 벤치 전원·점화 (`loom psu` + `loom io`) | 유지 |
| 2 | DLT (`loom dlt`) | 유지(1과 병렬 착수) |
| 3 | 타깃 셸 (`loom shell`) | 유지 |
| 4 | 디버그 프로브 (`loom debug`) | 유지 |
| **5** | **관측 계측: `loom scope`(loom.instr, SCPI/pyvisa, AD3는 dwf) + `loom la`(sigrok-cli subprocess)** | **신규. XCP를 밀어냄** |
| 예비 | XCP (Ethernet 먼저) | 사용자 Q6 답이 "있음"이면 5위 다음으로 복귀 |

근거:
1. **사용자 관심이 명시적이다.** XCP는 업무에 있는지조차 미확인이었다. 02 원칙의 "가치가 큰 것"은 사용자 근거가 우선한다.
2. **루프는 교차 자극으로 닫힌다.** 이미 추천된 PSU/IO/UART/CAN이 자극을 제공한다. 그래서 스코프/LA는 시나리오 러너와 결합할 때 벤치 디버깅 가치가 가장 크다(전원 인가 → 레일·리셋 캡처 → 부팅 로그 대조). 단독 쓰기가 △인 점은 정직하게 표시하고, M1 수용 기준으로 교차 루프를 강제한다.
3. **구현 비용이 작다.** 스코프는 PSU와 pyvisa 백엔드, 드라이버 프로파일, 폴링/아티팩트 구조를 공유한다. LA는 sigrok-cli가 드라이버와 디코더를 제공하므로 Loom은 프로세스 관리, 출력 파싱, 이벤트화만 하면 된다.
4. **왜 1–4위를 밀어내지 않나**: 단독 루프가 반쪽이고, HW 구매가 필요하며, 스코프는 단발 캡처라 실시간성이 약하다. 1–4위는 단독으로 4요소가 모두 ○다.
5. **왜 한 도메인으로 합치지 않나**: 백엔드(pyvisa vs sigrok subprocess), 라이선스 경계(MIT vs GPL-3.0 subprocess), 라이브 방식(단발 캡처 vs 연속 스트리밍)이 다르다. 공통점(파형 패널, 캡처 이벤트 스키마, 동사 체계)은 viz 패널과 CLI 규약으로 공유하면 된다. AD3처럼 둘 다 되는 장비는 두 도메인 모두에 백엔드로 등록한다.

착수 순서 제안: PSU+IO M1 → **`loom scope` M1**(SimScope, PSU 백엔드 재사용, 비용 최소) → 시나리오 러너 M1 → `loom la` M1(SimLa + sigrok demo). 이 사이에 DLT·Shell·Debug는 원래 순서대로 병렬로 진행한다. 실장비 M2는 사용자 보유 장비 답변을 보고 결정한다(질문 추가: "스코프/LA를 이미 가지고 있나? 없으면 세트 A/B/C 중 무엇을 살 의향인가" — 기본값 세트 B).

<!-- END R2b -->
