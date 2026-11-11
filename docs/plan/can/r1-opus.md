# ② CAN + 시각화 — R1 독립 초안 (can-opus)

작성: can-opus (Claude Opus 5.5), 2026-11-09
상태: R1 독립 초안 (긴급 추가 지시 `tasks/r1-addendum-core.md` / `02-core-principle.md` 반영 완료). 구현 코드 없음.
관점 가중치: 아키텍처 · 사용자/에이전트 UX · 스킬 설계 · 안전 규칙. 라이브러리 깊은 조사는 can-sol 쪽 몫이라 확신 없는 항목은 **미확인**으로 남겼다.

### 이 문서에서 직접 확인한 사실 (출처)

| 항목 | 확인 내용 | 출처 |
|---|---|---|
| python-can | 최신 4.6.1 (2025-08-12), LGPL-3.0-only, Python ≥3.9 | PyPI JSON API, 2026-11-09 조회 |
| python-can pcan on macOS | `platform.system()=="Darwin"` 이면 `PCBUSB` 라이브러리 로드 | `can/interfaces/pcan/basic.py:673` (4.6.1 wheel) |
| python-can vector | `app_name` 기본값 `"CANalyzer"`. `bitrate/fd/timing/listen_only` 중 하나라도 주면 init access(채널 설정 권한) 요청, 아니면 요청 안 함 | `can/interfaces/vector/canlib.py:93, 234-237` |
| python-can 주기 송신 | `_send_periodic_internal`(드라이버 기반 주기 송신) 구현은 socketcan(BCM), ixxat 뿐. vector/pcan은 python 스레드 기반 소프트웨어 주기 송신. `ModifiableCyclicTaskABC.modify_data()` 존재 | `can/broadcastmanager.py:195,218`, `grep` 결과 |
| python-can virtual | `protocol=CanProtocol.CAN_FD` 지정 가능, 프로세스 내부 큐 기반 | `can/interfaces/virtual.py:60` |
| python-can io | asc, blf, csv, mf4, trc, sqlite, canutils, player, logger | `can/io/` 디렉터리 |
| can-isotp | 2.0.7 (2025-05-14), MIT. `NotifierBasedCanStack(bus, notifier, ...)` 존재. 주소 모드 Normal/NormalFixed/Extended/Mixed × 11/29bit. Params: `stmin, blocksize, override_receiver_stmin, rx_flowcontrol_timeout, rx_consecutive_frame_timeout, tx_padding, wftmax, tx_data_length, can_fd, bitrate_switch, rate_limit_*, listen_mode, blocking_send` | `isotp/protocol.py:300-370, 1759`, `isotp/address.py:10` |
| cantools | 45.1.2 (2026-10-29), MIT | PyPI |
| canmatrix | 1.3.3 (2026-10-21), BSD-2-Clause | PyPI |
| asammdf | 8.8.32 (2026-11-03), LGPLv3+ | PyPI |
| textual | 8.2.8 (2026-06-30), MIT | PyPI |
| pyqtgraph / PySide6 | 0.14.0 MIT / 6.12.0 LGPL-3.0 | PyPI |
| tsdownsample | 0.1.5.1 (2026-06-01), MIT (MinMaxLTTB 등) | PyPI |
| websockets / fastapi / uvicorn / msgspec | 17.4 BSD / 0.162.2 MIT / 0.54.0 BSD / 0.22.0 BSD | PyPI |
| py_canoe | 26.4.1 (2026-08-30), MIT, CANoe COM 래퍼 (Windows) | PyPI |
| ldfparser / autosar-data | 0.26.0 MIT / 0.17.0 (라이선스 표기 없음 → 미확인) | PyPI |
| python-can-cvector | 1.6.0, LGPL v3 (Vector용 대체 백엔드로 보임, 내용 미확인) | PyPI |

---

## 0. 핵심 루프

> `docs/plan/02-core-principle.md` 준수 (브리프보다 우선): **에이전트가 CAN 장비에 직접 접근해 읽고 쓰며, 사용자는 같은 데이터를 같은 순간에 라이브로 본다 (출처 agent/user/device 표시, 사용자 직접 개입 가능).** 이 문서의 모든 기능은 이 루프를 위한 부수 기능이며, 루프에 기여하지 않는 것은 §1 "후순위/비범위"로 밀었다.

| 요소 | 에이전트 명령 (CLI) | 데몬 구성요소 (loomd) | 사용자 화면 | 첫 동작 마일스톤 |
|---|---|---|---|---|
| ① 직접 접근 | `loom can status`, `loom can open ch0 --iface virtual\|pcan\|vector ...` (사람 개입 없이 채널 열기, 데몬 자동 기동) | ChannelManager: python-can Bus + Notifier(RX 스레드)를 **데몬만** 소유. CLI는 장치를 열지 않음 | 상단 채널 바: `ch0 virtual 500k ● load 4% err 0` + 접속한 클라이언트 목록(`agent:claude-1`, `user:tui`) | M1 (virtual), M2 (PCAN), M4 (Vector) |
| ② 읽기 | `loom can trace --since @last --max 50`, `loom can last`, (M2) `sig get`, `sig wait-for "X > 10" --timeout 30s`, `stats` | RingBuffer(seq) + 커서 저장소 + (M2) Decoder/SignalCache, 경계 있는 질의만 | 에이전트가 무엇을 읽었는지 Activity 줄에 표시 (`agent read ch0 trace 50 rows`) | M1 (raw trace/커서), M2 (신호) |
| ③ 쓰기 | `loom can send 0x300#01 [--dry-run]`, (M2) `send --msg X --set S=V`, `periodic start`, `sig set` | Policy(R0~R4) → RateLimiter → TxScheduler → bus.send. **송신 프레임은 같은 EventBus에 origin=agent 로 주입** | 승인 배너/STOP ALL TX, 에이전트 프레임은 주황 `AGT` 배지 | M1 (raw 1회 송신 + stop-all), M2 (주기/IG/승인) |
| ④ 사용자 실시간 표시 + 개입 | (보는 쪽) `loom can monitor` (TUI, herdr 패인), 브라우저 `http://127.0.0.1:7341/` | WS Broadcaster: 모든 이벤트를 20~50 ms 배치로 구독자에게, 구독 시 최근 버퍼(backlog) 먼저 전송. 사용자 송신 = 같은 TX 경로에 origin=user | Trace(색: AGT 주황 / USR 파랑 / DEV 회색·흰색), Graphics, Data, IG. TUI `:send` 줄, 웹 IG 패널로 직접 송신 | M1 (TUI 라이브 트레이스 + `:send`), M2 (웹 Trace/IG), M3 (Graphics) |

루프 보장 장치 요약: 숨은 연결 금지 불변식 §3.11, 실시간성 목표 §3.12, 종단 간 시나리오 §2.0, 걷는 뼈대 M1 §11.

---

## 1. 범위 / 비범위

### 결론
CAN 컴포넌트는 **(a) CAN 채널을 소유하는 데몬 서비스, (b) 에이전트용 경계 있는 CLI, (c) CANoe 사용 경험을 옮긴 웹 뷰어 + 최소 TUI, (d) 도메인 공통 범용 시각화 `loom.viz`, (e) UDS 팀에 제공하는 ISO-TP 호스팅** 다섯 덩어리다. CANoe를 대체하는 시뮬레이션 툴(CAPL, 패널 디자이너, 테스트 모듈)은 만들지 않는다.

### 범위 (In)
핵심 루프 기여도로 우선순위를 매겼다: **필수(루프 자체)** → **강화(루프 품질: 신호 해석, IG, 플롯)** → **후순위(루프와 간접)** → **비범위**.

| 우선순위 | 항목 |
|---|---|
| 필수 (M1) | 데몬 채널 소유, virtual 백엔드, raw send/trace/커서, 출처 태깅, 라이브 TUI 트레이스 + 사용자 송신, 웹/TUI 구독 시 backlog, stop-all, 최소 자동 기록 |
| 강화 (M2~M4) | DBC 신호 인/디코드, wait-for, 주기 송신/IG, 승인 흐름, 웹 Trace/IG/Graphics, PCAN/Vector 실HW, ISO-TP 호스팅(UDS 루프의 전제), stats |
| 후순위 (M5 이후) | Replay 버스 송신, 레이아웃 저장/도킹, Statistics 고도화, MF4/TRC 등 다수 포맷 export, viz 파일 자동 렌더의 고급 규칙, PNG 스냅샷, CANoe COM 제어, ARXML, Kvaser/gs_usb, 가상 시계 모드 |
| 비범위 | CAPL, LIN/LDF, FlexRay/Ethernet, J1939/CANopen 해석, 물리층 분석, 데스크톱(Qt) 뷰어 |

| 영역 | 내용 | 마일스톤 |
|---|---|---|
| 백엔드 | python-can 기반: `virtual`(M1), `pcan`(M2), `vector`(M4), `socketcan`(CI 선택), `kvaser`/`gs_usb`(후순위). Classic CAN + CAN FD, 다채널, nominal/data 비트레이트, HW 타임스탬프 사용 | M1~M4 |
| 채널 공존 | CANoe/CANalyzer가 같은 Vector 채널을 쓰는 상태에서 init access 없이 붙기(관찰 + 제한적 송신) | M4 |
| DB | DBC 필수 (cantools). 멀티플렉스, 값 테이블(VAL_), 신호 스케일/단위, 노드/주기(GenMsgCycleTime) 속성 | M2 (M1은 raw) |
| 에이전트 동작 | 트레이스 조회(tail/since/filter/커서), 1회 송신, 주기 송신, 신호 단위 송신(IG), 신호 조회/대기(`wait-for`), 통계, 기록 시작/정지/내보내기, 시뮬 노드 시작/정지 | M1~M2 |
| 사용자 도구 | herdr용 TUI 라이브 트레이스 + 사용자 송신(M1). Trace, Data, IG(M2), Graphics, State(M3), Statistics(M4), Replay(M5) — 웹 | M1~M5 |
| loom.viz | CSV/JSONL/Parquet/BLF/ASC/MF4/라이브 스트림 → 자동 렌더, 한 줄 CLI, 수백만 포인트 다운샘플링, 타팀 요구 수용 | M3 |
| 기록 | 세션 자동 기록 (BLF + 사이드카 이벤트 JSONL), BLF/ASC/MF4/CSV/Parquet 내보내기, 가져오기 | M1~M3 |
| ISO-TP 호스팅 | CAN 데몬 내 can-isotp 스택, UDS 팀용 in-process API + RPC | M4 |
| 시뮬레이션 | virtual bus + DBC 기반 가짜 ECU 노드(주기 송신, 간단한 응답 규칙), ISO-TP 에코/UDS 시뮬 노드 자리 | M1~M2 |
| 안전 | 채널 정책(env/tx/listen-only), 위험 등급, 승인 흐름, dry-run, 레이트 제한, lease 기반 주기 송신 자동 정지, 감사 로그 | M1~M2 |
| 스킬 | `.claude/skills/loom-can/`, `.claude/skills/loom-viz/` + references | M1~M3 |

### 비범위 (Out) — 근거
| 항목 | 판단 | 근거 |
|---|---|---|
| CAPL 호환 / 스크립트 언어 | Out | 에이전트 자체가 "스크립트 작성자". 필요한 건 Python 시나리오(`loom can script run x.py`) 정도로 충분 (M5 후보) |
| ARXML 전체 지원 | **M5 선택** | 실사용 빈도 대비 비용 큼. cantools가 ARXML 일부(시스템/ECU extract) 로드 지원하나 품질은 미확인 → 우선 "ARXML→DBC 변환 후 사용" 가이드. 사용자 질문(§14-Q4) |
| LDF / LIN | Out (M5 이후 별도) | LIN은 별 HW·별 버스. 같은 데몬 구조에 얹을 수 있게만 설계 |
| CANoe 자체 제어 (COM/XIL) | **M5 선택, 얇게** | 아래 근거 |
| J1939 / CANopen 상위 프로토콜 | Out (디코드 플러그인 자리만) | 요구에 없음 |
| FlexRay / Automotive Ethernet(SOME/IP) | Out | 별도 도메인 |
| 오실로스코프 수준 물리층 분석 | Out | HW 영역 |
| 플래시(UDS 0x34/36/37) | CAN 팀 Out | UDS 팀 영역, CAN 팀은 ISO-TP 처리량만 보장 |

**CANoe 제어 범위 판단.** CANoe COM API는 Windows 전용이고 CANoe 라이선스가 있어야 동작한다. 우리 목표는 "CANoe 없이도 CANoe 수준"이며, 사용자가 CANoe를 이미 쓰는 경우 필요한 건 (1) 같은 HW 채널을 관찰/송신하는 것(= XL 공존, 범위 In)과 (2) 가끔 CANoe 측정 시작/정지, 시스템 변수 읽기/쓰기, 테스트 모듈 실행이다. (2)는 `py_canoe`(MIT, 26.4.1, COM 래퍼)로 얇게 감쌀 수 있으므로 **M5에 `loom canoe start|stop|sysvar get/set|test run` 4개 동사만** 선택 범위로 둔다. Vector 공식 Python 패키지/XIL API 지원 상태는 **미확인** (Sol 조사 결과 따름).

---

## 2. 사용 시나리오

공통 전제: 사용자는 터미널(herdr) 한 패인에 에이전트, 다른 패인에 `loom can monitor` TUI, 브라우저에 `http://127.0.0.1:7341/` 웹 뷰어를 띄워 둔다.

### 2.0 종단 간 시나리오 (핵심 루프) — M1에서 그대로 동작해야 하는 것

설정: `ch0 = virtual "bench-sim"`, 데몬 안에 가짜 ECU `BCM(sim)` 노드. 규칙: 0x300(Light_Req) 수신 시 5 ms 안에 0x301(Light_Ack) 응답 + 100 ms 주기 0x310(Light_Status)의 byte0 을 요청값으로 변경. 가짜 ECU 프레임은 실 ECU와 같은 코드 경로를 타도록 **origin=device** (메타 `sim_node=BCM`).

| 시각 | 누가 | 동작 | 사용자 화면 (목표 지연) |
|---|---|---|---|
| T0 | 사용자 | herdr 패인에서 `loom can monitor ch0` 실행 (에이전트가 이미 버스를 쓰는 중이어도 됨) | 즉시 최근 30 s backlog + 현재 Fixed 스냅샷 표시 → 이어서 라이브 (< 500 ms) |
| T1 | 에이전트 | `loom can send 0x300#01 --channel ch0` | — |
| T1+~2 ms | 데몬 | 정책(R1, sim) 통과 → bus.send → EventBus에 `tx agent` 주입, seq=5012 | TUI/웹 Trace에 주황 `AGT 0x300 01` 행 (< 200 ms, 목표 p95 120 ms) |
| T1+~5 ms | BCM(sim) | `rx 0x301 01` 응답, 다음 주기 0x310 byte0=01 | `DEV 0x301 01` 행, Fixed 행 0x310 의 byte0 하이라이트 (< 200 ms) |
| T2 | 에이전트 | `loom can trace --since @last --channel ch0` | Activity 줄 `agent:claude-1 read ch0 trace (3 rows)` |
| T3 | 사용자 | TUI에서 `:` → `send 0x300#00` 입력 (또는 웹 IG 패널 Light_Req → HeadLamp=Off → Send) | 파란 `USR 0x300 00` 행, 이어서 `DEV 0x301 00` (< 200 ms) |
| T4 | 에이전트 | `loom can trace --since @last --origin user,device` | — |

