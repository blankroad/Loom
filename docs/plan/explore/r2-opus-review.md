# Explore R2 — explore-opus의 r1-sol.md 교차 리뷰

작성: explore-opus, 2026-11-09
대상: `docs/plan/explore/r1-sol.md` (1053줄). 비교 기준: 내 초안 `r1-opus.md`, `02-core-principle.md`(최우선), `00-brief.md` §3, `explore/brief.md`, `tasks/r1-common.md`.
무게: 아키텍처·플러그인 구조, 사용자/에이전트 UX, 안전 규칙, 핵심 루프 가치 판단.

**한 줄 결론**: Sol 초안은 **사실 조사와 정직성(ACK와 실제 반영 구분, gap 출처 분류, late viewer 처리)이 내 초안보다 낫고**, 내 초안의 사실 오류 2건(pyOCD+PyLink 2.x 동시 설치, DLT 레벨 변경 반영 지연)을 바로잡는다. 반대로 **확인(승인) 주체가 모호해 에이전트가 스스로 승인할 수 있는 구멍**, 디버그 halt를 R2로 둔 등급, 교차 도메인 시뮬레이션·시나리오 러너 UX·벤치 프로파일 부재, ○ 판정 기준이 느슨해 FlexRay·LIN이 부풀려진 점이 약하다. 최종 추천은 §7 첫 줄.

---

## 1. 틀린 사실

Sol 초안에서 명백한 사실 오류는 적다. 아래는 오류 또는 부정확한 표현과, 반대로 **Sol이 맞고 내 초안이 틀린 것**(리드가 통합할 때 꼭 반영)을 함께 적는다.

### 1.1 Sol 초안의 오류·부정확

| # | 위치 | Sol 서술 | 사실 | 근거 | 확신도 |
|---|---|---|---|---|---|
| S1 | §3 평가표, USB 릴레이 행 | 총점 79 (4,3,3,4,4,5), 루프 ○/△/○/○ | Sol 자신의 공식 `10×(A+R+W+V)+4U+3F+2L+H+O+C`로 계산하면 35+16+9+8+4+4+5 = **81**. 다른 21개 행은 모두 재계산이 맞았고 이 행만 틀림 | Sol §1 공식으로 직접 재계산 | 높음 |
| S2 | §4.4 g, §0 | "QEMU GDB SRAM 왕복 통과"를 디버그 M1 시뮬레이션 근거로 씀 | 스파이크 기록 자체가 `"scope": "QEMU GDB RSP halted SRAM only; no target firmware/RTT/SWD throughput"` — **CPU가 멈춘 상태**의 메모리 왕복. 디버그 도메인의 핵심 가치인 "실행 중 변수 라이브 플롯"·RTT는 이 경로로 시연 불가. Sol도 §4.4 g에서 펌웨어가 필요하다고 인정하나, M1이 **크로스 툴체인으로 빌드한 펌웨어**에 의존하게 된다는 점은 적지 않음 | `spikes/explore/backend-run.log` 12행 | 높음 |
| S3 | §4.4 g | QEMU를 시뮬레이션 기반으로 제시 | 스파이크에 쓴 QEMU는 `/opt/homebrew/bin/qemu-system-arm` — 사용자 전역 설치본(Homebrew). CI·Windows에서 같은 경로를 쓰려면 별도 설치가 필요하고 `.venv`로 해결되지 않음. 계획에 이 의존성이 명시되지 않음 | `which qemu-system-arm` | 높음 |
| S4 | §4.4 c 끝/§4.5 g | "macOS의 python-can virtual은 프로세스 내부 공유 범위다" | 맞는 말이지만, 프로세스 간 가상 버스로 python-can `udp_multicast` 인터페이스가 있어(설치본 `can/interfaces/udp_multicast` 확인) "macOS에서 프로세스 간 CAN sim 불가"처럼 읽힐 여지는 틀림. XCP/CAN sim을 별도 프로세스로 돌릴 때 대안 | `spikes/can/.venv/.../can/interfaces/` 목록 | 중간 |
| S5 | §5.6 | "UART/CAN의 일부 R1은 R4도 사용하므로 번호만으로 호환되지 않는다" | 문장이 깨져 의미 불명. 다른 팀 등급 척도에 대한 사실 주장이라면 출처가 없음(이번 리뷰 시점에 UART/CAN 폴더에는 r1 초안이 보이지 않음 — Sol §8이 링크한 `../uart/r1-sol.md` 등은 확인 불가) | `docs/plan/uart|can` 파일 목록 | 중간 |
| S6 | §3 FlexRay 행 | 루프 ○/○/○/○ | Python에서 쓸 FlexRay 드라이버가 없다고 같은 행에 적어 놓고 접근 ○. Sol 정의상 ○ = "데몬이 장치/API를 열고 제어"인데 열 수단이 없음. 사실 판단 오류에 가까움 → 접근 △(또는 ✗) | Sol §3 같은 행 "python-flexray PyPI 404; XL C SDK wrapper 필요" | 높음 |

