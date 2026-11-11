# R2c — 저가 스코프/LA 사실 검증 (Sol)

기준일: 2026-11-09 KST. 대상: [r2b-opus-scope-la.md](r2b-opus-scope-la.md).
[user-notes.md](user-notes.md)의 저가·디지털 채널·PC 제어 요구를 반영했다.
최우선은 [핵심 원칙](../02-core-principle.md)의 장비 접근→읽기→쓰기→사용자 동시 표시·개입이다.
R1/R2/R2b 및 리드의 plan.md는 수정하지 않았다. 제품 구현·전역 설치·실 HW 접속도 하지 않았다.
원자료·스파이크는 `spikes/explore/r2c-sol/`에 있고 `sources.json`에 URL/조회 결과를 남겼다.
**결론: 관측 계측의 5위 승격과 scope/instr·la 분리에 동의한다. 단 BL 지원, AD3 아키텍처, LA 동시 기록 경로는 수정해야 한다.**

## 1. 틀린 사실(근거·확신도)

### F1. Hantek 6022BL 스코프 모드 “sigrok 미지원”은 소스와 다르다

R2b는 개별 wiki의 오래된 문구를 채택했다. 그러나 **정식 libsigrok 0.5.2**의 `hantek-6xxx/api.c`에는 다음 profile이 있다.