에이전트 CLI 화면:
```text
$ loom can send 0x300#01 --channel ch0
sent ch0 tx 0x300 [1] 01   seq 5012  t=812.004  origin=agent:claude-1  risk=R1(sim)

$ loom can trace --since @last --channel ch0
seq   t(s)      ch  dir org  id     dlc data
5012  812.004   ch0 tx  AGT  0x300  1   01
5013  812.009   ch0 rx  DEV  0x301  1   01          (sim:BCM)
5021  812.050   ch0 rx  DEV  0x310  8   01 00 00 00 00 00 00 00
-- 3 rows (0x310 periodic: 1 shown, use --all-periodic) cursor agent:claude-1 -> seq 5021

$ loom can trace --since @last --origin user,device --ids 0x300,0x301
seq   t(s)      ch  dir org  id     dlc data
5644  818.731   ch0 tx  USR  0x300  1   00          (user:tui)
5645  818.736   ch0 rx  DEV  0x301  1   00          (sim:BCM)
-- 2 rows; NOTE: user sent 1 frame since your last read
```
마지막 줄의 `NOTE` 처럼 **사용자 개입이 있었으면 CLI가 요약 한 줄로 알려준다** (에이전트가 사용자 행동을 놓치지 않게). 스킬은 이 NOTE를 보면 사용자에게 확인하라고 지시한다.

사용자 TUI 화면 (T3 직후, Chronological 모드):
```text
┌ ch0 virtual bench-sim  load 3%  REC s1  [Chrono] clients: agent:claude-1, user:tui ── LIVE  lag 40ms ┐
│ 812.004  tx AGT  0x300  1  01                       agent:claude-1                                   │
│ 812.009  rx DEV  0x301  1  01                       sim:BCM                                          │
│ 812.050  rx DEV  0x310  8  01 00 00 00 00 00 00 00                                                   │
│ ...                                                                                                  │
│ 818.731  tx USR  0x300  1  00                       user:tui                                         │
│ 818.736  rx DEV  0x301  1  00                       sim:BCM                                          │
├──────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ activity: 818.90 agent:claude-1 read ch0 trace (2 rows, origin=user,device)                          │
│ :send 0x300#00                                                         [Enter] send  [Esc] cancel     │
└ F fixed/chrono  / filter  : command  Space pause  Ctrl-X STOP ALL TX  q quit ─────────────────────────┘
```
같은 흐름이 실HW(M2 PCAN, M4 Vector)에서는 BCM(sim) 대신 실 ECU로 바뀔 뿐 명령/화면은 동일하다. 신호 단위(M2)에서는 `loom can send --msg Light_Req --set HeadLamp=On` / Trace 행 펼침에 `HeadLamp=On` 표시.

### 2.1 에이전트 관점

**A1. "차속 신호가 100 넘으면 알려줘" — 연결 확인 → DB → 관찰 → 대기**
```text
$ loom can status
daemon  : running (pid 41233, ws://127.0.0.1:7341, uptime 2h13m)
channel ch0  pcan:PCAN_USBBUS1  500k/2M FD  env=bench  tx=confirm  load 23.4%  err 0
channel ch1  virtual:sim1       500k        env=sim    tx=allow    load  4.1%  err 0
db      ch0: powertrain.dbc (142 msgs, 1381 sigs)

$ loom can sig get VehicleSpeed --channel ch0
VehicleSpeed = 37.25 km/h   (msg ESP_01 0x0FD, age 18 ms, t=7712.418)

$ loom can sig wait-for "VehicleSpeed > 100" --channel ch0 --timeout 120
MATCH VehicleSpeed = 100.75 km/h at t=7801.022 (waited 88.6 s)
exit 0
```
타임아웃이면 `TIMEOUT after 120 s; last VehicleSpeed = 87.0 km/h (max 97.5)` + exit 3.

**A2. "IG처럼 엔진 회전수를 3000으로 바꿔서 주기 송신해줘" — 신호 단위 송신**
```text
$ loom can periodic start EngineData --period 10ms --set EngRPM=3000 --set EngTemp=90 --channel ch1 --dry-run
DRY-RUN  risk=R1 (sim channel)  would start periodic id=0x100 EngineData every 10 ms
  data 0B B8 5A 00 00 00 00 00   (EngRPM=3000 rpm, EngTemp=90 degC, others=initial)
  lease 10 min (auto-stop unless renewed)

$ loom can periodic start EngineData --period 10ms --set EngRPM=3000 --set EngTemp=90 --channel ch1
started task p3  EngineData 0x100 @10ms on ch1  lease until 14:42:10

$ loom can sig set EngRPM=4500 --task p3          # 주기 메시지 안의 신호만 변경
p3 EngineData: EngRPM 3000 -> 4500 rpm (next frame)

$ loom can periodic stop p3
```

**A3. "방금 부팅 직후 에러프레임 났는지 보고, 그 구간 트레이스 보여줘" — 통계 + 커서 기반 조회**
```text
$ loom can stats --channel ch0 --window 60s
ch0 last 60 s: frames 41,882 (698/s)  load avg 23.1% peak 41.0%  err-frames 3  bus-off 0  state ERROR_ACTIVE
top ids: 0x0FD 6000  0x1A0 6000  0x3C0 1200 ...
errors at: t=7690.112, 7690.114, 7690.118

$ loom can trace --channel ch0 --around 7690.114 --window 20ms --decode
t           ch  dir  id     dlc data                     decoded
7690.104    ch0 rx   0x0FD  8   01 3A ...                ESP_01 VehicleSpeed=0.0 ...
7690.112    ch0 rx   ERR    -   -                        ErrorFrame (stuff error?) 
...
(14 lines)
```

**A4. "새로 온 것만" — 에이전트 커서**
```text
$ loom can trace --since @last --id 0x7E8 --max 50
(cursor agent:default advanced 1840221 -> 1840297, 3 lines shown, 0 dropped)
```
에이전트는 반복 폴링해도 같은 프레임을 다시 읽지 않는다.

**A5. "이 로그 파일에서 냉각수 온도 추이 그래프 띄워줘" — viz**
```text
$ loom viz show drive_0412.blf --db powertrain.dbc --y CoolantTemp,EngRPM
opened view v7 "drive_0412.blf" in viewer: http://127.0.0.1:7341/v/v7
  1.84 M frames, 2 signals, 0..1823.4 s (served with MinMaxLTTB, 2000 px)
```
에이전트는 수치 덤프 대신 요약(`loom viz summary v7 --json`: min/max/mean/구간)을 받는다.

**A6. UDS 팀 연계 (ISO-TP)** — UDS CLI가 `loom uds connect --can ch0 --tx 0x7E0 --rx 0x7E8` 하면 내부적으로 CAN 데몬의 `isotp.open`을 호출. CAN 트레이스에는 해당 프레임이 요청 주체 origin(agent/user) + `via=uds` 태그로 보이고 TUI/웹 Trace에 ISO-TP 세그먼트가 묶여 표시된다.

### 2.2 사용자 관점

**U1. CANoe 사용자가 처음 여는 화면** — `loom can monitor` (TUI, herdr 패인) 또는 브라우저. 기본 레이아웃 = 왼쪽 Trace(고정 모드: ID별 1행, 최신 값 덮어쓰기), 오른쪽 위 Graphics, 오른쪽 아래 Data. CANoe의 "Fixed/Chronological" 토글을 `F` 키 / 툴바 버튼으로 재현.

**U2. 에이전트가 뭘 보내는지 감시하고 막기** — 에이전트가 `tx=confirm` 채널에 송신을 시도하면 웹 뷰어 상단에 승인 배너(무엇을/어느 채널/주기/위험등급/dry-run 바이트)가 뜨고, TUI에는 하단 바에 `[A]pprove [D]eny` 표시. 에이전트 송신 프레임은 Trace에서 주황 `AGT` 배지. 상단 빨간 **STOP ALL TX** 버튼(TUI `Ctrl-X`)은 모든 agent/user 주기 송신과 리플레이를 즉시 중지.

**U3. Interactive Generator로 직접 송신** — 웹 IG 패널에서 DBC 메시지 선택 → 신호별 슬라이더/드롭다운(값 테이블) → 주기 체크 → Start. 에이전트가 같은 메시지를 이미 주기 송신 중이면 "p3 (agent) 가 이미 송신 중 — 신호만 덮어쓰기/새 태스크" 선택지 제시.

**U4. 로그 재생** — Replay 패널에 BLF 드래그 → 타임라인 스크러빙 → "뷰어로만 재생(버스 송신 안 함)"이 기본, "버스로 송신"은 채널 정책 확인 + 승인 필요.

**U5. UART 로그 수치 플롯** — UART 팀 요구: `loom viz show uart:COM3 --extract 'temp=(\d+\.\d+)' --live` → 라이브 시계열. CAN 신호와 같은 Graphics 창에 시간 동기화해 겹쳐 그릴 수 있다(공통 `ts`).

**U6. Statistics** — 채널별 버스 부하 그래프, ID별 주기 평균/지터/최소/최대 표, DBC 기대 주기 대비 이탈(±20%) 하이라이트, 에러 카운터(TEC/REC는 드라이버 지원 시).

---

## 3. 아키텍처

### 3.1 결론
- **단일 상주 프로세스 `loomd`** 가 도메인 서비스(UART/CAN/UDS/VIZ)를 호스팅하고, **하나의 포트에서 JSON-RPC 2.0 over WebSocket** 을 노출한다. 웹 뷰어 정적 파일도 같은 포트에서 서빙.
- CAN 서비스 내부는 **채널당 RX 스레드 1개 → 링버퍼(seq 부여) → 이벤트 버스 → 구독자(레코더, 디코더, 통계, ISO-TP, WS 브로드캐스터)** 파이프라인.
- **ISO-TP는 CAN 서비스 안에서** python-can `Notifier` 에 붙는 `NotifierBasedCanStack` 으로 돈다. UDS 서비스가 같은 프로세스에 있으면 in-process 호출, 아니면 RPC.
- 핫패스(RX/ISO-TP/주기 TX)는 asyncio 루프와 분리된 스레드. asyncio 루프는 IPC·구독·쿼리 담당.
- 도메인 서비스는 "같은 프로세스 or 자식 프로세스" 둘 다 가능한 인터페이스로 설계 → 측정 결과 GIL 경합이 문제면 CAN 서비스만 자식 프로세스(`loomd --service can`)로 분리.

### 3.2 프로세스 구성도
```text
 ┌──────────── 사용자 ────────────┐      ┌──────────── 에이전트 ─────────────┐
 │ Browser (웹 뷰어 SPA)          │      │ Claude Code / Codex  (Bash)        │
 │ herdr 패인: loom can monitor   │      │   └─ loom can ... / loom viz ...   │
 │ herdr 패인: loom uart attach   │      │      (단발 CLI 프로세스, 비대화형) │
 └──────┬──────────────┬──────────┘      └─────────────┬─────────────────────┘
        │ WS (JSON+bin)│ WS                            │ WS (JSON-RPC, 요청/응답만)
        ▼              ▼                               ▼
 ┌──────────────────────────────── loomd (127.0.0.1:7341) ───────────────────────────┐
 │  Gateway: HTTP(static) + WS JSON-RPC router + auth(token) + client registry       │
 │  Core: SessionManager · Policy/Approval · AuditLog · Cursor store · Clock          │
 │  ┌──────────── CAN service ─────────────┐ ┌─ UDS service ─┐ ┌─ UART svc ─┐ ┌ VIZ ┐ │
 │  │ ChannelManager                       │ │ udsoncan      │ │ pyserial   │ │data │ │
 │  │  ├ ch0: Bus(pcan) ─ RX thread        │ │ ↕ in-proc API │ │            │ │sets │ │
 │  │  ├ ch1: Bus(virtual) ─ RX thread     │ │               │ │            │ │LTTB │ │
 │  │ EventBus (seq, ring buffer/채널)     │◄┤ IsoTpService  │ │            │ │     │ │
 │  │ Decoder(DB) · Stats · Recorder(BLF)  │ └───────────────┘ └────────────┘ └─────┘ │
 │  │ TxScheduler(periodic/IG/replay)      │                                         │
 │  │ IsoTp hosts (can-isotp)              │                                         │
 │  │ SimNodes (가짜 ECU)                  │                                         │
 │  └──────────────────────────────────────┘                                         │
 └──────────────────┬───────────────────────────────────────────────────────────────┘
                    ▼ python-can drivers
        PCAN-Basic / PCBUSB · Vector XL Driver · SocketCAN · virtual
```

### 3.3 데이터 흐름 (RX)
```text
 Driver ──recv()──► RX thread(ch) ──► Frame{seq, ts_hw, ts_mono, ts_wall, ch, dir, id, flags, data, origin}
                                         │
                                         ▼
                               RingBuffer[ch] (기본 2M 프레임 or 512MB)  ◄── 커서 조회(trace --since)
                                         │ fan-out (lock-free-ish SPMC 큐, 구독자별 drop 카운트)
         ┌───────────────┬───────────────┼──────────────────┬───────────────────┬──────────────┐
         ▼               ▼               ▼                  ▼                   ▼              ▼
     Recorder        Decoder          Stats             IsoTp stacks        SignalCache     WS Broadcaster
  (BLF writer,     (cantools, 지연    (1s 윈도우 부하,   (python-can          (sig 최신값,    (20~50ms 배치,
   10s flush)       평가·캐시)         주기/지터)         Notifier 경유)       wait-for 트리거) 구독 필터별)
```
- **Decoder는 지연 평가**: 모든 프레임을 즉시 디코드하지 않고, (1) SignalCache 대상(구독/wait-for/IG 중 신호가 포함된 메시지), (2) 조회 시점에 디코드. 수천 fps에서 cantools 디코드 비용 회피. 실제 비용은 Sol 측정 필요(미확인).
- **WS 브로드캐스트는 배치**: 클라이언트별 필터(채널, ID 집합, 신호 집합) 적용 후 20~50 ms 단위로 묶어 바이너리(msgpack) 프레임 전송. 느린 클라이언트는 큐 상한 초과 시 오래된 것부터 drop 하고 `dropped` 카운트 통지 (데몬/버스는 절대 막히지 않음).