### 1.2 Sol이 맞고 내 초안(r1-opus)이 틀렸거나 빠뜨린 것 — 통합 시 Sol 쪽 채택

| # | 내 초안 서술 | 바른 사실 | 근거 | 확신도 |
|---|---|---|---|---|
| O1 | §5.11 `loom[dbg]` = pyocd + **pylink-square 2.1.0** 동시 설치 전제 | pyOCD 0.46.1의 의존성은 `pylink-square<2.0,>=1.0`, `libusb-package<2.0,>=1.0`. 2.x와 같이 고정하면 설치 실패. 1.x 최신은 1.7.0 | PyPI `pyocd` requires_dist 직접 조회; Sol §0 | 높음 |
| O2 | §4.1 b 시나리오: set-level 후 **+0.1 s**에 debug 로그 폭증, 제어→반영을 ≤100 ms처럼 묘사 | libdlt는 데몬 응답(레벨 변경 포함)을 **0.5초 주기**로 읽는다(사용자 `dlt-wrapper` README 262행 "libdlt가 데몬 응답을 0.5초 주기로 읽음"). 반영은 최대 수백 ms~1 s. Sol의 "ACK와 실제 반영(effective) 분리, 반영 관측창 2 s"가 맞음 | `~/Desktop/01_PROJECT/dlt-wrapper/README.md:262` | 높음 |
| O3 | §4.1 f dlt-daemon 라이선스 "MPL-2.0으로 알고 있음(미확인)" | 확인됨: MPL-2.0. BMW `python-dlt`도 MPL-2.0이고 pyproject의 이름이 `dlt` | `spikes/explore/upstream-raw/COVESA__dlt-daemon__LICENSE`, `bmwcarit__python-dlt__LICENCE.txt`, `…pyproject_toml.txt` | 높음 |
| O4 | DLT 버전을 V1만 암묵 가정 | dlt-daemon 3.0.1 헤더에 `DLT_HTYP2_*`(DLT v2 헤더 플래그)가 있음. M1을 V1로 한정하고 V2를 명시적 미지원으로 표시한 Sol 쪽이 정확 | `dlt-wrapper/third_party/dlt/orig/include/dlt/dlt_protocol.h:148-152` | 높음 |
| O5 | SOME/IP `someipy` 라이선스 "PyPI 미표기" | upstream LICENSE는 GPL-3.0 (PyPI 메타데이터는 비어 있음) | `upstream-raw/chrizog__someipy__LICENSE_md.txt` | 높음 |

---

## 2. 평가 이견

판정 기준 자체에 대한 이견이 먼저다. Sol은 §0/§1에서 "`○`는 **설계 가능한 도메인 능력**"이라고 정의한다. 이 정의는 "지금 Python으로 사람 손 없이 열 수 있는가"를 묻지 않아서, 드라이버가 없는 후보도 ○가 된다(FlexRay, LIN, CCP). 또 루프 가중치 40 + 가치 20이라 **가치가 낮아도 루프만 완전하면 높은 점수**가 나온다(Modbus U=2인데 84점, 사용자 가치 U=4인 릴레이 81점보다 위). 02 원칙은 "루프가 성립**하고** 가치가 큰 것"이므로 둘 다 무겁게 봐야 한다.

