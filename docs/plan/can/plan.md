# ② CAN + 시각화 (`loom.can`, `loom.viz`) — 통합 계획서

작성: can-lead (R3 통합 2026-11-09, v2.0 개정 2026-11-10)
입력: **v2.0: `v2-opus.md`, `v2-sol.md`, `spikes/can/rust/RESULTS.md`, 마스터 §2.3 C1–C8, `04-v2-tasks.md`** · 이전: `r1-opus.md`, `r1-sol.md`, `r2-opus-review.md`, `r2-sol-review.md`, `00-master-plan.md` v1.2(§2 D1–D14, §2.1 A1–A16, **§2.2 B1–B7**), `03-v1.2-tasks.md`, `master-review.md`, `_xteam/can-uds-isotp.md` v0.3+, `_xteam/viz-requirements.md`(uds·uart 절), 실측 `spikes/can/r1-results.json`·`r2-results.json`
기준 우선순위: **`docs/plan/02-core-principle.md`** > `00-brief.md` > `can/brief.md`
상태: 계획 **v2.0** — **Rust 코어·장치 경로, 웹 UI 1급, 통합 테스트(마스터 C1–C8)** 반영(§0.3, 결정 기록 D54~D75). 그 아래 v1.2(B1–B7)·v1.1(A1–A16) 결정은 C3에 따라 유지. 구현 코드 없음. 수치는 **실측**(spike), **추정**(산술), **목표**(수용 기준), **미확인**을 구분해 적는다.

> 상세 화면 목업·전체 명령 표가 더 필요하면 `r1-opus.md` §2·§4·§5, 라이브러리 원문 근거는 `r1-sol.md` §7과 `spikes/can/*-evidence.json`을 본다. 이 문서와 충돌하면 이 문서가 우선한다.

---

## 0. 핵심 루프 (v2.0)

**에이전트가 CAN 인터페이스(PCAN-USB FD, VN5640, SocketCAN, sim)를 `loom can`으로 직접 열어 읽고 쓰며, 사용자는 같은 프레임·신호를 같은 순간 Loom UI 앱의 CAN 패널에서 보고, 같은 패널에서 송신·IG·STOP·승인으로 개입한다.**

| 요소 | 에이전트 명령 | Rust 구성요소 | UI 앱 패널 (사용자) | 첫 M |
|---|---|---|---|---|
| ① 직접 접근 | `loom can status`, `loom can open ch0 --iface sim\|pcan\|vxl\|socketcan …` (데몬 자동 기동) | **CAN worker 프로세스**(백엔드별 `can-sim` / `can-pcan` / `can-socketcan` / `can-vxl`)만 드라이버·벤더 DLL을 로드·소유. gateway **CAN 모듈**이 채널 레지스트리·worker 라우팅 | **채널 상태 바**: `ch0 PCAN-USB FD 500k/2M ● load 23% err 0 ACTIVE · REC ok · env BENCH`, LIVE/LAG | M1 sim, M2a PCAN FD(Win+Linux), M2b VN5640 |
| ② 읽기 | `trace --since @last`, `last`, `expect`, (M2) `sig get`, `sig wait-for`, `check`, `stats` | worker: 수신 스레드·`source_seq`·고정 링·저널 writer. gateway CAN 모듈: 링 미러·Fixed·trace 질의·DBC 디코더·SignalCache·신호 시계열·증거 질의 | **Trace**(Fixed/Chrono), **Data/State**, **Graphics**. 에이전트가 읽고 기다리는 것은 **Activity**에 | M1 raw, M2 신호 |
| ③ 쓰기 | `send ID#DATA [--dry-run] [--expect]`, (M2) `send --msg`, `periodic start`, `sig set`, `isotp *` | core 쓰기 파이프라인(정책→확인/승인→감사, CORE) → worker **TX 중재 스레드**(latch·예약 ID·rate 재집행)·주기 heap·ISO-TP 상태기계. publish-before-reply | Trace `AGT` 행(`requested→sent→confirmed`), **IG**에 job 소유자·rev·lease, 셸 승인 대기열 | M1 raw 단발, M2 주기·IG·승인 |
| ④ 사용자 실시간 표시 + 개입 | 에이전트가 UI 링크를 건넴: `loom ui open can/trace?ch=ch0` | CORE 이벤트 허브: 원자적 snapshot+live, 우선 제어 레인, 적응형 배치. 사용자 입력은 viewer role → `origin=user` | **Trace 하단 송신 입력줄**(M1), **IG**(M2), 셸 **STOP**(M1)·승인(M2). 에이전트 다음 결과에 `others_since_last` / `NOTE:` | M1 |

루프 보장 장치: 가시성 불변식 I1~I6(§3.7), 실시간 목표·고부하 정직성(§3.8, 웹 기준 §5.1), 걷는 뼈대 M1(§11), C2 상한·soak(§9.4), 증거 기반 검증(§9.5).

### 0.1 마스터 계획 v1.1 정합 (요약 — 상세는 각 절, 결정 기록 §16 D30~D44)
| 마스터 결정 | 이 계획에서 |
|---|---|
| D1 프로세스 | gateway + CAN worker(`can-hw`/`can-sim`), UDS 세션 관리자는 gateway 안 (§3.1) |
| D2/A5 IPC·배치 | `hello{proto, token, client_id, want_role}`, 적응형 배치(40 ms는 상한, 조용한 뒤 첫 이벤트 즉시), 64 KiB = 요청·이벤트 배치 1개, snapshot 분할, viz 대용량은 HTTP GET, publish-before-reply (§3.3, §4.5) |
| D3/A13 레코드·손실 | `kind`, `ts{mono_ns, wall_ns, clock_id, source, quality}`, `{encoding, data}`, `metrics[]`, 손실 3층 `source.gap`(드라이버 오버런)·`record.gap`·`display.gap` (§3.8, §6.1) |
| D4/A14 쓰기 상태 | requested → accepted → sent(드라이버 접수) → confirmed(backend TX 확인) → effective_verified/effect_unverified, 어디서든 failed/outcome_unknown. ISO-TP `tx_state: driver_accepted/bus_confirmed` (§3.4) |
| D5/A6 CLI | 봉투(`cmd, warnings[], others_since_last, error.hint`), 종료 코드 0~10, `wait-for` 최대 3600 s·`--follow` 600 s, `--since mark:X`, `LOOM_CLIENT` 안정 id (§4.1) |
| D6/A8/A9/A10/A11 안전 | R2 = confirm(sim `auto`, 실HW `viewer`, `tx="allow"` 채널만 `token_or_viewer`), R3 승인 10분 = 사용 기한 + `max_job_duration`, 차단 3층 (§10). ~~vehicle tx deny~~ → **B4로 폐기**(§0.2) |
| A1/A2 쓰기 클래스·훅 | 단발 send = 요청형, 주기·IG·replay·ISO-TP 채널 = 잡형(worker가 단위 송신을 프레임마다 재집행), CAN 훅 `classify / approval_binding / presend_check` (§3.4, §10.2) |
| A3 STOP | agent·script 쓰기와 **모든 잡** latch, 사용자 1회 송신은 계속 허용 (§10.6) |
| D7/A12 기록 | `~/.loom/records`, 30일/20 GiB, 기록·감사 불능 시 agent·script R1+ 중단(sim 포함), 사용자 입력은 허용 + 배너 (§6.2) |
| D9 뷰어 | `Ctrl-] x` STOP(+`Ctrl-X`), `LIVE/LAG/STALLED/LIVE-DEGRADED`, heartbeat 1 s, `no_viewer_attached` (§3.7, §5.2) |
| D10/§3.2 작업 경계 | C0–C4는 CORE 소유. CAN = worker + **CAN gateway 측 모듈 G1**(링 미러 CAN 질의·Fixed 테이블·디코더·viz 데이터셋) (§12) |
| D12/A15 finally | `can.periodic.stop`, `can.stop_all` = `safe_state: true`. STOP 시 finally 자동 실행 없음, 뷰어 "복구 송신" 1키 (§10.6) |
| D13 스킬 | 원본 `skills/`, `loom skills sync`로 `.claude/skills/`·`.agents/skills/` 복사 (§15.5) |
| A16 ISO-TP 이름 | RPC `can.isotp.*`, 이벤트 `can.isotp_*` (§15.2, `_xteam/can-uds-isotp.md`) |

### 0.2 사용자 답변 v1.2 정합 (마스터 §2.2 B1–B7 — A·D보다 우선, 결정 기록 D45~D53)
| 마스터 결정 | 이 계획에서 |
|---|---|
| B1 플랫폼 | 실HW 호스트 = **Windows x64(PCAN-Basic·Vector XL·CANoe) + Linux x64(SocketCAN `peak_usb`)**, macOS는 개발·virtual만. M2 첫 실HW = PCAN을 **두 OS 모두**. Linux 인터페이스 up/비트레이트는 **사람이 사전 설정**(Loom은 검증·안내만, 권한 헬퍼 없음 — D47). 실HW 체크리스트 OS별 (§8, §8.3, §8.4) |
| B2 보유 장비 | Vector HW + CANoe, PEAK PCAN-USB(FD 미확인), USB-시리얼, Saleae. 모델·버전 미확인 → §14 남은 질문 |
| B3 CANoe | **CANoe 브리지 v0 `loom/canoe/`를 M2(P2)에** — Windows 전용, `canoe-bridge` worker의 COM 전용 STA 스레드, 측정 상태·시작/정지, 신호·시스템 변수 읽기/쓰기, 재연결, UDS `canoe` 백엔드용 내부 진단 API(`_xteam/canoe-bridge.md`) (§8.2). **Vector 채널 공존 실측을 P3(M3 기간)로** 앞당김, 실패 시 대체 = 브리지 + PCAN 관찰 (§8.1). 외부 트래픽 표시 `external:canoe`/`external:host`/`external:?` (§3.4, §5.2, §8.1) |
| B4 vehicle | **vehicle 송신 기본 차단 폐기** — vehicle의 위험 등급·R2 경로·lease·기본값 = bench. `default_blocked` CAN 기본 목록 비움, 하드 차단 = 사용자가 설정한 것만(`deny_ids`, `e2e_protected`) + ISO-TP 예약 ID(소유권 규칙). 남는 것: **`env=vehicle` 배지 상시**(TUI·웹·CLI 봉투 `meta.env`), R3/R4 뷰어 승인 유지 (§4.1, §5.2, §10) |
| B5 탐색 | CAN 영향 없음(Saleae LA는 EXPLORE). viz는 LA 결과 표시용 데이터셋 입력을 그대로 수용(§15.3) |
| B6 UART | CAN 영향 없음 |
| B7 남은 질문 | CANoe 버전·Vector 모델(VN16xx/VN56xx, 이더넷), PCAN FD 여부 — 기본값으로 진행 (§14) |

### 0.3 v2.0 방향 전환 정합 (마스터 §2.3 C1–C8 — C > B > A > D, 결정 기록 D54~D75)
| 마스터 결정 | 이 계획에서 |
|---|---|
| C1 언어 | 코어(`loomd`·CLI)는 CORE가 Rust로. CAN worker·gateway CAN 모듈·ISO-TP·DBC codec·XL/PCAN FFI 전부 Rust. **Python 의존 0**(python-can·can-isotp·cantools·asammdf 제거, §7). CANoe 브리지(P4)도 Rust COM 스파이크를 먼저 — Python 예외는 근거가 입증될 때만(D71) |
| C2 성능·메모리 | 모든 큐·링 상한과 넘칠 때 정책 표, worker 예산 정상 ≤ 192 MiB / 상한 256 MiB, 24 h soak 관문(drift ≤ 16 MiB, slope ≤ 0.5 MiB/h), 벤더 DLL은 해당 worker에서만 (§9.4, §3.3) |
| C3 계약 유지 | D2·D3·D4/A14·D5·D6·D7·A1–A16·B1·B4, ISO-TP 합의 그대로. 아래 절 중 §2·§3.4–3.5·§3.7·§4·§6.1–6.2·§10은 **계약 절이라 내용 유지** |
| C4/C4.1 Vector | **VN5640 = CAN FD 전용 1급 경로**: CAN 소유 XL FFI crate는 **CAN/CAN FD API만**(C4.1 — XL 이더넷·`smoltcp` 범위 밖). 공개 `vxlapi-sys` 2.1.0이 있음(마스터의 "공개 crate 없음"은 사실 정정 — D63). DoIP는 PC 랜카드 + 미디어 컨버터 위 OS TCP/IP(UDS 소유, `_xteam/vn5640-eth.md` 메인 확정). CANoe 브리지 P4로 강등, 공존 실측 유지(M2b) |
| C5 PEAK | **PCAN-USB FD를 M2a 첫 실HW부터 FD로**: Windows = PEAK 공식 Rust FFI 모듈(PCAN-Basic 5.1.0.1194 SDK 안 `PCANBasic.rs`, 확인 — D64), Linux = `socketcan` crate FD |
| C6 UI | 사용자 화면 = **Loom UI 앱(웹)**, M1부터. CAN = 패널 명세(`_xteam/can-ui.md`, §5) + gateway 데이터 질의. 웹·`loom.viz` 구현은 UI 팀 소유로 이관. TUI는 보조(M5) |
| C7 통합 테스트 | CAN이 주는 것: 대기·검증 명령(`expect`·`check`·`stats --expect-period`·`sig wait-for`), 증거 객체·`can.evidence.verify`, 세션 사전 승인 항목, 단계 마커 표시 (§9.5, `_xteam/can-test.md`) |
| C8 스킬 | `loom-can` v2: 짧은 본문, 정본은 `loom can help --json`, UI 링크 안내·증거 인용 규칙 (§15.5) |

**읽는 법(v2):** 유지한 계약 절에 남은 v1 구현 표현은 다음으로 읽는다 — python-can `Bus`/`Notifier` → Rust 백엔드 crate·수신 스레드(§3.3), can-isotp → CAN 소유 Rust ISO-TP 상태기계(§3.6), cantools → `can-dbc` 파서 + 자체 codec(§7), `loom can monitor`(TUI) → Loom UI 앱 CAN 패널(§5, TUI는 보조), `user:tui` → `user:web`, `can-hw` → 백엔드별 worker.

---

## 1. 범위 / 비범위

### 1.1 결론
CAN 컴포넌트는 다섯 덩어리다: **(a)** CAN 채널을 소유하는 CAN worker(데몬 일부), **(b)** 에이전트용 경계 있는 CLI, **(c)** CANoe 작업 감각을 옮긴 **UI 앱 CAN 패널의 명세와 데이터 질의**(렌더링·셸은 UI 팀, C6), **(d)** `loom.viz`에 넣을 CAN 데이터셋(viz 자체는 UI 팀 소유로 이관), **(e)** UDS 팀에 제공하는 ISO-TP(CAN-TP) 호스팅. CANoe 전체 대체(CAPL, 패널 디자이너, 테스트 모듈)는 만들지 않는다.

### 1.2 우선순위 (핵심 루프 기여도 순)

| 등급 | 항목 | 마일스톤 |
|---|---|---|
| **필수 (루프 자체)** | CAN worker 장치 소유, virtual 백엔드, raw send/trace/last/커서, 출처 태깅, 데몬 내 가짜 ECU, **웹 UI** 라이브 트레이스 + 사용자 송신(UI 팀 구현), 원자적 snapshot+live, stop-all, 자동 기록(정본 저널), 최소 안전 | M1 |
| **강화 (루프 품질)** | DBC 신호 인/디코드, `wait-for`, 주기 송신/IG(신호만 바꾸기), 정책 R0~R4 + 승인, **첫 실HW PCAN-USB FD(Windows·Linux, M2a) + VN5640 CAN FD(M2b, Windows)**, ISO-TP 최소판(UDS CAN 경로 전제), 검증 명령·증거, BLF/ASC export, `stats`, 24 h soak | M2 |
| | UI 앱 CAN 패널 완성(Graphics 커서·IG takeover·EXT/UDS 주석 — 렌더링은 UI 팀) | M3 |
| | Vector 공존 완성(FD·어댑터), CAN FD 실HW, ISO-TP 완성(주소 모드 전체·FD·기능 주소 그룹), Statistics 패널, MF4(선택) | M4 |
| **후순위** | **CANoe 브리지(P4, Rust COM 우선)**, 버스 Replay, CANoe 공식 MCP 평가, TUI 보조 뷰, ARXML 부분 지원, Kvaser/gs_usb, 레이아웃 저장/도킹, PNG 스냅샷, 가상 시계 모드, E2E/CRC 플러그인 | M5 |
| **비범위** | CAPL, LIN 통신(LDF), FlexRay/CAN XL, Automotive Ethernet 전체(VN5640 이더넷 포트 포함 — C4.1, DoIP는 UDS가 PC NIC로), J1939·CANopen 해석, 물리층 분석, 하드 리얼타임(100 µs급 STmin 보장), 데스크톱(Qt) 뷰어, UDS 서비스 의미·플래시(UDS 팀 소유) | — |

### 1.3 범위 판단 근거 (요약)
| 항목 | 판단 | 근거 |
|---|---|---|
| ASAM XIL API | M1–M4 비범위 | HIL/testbench 연동 후보이나 설치 API·entitlement·Python binding 미확인. 일반 CAN 송수신에 XIL을 요구하지 않는다 |
| CANoe 자체 제어 | **P4 선택(C4로 강등)** | 진단 기본 경로는 자체 스택(VN5640·PCAN FD). 브리지는 Rust COM 스파이크 후 결정(§8.2, D71). v1.2 설계는 이력. 공식 MCP는 M5 보완 평가. 참고: Vector는 **CANoe 20 SP2부터 공식 CANoe MCP Server/AI Package** 제공(vector.com/en/product/canoe/canoe-ai-features, R2에서 두 워커 모두 확인). 공식 MCP 우선, `py-canoe`(26.4.1, MIT, 비공식 COM 래퍼)는 fallback. CANoe 일반 제어용 Vector 공식 PyPI 패키지는 확인 못 함 |
| CANoe와 같은 HW 채널 공유 | **P3 실측(B3)**, M4 FD 완성 | 사용자가 CANoe를 띄운 채 에이전트가 같은 버스를 보고 쓰는 것은 루프 자체. 실패 시 브리지 + PCAN 관찰로 대체. §8.1 |
| ARXML | M5 부분 | v2: Rust 파서 미선정. 우선 "ARXML→DBC 변환 후 사용" 가이드(변환 도구는 사용자 환경) |
| LDF/LIN | 비범위 | 별도 HW·버스. 같은 데몬 구조에 얹을 수 있게만 설계 |
| MF4 | M4 선택 | v2: Rust 후보(`asammdf` crate 0.1.0은 production 미준비 명시, `mdfr` GPL-3.0·미배포) 심사 전 **미지원으로 명시**. Python asammdf를 숨은 의존으로 두지 않음 |

---

## 2. 사용 시나리오

공통 전제: herdr 한 패인에 에이전트, 다른 패인에 `loom can monitor`(M3부터는 브라우저도). 출력 예시는 제안 인터페이스다.

### 2.0 종단 간 시나리오 (핵심 루프) — M1 수용 시험 그대로
> **v2.0:** 사용자 쪽 화면은 **Loom UI 앱의 CAN Trace 패널**(§5, M1 와이어프레임 §5.3)이다. 아래 TUI 목업은 같은 데이터·같은 순서를 보여 주는 예시로 남기며, M1 수용은 브라우저 E2E로 판정한다(§11 ①). `user:tui`는 `user:web`으로 읽는다.

준비(M1 시험 스크립트와 1:1): `loom can open ch0 --iface virtual` → `loom can node start examples/sim/m1/bcm.yaml` (또는 한 줄 `loom can demo`).
설정: `ch0 = virtual`, `can-sim` worker 안 가짜 ECU `BCM(sim)`. 규칙: `0x300` 수신 → 5 ms 안팎에 `0x301` 응답(byte0 에코), 100 ms 주기 `0x310`의 byte0을 요청값으로 변경. 가짜 ECU 프레임은 실 ECU와 같은 경로로 `origin=device` (+`sim_node=BCM`).

| 시각 | 누가 | 동작 | 사용자 화면 (목표) |
|---|---|---|---|
| T0 | 사용자 | `loom can monitor ch0` (에이전트가 이미 1시간째 버스를 쓰는 중이어도 됨) | 1 s 안에 최근 backlog + Fixed 스냅샷 + 활성 TX 작업 표시 후 라이브로 이어짐 |
| T1 | 에이전트 | `loom can send 0x300#01` | — |
| T1+수 ms | 데몬 | 정책 통과(sim) → 이벤트 스트림에 `tx agent` (seq 5012) → `bus.send` | `AGT 0x300 01` 행 (p95 < 200 ms) |
| T1+~5 ms | BCM(sim) | `0x301 01` 응답, 다음 `0x310` byte0=01 | `DEV 0x301 01`, Fixed 행 0x310 byte0 하이라이트 |
| T2 | 에이전트 | `loom can trace --since @last` | Activity `agent:claude-1 read ch0 trace (3 rows)` |
| T3 | 사용자 | TUI `:` → `send 0x300#00` (M3: 웹 IG에서 Light_Req=Off → Send) | `USR 0x300 00`, 이어서 `DEV 0x301 00` |
| T4 | 에이전트 | `loom can trace --since @last --ids 0x300,0x301` | 에이전트 출력 마지막 줄 `NOTE: user sent 1 frame since your last read` (커서는 채널 단위라 필터를 바꿔도 기준점 유지, §3.5) |

에이전트 CLI:
```text
$ loom can send 0x300#01
sent ch0 0x300 [1] 01   seq 5012  state=sent  origin=agent:claude-1  risk=R1(sim)   viewers: 1

$ loom can trace --since @last
seq   t(s)      ch  dir org  id     dlc data
5012  812.004   ch0 tx  AGT  0x300  1   01
5013  812.009   ch0 rx  DEV  0x301  1   01          sim:BCM
5021  812.050   ch0 rx  DEV  0x310  8   01 00 00 00 00 00 00 00
-- 3 rows (periodic 0x310: 1 shown, --all-periodic) | cursor -> 5021

$ loom can trace --since @last --ids 0x300,0x301
seq   t(s)      ch  dir org  id     dlc data
5644  818.731   ch0 tx  USR  0x300  1   00          user:tui
5645  818.736   ch0 rx  DEV  0x301  1   00          sim:BCM
-- 2 rows | cursor -> 5645
NOTE: user sent 1 frame since your last read
```

사용자 TUI (T3 직후, Chrono 모드):
```text
┌ ch0 virtual  load 3%  REC ok  [Chrono]  clients: agent:claude-1, user:tui ───── LIVE lag 40ms ┐
│ 812.004  tx AGT  0x300  1  01                          agent:claude-1                       │
│ 812.009  rx DEV  0x301  1  01                          sim:BCM                              │
│ 812.050  rx DEV  0x310  8  01 00 00 00 00 00 00 00                                          │
│ 818.731  tx USR  0x300  1  00                          user:tui                             │
│ 818.736  rx DEV  0x301  1  00                          sim:BCM                              │
├─────────────────────────────────────────────────────────────────────────────────────────────┤
│ activity 818.90  agent:claude-1 read ch0 trace (2 rows)                                     │
│ :send 0x300#00                                                  [Enter] send  [Esc] cancel  │
└ F fixed/chrono  / filter  : command  Space pause  Ctrl-] x / Ctrl-X STOP  q quit ─────────────┘
```
실HW(M2/M4)에서는 `BCM(sim)` 이 실 ECU로 바뀔 뿐 명령과 화면은 같다. 에이전트와 사용자가 각자 버스를 따로 열거나 별도 가짜 스트림을 보면 **시험 실패**다.

### 2.1 에이전트 관점

**A1. "차속이 100 넘으면 알려줘"** — 연결 확인 → DB → 관찰 → 대기 (M2)
```text
$ loom can status
daemon  running (pid 41233, http://127.0.0.1:7341, up 2h13m)   viewers: 1 (user:tui)
ch0  pcan PCAN_USBBUS1  500k/2M FD  env=bench  tx=confirm  load 23.4%  err 0  REC ok
ch1  virtual sim1       500k        env=sim    tx=allow    load  4.1%  err 0  REC ok
db   ch0: powertrain.dbc (142 msgs, 1381 sigs, sha256 9f3c…)

$ loom can sig get VehicleSpeed
VehicleSpeed = 37.25 km/h   (ESP_01 0x0FD, age 18 ms, quality ok)

$ loom can sig wait-for "VehicleSpeed > 100" --timeout 120s
MATCH VehicleSpeed = 100.75 km/h at t=7801.022 (waited 88.6 s)
```
타임아웃: `TIMEOUT after 120 s; last VehicleSpeed = 87.0 km/h (max 97.5)` + exit 3 (`E_TIMEOUT`, 조건 대기형 timeout — A6). 대기 중 사용자 화면 Activity에 `agent:claude-1 waiting VehicleSpeed > 100 (timeout 120 s)`.

**A2. "IG처럼 EngRPM 3000으로 주기 송신, 그다음 4500으로"** — 신호 단위 송신 (M2)
```text
$ loom can periodic start EngineData --period 10ms --set EngRPM=3000 --channel ch1 --dry-run
DRY-RUN risk=R1(sim)  periodic 0x100 EngineData every 10 ms
  bytes 0B B8 5A 00 00 00 00 00  (EngRPM=3000 rpm; others = DBC start values: EngTemp=90)
$ loom can periodic start EngineData --period 10ms --set EngRPM=3000 --channel ch1
started job p3  EngineData 0x100 @10ms ch1  rev 1  lease none(sim)
$ loom can sig set EngRPM=4500 --job p3          # --expected-rev 생략 시 서버가 현재 rev로 CAS, 충돌이면 거절
p3 EngineData: EngRPM 3000 -> 4500 rpm  rev 2 (next cycle)
$ loom can periodic stop p3
```

**A3. "부팅 직후 에러프레임 났는지, 그 구간 트레이스"** — 통계 + 시간 질의
```text
$ loom can stats --window 60s
ch0 last 60 s: frames 41,882 (698/s)  load est 23.1% (peak 41.0%)  err-frames 3  state ERROR_ACTIVE
errors at t=7690.112, 7690.114, 7690.118   (error frame support: yes)
$ loom can trace --around 7690.114 --window 20ms --decode
```

**A4. "새로 온 0x7E8만"** — 커서
```text
$ loom can trace --since @last --ids 0x7E8 --max 50
-- 3 rows | cursor -> 1840297 | gaps: none
```

**A5. "이 BLF에서 냉각수 온도 추이 띄워줘"** — viz (M3)
```text
$ loom viz show drive_0412.blf --db powertrain.dbc --y CoolantTemp,EngRPM
view v7 "drive_0412.blf"  http://127.0.0.1:7341/v/v7   (1.84 M frames, 0..1823.4 s, ingest done)
$ loom viz summary v7 --json      # 에이전트는 수치 덤프 대신 요약만
```

**A6. UDS 팀 경로** — `loom uds connect --can ch0 --tx 0x7E0 --rx 0x7E8` 가 내부적으로 CAN worker의 `can.isotp.open` 을 호출. CAN Trace에 FF/CF/FC가 `origin`(요청 주체) + `purpose=uds` 로 보이고 ECU 응답 프레임은 `origin=device, request_origin=agent`.