### 3.4 데이터 흐름 (TX)
```text
 CLI/뷰어 ──RPC can.send / can.periodic.start / can.sig.set──► Gateway
     ──► Policy.check(client, channel, id, rate, risk)
           ├─ deny  → 오류 code=POLICY_DENIED (exit 4)
           ├─ confirm → Approval 대기 (뷰어로 요청 브로드캐스트, 응답까지 RPC 보류 or pending id 반환, exit 5)
           └─ allow → TxScheduler
 TxScheduler ──► RateLimiter(채널/클라이언트 토큰 버킷) ──► bus.send()
     └─► 송신한 프레임은 RX와 같은 EventBus에 dir=tx, origin=<agent|user|device(sim 노드)|replay>, via=<uds|isotp|periodic>, corr=<rpc id> 로 주입 (§3.10)
         (드라이버의 own-message 에코가 오면 seq 매칭으로 HW 타임스탬프만 갱신)
```
- **주기 송신**: socketcan은 BCM(커널) 사용, vector/pcan은 python-can 소프트웨어 태스크 대신 **TxScheduler 자체 스레드(단일 타이머 휠)** 로 모든 주기 태스크를 처리 — 태스크 수가 많아도 스레드 1개, 신호 변경은 다음 틱에 원자적 반영. (python-can `send_periodic`은 태스크당 스레드. 타이밍 품질 비교는 Sol 스파이크 대상)
- Windows: Python ≥3.11 `time.sleep` 고해상도 타이머 사용(기억 기반, **미확인**) → 10 ms 주기 지터 목표 ±1 ms. 실측 필요.

### 3.5 이벤트 버스 / 시퀀스 / 커서
- `seq`: 데몬 전역 단조 증가 64-bit 정수. 모든 도메인 이벤트 공통 (UART 바이트 청크, UDS 요청/응답, CAN 프레임). → 도메인 간 정렬, 커서, 손실 감지 모두 seq 하나로.
- 커서: `(client_name, topic) → last_seq` 를 데몬이 보관. `--since @last` 는 서버측 커서 사용 후 전진. `--since seq:N`, `--since -5s`, `--since 14:03:22.100` 지원. 링버퍼에서 이미 밀려난 구간이면 `gap: frames N..M evicted (recorded in session file)` 경고 + 레코드 파일 경로 안내.
- client_name: CLI는 `LOOM_CLIENT` 환경변수(기본 `agent:default`), 뷰어는 `user:<탭 id>`.

### 3.6 기록 파이프라인
```text
 EventBus ──► Recorder(ch별) ──► sessions/2026-11-09T14-02-11_s12/
                                     ├ can_ch0.blf         (python-can BLFWriter, zlib, 10 s 또는 4 MB마다 flush)
                                     ├ can_ch1.blf
                                     ├ events.jsonl        (tx origin/corr, 승인, 정책, 주석, 채널 설정 변경, 에러/상태)
                                     ├ session.json        (채널 설정, DB 해시/경로, 시계 기준점, loom 버전)
                                     └ audit.jsonl → 전역 audit에도 복제
```
상세는 §6.

### 3.7 클라이언트 연결 방식
| 클라이언트 | 연결 | 수명 | 사용 메서드 |
|---|---|---|---|
| `loom` CLI (에이전트/사람) | WS 1회 연결 → RPC 1~N회 → 종료 | 단발 | 요청/응답만. 대기형(`wait-for`, `trace --follow --timeout`)도 서버측에서 경계 내 완료 후 1응답 |
| `loom can monitor` (Textual TUI) | WS 상주, `subscribe` | 상주 | 이벤트 스트림 + 승인 응답 |
| 웹 뷰어 | 같은 오리진 HTTP → WS 상주 | 상주 | 이벤트 스트림, viz 데이터 질의, IG 송신, 승인 |
| UDS 서비스 | in-process Python API (같은 loomd) | 상주 | IsoTpService |
| 외부 스크립트/노트북 | `loom.client` Python SDK (WS) | 임의 | 전부 |

데몬 자동 기동: CLI가 연결 실패 시 `loomd` 를 백그라운드로 띄우고(`loom daemon start` 동일), discovery 파일을 기다린다. 데몬은 장치를 **자동으로 열지 않는다** — `loom.toml` 의 `autostart=true` 채널만 연다.

### 3.8 도메인 데몬 통합 vs 분리 — 의견
**통합(loomd 하나) 권장.** 이유: (1) 사용자에게 포트/URL 하나, 승인 UI 하나, 감사 로그 하나. (2) seq/시계 공통으로 UART-CAN-UDS 이벤트를 한 타임라인에 정렬. (3) UDS-on-CAN이 ISO-TP를 in-process로 호출 가능 → IPC 지연·직렬화 제거. (4) viz가 모든 도메인 라이브 데이터에 직접 접근.
**리스크와 완화:** 한 도메인 크래시가 전체를 죽임 → 서비스별 예외 격리 + 서비스 재시작, 그래도 불안하면 `--service can` 자식 프로세스 모드(같은 RPC 스키마, 게이트웨이가 프록시). GIL → Python 3.13+/3.14 free-threaded 빌드 검토(라이브러리 호환 미확인), 우선은 핫패스가 대부분 C 드라이버 호출·I/O 대기라 실측 후 판단.

### 3.9 ISO-TP 호스팅 위치
```text
 UDS service (udsoncan Client) ──Connection 어댑터──► IsoTpService.open(ch0, tx=0x7E0, rx=0x7E8, params)
                                                          │
                                         isotp.NotifierBasedCanStack(bus=ch0.bus, notifier=ch0.notifier)
                                                          │   (can-isotp 자체 스레드, FC 즉시 응답)
                                                          ▼
                                                      ch0 Bus  ── 프레임은 EventBus에도 origin=<요청 주체>, via=uds, isotp_ch=t1 태그
```
- FC/CF 타이밍은 전부 데몬 프로세스 내 스레드에서 처리 → IPC 지연 무관.
- 같은 채널의 ch0 RX 스레드와 ISO-TP 수신 경로: python-can `Notifier` 하나가 bus.recv를 독점하고 listeners(EventBus 주입기, isotp stack들)에 분배. **따라서 RX 스레드 = Notifier 스레드**로 통일한다.
- 세부 인터페이스는 §15.2.

### 3.10 출처(origin) 판정
| 경로 | origin | 근거 |
|---|---|---|
| 드라이버 수신 (실 ECU) | `device` | |
| SimNode(가짜 ECU) 송신 | `device` (+ `sim_node`) | 실 ECU와 같은 표시·코드 경로. 채널 env=sim 배지로 구분 |
| `loom` CLI | `agent` (기본) | `LOOM_CLIENT=agent:<이름>` 로 주체 구분 |
| TUI `:send`, 웹 IG | `user` | 사람 UI에서만 생성 |
| `loom can send --as-user` (사용자가 자기 셸에서) | `user` | 로컬 단일 사용자 신뢰 모델. 단 승인 권한은 여전히 뷰어 전용 (§10) |
| UDS 서비스 경유 ISO-TP | `agent`/`user` (요청 주체 승계) + `via=uds` | 공통 레코드 origin 3종 유지, 경로는 별도 필드 |
| 리플레이 | `replay` | |

### 3.11 사용자 화면이 에이전트 세션과 분리되지 않음 (가시성 불변식)
설계상 강제하는 규칙. 하나라도 깨지면 버그로 취급하고 테스트(§9)로 고정한다.
- **I1 단일 경로:** 장치 읽기/쓰기는 loomd 하나만 한다. CLI·SDK·웹·UDS 서비스 모두 loomd 클라이언트. 에이전트가 python-can으로 장치를 직접 여는 스크립트를 쓰는 것은 스킬에서 금지(§15.5). (Vector/PCAN은 같은 프로세스 밖에서 열면 공존 문제가 생기므로 기술적으로도 불리)
- **I2 비공개 토픽 없음:** RPC에 "private/hidden" 옵션이 존재하지 않는다. 구독 필터는 같은 EventBus를 걸러 보는 뷰일 뿐. 에이전트 송신, 사용자 송신, ISO-TP FF/CF/FC, 주기 태스크, 시뮬 노드 프레임 모두 **송신 시점에 EventBus로 주입**된 뒤에야 bus.send 결과가 RPC 응답으로 돌아간다.
- **I3 늦게 열어도 같은 화면:** 구독 시작 시 서버가 (a) 최근 backlog(기본 30 s 또는 10k 프레임, 설정 가능), (b) Fixed 모드 스냅샷(ID별 최신 프레임+카운트+주기), (c) 활성 주기 태스크/소유자, (d) 대기 중 승인, (e) 최근 Activity 를 먼저 보내고 라이브로 이어간다. 에이전트가 1시간 전부터 버스를 쓰고 있었어도 사용자는 열자마자 현재 상태를 본다. 더 과거는 세션 기록에서 스크롤백.
- **I4 에이전트의 "읽기"도 보인다:** 에이전트의 질의(trace/sig get/wait-for/stats)는 Activity 이벤트(`agent:claude-1 waiting VehicleSpeed > 100 (timeout 120 s)`)로 뷰어에 표시. 사용자는 에이전트가 무엇을 보고 무엇을 기다리는지 안다.
- **I5 뷰어 존재 알림:** `loom can status` 와 모든 송신 결과에 `viewers: 0` 이면 `hint: user is not watching — ask them to run 'loom can monitor' or open http://127.0.0.1:7341/` 를 붙인다. 스킬은 세션 첫 송신 전 이 힌트를 사용자에게 전달. (`loom.toml` `viewer.auto_open = true` 면 첫 에이전트 연결 시 데몬이 브라우저를 연다 — 기본 false)
- **I6 사용자 개입이 에이전트에 보인다:** 에이전트 커서 이후 user origin 이벤트가 있으면 CLI 출력에 `NOTE: user sent N frames / changed signal X / stopped task p3` 요약 줄 (§2.0).

### 3.12 실시간성 목표와 고부하 동작
**목표치** (로컬 루프백, M1은 virtual 기준, M2부터 실HW에서 측정. 측정법: 이벤트 `ts_mono` 와 클라이언트 렌더 직후 시각의 차이 — 웹은 데몬 시계 오프셋을 WS ping으로 추정)

| 경로 | 목표 (p95) | 상한 (p99) | 비고 |
|---|---|---|---|
| 장비 프레임 → 웹 Trace 행 표시 | < 150 ms | < 200 ms | 배치 50 ms + rAF 렌더 |
| 장비 프레임 → TUI Trace 행 표시 | < 200 ms | < 300 ms | TUI 화면 갱신 10 Hz |
| 신호 값 → Data 창 / Graphics 갱신 | < 200 ms | < 300 ms | Graphics 라이브 10~20 Hz |
| 에이전트 `send` → 버스 송신 | < 10 ms | < 30 ms | CLI 프로세스 기동 시간 제외 (기동은 별도 목표 < 300 ms) |
| 버스 프레임 → 에이전트 `trace --since @last` 에 보임 | < 10 ms | | 링버퍼 삽입 직후 |
| 프레임 → `wait-for` 반환 | < 20 ms | | SignalCache 트리거 |
| 승인 요청 → 뷰어 배너 | < 200 ms | | |
| STOP ALL TX → 마지막 agent 프레임 | < 50 ms | | 타이머 휠 즉시 해제 |

**고부하에서 "멈추지 않고 거짓말하지 않기"** — 버스 부하 100% 기준 처리량: Classic 500 kbit/s 8바이트 프레임 ≈ 3.7~4.5k fps, CAN FD(2~5 Mbit/s 데이터상) 수천~2만 fps 추정(정확 수치 Sol 계산).
1. **기록은 무손실, 화면은 예산제.** RX → 링버퍼 → 레코더 경로는 화면/클라이언트와 독립(별 큐). 화면용 경로만 클라이언트별 예산(웹 Chrono Trace 기본 2,000 행/s, TUI 300 행/s)을 둔다.
2. **생략은 반드시 표시.** 예산 초과분은 버리되 그 자리에 합성 행 `⋯ 3,812 frames skipped (0x0FD×1200, 0x1A0×1200, …) — recording intact` 을 넣는다. 헤더에 누적 `view-skipped 1.2M` 카운터.
3. **Fixed 모드는 고부하에서도 정확.** ID 수(보통 수백)에 비례하므로 모든 프레임을 반영(최신값·카운트·주기). 고부하 기본 모드를 Fixed로 자동 전환 제안 배너.
4. **플롯 다운샘플링 명시.** Graphics는 픽셀 버킷 min/max(MinMaxLTTB) — 스파이크 보존. 축 라벨 옆에 `decimated 1:40` 표시, 확대하면 원본까지 내려감.
5. **지연 정직 표시.** 헤더 `LIVE lag 40 ms`; 목표 초과 시 노란 `LAG 620 ms`, 2 s 초과 시 빨강 + 서버가 해당 클라이언트 backlog를 버리고 최신으로 점프(`⋯ jumped 2.3 s to live` 행).
6. **진짜 손실은 구분.** 드라이버 수신 큐 오버런/RX 스레드 지연으로 **기록에서도** 빠진 경우(유일한 진짜 손실)는 빨간 `DRIVER OVERRUN n` 배지 + `events.jsonl` 기록 + 에이전트 출력 `WARNING: data loss on ch0 (n frames, driver overrun)`. 레코더 큐가 밀리면 메모리 버퍼(기본 512 MB)로 흡수, 그래도 넘치면 동일하게 손실로 표기 — 조용히 빠지는 경로는 없다.
7. **일시정지는 수집을 멈추지 않는다.** Pause는 화면만 고정, 재개 시 `⋯ paused 12.4 s (51k frames)` 표시 후 라이브.
8. **에이전트 출력도 같은 규칙.** `--max` 초과 시 `-- 200 of 41,882 (truncated)` 를 항상 표시, 무음 절단 없음.

---

## 4. CLI / API 명세 초안

### 4.1 공통 규칙 (에이전트 출력 계약)
| 규칙 | 값 |
|---|---|
| 기본 출력 | 사람용 정렬 텍스트. 마지막 줄에 요약/커서/잘림 정보 |
| `--json` | 단일 JSON 객체(`{"ok":true,"data":..., "meta":{"truncated":..,"next_cursor":..}}`). 스트림형은 `--jsonl` |
| 출력 상한 | 기본 `--max 200` 행, 하드 상한 2000행 / 64 KB. 넘으면 앞쪽 잘라 `... 1,843 more (use --since seq:N or narrow filter)` |
| 무한 스트림 | 금지. `--follow` 는 `--timeout`(기본 10 s, 최대 600 s) 또는 `--count N` 필수 |
| 시간 지정 | `--since @last|seq:N|-5s|HH:MM:SS.mmm`, `--until`, `--around T --window 20ms` |
| 채널 | `--channel ch0` (생략 시 `loom.toml` default_channel, 1개뿐이면 자동) |
| 종료 코드 | 0 ok, 1 일반 오류, 2 사용법, 3 timeout/no-match, 4 정책 거부, 5 승인 대기/거부, 6 데몬 없음/연결 실패, 7 장치 오류(bus-off 등) |
| 식별 | `LOOM_CLIENT=agent:claude-1` 로 커서·감사 로그 주체 지정 |

### 4.2 `loom daemon`
```text
loom daemon start [--foreground] [--port 7341]
loom daemon stop | status | logs --tail 50
```