| 후보 | Sol 판정 | 내 판정 | 근거 |
|---|---|---|---|
| FlexRay | ○○○○, 64점 | △○△○, 탈락 | Python 드라이버 없음(S6). HW 고가, 사용자 맥락에 흔적 없음. Sol도 "독립 착수하지 않는다"라 결론은 같으나 ○ 판정은 기준을 흐림 |
| LIN | ○○○○, **U=5**, 80점 | △○○○, U=3 | macOS 경로 없음(Sol 스스로 "M 경로 미확인"), PLIN/XL은 Windows DLL ctypes → 접근 △. 사용자 가치 5는 PSU와 동급이라는 뜻인데 근거(사용자가 LIN을 쓴다는 흔적)가 없고 Sol §7 질문에서도 보유 여부를 묻고 있음 |
| Modbus | ○○○○, 84점 (XCP 85와 1점 차) | 70, 독립 도메인 아님 | U=2(Sol 자신 "실사용 근거 약함")인데 상위 게이트 85 직전. 가중치 문제의 증상. 릴레이·전자부하·챔버의 **전송 계층**으로만 두는 편이 맞음 |
| USB 릴레이/IO | ○△○○, 79(실제 81), 상위 탈락 | ○△○○, 83, 상위 5위 | 읽기 △ 판정 자체는 동의(접점 물리 피드백 없음). 그러나 릴레이의 가치는 단독이 아니라 **PSU와 함께 "사람 손 없는 벤치"(KL15 점화, KL30, 웨이크업 버튼)**를 만드는 데 있음. Sol 시나리오 러너 예시(전원→부팅→CAN→UDS)도 점화(KL15) 없이 전원만으로는 많은 ECU에서 성립하지 않음. 실제 효과는 PSU 전류·DLT/UART 로그로 교차 관측 가능 → 단독 추천이 아니라 **PSU 도메인과 묶어** 상위에 넣자(§7 T1) |
| 디버그 프로브 | 86, F=2 | 88, E=2 | 점수는 거의 같음. 이견은 **위험 등급**(§4.3, §6): Sol halt/resume R2, 나는 R3. 실행 중 ECU를 halt하면 워치독(STM32 IWDG 등은 DBGMCU freeze 설정이 없으면 계속 동작)·액추에이터 출력 고정·통신 타임아웃이 생겨 "되돌리기 쉬운 상태 변경"이 아님 |
| XCP | 85, 상위 5위(조건부) | 79, 후보 유지 | 루프는 완벽하다는 데 동의. 차이는 **U=5**: 사용자 맥락(UART/CAN/UDS/DLT)에 캘리브레이션 흔적이 없고 Sol도 "XCP slave·A2L·쓰기 권한 보유"를 착수 gate로 둠. 조건부 5위라는 Sol 처리 방식은 합리적이라 §7에서 수용(조건부) |
| SSH | 93 | 92 (SSH/ADB/telnet 통합) | 점수 동의. 범위 이견: Sol은 `loom ssh` 단일 전송, ADB는 별도 후보 79점. 콘솔 계약이 같으므로 **한 도메인(`loom shell`)의 전송 백엔드**로 두는 편이 CLI·스킬·뷰어 중복이 적음(전송별 착수 순서는 Sol처럼 SSH 먼저) |
| DLT | 89, F=3 | 94 | Sol의 F=3(구현 난이도 중~높)은 제어 왕복 미검증·0.5 s 반영 문제를 반영한 것이라 내 E=4보다 현실적. 순위(상위 2위 내)는 동의 |
| 시나리오 러너 | △△△△, 71점으로 같은 표에서 경쟁 | 점수 대신 위치 판정 | Sol도 "공통 기능 흡수"로 결론이 같으나 장비 후보와 같은 척도에 넣으면 △가 "가치가 낮다"로 오독될 수 있음. 별도 표로 분리 권장 |

---

## 3. 빠진 것

### 3.1 브리프 산출물·교차 기능
1. **시나리오 러너 UX가 거의 없음**. Sol은 §2에서 "공통 기능", §6 E7 "bounded scenario job, user abort와 공통 timeline" 두 줄뿐. 작업 지시(explore r1-opus 관점이지만 explore 브리프 씨앗에도 있음)가 요구한 "사용자가 시나리오 진행을 실시간으로 보고 일시정지/중단/수동 단계로 개입"하는 UX, 단계 타입, 사전 위험 요약, 에이전트 CLI(`run start/wait/status`)가 없음. → 내 초안 §5.12를 채택 후보로.
2. **통합 타임라인·녹화/재생 설계 없음**. Sol은 "새 장비보다 먼저 만들지 않는다"는 순서 판단만 있음. 녹화는 공통 계약 §3-6에 이미 있는 기본 기능이라 "나중"이 아니고, 재생은 "replay 네임스페이스 + 쓰기 금지" 같은 안전 규칙이 필요(내 §5.14).
3. **교차 도메인 시뮬레이션(공유 가상 DUT) 없음**. Sol EXP-S1은 "simulator manifest·deterministic fault profile"로 도메인별 sim만 다룸. PSU OFF → DLT/UART/CAN sim이 함께 멈추는 **SimWorld**가 없으면 시나리오 러너와 인터록을 HW 없이 테스트할 수 없음.
4. **벤치 프로파일(사용자 소유 한계 파일)** 개념이 흩어져 있음. Sol은 "DUT profile의 max V/I", "profile allowlist", "bench별 fail policy"를 각 소절에 쓰지만 단일 파일·소유자(에이전트 수정 불가)·로드 해시·변경 시 사용자 확인 규칙이 없음.
5. **교차 도메인 인터록**. Sol EXP-D1은 "power-cycle 전 UDS 진단/TesterPresent 중지"로 한 방향만. 플래시(UDS 0x34–0x37, 디버그 flash) 중 PSU OFF/KL30 차단을 막는 **인터록 레지스트리**가 필요(벽돌 위험이 가장 큰 조합).
6. **스킬 설계가 얕음**. §5.6에 5개 스킬 이름과 공통 순서 한 줄뿐. 도메인별 SKILL.md 핵심 규칙(예: 디버그 "attach만, halt 금지가 기본", DLT "레벨 올린 뒤 원복", PSU "이상 전류 시 OFF 제안")과 references 목록이 없음.
7. **화면 레이아웃·herdr 배치** 없음. 각 소절 e의 뷰가 4줄 텍스트이며, 여러 도메인을 동시에 보는 화면(에이전트 패인 + 뷰어 패인)과 확인 배너 위치가 없음.