### 2.2 사용자 관점

**U1. 진행 중 작업에 늦게 참가** — `loom can monitor` → 원자적 snapshot(최근 backlog, Fixed 스냅샷, 활성 주기 작업과 소유자, 대기 중 승인, 최근 Activity) + 이후 live. 중복·틈 없음. Pause는 화면만 멈추고 수집·기록은 계속(`⋯ paused 12.4 s (51k frames)`).

**U2. 에이전트 송신 감시와 차단** — `tx=confirm` 채널에 에이전트가 송신을 시도하면 TUI 하단(M3: 웹 배너)에 승인 요청(무엇/어느 채널/주기/위험 등급/dry-run 바이트) `[A]pprove once [S]ession [D]eny`. `Ctrl-] x`/`Ctrl-X`(웹 STOP 버튼)는 정책·승인 없이 즉시 agent·script 송신과 모든 주기·리플레이 잡을 정지(사용자 1회 송신은 계속 가능, 가짜 ECU는 유지 — §10.6).

**U3. IG로 직접 개입 (M3 웹, M2 TUI 최소)** — DBC 메시지 선택 → 신호 위젯(슬라이더/값 테이블 드롭다운) → dry-run 바이트 → Send/Start. 에이전트가 같은 메시지를 이미 주기 송신 중이면 소유자·revision·lease를 보여주고 "신호 patch(revision 확인)" 또는 "명시 takeover" 중 선택. 같은 ID에 두 번째 주기 producer를 만들지 않는다. 단 **사용자의 1회 송신은 producer가 아니며 우선한다**(§10.2): 에이전트 job이 0x300을 보내는 중에도 사용자의 `:send 0x300#00`은 나가고, Trace에 `USR override` 배지, 에이전트 출력에 `NOTE: user overrode 0x300 (job p3)`. TUI `:takeover p3`로 job 소유권을 넘겨받는다.

**U4. Trace → Graphics 조사 (M3)** — 심볼릭 Trace에서 신호 클릭 → Graphics 추가 → 확대 → 커서 2개 Δt/Δvalue. 커서 값은 근사 점이 아니라 해당 구간 원본 샘플을 조회해 확정.

**U5. UART/UDS 데이터 겹쳐보기 (M3)** — `loom viz show uart:dut --extract 'temp=(?P<temp>[-\d.]+)' --live` 와 CAN 신호를 같은 시간축에 표시. 시각 품질(CAN HW vs UART host read) 차이를 범례에 표시.

**U6. Statistics (M4)** — 채널별 부하(추정/실측 구분), fps, 에러 카운터, ID별 실제 주기 vs DBC `GenMsgCycleTime`(±20% 이탈 하이라이트). 지원되지 않는 값은 0이 아니라 `지원 안 됨`.

---

## 3. 아키텍처

### 3.1 결론 (v2.0)
1. 사용자에게는 여전히 **`loomd` 하나**(포트·URL·승인·감사 하나, D1). `loomd`·`loom` CLI는 CORE 소유 Rust.
2. CAN은 두 부분: **gateway 안 CAN 모듈**(Rust crate, `loomd`에 링크 — 데이터 질의·디코드·정책 훅·증거) + **CAN worker 프로세스**(Rust 실행 파일 — 장치·타이밍·기록). 경계: "**장치·벤더 DLL·타이밍 스레드는 worker, 사람과 에이전트가 묻는 질의는 gateway**".
3. **worker는 백엔드별로 나눈다**(D55): `can-sim` / `can-pcan`(Windows, `PCANBasic.dll`) / `can-socketcan`(Linux, PCAN `peak_usb` 포함) / `can-vxl`(Windows, `vxlapi64.dll` — VN5640 CAN FD). 실행 파일 하나(`loom-can-worker --backend …`), 프로세스는 백엔드마다. 기본은 물리 장치당 1 worker(최대 4 CAN 채널 프로파일), 독립 장애 범위가 필요하면 장치별로 더 나눈다. 같은 물리 채널을 두 worker가 열지 않는다.
4. **분리 근거가 바뀌었다(D56).** v1.2 근거(Python GIL: FC p99 1.5→52 ms [실측])는 Rust에서 약해진다. v2 근거는 **C2**: ① 벤더 DLL 크래시가 데몬·다른 장치·저널을 죽이지 않음, ② worker별 메모리 예산 집행, ③ 타이밍 스레드(주기·FC)를 다른 부하와 OS 수준에서 분리. Rust FC·주기 지연은 다시 잰다(§9.4, 첫 실측: macOS 기본 sleep 10 ms 주기 오차 p99 4.53 ms — 목표 3 ms 미달 [실측, `spikes/can/rust/RESULTS.md`]).
5. Python은 CAN 계획에 남지 않는다. CANoe 브리지(P4)도 Rust COM(`windows` crate `IDispatch`)을 먼저 시험하고, 재현 가능한 blocker가 나올 때만 Python 어댑터 worker 예외를 결정 기록에 남긴다(C1, D71).

### 3.2 gateway CAN 모듈 vs CAN worker — 책임 표
| 책임 | gateway CAN 모듈 (loomd 안) | CAN worker (별도 프로세스) | 이유 |
|---|---|---|---|
| 장치 열기·드라이버·벤더 DLL | – | ✓ (DLL은 해당 worker에서만) | 크래시 격리 (C2) |
| 채널 레지스트리·라우팅(ch → worker) | ✓ | – | 사용자에게 채널 이름 하나 |
| 수신 스레드, `source_seq`, 드라이버 시각 매핑 | – | ✓ | 타이밍 |
| 고정 링(저널·ISO-TP용) | – | ✓ 65,536 슬롯 (§9.4) | |
| **정본 저널 쓰기** | – | ✓ (CORE 저널 라이브러리를 worker 안에서) | gateway가 죽어도 기록 계속 (`can-core.md` W3) |
| TX 중재·TX latch·프레임별 예약 ID/rate/하드 차단 재집행 | – | ✓ (A1 잡형 단위 송신) | 장치 바로 앞의 마지막 방어선 |
| 주기 heap, ISO-TP FC/CF | – | ✓ 전용 스레드 | 타이밍 |
| SimNode·가짜 ECU·결함 주입 | – | ✓ `can-sim`만 | 실HW worker에 시뮬 부하 금지 |
| 버스 통계 원천(부하·에러 카운터·버스 상태) | – | ✓ 1 s 집계 | |
| 링 미러, Fixed, trace·expect 질의, 커서·`others_since_last` 집계, Activity | ✓ | – | |
| DBC 파서·codec, SignalCache, `wait-for`/`check` 평가, 신호 시계열(min/max 피라미드) | ✓ (요구된 메시지만, 상한 작업 큐) | 인코드된 바이트만 받음 | worker를 작게 (D58) |
| 증거 생성·재검증(`can.evidence.verify`) | ✓ (저널 재조회) | – | §9.5 |
| 정책 훅 `classify / approval_binding / presend_check`, 위험 표·`safe_state` | ✓ | presend용 장치 상태 응답 | A2 |

### 3.3 worker 분리안 비교 (C2 관점)
| 안 | 크래시 반경 | 메모리 예산 | 다채널 시계 일관성 | 판정 |
|---|---|---|---|---|
| A. 실HW worker 하나 | `vxlapi64.dll` 크래시가 PCAN까지 끊음 | 벤더 구분 불가 | 좋음 | ✗ |
| **B. 백엔드(장치)별 worker** | 그 벤더·장치만 재기동 | 벤더별 | 같은 XL 포트 안 채널끼리 같은 시계 | **채택** |
| C. 채널별 worker | 가장 작음 | 가장 세밀 | 나쁨 — XL은 한 앱 포트에 채널을 묶어야 같은 HW 시계, 같은 채널 두 프로세스면 init access 경합 | ✗ |

- worker는 필요할 때 띄우고(첫 채널 open 또는 `autostart`), 마지막 채널을 닫으면 유휴 30 s 뒤 종료(CORE 감독 규칙과 맞춤).
- `can-vxl` worker는 같은 VN5640의 CAN FD 채널들을 한 XL 포트로 함께 소유한다(같은 앱 이름·HW 시계). VN5640 이더넷 포트는 Loom이 열지 않는다(C4.1). 미디어 컨버터 모드로 DoIP에 쓰는 동안 같은 장치의 CAN FD 채널 동시 사용은 M2b 실측 항목.
- 크래시 → CORE 감독자 재기동(목표 backoff 1/2/4/8/30 s, 5분 내 3회면 자동 재시작 중단·failed). **주기 job·ISO-TP 전송·replay는 자동 재개하지 않는다**(v1.2 유지). `autostart` 채널만 장치 identity 재검증 후 다시 열고, 상태 바에 `RESTARTED — jobs stopped` + `source.gap{cause:worker_restart}`. shared open 실패를 configure로 바꾸지 않는다.

### 3.3.1 프로세스 구성도 (v2.0)
```text
  사용자: Loom UI 앱 (브라우저, 127.0.0.1)              에이전트: Claude Code / Codex
   [셸: 채널 바·STOP·승인·Activity·env·테스트]             └ loom can … (Rust 단일 바이너리, 단발)
   [CAN 패널: Trace · Graphics · IG · Data/State]                  │
            │ HTTP(정적) + WS (viewer role)                          │ WS JSON-RPC (agent role)
            ▼                                                        ▼
 ┌──────────────────────────── loomd (Rust, CORE 소유) ──────────────────────────────────┐
 │ IPC·role · 이벤트 허브(stream_seq·snapshot·우선 레인·적응형 배치) · 정책/승인/감사 · 감독자 │
 │ 테스트 세션 코어(C7) · 저널 색인/조회                                                  │
 │ ┌──────── CAN 모듈 (CAN 팀 crate) ─────────┐  ┌ UDS 모듈(세션 관리자) ┐ ┌ UART … ┐    │
 │ │ 채널 레지스트리 · 링 미러 · Fixed          │  │  can.isotp.* RPC ────┼─┐            │
 │ │ trace/expect/check 질의 · 커서·others 집계 │  └──────────────────────┘ │            │
 │ │ DBC codec·SignalCache·신호 시계열 · 증거   │                            │            │
 │ └──────────────────────────────────────────┘                            │            │
 └───────────┬───────────────┬────────────────┬──────────────────┬─────────┴────────────┘
   worker 프로토콜(CORE: 로컬 소켓/named pipe, 바이너리, 대량·우선 두 레인, 크레딧, 하트비트)
             ▼               ▼                ▼                  ▼
      ┌ can-sim ─────┐ ┌ can-pcan(Win) ┐ ┌ can-socketcan(Lx) ┐ ┌ can-vxl (Win) ─────────────┐
      │ sim 버스(소켓)│ │ PCANBasic.dll │ │ socketcan FD       │ │ vxlapi64.dll (자체 FFI)     │
      │ SimNode·가짜ECU│ │ 공식 Rust FFI │ │ netlink 읽기 전용  │ │ CAN FD (XL CAN API만)       │
      │ 결함 주입     │ │ RX·TX·주기·TP │ │ RX·TX·주기·TP      │ │ RX·TX·주기·TP               │
      │ 저널 writer   │ │ 저널 writer   │ │ 저널 writer        │ │ 저널 writer                 │
      └──────────────┘ └───────────────┘ └────────────────────┘ └─────────────────────────────┘
                                     (P4, Win) canoe-bridge: Rust COM 우선 ──COM──► CANoe
```
worker 내부 스레드: 벤더 RX(정규화·seq·링 fanout만 — decode·직렬화·압축 금지) · **TX 중재 + ISO-TP + 주기 heap 전용 스레드**(FC/만료·STOP·사용자 개입 우선, 주기 job은 유한 시간 단위) · 저널 writer · IPC 펌프. 소수 제어 RPC는 tokio에서 이 스레드로 메시지 전달. Windows는 notification HANDLE + 고해상도 waitable timer, 우선순위는 best effort(성공 여부를 capability에 표시), 전역 timer resolution 변경·busy spin은 기본값 아님. DLL 호출이 멈추면 watchdog이 worker를 끝낸다.

### 3.4 RX 데이터 흐름
```text
Driver ─event─► 벤더 RX 스레드 ─► (a) 고정 링 append(try_send, 즉시 반환)  (b) ISO-TP 디스패처(try_send)
                                        │
                  source_seq 부여 ──► RingBuffer[ch](단기) ──► 저널 writer (별 큐, 무손실 경로)
                                        │
          ┌ 우선 제어 큐: isotp PDU/rx_start/오류, 채널 상태, STOP, tx 결과 ──► 즉시 전달 (배치 대기 없음)
          └ 프레임 배치(≤10 ms 또는 200프레임, 내부 IPC) ──► gateway: stream_seq 부여 → 조회용 링 미러
                                                   (채널별 최근 N프레임 + Fixed 테이블) → 디코더/SignalCache,
                                                   커서·trace.query·snapshot·NOTE 집계 → 구독자별 필터·
                                                   화면 예산·WS 배치(≤40 ms) ──► UI 앱
```
- **gateway 링 미러**가 trace.query·snapshot·커서·NOTE 집계를 처리한다(worker에 매번 질의하지 않음). worker 링은 저널·ISO-TP용 단기 버퍼. 미러 밖 구간은 저널(source_seq/시간 기준)에서 읽는다.
- **적응형 배치(A5):** 40 ms는 상한. 조용한 구간 뒤 첫 이벤트는 즉시 flush, 구독 옵션 `max_batch_ms`. 64 KiB는 이벤트 배치 1개 상한.
- **우선 제어 큐**(UDS v0.3 R-TP-15, D2): ISO-TP PDU·`rx_start`·오류·STOP·채널 상태 이벤트는 raw 프레임 배치와 분리된 큐로 먼저 전달 → 버스 100% 부하에서도 PDU 전달 p99 ≤ 5 ms 목표와 양립.

규칙 (두 초안 합의):
1. RX 스레드 안에서 decode·직렬화·파일 I/O·압축·사용자 코드 금지. 상한 큐에 `try_send`하고 반환(드라이버 수신을 막지 않음, 넘치면 해당 gap 카운터 — §9.4).
2. 장치마다 수신 reader는 하나. ISO-TP는 RX 스레드가 나눠 주는 디스패처 큐로만 받는다(직접 recv 금지).
3. **관측 필터 ≠ 장치 필터.** 사용자 Trace 필터가 ISO-TP FC/RX를 제거하면 안 된다. HW 수신 필터는 전체 소비자의 합집합.
4. 화면 ack는 수신/기록 완료의 조건이 아니다. 기록 경로와 화면 경로는 큐가 분리.
5. 디코드 실패는 raw 프레임을 버리는 이유가 아니다.

### 3.4.1 TX 데이터 흐름
```text
CLI/뷰어/UDS ─RPC─► gateway: 인증 role → origin 결정(서버가 정함, 자칭 불가)
   → core 쓰기 파이프라인 (A1 요청형): classify 훅(risk) → policy(env/tx, 차단 3층, rate) ─ 거부 → exit 4
        → confirm/approval(R2 경로 정책 / R3·R4 viewer) ─ 대기·거부 → exit 5 → audit
        → presend_check 훅(버스 상태, 예약 ID, latch — 실패 시 승인 무효, E_LOCKED/E_QUARANTINE → exit 6)
   → CAN worker: TX 중재기 → 이벤트 `can.tx` 행 state=requested→accepted 발행(같은 request_id 행 갱신)
                → bus.send → state=sent(드라이버 접수) / failed / outcome_unknown(send timeout·끊김)
                → (backend가 TX 확인을 주면) confirmed, 아니면 `confirmation=unavailable`
                → (선택) `send --expect ID` 매치 시 effective_verified, 아니면 결과는 effect_unverified
   → publish-before-reply: 이벤트가 뷰어 스트림에 발행된 뒤 RPC 응답

잡형 (A1-②: 주기·IG·replay·ISO-TP 채널): core는 job의 start/update/renew/stop만 위 파이프라인으로 게이트·감사.
단위 송신(주기 프레임, CF/FC)은 CAN worker가 승인된 plan(ID·신호 범위·주기) 안에서 직접 하며,
단위마다 TX latch·예약 ID·레이트·하드 차단을 다시 집행하고 events/저널에만 남긴다(단위별 감사 없음).
```
- `sent`(= `bus.send` 성공) = **드라이버 접수**이지 wire 송신·ECU 수신 성공이 아니다. 화면/CLI에서 구분 표시(거짓 성공 금지, D4).
- 드라이버 own-message echo를 Loom seq로 자동 매칭하지 않는다(프레임에 seq가 없음, 같은 ID/payload 반복은 모호). backend receipt가 있을 때만 연결하고 아니면 별도 관측으로 남긴다. 원 이벤트 timestamp를 덮어쓰지 않는다.
- 외부 툴이 보낸 프레임은 `origin=device`(enum 유지) + `external_actor`로 구분한다(B3): `canoe` = 공존 Vector 포트에서 **다른 앱의 송신으로 표시된 프레임**(판정 근거는 P3 실측, §8.1) / `host` = Linux에서 같은 호스트 다른 프로세스가 보낸 프레임(`MSG_DONTROUTE`, python-can 4.6.1 `socketcan.py:663-664` 확인) / `unknown` = 그 밖에 Loom이 보내지 않았는데 ECU 응답이 아닌 것으로 보이는 프레임(예: Loom이 안 보낸 진단 요청 ID). 근거 없이 `canoe`로 단정하지 않는다.

### 3.5 세션 / seq / 커서
- **세션** = 데몬이 채널을 연 기간의 기록·소유 단위(`session_id`, 재오픈마다 `epoch` 증가). 사용자는 보통 의식하지 않는다: CLI는 `--channel`(기본값: `loom.toml default_channel`, 열린 채널이 하나면 생략)만 쓴다.
- **순번은 두 층**: (1) 원본 이벤트 ID = `(producer_id, producer_epoch, source_seq)` — 장치 owner worker(can-hw, can-sim, 타 도메인 worker)가 할당하고 저널에 보존. (2) **`stream_seq`** — gateway가 모든 도메인 이벤트(프레임, 승인, Activity, UART/UDS)에 단일 할당하는 전달 순번, `stream_epoch`(gateway 재시작마다 증가)와 함께. 구독 커서·snapshot high-water·`display.gap`은 `stream_seq`, 기록 손실 판정(`record.gap`)과 저널 조회는 원본 ID를 쓴다. 64-bit 값은 JSON에서 **문자열**.
- **커서 키 = `(client, channel)`** (필터와 무관). 필터는 읽을 때 적용하고 필터에 걸리지 않은 프레임도 "본 것"으로 커서가 전진한다. 전진시키지 않으려면 `--peek`. 응답에 `cursor_before`/`next_cursor`를 넣어, 응답 유실 시 클라이언트가 `--since seq:<cursor_before>`로 다시 읽을 수 있다. `NOTE:`는 필터와 무관하게 커서 이후의 user origin 이벤트 전부를 센다.
- `--since @last|seq:N|-5s|HH:MM:SS.mmm`. 미러에서 밀려난 구간은 `gap: stream_seq N..M not in memory (journal: <path>)` + 저널 경로.
- client 이름: CLI는 `LOOM_CLIENT`(없으면 부모 셸 세션+사용자로 안정 id 생성 — D5; 여러 에이전트가 같은 이름을 동시에 쓰면 gateway가 `hint:`로 경고), 뷰어는 `user:<tui|web-탭>`.
- `--since mark:X`(record mark 위치)도 지원(D5). CAN 고유 커서 접두어는 두지 않는다.

### 3.6 ISO-TP 호스팅 위치 (v2.0)
```text
UDS 세션 관리자(UDS 팀, gateway 안) ─ PDU RPC(can.isotp.*) ─► CAN worker: ISO-TP 상태기계(CAN 소유 Rust)
                                         TX 중재 전용 스레드가 FC/CF·STmin·BS·N_Bs/N_Cr 처리
       ISO-TP가 주고받은 FF/CF/FC 원시 프레임도 같은 이벤트 스트림/저널에 실림 (숨은 세션 없음)
```
- **엔진은 직접 구현**(D65). 근거는 "crate가 없어서"가 아니다: `automotive` 0.6.0·`can-iso-tp` 0.2.0은 codec·BS/STmin을 갖췄지만, Loom이 요구하는 최종 TX 관문·단일 reader·`driver_accepted`/`bus_confirmed` 두 완료·abort 뒤 잔여 보고·drain barrier·원시 프레임 증거·결함 훅을 한곳에서 소유해야 한다. 두 crate는 differential fixture oracle로 쓴다. `ecu_diagnostics`(GPL-3.0)·`isotp-rs`(모든 버전 yanked)는 채택하지 않는다.
- FC/CF 타이밍은 worker의 전용 스레드에서 처리 → IPC 지연 무관, IPC로는 PDU만. 계약(`can.isotp.*` 이름·필드·`tx_state`·예약 ID·barrier)은 `_xteam/can-uds-isotp.md` 그대로(C3).
- `bus_confirmed`는 그 백엔드에서 자기 전송의 TX 완료를 상관할 수 있을 때만(PCAN `WriteFD` 성공 = `driver_accepted`, XL TX_OK 등은 실기 확인 후). ACK가 없는 백엔드는 승격 금지.
- Rust 버전 FF→FC 지연은 미측정. M0에서 저널 on + 20k fps + gateway 디코드 부하 조건으로 잰다(목표 host p99 ≤ 3 ms, 약속 아님).

### 3.7 가시성 불변식 (사용자 화면이 에이전트 세션과 분리되지 않음)
하나라도 깨지면 버그이며 §9.3 자동 시험으로 고정한다.
- **I1 단일 경로.** 장치 읽기/쓰기는 CAN worker만 한다. CLI·SDK·웹·UDS 모두 gateway 클라이언트. 에이전트가 python-can으로 장치를 직접 여는 스크립트는 스킬에서 금지.
- **I2 비공개 토픽 없음.** RPC 스키마에 private/hidden 옵션이 없다. 에이전트 송신, 사용자 송신, ISO-TP FF/CF/FC, 주기 작업, 가짜 ECU 프레임 모두 `bus.send` **전에** 이벤트 스트림에 실리고(state=requested/accepted), RPC 응답은 발행 뒤에 나간다(publish-before-reply, D2).
- **I3 늦게 열어도 같은 화면.** 구독 = 원자적 `snapshot(high-water stream_seq S)` + `after_seq=S` live. snapshot에는 최근 backlog(기본 30 s 또는 10k 프레임, 상한 2 MiB), Fixed 스냅샷, 활성 TX 작업·소유자, 대기 중 승인, 최근 Activity. 재시작 시 `epoch` 와 retention gap 반환.
- **I4 에이전트의 읽기도 보인다.** trace/sig get/wait-for/stats 질의는 Activity 이벤트로 뷰어에 표시.
- **I5 뷰어 부재 경고.** `status` 와 모든 송신 결과에 `viewers: N`. 0이면 `warnings[]`에 `no_viewer_attached`(D9) + `hint: user is not watching — ask them to run 'loom can monitor'`. 스킬은 세션 첫 송신 전 이 안내를 사용자에게 전달. (`viewer.auto_open` 설정 시 첫 연결에 브라우저 열기, 기본 off)
- **I6 사용자 개입이 에이전트에 보인다.** 모든 결과에 `others_since_last{count, origins, from_cursor}`(D5) — 에이전트 커서 이후 user origin 이벤트(송신, 신호 변경, 작업 정지, takeover). 사람용 출력은 `NOTE:` 요약 줄.

### 3.8 실시간성 목표와 고부하 정직성

**목표치** (정상 프로파일 = 1채널 5,000 frames/s + 뷰어 2개 + ISO-TP 대화 1개, 기준 머신. 측정 정의: CAN worker host ingest(`mono_ns`) → 클라이언트 렌더 완료 시각. 웹은 WS ping으로 시계 오프셋 추정)

| 경로 | 목표 p95 | p99 | 비고 |
|---|---|---|---|
| 장비 프레임 → **UI 앱** Trace paint | < 150 ms | < 300 ms | 배치 40 ms + rAF, 브라우저 계측 회신 `ui.latency_sample` (**M1 gate**, D62) |
| agent/user TX 요청 → 같은 세션 화면 | < 100 ms | | state=requested 기준 |
| 신호 → Data/Graphics 갱신 | 표시 age < 250 ms | | Graphics 10~20 Hz (M3) |
| UDS 트랜잭션 이벤트 → 화면 (V-UDS-0) | ≤ 100 ms | ≤ 200 ms | M3, UDS 요구 |
| UART 추출 값 → 플롯 (V-UART-0) | ≤ 200 ms | | M3 |
| 늦게 붙은 뷰어 첫 화면 | < 1 s | | I3 |
| gateway accept → 드라이버 submit | < 10 ms | | CLI 기동 시간 제외(기동 목표 < 300 ms 별도) |
| STOP → 신규 submit 차단 | < 50 ms | | 이미 드라이버 큐에 들어간 프레임은 취소 불가 — 별도 보고 |
| ISO-TP PDU 완성 → UDS 수신 | | ≤ 5 ms | UDS v0.2 요구, M2에서 측정·재협의 |
| 과부하 배지 표시 | 탐지 후 < 1 s | | |

| 장비 프레임 → `trace --since @last`에 보임 | < 50 ms | | gateway 미러 반영 기준 |
| 장비 프레임 → `wait-for` 반환 (M2) | < 100 ms | | SignalCache 트리거 |

**지연 예산 (웹 p95 < 150 ms 배분, v2.0):** worker ingest → gateway ≤ 10 ms + gateway 처리 ≤ 10 ms + WS 배치 ≤ 40 ms(적응형) + 브라우저 디코드·상태 반영 ≤ 20 ms + paint ≤ 16 ms ≈ 96 ms 명목.

모든 성능 수치는 **기준 머신**에서 측정·판정하고, CI 3 OS는 같은 시나리오를 **논리 시험**(순서·출처·무중복·무누락)으로 돌린다. M1 sim에서 위 목표 중 웹 paint·late join·TX 표시·trace 가시성을 측정하고, M2부터 실HW에서 재측정한다. 고부하 프로파일은 다채널 합계 20,000 frames/s, 물리 버스 부하 100%(별 조건). 산술 추정: Classic 500 kbit/s 8바이트 프레임 100% 부하 ≈ 3.7~4.5k fps(프레임당 약 111~135 bit). FD는 payload·bitrate에 따라 다르므로 프로파일에 고정해 시험한다(frame rate ≠ bus load).