### 4.3 `loom can`
| 명령 | 설명 | 위험 |
|---|---|---|
| `loom can status [--json]` | 데몬/채널/DB/부하/정책 요약 | R0 |
| `loom can hw list` | python-can `detect_available_configs()` + 드라이버 정보 | R0 |
| `loom can open ch0 --iface pcan --hw PCAN_USBBUS1 --bitrate 500k [--fd --dbitrate 2M] [--listen-only] [--env bench] [--tx confirm]` | 채널 열기 (정책 오버라이드는 더 엄격한 방향만 허용) | R0 (listen-only) / R2(init access 요청) |
| `loom can close ch0` | 채널 닫기 (주기 태스크 중지) | R0 |
| `loom can db load powertrain.dbc --channel ch0` / `db list` / `db msg ESP_01` / `db sig VehicleSpeed` / `db find speed` | DB 로드·탐색 | R0 |
| `loom can trace [--channel] [--id 0x100,0x7E0-0x7EF] [--msg ESP_01] [--dir rx|tx] [--origin agent] [--since ..] [--max N] [--decode] [--raw] [--follow --timeout 10s]` | 트레이스 조회 | R0 |
| `loom can last [--id|--msg]` | ID/메시지별 최신 프레임(Fixed 모드 스냅샷) | R0 |
| `loom can sig get SIG[,SIG..] [--channel]` | 신호 최신값 + age | R0 |
| `loom can sig wait-for "EXPR" --timeout 30s [--channel]` | 조건 대기. EXPR: `Sig > 10`, `Sig == "Drive"`(값테이블), `changed(Sig)`, `A > 1 and B < 3` | R0 |
| `loom can sig series SIG --since -10s [--every 100ms]` | 짧은 시계열(상한 적용), 길면 viz 권고 | R0 |
| `loom can send 0x123#0102 [--ext] [--fd --brs] [--count N --gap 10ms] [--dry-run]` | 원시 1회/연속 송신 | R2~R4 |
| `loom can send --msg EngineData --set EngRPM=3000 [--dry-run]` | DBC 인코딩 1회 송신 (미지정 신호 = 초기값/최근 수신값 선택 `--base last|initial|zero`) | R2~R4 |
| `loom can periodic start MSG|ID#DATA --period 10ms [--set ..] [--lease 10m] [--dry-run]` | 주기 송신 → task id | R3~R4 |
| `loom can periodic list | stop ID | stop --all [--mine] | renew ID` | | R0 |
| `loom can sig set SIG=VAL [--task ID | --msg MSG]` | 주기 태스크 내 신호만 변경(IG) | R3 |
| `loom can stats [--window 10s] [--by id]` | 부하, fps, 에러, ID별 주기/지터 | R0 |
| `loom can record start|stop|status|list` / `record mark "text"` | 수동 구간 기록(자동 세션 기록과 별개로 이름 붙은 클립) | R0 |
| `loom can export SESSION|CLIP --to out.blf|.asc|.mf4|.csv|.parquet [--channel] [--range]` | 내보내기 | R0 |
| `loom can replay FILE --to viewer` / `--to ch1 [--speed 1.0] [--map 1:ch1]` | 재생 (버스 송신은 R3~R4) | R0/R3+ |
| `loom can node start ecu_sim --db x.dbc --node EMS --channel ch1` / `node list|stop` | 가짜 ECU | R1 (sim 채널만) |
| `loom can isotp open|send|recv|close` | ISO-TP 디버그용 (UDS 팀 기본 경로 아님) | R2+ |
| `loom can stop-all` | 모든 주기·리플레이·노드 TX 즉시 중지 (누구 것이든) | R0 (항상 허용) |

**출력 예시 — `trace`**
```text
$ loom can trace --channel ch0 --msg ESP_01,EngineData --max 4 --decode
seq      t(s)        ch  dir org  id     dlc  data                     decoded
1840201  7712.402    ch0 rx  dev  0x0FD  8    10 0E 00 00 3F 00 00 00  ESP_01: VehicleSpeed=37.25 km/h BrakeSw=Off
1840202  7712.404    ch0 tx  AGT  0x100  8    0B B8 5A 00 00 00 00 00  EngineData: EngRPM=3000 EngTemp=90
1840227  7712.412    ch0 rx  dev  0x0FD  8    10 0E 00 00 3F 00 00 00  ESP_01: VehicleSpeed=37.25 km/h BrakeSw=Off
1840229  7712.414    ch0 tx  AGT  0x100  8    0B B8 5A 00 00 00 00 00  EngineData: EngRPM=3000 EngTemp=90
-- 4 of 912 matching (truncated; next: --since seq:1840229)
```
**`--json` 예시**
```json
{"ok":true,
 "data":[{"seq":1840201,"t":7712.402,"wall":"2026-11-09T14:12:31.402+09:00","ch":"ch0","dir":"rx","origin":"device",
          "id":253,"ext":false,"fd":false,"brs":false,"dlc":8,"data":"100E00003F000000",
          "msg":"ESP_01","sig":{"VehicleSpeed":37.25,"BrakeSw":"Off"}}],
 "meta":{"matched":912,"returned":4,"truncated":true,"next_cursor":"seq:1840229","dropped":0}}
```

### 4.4 `loom viz`
| 명령 | 설명 |
|---|---|
| `loom viz show SRC [--x COL] [--y A,B] [--db X.dbc] [--kind auto|line|step|scatter|trace|table|state] [--title] [--layout file.json] [--group panel1:A,B panel2:C]` | 데이터셋 등록 + 뷰 생성 + 브라우저 열기(`--no-open`이면 URL만) |
| SRC 형식 | 파일 경로(csv/jsonl/parquet/blf/asc/mf4/trc), `-`(stdin JSONL/CSV), `live:can:ch0` (`--y` 신호), `live:uart:COM3 --extract REGEX`, `live:uds:did:F40D`, `session:s12` |
| `loom viz push VIEW --jsonl -` | 에이전트가 계산한 결과를 기존 뷰에 추가(짧은 배치) |
| `loom viz list` / `close VIEW` / `summary VIEW [--json]` / `snapshot VIEW --png out.png` | 관리, 요약 통계(에이전트용), PNG 스냅샷(브라우저 헤드리스 없이 서버측 렌더는 미확인 → M3 이후, 대안: matplotlib 폴백) |
| `loom viz open [VIEW]` | 뷰어 열기/포커스 |

자동 렌더 규칙(`--kind auto`):
1. CAN 프레임 형태(id,data 컬럼 또는 BLF/ASC) → Trace + (DB 있으면) 신호 선택 패널.
2. 시간형 컬럼(`ts`,`time`,`timestamp`, datetime, 단조 증가 수치) → x축. 없으면 행 인덱스.
3. 수치 컬럼 → line(정수 & 고유값 ≤16 이면 step), 문자열·enum 컬럼 고유값 ≤32 → State 타임라인, 그 외 문자열 → 테이블.
4. `--y` 미지정 + 수치 컬럼 >8 → 상위 8개(분산 기준) 표시 + 나머지는 체크박스.

### 4.5 데몬 RPC (JSON-RPC 2.0 over WebSocket)
엔드포인트 `ws://127.0.0.1:7341/rpc`, 첫 메시지 `auth{token, client}` 필수. 메서드 이름은 `<domain>.<noun>.<verb>`.

| 메서드 | params | result |
|---|---|---|
| `core.hello` | `{client, version}` | `{server_version, session_id, capabilities}` |
| `core.subscribe` | `{topics:["can.frame/ch0","can.sig/ch0","core.approval","viz.view/v7"], filter:{ids:[...], sigs:[...]}, format:"msgpack"}` | `{sub_id}` → 이후 notification `event` (배치) |
| `core.unsubscribe` | `{sub_id}` | |
| `core.approval.respond` | `{approval_id, decision:"approve"|"deny", scope:"once"|"session"}` | user origin 클라이언트만 허용 |
| `can.channel.open/close/list` | | |
| `can.db.load/describe/find` | | |
| `can.trace.query` | `{channel, filter, since, until, max, decode}` | `{frames, meta}` |
| `can.sig.get` / `can.sig.wait` | `{expr, timeout_ms}` | wait는 서버측 대기 후 응답 |
| `can.tx.send` | `{channel, frame|msg+signals, count, gap_ms, dry_run}` | `{sent, seqs}` 또는 `{pending_approval}` |
| `can.tx.periodic.start/update/stop/list/renew` | | |
| `can.stats.get` | | |
| `can.record.start/stop/list/export` | | |
| `can.replay.start/stop` | | |
| `can.node.start/stop/list` | | |
| `can.isotp.open/send/recv/close` | §15.2 | |
| `viz.dataset.register/query/summary`, `viz.view.create/update/close` | `query{dataset, cols, t0, t1, px}` → 다운샘플된 열 데이터 | |

이벤트 notification(배치):
```json
{"jsonrpc":"2.0","method":"event","params":{"sub":"s3","topic":"can.frame/ch0","seq0":1840201,"seq1":1840290,"dropped":0,
 "items":"<msgpack bytes, 바이너리 WS 프레임으로 별도 전송>"}}
```
바이너리 프레임 형식: `[u32 header_len][msgpack header][column blocks]` — 고빈도 CAN은 열 지향(ts f64[], id u32[], flags u8[], dlc u8[], data bytes) 블록으로 보내 웹에서 TypedArray로 바로 사용.

---

## 5. 사용자 뷰어 UX

### 5.1 비교와 결정
| 기준 | 웹 (loomd 내장 HTTP/WS + SPA, uPlot) | 데스크톱 (PySide6 + pyqtgraph) | TUI (Textual) |
|---|---|---|---|
| 플롯 성능 | uPlot: 수십만 점 Canvas 렌더 빠름(공개 벤치 기준, 실측 미확인) + 서버측 LTTB | 최상 (OpenGL 옵션) | 낮음 (문자 그래프만) |
| herdr 패인 | ✗ | ✗ | ✓ |
| Windows 벤치 ↔ macOS 원격 시청 | ✓ (SSH 포트포워드 하나) | ✗ (원격 데스크톱 필요) | ✓ (ssh) |
| 동시 다중 뷰어 | ✓ | 1개 프로세스=1창 | ✓ |
| 에이전트가 "띄워주기" | URL 열기(`open`/`start`) | 프로세스 실행, GUI 세션 필요 | 사용자가 패인에서 실행 |
| 도메인 공통(UART/UDS 재사용) | ✓ 같은 SPA에 패널 추가 | 가능하나 Qt 학습/배포 부담 | 부분 |
| 배포 | 빌드 산출물(js/css)을 wheel에 포함 → 사용자에 node 불필요 | PySide6 ~수백 MB, LGPL 동적 링크 | 가벼움 |
| 개발 비용 | 프런트엔드 스택 필요 (TS) | Python만 | Python만 |

**결정:** **웹 뷰어를 주 UI**, **Textual TUI를 herdr용 최소 UI**(Trace + 승인 + STOP + 간단 Data)로. 데스크톱은 하지 않는다. 결정적 근거: (1) 실HW가 Windows 벤치일 가능성이 높고 사용자는 macOS → 원격 시청이 웹이면 공짜. (2) UART/UDS 팀이 같은 뷰어를 재사용(브리프 §3-7). (3) 에이전트가 URL 한 줄로 사용자에게 보여줄 수 있음.
프런트 스택 제안: TypeScript + Vite, UI는 경량(Preact 또는 Svelte — 둘 다 MIT), 플롯 uPlot(MIT), 트레이스 표는 자체 가상 스크롤(Canvas 또는 DOM 재활용). 정확한 버전/번들 크기는 미확인(Sol/구현 시 확정).