### 3.2 상위 추천 소절 a~j 중 얕은 요소
| 추천 | 빠지거나 얕은 요소 |
|---|---|
| SCPI(§4.1) | e: 사용자 **비상 OFF**가 확인 없이 즉시 되는지 불명("사전 bench policy를 따르되…"). 안전 방향 사용자 동작은 어떤 정책으로도 막지 않는다고 명시해야 함. i: 출력 OFF/cycle과 플래시 진행 인터록 없음 |
| DLT(§4.2) | g/h: M1이 **네이티브 dlt-daemon + C 앱 빌드**에 의존 → Windows CI·HW 없는 CI에서 무거움. 순수 Python 가짜 DLT 서버(제어 메시지에 반응하는)를 M1 CI용으로, 네이티브는 M0 gate·통합 테스트로 분리 필요. i: STORE_CONFIG(0x05)/RESET_TO_FACTORY(0x06) 같은 영속 제어의 등급이 "초기 비범위"로만 처리 |
| SSH(§4.3) | i: 위험 명령 패턴(reboot, dd, mkfs, fw_setenv…)이 R3인지 예시가 없음. d: `pty` 명령이 별도 PTY 프로그램을 띄우는지, 사용자 터미널과 같은 PTY인지 모호 |
| Debug(§4.4) | b: 시나리오가 **halt된 SRAM 쓰기**뿐이라 디버그의 핵심 가치(부팅 중 실행 상태 관찰, RTT 로그 → 원인 추적)가 드러나지 않음. f: AURIX/RH850 같은 비Cortex 차량 MCU 경로(TRACE32 원격 API `lauterbach-trace32-rcl` 1.1.6, MIT, 2026-07-02 PyPI) 언급 없음 — 차량 MCU에서 pyOCD가 안 되는 경우가 흔함. i: 옵션 바이트·RDP·보안 잠금을 "M1에서 금지"로만 두고 **영구 금지(R5)** 개념 없음 |
| XCP(§4.5) | 대체로 충실. h의 시뮬레이터가 XCPlite(C) 빌드 의존이라 DLT와 같은 문제 |

---

## 4. 더 나은 선택

### 4.1 상위 추천 목록과 순서

| 기준 | Sol: SCPI → DLT → SSH → Debug → XCP(조건부) | Opus: DLT → PSU → Shell → Debug → IO | 제3안(추천): **벤치 전원·점화(PSU+IO) ∥ DLT → Shell → Debug → XCP(조건부)** |
|---|---|---|---|
| 핵심 루프 가치 | 높음. 단 점화(KL15) 없어 전원 시나리오가 반쪽 | 높음. XCP 빠짐 | 가장 높음: 전원+점화로 벤치 자동화 완성, XCP는 gate 통과 시 |
| 단순성 | 5개 독립 | 5개 독립 | PSU와 IO를 한 추천(두 CLI, 한 패키지 `loom.instr`/`loom.io` 또는 공동 M1)으로 묶어 4+1 |
| 구현 비용 | XCP 대형 | IO 소형 | IO 소형 추가, XCP는 gate 전 0 |
| HW 확보 | XCP는 ECU 측 조건 | 릴레이 저가 | 동일 |
| Windows/macOS | 동일 | 동일 | 동일 |
| 기존 3팀 경계 | 동일 | 동일 | 동일 |

E1 순서: Sol은 PSU 먼저(쓰기 게이트를 가장 단순한 장비로 검증), 나는 DLT 먼저(HW 없이 스트림·고부하 검증). 둘 다 근거가 있고 **의존성이 없으므로 병렬**(Opus 워커 PSU+IO, Sol 워커 DLT M0 gate→M1)이 최선. DLT는 Sol 지적대로 네이티브 제어 왕복 M0 gate를 먼저 통과해야 함.

### 4.2 플러그인 구조