**고부하에서 멈추지 않고 거짓말하지 않기**
1. **기록은 무손실 경로, 화면은 예산제.** 저널 큐와 화면 큐는 분리. 화면 예산: 웹 Chrono 2,000 행/s(U-CAN-1), (보조 TUI 300 행/s), 구독별 대기열 2 MiB, 보이는 Trace 최근 5,000행.
2. **생략은 항상 표시.** 예산 초과분 자리에 합성 행 `⋯ 3,812 frames not shown (seq 9001..12812; 0x0FD×1200 …) — recording ok` + 헤더 누적 카운터. 생략을 필터 결과나 "수신 0건"처럼 보이게 하지 않는다. 화면 생략(`display.gap`)과 기록 손실(`record.gap`)은 다른 이벤트·다른 문구.
3. **Fixed 모드**는 ID 수에 비례하므로 고부하에서도 모든 프레임을 반영(최신값·count·주기). 단 이 정확성은 capture 손실이 없을 때만 — capture/record gap 이후에는 count·주기에 `?` 표기(예: `×41210?`, `10.0ms?`)와 툴팁 "gap 이후 불완전". 고부하 시 Fixed 전환 제안 배너.
4. **플롯 다운샘플은 명시.** 기본 = 픽셀 버킷 min/max(M4 계열: first/last/min/max 보존) → 버킷 내 극값은 보존. MinMaxLTTB는 성능 옵션이며 극값 완전 보존을 약속하지 않는다. 축 옆 `downsampled 1,000,000→4,000`. 알람·wait-for·summary·커서 값은 **원본 샘플**로 계산.
5. **지연 정직 표시.** 헤더 `LIVE lag 40 ms` → 목표 초과 시 노란 `LAG 620 ms` → 2 s 초과 시 빨강 + 해당 클라이언트 backlog를 버리고 최신으로 점프(`⋯ jumped 2.3 s to live`).
6. **진짜 손실 구분 (손실 3층, A13).** Loom 수신 전 손실(드라이버/USB 수신 큐 오버런, 버스 에러로 인한 누락 보고) = `source.gap{cause:"driver_overrun"…}`, Loom 기록 손실(저널 큐 초과, 디스크 full, 프로세스 크래시) = `record.gap{range | count=unknown, cause}`, 화면 드롭 = `display.gap`. 앞의 둘은 빨간 `SOURCE LOSS`/`RECORDING INCOMPLETE` 배지 + 에이전트 `warnings[]`·`WARNING: data loss on ch0 (...)`. 정확한 수를 모르면 `unknown`.
6a. **배지 공통(D9):** `LIVE / LAG / STALLED(heartbeat 1 s 끊김) / LIVE-DEGRADED(생략·다운샘플 중)`.
7. **Pause ≠ 수집 정지.** 재개 시 `⋯ paused 12.4 s (51k frames)`.
8. **에이전트 출력도 같은 규칙.** `--max` 초과 시 `-- 100 of 41,882 (truncated; next: --since seq:N)`. 무음 절단 없음. 응답 메타에 `display/record gap` 포함.
9. 상태·오류·승인·STOP·최종 결과 이벤트는 화면 예산·생략 대상에서 제외(항상 전달).
10. CAN 수신은 backpressure로 멈출 수 없다 → "무한 부하/디스크 장애에서도 무손실"은 약속하지 않는다. 약속은 "정해진 처리량 프로파일에서 무손실 + 손실 시 반드시 표시". 기록 불가 상태에서는 실HW 신규 TX를 막는다(fail-closed).

---

## 4. CLI / API 명세 초안

### 4.1 공통 출력 계약 (마스터 D5/A6 확정안)
| 규칙 | 값 |
|---|---|
| 기본 출력 | 사람용 정렬 텍스트. 마지막 줄에 행 수/커서/잘림/gap 요약, 필요 시 `NOTE:`/`WARNING:`/`hint:` |
| `--json` | D5 봉투 `{schema_version, ok, cmd, data, meta{truncated, next_cursor, range, matched, display_gap, record_health}, warnings[], others_since_last{count, origins, from_cursor}, error{code, message, hint, retryable, request_id, detail}}`. stdout=결과, stderr=진단 |
| 출력 상한 | 기본 `--max 100` 행, 하드 상한 1,000행 / 64 KiB. 넘으면 잘림 표시 + 다음 커서 |
| 무한 스트림 | 에이전트용 무한 스트림 없음. `trace --follow` 는 `--count N`과 무관하게 **항상 timeout이 적용**(기본 10 s, 최대 600 s)되고 둘 중 먼저 도달하면 종료. 단일 결과 대기(`sig wait-for`)는 최대 3600 s(A6). 무한 구독은 사람용 `monitor` 만 |
| 시간 지정 | `--since @last\|seq:N\|mark:X\|-5s\|HH:MM:SS.mmm\|ISO8601`, `--until`, `--around T --window 20ms` |
| 채널 | `--channel ch0` (생략 시 default_channel, 열린 채널 1개면 자동) |
| 대기형 | `wait-for`, `--follow`, 승인 대기는 서버측에서 경계 내 완료 후 1응답 |
| 종료 코드 (D5/A6) | 0 성공 · 1 내부 · 2 사용법·설정·DB(`E_USAGE`, `E_AMBIGUOUS_CHANNEL`, `E_DB`) · 3 **조건 대기형 timeout만**(`sig wait-for`, `trace --follow` 무매치, 승인 대기 — `E_TIMEOUT`) · 4 정책 거부(`E_DENIED` — 사용자 설정 하드 차단·예약 ID(`detail.reason=tx_reserved`)·`e2e_protected`, `E_RATE_LIMIT`, `E_RECORDING_FAILED`) · 5 확인·승인 필요/거부/만료(`E_CONFIRM_REQUIRED`, `E_APPROVAL_REQUIRED`, `E_NO_APPROVER`) · 6 사용자·락 충돌(`E_LOCKED` = ISO-TP busy/producer 중재, `E_INTERRUPTED_BY_USER` = 사용자 takeover·STOP으로 job 중단, `E_QUARANTINE`) · 7 장치·전송(`E_NOT_OPEN`, `E_DISCONNECTED`, `E_BUS_OFF`, `E_NO_RESPONSE` — 쓰기는 `outcome_unknown` 확인) · 8 부분 결과(`E_CURSOR_GAP`) · 9 데몬 연결 불가(`E_DAEMON`) · 10 프로토콜 부정 응답(CAN 자체는 해당 없음) |
| 환경 배지 (B4) | 채널 env가 `vehicle`이면 사람용 출력 첫 줄에 `[env=vehicle ch0]`, `--json`은 `meta.env`(채널별). 경고가 아니라 상시 표시 — 송신 규칙은 bench와 같다 |
| 오류 객체 | `{"error":{"code":"E_DENIED","message":"…","hint":"…","retryable":false,"request_id":"r9","detail":{"reason":"tx_reserved","owner":"can.isotp:tp1"}}}` |
| mutation 재시도 | 타임아웃 난 송신·작업 시작은 자동 재시도하지 않음. `idempotency_key`로 상태부터 조회 |

### 4.2 `loom daemon`
```text
loom daemon start [--foreground] [--port 0(자동)] | stop | status | logs --tail 50
```
CLI는 연결 실패 시 데몬을 자동 기동하고(`LOOM_NO_AUTOSTART=1`이면 끔, D8) discovery 파일(`~/.loom/run/daemon.json`, 0600)을 기다린다. 데몬은 장치를 **자동으로 열지 않는다** — `loom.toml` 의 `autostart=true` 채널만.

### 4.3 `loom can`
| 명령 | 설명 | 위험 | M |
|---|---|---|---|
| `status [--json]` | 데몬/채널/env/tx 정책/DB/부하/기록 상태/viewers | R0 | M1 |
| `hw list` | 장치 탐색(`detect_available_configs`) + capability(fd, hw_timestamp, tx_confirmation, periodic_engine, shared_open, listen_only, bus_load, error_frames) — `unknown` ≠ `false` | R0 | M2 |
| `open ch0 --iface virtual\|pcan\|vector\|socketcan [--hw ...] [--bitrate 500k] [--fd --dbitrate 2M] [--mode shared\|configure] [--app Loom]` | 채널 열기. 실HW 기본 `--mode shared`(설정 변경 요구 안 함). 정책은 loom.toml이 정하며 CLI는 더 엄격하게만 | R0(loom.toml에 사람이 쓴 설정 그대로) / **R4(CLI가 loom.toml과 다른 bitrate·FD·mode를 요청)** | M1(virtual) |
| `close ch0` | 작업·ISO-TP 정리, 저널 seal | R0 | M1 |
| `send ID#DATA [--ext] [--fd --brs] [--count N --gap 10ms] [--expect ID --timeout 1s] [--dry-run]` | raw 1회/유한 연속 송신 (요청형). 결과 state=sent/confirmed, `--expect` 매치 시 effective_verified | R1~R4 | M1 |
| `confirm <tok>` | R2 확인 토큰 재제출(채널 경로 정책이 `token_or_viewer`일 때만 — 실HW 기본은 viewer, §10.3) | — | M2 |
| `trace [--ids 0x100,0x7E0-0x7EF] [--msg X] [--dir rx\|tx] [--origin agent,user,device] [--since …] [--max N] [--decode] [--follow --timeout 10s]` | 트레이스 조회 | R0 | M1(raw) |
| `last [--ids\|--msg]` | ID별 최신 프레임(Fixed 스냅샷) | R0 | M1 |
| `stop-all` / `tx resume` | **TX latch**: agent·user·script origin의 모든 송신 경로 차단 + 주기 job·replay·ISO-TP 진행 중 전송 중단(§10.6). 가짜 ECU(device 역할)는 멈추지 않음(`node stop` 별도). 해제는 `tx resume`(sim 채널은 누구나, 실HW 채널은 viewer 승인) | stop R0 항상 허용 | M1 |
| `demo` | sim 채널 + 가짜 ECU + UI 링크(`loom ui open can/trace`) 한 줄 | R1 | M1 |
| `expect --id ID [--data --mask] [--origin] [--after mark:X\|seq:N\|now] --within T` | 조건 프레임이 기한 안에 나타나는지(증거 반환, §9.5) | R0 | M1 |
| `check "<식>" --from mark:A --to mark:B --mode always\|never\|eventually` | 기록된 구간에 대한 단언(대기 아님) | R0 | M2 |
| `node start <profile.yaml> \| list \| stop` | 데몬 내 가짜 ECU (sim 채널만) | R1 | M1(raw 규칙) |
| `db load X.dbc [--channel] \| list \| msg NAME \| sig NAME \| find WORD` | DB 로드·탐색 (sha256·revision 반환) | R0 | M2 |
| `sig get A[,B]` | 신호 최신값 + age + quality | R0 | M2 |
| `sig wait-for "EXPR" --timeout 30s(최대 3600 s) [--accept-current]` | 조건 대기. 기본은 **새 샘플** 기준. EXPR: `Sig > 10`, `Sig == "Drive"`, `changed(Sig)`, `A > 1 and B < 3` → 제한 AST(eval 금지). 별칭 `--name --op --value` | R0 | M2 |
| `sig series SIG --since -10s [--every 100ms]` | 짧은 시계열(상한 적용), 길면 viz 권고 | R0 | M2 |
| `send --msg MSG --set S=V [--base start\|last]` | DBC 인코딩 1회 송신. 미지정 신호 = DBC 초기값(`GenSigStartValue`), 초기값 없는 신호는 필수 입력, `last`(수신값 복사)는 명시 옵션 | R1~R4 | M2 |
| `periodic start MSG\|ID#DATA --period 10ms [--set …] [--lease 10m] [--dry-run]` / `list` / `stop ID\|--mine\|--all` / `renew ID` | 주기 송신 job (revision, lease). `--lease`는 채널 상한을 넘을 수 없고 `renew`는 Activity·감사에 남음 | R1(sim) / R3 | M2 |
| `sig set S=V --job ID [--expected-rev N]` | 주기 job 내 신호만 변경(IG). revision CAS(생략 시 서버가 현재 rev로 CAS), mux/범위 위반은 원자적 거절. 승인된 신호 범위 안이면 재승인 없음(§10.3) | R1(sim) / R3 | M2 |
| `stats [--window 10s] [--by id]` | fps, 추정 부하, 에러 프레임, 버스 상태, ID별 주기/지터 — 추정/실측 구분 | R0 | M2 |
| `record status \| mark "text" \| clip start\|stop NAME` | 자동 기록 상태, 마크, 이름 붙은 구간(자동 기록과 별개) | R0 | M1(status/mark) |
| `export SESSION\|CLIP --to out.blf\|.asc\|.csv\|.parquet\|.mf4 [--range]` | 표준 포맷 내보내기 (CPU 작업은 CAN worker 밖) | R0 | M2(BLF/ASC) |
| `replay FILE --to viewer` / `--to ch1 [--speed] [--ids]` | 재생. 버스 송신은 plan+승인 | R0 / R3+ | M5 |
| `isotp open\|send\|recv\|abort\|close\|list` | ISO-TP 디버그용 (RPC `can.isotp.*`; UDS 기본 경로는 UDS 세션 관리자) | R1(sim) / R3 | M2 |
| `approval status ID` | 승인 상태 조회만 (승인 불가) | R0 | M2 |
| `monitor [ch] [--follow-agent]` | (v2: **보조**) 터미널 사용자용 TUI. 사람의 주 화면은 UI 앱 — `loom ui open can/trace?ch=…`(공통 명령, 딥링크 출력·열기) | — | M5 |
| `loom audit tail [--max 50]` | 감사 로그 조회 | R0 | M1 |
| `loom help --agent can` | 스킬 요약 출력(스킬 시스템이 없는 에이전트용) | R0 | M1 |

### 4.4 `loom viz` (v2.0: **UI 팀 소유로 이관**. 아래는 CAN이 요구한 동작의 이력 — 정본은 `ui/plan.md`)
| 명령 | 설명 | M |
|---|---|---|
| `show SRC [--x COL] [--y A,B] [--long --series COL --value COL] [--db X.dbc] [--kind auto\|line\|step\|state\|trace\|table] [--group p1:A,B p2:C] [--layout f.json] [--no-open]` | 데이터셋 등록 + 뷰 생성 + 브라우저 열기 → view id/URL 반환하고 CLI 종료. 큰 파일은 ingest job + 진행률 | M3 |
| SRC | 파일(csv/jsonl/parquet/blf/asc/mf4/trc), `-`(stdin), `live:can:ch0`, `live:uart:<port> --extract REGEX`, `live:uds:did:F40D`, `session:<id>` | M3 |
| `push VIEW --jsonl -` | 에이전트 계산 결과를 기존 뷰에 추가(짧은 배치) | M3 |
| `list` / `close VIEW` / `summary VIEW [--json]` | 관리, 에이전트용 요약 통계(min/max/mean/구간, 원본 기준) | M3 |
| `export VIEW --csv\|--png [--max-points]` | 제한된 데이터/스냅샷 export (PNG는 M5) | M3/M5 |

자동 렌더 규칙(`--kind auto`): ① CAN 프레임 형태(id/data 열 또는 BLF/ASC/MF4 bus log) → Trace + (DB 있으면) 신호 패널. ② 시간형 열(`ts`,`time`,`timestamp`, datetime, 단조 수치) → x축. 없으면 행 인덱스. ③ 수치 → line(정수 & 고유값 ≤16 → step), 문자열/enum 고유값 ≤32 → State, 그 외 → 표. ④ long 형식(`ts,series,value,unit`) 자동 인식. ⑤ `--y` 미지정 & 수치 열 > 8 → 8개 표시 + 나머지 체크박스. ⑥ 공통 이벤트 envelope JSONL은 도메인 어댑터로(UDS 트랜잭션 → V-UDS-1 표). 애매하면 UI에서 열/시간 형식/단위 선택 → sidecar preset 저장.

### 4.5 데몬 RPC (JSON-RPC 2.0 over WebSocket, `ws://127.0.0.1:<port>/rpc`)
첫 메시지 `hello{proto, token, client_id, want_role}` → 서버가 role(agent/viewer) 부여(D2). 메서드 `<domain>.<verb>` 또는 `<domain>.<noun>.<verb>`(소문자 snake), 이벤트 `<domain>.<event>`. 요청 메시지 64 KiB 상한, ping 20 s.

| 메서드 | 요지 |
|---|---|
| `core.hello` / `core.sessions` | 버전·capability·세션 목록 |
| `core.subscribe{topics, filter, after_seq?(stream_seq), snapshot:true, max_batch_ms:40, max_batch_bytes:65536}` | 원자적 snapshot(high-water S) + live. snapshot은 `snapshot_part n/N` 여러 메시지 → `snapshot_end{stream_seq}`(A5). 결과 `{sub_id, stream_epoch}` |
| `core.unsubscribe` | |
| `core.approval.respond{approval_id, decision, scope}` | **viewer role 전용** |
| `can.channel.open/close/list`, `can.hw.list` | |
| `can.trace.query{channel, filter, since, until, max, decode}` | |
| `can.sig.get` / `can.sig.wait{expr, timeout_ms, accept_current}` | |
| `can.tx.send{channel, frame\|msg+signals, count, gap_ms, dry_run, idempotency_key}` | `{request_id, state, seqs}` 또는 `{approval_id}` |
| `can.periodic.start/update/stop/list/renew` | 잡형(A1), revision, lease |
| `can.confirm{token}` | R2 토큰 재제출 |
| `can.stats.get`, `can.record.status/mark/clip`, `can.export.start` → job | |
| `can.node.start/stop/list`, `can.stop_all` | |
| `can.isotp.*` | §15.2 (A16) |
| `viz.dataset.register/query/summary`, `viz.view.create/patch/close`, `core.job.status/cancel` | `query{dataset, cols, t0, t1, px}` → 다운샘플된 열 데이터. 64 KiB 초과 결과는 같은 오리진 HTTP GET `/data/<query_id>`(토큰·쿠키 검사) 또는 `raw_ref`(A5) |

이벤트 notification (배치):
```json
{"jsonrpc":"2.0","method":"core.events","params":{"sub":"s3","stream_epoch":"3","stream_seq_first":"1840201","stream_seq_last":"1840290",
 "events":[ … 공통 이벤트 레코드 … ],"display_gap":{"count":0},"record_health":"ok","lag_ms":38}}
```
이벤트(`<domain>.<event>`, 정확한 표기는 CORE 계약 v1): `can.frame`, `can.state`, `can.stats`, `can.decode_error`, `can.signal_sample`, `can.tx`(같은 request_id 행을 state로 갱신, A2), `can.job_state`, `can.activity`, `can.isotp_*`, 공통 `core.policy_denied`, `core.approval_*`, `display.gap`, `record.gap`, `source.gap`, `record.health`, `core.session_closed`, `core.job_progress`.
M1은 JSON 배치. 대역 추정: 축약 schema microbench는 5k fps ≈ 0.98 MB/s·직렬화 ≈ 9 ms/s였으나 현 레코드(§6.1, ≈ 540 B/행)로는 ≈ 2.7 MB/s/구독 — 로컬 WS로는 감당 가능하다고 보되 M0(a)에서 현 schema로 재측정. 열 지향 바이너리(msgpack 헤더 + TypedArray 블록)는 M3 웹 Graphics에서 측정 후 도입.

---

## 5. 사용자 화면 — UI 앱 CAN 패널 명세 (v2.0, C6)

**정본 = `_xteam/can-ui.md`**(UI 팀이 `## ui 의견`으로 회신). 아래는 요약. v1.2의 TUI(§5.2)·웹(§5.3) 설계는 이 명세의 출발점이었고 TUI는 보조(M5)로 내려갔다.

### 5.0 소유 경계
- **CAN 팀:** 패널의 *내용* — 무엇을, 어떤 데이터(이벤트 kind·RPC)로, 어떤 지연·고부하 규칙으로, 사용자가 어떤 입력으로 개입하는지 + gateway CAN 모듈의 질의 RPC.
- **UI 팀:** 셸·패널 프레임워크·렌더링(가상 스크롤·플롯)·상태 관리·레이아웃·공통 요소(STOP·승인 대기열·env 배지·LIVE/LAG·Activity·마커·테스트 실행 화면).
- **CORE:** 이벤트 허브·구독·바이너리 전송 형식(`_xteam/core-ui.md`), 스키마 → TS 타입.

### 5.1 공통 규칙과 지연
- 출처 배지 색 + 문자: `AGT` / `USR` / `DEV` / `EXT`(`canoe`·`host`·`?`), 보조 태그 `sim:BCM`, `UDS tp1`, `job p3`, `test s2`. TX 행은 같은 `request_id` 행을 `requested → accepted → sent → confirmed`(또는 `failed`/`outcome_unknown`)로 갱신하고 `sent` = 드라이버 접수임을 표시(D4/A14).
- env 배지 `SIM`/`BENCH`/`VEHICLE`(VEHICLE 반전색, B4). 손실 3층 문구 분리: `display.gap` = 합성 행 `⋯ N frames not shown — recording ok`, `record.gap`·`source.gap` = 빨간 행 + `RECORDING INCOMPLETE` / `SOURCE LOSS`(A13). 신선도 `LIVE`/`LAG`/`STALLED`/`LIVE-DEGRADED`(D9). STOP 중 `STOP — agent/script TX latched`, 사용자 송신은 계속(A3).
- **지연 게이트 [목표]:** 장치 → paint p95 < 150 ms / p99 < 300 ms(UI 브리프 CAN 200 ms보다 엄격), TX 요청 → `requested` 행 p95 < 100 ms, Graphics 표시 age p95 < 250 ms. 계측: UI가 표본 프레임 paint 시각을 `ui.latency_sample`로 회신(U-CAN-7).

### 5.2 패널 요약
| 패널 | 핵심 내용 | 데이터 | 사용자 개입 | M |
|---|---|---|---|---|
| 채널 상태 바 | 드라이버(`PCAN-USB FD`/`VN5640 ch3`/`SocketCAN can0`/`sim`)·CC/FD 비트레이트·모드·버스 상태·부하·에러·TEC/REC·env·REC·신선도·EXT·worker `RESTARTED` | `can.channel_state`(우선), `can.stats` 1 Hz, `record.health`, `source.gap` | 채널 상세, bus-off 복구(R2, 반복 금지), 닫기 | M1 최소 |
| Trace | Chrono/Fixed, 열(시간·Δt·ch·dir·출처·ID·이름·FD 플래그·len·data·상태·주석), ISO-TP 묶음 + UDS 주석, EXT 배지, 필터식(관측 필터일 뿐) | `can.frame`(열 블록), `can.tx`, `can.isotp_*`, `can.job_state`, gap, `test.step`·`core.mark` / `can.trace.query`, `can.fixed.snapshot` | **하단 송신 입력줄**(M1), 행 메뉴(다시 보내기·IG로·필터·마커·Graphics에 추가) | M1 Chrono+송신, M2 Fixed·신호·ISO-TP, M3 EXT·UDS 주석 |
| Graphics | 다중 신호, 단위별 y축, 커서 2개(원본 샘플로 확정), gap에서 선 끊기, 다운샘플 표시, 에이전트 대기 임계선 | `can.signal_sample` / `can.signal.query{px, minmax}`, `can.signal.at` | 신호 추가, 마커, 구간 → Trace `around`, CSV 내보내기 | M2 RPC, M3 완성 |
| IG | DB 메시지·신호 위젯, 인코드 바이트 미리보기, 위험 등급, **job 소유자·rev·lease·승인 범위** | `can.db.catalog`, `can.encode.preview`(R0), `can.job_state` / `can.periodic.*`, `can.job.takeover` | 1회 송신, 주기 시작, 에이전트 job **patch(rev CAS)**·**takeover**·stop | M2 최소, M3 takeover |
| Data/State | 값·단위·raw·enum·age·출처·quality(`ok/stale/lagging/polled`) | `can.signal_sample` | Graphics/IG로 열기 | M2 |
| Statistics | 부하·fps·에러 타임라인, ID별 실제 주기 vs DBC | `can.stats`, `can.stats.query` | 보기 전용 | M4 |

### 5.3 M1 와이어프레임 (UI 셸 안, sim)
```text
┌ Loom ─ [■ STOP]  approvals 0  ───────────────────────────────────────────────────────────────┐
│ ch0 sim 500k  ● LIVE lag 38ms  REC ok  env SIM   clients: agent:claude-1 · user:web          │
├ CAN Trace ─ ch0 ─ [Chrono] ─ filter: ___________________________ ─ [Pause] ─ shown 1,204 ────┤
│ 812.004   tx AGT  0x300   1  01                        sent         agent:claude-1            │
│ 812.009   rx DEV  0x301   1  01                                     sim:BCM                    │
│ ⋯ 3,812 frames not shown (seq 9001..12812) — recording ok                                       │
│ 818.731   tx USR  0x300   1  00                        sent         user:web                   │
│ 818.736   rx DEV  0x301   1  00                                     sim:BCM                    │
├────────────────────────────────────────────────────────────────────────────────────────────────┤
│ send ▸ 0x300#00                                                         [Send ⏎]  (as user)   │
├ Activity ──────────────────────────────────────────────────────────────────────────────────────┤
│ 818.90 agent:claude-1 read ch0 trace (2 rows) · 818.73 user:web sent 0x300                     │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
```
M3 작업 공간(Trace·Graphics·IG·Data, 테스트 실행 중) 와이어프레임은 `_xteam/can-ui.md` §5.8.

### 5.4 데이터 계약 — CAN 제공 / UI 요구
- **CAN 제공(gateway CAN 모듈):** 이벤트 `can.frame`(UI용 열 블록), `can.tx`, `can.channel_state`(우선), `can.stats`, `can.activity`, `can.job_state`(우선), `can.signal_sample`, `can.isotp_*` / RPC `can.trace.query`, `can.fixed.snapshot`, `can.tx.send`, `can.db.catalog`, `can.encode.preview`, `can.signal.query/at`, `can.periodic.*`, `can.job.takeover`, `can.channel.recover`, `can.stats.query`, `can.evidence.verify`. 이름은 CORE 계약 v1 표기를 따른다.
- **UI 요구 U-CAN-1~10:** Trace 유입 2,000 행/s·클라이언트 보존 ≥ 100k 행, `can.frame`·`can.signal_sample` 바이너리 열 블록(M2 전), Graphics ≥ 16 신호·30 fps 줌, 패널 간 시간 커서·마커 동기, 셸 STOP·승인·env 연결, 입력 위젯 검증(11/29bit, FD DLC 단계), 지연 계측 회신, **딥링크**(`/#/can/trace?ch=ch0&since=mark:s2`), 테스트 마커 렌더, 브라우저 메모리 상한. 상세 `_xteam/can-ui.md` §5.9.
- 열린 합의: (a) 사용자 본인이 시작하는 R3(IG 주기)를 셸 확인창 1회로 처리할지(CORE·UI), (b) p95 < 150 ms 게이트, (c) 열 블록 형식(`core-ui.md`).

---

## 6. 데이터/기록 포맷