### 5.2 웹 뷰어 레이아웃 (CANoe 데스크톱 개념 재현)
"Desktop" = 탭, 각 탭 안에 도킹 가능한 패널 그리드. 레이아웃은 `~/.loom/layouts/*.json`(런타임 데이터)에 저장, 에이전트도 `--layout` 으로 지정 가능.
```text
┌ Loom ─ [Desktop: Analysis ▾] [+]  ch0 PCAN 500k/2M ●23%  ch1 sim ●4%   ⏺ REC s12  ⚠ 1 approval  [■ STOP ALL TX] ┐
├───────────────────────────────────────────────┬───────────────────────────────────────────────────────────────────┤
│ TRACE  [Fixed|Chrono] [Sym|Raw] [⏸] filter: id in 0x0FD,0x100 & org!=dev  │ GRAPHICS  [+sig] [cursor1|2] [fit] [sync⟷] │
│ t         ch  dir org  id    name        dlc data              Δt   │  VehicleSpeed ─── km/h     ┊      ┊           │
│ 7712.402  ch0 rx  dev  0x0FD ESP_01        8 10 0E 00 00 3F..  10.0 │  EngRPM       ─── rpm   ╱╲╱┊‾‾‾‾‾┊╲          │
│ ▸ VehicleSpeed 37.25 km/h  BrakeSw Off                              │  GearPos (state) [P][  D     ┊ ][N]           │
│ 7712.404  ch0 tx  AGT  0x100 EngineData    8 0B B8 5A ..       10.0 │  ───────────────────────────────────────────── │
│ 7712.410  ch0 rx  dev  0x7E8 ISO-TP[t1]    8 10 14 62 F1 90 .. ⓘ    │  cursor1 7701.2  cursor2 7712.4  Δ 11.2 s      │
│ ...                                                                 │  VehicleSpeed: 12.0 → 37.25 (Δ +25.25)         │
├───────────────────────────────────────────────┼───────────────────────────────────────────────────────────────────┤
│ DATA (신호 최신값)                            │ INTERACTIVE GENERATOR                                         │
│ name          value     unit  age   src       │ EngineData 0x100  ch1  period [10] ms  [▶ Start] [1x Send]     │
│ VehicleSpeed  37.25     km/h  8ms   ESP_01    │  EngRPM   [====|=====] 3000 rpm                               │
│ EngRPM        3000      rpm   2ms   AGT p3    │  EngTemp  [ 90 ] degC                                         │
│ GearPos       D (4)           40ms  TCU_02    │  Mode     [Normal ▾]  (value table)                           │
│                                               │  bytes: 0B B8 5A 00 00 00 00 00   owner: agent p3 (override?) │
├───────────────────────────────────────────────┴───────────────────────────────────────────────────────────────────┤
│ STATISTICS  load ▁▂▃▅▃▂ 23%  fps 698  err 3  │ id 0x0FD  10.0ms (DBC 10) jitter ±0.3   id 0x3C0 100ms (DBC 50 ⚠) │
└───────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
패널별 CANoe 대응과 핵심 동작:
| 패널 | CANoe 대응 | 동작 |
|---|---|---|
| Trace | Trace window | Fixed(ID별 1행 덮어쓰기, 변화 바이트 하이라이트)/Chronological(스크롤), 일시정지 시 버퍼 탐색, 심볼릭/원시 토글, 행 펼침으로 신호, ISO-TP 세그먼트 묶음, 필터 식(`id`, `msg`, `sig`, `dir`, `org`, `ch`), 출처 배지(DEV/AGT/USR/RPL, sim 노드·via=uds 는 보조 태그), 마크(`record mark`) 표시 |
| Graphics | Graphics window | 다중 신호, Y축 그룹(패널 분할), 커서 2개 + Δ, 마우스 휠 줌/드래그 팬, 라이브/정지, 신호 드래그-드롭(Data/DB 트리에서), 범위 요청시 서버 다운샘플 |
| Data | Data window | 신호 값/단위/age/출처, 값 테이블 텍스트, 범위 초과 경고색 |
| State | State tracker | enum/bool 신호 타임라인 막대 |
| IG | Interactive Generator | DB 메시지 선택, 신호 위젯(슬라이더/숫자/드롭다운), 1회/주기, 소유권 표시(agent 태스크와 충돌 처리) |
| Statistics | Statistics / Bus Statistics | 부하 스파크라인, fps, 에러, ID별 실제 주기 vs DBC 주기 |
| Replay | Replay block | 파일 로드, 타임라인, 속도, 뷰어-전용/버스 송신 |
| Approvals | (없음, Loom 고유) | 대기 요청 목록, dry-run 바이트, 승인 범위(1회/세션) |
| Audit | (없음) | 에이전트 행동 타임라인 |

### 5.3 TUI (`loom can monitor`) — herdr 패인
```text
┌ ch0 pcan 500k/2M  load 23%  err 0  REC s12  [Fixed]  filter: org!=dev ───────────────┐
│ 0x0FD ESP_01      8  10 0E 00 00 3F 00 00 00   10.0ms  VehicleSpeed=37.25            │
│ 0x100 EngineData  8  0B B8 5A 00 00 00 00 00   10.0ms  AGT p3  EngRPM=3000           │
│ 0x7E8 (ISO-TP)    8  10 14 62 F1 90 57 30 4C   --      UDS t1                        │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ ⚠ APPROVAL a17: agent:claude-1 wants periodic 0x2A0 @20ms on ch0 (bench) risk R3      │
│   [A]pprove once  [S]ession  [D]eny                                                   │
└ F fixed/chrono  R raw/sym  / filter  Space pause  Ctrl-X STOP ALL TX  q quit ──────────┘
```
- 처리량: 수천 fps 표시 불가 → 화면 갱신 10 Hz, Fixed 모드 기본, Chrono는 필터 걸린 상태에서만 권장 (필터 없으면 "샘플링 표시중" 경고).
- `loom can monitor --follow-agent` : 에이전트 origin 프레임만 보는 모드.
- UART 팀 TUI와 단축키 규약 공유 제안(§15).

---

## 6. 데이터/기록 포맷

### 6.1 내부 프레임 레코드 (메모리/IPC)
```text
Frame {
  seq: u64          # 데몬 전역 단조
  ts_mono: f64      # 데몬 monotonic 기준 초 (세션 시작 = 0 아님, 원시 monotonic)
  ts_hw: f64|null   # 드라이버 HW 타임스탬프 (드라이버 기준 시계)
  ts_wall: i64      # epoch ns (ts_mono를 세션 기준점으로 환산)
  domain: "can"     # 공통 계약 필드
  channel: "ch0"
  dir: "rx"|"tx"
  origin: "device"|"agent"|"user"|"replay"|"script"   # sim 노드 = device (§3.10)
  via: "uds"|"isotp"|"periodic"|null
  sim_node: str|null
  client: "agent:claude-1"|null   # 누가 (tx만)
  corr: str|null    # RPC 요청 id / 주기 태스크 id / isotp 채널 id
  payload: { id:u32, ext:bool, rtr:bool, err:bool, fd:bool, brs:bool, esi:bool, dlc:u8, data:bytes }
}
```
- 표시용 `t` = 세션 기준 상대초. HW 타임스탬프가 있으면 `ts_hw` 를 정렬 기준으로, 세션 시작 시 `ts_hw ↔ ts_mono` 오프셋을 기록하고 1분마다 드리프트 보정값을 `events.jsonl` 에 남긴다(보정 알고리즘 상세 미정).

### 6.2 세션 기록 (자동)
| 파일 | 포맷 | 내용 | 이유 |
|---|---|---|---|
| `can_<ch>.blf` | BLF (python-can BLFWriter) | 전 프레임 rx/tx, 에러프레임 | CANoe/CANalyzer에서 바로 열림, 압축, 표준 |
| `events.jsonl` | JSONL, 공통 레코드 | tx 프레임의 origin/client/corr(seq·ts·id로 BLF와 조인), 승인, 정책 결정, 채널 열기/설정, bus 상태 변화, 마크, DB 로드 | BLF에 출처 필드가 없으므로 사이드카 |
| `session.json` | JSON | loom 버전, 채널 설정, DB 경로+sha256, 시계 기준점, 호스트/OS | 재현성 |
| `isotp_<t>.jsonl` | JSONL | ISO-TP PDU 단위 (UDS 팀 트레이스 원천) | 프레임 재조립 없이 PDU 열람 |

- 회전: 파일당 512 MB 또는 1시간. 보존 기본 7일 / 20 GB (`loom.toml`), 승인/감사 로그는 보존 기간 별도(무기한 기본).
- 위치: `<project>/.loom/sessions/` (프로젝트 로컬, `.gitignore`), 없으면 OS 데이터 디렉터리.
- BLF 쓰기 성능/플러시 시 크래시 내구성(쓰다 죽으면 마지막 블록 손실) **미확인** → 대안: 빠른 내부 바이너리 저널(append-only, 고정 레코드) + 세션 종료 시 BLF 변환. Sol 측정 결과로 결정(§13 R5).
- `events.jsonl` 의 tx 레코드에 BLF 내 위치 매칭용 `(ch, ts_hw|ts_mono, id, seq)` 저장.

### 6.3 내보내기/가져오기
| 포맷 | 읽기 | 쓰기 | 라이브러리 |
|---|---|---|---|
| BLF | ✓ | ✓ | python-can |
| ASC | ✓ | ✓ | python-can |
| MF4 (ASAM MDF bus logging) | ✓ | ✓ | python-can `mf4`(asammdf 필요) / asammdf 직접 |
| TRC (PEAK) | ✓ | ✓ | python-can |
| CSV / Parquet | ✓ | ✓ | 자체 (pyarrow/polars) — 프레임 표 or 디코드된 신호 와이드/롱 테이블 |
| candump log | ✓ | ✓ | python-can canutils |

신호 내보내기 형태: `long`(ts, signal, value, unit, msg) 기본, `--wide --resample 10ms` 옵션.

### 6.4 loom.viz 데이터셋 모델
- 내부 표현: Arrow 테이블(열 지향). 파일은 lazy 스캔(polars `scan_*`), BLF/ASC/MF4 는 1회 변환 후 `<cache>/<sha>.parquet` 캐시.
- 질의: `(dataset, columns, t0, t1, px_width)` → 열별 MinMaxLTTB(tsdownsample) 다운샘플 → `px*2` 이하 포인트 반환. 확대 시 재질의.
- 라이브: 링버퍼 → 최근 N초는 원본, 그 이전은 레벨별 min/max 피라미드(1/10/100 다운) 유지.
- 메타: `{name, unit, kind: numeric|enum|bool|text|frame, enum_map, source}`.

### 6.5 설정 파일 `loom.toml` (프로젝트 루트)
```toml
[daemon]
port = 7341

[can]
default_channel = "ch0"

[can.channels.ch0]
interface = "pcan"
channel = "PCAN_USBBUS1"
bitrate = 500000
fd = true
data_bitrate = 2000000
env = "bench"            # sim | bench | vehicle
tx = "confirm"           # deny | confirm | allow
listen_only = false
db = ["db/powertrain.dbc"]
max_agent_fps = 200
max_agent_load_pct = 10
deny_ids = ["0x000-0x0FF"]   # 예: 안전 관련 ID 대역
allow_ids = []               # 비어 있으면 deny_ids 외 전부
autostart = true

[can.channels.ch1]
interface = "virtual"
channel = "sim1"
fd = true
env = "sim"
tx = "allow"
```

---

## 7. 라이브러리 선택과 근거

| 용도 | 선택 | 버전(확인) | 라이선스 | 근거 / 비고 |
|---|---|---|---|---|
| CAN 추상화 | **python-can** | 4.6.1 | LGPL-3.0 | 사실상 표준, vector/pcan/socketcan/kvaser/virtual/gs_usb, BLF/ASC/MF4/TRC IO. 최신 릴리스가 2025-08 → 개발 브랜치 활동은 **미확인**(Sol). LGPL: 수정 없이 의존성으로 사용 → 문제 없음으로 판단(법무 확인은 사용자 몫) |
| DBC | **cantools** | 45.1.2 | MIT | 인/디코드, 멀티플렉스, 값 테이블, `GenMsgCycleTime`, 활발한 릴리스(2026-10). ARXML 부분 지원 |
| DB 변환(보조) | canmatrix | 1.3.3 | BSD-2 | ARXML/DBF/XLS→DBC 변환 경로 (M5) |
| ISO-TP | **can-isotp** | 2.0.7 | MIT | `NotifierBasedCanStack` 로 Notifier 공유 가능, FD, 모든 주소 모드, 레이트 제한·listen_mode. udsoncan과 같은 저자 |
| MDF | asammdf | 8.8.32 | LGPLv3+ | MF4 읽기/쓰기, 대용량. 무거움(numpy/pandas 등) → optional extra `loom[mdf]` |
| IPC 서버 | **websockets** (+ 최소 HTTP) 또는 Starlette/FastAPI+uvicorn | 17.4 / 0.162.2 / 0.54.0 | BSD/MIT/BSD | 정적 파일+WS 하나면 충분 → 의존성 적은 `websockets` + 자체 정적 핸들러 vs FastAPI 편의. 개인 의견: **Starlette+uvicorn** (FastAPI 불필요) — 결정은 공통(메인) |
| 직렬화 | **msgspec** (JSON + msgpack, 스키마 구조체) | 0.22.0 | BSD-3 | 빠르고 타입 검증 내장 |
| 다운샘플 | **tsdownsample** | 0.1.5.1 | MIT | MinMaxLTTB, Rust 구현. 휠 플랫폼 커버리지 미확인 |
| 표 처리 | polars + pyarrow | 2.1.0 / 26.0.1 | MIT / Apache-2.0 | lazy 스캔, Parquet. polars 라이선스 필드는 PyPI상 비표준(MIT로 알려짐, 미확인) |
| TUI | **Textual** | 8.2.8 | MIT | herdr 패인용, UART 팀과 공유 가능 |
| 웹 플롯 | **uPlot** | 미확인 (1.6.x 로 기억) | MIT | 경량·고성능 시계열 |
| CANoe COM (M5 선택) | py_canoe | 26.4.1 | MIT | Windows+CANoe 필요. pywin32 기반 |
| LDF (Out) | ldfparser | 0.26.0 | MIT | 향후 LIN 시 |

탈락/보류: `python-can-cvector`(Vector 대체 백엔드로 추정, 이점 미확인), PySide6/pyqtgraph(§5 결정), cantools 대신 canmatrix 단독(인코드 API가 cantools가 더 편함), python-can `send_periodic` 그대로 사용(Vector/PCAN에서 태스크당 스레드 → 자체 스케줄러).

패키징: `loom` 코어 의존성 최소화, extras: `loom[can]`(python-can, cantools, can-isotp), `loom[mdf]`, `loom[viz]`(polars, pyarrow, tsdownsample), `loom[canoe]`(py_canoe, Windows).

---

## 8. HW/OS 지원 매트릭스

| 백엔드 | macOS (개발) | Windows (벤치) | Linux | FD | HW TS | 비고 |
|---|---|---|---|---|---|---|
| virtual (python-can) | ✓ | ✓ | ✓ | ✓ (`protocol=CAN_FD`) | – | 프로세스 내부. 데몬이 모두 호스팅하므로 충분 |
| PCAN (PCAN-Basic) | △ PCBUSB 필요 (python-can이 Darwin에서 `PCBUSB` 로드 확인). PCBUSB의 현 macOS 버전/Apple Silicon/FD 지원/라이선스는 **미확인** | ✓ | ✓ (PCAN-Basic Linux 또는 SocketCAN `peak_usb`) | ✓ (Win, FD 하드웨어) | ✓ | MacCAN 계열은 별도 경로, python-can 백엔드 여부 미확인 |
| Vector XL | ✗ | ✓ | ✗ | ✓ | ✓ | XL Driver Library(vxlapi64.dll) 필요. CANoe 공존: §8.1 |
| SocketCAN | ✗ | ✗ | ✓ | ✓ | ✓ | CI(Linux)에서 `vcan` 커널 모듈로 통합 테스트 가능(러너 권한 필요, 미확인) |
| Kvaser (CANlib) | △ (미확인) | ✓ | ✓ | ✓ | ✓ | HW 보유 시 |
| gs_usb (candleLight) | △ libusb | △ | ✓ | 부분 | – | 저가 대안, 선택 |

### 8.1 CANoe와 Vector 채널 공존
- XL Driver는 여러 애플리케이션이 같은 물리 채널을 동시에 열 수 있고, **채널 설정(비트레이트 등) 권한(init access)은 하나의 앱만** 가진다 (Vector XL 문서상 개념, 원문 출처 확인은 Sol).
- python-can VectorBus는 `bitrate/fd/timing/listen_only` 를 하나라도 주면 init access를 요청한다(소스 확인). 따라서 **CANoe 공존 모드 = 이 인자들을 모두 생략하고 연다**. Loom 채널 설정에 `coexist = true` 를 두어 해당 인자를 강제로 빼고, 실제 비트레이트는 드라이버에서 읽어 표시만 한다.
- FD 채널: python-can은 `fd=True` 일 때 interface version V4로 열고 init access도 요청 → CANoe가 이미 init access를 가진 FD 채널에 `fd=True` 로 붙으면 어떻게 되는지(경고 후 계속? 실패?) **미확인 — M4 실HW 검증 항목 1순위**.
- `listen_only` 도 init access가 필요(python-can 코드상 `_listen_only` 가 권한 요청 조건) → CANoe 공존 시 HW listen-only 불가. **대신 Loom 정책의 소프트 listen-only(tx=deny)** 로 보장.
- `app_name` 기본값 "CANalyzer" → Vector Hardware Config의 CANalyzer 앱 채널 매핑을 그대로 씀. Loom은 `app_name="Loom"` 을 쓰고 `Vector Hardware Config` 에 앱 등록하는 가이드 + `serial`/global channel index 지정 방식을 references에 문서화.
- 공존 시 송신: 기술적으로 가능(일반 Tx는 init access 불필요로 알려짐, 미확인). Loom 정책상 coexist 채널 기본 `tx=confirm`.

---

## 9. 시뮬레이션 & 테스트 전략

### 9.1 시뮬레이션 구성
```text
 loomd
  ├ ch1: virtual "sim1" (FD)  ◄─┬─ SimNode EMS  (DBC 노드 EMS의 TX 메시지들을 GenMsgCycleTime 주기로 송신, 신호 생성기)
  │                             ├─ SimNode ESP  (VehicleSpeed: ramp/sine/csv 재생)
  │                             ├─ SimNode UDS-ECU (ISO-TP 0x7E0/0x7E8, UDS 팀 가상 ECU를 in-proc로 연결)
  │                             └─ Replay node (BLF → 버스)
  └ ch2: virtual "sim1"  ← 같은 이름이면 같은 가상 버스 → 두 채널 간 게이트웨이/루프백 테스트