| 기준 | Sol §5.1 (DomainSpec/BackendFactory/SessionBackend/Capability/PolicySpec/SimulatorFactory/PanelSpec, entry point 그룹 5개) | Opus §5.2 (DomainPlugin.register(Registry) 하나, entry point 그룹 1개) | 추천 |
|---|---|---|---|
| 단순성 | 그룹 5개(`loom.domains/backends/panels/simulators/policies`) — 한 도메인이 다섯 곳에 등록, 버전 불일치 조합 가능 | 그룹 1개, 플러그인이 레지스트리에 모든 것을 등록 | **Opus 방식**(단일 진입점). 외부 백엔드만 따로 붙이고 싶은 경우(사내 PSU 드라이버)는 `Registry.backend()`를 다른 플러그인이 호출하는 것으로 충분 |
| 기능 선언 | **Capability 목록**(memory-read, output-control, log-level…) + 미지원 명시 — 좋음 | 없음 | **Sol의 Capability를 Opus 레지스트리에 추가** (`reg.capability(...)`). 에이전트가 `loom <d> status --json`에서 가능한 동작을 알 수 있음 |
| 쓰기 경로 강제 | "모든 IO가 daemon sequence·기록 경유", queue | 쓰기 파이프라인 명시(분류→한계→인터록→게이트→방송→송신→결과→감사) | 둘 합치기: Opus 파이프라인 + Sol 상태 단계(queued/approved/sent/acknowledged/effective/cancelled/failed) |
| 이벤트 확장 | `session_id,event_seq,request_id,correlation_id,backend_id,simulated,quality`, ts 세분화(`host_rx_mono_ns, device_ts, device_clock_id, uncertainty_ns`) | `v,seq,kind,corr,step,risk,metrics`, `ts.dev` | **합집합**: Sol의 `simulated`, `quality`, `backend_id`, clock uncertainty + Opus의 `kind`(스키마 등록 키), `step`, `risk`, **`metrics`(도메인 무관 자동 플롯)**. `metrics`가 없으면 새 도메인마다 viz 패널 설정이 필요해 "데이터만 주면 렌더링" 원칙이 약해짐 |
| 패널 | PanelSpec(메타데이터, 실행 코드 금지) | 선언형 PanelSpec + `actions`(키/버튼 → 같은 RPC, origin=user 자동) | **Opus의 `actions`**를 채택. 사용자 개입 수단을 도메인이 뷰어 코드 없이 얻는 핵심. Sol의 "payload에 실행 코드·임의 HTML 금지" 규칙은 채택 |

### 4.3 안전·확인 프로토콜 (가장 중요한 차이)

| 기준 | Sol | Opus | 추천 |
|---|---|---|---|
| 누가 승인하나 | "승인은 action hash·resource identity·한계·시간·횟수에 묶고 **GUI와 CLI에서 같은 token** 사용" — CLI로 승인 가능하면 **에이전트가 자기 요청을 스스로 승인**할 수 있음. origin을 client role로 인증(§5.2)하는 Sol 규칙과도 연결이 안 됨 | 승인 응답은 **뷰어(사용자 클라이언트)만**, CLI는 answer 미구현. **뷰어가 없으면 R3 이상 즉시 실패**(사용자가 보고 있을 때만 위험 동작) | **Opus 방식 + Sol의 승인 바인딩**(action hash·resource identity·profile hash·유효시간·횟수). 승인 토큰은 뷰어에서만 발급 |
| 등급 척도 | R0–R3 (R3 = 파괴/복구 곤란, 초기 비범위) | R0–R5 (R3 확인, R4 확인 토큰, R5 에이전트 영구 금지) | Opus 척도. "확인 필요"와 "토큰 필요"와 "영구 금지"를 구분해야 디버그 옵션 바이트·RDP 같은 벽돌 동작을 정책으로 막을 수 있음. UDS 팀 척도와의 매핑은 리드/메인 결정 |
| 디버그 halt | R2 | R3 (액추에이터 있으면 R4) | R3 (§2 근거: 워치독·액추에이터) |
| 사용자 비상 동작 | 정책 경유("사전 bench policy를 따르되") | 안전 방향 사용자 동작(PSU Esc OFF)은 확인 없음 | Opus. 단 Sol 지적처럼 OFF도 DUT 리셋을 유발할 수 있으므로 **사용자**에게는 허용, **에이전트**의 OFF는 R3 |
| 세션 사전 허용 | 승인에 횟수·시간 바인딩(사실상 grant) | `grant <method> 10m`, R4/R5 불가 | 같은 개념. 합치면 됨 |

### 4.4 교차 기능

| 기준 | Sol | Opus | 추천 |
|---|---|---|---|
| 시나리오 러너 위치 | 공통 기능, E7(2개 이상 도메인 M1 후), "별도 core 계획" | loom.core 데몬 잡, E3 | 위치는 같음. 시점은 "UART + 탐색 도메인 2개(PSU+IO, DLT) M1 후" — 사실상 양안 일치. **UX는 Opus §5.12**(pause/skip/abort/manual, 사전 위험 일괄 승인, `run wait` 경계) 채택 |
| 통합 타임라인 | 순서 판단만 | viz 패널 타입(레인 자동, 커서 동기화) | Opus |
| 녹화/재생 | "새 장비보다 먼저 만들지 않음" | 녹화 = 기본(계약 §3-6), 재생 = replay 네임스페이스 + 쓰기 거부 | 녹화는 기본, 재생은 후순위로 두되 "재생 채널 쓰기 금지" 규칙은 지금 계약에 |
| 원격 HW 호스트 | EXP-R1 별도 ADR, 인증·시계 차이까지 정한 뒤 | CR-6 SSH 로컬 포워딩 | Sol의 신중함 + Opus의 구체 경로(SSH 포워딩이면 로컬 전용 IPC 계약을 깨지 않음). 시계 차이는 데몬이 HW 쪽에 있으므로 단일 시계 유지 — Sol이 걱정한 "두 host monotonic 차이"는 뷰어 렌더 확인 지연 측정에만 해당 |