### 6.1 공통 이벤트 레코드 (CAN 프레임)
마스터 D3 레코드 필드를 따른다.
```json
{"schema_version":1,"session_id":"s12","producer_id":"can-sim","producer_epoch":"1","source_seq":"501",
 "stream_epoch":"3","stream_seq":"88213","seq":"88213","kind":"can.frame",
 "ts":{"mono_ns":"812004113000","wall_ns":"1794220351402000000","clock_id":"host:mono","source":"host","quality":"host_ingest"},
 "domain":"can","channel":"ch0","dir":"tx","origin":"agent","client_id":"agent:claude-1","actor_id":"agent:claude-1",
 "request_id":"r42","job_id":null,"epoch":null,"generation":"1","purpose":null,"risk":"R1",
 "request_origin":null,"sim_node":null,
 "payload":{"id":768,"extended":false,"fd":false,"brs":false,"esi":false,"remote":false,"error":false,
  "dlc_code":1,"data_len":1,"data":{"encoding":"hex","data":"01"},
  "device_ts":null,"tx_state":"sent"},
 "metrics":null}
```
- `origin` ∈ **agent / user / device / script** (공통 enum을 늘리지 않음). 구분은 별도 필드: `purpose`(periodic, replay, uds, tester_present, sim), `actor_id`, `request_origin`(ECU 응답이 누구 요청에 대한 것인지), `sim_node`, `original_origin`(replay 시 원본 출처), `external_actor`(`canoe`|`host`|`unknown`, §3.4).
- origin은 **서버가 연결 role로 정한다.** CLI는 agent, TUI/웹은 user, 드라이버 수신·가짜 ECU는 device, 시나리오/replay/keepalive는 script. 클라이언트가 자칭할 수 없다.
- 시각: `ts.mono_ns` = CAN worker host ingest 단조 시각(정의 고정), `ts.source/quality/clock_id`는 이 ingest 시각의 출처. 드라이버 시각은 `payload.device_ts{value_s, clock_id, source: hardware|kernel_sw|host, quality}` — python-can이 준 값(Vector는 이미 XL tick을 초로 바꾸고 host offset을 더한 값, 원 tick 아님, `canlib.py:761-765`).
- 필드 매핑: `generation` = 채널 open 세대(재오픈마다 증가), `epoch` = ISO-TP 채널 epoch(해당 시), `job_id` = 주기·IG·replay·ISO-TP 잡, `risk` = 송신 판정 등급. `tx_state`는 D4 단계(requested/accepted/sent/confirmed/failed/outcome_unknown).
- 신호 디코드 값은 선택 필드 `metrics[{name, value, unit}]`로도 낸다(디코드가 켜진 구독·기록에서만, D3) → viz가 도메인 무관하게 자동 플롯. 다른 어댑터의 HW 시계를 동기 근거 없이 빼서 응답 시간을 만들지 않는다. 자동 드리프트 보정은 하지 않고, 정렬 품질을 표시한다.
- 64-bit 정수(seq, ns)는 JSON 문자열. DLC code와 바이트 길이를 분리 저장. 저널에는 `stream_*`가 없다(gateway가 부여) — 저널 행은 원본 ID까지만.
- ISO-TP가 보낸/받은 원시 프레임(FF/CF/FC)에는 `request_id`(UDS `can.isotp.send`의 req_id)와 `isotp_id`를 주석으로 단다(R-TP-07) → CAN Trace와 UDS 트랜잭션을 같은 타임라인에서 연결.
- `dir`은 프레임에서 rx/tx, 상태 이벤트에서 null (D3). 바이트는 `{encoding: hex|base64|raw_ref, data}`, RPC 인자는 hex 문자열.

### 6.2 정본 저널 (자동 기록, 무손실 경로)
**결정: 정본(canonical) = Loom 저널, 표준 포맷(BLF/ASC/MF4)은 export.** (Sol 안 채택)
근거(실측, `r2-results.json`): BLF 단독 쓰기 처리량은 충분(100,000프레임 0.99 s ≈ 101k fps). stop/명시 flush 없는 child 프로세스 abrupt exit **1회** 시험에서 reader가 100,000 중 81,920개만 복구(오류 없이 18,080 누락) — 원인별 분해·전원 장애 내구성은 미측정, 일반 유실률로 읽지 않는다. 또 BLF에는 Loom seq·origin·요청 ID가 없어 사이드카 조인이 정확하지 않다(같은 ID/payload 반복). 저널이라고 내구성이 저절로 해결되지는 않으므로 아래 내구성 창을 시험으로 고정한다.
```text
~/.loom/records/can/2026-11-09T14-02-11_s12/
  manifest.json          세션·schema·OS/빌드/backend/driver 버전·채널 설정·DB 경로+sha256·chunk 목록(seq/시간 범위·checksum)
  can_ch0/000001.jsonl   열린 chunk (append, 길이 제한 레코드)
  can_ch0/000000.jsonl.zst  seal된 chunk (close → fsync → 원자적 rename → 압축은 CAN worker 밖에서)
  events.jsonl           승인·정책·채널 설정·버스 상태·마크·gap·DB 로드 (프레임 외 공통 이벤트)
  (감사는 core 감사 로그 — 해시 체인, 끌 수 없음)
```
- M1 형식 = 길이 제한 JSONL chunk(64 MiB 또는 60 s 회전). 산술 추정: §6.1 형태 compact 행 ≈ 540 B(1바이트 payload) → 5k fps에서 **≈ 2.7 MB/s, ≈ 9.7 GB/h 비압축**(최대값 아님, FD 64 B면 더 큼) → seal 후 zstd. 이전 "0.98 MB/s·직렬화 9 ms/s"는 축약 schema microbench라 현 schema 예산이 아니다 → M0(b)에서 현 schema로 재측정, 필드 축약/binary chunk(Arrow IPC/고정 레코드)와 비교해 M2에서 형식 확정.
- 내구성 창(두 종류로 구분): **프로세스 크래시 RPO** = 열린 chunk를 1 s마다 OS로 flush → 최대 약 1 s 손실 목표. **호스트/전원 장애 RPO** = seal 시 fsync(최대 60 s 또는 64 MiB) — 그 사이는 OS 캐시에 의존. seal 순서: chunk close → fsync → 원자적 rename → manifest에 commit(append+fsync) → 압축본 생성·검증 후에야 원본 삭제. 크래시 복구: 열린 chunk의 마지막 불완전 줄만 잘라내고 나머지는 유효, manifest에 없는 sealed 파일은 재등록. 모두 장애 주입 시험(§9.2)으로 확인.
- `record stop`/`clip stop` 은 추가 export만 끈다. 자동 기록 중단은 채널 close와 연결.
- 기록·감사 불능(디스크 full, 권한)이면 `record.health=failed` + **agent·script의 새 R1+ 쓰기 중단(sim 포함, `E_RECORDING_FAILED` exit 4)**. 사용자 뷰어 송신은 허용하고 `record.health=failed` 배너, 그 구간 감사는 데몬 로그(stderr)에 남긴다(A12).
- 보존: 기본 30일 / 전체 20 GiB quota(D7, `loom.toml`), 감사 로그 무기한. 위치: `~/.loom/records/`(D7). `--no-record`는 CAN에서 제공하지 않는다(버스 기록은 루프의 증거).
- 손실 카운터는 넷으로 분리: source(드라이버·USB 오버런, `source.gap`), record(저널, `record.gap`), decode 실패, display 생략(`display.gap`). 정확한 수를 모르면 `unknown`.

### 6.3 표준 포맷 import/export (v2.0)
정본은 CORE 저널(D7). 표준 포맷은 seal된 저널에서 **별도 변환 worker**가 만든다(CAN DLL worker 안에서 하지 않음, 캡처를 막지 않음).
| 포맷 | 방향 | Rust 경로 | 상한·검증 | M |
|---|---|---|---|---|
| BLF | export (import는 M3) | `autors-blf` 0.1.1(Apache-2.0) **후보** — streaming `BlfWriter`만 사용(`BlfFile::parse_reader`·`write()`는 전체 메모리 수집이라 금지), reader/writer 상한 명시(header 64 KiB·container 8 MiB·object 1 MiB·writer 256 KiB) | CANoe/PCAN-View와 독립 reader로 비교(CC 11/29·RTR·error·FD64/BRS/ESI·방향·복수 채널·시간 단위·container 경계·절단). 자기 writer-reader 왕복만으로 통과 금지 | M2 |
| ASC | export / import | `autors-asc` 0.1.1 후보 | CANoe 변형·FD 열·날짜/시간대 fixture | M2 |
| TRC | export / import | 자체 작은 line reader/writer(PEAK 사양 CC 1.3·FD 2.1) | PCAN-View 비교 | M3 |
| MF4 | — | **미지원으로 명시**(`asammdf` crate는 production 미준비 명시, `mdfr`은 GPL-3.0·미배포) | 후보 심사 후 재결정 | M4 검토 |
| CSV/JSONL | export | 저널 → 행 변환 | 출처·gap 열 포함 | M2 |
- origin·승인·원본 seq·불확실 시각은 sidecar로 보존하고, reader가 건너뛴 unknown object 수를 보고한다. 비정상 종료는 `.partial` + source 범위로 재생성(원자 rename).

### 6.4 `loom.viz` (v2.0: UI 팀 소유)
viz 데이터셋 모델·렌더러·`loom viz` CLI는 UI 팀으로 이관됐다(C6). CAN이 제공하는 것: CAN 저널·BLF/ASC를 viz 데이터셋으로 읽는 어댑터 요구(신호 = 시계열 열, 프레임 = 이벤트 열)와 라이브 소스 `live:can`(= `can.signal_sample` 구독). 형식은 `ui/plan.md`를 따른다.

### 6.5 설정 파일 `loom.toml` (프로젝트 루트, 사람만 편집)
```toml
[daemon]
port = 0                      # 0 = 자동 할당, discovery 파일에 기록

[can]
default_channel = "ch0"

[can.channels.ch0]
interface = "pcan"            # virtual | pcan | vector | socketcan | kvaser
channel = "PCAN_USBBUS1"
mode = "configure"            # shared(설정 안 바꿈, CANoe 공존) | configure
bitrate = 500000
fd = true
data_bitrate = 2000000
env = "bench"                 # sim(virtual/vcan만 허용) | bench | vehicle (B4: vehicle = bench 규칙, 배지만 다름)
tx = "confirm"                # deny | confirm | allow   (실HW 미설정 채널 기본 = confirm, R2 viewer — B4)
db = ["db/powertrain.dbc"]
allow_ids = ["0x300-0x3FF"]   # (선택) 사용자가 송신 범위를 좁힐 때만. 없으면 제한 없음(B4) — deny_ids·위험 등급은 그대로
deny_ids  = ["0x000-0x0FF"]   # 하드 차단 (승인으로 우회 불가)
e2e_protected = ["Brake_*"]    # 사용자가 지정한 E2E 보호 메시지 → 검증된 encoder 없으면 하드 차단. 이름 패턴·DBC 속성 자동 판별은 차단이 아니라 R4(§10.2)
max_agent_fps = 200
max_agent_load_pct = 10
lease = "10m"
max_job_duration = "1h"      # A9: renew 누적 상한, 넘으면 새 승인
r2_path = "viewer"            # A8: 실HW 기본 viewer, tx="allow" 채널만 token_or_viewer
autostart = true

[can.channels.ch0.isotp_profiles.ecu_engine]   # UDS가 참조하는 ECU 프로파일
tx_id = 0x7E0
rx_id = 0x7E8
padding = 0xCC
blocksize = 8
stmin = 0

[can.channels.ch1]
interface = "virtual"
channel = "sim1"
env = "sim"
tx = "allow"

[can.channels.ch2]               # Linux 실HW (B1). netdev는 사람이 미리 설정(§8.4) — Loom은 값 검증만
interface = "socketcan"
channel = "pcan_bench0"         # udev 규칙으로 고정한 이름 권장
mode = "configure"              # socketcan에서는 "검증": netdev 실제 값 ≠ 아래 값이면 open 거부(exit 2, hint에 ip 명령)
bitrate = 500000
env = "vehicle"
tx = "confirm"

[canoe]                          # Windows, B3 (§8.2)
enabled = true
attach = "running"               # 실행 중인 CANoe에만 붙음(자동 실행 안 함)
poll_hz = 10                     # 구독 신호 폴링 주기(상한 50)
```
정책 파일 2층(D6): 프로젝트 `loom.toml`(도메인 섹션 + `[bench]` 한계·별칭) + 사용자 `~/.loom/config/policy.toml`. 정책 **완화** 변경은 viewer 승인 후에만 적용된다(§10.1-7), 해시 기록.

---

## 7. 라이브러리 선택과 근거 (v2.0 — Rust, Python 의존 0)
확인일 2026-11-10, 공식 sparse registry·배포 archive `Cargo.toml` 기준(원자료 `spikes/can/rust/crates-evidence.json`, `source-checks.json`). "채택"은 계획상 선택이며 probe에서 실제 빌드한 외부 crate는 `crossbeam-channel`뿐이다.

| 용도 | 선택 | 버전 / 라이선스 | 근거·주의 |
|---|---|---|---|
| Linux SocketCAN(FD) | **`socketcan`** | 4.0.0 / MIT | raw·FD socket·timestamp(SW/RAW_HARDWARE API)·`nl` 읽기 getter(§8.4). async wrapper 대신 RX 전용 스레드. `rtnetlink`은 부족할 때만 |
| PCAN (Windows) | **PEAK 공식 Rust FFI 모듈** | PCAN-Basic SDK 5.1.0.1194 / PEAK EULA | SDK `Samples/…/Console/Rust/*/src/PCANBasic.rs`에 `InitializeFD`·`ReadFD`·`WriteFD` 확인. crates.io crate 아님. `PcanDevice` RAII 래퍼로 감쌈. 재배포 권한은 확인 전 — SDK를 저장소에 넣지 않음. `peak-can-sys`는 보류 |
| Vector XL | **CAN 소유 `loom-vector-xl` crate**(CAN/CAN FD API만, C4.1) | 자체 / `libloading` 0.9.0(ISC) | 공개 `vxlapi-sys` 2.1.0(MIT/Apache, FD 구현 있음)은 참고·비교. 자체가 필요한 이유: shared 모드에서 FD 포트도 `permissionMask=0`(init 요청 0) 유지, chip state·error·timer 이벤트 보존, `XL_ACTIVATE_RESET_CLOCK` 쓰지 않음(공유 시 시계 리셋 금지). `vxlapi.h`·DLL은 커밋하지 않고 설치 SDK 경로·버전·해시만 기록. `bindgen` 0.73.2는 SDK 있는 CI에서 ABI 비교용 |
| ISO-TP | **직접 구현**(§3.6) | — | `automotive` 0.6.0·`can-iso-tp` 0.2.0 = differential oracle. `ecu_diagnostics`(GPL-3.0)·`isotp-rs`(전 버전 yanked) 제외 |
| DBC | **`can-dbc` 파서 + 자체 codec** | 10.0.0 / MIT OR Apache-2.0 | 확장 mux(`SG_MUL_VAL_`)·value table·attrs 확인, `sigtype_attr_list` 미구현. codec은 Intel/Motorola·signed 1..64·IEEE32/64·scale/offset·DLC 0..64·mux 중첩을 한 IR로. 미지원 문법은 위치와 함께 경고, 쓰기에 영향을 주면 fail closed. 상한: 파일 16 MiB·메시지 50k·신호 500k·mux 깊이 16, compiled IR 32 MiB·동시 DB 4 [목표]. `dbc-codegen`·`can_decode`는 비교용 |
| 스레드·큐 | `crossbeam-channel`(bounded만) | 0.5.17 | 실측 11.6–11.9 M frame/s(128 B, 같은 프로세스) — 직렬화·IPC 미포함 |
| 우선순위·타이머 | `thread-priority` 3.1.1, `windows` 0.62.2 | MIT / MIT-Apache | best effort, 실패를 capability에 표시. Windows 고해상도 waitable timer(Win10 1803+) |
| 런타임 | `tokio`(CORE) | 1.54.1 | gateway·IPC 대기에만. FC·주기 deadline을 async task와 공유하지 않음 |
| 로그 포맷 | `autors-blf`·`autors-asc` 후보 | 0.1.1 / Apache-2.0 | §6.3 |
| 변환·압축 | `parquet`/`arrow`, `zstd` | 60.1.0 / 0.14.3 | seal 후 변환 worker에서만 |

toolchain: workspace Rust **1.93.1** 고정 제안(로컬 실측; `smoltcp` 제외로 MSRV 하한은 `socketcan` 1.89). 전이 라이선스·lock·checksum 검사는 CORE 배포 관문. cargo feature: `can-pcan`(Windows), `can-vxl`(Windows), `can-socketcan`(Linux), `can-sim`(전부) — macOS 빌드는 SDK·DLL을 요구하지 않는다.

**Python 제거 (C1):** python-can·can-isotp·cantools·asammdf는 제품에서 모두 빠진다. Python reference 도구(cantools·공개 pyvxl)는 **개발용 차등 비교**에만 별도 환경에서 쓴다. 공개 pyvxl(cmcerove, commit d45cca4)은 32비트·CAN 전용으로 확인 — FD64·TX ACK의 기준은 공식 C SDK 샘플 + CANoe 관측.

---

## 8. HW/OS 지원 매트릭스 (v2.0)

"정식/조건부"는 목표 등급이며 실HW 검증 완료가 아니다.

| 경로 | worker | CC | FD | timestamp | 목표 등급 |
|---|---|---|---|---|---|
| sim (mac/Win/Linux) | `can-sim` | ✓ | 플래그 모델 | host | **M1 정식** (CI 3 OS) |
| **Windows x64 PCAN-USB FD** | `can-pcan` | ✓ | ✓ | PCAN µs → host 매핑 | **M2a 첫 실HW (C5)** |
| **Linux x64 PCAN-USB FD (SocketCAN `peak_usb`)** | `can-socketcan` | ✓ | ✓ (사람 사전 설정) | kernel SW, HW는 장치·커널이 줄 때만 | **M2a 첫 실HW (C5, B1)** |
| **Windows x64 VN5640 (XL, CAN FD 채널)** | `can-vxl` | ✓ | ✓ | XL HW 시계 → host 매핑 | **M2b (C4)** — configure + CANoe 공존 shared |
| Linux vcan (CI) | `can-socketcan` | ✓ | 커널 설정 | kernel SW | M2 — vcan 권한 러너 확보가 전제 |
| VN5640 이더넷 포트 | — | | | | **비범위(C4.1)** — DoIP는 UDS가 PC NIC + 미디어 컨버터로 |
| Kvaser 등 | — | | | | M5 |
| macOS 실HW, Linux Vector/CANoe | — | | | | 비범위 (B1) |
- 장치별 capability(fd, hw_timestamp, tx_confirmation, periodic_engine, shared_open, listen_only, bus_load, error_frames, hardware_filters, thread_priority, timer)를 `hw list`·`status`로 보고. `unknown`과 `false`를 구분하고 미지원 기능을 조용히 SW로 흉내 내지 않는다.
- 데몬과 에이전트는 HW가 꽂힌 **Windows x64 또는 Linux x64** PC에서 실행한다(B1). macOS는 개발·sim·로그 분석.
- 벤더 DLL이 없거나 버전·심볼이 안 맞으면 그 백엔드만 `unavailable`(누락 함수·bitness 표시). gateway 기동을 실패시키지 않는다.

### 8.1 CANoe와 Vector 채널 공존 (v2.0: **M2b 실측 gate**, 자체 XL crate)
- XL 앱 채널 할당은 logical→physical 매핑이지 배타 잠금이 아니다. 여러 앱이 같은 물리 채널을 열 수 있고, 채널 파라미터를 바꾸는 **init access는 한 앱만** 가진다.
- **shared 모드 = `permissionMask` 입력 0**: CC든 FD든 init 요청을 하지 않고 열어 실제 timing을 읽어 표시만 한다. python-can은 FD면 init을 요청했지만(v1.2 문제), 자체 crate는 이 제약이 없다 — **FD 공존도 M2b에서 바로 실측**한다. 공유 중 설정 변경·`XL_ACTIVATE_RESET_CLOCK`·다른 앱 큐 flush를 하지 않는다. shared open 실패를 configure로 바꾸지 않는다. configure는 init grant를 확인한 뒤에만 timing을 쓴다.
- **실측 항목(K13, M2b):** ① CANoe 측정 중 shared open 성공·permission mask(CC·FD), ② 수신(ECU·CANoe 시뮬 노드), ③ Loom 송신이 버스·CANoe 트레이스에 보이는지, ④ **CANoe가 보낸 프레임이 Loom 포트에 어떤 표시(TX_OK 등)로 오는지** → `external_actor=canoe` 판정 근거, ⑤ 순서별(CANoe 먼저/Loom 먼저/측정 재시작/CANoe 종료), ⑥ VN5640을 미디어 컨버터로 DoIP에 쓰는 동안 CAN FD 채널 동시 사용. 결과는 `spikes/can/coexist-REPORT.md` + 버전 표.
- 실패 시 대체: 그 채널은 Loom이 열지 않고, raw Trace가 필요하면 **같은 버스에 PCAN-USB FD를 관찰용(tx=deny)으로** 붙인다. 둘 다 없으면 `bus not observed by Loom` 배지. (v1.2의 "브리지로 대체"는 브리지가 P4로 내려가 기본 대체가 아니다.)
- **외부 트래픽 표시 규칙(`external:canoe`):** Loom이 버스를 관찰할 때만 보인다. ④로 "다른 앱 송신"이 확인된 프레임 = `EXT canoe`. 근거가 없으면 Loom이 보내지 않은 진단 요청 ID·Loom 잡이 아닌 ID는 `EXT ?`(`external_actor=unknown`), 나머지는 `DEV`. Trace·Activity·`trace --external` 필터·`others_since_last.origins`에 같은 규칙.
- **Vector TX 큐 flush를 호출하지 않는다**(python-can 4.6.1 `flush_tx_buffer()` docstring: "send a high voltage message (ID = 0, DLC = 0)" — 대응 XL 함수는 SDK 대조 전 금지). CANoe가 먼저 종료돼도 비트레이트 재초기화·송신 자동 재개 없음. 공존 채널 기본 `tx=confirm`, CANoe가 송신 중인 ID 송신은 R4. `app_name="Loom"`과 Vector Hardware Config 등록 절차는 스킬 references.

### 8.2 CANoe 브리지 (v2.0: **P4 선택 기능**, C4 — v1.2 v0 설계는 이력)
- 진단 기본 경로는 Loom 자체 스택(DoIP over PC NIC, CAN-TP over VN5640·PCAN FD). 브리지가 없어도 루프가 닫힌다.
- **언어(C1, D71):** Rust로 COM이 불가능하다는 근거는 없다 — `windows` 0.62.2가 `IDispatch::GetIDsOfNames/Invoke`를 제공하고(소스 확인), STA·VARIANT·이벤트 싱크(`IConnectionPoint`)도 가능하다. 그래서 C1의 "대안이 없을 때만 Python" 조건은 **아직 충족되지 않았다**. P4 첫 작업 = 설치된 CANoe에서 Rust late-binding으로 신호 읽기 + 진단 이벤트 싱크 + CANoe 재시작 재부착을 **3일 timebox(추정)** 스파이크. 재현 가능한 blocker가 나오면 그때 Python 어댑터 worker 예외를 결정 기록에 남긴다. 비용만이 이유라면 C1 완화는 메인 결정.
- 기능 범위·STOP·쓰기 상태·사용자 화면 규칙은 v1.2 설계와 `_xteam/canoe-bridge.md`(메인 확정: `tx_state`와 UDS `write_state` 분리, UDS 추가 요청 1–6 채택)를 그대로 쓴다. 어느 언어든 같은 worker 프로토콜·메모리 예산·감독·`decision_id`·`generation`·늦은 응답 규칙을 지킨다. 자동 CANoe 선택·측정 시작·세션 중 백엔드 전환은 없다.

### 8.3 실HW 체크리스트 (B1 — OS별, M2 Q2 작업)
| Windows x64 | Linux x64 |
|---|---|
| PEAK 드라이버 + PCAN-Basic(Win 5.1.0), `PCANBasic.dll` x64. 장치 여러 개면 Device ID로 고정 | 커널 `peak_usb` 로드(`lsmod`). PEAK chardev 드라이버(PCAN-Basic Linux)와 함께 설치하지 않는다 |
| 채널 `PCAN_USBBUS1`, Loom이 비트레이트 설정(`mode=configure`) | netdev 설정은 사람이(§8.4): `ip link set <if> type can bitrate 500000 [dbitrate 2000000 fd on] restart-ms 0` → `up`, 또는 systemd-networkd. `loom can hw setup-hint ch2`가 loom.toml 기준 명령 출력 |
| — | 이름 고정: udev 규칙(장치 시리얼 → `pcan_bench0`). USB 재연결 후에도 같은 이름 확인 |
| — | `txqueuelen` 10 → 1000 권장(버스트 시 `ENOBUFS`). Loom은 ENOBUFS를 `failed`로 기록, 조용히 재시도 안 함 |
| — | 권한: raw CAN 소켓 열기는 일반 사용자 가능 여부 실기 확인(**미확인**), 설정 변경만 `CAP_NET_ADMIN` |
| open 후 `hw list` capability(fd, hw_timestamp, tx_confirmation) 확인 | open 시 `ip -details -json link show` 값(bitrate·fd·state·restart-ms)과 loom.toml 비교, 불일치 시 거부 확인 |
| 2채널 교차 루프백(종단 저항), TX receipt로 `confirmed` 확인 | 루프백. TX confirmation: v2는 `socketcan` crate로 `recvmsg` 플래그(`MSG_CONFIRM`·`MSG_DONTROUTE`)를 직접 읽을 수 있는지 M2a에서 확인 — 되면 own-echo로 `confirmed`, 안 되면 `tx_confirmation=false`(`sent`까지) (D66) |
| timestamp: PCAN µs → host 변환, `device_ts.quality` | timestamp: `SO_TIMESTAMPNS` kernel SW. 다른 프로세스 `cansend` → `EXT host` 행 확인 |
| 10 ms 주기 지터 실측(idle·부하), 타이머 해상도 기록 | BCM 주기 지터 실측, Windows 값과 나란히 게시 |
| USB 절전(selective suspend) 끄기 권장. 뽑기 → `E_DISCONNECTED`, 송신 잡 자동 재개 없음 | 뽑기 → netdev 사라짐 → `E_DISCONNECTED`, 같은 이름 재등장 시 수신만 재개, 송신 잡 재개 없음 |
| bus-off: 자동 복구 반복 없음 확인 | `restart-ms 0`(자동 복구 끔) 권장 — 켜져 있으면 `status` 경고 |
| Vector: XL Driver Library, Hardware Config에 앱 `Loom` 등록, 공존 실측(§8.1) | (Vector·CANoe 없음 — B1) |
| CANoe: 브리지 부착·측정 시작/정지·재시작 후 재부착·브리지와 공존 동시 사용, 버전 기록 | 해당 없음. 진단은 UDS 자체 스택(CAN-TP over SocketCAN, DoIP) |
| 경로 `%USERPROFILE%\.loom`, loopback 방화벽 프롬프트 없음 | 경로 `~/.loom`. systemd user service는 M5 |
| wire timing 3시각(send hook·드라이버 RX ts·외부 모니터) | 동일 |