```
- SimNode 정의: YAML
```yaml
node: EMS
db: db/powertrain.dbc
messages:
  EngineData:
    period: 10ms            # 생략 시 DBC GenMsgCycleTime
    signals:
      EngRPM: {gen: sine, min: 800, max: 4000, period: 8s}
      EngTemp: {gen: ramp, from: 20, to: 90, over: 120s}
      Counter: {gen: counter, mod: 16}
      CRC: {gen: none}      # E2E/CRC는 플러그인 훅 (미범위)
responses:
  - on: {msg: Diag_Req, sig: {Cmd: 1}}
    send: {msg: Diag_Resp, set: {Status: "OK"}}
faults:
  - at: 30s
    drop: EngineData       # 메시지 누락 → Statistics 경고 검증
```
- 데모 번들: `examples/sim/` (DBC + 노드 YAML + 시나리오) → `loom can demo` 한 줄로 sim 채널 + 노드 + 뷰어.

### 9.2 테스트 계층
| 계층 | 대상 | 환경 | CI |
|---|---|---|---|
| 단위 | 필터 식 파서, 커서, 링버퍼, 정책 엔진, 레이트 리미터, wait-for 식, 자동 렌더 규칙, 다운샘플 | 순수 Python | ✓ (mac/linux/win 매트릭스) |
| 서비스 | CAN 서비스 + virtual bus + SimNode, ISO-TP 왕복(멀티프레임/FC/WAIT/오버플로), 주기 송신 타이밍 통계 | loomd in-proc | ✓ |
| RPC 계약 | JSON-RPC 스키마 스냅샷, CLI 출력 골든 파일(사람용/`--json`), 출력 상한·종료 코드 | loomd 서브프로세스 | ✓ |
| 웹 | Playwright 스모크(트레이스 표시, 승인 배너, STOP 버튼), 프런트 단위 테스트 | headless | ✓ (Linux) |
| 성능 | 5k fps 가상 입력 2분: drop 0, CPU %, WS 지연 p99, BLF 기록 처리량; 2M 포인트 viz 질의 < 200 ms | 벤치 스크립트 | nightly |
| **핵심 루프 불변식** | §2.0 E2E 자동화(에이전트 CLI + user origin 테스트 클라이언트), 늦은 구독 시 backlog 수신(I3), RPC 스키마에 비공개/숨김 옵션 부재(I2), 모든 TX가 구독자에게 도달(ISO-TP·주기·sim 포함), 사용자 송신 후 에이전트 출력에 NOTE(I6), 지연 p95 측정(§3.12), 5k fps 시 skipped 표시 + 기록 무손실 | loomd 서브프로세스 + 헤드리스 TUI(Textual pilot) | ✓ (M1부터, 실패 시 머지 금지) |
| 에이전트 E2E | 스킬 시나리오(A1~A5)를 실제 에이전트에 시켜 결과 검증 — 사람이 판정하는 체크리스트 + 로그 | 수동/반자동 | ✗ (릴리스 전) |
| HW | PCAN(mac PCBUSB, Win), Vector(Win, CANoe 공존), 두 장치 크로스 연결 루프백 | 수동 체크리스트 | ✗ |
| Linux vcan | SocketCAN 통합 | Linux 러너 권한 필요(미확인) | 선택 |

- 결정성: 가상 시계 모드(`loomd --sim-clock`)로 SimNode/주기 송신/wait-for 타이밍 테스트를 실시간 대기 없이 수행 — 주기 스케줄러가 Clock 추상화를 통해 시간을 얻도록 설계.

---

## 10. 안전 / 가드레일

### 10.1 원칙
1. **정책 집행은 데몬에서.** 스킬 문서는 안내일 뿐, CLI/SDK/웹 어디서 와도 같은 정책 엔진을 통과. 에이전트가 스킬을 무시해도 우회 불가.
2. **기본은 안전 쪽.** 새 채널은 `tx=confirm`, `env=bench`. `env=vehicle` 은 기본 `tx=deny`.
3. **승인은 사람만.** 승인 응답(`core.approval.respond`)은 user origin 클라이언트(웹 뷰어/TUI)에서만 받는다. CLI에 `approve` 명령을 두지 않는다 → 에이전트가 자기 요청을 승인할 수 없다.
4. **멈출 수단은 항상 즉시.** `loom can stop-all`, 웹 STOP 버튼, TUI `Ctrl-X` 는 정책·승인 없이 즉시.
5. **잊혀진 송신 방지.** 에이전트 주기 송신은 lease(기본 10 분) 만료 시 자동 정지, 에이전트 세션 단절(클라이언트 이름 기준 하트비트 없음)과 무관하게 lease로 판단.

### 10.2 위험 등급 (CAN)
| 등급 | 정의 | 예 | 기본 처리 |
|---|---|---|---|
| R0 관찰 | 버스에 영향 없음 | trace, sig get/wait, stats, record, export, viewer-only replay | 항상 허용 |
| R1 시뮬 | `env=sim` 채널 송신 | sim 노드, virtual 송신 | 허용 + 기록 |
| R2 단발 | bench 채널에 유한 개 프레임(≤10개) | `send 0x123#..`, `send --msg` | `tx=allow` 면 허용, `confirm` 이면 승인(세션 범위 승인 가능) |
| R3 지속 | 주기 송신, IG 신호 덮어쓰기, 버스 리플레이, 버스트 >10, ISO-TP 오픈 | `periodic start`, `sig set`, `replay --to ch0` | `confirm` 필요 (scope=session 승인 시 같은 메시지 ID 재시작 자동 허용) |
| R4 위험 | 실차(`env=vehicle`) 송신, deny_ids 대역, 수신 중인 다른 노드의 ID를 덮어쓰는 송신(ID 충돌), 에러프레임/버스오프 유발, init access 획득(비트레이트 변경), 공존 채널에서 CANoe가 송신 중인 ID 송신 | | `tx=allow` 여도 **매번 승인**, `scope=once` 만 |

- **ID 충돌 감지:** 최근 N초 동안 `origin=device` 로 수신된 ID를 에이전트가 송신하려 하면 R4로 승격(실 ECU 메시지를 위조하는 상황). 메시지가 DBC상 Loom이 흉내내는 노드(SimNode 또는 `tester_node`) 소속이면 예외.
- 판정 결과는 dry-run 출력에 항상 표시(`risk=R3 reason=periodic on bench channel`).

### 10.3 승인 흐름
```text
agent: loom can periodic start X --period 20ms --channel ch0
 └► daemon: Policy → R3, tx=confirm → Approval a17 생성 (만료 120 s)
     ├► 웹 배너 / TUI 하단 바 / (선택) OS 알림
     ├► CLI: --wait-approval 기본 60 s 동안 RPC 보류, 
     │      승인 → 정상 결과 exit 0 / 거부 → "DENIED by user:web" exit 5 / 만료 → "PENDING a17 expired" exit 5
     └► 감사 로그: 요청·결정·결정자·dry-run 바이트
```
- `--no-wait` 이면 즉시 `PENDING a17` (exit 5) 반환, 에이전트는 `loom approval status a17` 로 확인 (조회만, 승인 불가).
- 사용자가 터미널만 쓰는 경우: TUI가 승인 UI 역할. 뷰어가 하나도 붙어 있지 않으면 → `NO_APPROVER: open 'loom can monitor' or the web viewer` 즉시 반환 (에이전트가 사용자에게 안내).

### 10.4 dry-run
모든 TX 명령에 `--dry-run`: 정책 판정, 위험 등급, 인코딩 결과 바이트, 신호 범위 클램핑 경고(물리값이 DBC min/max 밖이면 오류, `--force-range` 는 R4), 예상 부하 증가(%)를 보여주고 아무것도 보내지 않음. 스킬은 R2 이상에서 "처음 한 번은 dry-run" 을 요구.

### 10.5 레이트 제한
- 채널별 토큰 버킷: agent origin 합계 `max_agent_fps`(기본 200 fps), 예상 부하 `max_agent_load_pct`(기본 10%). 초과 요청은 거부(대기열에 쌓지 않음) + 경고.
- 버스 부하 전체가 80% 이상이면 새 R2/R3 요청 거부.
- 버스 상태 ERROR_PASSIVE/BUS_OFF 진입 시 agent 주기 태스크 자동 일시정지 + 이벤트.
- 사용자(user origin) IG는 별도 한도(기본 무제한 경고만).

### 10.6 채널 tx 정책 / 실차 vs 벤치
| env | 기본 tx | 변경 가능 | 표시 |
|---|---|---|---|
| sim | allow | – | 회색 SIM 배지 |
| bench | confirm | `allow` 로 완화는 `loom.toml` 편집(사람)만, CLI 오버라이드는 더 엄격하게만 | 노란 BENCH |
| vehicle | deny | `confirm` 까지만 완화 가능, 모든 송신 R4 | 빨간 VEHICLE |
- `env` 는 장치를 여는 쪽이 아니라 **설정 파일 소유자(사람)** 가 정한다. 미지정 하드웨어 채널은 `bench` + 첫 연결 시 뷰어에 "이 채널은 실차입니까?" 확인 배너.
- listen-only: HW 지원(init access 가능할 때) + 소프트(tx=deny) 이중.

### 10.7 감사 로그
`~/.loom/audit/YYYY-MM.jsonl` + 세션 디렉터리 복제. 레코드: `ts, client, origin, action(rpc method), params 요약, risk, policy_decision, approval_id, approver, result, frames_sent`. append-only, 데몬만 씀. 뷰어 Audit 패널 + `loom audit tail --max 50`.

### 10.8 스킬 레벨 규칙 (SKILL.md에 박을 문장)
- 송신 전 `loom can status` 로 채널 env/tx 확인. `vehicle` 채널에는 사용자가 명시적으로 요청한 경우에만 송신 시도.
- R2+ 송신은 먼저 `--dry-run`, 결과를 사용자에게 한 줄로 알린 뒤 실행.
- 주기 송신을 시작했으면 작업 끝에 반드시 `loom can periodic stop --mine`.
- 승인 요청이 뜨면 사용자에게 "뷰어에서 승인해 주세요"라고 말하고 기다린다. 승인을 우회하려 하지 않는다(설정 파일 수정 포함 금지).
- 정책 거부(exit 4)는 재시도하지 않고 사용자에게 보고.

---

## 11. 마일스톤

원칙(02-core-principle): **M1 = 걷는 뼈대.** virtual 백엔드 위에서 4요소(직접 접근·읽기·쓰기·사용자 실시간 표시+개입)가 최소 형태로 전부 동작해야 M1 완료. DBC, 웹, 승인 흐름, 실HW, 다수 포맷은 M1에 넣지 않는다. 이후 마일스톤은 "루프 품질을 올리는 순서"로 정렬.

| M | 이름 | 내용 | 수용 기준 |
|---|---|---|---|
| M0 | 측정 스파이크 (M1과 병렬, 1주 이내) | (a) virtual 5k/20k fps 주입 시 RX→링버퍼→WS 지연·드롭, (b) BLF 직접 기록 처리량, (c) can-isotp `NotifierBasedCanStack` 왕복·FC 지연, (d) Windows/mac 10 ms 소프트 주기 지터 | 숫자 보고서(`spikes/can/REPORT.md`). 결정: BLF 직접 vs 저널, 단일 프로세스 유지 여부 |
| **M1** | **걷는 뼈대 (virtual)** | loomd 자동 기동 + WS JSON-RPC + discovery/token · virtual 채널 열기(`loom.toml` 또는 `loom can open`) · 데몬 내 raw 규칙 SimNode(`on 0x300 → reply 0x301`, 주기 raw 프레임) · CLI `status / send (raw) / trace (--since @last, --ids, --origin, --max, --follow --timeout) / last / stop-all` · 출처 태깅(agent/user/device) + Activity 이벤트 + 사용자 개입 NOTE · **TUI `loom can monitor`**: 라이브 Chrono/Fixed, 구독 시 backlog, `:send` 사용자 송신, Ctrl-X STOP, LAG/skipped 표시 · 최소 기록(BLF + events.jsonl) · 최소 안전(env=sim 허용, 그 외 채널 tx=deny, stop-all) · `loom-can` 스킬 v0 | ① §2.0 종단 간 시나리오가 CI에서 자동 통과(에이전트 CLI + user origin 테스트 클라이언트). ② 프레임 → TUI 표시 p95 < 200 ms 측정값 보고. ③ 에이전트가 10분째 송신 중일 때 TUI를 새로 열면 backlog+라이브가 1 s 안에 보임. ④ 5k fps 60 s 주입: TUI 멈춤 없음, skipped 카운터 표시, BLF 프레임 수 = 주입 수(무손실). ⑤ RPC에 숨은 구독/비공개 옵션 없음(스키마 테스트). ⑥ macOS·Windows·Linux CI green |
| M2 | 신호·승인·웹·첫 실HW | DBC 로드/디코드/인코드(cantools), `sig get / wait-for`, `send --msg --set`, 주기 송신 + lease + `sig set`(IG), 정책 R0~R4 + 승인 흐름(뷰어 전용) + dry-run + 레이트 제한, **웹 뷰어 v1**(Trace, Data, IG, Approvals, STOP, LAG/skip 표시), PCAN 실HW(Windows 우선, mac은 PCBUSB 가능 시), **ISO-TP 최소판**(IsoTpService normal 11bit, virtual — UDS 팀 걷는 뼈대 선행조건) | A1·A2 시나리오, §2.0 시나리오를 PCAN 2채널 교차 루프백 또는 실 ECU에서 재현, 웹 Trace p95 < 150 ms, confirm 채널에서 승인 없이 송신 불가 테스트, UDS 팀 `read-did` over virtual ISO-TP green |
| M3 | Graphics & loom.viz | Graphics/State 패널(라이브, 커서, 줌, decimation 표시), viz 데이터셋 어댑터(CSV/JSONL/Parquet/BLF/ASC 우선), MinMaxLTTB 질의, `loom viz show/push/summary`, live:uart/live:uds 소스, UDS V-UDS-1/2 패널, `loom-viz` 스킬 | 라이브 신호 → Graphics 갱신 p95 < 200 ms, `loom viz show 2M-row.parquet --y a,b` 첫 화면 < 2 s·줌 < 200 ms, viz-requirements 우선순위 상위 항목 충족 |
| M4 | Vector & ISO-TP 완성 | Vector XL(Windows), CANoe 공존(coexist) 검증, CAN FD 실HW, ISO-TP 전체(모든 주소 모드, FD, 그룹/functional, 주기 PDU), stats/Statistics 패널 | CANoe 측정 중 Loom trace/send 동작 보고서, 4095B 멀티프레임 STmin 준수, UDS 팀 통합 테스트 green |
| M5 | 후순위 | Replay(뷰어/버스), MF4/TRC export, 레이아웃 저장·도킹, Statistics 고도화, PNG 스냅샷, (선택) CANoe COM 4동사, (선택) ARXML, Kvaser/gs_usb, 가상 시계 모드 | 사용자 체크리스트(§2 U1~U6) |

**M1에서 의도적으로 뺀 것과 이유:** DBC(루프는 raw로도 성립), 웹(TUI로 ④ 충족, 웹은 M2 첫 작업), 승인 흐름(M1은 sim만 송신 허용이라 불필요), 주기 송신(1회 송신으로 ③ 충족), 실HW(virtual로 루프 검증 후 백엔드만 교체).