### 4.5 도메인별 설계 선택

| 쟁점 | Sol | Opus | 추천 |
|---|---|---|---|
| CLI 이름 | `loom scpi`, `loom debug`, `loom ssh` | `loom psu`, `loom dbg`, `loom shell` | `scpi`는 프로토콜 이름이라 에이전트 의도("전원 꺼")와 거리가 있고, 스코프가 붙으면 `loom scpi`의 동사가 장비군마다 달라짐 → **장비군 이름(`psu`, 이후 `dmm`, `scope`) + 공유 패키지 `loom.instr`**. 디버그는 Sol의 `debug`가 더 명확(양보). 셸은 전송이 여럿이므로 `shell` |
| SSH exec | 별도 exec 채널(argv), 뷰어에 별도 channel ID로 표시 | 공유 PTY + 센티넬로 rc 회수 | **Sol 쪽이 나음(양보)**: 센티넬 방식은 셸 상태·전체화면 프로그램에 취약하고 exit status가 부정확할 수 있음. 조건: exec 채널은 **뷰어에 탭으로 자동 표시**(숨은 연결 금지) |
| 디버그 M1 sim | QEMU GDB (+ 펌웨어 빌드 필요) | 순수 Python SimTarget(심볼 JSON, 실행 중 변수 진화, RTT 링버퍼) | **M1 CI = SimTarget**(툴체인·전역 설치 없이 실행 중 watch·RTT·halt 확인 시연 가능), **M2 = QEMU/Renode + 실제 ELF**. Sol의 QEMU 스파이크는 M2 근거로 유효 |
| DLT M1 sim | 네이티브 dlt-daemon + 생성 앱 | 순수 Python SimDltServer(CI) + 네이티브(통합) | 양쪽 합침: **M0 gate = 네이티브 제어 왕복(Sol)**, M1 CI = Python 가짜 서버, 통합 테스트 = 네이티브 |
| PSU sim | pyvisa-sim YAML(정적, 상태 모델 추가 필요 인정) | SimPsu + SimWorld 부하 모델 + TCP SCPI 서버 | Opus(동적 부하 모델이 있어야 "이상 전류 → 사용자 OFF" 시나리오가 CI에서 돔) |
| PSU 폴링 기본 | 5 Hz, 장비당 미완료 query 1개 | 10 Hz | **Sol 5 Hz + 미완료 1개 규칙**(보수적 시작, 실측 후 상향). 실측 주기 화면 표시는 양안 공통 |

---

## 5. 합의 가능한 부분

그대로 plan.md에 넣어도 되는 Sol 안의 결정(내 초안보다 나은 것 표시 ★):

1. ★ **pyOCD 0.46.1 + PyLink 1.7.0 고정**(2.x 충돌), 공존 불가 시 백엔드 워커 환경 분리하되 장치 소유권은 하나.
2. ★ **DLT 제어 결과를 `acknowledged`와 `effective`로 분리**, 반영 관측창 2 s, ACK만으로 성공 표시 금지. 사용자 변경을 자동 원복으로 덮어쓰지 않음.
3. ★ **쓰기 상태 단계** queued/approved/sent/acknowledged/effective/cancelled/failed — 화면 거짓 성공 방지.
4. ★ **late viewer: snapshot at seq N + subscribe after N 원자적 전달**. 02 원칙 "뷰어를 열면 즉시 같은 스트림" 의 구현 조건으로 정확함.
5. ★ **origin은 클라이언트가 payload로 주장하는 값이 아니라 인증된 client role에서 데몬이 부여**. 위조로 user 우선권 획득 방지. (내 §5.6 "CLI는 answer 미구현"보다 근본적)
6. ★ **resource identity 정규화와 alias 감지**(같은 프로브/VISA 장치를 다른 URI로 이중 open 방지).
7. ★ **gap 출처 분류**(device/transport/daemon/recorder/viewer)와 "UI 생략 수와 원시 손실을 하나의 숫자로 합치지 않음". 기록 불가 시 세션 실패 처리.
8. ★ **`quality` 필드**(stale, aggregated, readback-only, coherent=false, target-loss-unknown) — 내 초안의 "실측 주기/찢긴 읽기 표시"를 구조화한 형태.
9. ★ 장치 응답 RX를 요청자 origin으로 바꾸지 않고 부모 request로 연결. 장비 front panel 조작은 origin=device로 관측하되 사용자 신원을 추측하지 않음.
10. ★ 재연결 시 새 epoch, **오래된 승인·pending write 자동 복원 금지**.
11. SCPI raw `query`도 부작용 가능 → 허용 목록 밖은 쓰기와 같은 심사. 바이너리(파형)는 artifact 참조(`artifact_id, sha256`)로.
12. 설정값 readback(`VOLT?`)과 물리 측정(`MEAS:VOLT?`)을 다른 quality로 라벨.
13. 뷰어 "Pause"는 화면만 멈춤(수신·기록 계속), 뷰 필터와 장비 로그 레벨 변경은 다른 버튼.
14. XCP는 **Ethernet 먼저**(CAN 소유권 계약과 독립 검증), DAQ는 M2, A2L(pya2ldb GPLv2)은 검증된 심볼 맵으로 M1에서 분리, pyXCP의 CAN 백엔드가 CAN 데몬을 우회하면 채택 금지.
15. 디버그: 실행 중 다중 워드 읽기 `coherent=false`, RTT 타깃 드롭은 카운터 없으면 `unknown`, watch가 CPU를 halt했으면 숨기지 않음, MMIO 읽기 부작용 → 주소별 등급(내 안과 일치).
16. HW 가격 표, GitHub 활동 표, 404 패키지 목록, `python-dlt`/`dlt` 이름 함정 — 근거 자료로 그대로 사용.
17. 파일 분석 도구·passive pcap·로직 애널라이저를 쓰기 루프 부재로 상위 제외 — 양안 일치.
18. 시나리오 러너는 독립 장비 도메인이 아니라 공통 기능 — 양안 일치.