- 6022BE: VID/PID `04b4:6022`, firmware `fx2lafw-hantek-6022be.fw`.
- 6022BL: VID/PID `04b4:602a` 및 Windows 변형 `04b5:602a`, firmware `fx2lafw-hantek-6022bl.fw`.
- 둘 모두 analog scope driver에 등록된다. 따라서 “LA만 지원”을 현재 배포 소스의 사실로 쓰면 안 된다.
- 이는 지원 코드 존재의 확인이며 실제 구매품·복제 chip·firmware·macOS 동작 보증은 아니다.
- BL이 scope/LA를 동시에 쓰는 MSO가 아니고 H/P 상태에 따라 VID/PID가 바뀐다는 제한은 유지한다.
- 근거: [0.5.2 태그 소스](https://github.com/sigrokproject/libsigrok/blob/libsigrok-0.5.2/src/hardware/hantek-6xxx/api.c), [fx2lafw 지원표](https://sigrok.org/wiki/Fx2lafw).
- **확신도: 높음.** 개별 BL wiki의 문구 충돌도 원자료에 보존했다.

### F2. “AD3 macOS는 Rosetta, arm64 Python과 x86 dwf 불일치”는 GUI와 runtime을 혼동했다

[WaveForms 3.25.1 공식 manual §9](https://files.digilent.com/manuals/WaveForms/3.25.1/main.html)은 둘을 구분한다.

- WaveForms **GUI application**: Intel 64-bit이며 Apple Silicon에서 Rosetta 사용.
- **dwf.framework**: dwf library와 Adept Runtime이 **x86_64 및 arm64**와 호환된다고 명시한다.
- 따라서 Loom의 arm64 Python SDK 경로를 Rosetta 필수로 분류할 근거가 없다. 실제 설치 framework의 아키텍처는 별도 확인해야 한다.
- Homebrew cask API의 현재 배포 버전도 3.25.1이다. 설치는 하지 않았다.
- 최신 release-note 원문 URL들은 접근 실패했다. **최초 native 지원 도입 버전/시점은 미확인**이며 3.25.1 공식 installer 설명으로 현 지원을 판단했다.
- **확신도: 높음**(공식 문서상 runtime 지원), **미확인**(이 머신의 runtime/HW 실행).

### F3. `demo`의 기본 채널을 8 logic+4 analog로 고정하면 틀린다

sigrok-cli man의 12-channel 예시는 예시 출력이다. libsigrok 0.5.2 `demo/api.c` 기본 상수는 **8 logic+5 analog**다.
analog 이름은 A0~A4이고 scan 옵션으로 개수를 바꿀 수 있다.
아날로그 패턴은 square/sine/triangle/sawtooth/random, amplitude·offset의 get/set도 존재한다.
즉 “analog 옵션 미확인”은 소스 확인으로 보완할 수 있다.
근거: [demo 0.5.2 api.c](https://github.com/sigrokproject/libsigrok/blob/libsigrok-0.5.2/src/hardware/demo/api.c), [protocol.h](https://github.com/sigrokproject/libsigrok/blob/libsigrok-0.5.2/src/hardware/demo/protocol.h).
**확신도: 높음.** 이 환경에서는 sigrok-cli 미설치라 실제 `--show` 출력은 얻지 않았다.

### F4. stock sigrok-cli 한 번으로 raw `.sr` 저장과 `-P/-A` live annotation을 동시에 얻는 설계는 성립하지 않는다

0.7.2 `session.c:datafeed_in()`은 logic packet을 `srd_session_send()`로 보낸다.
하지만 일반 output module 실행 조건은 **`!opt_pds`**다. decoder를 켜면 raw output 경로가 건너뛰어진다.
따라서 `-o cap.sr -O srzip -P uart ...`가 raw 기록과 live 주석을 동시에 해 준다고 전제하면 안 된다.
`decode.c:show_pd_annotations()`는 주석마다 `printf` 후 **`fflush(stdout)`**를 호출한다.
**주석 live stdout 가능**과 **동시 원시 기록 가능**은 서로 다른 사실이다.
근거: [sigrok-cli 0.7.2 session.c](https://github.com/sigrokproject/sigrok-cli/blob/sigrok-cli-0.7.2/session.c), [decode.c](https://github.com/sigrokproject/sigrok-cli/blob/sigrok-cli-0.7.2/decode.c).
**확신도: 높음**(소스 경로). 실시간 pipe 실행·처리량은 미검증이다.

### F5. DSO2D15 파형 경로를 커뮤니티의 `PRIVate:...` 하나로 규정할 수 없다

Hantek 공식 다운로드의 DSO2000 SCPI manual p42~43은 **`:WAVeform:DATA:ALL?`**를 문서화한다.
`#9` 길이와 Hantek 전용 packet header·진행 상태를 설명하므로 Rigol의 일반 waveform parser와 같지는 않다.
R2b의 `PRIVate:WAVeform:DATA:ALL?`는 해당 문서의 표기가 아니다. firmware별 별칭/비공개 경로 여부는 실기 확인 대상으로 둔다.
공식 PDF는 여러 채널을 포괄하는 설명도 포함하므로 DSO2D15 firmware와의 완전 일치를 보장하지 않는다.
근거: [Hantek 공식 SCPI PDF](https://www.hantek.com/uploadpic/hantek/files/20211231/DSO2000%20Series%C2%A0SCPI%C2%A0Programmers%C2%A0Manual.pdf); 관련 페이지 PNG도 렌더·확인했다.
**확신도: 높음**(공식 명령 존재), **미확인**(보유품 firmware에서 실제 응답).

### F6. AD3를 “sigrok planned”라고 특정할 근거는 부족하다

현재 [Supported hardware](https://sigrok.org/wiki/Supported_hardware)의 빨간 planned 표시는 **원래 Digilent Analog Discovery**다.
AD3 전용 wiki URL은 404였으며 이 사실만으로 AD3 driver 전체 부재를 증명하지는 않는다.
정확한 표기는 **“AD3의 mainline sigrok 지원 확인 못함; 공식 WaveForms SDK 사용”**이다.
**확신도: 높음**(wiki가 가리키는 모델), **미확인**(외부 fork 전체의 AD3 지원).

## 2. 확인된 사실

### 2.1 sigrok 배포·플랫폼

| 항목 | 확인 결과 | 근거 |
|---|---|---|
| 최신 정식 릴리스 | libsigrok 0.5.2, sigrok-cli 0.7.2; decode 0.5.3, PulseView 0.4.2 | [공식 Downloads](https://sigrok.org/wiki/Downloads) |
| 공식 macOS nightly DMG | 페이지에 “64 bit x86 only” 명시 | 같은 Downloads; native arm64 배포로 해석 금지 |
| Homebrew sigrok-cli | stable 0.7.2, arm64 bottle 다수 존재 | [formula JSON](https://formulae.brew.sh/api/formula/sigrok-cli.json) |
| Homebrew libsigrok | stable 0.5.2, revision 5, arm64 bottle 존재 | [formula JSON](https://formulae.brew.sh/api/formula/libsigrok.json) |
| Apple Silicon 판단 | Homebrew는 native 경로 후보, 공식 x86 DMG와 별개 | bottle 존재≠USB 장비 정상 동작 확인 |

이 머신에는 sigrok-cli가 없다. brew 설치·nightly 실행·실기 scan은 하지 않았다.
문서와 태그 소스 확인은 가능했으므로 CLI 미설치가 driver 사실 검증을 막지는 않았다.

### 2.2 driver별 지원 경계

| 장비/driver | 정식 0.5.2 / 현재 upstream 확인 | 계획 반영 |
|---|---|---|
| fx2lafw clones | 0.5.2 driver 존재, clone 목록과 firmware upload 경로 있음 | 정확한 VID/PID·firmware·channel width를 M0에서 확인 |
| Hantek 6022BE/BL | 0.5.2 hantek-6xxx에 둘 다 scope profile 존재 | F1대로 수정; BL 모드 전환/동시 수집 제한 유지 |
| Kingst LA2016 | 0.5.2 Makefile에 driver 없음, 태그의 해당 api.c는 404; master에는 존재 | “없을 가능성”이 아니라 **정식 0.5.2 미포함 확인** |
| Kingst 수집 방식 | wiki는 SDRAM capture 경로와 USB direct-stream 미구현을 구분 | fx2와 같은 연속 live latency 약속 금지 |
| DSLogic | master profile에 original/Pro/Plus/Basic, U2Basic PID 0x0029 없음 | U2Basic를 Basic과 같은 지원 모델로 판매명만 보고 선택 금지 |
| DSLogic 새 revision | wiki의 U2Basic/U2Pro16/U3Pro16 및 pgl12 계열은 No | exact PID/revision을 지원 matrix에 고정 |
| Rigol DHO800 | rigol-ds master 모델표에서 DHO800 확인 못함 | SCPI backend 우선; fork/PR을 mainline 지원으로 표시 금지 |
| Siglent SDS HD | siglent-sds master 모델표에 SDS800X HD/HD 계열 확인 못함 | 최신 SCPI guide의 HD 명령을 직접 사용 |

태그·master 원문은 `hantek-052.txt`, `lib-make-052.txt`, `kingst-master.txt`, `dslogic-master.txt`, `rigol-master.txt`, `siglent-master.txt`에 저장했다.
추가 근거: [Kingst wiki](https://sigrok.org/wiki/Kingst_LA2016), [DSLogic wiki](https://sigrok.org/wiki/DreamSourceLab_DSLogic), [libsigrok master](https://github.com/sigrokproject/libsigrok/tree/master/src/hardware).
Kingst firmware/FPGA 추출, DSLogic firmware 준비는 사용자 설치 경로이며 Loom의 무단 재배포를 전제하지 않는다.

### 2.3 CLI 스트리밍의 정확한 의미

- `--continuous`는 종료 지시까지 수집하며 **모든 장비가 지원하는 옵션은 아니다**.
- `-O <format>`은 출력 형식, `-o <file>`은 파일 경로, `-P`는 decoder 설정, **`-A`는 표시할 annotation 선택**이다.
- `-A` 자체가 decoder를 생성하거나 binary raw output을 live annotation으로 바꾸는 옵션은 아니다.
- `-O binary`는 logic raw stream, `-O analog` 등은 해당 output module의 packet 처리 방식에 따른다.
- stock session code는 output chunk를 쓸 때 flush하며, decoder는 packet마다 입력을 받고 annotation마다 stdout을 flush한다.
- 따라서 **decoder가 완결된 주석을 생성하는 시점에** 캡처 종료 전 pipe로 내보낼 수 있다는 소스 근거가 있다.
- 프레임 종결/EOF를 기다리는 decoder·USB buffering·긴 hardware capture에는 같은 지연을 보장할 수 없다.
- sample index→시간 변환에는 유효 samplerate가 필요하다. 주석 출력 시각을 장비 사건 시각으로 쓰지 않는다.
- daemon job의 `--continuous`에는 외부 TTL·cancel·byte cap이 필요하고 agent CLI는 bounded 요약만 반환한다.
- 근거: [0.7.2 man](https://github.com/sigrokproject/sigrok-cli/blob/sigrok-cli-0.7.2/doc/sigrok-cli.1), F4의 session/decode 소스.

### 2.4 SCPI 스코프: 전송과 파형 명령

| 모델 | 문서상 제어·명령 | 남은 미확인 |
|---|---|---|
| Rigol DHO800 | LAN/LXI·USB remote; `:WAV:SOUR`, `:WAV:MODE NORM/MAX/RAW`, `:WAV:FORM BYTE/WORD/ASC`, `:WAV:DATA?`, `:WAV:PRE?`, `:WAV:STAR/STOP` | VXI-11 실제 연결, firmware별 raw socket 5555 실기 왕복 |
| Siglent SDS800X HD | 공식 guide에 SDS800X HD 신명령 firmware 1.1.3.1 명시; LAN VXI-11/socket/Telnet, socket 5025/Telnet 5024; `:WAV:SOURCE`, `:WAV:WIDTH`, `:WAV:START`, `:WAV:POINT`, `:WAV:PRE?`, `:WAV:DATA?` | 보유품 firmware와 항목별 실제 동작·속도 |
| Hantek DSO2D15 | USB Host/USB Device, rear USB-B PC remote·NI IO 예시; 공식 waveform command는 F5 참조 | 실제 USBTMC descriptor/pyvisa-py macOS 왕복 |

Rigol RAW memory는 STOP 상태에서 읽도록 문서화돼 있다. 화면 NORM과 full-memory RAW는 서로 다른 작업이다.
Rigol **5555**는 공개 사용 구현에서 쓰이나 이번에 읽은 제조사 PDF에서 번호를 직접 확인하지 못했다.
따라서 이를 제조사 문서로 확정한 값으로 포장하지 않고 configurable endpoint+`*IDN?` gate로 둔다.
Hantek 공식 interface 목록에는 LAN이 없다. USB-only 모델로 계획하는 것은 타당하나 모든 제품 revision을 실측한 것은 아니다.
Siglent guide의 일부 socket 예문은 다른 SDS 모델명을 재사용한다. 모델/firmware 확인 없이 문서 한 줄만으로 모든 동작을 보증하지 않는다.

근거: [Rigol Programming Guide §3.28](https://download.rigol.com/en/Manual/Digital%20Oscilloscope/DHO800/DHO800900_ProgrammingGuide_EN.pdf), [SDS800X HD Programming Guide](https://www.siglent.com/wp-content/uploads/2024/09/SDS800XHD_Series_ProgrammingGuide_EN11G.pdf), [Hantek User Manual](https://www.hantek.com/uploadpic/hantek/files/20250326/DSO2000%20Series%20Digital%20Storage%20Oscilloscope%20User%20Manual20250326154940.pdf).
[PyVISA-py 공식 설치 문서](https://pyvisa-py.readthedocs.io/en/latest/installation.html)는 TCPIP와 USB INSTR/RAW 경로를 제공한다.
TCPIP는 socket 기반, USB는 PyUSB+libusb 등의 native library가 필요하다. macOS에서 **원리상 경로가 있음**과 해당 스코프 검증 완료는 구분한다.

### 2.5 AD3 사양·가격

| 항목 | 공식 확인 |
|---|---|
| analog input | 2 differential ch, 14-bit, 최대 125 MS/s/ch, ±25 V; flywire 9 MHz, BNC adapter 사용 시 30+ MHz |
| AWG | 2 ch, 14-bit, 최대 125 MS/s/ch, ±5 V; flywire 9 MHz, BNC adapter 사용 시 12 MHz |
| digital | 16 DIO, 3.3 V CMOS, 최대 125 MS/s/ch; 입력 5 V tolerant, 패턴 출력 가능 |
| 전원 | +0.5~+5 V / -0.5~-5 V, AUX 전원 사용 시 채널당 최대 800 mA **또는 2.4 W 중 먼저 도달한 한계** |
| Python | dwfpy 1.2.0, 2025-01-10, MIT, Python≥3.8; vendor dwf runtime은 별도 |
| 소유권 | 같은 AD3를 scope/la/awg에 등록해도 하나의 device worker/handle을 공유해야 함 |

근거: [AD3 datasheet Rev1, May 2025](https://files.digilent.com/datasheets/Analog-Discovery-3-Datasheet.pdf), [dwfpy PyPI](https://pypi.org/project/dwfpy/1.2.0/), F2 공식 runtime 설명.
125 MS/s는 device sampling 사양이며 USB로 모든 raw sample을 무한 스트리밍하는 속도가 아니다.
AD3 전원은 12 V 차량 PSU 대체물이 아니다. ±5 V 및 전력·입출력 정격을 bench profile에 고정한다.

| 제품 | 공식 표시가 재확인 | 주의 |
|---|---|---|
| DHO804 | **US$459** | [Rigol 모델 표](https://www.rigolna.com/products/digital-oscilloscopes/dho800/); DHO802 시작가/할인과 혼동 금지 |
| SDS804X HD | **US$461** | [Siglent 모델 표](https://www.siglentna.com/digital-oscilloscopes/sds800x-hd/) |
| AD3 | **US$379** | [Digilent 제품](https://digilent.com/shop/analog-discovery-3/); 조회 경로에 따라 KRW 등 지역 통화로 표시됨 |

세금·배송·프로브/BNC adapter·AUX PSU는 별도 확인한다. 학생가 $249는 일반 구매 기본값으로 사용하지 않는다.
알리 실매물·판매자·배송 포함 가격은 이번에 검증하지 못했다. R2b의 fx2/Hantek 가격과 세트 A/B 합계는 **추정·미확인**을 유지한다.

## 3. 보완

### 3.1 짧은 스파이크 결과

| 검사 | 실제 결과 | 증명하지 않은 것 |
|---|---|---|
| dwfpy 1.2.0 설치/import | venv에 설치, arm64 Python 3.14.1에서 import 성공 | runtime·USB·AD3 사용 성공 |
| `Application.get_version()` | `FileNotFoundError`: `/Library/Frameworks/dwf.framework/dwf` 없음 | 아키텍처 불일치 오류가 아님 |
| pyvisa-sim scope YAML | `*IDN?`, source CHAN2 set/query, `:WAV:DATA?` binary block `[0,1,2,3]` 조회 성공 | 실제 SCPI TCP·trigger·속도·동적 DUT waveform |
| sigrok-cli 실행 | 미설치, 실행하지 않음 | 실제 `demo`·pipe latency·HW 안정성 |

`dwfpy-result.json`, `scope-sim-result.json`, `scope-sim.yaml`, `scope-sim-check.py`가 근거다.
YAML은 정적 waveform과 mutable property를 표현할 수 있다. 동적 부하·trigger 상태기계가 필요할 때만 별도 SimScope TCP server를 추가한다.
소스/공식 문서 확인과 실제 실행의 증명 수준을 섞지 않는다.

### 3.2 LA M0에 raw 기록+decode 분기 시험을 추가

한 acquisition owner의 raw stream을 받아 **raw+metadata 기록**, **decoder 입력**, **UI 요약**으로 분기하는 구조를 검증한다.
가능안은 binary stdout tee→별도 stdin decoder subprocess이며, `-I binary`의 channel/samplerate와 재실행 경계를 맞춰야 한다.
이 경로의 낮은 지연·동시 기록은 아직 시험하지 않았다. sigrok-cli 프로세스 둘이 같은 USB 장비를 열게 하면 안 된다.
최종 `.sr` 패키징은 별도 단계로 둘 수 있다. 캡처 중 부분 srzip 파일을 랜덤 접근 가능한 완성 artifact처럼 읽지 않는다.
M0 gate: 종료 전 annotation 수신, raw byte/sample count 일치, decoder stall 중 기록 지속, TTL 취소·정리·gap 확인.
일반 digital waveform에 LTTB를 그대로 쓰면 edge가 사라진다. transition 보존 요약·toggle density·원본 sample span을 제공한다.
subprocess는 장애/의존성 격리 경계다. GPL 프로그램이나 firmware를 배포할 때의 의무가 없어지는 보장은 아니다.

### 3.3 사용자 개입과 타임라인의 수용 기준

관측 설정·재arm·취소도 실제 장비 명령이며 TX/RX·결과·origin을 같은 stream에 남긴다.
수집 중 decoder 변경은 live 가능한지, 새 capture/decoder epoch가 필요한지 backend capability로 명시한다.
다른 도메인의 자극과 capture를 동일 `run_id/correlation_id`에 묶고 사용자 설정 변경 뒤 pending agent 설정을 취소한다.
두 채널의 RESET 지연은 같은 scope sample clock에서 계산한다. PSU RPC 시각과 trigger 시각의 정밀 동기화를 별도로 보증하지 않는다.
USB/네트워크 buffering·unsynchronized clock을 고려한 오차/추정 라벨이 필요하다.
1~5 Hz waveform 표시·300 ms annotation 목표는 **미측정 SLO**이며 M0 결과로 조정한다.

## 4. 결론 이견

**사용자가 명시적으로 관심을 밝힌 계측을 XCP보다 앞에 두는 데 동의한다.**
단독 DUT 자극이 없는 모델은 관측 확장으로 명시하고, 상위 추천은 **PSU/UART/CAN 자극+scope/LA의 조합 M1**로 수용한다.
에이전트 자극·수집·동일 화면·사용자 재설정/중단·에이전트의 user 이벤트 확인을 함께 시연해야 한다.
AD3는 SDK 출력까지 검증하면 단독 네 요소를 만들 수 있는 후보지만, 현재 이 머신에서는 runtime 미설치다.

| 세트 | 내 판단 | R3 반영 |
|---|---|---|
| A: fx2+6022BE | 최저 예산 후보에는 동의, 실제 알리 가격·USB 안정성 미확인 | BL도 scope 지원 후보로 복귀 가능; MSO·자동 모드 전환처럼 추천하지 않기 |
| B: LAN scope+fx2 | **기본 조사/지원 우선순위에 동의** | “macOS 확실” 대신 “native USB driver 부담이 적은 경로, 실기 미검증”; scope 공식가+clone 미확인 분리 |
| C: AD3 | Rosetta-only 감점 제거, 적극 비교할 올인원 후보 | 14-bit/16DIO/2AWG/전원 한계 명시, accessory 총비용·runtime gate 확인 |

`loom scope`를 `loom.instr` 확장으로, `loom la`를 sigrok-cli backend 도메인으로 두는 구조에 동의한다.
AD3의 scope/la/출력은 단일 shared owner를 써야 하고, 두 plugin 등록이 두 물리 open으로 이어져서는 안 된다.
LA는 “프로세스 실행·파싱만 하면 됨”보다 작업이 크다. F4의 recording 분기·metadata·backpressure·decoder epoch가 필수다.
XCP는 조건부 예비 유지에 동의하며 이번 사용자 관심만으로 기존 IO의 읽기 gate가 자동 해결되는 것은 아니다.
R3 즉시 반영 우선순위: **BL scope 지원 정정 → dwf native runtime 정정 → raw/decode 분기 gate → Hantek 공식 명령 → 모델별 실기/가격 미확인 유지**.

<!-- END R2c -->