### 8.4 Linux 인터페이스 설정 주체 (B1 결정 → D47)
- **결정: 사람이 사전 설정한다.** Loom은 netdev를 바꾸지 않는다(`mode=configure`는 socketcan에서 "검증"). open 시 실제 값(bitrate·dbitrate·fd·state·restart-ms)을 읽어 loom.toml과 다르면 거부(exit 2 `E_USAGE`, `detail.reason=netdev_mismatch`, `hint`에 정확한 `ip` 명령), 링크가 down이면 exit 7(`E_NOT_OPEN`, `link_down`).
- **권한 헬퍼는 v1.2 범위 밖.** 이유: ① root 권한 헬퍼는 공격면이고, 에이전트가 sudo 프롬프트를 만나면 흐름이 끊긴다. ② CAN 정책상 비트레이트 변경을 에이전트가 요청하는 것은 이미 R4다 — 상시 경로가 필요 없다. ③ 벤치 PC는 systemd-networkd로 부팅 시 고정하면 충분. M5에서 polkit으로 제한한 systemd 템플릿 유닛(`loom-can-up@<if>`)을 검토.
- v2: `socketcan` 4.0.0 `nl` 모듈의 getter(`CanInterface::details`·`can_params`·`bit_timing`)만 감싸 노출한다(설정 setter는 노출하지 않음). 필수 비교값을 읽지 못하면 `unknown`이며 일치로 간주하지 않는다. open 직전과 재연결 때 다시 검증.

---

## 9. 시뮬레이션 & 테스트 전략

### 9.1 시뮬레이션 구성 (v2.0, HW 없이 전체 데모)
```text
can-sim worker (Rust)
 ├ sim 버스 owner ◄─┬─ SimNode BCM   (M1: raw 규칙 — on 0x300 → reply 0x301, 주기 raw)
 │  ch1 "sim1"      ├─ SimNode EMS   (M2: DBC 노드 TX 메시지를 GenMsgCycleTime 주기로, 신호 생성기)
 │                  ├─ SimNode UDS-ECU (M2: 서버 측 ISO-TP + UDS VirtualEcuModel + 결함 훅)
 │                  └─ (M5) Replay 노드
 ├ ch2 "sim1"  ← 같은 이름 = 같은 sim 버스 → 두 채널 루프백 시험
 └ 로컬 소켓 endpoint(UDS·외부 sim 프로세스용, 최대 16)
```
- **sim 버스는 프로세스 간**(D72): `can-sim` 안에 bus owner를 두고, 다른 Rust 시뮬레이터·클라이언트는 로컬 stream socket(Unix socket / Windows named pipe, CORE IPC 재사용 우선)으로 붙는다. 메시지 = length-prefix + version + 상한 payload(record ≤ 8 KiB, endpoint당 8,192 frame / 2 MiB). owner가 필드를 정규화하고 origin은 서버가 부여(가짜 ECU = device). 공유 메모리는 M0 기본에서 제외(근거 부족). 초기 전달은 즉시 — wire timing·중재·ACK 시뮬레이터가 아니므로 물리 버스 부하 검증에 쓰지 않는다.
- UDS 가상 ECU: UDS 팀이 `VirtualEcuModel`(Rust trait: `handle(pdu, now_ns) -> Vec<(delay_ms, Vec<u8>)>`, 상태 내부·I/O 없음·시드 고정)을 제공 → `can-sim`의 SimNode가 서버 측 ISO-TP 채널(R-TP-12)에 붙여 실행. 결함 주입(CF 누락·FC 지연/WAIT·잘못된 SN·bus-off·늦은 PDU — R-TP-13)은 SimNode 설정·RPC로, sim에서만.
- SimNode YAML 형식은 v1.2와 같다(노드·DB·주기 메시지·신호 생성기 sine/ramp/counter·responses·faults). 고정 seed, 명시 time source, 종료 deadline. `loom can demo` 한 줄로 sim 채널 + 노드 + UI 링크.
- macOS에서 되는 것: gateway·CLI·WS·웹·저널·codec·ISO-TP 상태기계·별도 프로세스 sim·가짜 CANoe·계약 시험. Windows DLL·Linux AF_CAN 경로는 target cfg + 가짜 백엔드로 분리.

### 9.2 테스트 계층
| 계층 | 대상 | 환경 | CI |
|---|---|---|---|
| 단위 | 필터식/`--since` 파서, 커서, 링버퍼, 정책 엔진, 레이트 리미터, wait-for AST, 자동 렌더 규칙, 다운샘플, 저널 chunk/복구 | 순수 Rust(`cargo test`), proptest | ✓ mac/Linux/Win × Py3.11–3.13 (+3.14 job) |
| 서비스 | CAN worker + virtual + SimNode, ISO-TP 왕복(SF/FF/CF/FC, BS 0/8, STmin 0/1/10, F1–F9 best-effort, FC 누락/Overflow/Wait 한도, CF 순번 오류, 초과 PDU, 중복 주소), 주기 송신 통계 | 프로세스 내 | ✓ |
| RPC/CLI 계약 | JSON-RPC 스키마 스냅샷, CLI 골든 출력(사람용/`--json`), 출력 상한, 종료 코드 | gateway+worker 서브프로세스 | ✓ |
| DB fixture | 주기 신호, mux 분기, Intel/Motorola, signed/float/enum, 신호 patch(초기값·비활성 mux·범위·revision race) | | ✓ (M2) |
| 포맷 round-trip | ASC/BLF(/MF4 extra): CC/FD, 11/29bit, RTR/error, 시각, 손실 보고 | | ✓ (M2) |
| 웹 (UI 팀 하네스 + CAN 시나리오) | Playwright: §2.0 E2E·같은 세션 개입·gap/lag 표시·STOP·지연 계측(`ui.latency_sample`), 이후 승인·XSS·줌 | headless Linux | ✓ (**M1**) |
| ABI | XL·PCAN 구조체 size/align/offset·event tag를 SDK C harness와 비교 | SDK 있는 Windows x64 CI | ✓ (M2) |
| 성능 (nightly) | 5k fps 1시간·20k fps burst 10분, 뷰어 2개(하나는 느리게), journal seq 대조, 1M/10M 포인트 viz 질의 | 기준 머신 | nightly — 기준 머신/OS/부하/표본 수/percentile을 artifact에 기록, PR 단위 assertion 금지 |
| 장애 주입 | 디스크 full/권한/I/O 지연, worker 크래시, gateway 크래시, client flood, abrupt close | | ✓ / nightly |
| Linux vcan | SocketCAN, BCM, (선택) kernel ISO-TP, 다른 프로세스 ECU | vcan 가능한 러너(`modprobe vcan` 권한) | 러너 확보 시 M2 정식 gate, 미확보 시 SocketCAN 등급은 조건부. 권한 없으면 **명시 skip**, virtual 결과로 대체 표기 금지 |
| HW gated | **Windows x64**: PCAN/Vector, CANoe 브리지·공존 / **Linux x64**: PCAN SocketCAN — 체크리스트 §8.3, wire timing 3시각 분리 | 수동/관리 벤치 | ✗ (HW 없는 PR에서 실HW 지원을 통과로 표시 금지) |
| 에이전트 E2E | 스킬 시나리오 A1~A6을 실제 에이전트에 시켜 결과 판정 | 반자동 | 릴리스 전 |

### 9.3 핵심 루프 불변식 시험 (M1부터, 실패 시 머지 금지)
1. **§2.0 E2E 자동화**: CLI(agent role)로 송신 → SimNode 응답 → viewer 테스트 클라이언트가 같은 seq의 AGT/DEV를 목표 지연 안에 수신 → viewer가 user 송신 → CLI `trace --since @last` 가 USR 프레임 + `NOTE:` 반환.
2. **늦은 참가**: 에이전트가 10분 송신 중일 때 새 viewer → snapshot+live가 1 s 안에, **중복·누락 0** (seq 연속성 검사).
3. **숨은 연결 없음**: RPC 스키마에 private/hidden 옵션 부재(스냅샷 테스트). ISO-TP·주기·SimNode TX가 모두 구독자와 저널에 도달.
4. **느린 viewer 격리**: 두 viewer 중 느린 하나만 `display.gap`, 다른 viewer·기록·송신 정상.
5. **무손실 기록**: 5k fps 60 s 주입 → 저널 프레임 수 = 생성기 기대 집합(송신·응답 포함), close 후 재독출 일치. TUI 멈춤 없음, 생략 카운터 표시.
6. **지연 측정**: 프레임 → **UI 앱 paint** p95 < 150 ms 보고(이벤트 수신 시각과 브라우저 paint 완료를 구분, `ui.latency_sample` — v2).
7. **STOP**: FF/CF 송신 중, 수신 후 FC 직전, BCM/주기 job 중, 승인 대기 중 각각 stop-all → latch 확인 후 agent/user/script origin 신규 driver submit 0(CF/FC 포함), 대기 승인 취소, 잔여 가능 수 보고, 가짜 ECU는 계속 동작, 재연결/재시작 후 자동 재개 없음.
8. **정리**: viewer 종료/재접속 vs 데몬 종료 구분, 모든 스레드/job/worker 정리.
9. **ISO-TP 우선 전달**: 버스 100% 부하(raw backlog) 상태에서 PDU/`rx_start`/STOP 이벤트가 raw 배치보다 먼저 도착, PDU 전달 p99 측정(M2).
10. **P2 barrier**: FF는 deadline 이전 ingest, `rx_start` 발행은 지연되는 fixture에서 UDS가 조기 timeout하지 않음, barrier 단절/만료는 유한 오류(M2).

### 9.4 C2 — 큐·링·메모리 상한과 24 h soak (v2.0)
값은 **제품 목표·초기 추정**(장비별 조정). worker admission profile = CAN 4채널·ISO-TP endpoint 32·주기 job 128. 넘는 open/job은 accepted 전에 `resource_exhausted`로 거절.

| 경로 | 상한 [목표] | 가득 찼을 때 | 카운터 |
|---|---|---|---|
| 드라이버/커널 → RX 스레드 | XL RX queue 8,192 요청, Linux `SO_RCVBUF` 1 MiB 요청(실효값 조회), PCAN은 vendor 고정값 게시 | HW 쪽은 backpressure 불가 → 연속 drain, 오버런 보고, 개수를 모르면 unknown | `source.gap`, `source_overrun_total`, `source_loss_unknown_total` |
| RX → 고정 링 | **65,536 × 128 B = 8 MiB** | 고정 overwrite, RX block 금지. 저널·TP가 못 따라와 밀리면 그쪽 gap | `ring_overrun_total`, consumer lag |
| 링 → 저널 writer | 32,768 슬롯(payload 4 MiB) | `try_send`; full이면 drop 범위 계수 + record unhealthy(A12 즉시). 무한 RAM·무한 재시도 없음 | `record.gap`, `record_queue_full_total`, durable watermark |
| 저널 쓰기 버퍼 | scratch 2 MiB, BufWriter 1 MiB, in-flight chunk 1개 | 디스크 오류·full·flush 지연 → degraded, 메모리 보관 확대 금지 | `record.health`, `journal_write_error_total` |
| 링 → gateway 대량 레인 | 8,192 슬롯 + wire 2 MiB 중 먼저 | RX block 안 함. live drop, 저널 정상이면 재조회 가능 | `display.gap`(원본 ID 범위), `ipc_live_drop_total` |
| 우선 레인(TP·오류·상태) | 256 entry, entry ≤ 8 KiB, 총 2 MiB | 조용한 drop 금지. 유한 대기 뒤 full → 관련 세션 실패·barrier incomplete·감독자에 fatal | `priority_queue_full_total` |
| TX 일반 요청 | 1,024 × 256 B | **accepted 전에 거절**. accepted 뒤는 deadline으로 terminal 실패 | `tx_rejected_total`, `tx_deadline_total` |
| FC·사용자·제어 예약 레인 | FC 64, 사용자 64, STOP = atomic latch | 에이전트 요청이 예약 레인을 못 씀. 사용자 flood도 상한 거절 | `fc_queue_full_total`, `user_tx_rejected_total` |
| 주기 heap·patch | job 128 + 변경 256 (256 KiB) | tick 큐 없음, 놓친 주기 skip+count, patch는 rev CAS | `periodic_missed_total`, `periodic_lateness_ns` |
| RX → ISO-TP 디스패처 | 4,096 × 128 B, endpoint 32 | full → 그 endpoint abort/error | `isotp_ingress_overflow_total` |
| ISO-TP PDU pool | 64 × 4 KiB, endpoint당 TX/RX in-flight 1 | 할당 전 거절·FC overflow | `isotp_pool_exhausted_total` |
| gateway 미러·구독(CORE/UI 경계) | 구독별 2 MiB·동시 16 [제안], 질의 응답 ≤ 1 MiB/page | 느린 뷰어만 drop/끊기, 캡처 reader block 금지 | `display.gap`, `subscriber_lag`, `query_truncated` |
링 완충 산술 [추정]: 65,536 슬롯 = 5k fps에서 약 13.1 s, 20k fps에서 3.3 s. 오래된 이벤트 참조를 붙잡는 구독자를 만들지 않는다(고정 슬롯 복사).

**worker 메모리 예산 [추정→목표]:** 링 8 + 저널 8 + IPC 8 + TX/스케줄러/TP 4 + 직렬화 4 + 스레드 스택 16 + 벤더·사용자 버퍼 24 + 런타임·코드 32 ≈ **104 MiB** → 벤더 DLL 실메모리 미측정이라 **정상 RSS ≤ 192 MiB, 상한 256 MiB/worker**. 감독자가 1 s마다 RSS·큐 고수위를 읽는다(CORE). 192 MiB가 10 s 지속 → 새 채널·job·변환 admission 중단 + degraded 알림(**정본 캡처는 조용히 끄지 않음**). 256 MiB 도달·계속 증가 → TX latch → 진행 요청 terminal/unknown → 가능한 flush·상태 보고 → worker 종료(불응 시 terminate). OS 제한(Job Object/cgroup)은 마지막 방어. ID 캐시 8,192 + other, 요청 dedupe 4,096 + TTL 등 모든 맵에 상한.

**24 h soak 관문 (M2a, Windows x64·Linux x64 각각) [목표]:** CAN 부하 = sim 5k fps(CC/FD64 반반) + TP endpoint 8개 왕복(256 B·4095 B) + 주기 job 128(≥ 100 ms, 합 ≤ 1,280 fps) + 매시간 10분 20k fps burst + 뷰어 4·늦은 구독·질의·export·사용자 송신·STOP 반복, CORE 하네스에서 UART flood·UDS 폴링과 동시. 실HW는 채널별 버스 사용률 ≤ 70%로 맞춤(sim 과부하와 분리). 판정: 정상 구간 `source.gap`·`record.gap` = 0, accepted 요청의 terminal 누락 0, **RSS ≤ 192 MiB·256 MiB 초과 0·24 h drift ≤ 16 MiB·slope ≤ 0.5 MiB/h**, 지연 p50/p99/max 게시. 고의 장애(bus-off 2 h마다, 수신기 분리, gateway 재시작, worker kill, 디스크 full·writer pause)는 별도 구간으로 판정(기대 gap·unknown·A12 latch·복구 표시). 계획·개발 중 에이전트가 돌리는 측정은 10분 이내(재개 공지 6).

**타이밍 실측 (v2, `spikes/can/rust/RESULTS.md`, macOS aarch64, 기본 우선순위·`thread::sleep`) [실측]:** 10 ms 전용 스레드 간격 오차 p99 **4.53 ms**(idle) / 4.58 ms·max 12.5 ms·skip 1(CPU 경쟁 1 스레드) → 잠정 목표 p99 < 3 ms **미달**. "Rust라서 해결"을 주장하지 않는다. M0/M2에서 Windows 고해상도 timer·우선순위·실드라이버·저널 on 조건으로 재측정하고 FF→FC(목표 host p99 ≤ 3 ms)·PDU→UDS(p99 ≤ 5 ms)도 처음 잰다.

### 9.5 통합 테스트 제공 동작 (C7 — `_xteam/test-domains.md`·`core-test.md` 형식에 맞춤)
CAN은 **사실을 평가하고 증거 ref를 돌려줄 뿐 판정하지 않는다.** 판정(pass/fail/inconclusive)은 테스트 세션 코어가 인용 증거를 검증해 내린다(D59).

| 명령 | 의미 | `data.evidence[]` | M |
|---|---|---|---|
| `send ID#DATA [--expect ID --within T]` | 송신(+응답 확인) | `{ref:"can:ch0@seq:5012", kind:"can.tx"}` + 응답 `{ref:"can:ch0@seq:5013", kind:"can.frame"}` | M1 |
| `expect --id ID [--data --mask] [--origin device] [--after mark:X\|seq:N\|now] --within T` | 조건 프레임이 기한 안에 나타나는가 | 첫 일치 `can.frame` ref / timeout이면 exit 3 + **timeout 이벤트 ref**(`kind:"can.wait_timeout"`, 창 범위) | M1 |
| `trace --since mark:A --until mark:B` | 구간 원본 조회 | `{ref:"can:ch0@seq:a..b", kind:"can.range"}` | M1 |
| `sig get` / `sig wait-for "EngRPM > 2900" --timeout T` | 신호 값 / 처음 참이 되는 순간 | `{ref:"can:ch0@seq:N#/signals/EngRPM", kind:"can.signal", value, unit, raw, msg, db_sha256}` | M2 |
| `check "EngRPM between 2900 3100" --from mark:A --to mark:B --mode always\|never\|eventually` | 이미 기록된 구간 단언(대기 아님) | `result: holds\|violated\|insufficient_data` + 범위 ref + 첫 위반 ref. 구간에 `record.gap`/`source.gap`이면 `insufficient_data` + exit 8 | M2 |
| `stats --id 0x310 --from mark:A --to mark:B --expect-period 100ms --tol 10%` | 주기 측정 | n·min·mean·p99·max·허용 밖 개수·`ts.source`(hardware/host) + 범위 ref | M2 |
- **ref 문법**은 TEST 정본 `<domain>:<channel>@seq:<a>[..<b>][#<json-pointer>]` 그대로. 신호 pointer `#/signals/<name>` 은 프레임 seq를 세션에 고정된 DB revision(`db_sha256`)으로 디코드해 해석한다(디코드 값은 저널에 따로 없어도 재현 가능).
- **술어가 읽는 안정 필드**(JSON Pointer, 스키마 고정): 프레임 `/payload/id`(u32), `/payload/extended`(bool), `/payload/fd`, `/payload/data_len`(u8), `/payload/data/data`(hex 문자열), `/dir`, `/origin`, `/ts/mono_ns`(문자열 u64), `/ts/source`; 신호 `/signals/<name>/value`(f64), `/unit`(문자열), `/raw`(i64/u64).
- **CAN 도메인 술어**(코어 `EvidenceVerifier`가 호출): `can.frame_match{id, data?, mask?, dir?, origin?}`, `can.signal_range{signal, op|between, value, tol}`, `can.id_period{id, period_ms, tol_pct}`(HW 시각이 있으면 그것으로). 코어 공통 `hold{signal,…}`·`measure`·`absent`·`count`에 CAN은 **신호 샘플 추출기**(`samples(signal, range)` — 같은 source·epoch·DB revision)를 제공한다.
- 시간 기준은 `ts.mono_ns`(같은 호스트 단조 시계). `mark:`·`@step:`은 마커의 `mono_ns`와 각 worker fence ACK(`covered_source_seq`)로 풀린다 — 배치 지연 때문에 `stream_seq`로 자르지 않는다.
- **사전 승인 항목(O7):** CAN plan 항목 = `{domain:"can", method:"can.tx.send"|"can.periodic.start"|"can.sig.set", pattern:{channel, ids, signal ranges, period range, count/duration 상한}, max_uses, risk}`. CAN은 `within_pattern(req, pattern)`과 `approval_binding`을 구현한다. R4는 사전 승인 불가. 세션 소유 job은 `owner=test:<session>`, 세션 종료·abort 때 finally로 `can.periodic.stop`(A15), 사용자 STOP이면 finally 자동 실행 없음.
- **단계 마커 표시:** `test.step_started` → Chrono Trace 전폭 행 `━━ TEST TC-012 · step 3 · … ━━`, Fixed는 상단 띠, Graphics 수직선. `test.verdict`의 증거 ref 클릭 → 해당 프레임으로 스크롤(딥링크).
- **예시 TC**(자연어 → 명령 → 증거): "sim BCM에 0x300=01을 보내면 100 ms 안에 0x301 응답, 이후 3초 동안 0x310이 100 ms ±10% 주기로 byte0=01 유지" → `loom test start` → step 1: `loom can send 0x300#01 --expect 0x301 --within 100ms --json`(ref 2개) → verdict 인용 → step 2: `loom can expect --id 0x310 --data 01 --mask FF --after mark:s2 --within 200ms`, 3초 대기(코어 `test wait`), `loom can stats --id 0x310 --from mark:s2 --to now --expect-period 100ms --tol 10%`, `loom can check "byte(0x310,0) == 0x01" --from mark:s2 --to now --mode always` → verdict 인용 → `loom test end`(코어 재검증). 사용자가 단계 2 도중 IG로 0x300=00을 보내면 `check`=`violated` + `others_since_last{origins:[user]}` → 스킬 규칙상 PASS 금지, 사용자 개입을 보고하고 `inconclusive` 또는 재실행을 묻는다.

---

## 10. 안전 / 가드레일

### 10.1 원칙
1. **집행은 데몬에서.** 스킬은 안내일 뿐. CLI/SDK/웹/UDS 어디서 와도 같은 core 쓰기 파이프라인(`loom.core.policy`/`safety`)을 통과(D6/D10). 잡형 단위 송신은 worker가 같은 규칙(latch·예약 ID·rate·하드 차단)을 프레임마다 재집행(A1).
2. **기본은 사람 확인.** sim 채널만 확인 없이 송신. **env=sim은 interface가 `virtual`(또는 Linux `vcan`)일 때만 유효** — 실HW interface에 sim을 지정하면 open 거부, virtual은 설정 없어도 sim. 실HW 채널 기본 `tx=confirm`(R2 경로 `viewer`) — 설정이 없어도 사람 확인을 거쳐 송신할 수 있다(v1.2, B4). **bench와 vehicle은 같은 규칙**(위험 등급·R2 경로·lease·rate)이고 다른 것은 상시 `env=vehicle` 배지뿐이다(B4, A10 폐기). 사람이 원하면 `tx=deny`·`deny_ids`·`allow_ids`로 좁힌다.
3. **승인은 viewer role에서만.** CLI에 approve 명령이 없다. `core.approval.respond` 는 viewer role 연결만 허용.
4. **멈출 수단은 항상 즉시.** `loom stop-all`/`loom can stop-all`, **UI 앱 셸 STOP**(보조 TUI `Ctrl-] x`)은 정책·승인 없이.
5. **잊혀진 송신 방지.** 주기 job은 lease, 끊김/만료 시 자동 정지. 크래시·재연결 후 주기 TX·ISO-TP·replay를 **자동 재개하지 않는다**.
6. **user도 무제한이 아니다.** 사용자 송신에도 rate·사용자가 설정한 하드 차단(deny_ids/allow_ids/`e2e_protected`)·ISO-TP 예약 ID 차단이 적용된다(단 승인 불필요, 1회 송신은 job ID override 허용 — §10.2).
7. **정책 완화는 viewer 승인 후에만 적용.** 데몬은 `loom.toml` 변경을 감지하면 강화(tx 축소, deny 추가, rate·lease 하향)는 즉시 적용하고, 완화(tx deny→confirm/allow, env 변경, allow_ids 확대, rate·lease 상향)는 `policy.change` 승인 요청을 띄워 viewer가 승인할 때까지 이전 정책을 유지한다. 데몬 기동 시 지난 승인 이후 바뀐 완화 항목도 같다. → 에이전트가 파일을 고쳐도 사람 승인 없이는 효력이 없다.