---

## 6. 핵심 원칙 점검

Sol 상위 추천 5개 × 02 체크리스트 6개. (충족 / 부분 / 미충족)

| 항목 | SCPI §4.1 | DLT §4.2 | SSH §4.3 | Debug §4.4 | XCP §4.5 |
|---|---|---|---|---|---|
| 1 핵심 루프 표 | 충족 | 충족 | 충족 | 충족 | 충족 |
| 2 종단 간 시나리오(사용자 개입·에이전트가 origin=user 읽기) | 충족 | 충족 | 충족 | **부분** — halt된 SRAM 쓰기만, 실행 중 관찰 가치 미시연 | 충족(조건부 HW) |
| 3 지연 목표 + 고부하 정직성 | 충족(전제 조건 명시가 우수) | 충족(MCNT·target overflow 구분 우수) | 충족 | 충족 | 충족(“그래프만 부드러운 상태 불수용” 우수) |
| 4 M1 = sim 위 걷는 뼈대 | **부분** — pyvisa-sim이 정적, 상태 sim은 "추가 필요"로만 | **부분** — M1 sim이 네이티브 C 빌드 의존(CI/Windows 현실성) | 충족 | **부분** — QEMU 전역 설치 + 펌웨어 빌드 의존, 실행 중 watch 미시연 | **부분** — XCPlite C 빌드 의존, 슬레이브 왕복 미검증(Sol 스스로 M0 gate) |
| 5 사용자 화면이 에이전트 세션과 분리 안 됨 | 충족(late viewer 수용 기준) | 충족 | 충족(exec 채널 별도 표시) | 충족(GUI가 probe 직접 열지 않음) | 충족 |
| 6 비기여 기능 후순위/비범위 | 충족 | 충족 | 충족 | 충족 | 충족 |

지연 목표의 근거: Sol은 모든 수치를 "제안 목표"와 "실측(sim 메모리 속도)"으로 구분했고 sim 수치를 실 HW로 오용하지 말라고 명시 — 양안 중 더 정직함. 단 **사용자 화면 지연(render)을 측정하는 방법**(뷰어가 렌더 완료 시각을 데몬에 ack, `render_mono`)이 용어 정의에만 있고 M1 수용 기준에는 없음 → 각 M1 수용 기준에 "뷰어 render ack로 p95 측정"을 추가 권장.

고부하 화면 동작: SCPI(집계 min/max/last + 누락 수), DLT(행 제한·APID 집계·생략 수·렌더 lag), SSH(숨긴 byte 수), XCP(envelope + 원본 샘플 수) — 모두 충족. Debug는 watch 다운샘플 표시만 있고 RTT 폭주 시 화면 정책이 없음 → UART 콘솔 정책 준용 명시 필요.

보완안 요약: (1) 각 도메인 M1 CI는 순수 Python sim, 네이티브/QEMU는 M0 gate·M2 통합으로 분리, (2) 디버그 시나리오를 "실행 중 watch + RTT → halt(사용자 확인) → 사용자 resume"로 교체, (3) M1 수용 기준에 render ack 기반 지연 측정, (4) 확인 응답 주체를 뷰어로 한정.

---

## 7. 쟁점 목록