---

## 12. 구현 단계 작업 분해

담당 추천: **O** = Opus 워커(아키텍처·UX·스킬·정책·통합), **S** = Sol 워커(드라이버·성능·프로토콜·테스트 인프라). M1 작업은 최소 범위로 자른 "a" 버전, 이후 확장은 "b".

**M1 (걷는 뼈대) — 임계 경로: C0 → C1 → K1a → {K3a, T1a} → Q0**
| ID | 작업 | 담당 | 의존 | 산출물 |
|---|---|---|---|---|
| C0 | loomd 골격: 게이트웨이(WS JSON-RPC), auth/discovery, client registry, seq/Clock, 자동 기동 | O (공통 소유는 메인 결정) | – | `loom.core.daemon` |
| C1 | EventBus + 링버퍼 + 커서 저장소 + 구독 fan-out(backlog 전송, 클라이언트별 예산·skipped 카운트, lag 계산) | S | C0 | `loom.core.events` |
| C2 | CLI 프레임워크: 출력 상한, `--json`, 종료 코드, `--since` 파서, NOTE 요약 | O | C0 | `loom.cli` |
| K1a | ChannelManager(virtual만) + Notifier RX + TX 경로(EventBus 선주입) | S | C1 | |
| K3a | `status/send(raw)/trace/last/stop-all` + Activity 이벤트 | O | K1a, C2 | |
| K4a | 최소 정책(env/tx, sim 허용·그 외 deny) + stop-all + 감사 로그 | O | C0 | `loom.core.policy` 초기 |
| K6a | 레코더: BLF + events.jsonl (회전 없음) | S | K1a | |
| K8a | raw 규칙 SimNode(YAML: on-id reply, 주기 raw) | S | K1a | `examples/sim/m1` |
| T1a | TUI monitor: Chrono/Fixed, backlog, `:send`(origin=user), Ctrl-X, LAG/skipped 배지 | O | C1, K3a | |
| Q0 | E2E 하네스: §2.0 자동화(user 테스트 클라이언트), 지연 측정, 5k fps 부하, CI 3 OS | S | K3a, T1a, K8a | |
| D0 | `loom-can` 스킬 v0 (status→trace→send→stop-all, 뷰어 안내) | O | K3a | |
| S0 | M0 측정 스파이크 | S | – (병렬) | `spikes/can/` |

**M2 이후**
| ID | 작업 | 담당 | 의존 | M |
|---|---|---|---|---|
| K1b | pcan/vector/socketcan 백엔드 매핑, FD, HW 타임스탬프, coexist 모드 | S | K1a | M2/M4 |
| K2 | DB 서비스(cantools): 로드/검색/지연 디코드/SignalCache | S | K1a | M2 |
| K3b | `sig get/wait-for` 식 파서·평가, `send --msg` | O | K2, C2 | M2 |
| K4b | 정책 R0~R4 전체 + 승인 흐름 + 레이트 리미터 + ID 충돌 감지 | O | K4a | M2 |
| K5 | TxScheduler(타이머 휠, lease, 원자적 신호 갱신, socketcan BCM) | S | K1a, K4b | M2 |
| K9a | IsoTpService 최소판(normal 11bit) + RPC | S (UDS 팀과) | K1a | M2 |
| W1 | 웹 SPA 골격(빌드, WS 클라이언트, 바이너리 디코드, backlog 처리) | O | C1 | M2 |
| W2 | Trace 패널(가상 스크롤, Fixed/Chrono, 필터, 출처 색, skipped 행) | O | W1 | M2 |
| W4a | Data/IG/Approvals/STOP 패널 | O | W1, K4b, K5 | M2 |
| Q2 | HW 검증 체크리스트(PCAN Win/mac, 이후 Vector+CANoe) | S (사용자 HW) | K1b | M2/M4 |
| V1 | viz 데이터셋 어댑터 + 캐시 + MinMaxLTTB 질의 + live 피라미드 | S | C1 | M3 |
| W3 | Graphics/State 패널 + 커서/줌 + uPlot + decimation 표시 | O | W1, V1 | M3 |
| V2 | 자동 렌더 규칙 + `loom viz` CLI + summary + live:uart/uds + UDS 패널 | O | V1, W3 | M3 |
| K9b | ISO-TP 완성(주소 모드 전체, FD, 그룹, 주기 PDU) | S | K9a, K5 | M4 |
| K7 | Stats + Statistics 패널 | S/O | K1a | M4 |
| D1 | 스킬 `loom-can` 완성 + `loom-viz` + references + Codex 호환 | O | K3b, V2 | M2~M3 |
| X1 | 후순위 묶음(Replay, MF4/TRC, CANoe COM, ARXML) | S | – | M5 |

병렬 경로(M1): {C1, C2, K4a} 동시 → {K1a} → {K3a, K6a, K8a} 동시 → {T1a, D0} → Q0. S0는 처음부터 병렬.

---

## 13. 리스크

| # | 리스크 | 영향 | 가능성 | 완화 |
|---|---|---|---|---|
| R1 | Python GIL/스레드 경합으로 고부하(FD 2M, >5k fps)에서 RX 드롭·주기 지터·ISO-TP FC 지연 | 높음 | 중 | M0 측정, 드라이버 수신 큐 확대(vector `rx_queue_size`), 지연 디코드, 필요 시 CAN 서비스 자식 프로세스 분리, free-threaded Python 검토 |
| R2 | Vector 공존: FD 채널에서 init access 없이 열기 실패/제약 | 높음(CANoe 사용자) | 중 | M4 1순위 실HW 검증, 실패 시 CANoe 측 채널 공유 설정 가이드 또는 CANoe COM 경유 송신(X1) |
| R3 | macOS PCAN(PCBUSB) 사용 불가/불안정/FD 미지원 | 중 | 중 (미확인) | 개발은 virtual 중심, 실HW는 Windows 벤치 + 웹 원격 시청 |
| R4 | Windows 타이머 해상도/스케줄링으로 소프트 주기 송신 지터 | 중 | 중 | 타이머 휠 + 고해상도 대기, 실측, 필요 시 HW 주기 송신 API(Vector XL 고유 기능 존재 여부 미확인) |
| R5 | BLF 쓰기 처리량·크래시 시 손실 | 중 | 미확인 | 내부 저널 + 사후 변환 대안 준비 |
| R6 | 웹 프런트엔드 개발 범위가 커짐 (CANoe 수준 기대) | 높음(일정) | 높음 | 패널 우선순위(Trace→Data→IG→Graphics→Stats→Replay), 프레임워크 최소화, 패널 단위 병렬 |
| R7 | 에이전트가 출력 상한/커서 규칙을 무시해 컨텍스트 폭발 | 중 | 중 | 서버측 하드 상한, 기본 `--max`, 요약형 명령(`stats`, `viz summary`) 유도 |
| R8 | 승인 UI가 없을 때 에이전트 작업 정체 | 중 | 중 | `NO_APPROVER` 즉시 반환 + 안내 문구, sim 채널은 승인 불필요 |
| R9 | 오탐 R4 승격(ID 충돌 감지)로 사용성 저하 | 낮음 | 중 | DBC 노드/`tester_node` 예외, 승인 scope=session(R3까지) |
| R10 | 라이선스(LGPL python-can, asammdf, PySide 미사용) 이슈 | 낮음 | 낮음 | 동적 의존만, 수정 시 공개 |
| R11 | 단일 데몬 내 타 도메인 크래시가 CAN 기록 중단 | 중 | 낮음 | 서비스 격리, 레코더 flush 주기 짧게, 자식 프로세스 모드 |
| R13 | 실시간성 목표(§3.12) 미달 — 특히 Windows + 고부하에서 화면 지연 | 높음(핵심 루프) | 중 | M1부터 지연을 CI 지표로 측정·회귀 감지, 화면 예산·skipped 표시로 "멈춤" 대신 "생략 명시", 필요 시 CAN 서비스 자식 프로세스 분리 |
| R14 | 사용자가 뷰어를 안 열어 "에이전트 혼자 보는" 상태가 됨 | 높음(원칙 위반) | 중 | I5 `viewers: 0` 힌트, 스킬의 첫 송신 전 안내 규칙, 선택적 `viewer.auto_open` |
| R12 | 사용자가 HW 시계와 PC 시계 정렬(UART/CAN 겹쳐보기) 정확도 기대 | 낮음 | 중 | 정렬 정확도(ms 수준) 문서화, 드리프트 보정 기록 |

---

## 14. 사용자에게 물어볼 질문 (기본값 제안)

| # | 질문 | 기본값(답 없으면 이대로 진행) |
|---|---|---|
| Q1 | 보유 CAN HW 모델은? (Vector VN16xx/VN56xx, PCAN-USB/PCAN-USB FD, Kvaser …) | PCAN-USB FD 1대 + Vector 1대 가정, 둘 다 1순위 지원 |
| Q2 | 실HW를 붙이는 PC OS는? macOS에서도 PCAN 직접 쓰길 원하나? | Windows 벤치 PC, macOS는 virtual + 원격 웹 시청 |
| Q3 | CANoe/CANalyzer를 동시에 띄운 상태로 Loom을 쓸 일이 많은가? | 많다 → CANoe 공존(coexist) 모드 M4 필수 |
| Q4 | DB 포맷: DBC만? ARXML/LDF 필요? | DBC만, ARXML은 변환 가이드 |
| Q5 | 실차(차량)에 연결해 송신할 일이 있나? | 벤치/시뮬만. vehicle 채널은 tx=deny |
| Q6 | 송신 승인 정도 — 벤치 채널 단발 송신까지 매번 승인? | bench: 세션 단위 승인(R2/R3), R4는 매번 |
| Q7 | 사용자 뷰어로 웹 브라우저 사용 OK? (데스크톱 앱 원하나?) | 웹 + herdr TUI |
| Q8 | CANoe 자체 제어(측정 시작/정지, 시스변수, 테스트 실행) 필요? | M5 선택 범위, 기본 미포함 |
| Q9 | 주기 송신 정확도 요구 (예: 10 ms ±1 ms면 충분?) | ±1 ms 소프트웨어, 그 이상은 HW 기능 조사 후 |
| Q10 | 기록 보존 기간/용량 | 7일 / 20 GB, 감사 로그 무기한 |
| Q11 | 대용량 로그 규모 (가장 큰 BLF 크기/길이)? | 1 GB / 수천만 프레임까지 viz 지원 목표 |
| Q12 | E2E 보호(CRC/Alive counter) 계산이 필요한 메시지를 송신해야 하나? | 필요 시 플러그인 훅(M5), 기본 미지원 |
| Q13 | LGPL 의존성(python-can, asammdf) 사용 가능? | 가능 |

---

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 공통 계약 변경 요청
| # | 대상 | 요청 | 근거 |
|---|---|---|---|
| CR1 | §3-2 데몬 | "장치마다/도메인마다 데몬"이 아니라 **단일 `loomd` + 도메인 서비스** 로 명문화. 서비스는 자식 프로세스로 분리 가능한 인터페이스 | §3.8 |
| CR2 | §3-3 IPC | **JSON-RPC 2.0 over WebSocket, 127.0.0.1 단일 포트**, 이벤트는 `core.subscribe` → `event` notification(배치), 대용량은 바이너리 WS 프레임(msgpack 헤더 + 열 블록). 인증: 데몬 기동 시 랜덤 토큰을 discovery 파일(`<runtime_dir>/loom/daemon.json`: port, pid, token, version)에 쓰고 첫 메시지로 제출. `Origin` 헤더 검사(로컬 뷰어 오리진만)로 브라우저 DNS rebinding/타 사이트 접속 차단. 웹 뷰어는 같은 오리진 HTTP에서 쿠키로 토큰 획득 | 웹 직결, 크로스플랫폼(named pipe 회피), 단순 |
| CR3 | §3-6 기록 | 공통 레코드에 `seq`(데몬 전역 단조), `session`, `client`, `corr` 필드 추가. `origin` 은 agent/user/device 3종 + `replay`/`script` 만 두고, 경로는 별도 `via` 필드(`uds`, `isotp`, `periodic`), 가짜 장비는 `device`+`sim_node` (사용자 화면의 3색 규칙을 단순하게 유지). `ts` 는 `ts_mono` + `ts_wall` + 선택 `ts_hw` 3개 | 커서·크로스도메인 정렬·감사 |
| CR4 | §3-4 CLI | 공통 종료 코드(§4.1), 공통 `--since` 문법(`@last`, `seq:N`, `-5s`, 시각), 공통 출력 상한 기본(200행/64 KB), `LOOM_CLIENT` 환경변수 | 세 도메인 스킬이 같은 규칙을 공유 |
| CR5 | §3-10 안전 | 공통 위험 등급 골격 R0(관찰)~R4(위험) + 도메인별 매핑, **승인은 user origin 뷰어에서만**, `--dry-run` 공통, `stop-all` 공통(`loom stop-all` = 전 도메인), lease 개념(UDS TesterPresent·UART 자동 송신에도 적용), 정책 엔진은 `loom.core.policy` 하나 | 에이전트가 자기 승인 불가 보장 |
| CR6 | §3-11 스킬 | 도메인 스킬 외에 **`loom` 공통 스킬**(데몬 기동, 승인/안전 공통 규칙, 출력 규칙)을 두고 도메인 스킬은 이를 참조 | 반복 제거, 일관성 |
| CR7 | 설정 | 프로젝트 루트 `loom.toml` 하나에 도메인별 섹션 | 사용자 한 곳에서 관리 |
| CR8 | 뷰어 | 웹 SPA는 CAN 팀(viz) 소유, UART/UDS는 패널 플러그인으로 기여(패널 API: `register_panel(kind, topics, component)`). TUI 단축키 공통(`Ctrl-X` STOP, `/` 필터, `Space` 일시정지, `q`) | 사용자 경험 통일 |

### 15.2 CAN ↔ UDS: ISO-TP 인터페이스 제안 (→ `_xteam/can-uds-isotp.md` 초안 소재)

**배치:** ISO-TP 스택(can-isotp `NotifierBasedCanStack`)은 CAN 서비스 안. UDS 서비스는 같은 loomd 안이면 Python API, 밖이면 RPC(동일 의미).