### 10.2 위험 등급 (공통 R0~R4 골격, CAN 매핑)
| 등급 | 정의 | 예 | 기본 처리 |
|---|---|---|---|
| R0 관찰 | 버스 영향 없음 | trace, last, sig get/wait, stats, record, export, viewer-only replay, stop-all | 항상 허용 |
| R1 시뮬 | env=sim 채널 송신 | virtual send, SimNode, sim 주기 | 허용 + 기록 |
| R2 단발 | bench·vehicle 채널에 유한 프레임(≤10) | `send 0x3A0#..`, `send --msg` | **확인(confirm)** — 경로 정책(A8): sim `auto`, 실HW 채널 기본 **`viewer`**(뷰어 Y), `loom.toml`에 사람이 `tx="allow"`를 둔 채널만 `token_or_viewer`(`loom can confirm <tok>` 또는 뷰어 Y). 동작 단위 `auto` 완화는 viewer 승인 후 |
| R3 지속 (잡형) | 주기 송신, IG 신호 patch, 버스 replay, 버스트 >10, ISO-TP open(디버그) | `periodic start`, `sig set`, `isotp open` | **뷰어 승인만**(scope once/session). 승인 객체 사용 기한 10분(A9) — 그 안에 job 시작. 시작된 job은 plan·lease 안에서 계속 |
| R4 위험 | **최근 device가 송신 중인 ID 송신(ECU 위조)**, **E2E 보호 추정 메시지**(이름 패턴·DBC 속성 — 사용자 목록 아님)를 검증된 encoder 없이 송신, init access/비트레이트 변경을 **에이전트가 요청**(loom.toml 밖), CANoe가 송신 중인 ID, 비보호 메시지에 `--base last`(RX 값 복사) | | `tx=allow`여도 **매번 승인**(scope=once만) |
| default_blocked (A11) | **CAN 기본 목록 없음**(B4로 vehicle 항목 폐기) | | 해당 없음 |
| immutable_deny (A11) | CAN은 v1.1에서 선언 항목 없음(빈 목록). 후보: 에러 프레임 주입·비트레이트 강제 변경을 지원하는 backend가 생기면 검토 | | 설정으로도 해제 불가 |
| 하드 차단 | **사용자가 설정한 것만**(B4, 기본 비어 있음): `deny_ids`, `allow_ids`(설정한 경우 그 밖), `e2e_protected` 목록 메시지를 검증된 encoder 없이 송신. + 소유권 규칙: 활성 ISO-TP 채널의 tx_id로 raw/IG/**주기/replay** 송신(UDS R4 §12, 채널이 열려 있는 동안만) | | **승인으로 우회 불가**(exit 4). agent·user 모두. 바꾸려면 loom.toml 변경 + viewer 승인(§10.1-7). `--base last`·R4 승인도 E2E 차단을 풀지 않음 |
| 중재 | 같은 (채널, ext, ID)에 두 번째 **producer**(주기 job·replay·ISO-TP 채널) | | 거절, 명시 takeover(기존 소유자 정지·권한 이전·감사)만. **사용자 1회 송신은 producer가 아니므로 우선**: 송신 + `USR override` 배지 + 소유 에이전트에 `NOTE: user overrode …`. TUI `:takeover <job>` |

- ID 충돌 승격: 최근 N초 `origin=device` 로 수신된 ID를 에이전트가 송신하려 하면 R4. DBC상 Loom이 흉내 내는 노드(SimNode, `tester_node`) 소속 메시지는 예외. env=sim에서는 승격하지 않고 경고만.
- E2E 보호 판별: `loom.toml e2e_protected` 목록 = 하드 차단. DBC 속성·이름 패턴(CRC, Chk, Checksum, Counter, Alive)만 맞는 경우 = **R4**(뷰어가 매번 승인, 사유 `E2E 보호 추정: CRC/카운터 미갱신 → ECU가 거부·DTC 가능`) — B4로 기본 차단에서 내림. 비보호 메시지만 fresh·mux·필수값·revision 검증 후 `--base last` 허용.
- 판정은 dry-run에 항상 표시: `risk=R3 reason=periodic on bench channel; plan sha 4e1f…`.
- **CAN 도메인 훅(A2):** `classify(frame|msg, channel) → (risk, reason)` = env·ID 충돌·E2E·예약 ID·버스트 판정. `approval_binding(req)` = plan digest에 {channel, id, 신호 허용 범위, 주기 범위, lease, generation} 추가. `presend_check(req)` = 승인 후·backend 직전에 버스 상태(BUS_OFF·ERROR_PASSIVE), 예약 ID, latch, channel generation 재확인 — 실패 시 승인 무효 + `E_LOCKED`/`E_QUARANTINE`(exit 6).
- UDS 서비스 의미(리셋/보안접근/플래시)는 UDS 팀 정책이 판정. **UDS 모듈이 연 ISO-TP 채널은 CAN이 별도 R3 승인을 요구하지 않고 "UDS 정책 위임"으로 감사만** 남긴다(이중 승인 방지, UDS 제안 수용). CAN은 채널 레벨 tx 정책(tx=deny면 listen_mode만), STOP, ID 예약, 프레임 rate를 그대로 집행. 디버그용 `loom can isotp open`은 R3.
- 실HW 초기 rate 수치(아래)는 안전 보증이 아니라 운영 한도·측정 기준이다. 안전 범위는 사용자가 정한 deny/allow_ids·lease와 벤치 검증으로 정한다.
- 실HW 초기 rate 제안: agent 200 fps 이하·예상 부하 +10% 이하, 버스 부하 80% 이상이면 새 R2/R3 거절. 벤치에서 확정할 값(모든 차량에 안전하다는 뜻 아님).

### 10.3 승인 흐름
```text
agent: loom can periodic start X --period 20ms
 └► gateway Policy → R3, tx=confirm → plan 생성 {channel, id, bytes, period, duration/lease, rate, sha256}
     → approval a17 (만료 120 s) → UI 앱 셸 승인 대기열 / (선택) OS 알림
     → CLI: 기본 60 s 동안 RPC 보류(`--no-wait`이면 즉시 PENDING a17, exit 5)
         승인 → 실행, exit 0 | 거부 → "DENIED by user:web" exit 5 | 만료 → "EXPIRED a17" exit 5
     → 감사: 요청·plan·결정·결정자·scope
```
- 승인 객체(D6): digest(plan hash + approval_binding 필드)·epoch·profile hash·만료·횟수에 바인딩. scope: `once` / `session`(같은 plan hash·채널·ID·rate 상한 이내 재사용, **최대 10분 = 사용 기한**).
- **잡 수명(A9):** 승인으로 시작된 job은 자기 plan(ID·신호 범위·주기)과 lease 안에서 계속 돈다. `renew`는 `max_job_duration`(기본 1 h, 채널 설정) 안에서 재승인 없이 허용(Activity·감사에 기록), 넘으면 새 승인. 실행 직전 plan을 재검증(bytes/ID/rate/기간이 승인 범위 안인지).
- **주기 job의 plan = {channel, id, 주기 범위, 신호별 허용 범위(기본 DBC min/max, 승인 시 사용자가 좁힐 수 있음), lease}.** hash는 이 범위로 계산하고, 범위 안의 `sig set`은 재승인 없이 revision만 올린다. 범위를 벗어나면 새 승인. (IG 쓰기 루프가 매 변경마다 멈추지 않게)
- `stop-all` 상태에서 실HW 채널의 `tx resume`과 `policy.change`도 같은 승인 흐름을 탄다.
- **R2 토큰 주의(A8):** 토큰 재제출은 사람 동의의 증거가 아니다. Claude Code에는 `Bash(loom * confirm:*)` `ask` 규칙을 함께 설치하고, 그 규칙이 없는 환경(Codex, bypass 권한 모드)에서는 실HW를 `viewer`로 둔다 — CAN 실HW 기본이 `viewer`인 이유.
- viewer가 하나도 없으면 즉시 `NO_APPROVER: ask the user to open the Loom UI (loom ui open)` (exit 5).
- **위협 모델과 한계 (명시):** 에이전트는 "협조적이지만 실수할 수 있는 주체"로 본다. viewer role은 (v2: **UI 앱의 일회성 bootstrap이 주 경로** — CORE·UI 계획) 또는 TTY에서 실행된 보조 `loom can monitor`(시작 시 데몬이 표시한 4자리 코드를 사람이 입력해야 발급, 비 TTY 실행은 승인 권한 없는 보기 전용) 또는 웹의 일회성 bootstrap(HttpOnly same-site 쿠키, Origin/Host 검사)으로만 발급하고 discovery 토큰(agent role)으로는 승인할 수 없다. 그러나 **같은 OS 계정의 악의적 프로세스가 사람 행세를 하는 것은 UI 분리만으로 막을 수 없다.** 이를 막으려면 별도 OS 계정/보안 경계가 필요하며 이번 범위 밖이다. 스킬은 승인 우회 시도(설정 파일 수정, viewer 흉내)를 금지하고, 감사 로그에 승인한 연결의 종류·PID를 남긴다.

### 10.4 dry-run
모든 TX 명령에 `--dry-run`: 정책 판정·위험 등급·사유, 인코딩 바이트, 신호 범위 검사(DBC min/max 밖이면 오류, `--force-range`는 R4), 비활성 mux 경고, 예상 부하 증가, plan hash. 아무것도 보내지 않음. 스킬은 R2 이상에서 "처음 한 번은 dry-run → 사용자에게 한 줄 보고 → 실행"을 요구.

### 10.5 주기 송신 · lease · 레이트
- **스케줄러(M2 기본안):** CAN worker 안 단일 스레드, 단조 절대 deadline min-heap + 깨울 수 있는 wait + 짧은 send timeout. 놓친 주기는 **skip + 카운트**(지연분 burst catch-up 금지). 신호 patch는 다음 주기에 원자적 반영(revision CAS). v2: worker의 TX 중재 전용 스레드가 heap을 소유(§3.3.1). SocketCAN BCM은 비교 측정 후보, 타이머 휠은 job 수가 많을 때 대안.
- **지터 목표는 측정 후 확정.** 실측 v1(macOS, python-can 스레드): p99 4.6~6.8 ms, CPU 경쟁 시 47~60 ms. 실측 v2(macOS, Rust 전용 스레드·기본 sleep): idle p99 4.53 ms, CPU 경쟁 p99 4.58 ms·max 12.5 ms·skip 1(§9.4). 따라서 "±1 ms"는 약속하지 않는다. 잠정 목표: CAN worker에 디코드 등 CPU 작업이 없는 조건에서 10 ms 주기 오차 p99 < 3 ms — 달성 예측이 아니라 측정 목표이며, Windows 실HW 실측 전에는 지원 SLA로 게시하지 않고 M2에서 "정확도 등급"으로 보고한다. 더 정확해야 하면 HW 주기 송신 capability를 조사(vendor API, M4/M5).
- **lease:** sim = 없음, bench·vehicle = 10 min + `renew`(스킬이 장시간 작업 시 갱신, 누적 `max_job_duration` 1 h까지 재승인 없음 — A9). vehicle 30 s는 B4로 폐기. CLI `--lease`는 채널 상한을 넘을 수 없다. lease 만료로 정지하면 Activity와 에이전트 다음 출력에 `NOTE: job p3 stopped (lease expired)`. 소유 클라이언트가 상주형(UDS 세션 관리자, viewer)이면 연결 끊김 시 정지. 단발 CLI 종료는 job 정지 사유가 아님(lease로 판단).
- **레이트:** 채널별 토큰 버킷(agent 합계 `max_agent_fps`, `max_agent_load_pct`), user는 별도 한도. 초과 요청은 큐에 쌓지 않고 거절 + 경고. ERROR_PASSIVE/BUS_OFF 진입 시 agent 주기 job 자동 일시정지 + 이벤트. bus-off 자동 복구 반복 금지.

### 10.6 STOP 의미
- `stop-all` (A3: agent·script의 새 쓰기와 **모든 잡** latch, 사용자 1회 송신은 latch하지 않음): ① CAN worker의 **최종 send 경로에 TX latch**를 먼저 건다 — agent·script origin 단발 송신과 모든 잡(사용자 IG 잡 포함)의 단위 송신, ISO-TP 스택의 CF/FC(can-isotp 기본 txfn은 `bus.send`를 직접 부르므로 txfn을 latch 경유로 감싼다; 사용자 origin 요청의 ISO-TP 전송은 예외). 사용자 뷰어의 1회 송신은 계속 허용(개입 보장). ② 주기 job·BCM 작업·replay 해제, 대기 TX 큐 비우기, 대기 중 승인 취소. ③ agent·script 요청의 활성 ISO-TP 송수신 abort(`can.isotp.abort`, R-TP-08; can-isotp `stop_sending/stop_receiving`은 완료 대기가 최대 1 s인 경로가 있어 latch가 먼저 효력을 가진다). ④ 결과에 "host 큐에서 제거한 수 / 이미 드라이버에 접수돼 취소 불가할 수 있는 수" 보고. ⑤ 가짜 ECU(device 역할 SimNode)는 멈추지 않는다 — 실HW에서 ECU가 멈추지 않는 것과 같게(`node stop` 별도). ⑥ 해제는 명시 `tx resume`만, 자동 재개 없음. ⑦ STOP이면 러너의 `finally`를 자동 실행하지 않는다(A15). 대신 뷰어에 safe_state 동작(`can.periodic.stop`, `can.stop_all` — Registry `safe_state: true`)을 "복구 송신" 1키로 제시하고, 이 동작도 인터록·deny list를 통과해야 한다. CAN은 출력 유지 장치가 없으므로 "출력 유지 중" 배지 대상 없음.
- **Vector TX 큐 flush를 호출하지 않는다**(대응 XL 함수는 SDK 대조 전 금지). 근거 python-can 4.6.1 `flush_tx_buffer()` docstring: "will flush the queue and send a high voltage message (ID = 0, DLC = 0)" (`canlib.py:934-943`). 검증 전 금지.
- 벤치 전원 차단 기능이 아니며 이미 버스에 나간 프레임은 취소할 수 없음을 UI에 명시.

### 10.7 데이터·입력 안전
- DBC/CSV/ARXML/JSONL은 비신뢰 입력: XML entity/네트워크 해석 차단, 파일 크기·중첩 깊이·문자열 길이·PDU/WS 메시지 크기·regex 실행 시간 상한.
- 파일명·choice label·UDS 디코드 문자열은 HTML로 실행하지 않는다(escape). 뷰 preset에 임의 JS/Python 금지.
- UDS seed/key·토큰 같은 비밀은 Trace/웹/export에 원문 노출 금지(UDS 팀이 redaction 표시, 임의 누락과 구분).
- 웹: loopback 바인딩, 토큰 + Origin/Host 검사(DNS rebinding 차단), URL query 토큰 장기 저장 금지, 세션 데이터/DB를 CDN에 올리지 않음(정적 파일 로컬 제공).

### 10.8 감사 로그
`~/.loom/audit/YYYY-MM.jsonl` + 세션 디렉터리 복제. 레코드: `ts, actor_id, role, origin, method, params 요약, risk, policy_decision, plan_sha, approval_id, approver(연결 종류·PID), result, frames_submitted`. append-only, 데몬만 씀. 뷰어 Audit 패널(M3), `loom audit tail --max 50`.

### 10.9 스킬에 박을 규칙 (요지, v2.0)
- 송신 전 `loom can status`로 env/tx/viewers 확인. viewers 0이면 **`loom ui open can/trace` 링크를 주고 UI를 열어 달라고** 먼저 말한다.
- R2+ 송신은 `--dry-run` 먼저, 한 줄 보고 후 실행. 주기 송신은 끝에 `periodic stop --mine`, 길면 `renew`.
- exit 4 → 보고, 재시도 안 함. exit 5 → "UI에서 승인해 주세요"(`NO_APPROVER`면 "UI를 열어 주세요"). exit 6 → 사용자 개입·잠금 보고. exit 8 / `WARNING: data loss` → 결론 전에 손실 구간 보고. exit 9 → `loom daemon status`.
- `others_since_last.count > 0` / `NOTE: user …` → 사용자 개입을 언급하고 맞춰 행동, 조용히 덮어쓰지 않는다.
- Loom이 장치를 소유하는 동안 다른 도구·스크립트(python-can, 벤더 툴, CANoe 송신 노드 추가)로 같은 장치를 열거나 송신하지 않는다(I1). 정책 완화를 위해 `loom.toml`을 고치거나 승인 UI를 흉내 내지 않는다. 비상 시 `loom can stop-all`.
- `env=vehicle`도 bench 규칙. 송신 보고에 env를 함께 말한다.
- **테스트 세션:** 판정은 검증 명령이 돌려준 `evidence[].ref`를 **복사해** 인용한다(seq·값을 지어내지 않음). trace를 눈으로 읽은 결론은 증거가 아니다. gap(exit 8)·`insufficient_data`·사용자 개입이 있으면 PASS 금지 → `inconclusive` 또는 사람에게 묻기. 자기 송신(`can.tx`)만으로 PASS 하지 않는다.

## 11. 마일스톤 (v2.0)

원칙(02): **M1 = 걷는 뼈대.** Rust sim worker + loomd + CLI + **웹 UI 최소 CAN 패널** 위에서 4요소가 모두 동작해야 완료. 이후는 루프 품질을 올리는 순서.

| M | 이름 | 내용 | 수용 기준 |
|---|---|---|---|
| **M0** | Rust 측정·계약 스파이크 (M1과 병렬) | 두 프로세스 sim 버스·STOP·강제 종료, FF ingest→FC submit(저널 on·20k fps·별도 프로세스 디코드 부하, CC/FD·BS 0/8·STmin 0/1 ms/100 µs), Windows 고해상도 timer·우선순위 전후 10 ms 주기, XL·PCAN ABI probe, DBC codec fixture, 고정 링·bounded 큐 gap 카운터 | `spikes/can/REPORT.md`에 p50/p99/max·표본 수·환경. 측정은 10분 이내 단위. CORE P0 stub(실행 가능)에 연결 — 문서 stub만으로 M1 착수 완료 선언 금지 |
| **M1** | **걷는 뼈대 (Rust + 웹, sim)** | `loomd`(CORE M1) + **`can-sim` worker**(sim 버스, RX·TX 중재·latch, 저널 writer, raw SimNode) + **gateway CAN 모듈 G1**(레지스트리·링 미러·trace/last/expect·커서·others·Activity) + **`loom can` CLI**(`status/open/close/send(--dry-run,--expect)/trace(--since @last\|mark:, --until, --peek)/last/expect/stop-all/tx resume/demo/node/record status·mark`, `loom ui open`) + **UI 최소 CAN 패널**(UI 팀: 상태 바 최소, Trace Chrono + 송신 입력줄 + 생략 행 + 출처 배지, 셸 STOP, Activity) + `loom-can` 스킬 v0 | ① §2.0 E2E를 **브라우저 E2E**로 자동 통과(에이전트 송신 → UI 행 → 사용자 웹 송신 → 에이전트 `others_since_last.count=1, origins=[user]` + `NOTE:`). ② 장치 → paint p95 < 150 ms / p99 < 300 ms(`ui.latency_sample`), TX → `requested` 행 p95 < 100 ms, trace 가시성 p95 < 50 ms. ③ 늦은 참가 < 1 s, stream_seq 중복·누락 0. ④ 20k fps 60 s: UI long task < 200 ms, 생략 행·카운터·`LIVE-DEGRADED`가 실제와 일치, 저널 = 생성 수. ⑤ 느린 탭만 `display.gap`. ⑥ UI STOP → 신규 agent submit 0까지 < 50 ms, 사용자 송신 계속, 자동 재개 없음. ⑦ `can-sim` kill → 데몬·UI 생존, `RESTARTED`, `source.gap{worker_restart}`, job 재개 없음. ⑧ 단기 soak(5k fps + 탭 2 + 에이전트 1 s마다 trace) RSS 추세 게시. ⑨ 숨은 구독 없음, publish-before-reply. **TUI만 있거나 로그만 남으면 M1 실패** |
| **M2a** | 신호·승인·검증·**PCAN-USB FD 첫 실HW** | Windows `can-pcan`(공식 FFI, FD) + Linux `can-socketcan`(FD, netdev 검증) · DBC codec·SignalCache·`sig get/wait-for`·`check`·`stats --expect-period`·증거 ref·도메인 술어 · 주기 job·IG·승인(R2 viewer, R3)·`send --msg` · Graphics/IG/Data 데이터 RPC(UI 기본 패널) · **ISO-TP 최소판**(= UDS M1b 전제, normal 11/29, CC/FD, abort·barrier·예약 ID) · BLF/ASC export · **24 h soak 관문(§9.4)** | §2.0을 PCAN FD 실HW에서 **Windows·Linux 각각**(§8.3), FD64/BRS/ESI·타임스탬프 품질 보고. confirm 채널 승인 없이 송신 불가 시험. UDS M1b 계약 시험 green, PDU→UDS p99 측정. BLF/ASC 외부 도구 호환. 벤더 worker 크래시 격리 시험. 24 h soak 통과 |
| **M2b** | **VN5640 CAN FD** (M2a와 병렬, K1 이후) | 자체 XL crate(CAN/CAN FD API만) + `can-vxl` worker, configure/shared 모드, **CANoe 측정 중 공존 실측(§8.1 ①~⑥)**, `EXT canoe` 판정 | VN5640에서 §2.0 재현, 공존 보고서 게시(성공 → shared 정식, 실패 → 그 채널은 PCAN 관찰로 대체 결정), ABI probe green |
| **M3** | UI 패널 완성 | Trace Fixed·EXT·UDS 주석·질의 기반 과거 탐색, Graphics 커서·state 레인·마커, IG takeover·mux, TRC, 장기 질의 | 웹 Trace p95 < 150 ms(정상 프로파일), Graphics age p95 < 250 ms, 웹 IG 사용자 개입 E2E, V-UDS-0 p95 ≤ 100 ms(CAN 100% 부하 포함), 20k fps에서 배지가 사실과 일치 |
| **M4** | ISO-TP 완성 (= UDS M3 전제) | 주소 7종·FD·functional group·R-TP-04/11·결함 훅 확대·예약 ID 예상 밖 프레임 감지, Statistics 패널, MF4 후보 심사 | 4095 B 멀티프레임 STmin 준수(외부 시각), UDS 통합 시험 green |
| **M5 / P4** | 후순위 | **CANoe 브리지(Rust COM 스파이크 먼저, §8.2)**, 버스 Replay, ARXML, Kvaser, TUI 보조 뷰, Linux 권한 헬퍼 검토 | 사용자 체크리스트 |

**M1에서 일부러 뺀 것:** DBC·신호·Graphics·IG·Data(루프는 raw로 성립), 승인(sim만 송신 → R1), 주기 송신, 실HW, ISO-TP, export, TUI, CANoe.

---

## 12. 구현 단계 작업 분해 (v2.0)

담당: **O** = Opus 워커(구조·CLI·정책·스킬·패널 명세), **S** = Sol 워커(드라이버·FFI·타이밍·저널·ISO-TP·시험). CORE·UI·TEST 작업은 "요구"로만. v1.2 웹·viz 작업(W1–W5, V1–V3)은 **UI 팀으로 이관**.

| ID | 작업 | 담당 | 의존 | M |
|---|---|---|---|---|
| P1 | `_xteam/can-ui.md` 패널 명세 유지·UI 회신 반영 | O | UI 셸·프레임워크 | M1 전 |
| P2 | `_xteam/can-core.md` 요구 유지·CORE 회신 반영(worker 프로토콜 두 레인, 저널 lib in worker, 도메인 모듈 trait, 열 블록, `EvidenceVerifier`) | O | CORE P0 | M1 전 |
| P3 | `_xteam/test-domains.md` CAN 의견(ref·술어·사전 승인 항목) 유지 | O | TEST·CORE | M1 전 |
| S0 | M0 스파이크(§11) | S | – (병렬) | M0 |
| K1 | `loom-can-worker` 골격 + `can-sim`(sim 버스 owner·소켓 endpoint, RX·TX 중재·latch, source_seq, 고정 링, 우선 레인) | S | CORE worker 프로토콜 stub | M1 |
| K2 | worker 저널 writer(CORE 저널 lib), `record.gap`/`source.gap`, 큐 상한·카운터(§9.4) | S | K1, CORE 저널 | M1 |
| K4 | SimNode raw 규칙 + `node`/`demo` + 결함 주입(크래시·drop) | S | K1 | M1 |
| G1 | gateway CAN 모듈: 레지스트리·라우팅, 링 미러·Fixed, `trace.query`/`fixed.snapshot`/`expect`, 커서·others, Activity, 증거 ref | O | CORE 도메인 모듈 trait | M1 |
| K3 | `loom can` CLI(M1 명령) + Registry(위험 표·훅·`safe_state`) + `loom ui open` 딥링크 | O | G1, CORE CLI | M1 |
| Q1 | M1 수용 시험: 브라우저 E2E·지연 계측·고부하·느린 탭·STOP·크래시·단기 soak | S (+UI) | K1–K4, G1, K3, UI M1 패널 | M1 |
| D0 | `loom-can` 스킬 v0 + `loom can help --json` 정본 | O | K3 | M1 |
| K5a | `can-pcan`(Windows, 공식 FFI 래퍼, CC/FD, 이벤트 수신, 시각 매핑) | S | K1 | M2a |
| K5b | `can-socketcan`(Linux FD, netdev 읽기 검증·`setup-hint`, `EXT host`, TX 확인 플래그 확인) | S | K1 | M2a |
| K6 | DBC 파서·codec(G1 안), SignalCache, 신호 시계열·min/max 피라미드 | S | G1 | M2a |
| G2 | 신호·IG 데이터 RPC(`signal_sample`, `signal.query/at`, `db.catalog`, `encode.preview`) | O | K6 | M2a |
| K7 | `sig get/wait-for`, `check`, `stats --expect-period`, 증거 ref·도메인 술어·샘플 추출기 | O | K6, G1, CORE 테스트 세션 | M2a |
| K8 | CAN 정책 매핑(위험 표, R2 viewer, approval_binding·`within_pattern`, presend, takeover/override) | O | CORE 정책 | M2a |
| K9 | 주기 heap(TX 중재 스레드 안, rev CAS, lease, 프레임별 재집행) | S | K1, K8 | M2a |
| K10 | ISO-TP 최소판(Rust 상태기계, `can.isotp.*` 계약, oracle 차등 시험) | S (+UDS) | K1, K8 | M2a |
| K11 | 버스 통계 원천 + `can.stats` | S | K1 | M2a |
| K12 | BLF/ASC export 변환 worker + 외부 호환 fixture | S | K2 | M2a |
| Q2 | 실HW 체크리스트 §8.3(Win·Linux) + 24 h soak CAN 부하·판정 | S (+CORE) | K5a/b, K9, K10 | M2a |
| K13 | `loom-vector-xl` crate(CAN/CAN FD) + `can-vxl` worker + ABI probe + 공존 실측·`EXT canoe` | S | K1 | **M2b** |
| G3 | UI 패널 완성용 질의(과거 탐색·EXT·UDS 주석·takeover) | O | G1, G2 | M3 |
| K14 | ISO-TP 완성 | S | K10 | M4 |
| K15 | Statistics 질의, MF4 후보 심사 | S | K11, K12 | M4 |
| K16 | CANoe 브리지 P4: Rust COM 스파이크 → 결정 → 구현 | S(COM) + O(CLI·정책) | CORE worker 프로토콜 | M5 |
| D1 | `loom-can` 스킬 완성(references는 `help --json`에서 생성) | O | K7, K8 | M2a~M3 |

병렬: M0 {S0} ∥ CORE P0 → M1 {K1 → K2, K4, G1 → K3 → D0, Q1} → M2a {K5a, K5b, K6 → G2, K7; K8 → K9, K10; K11, K12} ∥ M2b {K13}.

---

## 13. 리스크

| # | 리스크 | 영향 | 가능성 | 완화 |
|---|---|---|---|---|
| R1 | (v2: Rust에서도 OS 스케줄링 지연은 남음 → R21) Python GIL/스케줄링으로 FC·주기 TX 지연 (실측: CPU 경쟁 시 FC p99 52 ms, 10 ms 주기 오차 p99 47~60 ms) | 높음 | 중 | CAN worker 프로세스 격리(CPU 작업 금지), M0 재측정, 필요 시 switchinterval/우선순위, HW 주기 송신 capability 조사 |
| R2 | Vector 채널 CANoe 공존 불가/제약 (FD는 `fd=True`가 init 요청) | 높음(사용자가 CANoe 상시 사용, B3) | 중 | **P3 실측**, 실패 시 브리지(§8.2) + PCAN 관찰 대체, FD는 M4 어댑터 |
| R3 | 실시간 목표 미달 (특히 Windows + 고부하) | 높음(핵심 루프) | 중 | M1부터 지연을 CI/nightly 지표로, 화면 예산·생략 표시로 "멈춤" 대신 "생략 명시", 바이너리 WS(M3) |
| R4 | 사용자가 뷰어를 안 열어 "에이전트 혼자 보는" 상태 | 높음(원칙 위반) | 중 | I5 `no_viewer_attached` 경고, 스킬 첫 송신 전 안내, 승인 필요 동작은 NO_APPROVER로 자연 차단, `viewer.auto_open` 옵션 |
| R5 | 저널 디스크 사용·쓰기 비용 (현 schema 5k fps ≈ 2.7 MB/s, ≈ 9.7 GB/h 비압축 산술 추정) 및 worker 내 직렬화 CPU | 중 | 중~높음 | 필드 축약, seal 후 zstd, binary chunk 비교(M0/M2), 보존 quota, M0(c)에서 FC 영향 측정 |
| R6 | 기록 손실 과장/은폐 (CAN은 backpressure 불가) | 높음 | 중 | 처리량 프로파일 명시, capture/record/display 카운터 분리, `unknown`, 기록 실패 시 실HW TX fail-closed |
| R7 | 같은 OS 계정에서 에이전트가 사람 승인을 흉내 | 중 | 낮음 | viewer role 분리 + 감사, 위협 모델 한계 명시(§10.3), 필요 시 OS 경계(M5) |
| R8 | timestamp 허위 정밀도 (Vector float+offset, SocketCAN SW, 어댑터 간 시계) | 중 | 높음 | ts_source/quality, 원 시각 보존, 어댑터 간 차이 계산 금지 |
| R9 | ~~macOS PCBUSB~~ → B1로 macOS 실HW 비범위. 대신 **Linux SocketCAN 설정 편차**(netdev 이름 변동, txqueuelen, restart-ms, 권한) | 중 | 중 | 사람 사전 설정 + open 시 검증·`setup-hint`(§8.4), udev 이름 고정, 체크리스트 §8.3 |
| R10 | ~~MF4 의존성 충돌(asammdf → canmatrix)~~ → v2: Python 의존 없음. MF4는 Rust 후보 심사 전 미지원(§6.3) | 낮음 | — | 해소 |
| R11 | 웹 프런트 범위 확장 (CANoe 수준 기대) | 높음(일정) | 높음 | 패널 우선순위(Trace→Data/IG→Graphics→Stats→Replay), 패널 단위 병렬, M1은 TUI |
| R12 | 신호 patch 오류 (불완전 초기값, mux, user/agent race, 카운터/CRC 복사) | 중 | 중 | DBC 초기값 규칙, revision CAS, mux 명시, `--base last` R4, E2E 플러그인 전까지 해당 메시지 경고 |
| R13 | ISO-TP 세션 충돌 (같은 ID raw 송신) | 중 | 중 | 기본 차단 + 명시 takeover |
| R14 | 출력 상한 무시로 에이전트 컨텍스트 폭발 | 중 | 중 | 서버측 하드 상한, 요약형 명령(stats, viz summary) 유도 |
| R15 | 승인 대기로 에이전트 작업 정체 | 중 | 중 | NO_APPROVER 즉시 반환, scope=session, sim 채널 승인 불필요 |
| R16 | 프로세스 분리(gateway / 백엔드별 worker)로 M1 복잡도 증가 | 중 | 중 | M1은 `can-sim` 하나, 실HW worker는 M2a/M2b. worker 프로토콜은 CORE 공통 |
| R18 | CANoe COM 불안정(STA, 측정 재시작 시 객체 무효화, 모달 대화상자로 호출 정지, 버전별 API 차이) | 중 | 중~높음 | 별도 worker + 전용 스레드, 상태 변화 시 객체 전부 재해석, 호출 timeout → `outcome_unknown`, 버전 표, 가짜 COM CI(§8.2) |
| R19 | vehicle 송신 기본 차단 폐기(B4)로 실차에 잘못된 프레임이 나갈 수 있음 | 높음 | 낮음~중 | 실HW R2 기본 `viewer`(사람 Y), R3/R4 뷰어 승인, `env=vehicle` 배지 상시, lease, STOP, 사용자 설정 deny_ids, E2E 추정 메시지 R4. 위험 수용은 사용자 결정(B4) |
| R20 | CANoe UI에서 사람이 보낸 트래픽이 Loom 화면에 안 보임(관찰 경로 없음) | 중(원칙) | 중 | `bus not observed by Loom` 배지, PCAN 관찰 경로 안내, `EXT` 판정 근거를 실측으로만(§8.1) |
| R17 | Vector/PCAN은 여러 앱의 같은 채널 open을 허용 → 에이전트가 스킬을 어기고 직접 연 숨은 연결을 데몬이 막거나 탐지하지 못할 수 있음(탐지 가능성 미확인) | 높음(원칙) | 낮음 | 스킬 규칙(I1), 감사, 예약 ID의 예상 밖 프레임 감지(M4), `external_actor=unknown` 표시 |
| R21 | Rust에서도 주기·FC 타이밍 목표 미달(macOS 실측 p99 4.53 ms > 3 ms 목표) | 중~높음 | 중 | Windows 고해상도 timer·우선순위 실측(M0/M2), TX 중재 전용 스레드, 정확도 등급 게시, 필요 시 HW 주기 송신 capability 조사. 약속 대신 측정값 공개 |
| R22 | 벤더 SDK(`vxlapi.h`, PCAN-Basic Rust 모듈) 재배포 권리 미확인 | 중(배포) | 중 | SDK를 저장소에 넣지 않고 설치 경로·버전·해시만 기록, 빌드 시 사용자 설치 SDK 사용, 배포 전 EULA 확인 |
| R23 | 자체 ISO-TP·DBC codec 구현 비용과 표준 적합성 | 중 | 중 | `automotive`/`can-iso-tp`·cantools와 차등 fixture, 일정에 포함, 적합성 주장은 fixture 범위로 한정 |
| R24 | Rust BLF/ASC crate가 0.x 신규라 CANoe 호환 깨짐 | 중 | 중 | 외부 도구 비교 관문, 정본은 저널이라 export 장애가 캡처를 막지 않음, 필요 시 자체 writer |
| R25 | CANoe Rust COM 실기 비용(버전별 type library·이벤트 시그니처) | 낮음(P4) | 중 | 3일 timebox 스파이크 후 결정, 브리지는 진단 필수 경로 아님 |
| R26 | worker 메모리 예산(192/256 MiB)이 벤더 DLL 실사용과 안 맞음 | 중 | 중 | M2a에서 private bytes 실측 후 조정, 예산 초과 시 admission 중단 → 종료 순서 |

---

## 14. 사용자에게 물어볼 질문 (답 없으면 기본값으로 진행)

| # | 질문 | 기본값 | 영향 |
|---|---|---|---|
| Q1 ✅B2 (모델 미확인) | 보유 CAN HW 모델·채널 수·FD 여부? (Vector VN16xx/VN56xx·이더넷 여부, PCAN-USB / PCAN-USB FD) | PCAN-USB(FD 여부 모름 → CC로 시작) Win·Linux M2 + Vector 1대(공존 P3) | 첫 실HW 대상, FD 일정 |
| Q2 ✅B1 | HW가 꽂히는 PC에서 에이전트·데몬을 실행할 수 있나? | 예 — Windows x64·Linux x64 모두, macOS는 개발/virtual | 원격 bridge 필요 여부 |
| Q3 ✅B3 (버전 미확인) | CANoe를 띄운 채 같은 채널로 Loom을 쓸 일이 많나? CANoe 버전? | 많다(진단을 CANoe로) → 브리지 M2, 공존 실측 P3. 버전은 브리지가 `Version`으로 읽어 기록 | 브리지 API 차이, MCP 평가 |
| Q4 | DB 포맷: DBC만? ARXML/LDF? E2E(CRC/alive counter) 보호 메시지 송신 필요? | DBC만, ARXML은 변환 가이드, E2E 메시지 송신은 경고+차단(플러그인 M5) | 인코딩 범위 |
| Q5 ✅B4 | 실차 연결 송신이 있나? | 벤치·실차 모두, vehicle 송신 기본 차단 없음(배지만) | 위험 정책 |
| Q6 | 송신을 좁히고 싶은 ID 대역·rate·기간이 있나? | 제한 없음(B4) — 실HW R2 뷰어 확인, agent 200 fps·+10% 부하, 원하면 `deny_ids`/`allow_ids` | 실제 송신 범위 |
| Q7 | 주기 송신 정확도 요구 (10 ms ± 몇 ms면 충분?) | 소프트웨어 p99 < 3 ms(측정 후 확정), 그 이상은 HW 기능 조사 | 스케줄러 투자 |
| Q8 | 승인 강도: bench 단발 송신까지 매번? | bench R2/R3는 세션 단위 승인, R4는 매번 | UX |
| Q9 | 뷰어: herdr TUI(M1) → 웹(M3) 순서 OK? 데스크톱 앱 원하나? | TUI → 웹, 데스크톱 없음 | 일정 |
| Q10 | ISO-TP ECU 기본(주소·패딩·STmin·timeout)? ECU별 패딩 요구 자료가 있나? | normal 11bit, padding 0xCC(초기 정책값 — ECU 검증 대체 아님), BS 8, STmin 0, N_Bs/N_Cr 1000 ms — ECU 프로파일로 덮어씀 | UDS CAN 경로 |
| Q11 | 기록 보존 기간/용량, 가장 큰 로그 규모? (마스터 U12) | 30일 / 20 GiB, 감사 무기한; viz는 1M 즉시·10M progress job | 디스크·viz 목표 |
| Q12 | MF4가 초기 필수 교환 포맷인가? | 아니오 — BLF/ASC 먼저(M2), MF4 M4 선택 | 의존성 순서 |
| Q13 ✅(v2) | LGPL 의존성(python-can, asammdf) 사용 가능? | v2에서 질문 소멸 — Python 의존 없음. 벤더 SDK EULA는 Q15·R22 | — |
| Q14 (B7) | Linux 벤치 PC 배포판·커널 버전, netdev를 systemd-networkd로 고정해도 되나? | Ubuntu LTS 계열, 사람이 networkd로 고정 | §8.4 안내문 |
| Q15 (v2) | 벤치 PC에 설치된 Vector XL Driver Library·PCAN-Basic 버전은? SDK(헤더·Rust 모듈)를 빌드 머신에 둘 수 있나? | XL 26.30, PCAN-Basic 5.1 이상, 빌드는 벤치 PC에서 | FFI 빌드·ABI 시험 |
| Q16 (v2) | VN5640의 CAN FD 채널 몇 개를 쓰나? 미디어 컨버터로 DoIP를 쓰는 동안 같은 VN5640의 CAN FD도 동시에 쓰나? CANoe와 같은 채널을 동시에 쓰나? | 2채널, 동시 사용 있음, CANoe 공존 있음 | M2b 실측 항목 |

---

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 공통 계약 변경 요청 → **마스터 v1.1에서 확정** (CR1→D1, CR2→D2/A5, CR3→D3/A13, CR4→D5/A6, CR5→D6/A8–A11, CR6→D13, CR7→D6 정책 2층, CR8→D9/D10). 아래 표는 이력이며 차이가 나면 마스터 결정과 §0.1이 우선
| # | 대상 | 요청 | 근거 |
|---|---|---|---|
| CR1 | §3-2 데몬 | **사용자에게는 `loomd` 하나(포트·URL·승인·감사 하나), 내부는 gateway + 장치 owner worker 프로세스.** CAN은 M1부터 CAN worker 분리(ISO-TP·주기 TX hot path 격리), 실HW용 `can-hw`와 virtual+가짜 ECU용 `can-sim` 두 인스턴스. gateway 재시작 시 살아 있는 worker에 재부착. 다른 도메인의 worker 분리 여부는 각 팀 판단(UDS는 gateway 안 — UDS 답변) | 실측: 같은 프로세스 CPU 경쟁 시 FC p99 52 ms, 주기 오차 p99 47~60 ms. UDS 실측: 프로세스 간 PDU IPC p99 3.09 ms |
| CR2 | §3-3 IPC | 외부: **JSON-RPC 2.0 over WebSocket, 127.0.0.1 단일 포트**, 같은 오리진 HTTP 정적 웹. 이벤트 = `core.subscribe`(원자적 snapshot+`after_seq`) → `core.events` 배치(≤ 40 ms/200건/64 KiB). 바이너리 열 블록은 측정 후. 인증: discovery 파일(`<runtime_dir>/loom/daemon.json`: port, pid, token, version, 사용자 전용 권한) + role(agent/viewer) 서버 발급, viewer는 TTY 대화형 또는 웹 일회성 bootstrap→HttpOnly 쿠키, Origin/Host 검사. 내부 worker IPC는 OS별 로컬 소켓/pipe | 웹 직결, 크로스플랫폼, 승인 role 분리 |
| CR3 | §3-6 기록 | **순번 두 층**: 원본 ID `(producer_id, producer_epoch, source_seq)`(worker 할당·저널 보존) + gateway 단일 `stream_epoch/stream_seq`(구독·커서·snapshot). 공통 레코드에 `schema_version, session_id, event, actor_id, request_id, purpose, request_origin, original_origin`, `ts={mono_ns, wall_ns}` + `ts_source/ts_quality`, nullable `dir`. origin enum은 **agent/user/device/script 4종 고정**, origin은 서버가 role로 결정. 손실 이벤트 `display.gap`/`record.gap`/`record.health`(`count=unknown` 허용) 공통. 정본 = 도메인 저널, 표준 포맷은 export | 크로스도메인 정렬·커서·감사·정직한 손실 표시. UDS v0.2와 일치 |
| CR4 | §3-4 CLI | 공통 출력 계약 §4.1: 기본 100행/최대 1,000행/64 KiB, 종료 코드 0~8(5=승인, 8=부분 손실), `--since` 문법(`@last`, `seq:N`, `-5s`, 시각), `LOOM_CLIENT`, `NOTE:`(사용자 개입)·`hint:`(viewers 0)·`WARNING:`(손실) 요약 줄, mutation 자동 재시도 금지 + idempotency_key | 세 도메인 스킬이 같은 규칙 공유 |
| CR5 | §3-10 안전 | 공통 위험 골격 R0(관찰)~R4(위험) + 도메인 매핑, 환경 축 sim/bench/vehicle, **승인은 viewer role 전용**(CLI approve 없음), plan hash 승인 scope(once/session), `--dry-run` 공통, `loom stop-all`(전 도메인), lease, 크래시 후 자동 재개 금지, 정책 엔진 `loom.core.policy` 하나, 위협 모델 한계 명시 | 에이전트 자기 승인 차단, 일관성 |
| CR6 | §3-11 스킬 | 도메인 스킬 외에 **`loom` 공통 스킬**(데몬, 출력·종료 코드 규칙, 승인/안전, 뷰어 안내, stop-all) — 골자 초안은 CAN 팀 제공(D0). Codex는 공식 지원 경로 `.agents/skills/`(SKILL.md + references, frontmatter name/description)에 같은 원본을 **`loom skills sync`(복사) 기본**, symlink 선택(Windows symlink 권한 미확인) | 반복 제거. Codex skill 형식은 공식 문서로 확인(R2 Sol) |
| CR7 | 설정 | 프로젝트 루트 `loom.toml` 하나, 도메인별 섹션. 정책 **완화는 viewer 승인 후에만 적용**(강화는 즉시), 모든 변경 감사 | 한 곳 관리, 에이전트의 파일 수정 우회 차단 |
| CR8 | 뷰어 | 웹 SPA는 CAN 팀(viz) 소유, UART/UDS는 **패널 플러그인**(`register_panel(kind, topics, component)`)과 **라이브 소스 플러그인**(`register_source`)으로 기여. TUI 단축키 공통(`Ctrl-X` STOP, `/` 필터, `Space` pause, `q`) | UX 통일 |

### 15.2 CAN ↔ UDS: ISO-TP 경계 (→ `_xteam/can-uds-isotp.md` v0.3 수용 + `## can 의견`, UDS 답변으로 합의)
UDS 리드가 CAN 의견 전체(CR1 포함)를 수용했다(`## uds 답변`). UDS 세션 관리자는 gateway 프로세스 안, CAN worker와 PDU RPC. 마스터 D11 확정, A14·A16에 따라 이름 변경(아래 표기가 정본).
**수용:** PDU 단위 IPC, ISO-TP 프레임 처리는 CAN worker 안, 원시 FF/CF/FC는 공통 스트림·저널에 보임, (v2: Rust 자체 엔진 — v1은 can-isotp 2.0.7), v0.2 §9의 epoch·req_id·deadline·`rx_start`/`rx_complete`·ts_source·소유권 키·raw tx 차단·functional SF 길이(normal 7/62, extended·mixed 6/61)·max_pdu 4095·origin 4종, §10 일정(CAN M2 최소판, M4 완성; UDS M1은 DoIP).

```text
can.isotp.open({can_channel:"ch0", addressing:"normal_11", tx_id:0x7E0, rx_id:0x7E8, func_tx_id:0x7DF,
            profile:"ecu_engine"?,            # loom.toml isotp_profiles 참조(선택)
            params:{can_fd:false, tx_dl:8, bitrate_switch:false, tx_padding:0xCC|null,
                    stmin_rx:0, blocksize_rx:8, n_bs_ms:1000, n_cr_ms:1000, wft_max:0,
                    max_pdu:4095, override_tx_stmin_s:null},
            owner:{actor_id:"uds-session-u1", origin:"agent"}, lease_ms:10000})
  -> {isotp_id:"tp1", epoch:"3", effective_params, capabilities, ts_quality}
can.isotp.send({isotp_id, epoch, data_hex:"22F190", functional:false, req_id:"r42", deadline_ms, idempotency_key})
  -> {accepted:true, transfer_id:"x42"}            # 즉시 반환 (D4 accepted 단계)
event can.isotp_tx_done   {isotp_id, epoch, req_id, transfer_id, ts_first_frame, ts_last_frame, ts_source, tx_state:"driver_accepted"|"bus_confirmed"}
event can.isotp_tx_error  {…, code:"N_BS_TIMEOUT"|"FC_OVERFLOW"|"WFT_OVRN"|"BUS_ERROR"|"BUSY"|"CANCELLED"}
event can.isotp_rx_start  {isotp_id, epoch, ts_first_frame, expected_length, rx_can_id, ts_source}
event can.isotp_rx_complete {isotp_id, epoch, pdu_seq, data_hex, ts_first_frame, ts_last_frame, frame_count, rx_can_id, functional_rx, ts_source}
event can.isotp_rx_error  {…, code:"N_CR_TIMEOUT"|"WRONG_SN"|"UNEXPECTED_PDU"|"OVERFLOW"}
event can.isotp_state     {isotp_id, state:"open"|"closed"|"bus_off"|"error_passive"}
can.isotp.cancel({transfer_id}) -> {state, residual_tx_possible}
can.isotp.abort({isotp_id}) -> {aborted_tx, aborted_rx, residual_tx_possible}   # R-TP-08, STOP이 내부적으로 사용
can.isotp.barrier({isotp_id, epoch, deadline_mono_ns, timeout_ms}) -> {covered_ingest_seq, transport_event_seq, covered_mono_ns, complete}
can.isotp.open({…, role:"server"})   # R-TP-12: virtual에서 tx/rx 뒤집어 가상 ECU 쪽 채널
can.isotp.recv({isotp_id, after_pdu_seq, timeout_ms, max_pdus})      # 이벤트를 못 받는 단순 클라이언트용
can.isotp.close / can.isotp.list / can.isotp.open_group(M4)
```
**CAN 팀 결정/의견 (v0.2 미결 항목):**
| 항목 | CAN 결정 | 이유 |
|---|---|---|
| 엔진 | (v2) CAN 소유 Rust 상태기계(§3.6, D65). v1: can-isotp 2.0.7 `NotifierBasedCanStack` | 계약(이름·필드·`tx_state`)은 그대로 |
| 패딩 기본 | **Classic CAN 기본 0xCC**(초기 호환성 정책값), `null`(패딩 없음) 명시 가능, ECU 프로파일이 우선. 실효 tx_dl·패딩 바이트를 open 결과와 Trace에 공개 | DLC 8을 요구하는 ECU에서 패딩 없음 기본은 원인 찾기 어려운 무응답이 된다고 판단. ECU별 패딩 요구 검증을 대신하지 않음(근거 자료 없음 — Q10). UDS 동의 (D11) |
| BS 기본(우리가 수신측) | 8 | 수신 큐 보호, 프로파일로 0 가능 |
| 같은 tx_id raw 송신 | agent·user 모두 기본 차단, 명시 takeover만(감사) | v0.2 §9-7과 일치 |
| TesterPresent | **UDS 세션 관리자 소유**(v0.2 §9-5 동의). CAN은 일반 주기 job 기능만 있고 UDS 기본 미사용 | 진행 중 요청과의 충돌은 UDS만 안다 |
| 시각 | `tx_done`은 기본 **드라이버 접수 시각**(`tx_state=driver_accepted`, A14), TX 확인 backend에서만 `bus_confirmed`. UDS 요청의 D4 단계로는 driver_accepted = `sent`, bus_confirmed = `confirmed`. 첫/마지막 프레임 시각은 frame→PDU 추적 hook으로 붙이고 불확실하면 `ts_quality=estimated` | `bus.send` 반환 ≠ wire 완료 |
| P2 판정 | UDS 측 단조 deadline + **drain barrier** `can.isotp.barrier{isotp_id, epoch, deadline_mono_ns}`: CAN worker가 해당 host-ingest 시각까지의 **transport 처리와 `rx_start` 이벤트 발행 완료**를 확인한 뒤 `{covered_ingest_seq, transport_event_seq, covered_mono_ns, complete}` 반환. raw 큐 drain만으로 완료라 하지 않는다. barrier 자체도 유한(timeout_ms), 실패 시 "판정 불확실" 보고. late 이벤트는 epoch/req_id로 격리, 이전 응답의 req_id가 확정 안 되면 estimated | "timestamp만 쓰면 IPC 무관"은 불충분 + raw 경로와 ISO-TP 스레드 경로는 동기 완료 관계가 아님(Sol R4) |
| unknown responder | 관측(SF 기록)만, 새 주소에 자동 FC·자동 stack 생성 금지, 응답 범위는 allowlist | 자동 FC는 장비에 새 쓰기 |
| 전달 지연 | PDU 완성 → UDS 수신 p99 ≤ 5 ms를 M2에서 2프로세스 배치로 측정·보고(UDS 실측 3.09 ms 왕복이 근거). "같은 프로세스면 0"은 해당 없음 | CR1 |
| 바이트 인코딩 | RPC는 hex 문자열로 통일 | |
| 일정 | **CAN M2 (K10) = UDS M1b 전제**: normal 11/29, CC, virtual+실HW, v0.2 필드 + v0.3 R-TP-06/07/08/12/13(최소)/14/15. **CAN M4 (K14) = UDS M3 전제**: FD, 주소 7종, group, R-TP-04/11, 결함 훅 확대, 예약 ID 예상 밖 프레임 감지. UDS 문서의 "CAN K9a/K9b" = CAN K10/K14 | v0.3 §11 수용 |
| 위험 등급 조율 | **UDS 모듈이 연 ISO-TP 채널은 CAN이 별도 R3 승인을 요구하지 않음**("UDS 정책 위임" 감사만). CAN은 채널 tx 정책·STOP·ID 예약·프레임 rate만 집행, PDU 의미는 UDS 가드. 승인이 필요하면 공통 승인 서비스에서 한 패널로 | UDS 제안 수용(이중 승인 방지) |
| 예약 ID 차단 범위 | raw뿐 아니라 IG·주기·replay 경로에도 적용 (진단 프레임이 섞인 BLF 재생이 UDS 세션에 주입되는 것 방지) | UDS §12 수용 |
| 우선 전달 | PDU·rx_start·오류·STOP·상태 이벤트는 raw 프레임 배치와 분리된 우선 제어 큐(§3.3). 100% 부하에서 장치 응답 도착 시간은 보장 대상 아님, 수신 이벤트→화면 지연과 degraded 표시를 시험 | R-TP-15, UDS §12 |

### 15.3 viz 요구 (v2.0: UI 팀으로 이관)
`_xteam/viz-requirements.md`의 UART·UDS 요구(V-UART-*, V-UDS-*)는 viz 소유와 함께 **UI 팀이 넘겨받는다**(C6). CAN이 수용했던 항목 중 CAN 데이터에 걸린 것(V-UDS-5 Trace의 ISO-TP 묶음 + UDS 주석, R-TP-15 우선 전달)은 CAN 패널 명세(§5)·우선 레인(§3.4)에 남는다.

### 15.4 타팀 의존성 요약 (v2.0)
| 상대 | 우리가 필요 | 우리가 제공 | 문서 |
|---|---|---|---|
| CORE | worker 프로토콜(두 레인·크레딧·하트비트·감독·메모리 예산), worker 안 저널 lib, gateway 도메인 모듈 trait, 쓰기 파이프라인 훅, 열 블록, 테스트 세션 `EvidenceVerifier` 호출, soak 하네스 | CAN 요구·이벤트 kind·부하 생성기·측정 데이터 | `_xteam/can-core.md` |
| UI | 셸·패널 프레임워크·렌더링(U-CAN-1~10), 지연 계측 회신, 딥링크 | 패널 명세·데이터 RPC | `_xteam/can-ui.md` |
| TEST | ref 문법·술어 이름·사전 승인 형식 확정 | 검증 명령·증거 ref·도메인 술어·샘플 추출기·`within_pattern` | `_xteam/test-domains.md`, `core-test.md` |
| UDS | ISO-TP 계약(그대로), `VirtualEcuModel` Rust trait, 처리량 요구 | ISO-TP 최소판(M2a)·완성(M4), Trace ISO-TP 묶음 | `_xteam/can-uds-isotp.md` |
| UDS (CANoe) | P4에서 `canoe` 백엔드 의견 | 브리지 코어(P4) | `_xteam/canoe-bridge.md` |
- DoIP: C4.1로 PC NIC + 미디어 컨버터(UDS 소유) — CAN 의존 없음(`_xteam/vn5640-eth.md` 메인 확정).

### 15.5 스킬 설계 (v2.0, C8)
```text
skills/                     # 원본 (D13). `loom skills sync` → .claude/skills/, .agents/skills/
  loom/SKILL.md             # 공통(CORE): 데몬, 봉투·종료 코드, 승인/STOP, UI 안내, others_since_last
  loom-test/SKILL.md        # TEST 소유: TC 해석·실행·증거 인용·보고
  loom-can/SKILL.md         # ≤ 60줄. 상세 명령은 `loom can help --json`이 정본, references는 거기서 생성
```
```markdown
---
name: loom-can
description: Read, decode, send and record CAN / CAN FD through the Loom daemon (PCAN-USB FD, Vector VN5640, SocketCAN, sim). Use for CAN traces, DBC signals, waiting for and checking conditions, periodic/IG sending, ISO-TP debugging, and CAN steps in integration tests. The user watches and intervenes in the Loom UI.
---
# Loom CAN
The daemon owns the hardware. Use `loom can …` only — never open the device with other tools or scripts.
Full command reference: `loom can help --json` (authoritative).

## Workflow
1. `loom can status` — channels, env, tx policy, DB, viewers.
2. viewers 0 → give the user the UI link (`loom ui open can/trace`) and ask them to open it before sending.
3. Observe first: `trace --since @last --max 50`, `last`, `sig get`.
4. Wait, don't poll: `expect`, `sig wait-for`. Assert over a window: `check`, `stats --expect-period`.
5. Send: `--dry-run` for R2+ → one line to the user → run. Periodic: note the job id, `periodic stop --mine` when done.
6. Trends → `loom ui open can/graphics?sig=…`; don't dump numbers.

## Rules
- exit 4 denied → report, don't retry. exit 5 → approval needed in the UI; tell the user and wait. exit 6 → the user intervened or a lock. exit 8 / data-loss warning → report the gap before concluding. exit 9 → `loom daemon status`.
- `others_since_last` / `NOTE:` = the user acted. Mention it and adapt; never silently overwrite.
- Never edit loom.toml to relax policy, never approve your own request or drive the UI's approval buttons.
- env=vehicle follows bench rules; say "vehicle" when you report a send.

## As a test step (loom-test)
- Judge with: `expect` / `sig wait-for` (event happened), `check` (held over a window), `stats --expect-period` (timing).
- Copy `data.evidence[].ref` (e.g. `can:ch0@seq:5013`, `can:ch0@seq:900..1200#/signals/EngRPM`) into the verdict — never type seq numbers or values yourself.
- Your own `can.tx` alone is not evidence of DUT behavior. Gap (exit 8), `insufficient_data`, or user intervention → not PASS.
- Emergency: `loom can stop-all`.
```
`loom-viz`는 UI 팀 소유로 이관. `loom help --agent can`은 `help --json` 요약을 출력해 스킬 시스템이 없는 에이전트도 자가 발견.

---

## 16. 결정 기록 (R1/R2 의견 차이와 리드 결정)

| # | 쟁점 | Opus 안 | Sol 안 | 결정 | 이유 |
|---|---|---|---|---|---|
| D1 | 프로세스 구성 (D55·D56으로 수정) | 단일 loomd, 측정 후 분리 | supervisor + CAN/recorder/decode/gateway 다프로세스 | **loomd 하나(사용자 관점) + M1부터 gateway / CAN worker 분리, worker는 `can-hw`·`can-sim` 두 인스턴스**, 추가 분리는 측정 후 | Sol 실측(FC p99 52 ms) → hot path 격리 필요. 5프로세스는 M1에 과함. Opus R2는 2프로세스, Sol R2는 "M1 단일 허용·M2 decode 격리" — M1부터 분리는 리드 판단(M0 검증 전제). sim worker 분리는 Opus R4 P1-4 |
| D2 | 기록 정본 | BLF + events.jsonl 사이드카 | 저널 chunk + manifest, BLF는 export | **Sol** + seal 후 압축, M2에 binary chunk 비교, 내구성 창(크래시 1 s / 전원 seal 단위) 시험 | BLF엔 seq/origin 없음 + abrupt exit 1회 시험에서 reader 복구 81.9%(일반 유실률 아님). 저널도 내구성은 시험으로 확인 |
| D3 | 외부 IPC 인코딩 | msgpack 열 블록 조기 | JSON 배치 먼저 | **M1 JSON 배치, M3 측정 후 바이너리** | 5k fps JSON ≈ 1 MB/s 추정으로 M1 충분 |
| D4 | 뷰어 순서 (D61로 대체) | TUI M1 → 웹 M2 | TUI M1 → 웹 M3 | **웹 M3** | M2는 실HW/DBC/승인/ISO-TP로 이미 무거움, 승인은 TUI로 충분. Opus도 R2에서 동의 |
| D5 | 주기 스케줄러 | 자체 타이머 휠 | python-can 태스크 + capability | **단일 스레드 deadline heap(skip+count) 기본, M0/M2 비교 측정**, SocketCAN은 BCM | 타이머 휠은 정확도 증거가 아님. head-of-line·catch-up 정책이 우선 |
| D6 | 지터 목표 | 10 ms ±1 ms | 실측 기반, 미약속 | **±1 ms 철회. 잠정 측정 목표 p99 < 3 ms(CPU 격리 조건), SLA 아님, Windows 실측 후 정확도 등급으로 보고** | 실측 idle p99 4.6~6.8 ms(python-can 스레드, macOS). 개선 가능성은 미검증 |
| D7 | origin | 5종(+replay) + via, `--as-user` | 4종 + actor/purpose, 서버 결정 | **Sol**(4종, 서버가 role로 결정, `--as-user` 폐기) + Opus `sim_node` | 공통 enum 유지(UDS v0.2 동일), 승인·감사 신뢰성 |
| D8 | 위험 등급 (D49로 "미설정 read-only" 폐기) | R0~R4 + env + ID 충돌 승격 | R0~R3, 실HW 기본 read-only, plan hash grant | **병합**: R0~R4 + env + 실HW 미설정 read-only + plan hash + ID 충돌 R4 + ISO-TP 예약 ID 차단 | 서로 빈 곳을 메움 |
| D9 | 승인 주체 | viewer origin만 | grant(주체 미정의) | **viewer role(서버 발급) 전용, CLI approve 없음, NO_APPROVER, 위협 모델 한계 명시** | Sol F8 지적(자칭 origin은 경계가 못 됨) 반영 |
| D10 | lease (D49로 일부 수정) | 10 min + renew | 5 s 기본/30 s 최대 | **env별: sim 없음 / bench 10 min + renew / vehicle 30 s** | 에이전트 한 턴이 30 s를 넘기 쉬움, 위험한 곳만 짧게 |
| D11 | ISO-TP 패딩 기본 | 0xCC | null | **0xCC(Classic, 초기 정책값), null 명시 가능, 프로파일 우선, 실효 바이트 공개** | DLC 8 요구 ECU에서 패딩 없음 기본은 무응답 위험이라는 판단(보편 근거 자료는 없음 — Sol 반대 의견 기록). UDS 동의 |
| D12 | BS 기본 | (명시 없음) | 8 | **8** | 수신 큐 보호 |
| D13 | 같은 tx_id raw / producer 중재 | agent R4 승인, user 경고 | 기본 차단 | **ISO-TP 예약 tx_id = 모두 하드 차단(raw·IG·주기·replay). 일반 주기 job ID = 두 번째 producer 거절 + takeover, 단 사용자 1회 송신은 override 허용(배지+NOTE)** | 승인만으로 상태기 간섭을 막지 못함(UDS v0.2/§12 동일). 사용자 직접 개입은 원칙 4요소라 막지 않음(Opus R4 P1-11) |
| D14 | ISO-TP 일정 | M2 최소판 | M4 | **M2 최소판(v0.2 필드 포함), M4 완성** | UDS v0.2 §10 요청, UDS CAN-TP 경로가 UDS M2 |
| D15 | 출력 상한 | 200/2000행/64 KB | 100/1000행/256 KiB | **100/1000행/64 KiB** | 256 KiB는 컨텍스트 보호 목적에 어긋남 |
| D16 | 종료 코드 | 0~7(5=승인) | 0,2~6(6=부분 손실) | **0~8 통합(4=정책 거부·하드 차단, 5=승인 필요, 8=부분 손실)** | 정책 거부(보고)와 승인 필요(사용자에게 요청)를 종료 코드로 구분. 승인 대기/거부/만료/NO_APPROVER는 `error.code`로 구분 |
| D17 | 신호 송신 기준값 (D49: 하드 차단은 `e2e_protected` 목록만, 추정은 R4) | `--base last\|initial\|zero` | 전체 초기값 필수, RX 복사 금지 | **DBC 초기값 기본, 없는 신호만 필수, `last`는 비보호 메시지에만 명시 + R4. E2E/CRC/카운터 보호 메시지는 검증된 encoder 없으면 하드 차단(승인으로 해제 불가)** | 승인으로 올바른 CRC/alive counter가 생기지 않음(Sol R4 P1-06) + IG 사용성 |
| D18 | wait-for 문법 | 식 문자열 | `--name --op --value`, 제한 AST | **식 문자열 → 제한 AST, 별칭 제공, 기본은 새 샘플** | 에이전트 친화 + eval 금지 |
| D19 | 다운샘플 | MinMaxLTTB = 스파이크 보존 | MinMaxLTTB는 극값 완전 보존 아님 | **기본 픽셀 min/max(M4 계열), MinMaxLTTB는 옵션, 알람/통계는 원본** | Sol F9 사실 지적 |
| D20 | CANoe 제어 (D48→D71로 대체) | py-canoe COM 4동사 M5 | 공식 MCP 우선 | **M5, 공식 CANoe MCP → COM fallback** | CANoe 20 SP2 MCP Server 공식 확인 |
| D21 | 세션 지정 | `--channel` | `--session` 필수 | **세션은 내부 개념(epoch/기록 단위), CLI는 `--channel`(생략 가능)** | 에이전트 입력 부담 |
| D22 | SocketCAN timestamp (D66 재검토) | HW ✓ | kernel SW | **kernel SW** (Sol F1) | python-can 4.6.1 `SO_TIMESTAMPNS` |
| D23 | 첫 실HW (D46으로 수정) | M2 PCAN, M4 Vector | M2 Vector/PCAN 1종 이상 | **M2 PCAN(Windows) 우선, Vector는 M4(공존과 함께)** — 사용자 답(Q1)에 따라 교체 | 공존 검증과 묶는 것이 효율적 |
| D24 | 실HW 초기 rate (D49로 allow_ids 기본 변경) | 200 fps / +10% | 50 fps, job 5 s | **200 fps / +10%, 버스 80% 이상 거절 — 안전 보증이 아닌 운영 한도·측정 기준**, 벤치에서 확정 | 위험 범위는 빈 allow_ids = 송신 불가·하드 차단·env·lease가 제한. 50 fps도 보편 안전값은 아님 |
| D25 | 순번 | 데몬 전역 seq | (미정) | **원본 ID(producer, epoch, source_seq) + gateway stream_seq 두 층** | 프로세스 분리 후 단일 할당 주체 필요(Sol R4 P1-02) |
| D26 | 커서 키 | (client, topic, filter) | (미정) | **(client, channel), 필터 무관, `--peek`, cursor_before 반환** | 필터를 바꾸면 @last 기준점이 사라짐(Opus R4 P1-1) |
| D27 | 정책 파일 변경 | 사람만 편집(감사) | 명시 설정·감사 | **완화는 viewer 승인 후 적용, 강화는 즉시** | 에이전트 파일 수정으로 승인 체계 우회 차단(Opus R4 P1-6) |
| D28 | STOP 범위 (D36으로 일부 수정) | 주기·리플레이·노드 TX 해제 | 신규 차단 | **최종 send 경로 TX latch(ISO-TP CF/FC 포함) + abort, 가짜 ECU는 유지, 명시 resume만** | 진행 중 CF/FC·BCM이 계속되는 구멍(Sol R4 P1-01), 실HW와 같은 화면 유지(Opus R4 P1-5) |
| D29 | UDS 채널 승인 | isotp open R3 | — | **UDS가 연 채널은 CAN 승인 생략(위임 감사), 채널 정책·STOP·ID 예약·rate는 CAN 집행** | 이중 승인 방지(UDS 제안) |
| D30 | 쓰기 클래스 (마스터 A1) | — | — | **단발 send = 요청형(전체 파이프라인), 주기·IG·replay·ISO-TP 채널 = 잡형(core는 start/update/renew/stop만 게이트, worker가 단위 송신을 프레임마다 재집행)** | master-review (a)1 수용. FC·주기 타이밍 보존 |
| D31 | R3 승인 기한 (A9) | — | — | **10분 = 승인 객체 사용 기한, 시작된 job은 plan·lease 안에서 계속, renew는 `max_job_duration` 1 h까지 재승인 없음** | master-review (a)2 수용. 장시간 IG 루프 보존 |
| D32 | 메시지 크기·배치 (A5) | 64 KiB 배치 | — | **64 KiB = 요청과 이벤트 배치 1개, snapshot 분할(`snapshot_part`/`snapshot_end`), viz 대용량은 HTTP GET/`raw_ref`, 40 ms는 적응형 상한** | master-review (a)3 수용 |
| D33 | R2 경로 (A8) | tx=allow면 자동 | — | **R2 = confirm. sim `auto`, CAN 실HW 기본 `viewer`, `tx="allow"` 채널만 `token_or_viewer`(`loom can confirm`)** | 토큰은 사람 동의의 증거가 아님(bypass·Codex). master-review (a)4 수용 |
| D34 | 종료 코드 (D5/A6) | 0~8 (6=데몬) | — | **0~10 공통: 3 = 조건 대기 timeout만, 6 = 사용자·락 충돌, 7 = 장치·전송(`E_NO_RESPONSE` 포함), 9 = 데몬** | 마스터 확정. 예약 ID·하드 차단은 4(`E_DENIED`) |
| D35 | 쓰기 상태 (D4/A14) | requested/accepted(드라이버)/confirmed | — | **requested → accepted(정책·중재) → sent(드라이버 접수) → confirmed(backend 확인) → effective_verified(`--expect` 매치)/effect_unverified. ISO-TP `tx_state: driver_accepted/bus_confirmed`** | 마스터 확정, 거짓 성공 금지 |
| D36 | STOP 범위 (A3) | agent·user·script latch | — | **agent·script 쓰기 + 모든 잡 latch, 사용자 1회 송신은 허용**, 사용자 요청의 ISO-TP 전송은 abort 안 함, 가짜 ECU 유지 | 사용자는 STOP 중에도 개입 가능해야 함(원칙 4요소) — D28 수정 |
| D37 | 기록 불능 (A12) | 실HW만 차단, sim 경고 | — | **agent·script R1+ 중단(sim 포함), 사용자 송신 허용 + 배너, 감사는 stderr** | 마스터 확정 |
| D38 | 손실 3층 (A13) | 드라이버 오버런 = 기록 손실 | — | **드라이버/USB 오버런 = `source.gap`, 저널 손실 = `record.gap`, 화면 = `display.gap`** | Loom 수신 전 손실과 Loom 기록 손실을 구분 |
| D39 | 차단 3층 (A10/A11) (D49로 vehicle 항목 폐기) | 하드 차단 + R4 | — | **vehicle 송신 = `default_blocked`(unlock + viewer 승인 후 R4), 하드 차단 유지, CAN `immutable_deny`는 v1.1 빈 목록** | 마스터 확정 |
| D40 | 레코드 필드 (D3) | `event`, ts{mono,wall}+payload ts_source | — | **`kind`, `ts{mono_ns, wall_ns, clock_id, source, quality}`, 드라이버 시각은 `payload.device_ts`, 바이트 `{encoding, data}`, `metrics[]`, `client_id/job_id/generation/risk`** | 마스터 확정 |
| D41 | RPC·이벤트 이름 (D2/A16) | `isotp.*`, `can.tx.periodic.*`, `job.*` | — | **`can.isotp.*` / `can.isotp_*`, `can.periodic.*`, `core.job.*`, `hello{proto, token, client_id, want_role}`** | 마스터 확정, `_xteam/can-uds-isotp.md` 동기화 |
| D42 | 경로·보존 (D7) | 프로젝트 `.loom/`, 7일 | — | **`~/.loom/{run,records,config}`, 30일 / 20 GiB, 정책 2층, CAN은 `--no-record` 미제공** | 마스터 확정. 버스 기록은 루프의 증거 |
| D43 | 작업 경계 (§3.2) | C0–C4 CAN 담당 | — | **C0–C4 = CORE. CAN = worker + gateway 측 모듈 G1(링 미러 CAN 질의·Fixed·디코더·viz 데이터셋)** | 마스터 확정 |
| D44 | finally·복구 (D12/A15) | — | — | **`can.periodic.stop`·`can.stop_all` = `safe_state`. STOP 시 finally 자동 실행 없음, 뷰어 "복구 송신" 1키** | 마스터 확정 |
| **D45** | **v1.2 사용자 답변 반영 (마스터 §2.2 B1–B7)** | — | — | **Windows+Linux 실HW, CANoe 브리지 M2, 공존 실측 P3, vehicle 기본 차단 폐기 — 세부는 D46~D53. 바뀐 절만 수정(§0.2, §3.2, §3.4, §4.1, §5.2, §6.5, §7, §8~8.4, §10, §11~§14, §15.4)** | 사용자 답변 원문(마스터 §2.2) |
| D46 | 첫 실HW (B1) | — | — | **M2 = PCAN, Windows(PCAN-Basic)와 Linux(SocketCAN `peak_usb`) 둘 다. macOS 실HW(PCBUSB) 비범위. Vector는 단독 configure 가능, 공존은 P3** | 실HW 호스트가 두 OS. 같은 장치로 두 경로를 검증하는 것이 가장 쌈 |
| D47 | Linux netdev 설정 주체 (B1) | — | — | **사람 사전 설정(ip / systemd-networkd), Loom은 open 시 검증 + `setup-hint`. 권한 헬퍼 없음(M5 polkit 검토)** | root 헬퍼 공격면·sudo 흐름 단절, 에이전트의 비트레이트 변경은 이미 R4. python-can socketcan에 비트레이트 인자 없음(소스 확인) |
| D48 | CANoe 브리지 (B3) (D71로 대체 — P4) | — | — | **M2 정식: `loom/canoe/` pywin32 얇은 래퍼, `canoe-bridge` worker의 COM STA 스레드 하나, 측정·신호·sysvar·재연결 + UDS용 내부 진단 API. CANoe 자동 실행 안 함. py-canoe는 참고만. 공식 MCP는 M5 평가** | 사용자가 진단을 CANoe로 수행. COM 정지·무효화를 hot path와 격리 |
| D49 | vehicle·기본 차단 (B4) | — | — | **vehicle = bench 규칙(R2 경로·lease 10 min·rate). `default_blocked` CAN 목록 비움. 하드 차단 = 사용자 설정(deny_ids·allow_ids·`e2e_protected`) + ISO-TP 예약 ID. 실HW 미설정 채널 = `tx=confirm`(read-only 아님), `allow_ids` 없으면 제한 없음. E2E 이름 패턴 추정은 R4로 내림. `env=vehicle` 배지 상시** | 사용자: "굳이 송신을 막을 이유는 없지". 사람 확인(R2 viewer)·R3/R4 승인은 유지 |
| D50 | 외부 트래픽 표시 (B3) | — | — | **origin enum 유지(`device`) + `external_actor` = `canoe`(공존 포트의 다른 앱 송신 표시, P3 실측으로 근거 확보) / `host`(Linux `MSG_DONTROUTE`) / `unknown`. 배지 `EXT`, 관찰 경로 없으면 `bus not observed by Loom`** | 근거 없는 단정 금지, 안 보이는 것도 화면에 명시 |
| D51 | 공존 실측 시점 (B3) | — | — | **P3(CAN M3 기간) K13a, 실패 시 브리지 + PCAN 관찰 대체. FD 어댑터는 M4 K13b** | 사용자가 CANoe 상시 사용 → 조기 확인 필요 |
| D52 | 브리지 위험 등급 | — | — | **측정 시작 R3, 정지 R2, sig/sysvar set R2, get R0. STOP은 브리지 agent·script 쓰기만 latch, CANoe 측정은 멈추지 않음(뷰어에 별도 정지 키)** | CANoe 측정·시뮬 노드는 Loom 소유가 아님 |
| D53 | Linux TX 확인 (D66으로 대체) | — | — | **v0 `tx_confirmation=false`(`sent`까지), M4 MSG_CONFIRM 어댑터 검토** | python-can socketcan이 own-echo와 다른 로컬 송신을 구분하지 않음 |
| **D54** | **v2.0 방향 전환 (마스터 §2.3 C1–C8, C4.1)** | — | — | **Rust 코어·worker·FFI, Python 의존 0, 웹 UI 1급(CAN = 패널 명세 + 데이터 질의, 웹·viz는 UI 팀), 통합 테스트 증거, VN5640 = CAN FD 전용 1급(M2b), PCAN FD M2a, CANoe P4. 바뀐 절만 수정(§0·§0.3·§1·§3.1–3.3·§3.6·§5·§6.3–6.4·§7·§8·§9.1·§9.4–9.5·§10.9·§11·§12·§13–15)** | 마스터 C1–C8, C4.1, 워커 초안 `v2-opus.md`·`v2-sol.md` |
| D55 | worker 분리 단위 | 백엔드별 | 물리 장치별 | **백엔드(장치)별: `can-sim`/`can-pcan`/`can-socketcan`/`can-vxl`, 실행 파일 하나, 기본 장치당 1 worker(≤ 4채널), 채널별 분리 거부** | C2 크래시·메모리 격리, XL 다채널 시계 일관성 |
| D56 | 분리 근거 | GIL → C2 | 같음 | **근거 = C2 격리 + 타이밍 스레드 분리. Python 실측을 Rust 근거로 쓰지 않음** | 정직성 |
| D57 | VN5640 범위 | CAN FD + 이더넷, worker `vxl` | CAN FD + 이더넷 L2 M2 | **CAN FD 전용(C4.1), worker `can-vxl`, XL 이더넷·smoltcp 범위 밖** | 사용자 확인: DoIP는 PC NIC + 미디어 컨버터 |
| D58 | 디코드 위치 | gateway | gateway bounded pool | **gateway CAN 모듈, 요구된 메시지만, 상한 작업 큐** | worker를 작게 |
| D59 | 검증 명령 | 사실 평가, 판정은 세션 | registry 등록 | **`expect`·`check`·`stats --expect-period`·`sig wait-for`는 사실·증거만, 판정은 테스트 세션 코어** | 환각 방지·책임 분리 |
| D60 | 증거 형식 | `src{producer,epoch,source_seq}`+query+digest | 원본 ID 범위 | **TEST 정본 ref `can:<ch>@seq:a..b[#pointer]` + CAN 도메인 술어(`can.frame_match`·`can.signal_range`·`can.id_period`) + 신호 샘플 추출기. 원본 ID 매핑·digest는 코어 검증기가 처리** | `test-domains.md`·`core-test.md` O3–O4와 통일 |
| D61 | M1 뷰어 | 웹 최소 패널 | 웹 필수 | **UI 앱 최소 CAN 패널(UI 팀 구현), TUI는 M5 보조** | C6 |
| D62 | UI 지연 게이트 | p95 < 150 ms | 계승 | **장치→paint p95 < 150 ms / p99 < 300 ms, 브라우저 계측 회신** | UI 브리프 200 ms보다 여유 |
| D63 | Vector crate | 자체 | 자체 + `vxlapi-sys` 존재 정정 | **CAN 소유 `loom-vector-xl`(CAN/CAN FD API만), `vxlapi-sys` 2.1.0은 비교용. 마스터 C4의 "공개 crate 없음"은 사실 정정 요청** | shared FD `permissionMask=0`, 이벤트 보존, 시계 리셋 금지 |
| D64 | PCAN Windows | 공식 FFI 여부 미확인 | 공식 모듈 확인 | **PCAN-Basic 5.1.0.1194 SDK의 공식 Rust 모듈 + RAII 래퍼, SDK는 커밋 안 함** | `InitializeFD/ReadFD/WriteFD` 소스 확인 |
| D65 | ISO-TP 엔진 | 미정 | 직접 구현 | **CAN 소유 Rust 상태기계, `automotive`/`can-iso-tp`는 oracle** | 최종 TX 관문·두 완료 상태·barrier·증거를 한곳에서 |
| D66 | SocketCAN | — | `socketcan` 4.0.0, nl getter | **`socketcan` crate, netlink 읽기 전용 검증, TX 확인은 recvmsg 플래그 직접 확인 후 결정(D53 대체)** | python-can 제약을 그대로 옮기지 않음 |
| D67 | DBC | — | `can-dbc` + 자체 codec | **채택, 미지원 문법은 경고·쓰기 영향 시 fail closed** | 사용 중 DB 교체(IG·CLI)에 codegen 부적합 |
| D68 | 로그 포맷 | — | autors-blf/asc 후보 | **BLF/ASC M2 후보(외부 호환 관문), TRC 자체 M3, MF4 미지원 명시** | Python asammdf 숨은 의존 금지 |
| D69 | C2 상한 | 자리 | 표 | **§9.4 표 채택, worker 192/256 MiB, 24 h soak drift ≤ 16 MiB·slope ≤ 0.5 MiB/h** | C2 |
| D70 | VN5640 시점 | M3 | M2 | **M2b(M2a와 병렬)** | C4 1급 경로, 자체 crate로 FD 공존 바로 실측 가능 |
| D71 | CANoe 언어 | Python P4 | Rust COM 가능 | **P4, Rust COM 3일 스파이크 먼저, Python 예외는 blocker 입증 시에만 결정 기록** | C1 "대안이 없을 때만 Python" 조건 미충족 |
| D72 | sim 버스 | — | 로컬 소켓 | **`can-sim` 안 bus owner + 로컬 소켓 endpoint, 공유 메모리 제외** | 프로세스 간 sim 필요(UDS·외부 sim), 근거 없는 최적화 배제 |
| D73 | 웹·viz 소유 | 이관 | 이관 | **UI 팀. CAN은 패널 명세(`can-ui.md`)·질의 RPC** | C6 |
| D74 | 공존 대체 경로 | — | — | **공존 실패 시 PCAN 관찰 채널(브리지 대체 아님)** | 브리지 P4 강등 |
| D75 | 스킬 | v2 골자 | 짧게 | **`loom-can` ≤ 60줄 + "테스트 단계" 절, 정본 `help --json`** | C8, TEST 요청 §4 |

---

## 핵심 원칙 체크리스트 (02-core-principle.md) — 리드 검증

**v2.0 검증:**
- [x] **핵심 루프 표** — §0 (에이전트 명령 / Rust 구성요소 / UI 앱 패널 / 첫 M)
- [x] **종단 간 시나리오** — §2.0(데이터·순서) + §11 M1 ①(브라우저 E2E로 판정) + §5.3 M1 와이어프레임 + §9.5 TC 예시(사용자 개입 포함)
- [x] **실시간성 목표 + 고부하** — §3.8(웹 p95 < 150 ms 예산), §5.1, §9.4(큐 상한·넘칠 때 정책·24 h soak), Rust 타이밍 실측 미달을 그대로 공개
- [x] **M1 = 걷는 뼈대** — §11 M1(Rust sim worker + loomd + CLI + 웹 최소 패널), 뺀 것과 "TUI만이면 실패" 명시
- [x] **사용자 화면이 에이전트와 분리되지 않음** — §0 ④, §5.2 송신 입력·IG patch/takeover, I1~I6 유지, `bus not observed by Loom` 배지
- [x] **후순위/비범위** — §1.2, §11 M5/P4(CANoe), C4.1 이더넷 비범위

**v1.2 검증(이력):**

- [x] **맨 앞 "핵심 루프" 절, 4요소 표** — §0 (에이전트 명령 / 데몬 구성요소 / 사용자 화면 / 첫 동작 마일스톤)
- [x] **종단 간 시나리오** — §2.0: 에이전트 `send 0x300#01` → 가짜 ECU `0x301` 응답 → TUI에 AGT/DEV 행 p95 < 200 ms → 사용자 `:send 0x300#00` 개입 → 에이전트 `trace --since @last` 에 USR 행 + `NOTE:`. CLI·TUI 화면 포함, §9.3-1로 자동 시험
- [x] **실시간성 목표 + 고부하 동작** — §3.8: 경로별 p95/p99 표(TUI < 200 ms, 웹 < 150 ms, UDS ≤ 100 ms, late join < 1 s …), 정상/고부하 프로파일 정의, 기록 무손실 경로와 화면 예산 분리, 생략 행·`display.gap`/`record.gap`·다운샘플 명시·LAG/라이브 점프·`unknown` 손실
- [x] **M1 = 걷는 뼈대** — §11 M1(virtual + loomd/CAN worker + send/trace + TUI 라이브 + 사용자 `:send` + 저널 + 최소 안전), 뺀 항목과 이유, 실패 판정 문구. §12.1 임계 경로
- [x] **사용자 화면이 에이전트 세션과 분리되지 않음** — §3.7 불변식 I1~I6(단일 경로, 비공개 토픽 없음, 원자적 snapshot+live, 읽기도 Activity, viewers:0 경고, 사용자 개입 NOTE), §9.3 자동 시험, 스킬 규칙 §10.9
- [x] **원칙에 기여하지 않는 기능은 후순위/비범위** — §1.2 우선순위 표(필수/강화/후순위/비범위), §11 M5
- [x] **v1.2 새 경로도 "사용자가 무엇을 언제 보는지" 명시** — CANoe 브리지 §8.2 표(배지·Activity·승인 바·`EXT`·`bus not observed by Loom`), Linux 실HW §8.3(같은 화면, `EXT host`), vehicle 배지 §5.2·§4.1