| # | 쟁점 | opus 안 | sol 안 | 내 추천 | 근거 |
|---|---|---|---|---|---|
| **T1** | **최종 상위 추천과 순서** | DLT → PSU → Shell → Debug → IO | SCPI PSU → DLT → SSH → Debug → XCP(조건부) | **1. 벤치 전원·점화(`loom psu` + `loom io`, 한 추천) ∥ 2. DLT (병렬 착수, DLT는 네이티브 제어 M0 gate 먼저) → 3. 타깃 셸(SSH 먼저, ADB/telnet은 같은 도메인 후속 백엔드) → 4. 디버그(`loom debug`, SimTarget M1 → 실프로브 M2) → 5. XCP(조건부: Q6 답이 "있음"일 때만, Ethernet 먼저)** | 전원+점화가 있어야 벤치 시나리오가 완결(§2 릴레이 행). PSU와 DLT는 서로 의존 없음 → 두 워커 병렬. XCP는 Sol의 조건부 처리 수용 |
| T2 | 루프 ○ 판정 기준 | "지금 Python으로 사람 손 없이 열 수 있는가" | "설계 가능한 능력" | Opus | 드라이버 없는 FlexRay/LIN이 ○가 되어 표가 왜곡됨 |
| T3 | 점수 가중치 | 루프 30 + 가치 30 | 루프 40 + 가치 20 | 루프 30 + 가치 30 (또는 Sol 공식 유지 시 U 가중 25로 상향) | Modbus(U=2)가 릴레이(U=4)보다 높게 나오는 역전 |
| T4 | 확인(승인) 응답 주체 | 뷰어만, 뷰어 없으면 R3+ 실패 | GUI와 CLI가 같은 token | **뷰어만** + Sol의 승인 바인딩 | CLI 승인 = 에이전트 자기 승인 가능 |
| T5 | 위험 등급 척도 | R0–R5 | R0–R3 | R0–R5 (UDS 팀 척도와 매핑은 메인 결정) | 확인/토큰/영구 금지 구분 필요 |
| T6 | 디버그 halt 등급 | R3 | R2 | R3 | 워치독·액추에이터·통신 타임아웃 |
| T7 | 플러그인 등록 형태 | entry point 1그룹 + Registry | 5그룹 | 1그룹 + Sol의 Capability 선언 추가 | 단순성, 버전 조합 문제 회피 |
| T8 | 이벤트 확장 필드 | `kind, corr, step, risk, metrics, ts.dev` | `session_id, event_seq, request_id, correlation_id, backend_id, simulated, quality, clock uncertainty` | 합집합(§4.2 표) | `metrics` = 도메인 무관 자동 플롯, `quality`/`simulated` = 정직성 |
| T9 | CLI 도메인 이름 | `psu`, `dbg`, `shell` | `scpi`, `debug`, `ssh` | `psu`(+`dmm`/`scope`), `debug`, `shell` | 장비군 이름이 에이전트 의도와 일치. `debug`는 Sol 쪽이 명확 |
| T10 | 셸 exec 방식 | 공유 PTY + 센티넬 | 별도 exec 채널(뷰어에 표시) | **Sol** + 뷰어 자동 탭 | exit status 정확, 셸 상태 비의존 |
| T11 | 디버그 M1 시뮬레이션 | 순수 Python SimTarget | QEMU GDB | M1 SimTarget, M2 QEMU/Renode | CI 무설치, 실행 중 watch 시연 가능 |
| T12 | DLT M1 시뮬레이션 | Python 가짜 서버 | 네이티브 dlt-daemon | M0 gate 네이티브(Sol), M1 CI Python(Opus) | 둘의 장점 결합 |
| T13 | 교차 도메인 시뮬레이션 | SimWorld(공유 가상 DUT) core 요청 | 도메인별 simulator manifest | SimWorld | 시나리오 러너·인터록 CI 테스트 |
| T14 | 시나리오 러너 시점 | E3(도메인 3개 M1 후) | E7(도메인 2개 이상 M1 후) | PSU+IO와 DLT M1 직후(UART 포함 3도메인) — 실질 동일, **UX는 Opus §5.12** | 양안 차이 작음 |
| T15 | E0(플러그인 계약) 시점 | 세 팀 M1과 병행(문서) | 기존 M1 완료 후 | 문서 합의는 지금, 구현은 core M1 후 | core가 굳기 전에 확장 지점을 넣어야 비용이 낮음 |
| T16 | 벤치 프로파일 | 단일 `loom.bench.toml`, 에이전트 수정 불가 | 소절별 profile 언급 | 단일 파일 공통 계약화 | 한계값을 데몬이 강제하는 근거 |
| T17 | 인터록 | 레지스트리(플래시 중 전원 차단 승격) | EXP-D1(전원 사이클 전 UDS 중지) | 레지스트리 + EXP-D1 둘 다 | 벽돌 위험 조합 |
| T18 | PSU 폴링 기본 | 10 Hz | 5 Hz, 미완료 query 1개 | Sol | 보수적 시작 |
| T19 | 릴레이/IO 처리 | 독립 상위 5위 | 상위 밖(79→실제 81), PSU/IO 하위 백엔드 후보 | PSU와 한 추천으로 묶음(T1) | 가치는 결합에서 나옴 |
| T20 | 비Cortex 차량 MCU(AURIX/RH850) 경로 | TRACE32 원격 API 언급 | 없음 | 백엔드 후보에 포함, 사용자 질문(MCU 계열)으로 확정 | 차량 MCU에서 pyOCD 불가 사례 흔함 |

<!-- END R2 -->