**Python API (in-process)**
```python
class IsoTpParams(TypedDict, total=False):
    addressing: Literal["normal11","normal29","normalfixed29","extended11","extended29","mixed11","mixed29"]
    tx_id: int; rx_id: int                      # normal
    source_address: int; target_address: int    # fixed/extended/mixed
    address_extension: int                       # mixed
    functional_tx_id: int | None                 # 기능 주소 송신 ID (예 0x7DF)
    can_fd: bool; bitrate_switch: bool; tx_data_length: int   # 8|12|..|64
    tx_padding: int | None                       # 예 0xCC, None=패딩 없음
    stmin: int                                   # 우리가 수신측일 때 FC로 알리는 STmin (raw 0x00-0x7F, 0xF1-0xF9)
    blocksize: int
    override_receiver_stmin: float | None        # 송신 시 상대 STmin 무시(초)
    rx_flowcontrol_timeout_ms: int               # N_Bs (기본 1000)
    rx_consecutive_frame_timeout_ms: int         # N_Cr (기본 1000)
    wftmax: int
    max_frame_size: int                          # 기본 4095 (FD 시 더 크게)
    listen_mode: bool                            # 관찰 전용(FC 미송신)

class IsoTpChannel(Protocol):
    id: str                                      # "t1"
    def send(self, pdu: bytes, *, functional: bool = False, timeout_s: float = 2.0) -> None: ...
    def recv(self, timeout_s: float) -> IsoTpPdu | None: ...   # IsoTpPdu{data, ts_first, ts_last, seq_first, seq_last, functional}
    def set_params(self, **p) -> None: ...
    def stats(self) -> dict: ...                 # tx/rx PDU 수, FC WAIT/OVFL, 타임아웃
    def close(self) -> None: ...

class IsoTpService(Protocol):
    def open(self, channel: str, params: IsoTpParams, *, client: str) -> IsoTpChannel: ...
```
- 오류 타입: `IsoTpTimeout(kind="N_Bs"|"N_Cr"|"N_As")`, `IsoTpOverflow`, `IsoTpUnexpectedPdu`, `ChannelDown`, `PolicyDenied`, `ApprovalPending`.
- `open` 은 정책 판정 R3(bench, tx=confirm이면 세션 승인 1회). 이후 send는 UDS 팀 정책(서비스별 위험 등급)이 담당 — **CAN 팀은 프레임 레벨 레이트 제한·STOP만 적용**, UDS 서비스 의미는 판단하지 않는다.
- 같은 (channel, rx_id) 에 두 IsoTpChannel 금지(충돌) → `AddressInUse`.

**RPC (외부 프로세스용)**
| 메서드 | params | result/event |
|---|---|---|
| `can.isotp.open` | `{channel, params}` | `{isotp_id}` |
| `can.isotp.send` | `{isotp_id, data_hex, functional}` | `{ok, ts_last}` (전송 완료까지 보류) |
| `can.isotp.recv` | `{isotp_id, timeout_ms}` | `{data_hex, ts_first, ts_last}` 또는 timeout |
| `can.isotp.close` | `{isotp_id}` | |
| 이벤트 `can.isotp.pdu/<id>` | 구독 시 PDU 단위 rx/tx 통지 | 뷰어 UDS 트레이스용 |

**TesterPresent:** UDS 팀 소관이나, 단일 프레임 주기 송신(예 `0x7DF 02 3E 80`)은 CAN TxScheduler의 주기 태스크로 제공 가능 → `can.tx.periodic.start` 를 UDS 서비스가 `via=uds` 로 사용(lease = UDS 세션 수명). UDS 팀이 자체 타이머를 원하면 그대로 ISO-TP send 사용.

**타이밍 보장 범위(제안):** FC 응답은 can-isotp 스레드에서 생성 → 수 ms 이내 기대(실측 M0/M4, 미확인). 데몬은 ISO-TP 채널 존재 시 해당 채널 RX 우선 처리(디코드/브로드캐스트보다 앞). P2/P2* 타이머는 UDS 쪽.

**UDS 리드 초안 `_xteam/can-uds-isotp.md` v0.1 에 대한 can-opus 의견** (R1 중 발견, 이번 라운드는 `_xteam/` 쓰기 금지라 여기에 적음 → 리드가 `## can 의견` 절로 옮길 것)
| UDS 초안 항목 | 의견 |
|---|---|
| §1 PDU 단위 IPC, 프레임은 CAN 트레이스에 그대로 | **동의.** 내 위 제안과 같음. 숨은 세션 금지는 §3.11 I2로 보장 |
| §3 `send` 즉시 반환 + `tx_done/rx` 이벤트 | **UDS 안 채택**, 위 RPC의 "전송 완료까지 보류"는 철회. 단순 클라이언트용 `send_and_wait` 동기 헬퍼 함께 제공. in-process API도 같은 의미(콜백/큐) |
| §3 P2 기준 = `tx_done.ts_last_frame`, 응답 = `rx.ts_first_frame` | 동의. HW 타임스탬프 있으면 그 값, 없으면 RX 스레드 수신 시각(`ts_mono`) — 어느 쪽인지 이벤트에 `ts_src` 표기 |
| 미결 Q1 엔진 | can-isotp 2.x `NotifierBasedCanStack` 사용 (Notifier 공유, MIT, 소스 확인) |
| 미결 Q2 패딩 기본값 | 채널 open 파라미터 필수 아님, **기본 0xCC**, `null`=패딩 없음(CAN FD에서 DLC 맞춤 패딩은 라이브러리 규칙 따름 — 미확인). OEM별로 `loom.toml` 에서 덮어씀 |
| 미결 Q3 같은 tx_id raw 송신 | agent origin raw 송신은 **R4 승격(매번 승인)**, user origin은 경고 배지만. ISO-TP 상태기 오염 방지 |
| 미결 Q4 주기 PDU(§5) | **수용.** TxScheduler가 호스팅. 정지 조건 = 소유 클라이언트 연결 끊김 **또는** lease 만료(먼저 오는 것). 트레이스에는 `via=periodic` 로 보이고 뷰어에서 기본 접기 |
| §4 isotp 그룹 / functional SF 제한 | 수용. `open_group` 은 멤버별 can-isotp 상태기 N개 + functional 송신 전용 SF 경로로 구현. 탐색(응답자 범위 수신)은 M4 |
| §2 소유권 `ERR_BUSY` | 동의(위 `AddressInUse` 를 `ERR_BUSY` 로 이름 통일) |
| §7 성능 | FC ≤ 수 ms·PDU 이벤트 ≤ 10 ms 는 M0 스파이크로 측정해 확정. F1–F9(µs STmin)는 best-effort 동의 |
| §2 `owner.origin` | 공통 3종(agent/user/device) + `script` 와 맞춤. 트레이스 프레임은 owner origin 승계 + `via=uds` |
| 일정 | UDS 걷는 뼈대가 CAN-TP를 쓰면 CAN M2 의 ISO-TP 최소판(normal 11bit, virtual)이 선행조건 — UDS M1이 DoIP 경로로 먼저 성립하는지 UDS 팀 확인 필요 |

**UDS 팀에 질문:** (1) udsoncan `Connection` 어댑터를 UDS 팀이 쓸지 CAN 팀이 제공할지, (2) 기능 주소 요청 후 다중 응답(여러 ECU) 수집 방식 — 하나의 functional 채널 + 여러 rx_id 리스너가 필요한지, (3) 플래시 시 처리량 요구(KB/s)와 FD 사용 여부, (4) ISO-TP 레벨 트레이스를 UDS 뷰가 직접 그릴지 CAN Trace에서 묶어 보여줄지.

### 15.3 viz 요구 수용 구조 (UART/UDS → CAN)
- `_xteam/viz-requirements.md` 에 UDS 절(V-UDS-1~5) 작성됨(2026-11-09 확인), UART 절은 아직 없음. V-UDS-2(long 형식 `{ts, series, value, unit}`)는 아래 표준 입력 계약과 일치, V-UDS-1/4 는 웹 패널 플러그인(M3), V-UDS-5(ISO-TP→UDS 묶음 주석)는 Trace 패널 기능(M3~M4). 수용 구조 제안:
  - 소스 플러그인: `register_source("uart", open_fn)` — 도메인 이벤트 토픽 → 열 데이터 변환기(예: UART 줄 → regex 그룹 → 수치 열, UDS DID 응답 → 스케일 적용 수치).
  - 표준 입력 계약: JSONL `{"ts": <epoch s 또는 ISO8601>, "<name>": <num|str|bool>, ...}` 또는 롱 형식 `{"ts","name","value","unit"}` — 어느 도메인이든 이 형식으로 `loom viz push`/파일 저장하면 자동 렌더.
  - 패널 플러그인: UDS 요청/응답 트레이스 표, UART 터미널 미러는 각 팀이 패널로 기여(CR8).
- 예상 요구: UART 정규식 추출 플롯, UDS DID 폴링 추이, UDS 요청/응답 타이밍 표, 크로스 도메인 동기 타임라인 → 모두 위 구조로 수용 가능.

### 15.4 타팀 의존성 요약
| 상대 | 우리가 필요 | 우리가 제공 |
|---|---|---|
| UDS | ISO-TP 사용 패턴/처리량 요구, 가상 UDS ECU(SimNode에 붙일 in-proc 서버), 서비스 위험 등급 | IsoTpService, TesterPresent 주기 태스크, Trace의 ISO-TP 묶음 표시, viz 패널 플러그인 API |
| UART | viz 요구, TUI 단축키 합의 | viz 소스/패널 API, 공통 데몬·정책 엔진(공통 소유 결정 필요) |
| 메인 | CR1~CR8 결정, `loom.core` 소유자 지정 | 데몬/IPC/정책 설계 초안 |

### 15.5 스킬 설계 (`.claude/skills/`)

**구조**
```text
.claude/skills/
  loom/                     # (CR6) 공통: 데몬, 출력 규칙, 승인/안전 공통
    SKILL.md
  loom-can/
    SKILL.md                # ≤ 90줄
    references/
      cli.md                # 전체 명령·인자·출력 예·종료 코드
      workflows.md          # 시나리오별 명령 시퀀스 (A1~A6 확장)
      safety.md             # 위험 등급, 승인 흐름, 금지 행동, vehicle 규칙
      signals-and-dbc.md    # DB 로드, 신호 이름 찾기, 멀티플렉스/값 테이블/범위, wait-for 식 문법
      hardware.md           # 백엔드별 채널 지정, Vector app 설정, CANoe 공존, PCAN mac, 트러블슈팅(에러 코드 → 조치)
      isotp.md              # ISO-TP 디버그 명령 (UDS 스킬로 리다이렉트 안내)
      json-schemas.md       # --json 출력 스키마 (파싱용)
  loom-viz/
    SKILL.md                # ≤ 60줄
    references/
      inputs.md             # 지원 소스/형식, 컬럼 자동 인식 규칙, 표준 JSONL 계약
      layouts.md            # --layout JSON, --group, 패널 종류
      recipes.md            # "로그에서 X 그래프", "두 신호 비교", "UART 숫자 플롯", "DID 추이"
```

**`loom-can/SKILL.md` 골자 (초안)**
```markdown
---
name: loom-can
description: Read, decode, send, and record CAN/CAN FD traffic through the Loom daemon (Vector, PCAN, virtual). Use for CAN trace queries, DBC signal values, waiting for signal conditions, periodic/IG-style sending, bus statistics, and recording. The user watches the same bus live in the Loom viewer.
---
# Loom CAN
Always: the daemon owns the hardware; you use `loom can ...` CLI only. Never write python-can scripts that open devices.

## Workflow
1. `loom can status` — channels, env (sim/bench/vehicle), tx policy, DB.
2. No DB? `loom can db load <file.dbc> --channel chX`. Find names: `loom can db find <word>`.
3. Observe first: `sig get`, `trace --since @last --max 50`, `stats --window 10s`.
4. Wait, don't poll: `sig wait-for "Sig > 10" --timeout 30s`.
5. Send (R2+): `--dry-run` first → tell the user one line → run. Periodic: note task id, `periodic stop --mine` when done.
6. Show, don't dump: >50 values or a trend → `loom viz show ...` and give the user the URL.

## Rules
- Output is capped; narrow with --id/--msg/--since. Use `--json` when you parse.
- exit 4 = policy denied → report, don't retry. exit 5 = needs user approval in the viewer → ask the user, wait.
- Never edit loom.toml to relax policy. Never send on `env=vehicle` unless the user explicitly asked in this conversation.
- The user must see what you do: if `status` says `viewers: 0`, tell the user to run `loom can monitor` (or open the URL) before your first send.
- If output shows `NOTE: user sent ...`, the user intervened — mention it and adapt; don't overwrite their action silently.
- Emergency: `loom can stop-all`.

## References
cli.md · workflows.md · safety.md · signals-and-dbc.md · hardware.md · json-schemas.md
```

**`loom-viz/SKILL.md` 골자**: 사용 시점(추이·비교·대용량은 그리기, 단일 값은 텍스트), 한 줄 명령 3개(`show file`, `show live:...`, `push`), 반환 URL을 사용자에게 전달, 에이전트 자신은 `viz summary --json` 으로 수치 요약 확인.

**Codex 호환 방안**
- 스킬 본문을 도구 중립으로 작성(“Bash로 `loom ...` 실행”만 가정, Claude 전용 도구명 언급 없음). 실제 인터페이스는 CLI이므로 어떤 에이전트든 동작.
- Codex의 스킬 디렉터리 규약/프론트매터 호환 여부는 **미확인** → 대안 2개 준비: (a) `loom skills export --target codex` 가 SKILL.md+references를 Codex가 읽는 위치/형식으로 복사(사용자 설정 파일은 건드리지 않고 프로젝트 로컬로), (b) 저장소 `AGENTS.md` 에 "CAN 작업 시 `.claude/skills/loom-can/SKILL.md` 를 먼저 읽어라" 한 단락.
- `loom help --agent can` 이 SKILL.md 요약을 출력 → 스킬 시스템이 없는 에이전트도 자가 발견 가능.

---

## 핵심 원칙 체크리스트 자기점검

`docs/plan/02-core-principle.md` 의 6개 항목 기준.

- [x] **핵심 루프 절이 맨 앞에, 4요소별 구현 표** — §0 (명령 / 데몬 구성요소 / 사용자 화면 / 첫 마일스톤)
- [x] **종단 간 시나리오 1개 이상** — §2.0 (에이전트 `loom can send 0x300#01` → sim BCM 0x301 응답 → TUI/웹에 AGT/DEV 색으로 < 200 ms → 사용자 TUI `:send 0x300#00` 개입 → 에이전트가 `trace --since @last --origin user,device` 로 USR 프레임과 NOTE 확인). 보조: §2.1 A1~A6, §2.2 U2/U3
- [x] **실시간성 목표치 + 고부하 동작** — §3.12 (웹 Trace p95 < 150 ms, TUI < 200 ms, Graphics/Data < 200 ms 등; 기록 무손실·화면 예산제, `N frames skipped` 합성 행, decimation 표시, LAG 표시·라이브 점프, DRIVER OVERRUN 구분). 측정은 §9 핵심 루프 불변식 테스트, 리스크 §13 R13
- [x] **M1 = 걷는 뼈대** — §11 M1 (virtual + loomd + `send/trace` + TUI 라이브 + 사용자 `:send` + 최소 기록·안전), 뺀 항목과 이유 명시. 작업 분해 §12 M1 표와 임계 경로
- [x] **사용자 화면이 에이전트 세션과 분리되지 않음 / 숨은 연결 금지** — §3.11 불변식 I1~I6 (단일 경로, 비공개 토픽 없음, 구독 시 backlog, 에이전트 읽기도 Activity로 표시, viewers: 0 힌트, 사용자 개입 NOTE), §3.7 클라이언트 연결 표, 스킬 규칙 §15.5
- [x] **원칙에 기여하지 않는 기능은 후순위/비범위로 명시** — §1 우선순위 표(필수/강화/후순위/비범위) + 비범위 표, §11 M5 "후순위"

<!-- END R1 -->
