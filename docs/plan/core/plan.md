docs/plan/02-core-principle.md 준수

# CORE 계획 v2.0 — Rust 코어 (`loomd` · `loom` · worker 프로토콜 · 저널 · 정책 · 테스트 세션 코어)

작성: core-lead (R3 통합), 2026-11-10 · 입력: `r1-opus.md`, `r1-sol.md`, `r2-sol-review.md`, `r2-opus-review.md`, 스파이크 `spikes/core/`, 타팀 요청 `_xteam/{can,uart,uds}-core.md`, `core-ui.md`, `core-test.md`, `test-domains.md`, `ui-test.md`
우선순위: 마스터 **C > B > A > D** (C4.1 포함: DoIP = PC 랜카드 + 미디어 컨버터, VN5640은 CAN FD 전용)
표기: **[확인]** 공식 문서·소스·직접 실행으로 확인 · **[실측-mac]** macOS(Apple M1) 스파이크 수치 — Windows·Linux로 옮겨지지 않음 · **[추정]** 산술·설계 목표, 측정 전 · **[결정]** 이 계획의 선택(근거는 §16 결정 기록)

> 1순위 품질 목표(사용자 경험): **오래 켜 두어도 죽지 않고, 메모리가 늘지 않고, 장치→화면 지연이 낮은 코어.** 모든 큐는 상한과 넘칠 때 정책이 있고(§9.2), 원본은 디스크로 흘려보내며(§6), 벤더 DLL은 worker 프로세스에만 올린다(§3.1).

---

## 0. 핵심 루프 (코어 관점)

코어는 장치를 해석하지 않는다. 코어는 두 배관을 소유한다 — **장치 worker → 이벤트 허브 → (에이전트 CLI, 사용자 UI) 동시 전달**, 그리고 **쓰기 → 정책·승인·감사 → worker**. 이 배관이 "같은 데이터를 같은 순간에"를 깨지 않게 보장하는 것이 코어의 일이다.

### 0.1 4요소 표

| 요소 | 코어 부품 | 경로 | 코어가 보장하는 것 | 첫 수용 |
|---|---|---|---|---|
| ① 직접 접근 | `WorkerSupervisor`, worker 프로토콜(`loom-worker-sdk`), `Registry` | `loom <domain> open …` → gateway → worker spawn·handshake → worker가 OS 핸들 단독 소유 | 물리 장치는 worker(또는 순수 Rust gateway 모듈) 하나만 연다. CLI·UI가 장치를 여는 공개 경로 없음(불변식 I1). 벤더 DLL은 worker에서만 로드 | M1 |
| ② 읽기 | 허브(`loom-hub`: stream_seq·링·snapshot), 저널(`loom-journal`), CLI 킷(`loom-cli-kit`: 봉투·커서·상한) | worker RX → (worker 저널 ∥ data lane) → 허브 seq → 링 → `<domain>.read`·`core.events.query`(경계 있음) | 에이전트 읽기는 항상 행·바이트·시간 상한. 링 밖은 저널 유한 스캔. 생략·손실은 필드로 정직하게(gap 3층) | M1 |
| ③ 쓰기 | 쓰기 파이프라인(`loom-policy`), STOP latch | RPC → classify 훅 → 정책 → confirm/approval(원자 예약) → 감사 → presend 훅 → worker ctl `Write` → TX → write_state 이벤트 → **publish-before-reply** → 응답 | 모든 도메인·모든 origin의 쓰기가 같은 게이트. origin은 연결 자격으로 서버가 결정. 기록·감사 불능이면 agent·script R1+ 차단 | M1(R0–R2 + R3 웹 승인 최소), M2(전체) |
| ④ 사용자 실시간 표시 + 개입 | axum WS, 구독 관리자(원자적 snapshot + `after_seq`, 적응형 배치, 우선 제어 큐, `display.gap`), viewer role, 승인 대기열, `core.stop` | 허브 → 구독별 큐 → `core.events` → Loom UI. UI의 입력·STOP·승인은 viewer 연결 RPC로 같은 파이프라인 | 에이전트가 응답을 받는 시점에 그 결과 이벤트는 이미 모든 viewer 우선 큐에 들어가 있다. UI를 언제 열어도 snapshot+live로 같은 스트림. 에이전트 전용 숨은 토픽 없음 | M1 |

### 0.2 종단 간 시나리오 E1 — UART: 에이전트 송신 → 장치 응답 → UI → 사용자 개입

전제: Linux x64 벤치 PC. 에이전트는 herdr 패인의 Claude Code, 사용자는 같은 PC 브라우저에서 Loom UI(`http://127.0.0.1:<port>/`)의 UART 터미널 패널 `dut`를 보고 있다. 장치는 M1 sim `bootshell`, M2 실 `/dev/ttyUSB0`.

```
$ loom uart send dut "reboot\n"
sent 7 B to dut (gen 1) · seq 88124 · state=sent · 3 ms
NOTE: no other activity since your last call
```

| # | 홉 | 부품 | 예산 p95 [추정] | 그 홉의 큐 (상한 / 넘칠 때) |
|---|---|---|---|---|
| 0 | CLI 기동 → `daemon.json` → WS → `core.hello` → RPC 송신 | `loom`(동기 클라이언트) | ≤ 10 ms (macOS 실측 전체 p95 3.3 ms **[실측-mac]**, Windows 여유) | — |
| 1 | 파싱 → classify(R1) → 정책(auto) → 감사 intent `write()` → `requested→accepted` 발행 | gateway 파이프라인 | ≤ 2 ms | RPC admission 128건/2 MiB 전체, 클라이언트당 16 → `E_BUSY`(승인·STOP 예약 슬롯 별도) |
| 2 | gateway → uart worker ctl lane `Write` | worker 프로토콜 | ≤ 1 ms | ctl 송신 256 KiB/256건 → `E_WORKER_BUSY`(exit 7, 미전송 확정 `failed`) |
| 3 | worker TX → 드라이버 수용 → `sent`(PriorityEvents, ctl) | uart worker | ≤ 5 ms | 채널 TX 32건/256 KiB → admission 거절 |
| 4 | 허브가 `sent` 발행(모든 viewer 우선 큐 enqueue) → **그다음** RPC 응답 | 허브 | ≤ 2 ms | 구독 우선 큐 256 KiB/256건 → 드롭 금지, 넘치면 그 연결 종료 + 재snapshot |
| 5 | 장치 출력 → worker reader read 완료(`ts.source=host_read_complete`) | 장치·USB | (Loom 밖, 별도 분포로 보고) | 드라이버/OS RX(요청 4 MiB, 실제값 기록) → 넘치면 `source.gap` |
| 6 | worker data lane 적응형 배치(조용한 뒤 첫 청크 즉시, 이후 ≤ 10 ms) | worker-sdk | ≤ 10 ms | 화면 미러 4 MiB/8192건 → **화면만** 드롭 + `display.gap`(원본 seq 범위), 저널은 이미 기록 |
| 7 | 허브 seq·링·팬아웃 | 허브 | ≤ 2 ms | 허브 입력 8 MiB/16k건 → producer별 공정 드롭 + `display.gap` |
| 8 | 구독 배치(터미널 `max_batch_ms=10`, 조용한 뒤 즉시) + WS 송신 | 구독 관리자 | ≤ 13 ms (배치 10 + 송신 3) | 구독 data(유입률 × 500 ms, 1–4 MiB) → 오래된 것부터 드롭 + `display.gap{cause:"slow_client"}` |
| 9 | 브라우저 decode + xterm.js 그리기 완료 | UI | ≤ 33 ms | UI 팀 소유(패널 버퍼 상한 → gap 표시) |
| | **장치 read 완료 → 화면 (6–9)** | | **예산 합 ≤ 58 ms(코어 구간 6–8 = 25 ms), 목표 p95 < 100 ms** | 예산 합은 종단 p95 증명이 아니다 — §9.4 하네스로 실측 |

사용자 개입 (장치가 `Hit any key to stop autoboot` 출력 중):

| 개입 | UI 입력 | 코어 경로 | 목표 | 에이전트가 받는 것 |
|---|---|---|---|---|
| 부팅 중단 키 | 터미널 패널에 Space | viewer 연결 `uart.input` → **연결 태스크가 즉시** worker ctl lane에 넣음(허브·agent 요청 큐 우회, 쓰기 클래스 ③) | UI 키 → worker 수신 p95 ≤ 5 ms, flood 중 Ctrl-C → `sent` p95 < 100 ms | 다음 결과 `others_since_last{count:1, origins:["user"]}` + `NOTE: user sent 1 input since your last call` |
| 전 도메인 STOP | 상단 ■ STOP / `Ctrl-] x` | `core.stop` → **메모리 latch → 모든 worker `Latch`(ctl, IPC 스레드에서 원자 플래그 즉시 set) → ack** → latch 영속은 그 뒤 별도(§10.8) | worker당 ack p99 ≤ 10 ms, 전체 ack p95 ≤ 50 ms | 다음 쓰기 exit 6 `E_STOPPED` + `hint: user pressed STOP; ask before continuing` |
| R3 승인 | 승인 대기열에서 presence(확인 코드) 입력 → 승인 | viewer(web) `core.approval.respond` | 클릭 → 대기 RPC 재개 < 50 ms | `--wait-approval` 중이던 호출이 진행, 결과에 `approval_ref` |

### 0.3 종단 간 시나리오 E2 — CAN 고부하 중 UDS 요청과 R3 승인

1. `loom can open ch0 --sim --profile busload-100` — sim 버스 1 Mbit/s classic, DLC 0 표준 프레임 ≈ 20k fps [추정 산술: 47 bit+IFS ≈ 21.3k fps]. UI CAN Trace는 화면 예산(웹 ≈ 2,000 행/s)만 그리고 `⋯ 17,940 frames not shown (seq …) — recording ok` 합성 행 + `LIVE-DEGRADED` 배지. CAN FD 64 B·BRS 8 Mbit/s 부하는 별도 프로파일(≈ 5.7–10.3k fps, nominal/data 비트레이트와 ID/DLC 패턴 명시 — `r1-sol.md` §9.1).
2. `loom uds security --level 1 ecu1` → classify R3 → `core.approval_requested`(우선 큐 → 모든 viewer) → CLI는 `--wait-approval 120s`면 대기, 아니면 exit 5 `E_APPROVAL_REQUIRED` + `approval_id`.
3. 승인 → UDS 세션 관리자(gateway 모듈) → `can.isotp.send`(CAN worker ctl lane — raw 프레임 data lane과 물리적으로 다른 연결) → `uds.tx` → `uds.rx`(우선) → UI UDS 콘솔 행 갱신. 보장 범위는 **응답의 host-ingest → 화면** p95 ≤ 100 ms / p99 ≤ 200 ms. 요청 → ECU 응답 시간은 버스 중재·드라이버 큐가 좌우하므로 별도 분포로 보고한다(IPC lane 분리가 버스 부하를 없애지 않는다).
4. 화면은 생략을 숨기지 않는다: Trace 생략 수, UDS 행은 우선 큐라 생략 없음, `record_health=ok`가 함께 보인다.

### 0.4 실시간성 목표 (코어 구간)

코어 구간 = **worker host-ingest `mono_ns` → WS 프레임 송신 완료**(같은 호스트 monotonic, §9.4). 화면 렌더는 UI 팀 예산. 전체(장치→화면) 게이트는 실제 브라우저 하네스로 따로 잰다 — **둘 다 M1 합격 조건**.

| 경로 | 코어 p95 | 코어 p99 | 전체 목표 |
|---|---|---|---|
| UART RX → WS 송신(터미널 구독 10 ms) | ≤ 25 ms | ≤ 50 ms | p95 < 100 ms, p99 ≤ 200 ms |
| CAN 프레임 → WS 송신(Trace 구독 40 ms 상한) | ≤ 60 ms | ≤ 120 ms | p95 < 200 ms, p99 ≤ 500 ms |
| 우선 제어 이벤트(응답·write_state·승인·STOP·상태·오류) → WS 송신 | ≤ 5 ms | ≤ 15 ms | UDS p95 ≤ 100 / p99 ≤ 200 ms |
| RPC 수신 → worker 도착(쓰기) | ≤ 3 ms | ≤ 10 ms | 에이전트 send → `sent` p95 < 20 ms |
| worker 1개 `Latch` 전달 → ack | ≤ 5 ms | ≤ 10 ms | (can-core W5) |
| STOP RPC → 모든 worker ack | ≤ 50 ms | ≤ 100 ms | 미응답 worker는 `stalled`로 표시, 성공이라 하지 않음 |
| 늦은 합류 `core.subscribe` → `snapshot_end` | ≤ 300 ms | ≤ 600 ms | 첫 화면 < 1 s |
| `loom <domain> <verb>` 왕복(데몬 실행 중, R0, 장치 시간 제외) | Linux·macOS ≤ 30 ms, Windows ≤ 60 ms | ×2 | (macOS 스파이크 p95 3.3 ms **[실측-mac]**) |
| 자동 기동 포함 첫 호출 | ≤ 1 s (Windows ≤ 1.5 s) | ≤ 2 s | — |

### 0.5 고부하에서 멈추지도 거짓말하지도 않기 (코어 규칙)

1. **기록과 화면은 다른 큐다.** 원본은 worker 안에서 바로 저널로 간다. 화면 경로(IPC → 허브 → 구독)는 상한 있는 별도 큐이고, 넘치면 화면만 드롭하고 `display.gap{producer_range | seq_range, count, cause}`로 말한다. 기록 손실은 `record.gap`, 장치 쪽 손실은 `source.gap` — 이름·필드·배지가 다르다(A13). worker에서 드롭돼 허브에 들어오지 않은 이벤트는 stream seq가 없으므로 **원본 seq 범위**로 보고하고 stream seq를 지어내지 않는다.
2. **우선 제어 이벤트는 드롭하지 않는다.** 우선 큐가 상한을 넘으면 조용히 버리지 않고 **그 연결을 끊는다**(`core.closed{reason:"slow_consumer"}`) → 재연결 + snapshot. STOP은 연결별 1칸 mailbox로 따로 보존(CR-C2).
3. **적응형 배치**: 조용한 구간 뒤 첫 이벤트 즉시, 바쁠 때만 ≤ `max_batch_ms`(기본 40, 터미널 10)·200건·64 KiB(A5). worker → gateway도 같은 규칙(≤ 10 ms, uart-core #2).
4. **lag 정직 표시**: 배치마다 `lag_ms`(배치 안 가장 오래된 이벤트 ingest → 송신)와 gap 누계. 구독 lag > 2 s면 backlog를 버리고 live로 점프 + `display.gap{cause:"jump_to_live"}`.
5. **heartbeat**: 데이터가 없어도 1 s마다 `core.heartbeat{stream_seq_hw, daemon_mono_ns, daemon_wall_ns, record_health, latch, sub_stats}` → UI가 `LIVE / LAG / STALLED / LIVE-DEGRADED`를 계산(장치가 조용한 idle을 LAG로 표시하지 않는다).
6. **축약·다운샘플은 화면용이고 표시한다.** 허브는 이벤트를 변형하지 않는다. `coalesce_by:"id"`(CAN Fixed 등) 구독은 `coalesced_count`를 싣고, 플롯 질의는 `downsampled{method, from, to}`를 결과에 명시. CLI·저널 원문은 그대로.

### 0.6 M1 = 걷는 뼈대, P0 = 스키마 + stub crate

- **P0(첫 주, M0)**: Day 1–2 `loom-proto` 타입·오류 코드·fixture + **compile-only** trait crate(`loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit`) 공개 → 도메인 팀이 바로 `cargo add`. Day 3–5 `loom-testkit`(인프로세스 FakeWorker·fixture assert), 견본 도메인 `loom-echo`, JSON Schema/TS 생성, **UI용 fixture 재생 stub `loomd --fixture-replay`**(core-ui U7). 상세 §3.9, §11.
- **M1**: 시뮬레이션 위에서 `loomd`가 echo + 세 도메인 sim worker/모듈을 띄우고, 에이전트 송신 → 허브 → **실제 브라우저의 최소 웹 UI**(UI 팀) → 사용자 입력·STOP·R2 확인·R3 승인까지. 수용 §11.
- 후순위·비범위 §1.3.

---

## 1. 범위 / 비범위

### 1.1 범위 (CORE 소유)

| # | 항목 | 핵심 루프 기여 |
|---|---|---|
| 1 | 프로세스 구조: `loomd` gateway + worker 프로세스, 배치 판정 기준 | ① ③ |
| 2 | worker 프로토콜(Rust worker와 Python 어댑터 공통), 감독·재기동·메모리 예산 집행 | ① ② ③ |
| 3 | 외부 IPC(JSON-RPC 2.0 over WS), hello/role, 이벤트 허브, 구독, 대용량 HTTP GET | ② ④ |
| 4 | 저널 라이브러리(worker에 링크)·stream mapping·인덱스·조회·재생·보존 | ② |
| 5 | CLI `loom` 프레임워크·코어 명령·자동 기동·봉투·종료 코드·커서 | ② ③ |
| 6 | 정책·확인·승인·감사·STOP·쓰기 파이프라인·도메인 훅 | ③ ④ |
| 7 | 스키마 정본(Rust) → JSON Schema → TS/Python 생성, drift 검사 | 전 팀 계약 |
| 8 | 러너 코어(`loom run`)와 테스트 세션 코어(`loom test`의 세션·마커·증거 검증·보고 데이터) | C7 |
| 9 | C2 성능·메모리: 큐 상한 표, 바이트 장부, 프로세스 예산, soak·크래시 격리, 지연 하네스 | 1순위 품질 |
| 10 | 빌드·배포·CI 3 OS·workspace 구조·도메인 경계 | 전 팀 |
| + | 공통 `loom` 스킬, `help --json` 체계, `loom skills sync`, `loom doctor` | C8 |

### 1.2 다른 팀 소유 (코어는 계약만)
- 도메인 장치 코드: UART(`loom-console` A4, 시리얼), CAN(PCAN·Vector XL CAN FD·SocketCAN 바인딩, ISO-TP, DBC), UDS(DoIP over OS TCP — C4.1, 세션 관리자, CAN-TP 사용), EXPLORE(DLT·SSH·LA). 도메인 risk table·훅 구현·sim.
- UI 앱(셸·패널 프레임워크·viz) — UI 팀. 코어는 `_xteam/core-ui.md`의 데이터 계약과 정적 파일 임베드만.
- TC 형식·`loom-test` 스킬·술어 AST 명세·보고서 템플릿 — TEST 기획(EXPLORE 팀). 코어는 세션 엔진·검증기·보고 데이터 API(`_xteam/core-test.md`).

### 1.3 후순위 / 비범위

| 항목 | 판정 | 이유 |
|---|---|---|
| 공유 메모리 링 IPC(iceoryx2·mmap) | 후순위(측정 후) | 로컬 소켓 왕복 p99 21–29 µs·100k ev/s 수신에 CPU ≈ 5 % **[실측-mac]**. Windows 이식·크래시 복구 복잡도 |
| 동적 플러그인(dylib) | 비범위 | Rust ABI 불안정. 도메인은 컴파일 시 등록, 외부 언어는 worker 프로토콜 |
| 원격 LAN bind·TLS·다중 사용자 인증 | 후순위(M5) | 기본 127.0.0.1 + SSH 터널. 같은 PC에서 원칙 충족 |
| OS 서비스 설치(systemd·Windows 서비스) | 후순위 | 자동 기동(D8)으로 충분, 로그인 시 기동은 선택(§14 Q3) |
| Arrow/Parquet 정본 | 비범위(export만) | 스트리밍 원본에 부적합 |
| 데스크톱 셸(Tauri) | UI 팀 후순위 | 코어는 같은 WS 프로토콜만 |
| 공통 TUI 키트 | **삭제**(CR-C9) | C6로 웹 UI가 1차. 터미널 뷰어는 도메인 선택, 코어는 `viewer-tty` role만 |
| 코어 내장 LLM·자연어 해석 | 비범위 | 해석은 에이전트·`loom-test` 스킬. 코어는 결정적 검증만 |
| Vector XL 이더넷·smoltcp | 비범위(C4.1) | DoIP는 OS TCP/IP |

---

## 2. 사용 시나리오

### 2.1 에이전트 관점

| # | 상황 | 명령 흐름 | 코어가 해 주는 것 |
|---|---|---|---|
| A1 | 첫 호출(데몬 없음) | `loom uart open dut --port /dev/ttyUSB0 --baud 115200` | 자동 기동(≤ 1 s), `warnings:["daemon_autostarted","no_viewer_attached"]`, `hint: ask the user to open the Loom UI (loom ui)` |
| A2 | 사용자가 중간에 개입 | `loom can send ch0 0x123#1122` → `NOTE: user sent 2 frames, pressed STOP since your last call` → exit 6 `E_STOPPED` | `others_since_last` 집계, STOP latch, 자동 재시도 없음. 스킬 규칙: 멈추고 사용자에게 묻는다 |
| A3 | 혼합 TC 실행 | `loom test start --tc tc/boot-can-dtc.tc.md --plan …plan.yaml --wait-approval 120s` → 단계마다 `loom test step --begin` / 도메인 명령 / `loom test check --step 2` / `loom test verdict --evidence …` → `loom test end` → `loom test report --html --junit` | 일괄 사전 승인, 단계 마커가 모든 도메인 타임라인에 찍힘, 인용 증거를 저널에서 재검증(§10.9) |
| A4 | 과거 구간 확인 | `loom events --since mark:boot --domain uart --max 100` | 경계 있는 조회, 링 밖은 저널 유한 스캔, `meta.truncated`·`next_cursor` |
| A5 | 데몬 재시작 중 쓰기 | `loom uds write-did …` 응답 전 데몬 크래시 | exit 9 `E_DAEMON` + `detail.outcome:"unknown"` + `request_id` → 재기동 후 `loom status --request <id>` = `outcome_unknown`. 재송신 금지 |
| A6 | 사용자에게 보여 주기 | `loom ui open can.trace --filter id=0x7E8 --at seq:90211` | `ui.focus_request` 이벤트 → UI가 비침입 토스트로 제안(강제 전환 없음) |

### 2.2 사용자 관점

| # | 상황 | UI | 코어가 해 주는 것 |
|---|---|---|---|
| U1 | 에이전트가 작업 중일 때 UI를 연다 | `loom ui`(또는 이미 연 브라우저) | snapshot(백로그·채널 상태·대기 승인·활성 job·입력권·latch·env·record health·진행 중 test/run) + live, 누락·중복 0 |
| U2 | 위험 동작 승인 | 승인 대기열 → digest 요약·대상·횟수·기한 → presence 코드 입력 → 승인 | 승인은 웹 viewer 연결만, presence는 agent 직렬화에 없음, 감사 해시 체인 |
| U3 | 이상하다 → 전부 멈춤 | ■ STOP | agent·script 쓰기와 모든 job latch(사용자 입력은 계속), "STOP — 출력 유지 중" 배지 + safe_state "복구 송신" 1키(A3·A15) |
| U4 | 장치 worker 사망 | 배너 `CAN worker crashed (exit -11) — restarting in 1s (2/5)` | 다른 도메인 스트림·저널 영향 없음, TX·job 자동 재개 없음 |
| U5 | 디스크 가득 | 빨간 `REC FAILED` 배너 | agent R1+ 쓰기 차단(exit 4), 사용자 입력 허용, 라이브 표시 지속(A12) |
| U6 | 느린 노트북 브라우저 탭 | 그 탭만 `LAG`/gap 배너 | 다른 viewer·에이전트·저널은 영향 없음(구독별 큐) |

---

## 3. 아키텍처

### 3.1 프로세스 구조와 배치 판정 [결정]

사용자에게는 `loomd` 하나(포트·URL·승인·감사 하나). 내부는 **gateway 프로세스 + 장치 worker 프로세스 N + (선택) Python 어댑터 worker**. 마스터 D1 유지.

| 안 | 판정 |
|---|---|
| 모놀리스(모든 도메인을 gateway crate로) | 기각 — 벤더 DLL 크래시 = 데몬 사망, 프로세스 단위 메모리 상한 불가(C2 위반) |
| 도메인마다 worker | 과함 — UDS↔CAN PDU가 worker↔worker, 홉·프로세스 증가 |
| **판정 기준에 따른 혼합** | **채택** |

**worker 판정 기준 — 하나라도 "예"면 worker**

| # | 기준 | 이유 |
|---|---|---|
| W1 | 벤더 네이티브 라이브러리(vxlapi64, PCANBasic, seed-key DLL, J-Link 등)나 Rust 밖 코드를 로드하는가 | 크래시·hang 격리(C2) |
| W2 | ms 단위 마감 타이밍 스레드가 있는가(ISO-TP FC/STmin, 10 ms 주기 송신) | gateway의 직렬화·질의와 CPU 경쟁 격리(CAN 실측 FC p99 1.5 → 52 ms) |
| W3 | OS가 취소할 수 없는 블로킹 호출이 있는가(USB-시리얼 드라이버 무기한 블록) | 스레드는 강제 종료 불가, 프로세스는 가능 |
| W4 | 독립 메모리 예산이 필요한가(대용량 export·디코드) | Job/cgroup은 프로세스 단위 |
| W5 | 다른 언어(Python)인가 | C1: 반드시 별도 프로세스 |
| 모두 아니오 | 순수 Rust, 소켓 기반, 저속·저CPU | gateway 모듈 |

**보조 기준(Sol)**: gateway 모듈이 지속 CPU > 1코어의 20 %를 쓰거나, 타이머 지터 관문을 못 넘으면 같은 trait의 worker로 옮긴다.

| 도메인 | 배치 | 근거 |
|---|---|---|
| CAN(PCAN·Vector XL CAN FD·SocketCAN·sim) | worker `can-hw` / `can-sim` | W1, W2 |
| UART | worker `uart-<port>` — **포트당 1 프로세스, 최대 8**(`WorkerSpec.instances = PerChannel{max:8}`, UART v2 DR-V13) | W3, 포트 간 크래시 격리(C2), uart-core #1 |
| UDS 세션 관리자 | **gateway 모듈**, `ModuleRuntime::Dedicated`(전용 런타임 스레드 1) | DoIP = OS TCP(C4.1), CAN-TP = CAN worker PDU RPC. **관문: 타이머 지터 p99 ≤ 2 ms(uds-core C-13), 넘으면 `uds-session` worker로 이동** |
| seed-key DLL | 별도 helper worker(32비트 DLL이면 i686 빌드), 호출 상한 2 s | W1, uds-core C-8 |
| DLT(TCP), SSH | gateway 모듈 | 순수 Rust 소켓 |
| LA(Saleae gRPC), 디버그 프로브 | worker | W4, W1 |
| CANoe COM(P4 선택) | Python 어댑터 worker(Windows) | W5. CAN 결정 기록에 "왜 Rust로 안 되는가" 필수(C1) |

### 3.2 프로세스·스레드 구성도

```
 사용자: 브라우저 Loom UI (viewer, 쿠키)        에이전트: Claude Code/Codex → `loom` CLI (agent, 단발, 동기)
   │ HTTP GET(정적·대용량) + WS /rpc                 │ WS /rpc (daemon.json 토큰)
   └──────────────────────┬──────────────────────────┘
┌─ loomd (gateway) ───────▼──────────────────────────────────────────────────────────────────┐
│ tokio 멀티스레드 런타임                                                                      │
│  ├ axum: /rpc(WS) · /(rust-embed 정적 UI) · /api/v1/{query,raw,report}(토큰·쿠키·Origin)  │
│  ├ 연결 태스크(연결당 1, **단일 writer**, send timeout 100 ms): hello → role → 디스패치      │
│  │    └ viewer 입력 빠른 경로: `<domain>.input`은 스폰 없이 worker ctl에 바로                │
│  ├ 쓰기 파이프라인(요청당) · 승인 서비스 · 정책 로더(notify) · 러너/테스트 세션 코어         │
│  ├ 도메인 gateway 모듈: uds(전용 런타임), can 질의·SignalCache, uart(얇음), dlt, shell …    │
│  ├ WorkerSupervisor + worker당 ctl/data lane 태스크                                          │
│  └ 질의 풀(blocking 4, pending 16/4 MiB)                                                      │
│ 전용 OS 스레드                                                                               │
│  ├ [hub] 시퀀서 1: stream_seq 부여 → 링 → 리듀서 → 구독 팬아웃(단일 소유 = 원자적 snapshot) │
│  ├ [map] stream mapping 로그 writer: (stream 범위 ↔ producer 범위) **정본**, 그룹 sync       │
│  ├ [journal-gw] gateway 이벤트 저널 writer · [audit] 감사 writer · [seal] 압축·GC(낮은 우선) │
└─────┬───────────────────────────┬──────────────────────────────┬──────────────────────────┘
      │ ctl + data (로컬 소켓 2개) │                              │
┌─────▼──────────────┐  ┌─────────▼────────────────┐  ┌──────────▼─────────────┐  ┌─────────────────┐
│ loomd worker uart  │  │ loomd worker can (hw/sim) │  │ loom-adapter-canoe(Py) │  │ seedkey helper  │
│ 포트당 reader 스레드│  │ 채널당 RX 스레드           │  │ COM STA 스레드(P4)      │  │ (i686 가능)      │
│ TX actor           │  │ TX 스케줄러·ISO-TP 스레드  │  │ 같은 worker 프로토콜    │  └─────────────────┘
│ loom-console 엔진  │  │ (도메인 전용 OS 스레드)     │  └────────────────────────┘
│ 저널 writer 스레드 │  │ 저널 writer 스레드         │
│ SDK IPC 스레드     │  │ SDK IPC 스레드(current_thread)│
└────────────────────┘  └───────────────────────────┘
```
- 허브를 tokio 밖 단일 스레드로: stream_seq 부여·링 append·구독 등록이 한 단계 → snapshot 원자성, 락 없는 순서, 런타임 혼잡과 격리. 직렬화는 1회(JSON `Arc<[u8]>` 공유), role별 가림본(presence·비밀값)만 따로.
- worker는 같은 실행 파일 `loomd`를 `loomd worker <kind> --instance <id>`로 재실행 → 항상 같은 빌드. 벤더 DLL은 worker 모드에서만 `libloading`.

### 3.3 worker 프로토콜 `loom-worker/1` [결정]

**전송**: 이름 있는 로컬 소켓 — Unix domain socket(Linux·macOS, 디렉터리 0700) / Windows named pipe(DACL = 현재 사용자, 원격 pipe 거부). `interprocess` 2.x 기본, 문제 시 tokio 직접(UnixStream·`windows::named_pipe`). [확인] 두 crate 모두 3 OS 매핑 문서화. TCP 127.0.0.1은 타 사용자 차단이 어려워 기각, stdio는 재부착 불가로 기각.

**두 물리 연결**: `ctl`(요청·응답·write_state·latch·입력·하트비트·우선 이벤트·PDU) / `data`(대량 RX 배치). 큰 data 배치가 STOP·ISO-TP PDU를 막지 않는다(can-core W1). **ctl 안에서도** STOP·Ctrl-C가 긴 Call 뒤에 갇히지 않도록: 프레임 상한 64 KiB(최악 대기 = 1프레임), worker SDK IPC 스레드가 `Latch`·우선 `Input`을 큐에 넣지 않고 즉시 처리(원자 플래그·우선 mailbox).

**직렬화**: MessagePack(`rmp-serde` 1.3.x) **map + 명시 kind 문자열, 바이트는 bin(`serde_bytes`)**. Python 어댑터와 같은 프로토콜(브리프 2)·스키마 진화가 근거. 같은 serde 타입이 JSON(u64=문자열, bytes=hex/base64)과 MessagePack(u64=정수, bytes=bin)을 동시에 만족함 **[실측-mac, opus-repr]**. 튜플(array) 인코딩은 크기 1/3이지만 고정 레이아웃 golden·형식 hash 협상 후 M2 이후 옵션. postcard/bincode는 Python 구현 부재·bincode 유지보수 종료 [확인]로 기각, protobuf/FlatBuffers/Cap'n Proto는 정본 이중화(C3)로 기각.

**프레이밍**: `u32 LE len | u8 msg_type | u8 flags | u16 reserved | payload`. **모든 프레임 ≤ 64 KiB**, 길이는 할당 전 검사, read deadline·잘린 프레임·모르는 tag 시험. 큰 PDU·raw는 chunk(id/offset/total/sha256, 재조립 상한 4 MiB/대상) 또는 `raw_ref`.

**메시지 집합** (상세 필드는 `r1-opus.md` §3.3 표를 v1 초안으로 사용)

| 방향 | lane | 메시지 |
|---|---|---|
| W→G | ctl | `Hello{proto{major,minor}, kind, instance, producer_id, producer_epoch, pid, build, schema_hash, codecs, domains, capabilities, journal_dir, resume_nonce?}` |
| G→W | ctl | `Welcome{accepted_minor, stream_epoch, config, budgets, latch}` / `Reject{reason}` |
| G→W | ctl | `Call{call_id, method, args, origin, client_id, actor_id, request_id, deadline_mono_ns, approval_ref?}` · `Write{…, plan_digest, approval_ref?}` · `JobStart/Update/Renew/Stop` · `InputLease`/`Input` · `Latch{on, scope, reason, latch_seq}` · `Credit{bytes}`(data) |
| W→G | ctl | `Reply{call_id, ok|err, covers{producer_epoch, source_seq}}` · `PriorityEvents[…]` · `Health{mono_ns, rss, commit, queues, drops, journal{written_through, durable_through, health}, threads}`(1 s) · `LatchAck{latch_seq}` · `Segment{sealed …}` |
| W→G | data | `Events{first_source_seq, count, records[]}` |

**publish-before-reply** [결정, Sol F4 반영]: worker는 쓰기 결과를 ctl `PriorityEvents`로 보낸 뒤 `Reply{covers}`를 보낸다(같은 FIFO 소켓이라 순서 보장). gateway는 허브 발행 워터마크 ≥ `covers`일 때만 성공 응답을 보낸다. **의미 = 허브가 seq를 부여하고 모든 viewer 우선 큐에 enqueue 완료.** 소켓 write 완료나 브라우저 렌더는 보장하지 않는다(렌더는 UI의 `core.metrics.report`·`rendered_through_seq`로 따로 관측). 허브 입력에는 우선 이벤트 **예약 용량**을 두어 정상 경로에서 enqueue가 실패·대기하지 않게 한다(Opus S3 — 타임아웃 경로를 정상 흐름에서 제거). 허브 스레드 자체가 멈춘 이상 상황에서만, RPC deadline 안에 발행하지 못하면 정상 성공으로 응답하지 않고 **exit 8 `E_PARTIAL`{write_state:"sent", published:false}** — 장치 동작은 일어났으므로 `failed`로 바꾸지 않고 재시도도 권하지 않는다. 시험 T2가 이 정의를 검사한다.

**흐름 제어**: data lane 바이트 크레딧, 창 = 최대 유입률 × 100 ms(기본 1 MiB, worker별 설정). 크레딧이 없으면 worker는 **장치 수신을 막지 않고** 화면 미러(4 MiB/8192건)에 쌓다가 넘치면 화면 전달만 버리고 `display.gap{producer_range, cause:"ipc_backpressure"}`를 ctl로 보고. 저널 기록은 크레딧과 무관(can-core W2·W3). ctl 수신 측(Q6)이 밀릴 때 block은 SDK IPC 스레드에서만 허용 — 도메인 `Emitter`는 절대 block하지 않고 실패를 반환한다(RX·FC 스레드 보호).

**버전 협상**: major 불일치 → `Reject` + `core.worker_state{incompatible}`(그 worker만 비활성). minor = 교집합. Rust worker는 항상 같은 빌드, 협상은 Python 어댑터·향후 분리 배포용. `schema_hash` 불일치는 경고.

**감독·재기동**

| 상태 | 전이 | 동작 |
|---|---|---|
| starting → ready | Hello/Welcome ≤ 5 s | `core.worker_state` |
| ready → suspect → stalled | Health 3회(3 s) 누락 → 5 s | 노랑 배너, 신규 쓰기 수락 중단 exit 7 `E_WORKER_DOWN` |
| stalled → killed | 10 s 무수신 또는 hard 메모리 초과 | TerminateJobObject / SIGKILL |
| 종료 감지 | exit / ctl EOF | 진행 중 쓰기 `outcome_unknown` 확정·감사, 열린 세그먼트는 재기동 worker가 tail 복구 후 `record.gap`(범위, 모르면 `count:"unknown"`) |
| 재기동 | 백오프 250 ms → 500 ms → 1 → 2 → 4 … 최대 30 s, ±20 % jitter | 채널 재open은 `WorkerSpec.reopen_after_crash`: `None`(UART 기본 — DTR 리셋 위험) / `RxOnly`(CAN, can-core W9 — **`autostart` 채널만, 장치 identity 재검증 후** RX 재개; D8 "데몬은 장치를 자동으로 열지 않음"과 일치). **job·주기 TX·ISO-TP 송신은 어느 경우든 자동 재개 금지** |
| quarantined | 60 s 안 5회 크래시 | 재기동 중단, 빨간 배너 + viewer `core.worker.restart`, 에이전트 exit 7 `{quarantined:true}` |
| gateway 사망 | worker가 ctl EOF 감지 | **즉시 TX latch**, 저널 계속, orphan 대기(기본 30 s) |

**gateway 재부착(can-core W6)**: M1은 orphan 모드만 — latch + 저널 계속 + 30 s 뒤 seal·종료. 다음 gateway는 새로 spawn(장치 재open은 `reopen_after_crash` 규칙). **M1에서 W6은 미충족**이며 orphan worker가 살아 있는 동안은 저널이 계속되지만, worker 종료 후 새 worker가 장치를 다시 열기까지의 수신 공백은 Loom이 받기 전 손실이므로 `source.gap{cause:"gateway_restart"}`로 기록한다(can-core v2.0 갱신과 일치). M2에서 재부착: worker discovery 파일 + 단일 인스턴스 lock + `resume_nonce` 인증 → `Hello{resumed:true}`. Windows Job은 `KILL_ON_JOB_CLOSE`를 쓰지 않는다(orphan seal을 위해) — 대신 worker 자체 deadline.

**메모리 예산 집행**(§9.3 수치)

| OS | hard | soft·관측 |
|---|---|---|
| Windows | Job Object `ProcessMemoryLimit` — **commit 한계**이지 RSS 한계가 아님 [확인] | `GetProcessMemoryInfo` WorkingSet·PrivateUsage 1 s 샘플 |
| Linux | 위임된 cgroup v2 있으면 `memory.high/max` + `memory.events`, 없으면 `RLIMIT_DATA` 백스톱(가상 크기 기준, RSS 한계가 아님) | `/proc/<pid>/status` VmRSS·RssAnon |
| macOS(개발) | 없음 | `task_info` phys_footprint, watchdog |
- 단계(can-core W8): soft 10 s 지속 → `core.worker_state{mem:"pressure"}` + 화면 미러·구독 축소 → 60 s 지속 또는 hard → latch → seal 시도 → 재기동 → gap·`outcome_unknown`. gateway는 hard 전에 신규 구독·질의 admission을 막고 제어 슬롯은 남긴다.

**Python 어댑터**: 코어가 `adapters/python/loom_worker_sdk/`(프레이밍·Hello·Health·Latch·covers)를 제공, 타입은 JSON Schema에서 생성. 어댑터는 정책을 판단하지 않는다. **P0에서는 wire golden fixture(Rust↔Python 동일 바이트)만 고정**, 실행 SDK 완성은 P4 CANoe 선행 작업.

### 3.4 외부 IPC와 이벤트 허브 [결정]

**외부 IPC(D2 유지)**: axum 0.8 + WS, **제한된 JSON-RPC 2.0 자체 dispatcher**(공식 오류 코드 −32700/−32600/−32601/−32602, id echo, notification 무응답을 규격 fixture로 시험, batch 배열은 M1 거부). jsonrpsee는 불가능하다는 확인이 없으므로 같은 fixture를 통과하는 대안으로 남긴다. 127.0.0.1 단일 포트, `/rpc`·`/`·`/api/v1/*`. **UI 오리진 정본 = `http://localhost:<port>`**(바인딩은 127.0.0.1, `127.0.0.1:<port>` 접속은 localhost로 리디렉트 — WebAuthn RP ID는 IP 불가, CR-UI-10). Origin/Host allowlist = `localhost:<port>`·`127.0.0.1:<port>`. 응답 헤더 CSP·`Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()`·`Referrer-Policy: no-referrer`. 메시지 64 KiB(클라이언트→서버, `core.events` 배치 1개), ping 20 s, 연결당 단일 writer + send timeout 100 ms.

**hello와 role**

| 연결 자격 | role | 할 수 있는 것 |
|---|---|---|
| `daemon.json` 토큰(0600) | `agent` | 도메인 RPC(origin=agent), 읽기, `core.confirm`(R2 토큰), 승인 **목록·대기만**, `core.stop`(멈추기는 누구나) |
| 웹 bootstrap → HttpOnly·SameSite=Strict 쿠키 | `viewer` + **rights ∈ {watch, operate, approve}**(core-ui CR-UI-07): TTY `loom ui` bootstrap = operate+approve, `loom ui pair`(원격·TTY) = watch 기본. 입력·STOP 해제·재시작은 operate 이상, 승인·정책 완화·판정 수정은 approve | + 사용자 입력(origin=user), R2 viewer 확인, R3/R4 승인, STOP 해제, 정책 완화 승인, worker 재시작, 판정 수정(`test.verdict` override) |
| (M2, 도메인 TUI가 남을 때만) 대화형 TTY 뷰어 + 토큰 | `viewer-tty` | 사용자 입력·hold·STOP만. **모든 확인·승인·해제·정책·재시작은 웹 전용** — 에이전트도 PTY에서 돌 수 있어 TTY로 사람을 판정할 수 없다. **M1은 웹 viewer만**(uart-core #5가 브라우저 세션으로 전환) |
| 러너·테스트 세션 내부 | `script` actor | origin=script, `actor_id=run:<id>|test:<id>` |
- 요청 필드의 origin은 무시(T5). bootstrap 코드는 `loom ui`가 OS 브라우저로 **URL fragment → POST 교환**으로만 전달(stdout이 TTY가 아니면 출력하지 않음), 1회용·60 s(uds-core C-9, uart-core #5). UI 초안의 `GET /bootstrap?t=` 형식은 `no-store`·`Referrer-Policy: no-referrer`·로그 제외·즉시 리다이렉트 조건으로만 허용 — 최종 형식은 `core-ui.md`에서 UI 팀과 확정.
- 승인 객체는 `presence{method: code|webauthn|oob, proof}` 필드를 가진다(CR-UI-07). M1 = `code`(viewer 화면에만 보이는 짧은 코드 R3 2자/R4 4자 — **오클릭 방지용**, 사람 존재 증명 아님). M2 = WebAuthn 옵션: 공개키 등록(**등록 게이트** = TTY 1회 코드 또는 기존 자격증명) + assertion 검증, 감사에 승인 방법·UA·`navigator.webdriver` 기록. 등록 게이트가 있는 WebAuthn은 "돕겠다고 브라우저를 조작하는 자동화"를 실제로 막는다. rights는 M1.

**이벤트 허브**
- **두 층 순번(D3)**: worker `(producer_id, producer_epoch, source_seq)`, 허브 `(stream_epoch, stream_seq)`. 허브는 배치마다 **stream mapping 로그**에 `(stream_epoch, seq_first, seq_last, producer_id, producer_epoch, src 목록 또는 범위, mono_first, mono_last)`를 쓴다. 이것이 정본이다(Sol F3) — 원본만으로는 producer 간 전역 순서를 복원할 수 없으므로 드롭 가능한 캐시가 아니다. mapping이 durable하지 않은 구간의 `seq:N` 커서는 `uncertain`으로 표시하고 source ref로 재조회한다.
- **링**: 바이트 예산 고정(기본 32 MiB, 채널별 ≤ 4 MiB), 오래된 것부터 제거. 링 = 화면용, 원본 = 저널(C2).
- **상태 리듀서 = snapshot provider**(uds-core C-5, core-ui U2): 도메인이 `Reducer` 등록 — UART 화면 백로그(줄 경계), CAN Fixed·신호 최신값(화면 드롭 후에는 `quality:stale` + 복구 질의), UDS 대상 상태·최근 txn, 승인 대기, 입력권·hold, latch, env, record health, worker 상태, test/run 진행.
- **구독**: `core.subscribe{filter{domains, channels, kinds, origins}, after_seq? | snapshot:true, backlog{max_events, max_bytes, max_age_ms}, max_batch_ms, encoding:"json"|"binary", coalesce_by?}` → `{sub_id, stream_epoch}` → `core.snapshot_part{n,N}`… `core.snapshot_end{stream_seq:H}` → `core.events{sub, first, last, events[], display_gap, lag_ms}`.
  - snapshot은 허브 턴 H에서 **불변 참조**로 캡처하고 직렬화는 밖에서. 전송 중 들어오는 live는 구독별 pending ≤ 1 MiB/2 s, 넘으면 그 snapshot 취소 + gap + 재시도(동시 snapshot 2개 상한).
  - `after_seq`가 링 안이면 이어 보냄, 밖이면 `display.gap{cause:"resume_out_of_ring"}` + snapshot. epoch가 다르면 `core.epoch_changed{old,new}` + snapshot.
  - **셸 전역 제어 구독**(core-ui U2): **viewer 연결마다** 1개 자동(agent 단발 연결에는 없음) — 응답·write_state·승인·STOP·상태·오류·gap. 데이터 배치보다 먼저 별도 프레임.
- **core-ui v0.2 추가(CR-UI-08·11·13)**: `core.latch` payload = `{on, by, origin, jobs_stopped[], approvals_cancelled[], driver_uncancellable, outputs_held[], missing[], persisted}`. `core.agent_cursor{client_id, from_seq, to_seq, others_count}` = 에이전트가 사용자 개입을 전달받은 지점(UI `⇢ 전달됨`). `core.activity`에 읽은 범위 `range{seq_a, seq_b}`·대기 술어. snapshot·리듀서 공통 **`control{channel, owner_origin, actor, kind, reason, ttl}` 뷰**(도메인 lease·lock·job owner 정규화, 이벤트는 도메인 것 그대로).
  - 필터로 빠진 seq는 손실이 아니다(불연속 seq는 명시 seq 열로 전달).
- **role 가림**: 이벤트를 숨기는 기능은 없다(I2). 필드 단위로 presence·opt-in 비밀 원문만 viewer 직렬화본에.

**UI 고속 전송**: JSON 텍스트가 기본(M1). `encoding:"binary"`는 협상 opt-in으로 M2에 `loom.columns.v1`(헤더 magic/version/sub_id/batch_id/epoch/count/first·last seq/publish mono·wall/flags + 열 본문, 필터 후 seq는 delta 열 필수, 정수 LE, BigInt 경계 명시). 근거: 4 클라이언트 WS 수신 p99 JSON 0.90–1.12 ms·57.5 KB/배치 vs 고정 열 0.32–0.43 ms·14.4 KB/배치 **[실측-mac, 축약 합성 이벤트·Rust 수신, 브라우저 아님]**. 제어·쓰기 최종 결과는 binary 프레임에만 싣지 않는다. 상세 계약은 `_xteam/core-ui.md` `## core 의견`.

### 3.5 데이터 흐름 4경로

**(R) 읽기**
```
장치 ─▶ worker reader: read → ts{mono_ns, wall_ns} → source_seq
        ├─▶ [worker 저널 writer] 세그먼트 append (IPC·gateway와 무관)
        ├─▶ worker 단기 링(expect·ISO-TP 등 도메인용)
        └─▶ 화면 미러 → data lane(적응형 ≤ 10 ms, 크레딧) ─▶ gateway
               ─▶ [hub] stream_seq → JSON 1회 직렬화 → 링 → 리듀서 → mapping 로그
               ─▶ 구독별 필터 → data 큐 → 배치기 → WS ─▶ UI / `loom events --follow`
에이전트 질의 ─▶ 링 → (밖이면) mapping+인덱스로 세그먼트 → 유한 스캔(바이트·시간 상한, E_SCAN_LIMIT + resume)
```

**(W) 쓰기(요청형)** — 의사코드 §10.3
```
RPC ─▶ role→origin ─▶ classify 훅 ─▶ 정책(차단 3층→레이트→한계→경로)
   ─(deny)→ 감사 denied + `failed` 행 → exit 4
   ─(confirm/approve)→ 토큰 exit 5 | 승인 대기
   ─▶ 승인 사용권·idempotency **원자 예약** ─▶ 감사 intent ─▶ `accepted` ─▶ presend 훅(실패: 예약 해제·승인 무효)
   ─▶ worker `Write` ─▶ latch 재확인 → TX → `sent|failed|outcome_unknown` + Reply{covers} ─▶ 사용권 **확정 소비**
   ─▶ 허브 워터마크 ≥ covers ─▶ 감사 result ─▶ 응답(봉투 + others_since_last)
```

**(S) STOP** [결정, Sol F7 반영]
```
UI ■ / Ctrl-] x / `loom stop-all`
 ─▶ core.stop ─▶ ① 메모리 LatchState.on, latch_seq++ (파이프라인 즉시 거절 시작)
 ─▶ ② 모든 worker ctl `Latch` (IPC 스레드가 원자 플래그 즉시 set, job 단위 송신 중단, 대기 쓰기 취소) ─▶ LatchAck
 ─▶ ③ 허브 `core.latch{on, by, origin}`(우선) ─▶ ack 집계(≤ 200 ms, 미응답 = stalled + `core.latch{missing:[…]}`)
 ─▶ ④ 별도로 latch.json 영속(fsync). 실패해도 STOP은 유효 — `core.latch{persisted:false}` 배너
해제: web viewer 전용 `core.stop.release` → 중단된 job은 재개하지 않는다.
재기동: latch.json이 on이거나 **읽을 수 없거나 불명이면 latched로 시작**.
```

**(A) 승인**
```
R2(token 경로) ─▶ ConfirmToken{tok, digest, client_id, ttl 120 s, stored_request} ─▶ exit 5 ─▶ `loom confirm <tok>`
R2(viewer)/R3/R4 ─▶ Approval{id, digest, binding, scope, use_by ≤ 10 min, uses_left, presence} ─▶ `core.approval_requested`(우선)
   ─▶ CLI `--wait-approval ≤ 3600 s`면 대기, 아니면 exit 5 + id (`loom approvals wait <id>`)
   ─▶ web viewer `core.approval.respond{id, decision, scope, presence.proof}` ─▶ 재검증(digest·epoch·generation·정책/프로파일 hash·기한)
뷰어 0개: R3/R4 즉시 exit 5 `E_NO_APPROVER` + hint
```

### 3.6 장애 시 흐름

| 장애 | 감지 | 코어 동작 | 사용자 화면 | 에이전트 |
|---|---|---|---|---|
| worker 크래시(kill -9·패닉·DLL segfault) | exit + ctl EOF | 진행 중 쓰기 `outcome_unknown`·감사, `core.worker_state{crashed}`, 백오프 재기동, tail 복구 후 `record.gap` | 해당 패널 빨간 배너 + 재기동 카운트, 다른 패널 영향 없음 | 진행 중 호출 exit 7 `E_WORKER_DOWN` + `outcome_unknown`, 재시도 금지 |
| worker hang | Health 3 s / 10 s | suspect → stalled → kill → 재기동 | 노랑 → 빨강 | exit 7 |
| **저널 실제 손실**(디스크 가득·쓰기 오류·큐 초과로 원본을 버림) | writer 오류 / 큐 overflow | **그 producer `record.health=failed`**(우선) → agent·script 새 R1+ 차단(A12), user 입력 허용, 감사는 데몬 stderr 폴백, 5 s마다 재시도, 회복 시 `record.gap{range, cause}` + `ok` | `REC FAILED` | exit 4 `E_RECORDING_FAILED` |
| 저널 backlog만 큼(아직 무손실) | 큐 깊이 > 50 % | `record.health=degraded` | 노랑 `REC SLOW` | 허용 + `warnings:["record_degraded"]` |
| UI 끊김 | WS close / ping 40 s | 구독 제거, 그 viewer 입력 lease 5 s 유예 후 해제, 대기 승인 유지, viewer 0 → 다음 결과 `no_viewer_attached` | 재연결 시 `after_seq` 이어 받기 | 경고만 |
| CLI 대기 중 데몬 재시작 | 연결 끊김 | 새 `stream_epoch`, 감사·mapping에서 미완 요청을 `outcome_unknown` 확정(`core.write_recovered`) | `daemon restarted (epoch 3→4)` | 쓰기 exit 9 + `outcome:"unknown"`, 읽기 exit 9 `retryable:true` |
| gateway 패닉 | 프로세스 종료 | worker latch + orphan 30 s(M1) / 재부착(M2) | UI 자동 재연결, epoch 배너 | exit 9 |
| latch 중 데몬 재시작 | latch.json | latch 유지(불명이면 latched) | STOP 배지 유지 | exit 6 계속 |

### 3.7 사용자 화면과 에이전트 세션의 비분리 (코어 불변식)

| # | 불변식 | 강제 |
|---|---|---|
| I1 | 장치는 worker(또는 등록된 gateway 모듈)만 연다 | 도메인 장치 열기 API는 `pub(crate)`, 공개 표면은 `Registry`뿐. `loom` CLI는 장치 crate를 링크하지 않음 — CI 의존 그래프 검사(`cargo deny` + 스크립트) |
| I2 | 숨은 토픽 없음 | `EventRecord`에 visibility 필드 없음, 가림은 필드 단위만 |
| I3 | 늦게 열어도 같은 화면 | 허브 단일 스레드 snapshot, 시험 T1 |
| I4 | 에이전트의 읽기도 보인다 | 모든 agent RPC → `core.activity{client_id, method, summary}`(1 Hz 집계) |
| I5 | 사용자 개입이 에이전트에게 보인다 | 모든 봉투 `others_since_last`, 사람용 `NOTE:` |
| I6 | 응답보다 화면 스트림이 먼저 | publish-before-reply(§3.3), 시험 T2 |

### 3.8 도메인 등록 (D10 → C3)
- `loomd`의 `domains.rs`가 cargo feature로 켜진 도메인 crate의 `module()`을 Registry에 넣는다. 동적 로딩 없음(CR-C8).
- 도메인 crate는 세 조각: `gateway`(Registry 등록·훅·리듀서·질의), `worker`(선택), `cli`(clap 조각 + 사람용 렌더러 — `loom`이 링크).
- 외부 언어 어댑터는 manifest(명령 스키마·risk table·이벤트 kind)를 Hello에 싣고, classify는 manifest 정적 표로 하는 범용 `adapter` gateway 모듈이 한다(P4).

### 3.9 P0 stub crate와 안정 API 표면

| crate | 내용 | 도메인이 쓰는 곳 | P0 상태 |
|---|---|---|---|
| `loom-proto` | 이벤트 레코드·`SourceRef`·`StreamCursor`·Ts·Gap·WriteState·봉투·오류 코드·RPC 기본 타입·worker 메시지·`U64`/`Bytes` 이중 표현(fallible decoder) | 전부 | **Day 1–2 완성**, serde + schemars, tokio 의존 없음 |
| `loom-domain-api` | `DomainModule`, `Registry`, `CommandSpec`, `WriteHooks`, `Reducer`, `EvidenceVerifier`, `SafeStateSpec`, `HelpSpec`, **`CallCtx`, `JournalReader`**(P0는 시그니처만) | gateway 조각 | Day 1–2 compile-only |
| `loom-worker-sdk` | `run_worker`, `Worker`, `WorkerCtx`, `Emitter`(non-blocking), `LatchFlag`, `RateGuard`, `Clock` | worker 조각 | Day 1–2 compile-only, Day 3–5 인프로세스 가짜 전송 |
| `loom-cli-kit` | `DomainCli`, 봉투 출력, 커서 파서, 종료 코드 매핑, 동기 클라이언트·자동 기동 | cli 조각 | Day 1–2 trait, 동작은 P1 |
| `loom-testkit` | 인프로세스 FakeWorker 하네스, **`RegistryProbe`**(register() 호출 + 이름·스키마 검사), **`HookHarness`**(fixture `WriteRequest`로 classify·approval_binding·presend·within_pattern 호출 — gateway 쪽 도메인(UDS 세션 관리자·CAN 질의 모듈)이 첫 주에 돌려 볼 순수 함수 하네스), fixture(D3 모든 필드·gap 3종·write_state 전 단계), assert(`publish_before_reply`·`snapshot_seam` 형식 검사) | 도메인 시험 | Day 3–5 최소 — **제품 hub/policy를 재현하지 않는다**(실제 허브 시험은 P1 `loomd`로) |
| `loom-schema` | JSON Schema 내보내기 bin + 생성 스크립트(TS·Python) | CI | Day 3–5 |
| `loom-echo` | 견본 도메인(gateway + worker + cli, sim 루프백) | 복사 시작점 | Day 3–5 |
| `loomd --fixture-replay` | fixture 이벤트를 재생하는 **WS 서버 모드** stub(UI가 브라우저로 붙음, UI U7·CR-UI-14). fixture에 고부하·gap 3층·`outcome_unknown`·늦은 응답·제어 큐 포화 포함 | UI 팀 | Day 5 |

**핵심 trait (시그니처 초안 — 전체는 `r1-opus.md` §3.9, 아래 수정 반영)**
```rust
pub trait DomainModule: Send + Sync + 'static {
    const NAME: &'static str;            // "uart" → RPC 접두·CLI 그룹·이벤트 domain
    const API_VERSION: u32;
    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError>;
}
// Registry: command · event_kind · risk_table · hooks · worker(WorkerSpec{instances: PerChannel{max}|Shared, budgets, reopen_after_crash, arch}) ·
//           runtime(Shared|Dedicated) · reducer · evidence_verifier · display_summarizer · capability ·
//           safe_state · interlock · panel_manifest · policy_section · help

/// 세 훅 모두 동기·논블로킹, 호출당 ≤ 1 ms, I/O 금지(uds-core C-1). 코어가 시간을 재서 1 ms 초과 경고, 10 ms 초과 버그 기록.
pub trait WriteHooks: Send + Sync {
    fn classify(&self, req: &WriteRequest) -> Classification;            // risk, reason, block(DefaultBlocked|HardDeny|ImmutableDeny)
    fn approval_binding(&self, req: &WriteRequest) -> Binding;           // digest_fields + display(approval.display 스키마)
    fn presend_check(&self, req: &WriteRequest, grant: Option<&Grant>) -> Result<(), PresendError>;  // 동기 — .await 없음
    fn within_pattern(&self, req: &WriteRequest, pattern: &PatternBinding) -> bool { false }        // 사전 승인 패턴(uart #16, uds C-7)
}
// 무효화: CallCtx::approvals().invalidate(approval_id, reason)  (uds C-2)

/// C7: 코어는 범위·coverage·gap·단계 창·epoch/generation, 도메인은 의미 술어를 원본으로 재평가.
pub trait EvidenceVerifier: Send + Sync {
    fn predicates(&self) -> &[&'static str];        // "uart.text_contains", "can.signal_range", "uds.response_eq" …
    fn coverage_need(&self, pred: &Predicate) -> CoverageNeed;   // Positive | Complete | Measure  (§10.9)
    fn verify(&self, pred: &Predicate, events: &[EventRecord], cov: &Coverage, budget: &mut EvalBudget) -> Verification;
}
pub enum Verification { Match{matched: Vec<EventRef>, observed: Value}, Mismatch{observed: Value, reason: String}, Inconclusive{reason: String} }
```
- `loom-worker-sdk`는 hot path에 async를 강제하지 않는다(can-core W7): SDK IPC는 자기 스레드(tokio current_thread), 도메인 RX·ISO-TP·주기 스케줄러는 도메인이 만든 전용 OS 스레드에서 `Emitter`(동기·non-blocking, 실패 반환)로만 만난다.
- **가져오는 방법**: 같은 저장소 workspace. Day 2에 CORE 브랜치 `core/p0`에 태그 `core-p0-d2`(타입·trait), Day 5에 `core-p0-d5`(testkit·echo·schema·replay stub). 도메인 팀은 자기 worktree에서 `crates/loom-*`를 workspace path 의존으로 쓰고, 태그 사이 변경은 `docs/contract/CHANGELOG.md`에 기록한다.
- 안정 API 표면 = `loom-proto`, `loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit`, `loom-testkit`. 나머지 CORE crate는 도메인 직접 의존 금지(CI 검사). 변경은 `docs/contract/CHANGELOG.md` + 도메인 리뷰.

---

## 4. CLI / API 명세 초안

### 4.1 `loom` 명령 트리 (CORE 소유)
```
loom
├─ daemon  start [--foreground] | stop [--force] | restart | status | logs [--tail N]
├─ status  [--request <id> | --idempotency-key <k>] [--workers] [--json]
├─ stop-all [--reason "<text>"]                      # 전 도메인 latch (agent도 실행 가능)
├─ events  [--since <cursor>] [--until <cursor>] [--domain d] [--channel c] [--kind k]… [--origin o]
│          [--max N] [--follow --timeout ≤600s] [--peek]
├─ mark    <label> [--note "<text>"]
├─ record  status | list | path <session> | export <session> --format jsonl|… | verify [--audit] [--rebuild-index]
├─ confirm <token>                                   # R2 토큰 재제출 (도메인 별칭 동일)
├─ approvals list | wait <id> --timeout ≤3600s      # approve 동사 없음 (D6)
├─ run     plan | start | wait | status | log | pause | resume | abort
├─ test    import | plan check|show | start | step | check | verdict | evidence | note | ask | status | wait | end | report | freeze
├─ policy  show [--effective] | diff | propose <file>
├─ ui      [open [<panel-ref>] [--filter …] [--at <cursor>]]
├─ help    [--json] | <domain> help --json           # C8 정본
├─ skills  sync [--target claude|codex|both] [--symlink] [--dry-run]
├─ doctor  [--json]                                   # 경로·권한·포트·dialout·드라이버·DLL·샌드박스 탐지
└─ version [--json]
```
- `events`가 에이전트용 구독을 대신한다(무한 금지, `--follow` ≤ 600 s, `--max` 기본 100, 하드 1,000행/64 KiB).
- `ui open`: RPC `core.ui.focus` → 이벤트 `ui.focus_request{panel, filter, at, actor}`(core-ui U6 이름). 사용자 화면을 강제로 바꾸지 않는다.

### 4.2 출력 예시
```
$ loom status
loomd 0.1.0 · pid 48122 · up 2h13m · epoch 3 · UI http://localhost:47123/ · viewers 1 web (visible 1)
env bench (desk-bench-1) · policy 9c1e… · latch off · record ok (12.4 GiB / 20 GiB, 30 d)
workers  uart ready rss 21 MiB ports 2 · can-sim ready rss 38 MiB ch 1 load 4.9k fps
         can-hw quarantined (5 crashes/60s) — user must restart in UI
pending approvals 1   ap-7Q2 uds.security level 1 ecu1 R3 expires 8m12s
NOTE: user sent 3 inputs since your last call
```
```
$ loom can send ch0 0x123#11
error[E_STOPPED]: all agent writes are latched by STOP (user, 14:02:11 via web UI)
hint: ask the user before continuing; only the user can release STOP in the Loom UI
$ echo $?   → 6
```
```
$ loom uart send dut "ls\n"          # 데몬이 없을 때
loom: started daemon (pid 48122) — UI: http://localhost:47123/ (open it with: loom ui)
sent 3 B to dut · seq 12 · state=sent
WARNING: no viewer attached — the user cannot see this. Ask them to run `loom ui`.
```
`--json` 예시는 `r1-opus.md` §4.2를 기준으로 한다.

### 4.3 봉투 (D5 정본, `loom-proto`)
```rust
pub struct Envelope<T> {
    pub schema_version: u16, pub ok: bool, pub cmd: String, pub data: Option<T>,
    pub meta: Meta, pub warnings: Vec<Warning>,
    pub others_since_last: OthersSinceLast,      // 모든 결과, 오류 포함
    pub error: Option<ErrorBody>,
}
pub struct Meta { truncated: bool, next_cursor: Option<Cursor>, range: Option<SeqRange>, display_gap: Option<GapSummary>,
                  record_health: RecordHealth, env: Env, latch: bool, viewers: Viewers, stream_epoch: U64, visibility: Visibility }
pub struct Viewers { subscribed: u16, visible: u16 }       // uart #13 — visible은 패널 가시 보고(사람 존재 증명 아님)
pub struct OthersSinceLast { count: U64, origins: Vec<Origin>, from_cursor: Cursor, kinds: BTreeMap<String, U64>, by_channel: BTreeMap<String, U64> }
pub struct ErrorBody { code: ErrorCode, message: String, hint: Option<String>, retryable: bool, request_id: Option<String>, detail: Value }
```
- `others_since_last` 기준 커서는 **클라이언트 전역** `(LOOM_CLIENT, "others")`로 서버 저장(TTL 24 h, 전체 4096개 LRU) + 결과에 `by_channel` 내역. 채널별 커서로 하면 에이전트가 UART를 쓰는 동안 사용자가 CAN에서 보낸 프레임·전역 STOP·정책 변경이 빠진다(Opus E8). 세는 대상: user, 다른 actor의 script, 다른 agent client의 쓰기, `core.latch`·`core.policy_changed`. 자기 이벤트·device RX는 세지 않는다.
- 경고: `no_viewer_attached`(구독 0) / `no_visible_viewer`(구독은 있으나 보이는 패널 0)를 구분(core-ui U1).
- 증거 공통 DTO(test-domains TC-C5): 대기·검증형 명령 결과 `data.evidence[]` = `{ref:{stream_epoch, seq_range, source_refs[]}, pointer, predicate_id, observed, complete}`. 사람용 REF 문법은 TEST 안 `<domain>:<channel>@seq:a..b#<json-pointer>`에 **epoch를 넣은 `@e<epoch>:seq:a..b`**를 제안(데몬 재시작이 낀 세션의 모호성 제거 — `core-test.md`로 TEST와 합의).

### 4.4 오류 코드 → 종료 코드
`loom-proto::ErrorCode`(SCREAMING_SNAKE). 묶음(D5·A6 + 추가):
- 1 `E_INTERNAL E_NOT_IMPLEMENTED` · 2 `E_USAGE E_AMBIGUOUS_CHANNEL E_CONFIG E_DB E_UNKNOWN_METHOD E_SCHEMA`
- 3 `E_TIMEOUT` (+ CR-C12 승인 시 `E_NO_MATCH E_UNEXPECTED_MATCH E_COUNT_OUT_OF_RANGE`)
- 4 `E_DENIED E_UNSUPPORTED E_RATE_LIMIT E_BUSY E_RECORDING_FAILED E_EVIDENCE_MISSING E_EVIDENCE_MISMATCH E_OUT_OF_WINDOW E_PLAN_MISMATCH`
- 5 `E_CONFIRM_REQUIRED E_APPROVAL_REQUIRED E_NO_APPROVER E_APPROVAL_DENIED E_APPROVAL_EXPIRED`
- 6 `E_USER_ACTIVE E_LOCKED E_LOCK_BROKEN E_HELD_BY_USER E_INTERRUPTED_BY_USER E_QUARANTINE E_STOPPED E_LEASE E_SESSION_ACTIVE E_SESSION_PAUSED`
- 7 `E_NOT_OPEN E_DISCONNECTED E_CONNECTION_CHANGED E_BUS_OFF E_NO_RESPONSE E_PORT_BUSY E_PERMISSION E_NOT_FOUND E_WORKER_DOWN E_WORKER_BUSY`
- 8 `E_CURSOR_GAP E_SCAN_LIMIT E_PARTIAL` · 9 `E_DAEMON E_PROTO_MISMATCH` · 10 `E_NRC` · (CR-C17) 11 `E_TEST_FAILED` — `test end`·`run --test`만
- 새 코드는 `loom-proto` PR + 묶음 지정 필수, enum은 추가만.

### 4.5 커서
- 정본은 epoch를 포함한 opaque 문자열. 사람 문법: `@last | now | seq:N | mark:X | -5s | HH:MM:SS.mmm | ISO8601`, 도메인 접두 확장(UART `off:N`, `send:last` — A7).
- **`now`** [결정, CR-C3]: 요청 접수 시점의 허브 high-water H에 원자적으로 바인딩(uart #7).
- `HH:MM`은 세션 시간대·날짜를 결과에 명시, 모호하면 exit 2. 보존 범위 밖 = exit 8 `E_CURSOR_GAP`. mapping 불확실 구간 = `meta.range.quality:"uncertain"`.
- idempotency: `(client_id, key)` → 처리 중이면 **pending 예약**(동시 두 번째 호출은 같은 결과를 기다림), 결과 TTL 24 h·전체 32 MiB. 만료·부재면 재송신하지 않고 `unknown` 반환.

### 4.6 기동 시간 설계 [결정]
- `loom`은 **tokio 없는 동기 클라이언트**(`tungstenite` + std TcpStream), clap derive, 웹 자산·장치 crate 미링크. 스파이크: 1.0 MB 바이너리, 프로세스 생성 → 연결 → hello → RPC → 종료 p95 3.3 ms / help만 p95 2.8 ms **[실측-mac, n=300, `spikes/core/opus-cli-rtt`]**. 목표는 §0.4(OS별 30/60 ms, Windows Defender·페이지 폴트 여유). 하위 게이트로 **CLI 고정 비용(생성→연결→hello→종료) p95 ≤ 10 ms(Windows ≤ 25 ms)**를 따로 재서, 왕복 예산이 감사 sync·worker 홉에 어떻게 쓰이는지 분해해 보인다.
- 자동 기동(D8): `daemon.json` 연결 시도 → 실패·stale이면 `start.lock` 파일 락(대기 상한 2 s) → detach spawn(Unix `setsid` + `/dev/null` stdio + `~/.loom/run/loomd.log`; Windows `DETACHED_PROCESS|CREATE_NEW_PROCESS_GROUP|CREATE_NO_WINDOW`, 부모가 Job 안이면 `CREATE_BREAKAWAY_FROM_JOB` 시도 → 거부 시 `daemon_tied_to_parent_job` 경고) → `daemon.json` 폴링 10 ms → 연결. `LOOM_NO_AUTOSTART=1`이면 exit 9.
- 샌드박스 주의: 에이전트 샌드박스 안에서 자동 기동된 데몬이 장치 접근을 못 할 수 있다 → `loom doctor` 탐지 + 스킬 안내("권한 오류면 사용자에게 `loom daemon start` 직접 실행 요청").

### 4.7 RPC·이벤트 이름 규칙 (D2·A16 정본화)
- 메서드 `<domain>.<verb>` 또는 `<domain>.<noun>.<verb>`, 토큰 `[a-z][a-z0-9_]*`, 최대 3토큰, 등록 시 검사. 이벤트 kind `<domain>.<event>`(2토큰, snake — `can.isotp_rx_start`). 상태 갱신형은 같은 `request_id` 행 갱신(A2).
- 예약 domain(CORE): `core, record, display, source, run, test, ui, adapter`(CR-C7).

| 메서드 | role | 요지 |
|---|---|---|
| `core.hello` / `core.subscribe` / `core.unsubscribe` | 전부 | §3.4 |
| `core.events.query{since, until, filter, max, peek}` | 전부 | uds C-6의 `core.events.get` 포함 |
| `core.status{request_id?|idempotency_key?}` | 전부 | |
| `core.stop` / `core.stop.release` | 전부 / web viewer | §3.5(S) |
| `core.confirm{token}` | agent | R2 |
| `core.approval.list` / `.wait` / `.respond` | 전부 / 전부 / web viewer | §3.5(A) |
| `core.mark{label, note}` | 전부 | origin은 연결 자격 |
| **`core.time`** → `{recv_mono_ns, send_mono_ns, wall_ns, clock_id}` | 전부 | 수신 즉시 우선 처리(uart #12, core-ui U4) |
| **`core.metrics.report{kind, channel, p50, p95, p99, n, skew_err_ms, rendered_through_seq, oldest_pending_age_ms}`** | viewer | 화면 게이트 지표 |
| `core.viewer.presence{channel?, visible}` | viewer | `meta.viewers` 근거 |
| `core.safe_state.run{ids[]}` / `core.safe_state.list` | web viewer / 전부 | A15, uds C-4 |
| `core.worker.list` / `core.worker.restart` | 전부 / web viewer | |
| `core.policy.show` / `.propose` / `.apply` | 전부 / agent·viewer / web viewer | 완화는 승인 후 |
| `core.ui.focus{panel, filter, at}` | agent | → `ui.focus_request` |
| `POST /api/v1/stop`(HTTP) | viewer(쿠키)·agent(토큰) | WS가 끊긴 중에도 STOP(CR-UI-08), `core.stop`과 같은 처리 |
| `core.ui.layout.list/get/put` | viewer | `~/.loom/config/layouts/*.json`(core-ui U6), M2 |
| `core.replay.open{range, speed}` | viewer | M3, 재생 채널 쓰기 거부 |
| `core.help{domain?}` | 전부 | `help --json` |
| `record.status/list/export/verify` · `core.job.status/cancel` | 전부 | export·run·test 공통 job |
| `run.*`, `test.*` | §10.9 | |

### 4.8 대용량 HTTP
`GET /api/v1/query/<query_id>`(64 KiB 넘는 질의 결과, 60 s), `/api/v1/raw/<raw_ref>`(서버 소유 opaque id, 임의 경로 아님), `/api/v1/report/<session>.{html,xml,json}`. 인증: agent = `Authorization: Bearer`, viewer = 쿠키. NDJSON 스트리밍, 응답 상한 256 MiB, 동시 4 → 429/503 + `retry-after`.

---

## 5. 사용자 뷰어 UX (코어 관점)

C6: 사용자 화면은 **Loom UI(웹)**가 1차. 코어는 그리지 않고, UI가 받을 것을 같은 WS로 제공한다(상세 `_xteam/core-ui.md`).

| UI가 받는 것 | 코어 수단 |
|---|---|
| 라이브 스트림 | `core.subscribe` snapshot + live, 패널별 구독을 연결 1개에 다중화(연결당 ≤ 8 구독, 전체 ≤ 32) |
| 늦은 합류 상태 | 리듀서 snapshot(§3.4) |
| 질의 | 도메인 질의 RPC + `core.events.query` + HTTP GET, 다운샘플 명시 |
| 사용자 쓰기 | 도메인 RPC를 viewer 연결로 → origin=user, 같은 파이프라인. 사용자 쓰기도 R3/R4면 본인 승인(같은 UI 1클릭 + presence) |
| 승인 대기열 | `core.approval_requested/resolved` + list/respond, presence는 viewer에만 |
| STOP·비상 OFF·복구 | `core.stop`, `core.safe_state.list` → `core.safe_state.run`(인터록 통과), 도메인 비상 OFF(A3, 확인 없음·인터록 경고 — Registry에 대상 명시) |
| env 배지 | hello·`meta.env`·`core.env`, `vehicle` 상시 표시(B4) |
| gap·lag·연결 | gap 3층·`record.health`·`lag_ms`·heartbeat·`sub_stats` |
| 에이전트 행동 | `core.activity`, write_state 행, `ui.focus_request` |
| 테스트·러너 | `test.*`, `run.*` + 리듀서(`_xteam/ui-test.md`) |
| 시계 | `core.time` 왕복 최소값으로 오프셋·오차 추정, `core.metrics.report`로 렌더 지연 보고 |

TUI: 코어는 TUI 키트를 만들지 않는다. 터미널 뷰어가 필요한 도메인은 `viewer-tty` role만 받는다. 코어가 직접 주는 터미널 화면은 `loom events --follow`(사람용 한 줄 렌더)뿐.

---

## 6. 데이터 / 기록 포맷

### 6.1 이벤트 레코드 (`loom-proto::event`, D3 전 필드)
`r1-opus.md` §6.1의 `EventRecord` 타입을 정본 초안으로 채택하고 아래를 고친다.
- 필드: `schema_version, session_id, domain, channel, kind, seq(stream), epoch(stream), producer_id, producer_epoch, source_seq, ts{mono_ns, wall_ns, clock_id, source, quality}, dir, origin, client_id, actor_id, external_actor, original_origin, request_id, job_id, generation, purpose, risk, test_session, test_step, env, payload, metrics[], quality[]`.
- **payload 모양** [결정, R4 수정 2 — 도메인·UI·TEST가 이미 이 모양]: `payload` = kind별 스키마를 가진 **도메인 구조 객체**. 원시 바이트는 `payload.bytes{encoding: hex|base64|raw_ref, data | ref}`, 쓰기 상태는 `payload.write_state`(쓰기 행 이벤트 필수, `request_id`와 함께). 별도 `data` 필드는 두지 않는다. D3의 "바이트 payload {encoding, data}"를 바꾸므로 CR-C6에 포함. (`r1-opus.md` §6.1 타입 초안의 `payload: Option<Payload>`·`write_state`·`data` 필드는 이 모양으로 고쳐 C00에서 정의한다.)
- **원본 레코드(`SourceRecord`, 저널)와 발행 레코드(`PublishedRecord`, 스트림)를 타입으로 구분**: 원본에는 stream seq·epoch가 없다(worker가 알 수 없음). 발행 시 허브가 붙이고 mapping 로그에 남긴다.
- `epoch` 의미 통일(CR-C5): 최상위 `epoch` = stream_epoch(커서용). 장치·링크 세대(UART gen, UDS link epoch, CAN 채널 재open)는 모두 `generation`. 도메인 프로토콜 필드(`can.isotp.open`의 epoch 등)는 `payload.isotp_epoch`처럼 도메인 이름으로.
- 증거 경로 정본: TEST·도메인 술어의 JSON Pointer는 **발행 레코드 JSON 표현 기준**(`/payload/<field>`, `/payload/bytes/data`, `/metrics/<i>` — 예: UART `/payload/state`, UDS `/payload/sid`·`/payload/decoded/…`)으로 하나로 고정하고 fixture로 검증(test-domains TC-C5/C7).
- `U64` = JSON 10진 문자열(schema pattern), `Bytes` = JSON hex(RPC 인자)/base64(이벤트 payload 기본), MessagePack 정수/bin. **디코더는 fallible**(홀수 길이·비ASCII·상한 초과 → 오류, panic 금지 — Sol F13).
- 손실 3층 공통 `GapData{seq_range?, producer_range?, count: Exact|Unknown, cause, bytes?, detail}`.
- 코어 이벤트 kind: `core.latch, core.approval_requested, core.approval_resolved, core.confirm_issued, core.policy_denied, core.policy_changed, core.worker_state, core.job_state, core.job_progress, core.activity, core.mark, core.epoch_changed, core.write_recovered, core.session_closed, core.env, display.gap, record.gap, record.health, source.gap, ui.focus_request, run.*, test.*`. 기록하지 않는 것: `core.heartbeat`, hello ack.

### 6.2 저널 [결정]

| 후보 | 판정 |
|---|---|
| **자체 append 세그먼트**(길이 + CRC32C 프레임, 헤더에 codec id) | **원본 정본** — 순차 쓰기, tail 복구 단순, 메모리는 버퍼만 |
| SQLite WAL | **인덱스·카탈로그만**(세션·세그먼트·sparse 인덱스·pin) |
| Arrow/Parquet | export·viz 파생물만 |
| redb 등 KV | 기각 |

- **레코드 codec**: M1은 **JSON(한 프레임 = D3 레코드 1개)**. D7 "JSONL 청크 + 원시"와 내용이 같고(`loom record export --format jsonl`은 프레임 벗기기만), 프레임·CRC로 tail 복구가 쉽다. 큰 원시 바이트는 raw chunk 파일 + `raw_ref`. MessagePack codec은 **CR-C14**(full schema·같은 내구성으로 크기·CPU 실측 후, M2)로 메인에 요청 — 실측 근거: JSONL 평균 581 B/event **[실측-mac]**, MessagePack array 120 B·named 351 B(8 B payload 축약 이벤트) **[실측-mac]**.
- 세그먼트: `<세션>/<producer_id>/<producer_epoch>/<seg_no>.lj` — 헤더 `magic "LOOMJ1" | format_ver | codec | checksum_alg(crc32c) | producer | epoch | created wall | clock_id`, 레코드 `u32 len | u32 crc32c | u8 rec_type | payload`. CRC는 **CRC32C 고정**(`crc32c` crate; `crc32fast`는 IEEE CRC32라 다르다 — Sol F9).
- 회전 64 MiB 또는 60 s. seal: close → fsync → rename → index.db commit → zstd 블록 압축본(seekable 1 MiB 블록) 검증 → 원본 삭제. 압축 대기 상한 2청크·1 MiB, 밀리면 비압축 seal.
- 크래시 복구: 열린 세그먼트의 마지막 유효 CRC까지 잘라내고 `record.gap{cause:"crash_tail", bytes}`. **sealed 파일 중간 CRC 오류는 tail로 숨기지 않고 `record.gap{cause:"corruption"}` + quarantine.**
- 기록 위치: 대량 원본은 **worker가 직접**(can-core W3). gateway 이벤트(승인·정책·UDS 트랜잭션·run/test·core)는 producer `gateway`로 gateway 저널. stream mapping 로그는 gateway의 별도 정본 파일(§3.4).
- sparse 인덱스: 4096건 또는 1 s마다 `(source ref, file, offset, mono)` — 세그먼트에서 재구축 가능(`record verify --rebuild-index`). **mapping 로그는 재구축 불가(정본)**.
- 세션 디렉터리 `~/.loom/records/<YYYY-MM-DD>/<daemon_session>/` + `manifest.json`(OS·빌드·worker 버전·정책/DB 해시·채널 설정·드라이버 큐 실제값).

**내구성 (written / durable 두 워터마크를 구분해 표시)**

| 대상 | OS로 write | 디스크 sync | 비고 |
|---|---|---|---|
| 데이터 세그먼트 | ≤ 10 ms 또는 64 KiB | 그룹 ≤ 100 ms + seal | `written_through_source_seq` / `durable_through_source_seq`를 Health에 |
| gateway 이벤트·mapping 로그 | ≤ 10 ms | 그룹 ≤ 100 ms | |
| **감사 로그** | **레코드마다 `write()` 성공 확인 후 진행** | **R2 이상 쓰기 intent는 송신 전 즉시 sync**, R1·결과·기타는 그룹 ≤ 100 ms | macOS F_FULLFSYNC 1회 ≈ 4–6 ms [실측-mac] — R1(UART 일반 입력)에 매번 걸면 send→sent < 20 ms를 위협. Windows `FlushFileBuffers` 실측(M1 기준 머신)으로 재확인 |
| index.db | WAL, `synchronous=NORMAL` | 체크포인트 | 재구축 가능 |
- macOS `File::sync_data`는 `F_FULLFSYNC`를 호출한다 [확인: Rust 1.93.1 std 소스] — SQLite `fullfsync=OFF`와 같은 조건으로 비교하지 않는다. Windows `FlushFileBuffers` 비용은 M2 실기 측정으로 주기 확정.
- 측정 [실측-mac]: JSONL 세그먼트 100k건, 200건마다 F_FULLFSYNC 46k ev/s, 2,000건마다 368k ev/s, 10,000건마다 882k ev/s. SQLite WAL fullfsync=ON 같은 배치 41k/230k/420k ev/s.

**조회·재생·보존**
- 조회: 링 → mapping + index.db → 블록 해제·필터. 상한: 스캔 256 MiB·2 s(기본)·결과 64 KiB(RPC) → `E_SCAN_LIMIT` + `resume_from`. 질의 풀 4 동시.
- 재생(M3): `core.replay.open` → producer `replay:<session>`, `original_origin`·`purpose:"replay"` 보존, 재생 채널 쓰기 전부 거부.
- **보존**(Sol F15 반영): 기본 30일 / 20 GiB 중 먼저 도달. GC 단위는 **sealed 세그먼트**(세션 단위 아님) — active 세그먼트와 **pin된 세그먼트**(테스트 증거로 인용된 것, `record protect`)만 제외. quota 80 % 경고, 90 % 오래된 unpinned sealed 제거, 100 % → `record.health=failed`(A12). 삭제는 retention manifest에 남기고 `record.gap`과 구분.
- **감사는 무기한 + 별도 용량 경고**: 20 GiB와 별개 계정, 감사 디스크 부족 시 agent 쓰기 차단(CR-C15로 메인에 명시 요청).
- 디스크 산정 [추정 산술, 압축 전]: 정상 프로파일 CAN 5k fps × 581 B ≈ 2.9 MB/s → **20 GiB를 약 2 h에 채운다**. 15k fps면 ≈ 750 GiB/일. **30일 원본 전체 보존은 기본 quota로 보장되지 않는다.** UI·`loom status`에 "보존 가능 기간 추정"을 표시하고, M2 전에 full schema MessagePack + zstd의 실제 압축률(실제 CAN 로그 샘플)을 재서 CR-C14·CR-C15 근거로 올린다(C13·C22 작업에 포함).

### 6.3 감사 로그 (해시 체인, 끌 수 없음)
- `~/.loom/audit/audit-YYYY-MM.jsonl`, 레코드 `{v, n, ts, kind, who{origin, client_id, conn, role}, what{method, target, risk, digest, request_id, env}, how{path, approval_ref, policy_hash, profile_hash}, prev, h}`.
- `h = SHA-256(prev ‖ JCS(record without h))` — RFC 8785 JCS(`serde_jcs`, 성숙도는 P0에서 golden 검증, 부족하면 자체 정규화). 첫 레코드 `prev` = 이전 파일 마지막 h.
- kind: `write.requested|accepted|denied|result|presend_failed`, `confirm.issued|used`, `approval.requested|resolved|reserved|used|released|invalidated`, `latch.on|off|persist_failed`, `policy.loaded|proposed|applied|rejected`, `worker.crashed`, `input.batch`, `record.health`, `test.verdict|override`, `daemon.start|stop`.
- **대화형 입력 묶음(uart #4, CR-C10)**: `input.batch{lease_id, channel, origin:"user", actor_id, source_range, start_mono_ns, end_mono_ns, byte_count, lines:[{n, len, sha256}], redacted, line_complete, record_health}` — 줄 종결 또는 1 s/4 KiB 중 먼저. 원문은 저널(데이터)에만.
- 매시 head h를 데몬 로그·세션 manifest에 앵커. 해시 체인은 **변조 탐지**이지 같은 OS 사용자에 대한 방지가 아니다.

---

## 7. 라이브러리 선택과 근거

전체 40개 crate의 버전·SPDX·최근 릴리스·다운로드·플랫폼 제한 근거: `spikes/core/library-evidence.md`, `spikes/core/evidence/metadata.json`, R2 재조회 `spikes/core/r2-evidence/`. 버전 표기는 "후보 최신 / 스파이크 lock"을 구분한다. **기준 = crates.io `max_stable_version`**(`newest_version`은 백포트를 최신으로 잡는다 — Opus E1–E4: zstd 0.14.3, fastwebsockets 0.10.0, process-wrap 10.0.1로 정정). 수집 스크립트도 이 기준으로 고친다(C02).

| 영역 | 선택 | 버전(조회 2026-11-10) | 근거·대안 |
|---|---|---|---|
| 런타임 | tokio | 1.54.x | 허브·저널·hot path는 전용 스레드 |
| HTTP+WS 서버 | axum (내부 tokio-tungstenite 0.29) | 0.8.9 | 같은 포트에 정적·WS·GET. fastwebsockets(최고 안정 0.10.0)는 병목 실증 전 교체 안 함 |
| CLI WS 클라이언트 | tungstenite(동기) | axum과 버전 정렬 | 기동 최소화(§4.6) |
| 정적 임베드 | rust-embed | 8.13.0 (MIT) | 오프라인 벤치 PC |
| JSON | serde_json | 1.0.x | simd-json은 프로파일 후 |
| worker codec | rmp-serde | 1.3.1 (MIT) | §3.3 |
| 로컬 IPC | interprocess (대안 tokio 직접) | 2.4.8 | UDS + named pipe [확인] |
| JSON-RPC | 자체 제한 dispatcher | — | jsonrpsee 0.26 대안(같은 fixture) |
| 스키마 | schemars → JSON Schema(draft 2020-12 고정, 안 되면 합의 subset) → json-schema-to-typescript 16.0.0 / datamodel-code-generator 0.91.0 | 1.2.x | 정본 1개 경로. ts-rs·specta는 비교용 |
| 인덱스 DB | rusqlite(bundled) | 0.41.x (MIT) | 카탈로그·sparse 인덱스 |
| 압축 | zstd (CPU 병목 시 lz4_flex) | 0.14.3 후보 / 0.13.4 lock | seal 후 블록 |
| 체크섬·해시 | crc32c, sha2, serde_jcs | 0.6.x / 0.11.x / 0.2.0 | CRC32C 고정 |
| CLI | clap(derive) | 4.6.x | 1.0 MB 바이너리 실측 |
| Windows API | windows(-rs) 직접 (win32job 2.0.3·process-wrap 10.0.1은 편의 wrapper 검토) | 0.62.x | Job·DACL·pipe |
| 파일 감시 | notify | **8.2.2 정식**(9.0.0-rc는 사용 안 함) | 정책 파일 |
| 파일 락 | fs4 또는 fd-lock | P0에서 선택 | 자동 기동 경쟁 |
| 로깅 | tracing + tracing-appender | 0.1.44 | 진단 채널 상한 1 MiB(§9.2) |
| 메모리 측정 | OS API 직접(sysinfo 0.40은 MSRV 1.95 불일치) | — | §9.3 |
| 할당자 | 시스템 기본 → mimalloc/jemalloc는 soak A/B 후 | — | |
| 동적 로드 | libloading | — | 벤더 DLL worker 전용 |
| 배포 | cargo-dist(고정 버전, CI에서만) | 0.34.x | 전역 설치 없음 |

---

## 8. HW / OS 지원 매트릭스 (코어 기능)

| 기능 | Windows x64 (실HW) | Linux x64 (실HW) | macOS (개발·sim) |
|---|---|---|---|
| WS·HTTP 127.0.0.1 | ✅ | ✅ | ✅ |
| worker 로컬 소켓 | named pipe + DACL | UDS 0700 | UDS |
| worker hard 메모리 | Job(commit 한계) | cgroup v2(위임 시) / RLIMIT_DATA 백스톱 | ✗(watchdog만) |
| 자동 기동 detach | DETACHED_PROCESS (+ breakaway 시도) | setsid | setsid |
| monotonic | QPC | CLOCK_MONOTONIC | mach_absolute_time |
| 고정밀 타이머 헬퍼(도메인 worker용) | high-resolution waitable timer [추정] | clock_nanosleep/timerfd | 개발용 |
| fsync 비용 | 높음 [추정, M2 측정] | 보통 | F_FULLFSYNC(높음) |
| viewer-tty 판정 | `GetConsoleMode` | isatty | isatty |
| 벤더 DLL | worker만(vxlapi64 CAN FD, PCANBasic) | worker만(.so 있으면) | ✗ |
| DoIP | OS TCP(PC NIC + 미디어 컨버터, C4.1) | 동일 | sim |
| Python 어댑터 | CANoe(P4) | — | 견본만 |
| CI | windows-latest(MSVC, 벤더 DLL 없이 fake) | ubuntu-latest(+ vcan job) | macos-latest |

원격 보기: 127.0.0.1 고정 + SSH 로컬 포워딩(정확한 Host/Origin 설정, wildcard 금지). LAN bind·TLS·페어링은 M5.

---

## 9. 시뮬레이션 & 테스트 전략 (C2 성능·메모리 포함)

### 9.1 부하 기준 [추정 산술]
- UART 3 Mbaud 8N1 = 300,000 B/s/포트, 읽기는 4 KiB/5 ms 청크(1 B = 1 이벤트 금지).
- CAN classic 1 Mbit/s DLC 0 ≈ 20k fps/채널. CAN FD 64 B·BRS 500k/8M ≈ 5.7–7.8k fps, 1M/8M ≈ 7.7–10.3k fps(ID/DLC/BRS·스터핑 명시 필수).
- 기준 부하: CAN FD 1채널 10k fps + UART flood + UDS 50 req/s. 제품 확장: CAN 4채널 40k fps. 100k ev/s는 다채널 합성 스트레스(한 버스가 100k fps라고 쓰지 않는다).

### 9.2 큐 상한 표 (코어 소유 큐 전부 — 표에 없는 큐는 merge 금지) [추정 초기값]
상한은 **건수와 바이트 중 먼저**. 공통 bounded wrapper가 큐마다 depth·bytes·최대치·드롭을 계측한다.

| # | 큐 | 위치 | 상한 | 넘칠 때 | 지표 |
|---|---|---|---|---|---|
| Q1 | 드라이버/OS RX | worker | 요청 4 MiB(실제값 manifest 기록) | 수신 계속, 오버런 `source.gap`(unknown 허용) | overruns |
| Q2 | RX → 저널 | worker | 채널 8 MiB, worker 합 16 MiB/32k건 | backpressure 가능한 소스만 ≤ 5 ms block, 그 외 **원본 버림 = `record.gap` + health failed(A12)** | depth, oldest_age, record_loss |
| Q3 | RX → 화면 미러 | worker | 4 MiB/8192건 | 화면만 드롭 + `display.gap{producer_range}` | display_drop |
| Q4 | data lane 크레딧 | G→W | 유입률 × 100 ms, 기본 1 MiB | 크레딧 0 → Q3로 | credit |
| Q5 | ctl 송신 G→W | worker당 | 256 KiB/256건 | `E_WORKER_BUSY`, 쓰기는 미전송 `failed` | ctl_tx_depth |
| Q6 | ctl 수신 W→G | worker당 | 256 KiB/4096건 | SDK IPC 스레드만 block, `Emitter`는 실패 반환 + `core.worker_state{ctl_backpressure}` | ctl_rx_depth |
| Q7 | TX 요청 | worker 채널당 | 32건/256 KiB | admission 거절, 사용자 Ctrl-C mailbox 우선 | tx_rejected |
| Q8 | 대화형 입력 | worker 채널당 | 64청크/16 KiB, Ctrl-C mailbox 1 | busy 표시, 재전송 금지 | input_rejected |
| Q9 | 허브 입력 | gateway | 8 MiB/16k건 | producer별 공정 드롭 + `display.gap` | hub_depth |
| Q10 | 허브 링 | gateway | 32 MiB, 채널 ≤ 4 MiB | 오래된 것 제거(저널에 있음) | ring_bytes, oldest_age |
| Q11 | 구독 data | 구독당 | 시간 기반: 구독 유입률 × 500 ms(하한 1 MiB, 상한 4 MiB), **연결당 ≤ 8 구독, 전체 ≤ 32 구독, 전체 큐 합 ≤ 32 MiB** (8배치 고정 큐는 Sol 첫 실행에서 정상 구독자 손실 — Opus E5) | 그 구독만 드롭 + gap/축약, lag > 2 s jump_to_live | sub_dropped, lag_ms |
| Q12 | 구독 우선(셸 전역) | **viewer 연결당**(≤ 8) | 256 KiB/256건 + STOP mailbox 1 | 드롭 금지 → 연결 종료 + 재snapshot | prio_disconnects |
| Q13 | snapshot 진행 중 live | 구독당 | 1 MiB/2 s, 동시 snapshot 2 | 취소·gap·재시도 | snapshot_aborted |
| Q14 | snapshot 크기 | 구독 시 | 2 MiB 또는 10k 이벤트 또는 30 s | 잘라서 `display.gap{cause:"snapshot_budget"}` | snapshot_truncated |
| Q15 | 연결 | gateway | 64 연결(agent 단발 포함), viewer 8 | 거절 | conn_count |
| Q16 | RPC admission | gateway | 전체 128건/2 MiB, 클라이언트당 16, 승인·STOP 예약 슬롯 | `E_BUSY` | inflight |
| Q17 | 승인·확인 대기 | policy | 256건/2 MiB, 클라이언트당 8, TTL 10분 | 새 요청 거절 | approval_pending |
| Q18 | gateway 저널·mapping writer | gateway | 16 MiB | 이벤트 저널: health failed + `record.gap`; mapping: 허브 발행 지연(backpressure) — **mapping은 버리지 않는다** | gw_depth |
| Q19 | 감사 writer | gateway | 1 MiB/1024건 | 파이프라인 ≤ 200 ms block → `E_RECORDING_FAILED`, 사용자 입력은 stderr 폴백 | audit_depth |
| Q20 | 질의 풀 | gateway | 4 동시, pending 16/4 MiB, 질의당 256 MiB·2 s 스캔 | `E_SCAN_LIMIT`/429 | query_age |
| Q21 | HTTP 대용량 | gateway | 4 동시, 256 MiB/응답 | 503 + retry-after | http_active |
| Q22 | 커서·idempotency | gateway | 커서 4096/4 MiB, 결과 32 MiB, TTL 24 h | LRU, 만료는 `unknown` 명시 | evictions |
| Q23 | job·run·test 상태 | gateway | job 128, run 16, test 세션 1(설정 ≤ 4), 단계 ≤ 500/TC, REF ≤ 64/판정, 전체 8 MiB | admission 거절, 이력은 디스크 | active_* |
| Q24 | 증거 검증·export | gateway | 동시 2, pending 8, 캐시 64 MiB | 거절·paging | pinned_bytes |
| Q25 | seal·압축 | worker | 동시 1, pending 2청크/1 MiB | 비압축 seal | compress_age |
| Q26 | 진단 로그 채널 | 전부 | 1 MiB/1024건, 파일 10 MiB × 5 | debug 드롭 집계(감사와 분리) | diag_dropped |
| Q27 | UDS 대상·txn | UDS 모듈 | 대상 32, 대상당 pending 32/64 KiB, txn 링 10k행·전체 8 MiB | busy, 오래된 행 → 저널 | cache_bytes |
| Q29 | test 제어·메모 | gateway | 제어 큐 1k(backpressure, 드롭 금지), 메모 ≤ 4 KiB·세션당 ≤ 1k, 동시 검사 ≤ 2, 세션 엔진 ≤ 64 MiB | 요청 대기(제어)·거절(메모) | test_queue_depth |
| Q28 | UI decode/render | 브라우저 | UI 팀(패널 4 MiB/2만행, 전체 64 MiB 제안) | display gap·축약 | render_ack |

### 9.3 프로세스 예산 (바이트 장부)

| 프로세스 | soft / hard | 장부(최대치 합) |
|---|---|---|
| gateway | 256 / 384 MiB | 링 32 + 구독 큐 전체 32 + 우선 큐 8×0.25 + snapshot 2×(2+1) + 허브 입력 8 + 저널·mapping·감사 버퍼 18 + RPC 2 + 승인 2 + 커서 4 + idempotency 32 + 질의 4×(버퍼 8) + job/test 8 + 증거 캐시 64 + UDS 8 + 진단 1 ≈ **251 MiB** + 런타임·할당 여유 → soft 256 초과 시 신규 질의·구독·증거 admission부터 제한(제어 슬롯 유지). **idempotency 32·증거 캐시 64는 soft 압박 시 먼저 축소** |
| CAN worker(채널 2) | 96 / 192 MiB | 저널 16 + 미러 4 + 드라이버 4 + ctl 0.5 + 디코드·캐시 16 + seal 1 + 런타임·여유 54 (DLL native 포함) |
| UART worker(**포트당**) | 48 / 64 MiB | 런타임·IPC 20 + 링 4 + 저널 큐 8 + 콘솔 엔진 8 + 여유 8 (UART v2 §9 값). 포트 최대 8 → UART 전체 hard 512 MiB. worker admission을 넘으면 open 거절 `E_DENIED{budget}` (Sol F2) |
| Python COM(P4) | 192 / 384 MiB | 실측 후 조정 |
| export helper | 128 / 256 MiB | 기본 미기동 |
| seed-key helper | 32 / 64 MiB | |
| `loom` CLI | 16 MiB | 단발 |
- 기본 번들(gateway + CAN 2채널 + UART 2포트) soft 합 ≈ 448 MiB. 감독자 **worker admission = worker hard 합 ≤ 1 GiB**(gateway 제외, 설정 가능): CAN 192 + UART 8×64 + seed-key 64 + export 256 = 1,024 MiB가 기본 최대 구성. Python COM(P4)을 켜려면 admission을 올리거나 다른 worker를 줄인다.
- 측정(1 s): Linux VmRSS·RssAnon + cgroup memory.current, Windows WorkingSet·PrivateUsage + Job commit, macOS phys_footprint. `getrusage.ru_maxrss`는 peak라 추세 판정에 쓰지 않는다. allocator 보유·파일 캐시를 누수와 구별.

### 9.4 지연 측정 하네스 (`loom-bench`, CORE 소유)
- 생산: sim worker가 `inject_mono_ns`를 payload에 넣는다(echo, uart-flood, can-load 프로파일).
- 소비: (a) Rust WS 구독자 — 같은 호스트 monotonic(코어 게이트), (b) 실제 브라우저 — UI 하네스가 `core.time` 오프셋으로 rAF 렌더 완료 시각 → `core.metrics.report`(화면 게이트). **둘 다 M1 합격 조건.** 섞지 않는다.
- 이벤트마다 tracing 할당 금지: 1/1000 샘플 + bounded histogram. 결과에 window·표본 수·p50/p95/p99/max·미수신/제외 수(생존 편향 노출).
- CLI 왕복: `loom-bench cli --n 1000 -- loom echo send x`(프로세스 생성 포함).
- 결과 `bench/results/<scenario>-<os>-<date>.json` 누적, PR 코멘트로 회귀 비교. **한 번 실행 10분 이내**(재부팅 공지 6 — 장시간은 기준 머신에서만).

### 9.5 코어 불변식 시험 (CI 3 OS, 실패 시 merge 금지)

| # | 시험 | 판정 |
|---|---|---|
| T1 | snapshot 이음매: 10k ev/s 중 구독 1,000회 | 누락·중복 0 |
| T2 | publish-before-reply: 성공 응답 수신 전에 같은 request_id 최종 write_state가 허브 발행 워터마크 안 | 위반 0, 허브 정지 주입 시 `E_PARTIAL{published:false}` |
| T3 | 느린 구독자 격리: 1 KiB/s 구독 1 + 정상 3 | 정상 lag p95 변화 < 10 ms, 느린 쪽 gap 수 = 실제 드롭 수 |
| T4 | 우선 이벤트 보존: data 폭주 중 승인·STOP·write_state | 모두 도착(또는 연결 종료 → 재snapshot 후 도착), 순서 보존 |
| T5 | origin 위조: agent 연결이 `origin:"user"` 필드 | 무시, origin=agent |
| T6 | 자기 승인 차단: agent·viewer-tty로 respond·stop.release·policy.apply·verdict override | `E_DENIED` + 감사 |
| T7 | presence 비노출: agent 구독·질의 전체에서 코드 문자열 검색 | 0건 |
| T8 | 기록 불능(A12): 저널 디렉터리 쓰기 불가 / Q2 overflow | agent R1+ exit 4, user 입력 허용, 라이브 지속, 회복 후 `record.gap` |
| T9 | latch 지속: STOP 후 kill -9 → 자동 기동 / latch.json 손상 | latch 유지 / 불명 → latched |
| T10 | 데몬 재시작 중 쓰기 | CLI exit 9 + outcome unknown, 재기동 후 `status --request` = `outcome_unknown` |
| T11 | 감사 체인 1만 건 중 1바이트 변조 | 정확한 위치 보고 |
| T12 | 승인 once 동시 2요청 | 하나만 송신, 다른 하나 `E_APPROVAL_EXPIRED`(사용됨) (Sol F6) |
| T13 | STOP 중 디스크 hang 주입 | worker ack p99 ≤ 10 ms 유지, `persisted:false` 배너 (Sol F7) |
| T14 | mapping 로그 유실 주입 | 해당 구간 커서 `uncertain`, source ref 재조회로 복구, 거짓 seq 0 (Sol F3) |
| T15 | 바이트 디코더 비정상 corpus | panic 0, 전부 오류 (Sol F13) |

### 9.6 24시간 soak 관문 (C2) [잠정 — 기준 머신 측정 후 확정]

**부하(동시)**: CAN sim 2채널(ch0 FD 5k fps 혼합 DLC, ch1 classic 100 % 최소 프레임 10분 on/10분 off, 주기 job 10 ms × 3, 매시 60 s 20k fps burst) · UDS(CAN-TP 22F190 10 Hz + DoIP sim 20 Hz + 0x78/늦은 응답 + 50 req/s burst, 매시 R2 쓰기) · UART(sim flood 300 kB/s 상시 + bootshell 60 s마다 reboot/expect) · 클라이언트(실제 브라우저 viewer 2 — 하나는 2 s 주기 pause 느린 구독, 30분마다 재연결, attach/detach 1000회 + 에이전트 CLI 루프 2 s) · 테스트 세션(10분마다 혼합 TC 1회 — 세션 종료 후 RSS 반환 확인, TEST TC-C9) · 장애(2 h마다 CAN worker kill -9, 6 h마다 UART worker 패닉, 12 h에 디스크 쓰기 불가 5분).

**관문**

| 지표 | 합격선 |
|---|---|
| RSS | **PID·producer_epoch 수명 구간별**, 워밍업 1 h·fault 복구 구간 제외. 선형 기울기 ≤ 1 MiB/h **그리고** 마지막 30분 중앙값 − 첫 30분 중앙값 ≤ max(16 MiB, 초기의 5 %). hard cap 초과 0 |
| 핸들·FD·스레드 | 수명 구간별 baseline + 4 이내, 증가 추세 0 |
| 손실 | fault 구간 외 `record.gap`·`source.gap` 0, **설명되지 않는 전송 손실 0**. 계획된 화면 생략(Trace 화면 예산·축약)은 gap 카운터와 정확히 일치하면 합격 |
| 지연 | 정상 viewer 전 시간대 §0.4 목표 이내, 마지막 1 h p95 ≤ 첫 1 h × 1.2 **또는** +5 ms 이내 |
| 복구 | 단일 크래시 후 ≤ 5 s `ready`, 다른 스트림 중단 0. crash-loop는 backoff·quarantine 관문으로 따로 |
| 저장 | GC 동작·retention manifest 확인, 감사 체인 검증 통과. 전 구간 보존 soak은 충분한 별도 저장소(기본 20 GiB 아님) |
| 도메인 관문 | **도메인 계획이 더 엄격한 관문을 두면 그것이 우선**(예: UART v2 worker ≤ 0.25 MiB/h·창 차 ≤ 8 MiB) |
| UI 탭 | M2 soak에 headless Chromium UI 탭 포함, 탭 힙 관문은 UI 팀(CR-UI-15) |
- 실행: CI nightly 축약판(Linux·Windows, **10분 이내 조각** — 재부팅 공지 6), 전체 24 h는 기준 머신(Windows x64·Linux x64 벤치 PC 각 1, OS·CPU·RAM·전원·스토리지·브라우저 버전 고정)에서 M2 관문.

### 9.7 크래시 격리 시험
worker kill -9 / 패닉(`panic = "abort"`) / 가짜 벤더 DLL(C shim 널 역참조) / FFI 무한 블록(3 s stalled → 10 s kill) / crash-loop(5회/60 s → quarantined) / gateway kill(orphan latch, M2 재부착) / 메모리 폭주(Windows commit·Linux cgroup hard에서 종료, gateway 생존). 기대 결과는 §3.6 표.

### 9.8 HW 없는 CI
- 두 층: `loom-testkit`(인프로세스) + 실제 `loomd` + sim worker(프로세스).
- 3 OS 매트릭스: fmt·clippy `-D warnings`·test·schema drift·불변식 T1–T15·축약 E2E. 브라우저 E2E는 UI 팀(`loomd --sim-all`). Linux vcan job(권한 별도). Windows MSVC는 벤더 DLL 없이 fake link.

---

## 10. 안전 / 가드레일

### 10.1 원칙
1. **단일 게이트**: 모든 도메인·origin의 쓰기가 `loom-policy` 파이프라인을 지난다(사용자 쓰기 포함).
2. **판단은 Rust gateway에서만**: worker·Python 어댑터는 집행(latch·레이트·예약 ID·하드 차단 재확인)만.
3. **origin은 연결 자격.**
4. **실패는 닫힌 쪽**: 기록·감사 불능, 정책 파싱 실패, 승인 검증 실패 → 쓰기 거부(사용자 입력은 A12 예외).
5. **목표는 실수 방지와 사용자 통제권**, 같은 OS 사용자로 악의적으로 우회하는 에이전트를 막는 보안 경계가 아니다(§10.7).

### 10.2 등급·경로 (D6·A8·A9·A11·B4 — 코어 기본값)

| 등급 | 기본 경로 | 코어 동작 |
|---|---|---|
| R0 | 자동 | 감사 없음, Activity만 |
| R1 | 자동 | 감사 1행 |
| R2 | `r2_path`: sim=`auto`, bench/vehicle=도메인 기본(`token_or_viewer`, CAN 실HW `viewer`) | 토큰 120 s 또는 viewer 확인 |
| R3 | web viewer 승인, scope once/session, 사용 기한 ≤ 10분 | presence 코드 2자 |
| R4 | web viewer 매번 once | presence 코드 4자 + 요약 재확인 |
| default_blocked | `loom.toml unlock` + web 승인 후 R4 대상 | |
| hard deny | 승인 우회 불가, 설정 변경 + web 승인 | |
| immutable_deny | 해제 불가 | exit 4 |
- env(`sim|bench|vehicle`)는 등급·경로를 바꾸지 않는다(B4) — 배지·`meta.env`·쓰기 이벤트 `env`에만.

### 10.3 쓰기 파이프라인 (요청형, A1-①) — R2 수정 반영
```rust
async fn write_pipeline(conn: &Conn, call: RpcCall) -> Envelope {
    let origin = conn.role.origin();                       // 요청 필드 무시
    let spec   = registry.command(&call.method)?;
    let req    = WriteRequest::from(&call, origin, conn.client_id, env());
    let rid    = call.request_id.unwrap_or_else(new_request_id);
    let idem   = idem.reserve(conn.client_id, call.idempotency_key)?;  // 처리 중이면 기다림 / 끝났으면 저장 결과 반환
    // 0) 전역 차단 — safe_state 예외는 web viewer의 core.safe_state.run에만(agent 불가)
    publish(write_row(rid, Requested));                    // 거절도 사용자 화면에 행으로 보인다
    if latch.on() && origin != User { return deny(rid, E_STOPPED) }            // deny() = Failed 행 발행 + 감사 + idem.complete
    if record.health_failed_for(&req) && origin != User && spec.min_risk() >= R1 {
        daemon_log::audit_fallback(&req); return deny(rid, E_RECORDING_FAILED) }
    // 1) 분류  2) 정책
    let cls = hooks.classify(&req);
    let grant = match policy.decide(&req, &cls) {
        Deny(why)    => return deny(rid, E_DENIED.reason(why)),
        Allow        => None,
        Confirm      => { let tok = confirms.issue(..); audit.append(ConfirmIssued, ..)?; idem.release(); return err(E_CONFIRM_REQUIRED).detail(tok) }
        Approve(s)   => { let ap = approvals.create(..); publish_priority(approval_requested(&ap));
                          if viewers.web() == 0 { idem.release(); return err(E_NO_APPROVER) }
                          if !call.wait_approval { idem.release(); return err(E_APPROVAL_REQUIRED).detail(ap.id) }
                          Some(approvals.wait(ap.id, call.wait_timeout).await?) }
    };
    // 3) 사용권 원자 예약 (once 승인·세션 패턴 횟수) — Sol F6
    let resv = grant.as_ref().map(|g| approvals.reserve(g, &req)).transpose()?;   // 실패: E_APPROVAL_EXPIRED(used)
    // 4) 감사 intent (write() 성공이 송신 전제; R2 이상은 송신 전 sync)
    audit.append_intent(Accepted, &req, &grant).map_err(|_| { record.set_failed(); E_RECORDING_FAILED })?;
    publish(write_row(rid, Accepted));
    // 5) presend (동기 훅) — 실패 시 예약 해제·승인 무효
    if let Err(e) = hooks.presend_check(&req, grant.as_ref()) {
        resv.release(); approvals.invalidate(&grant); audit.append(PresendFailed, ..)?;
        publish(write_row(rid, Failed).reason(&e)); return err(e.code()) }
    // 6) backend — 제출 순간 사용권 확정 소비(결과가 outcome_unknown이어도 재사용 불가)
    resv.commit();
    let out = backend.write(&req, grant.as_ref(), deadline).await;      // sent|failed|outcome_unknown + covers
    // 7) publish-before-reply
    if !hub.wait_published(out.covers, call.deadline).await { return partial(rid, out, E_PARTIAL /* published:false */) }
    audit.append(Result, &req, &out)?;                                   // 실패해도 응답은 보냄 + health 갱신
    let env = envelope(out).others_since_last(cursors.take(conn.client_id));
    idem.complete(env.clone()); env
}
```
- 조기 반환 규칙: 확인·승인 대기로 끝나는 경로는 idempotency 예약을 **해제**(재제출 가능), 거절·결과 경로는 **완료**로 저장. 모든 거절은 `Failed` 행으로 사용자 화면에 보인다(STOP 중 에이전트의 쓰기 시도 포함).
- **잡형(A1-②)**: `JobStart/Update/Renew/Stop`만 파이프라인. 단위 송신은 worker가 plan 범위 안에서, 단위마다 `LatchFlag`·예약 ID·`RateGuard`·하드 차단 재확인. renew는 `max_job_duration`(기본 1 h) 안에서 재승인 없이(A9). 사용자는 에이전트 job도 정지 가능(uds C-3). idle gating은 도메인 콜백.
- **대화형(A1-③)**: 입력권 lease 획득 시 1회 R1 판정 → `Input`은 파이프라인 생략, 감사 `input.batch`. viewer 빠른 경로(§0.2), STOP·`record.health=failed` 중에도 열림(A3·A12).
- **`--dry-run`**: 분류·정책·필요 승인 종류까지. 감사 `dry_run` 1행(레이트 제한). **UI 입력 중 미리보기**(uds C-5, 150 ms 디바운스)는 viewer 전용 `preview` 플래그로 감사 제외·레이트 제한.

### 10.4 확인 토큰·승인 객체
- `ConfirmToken{tok:"cf_"+base32(80bit), digest, client_id, stored_request, expires(+120 s), used}`.
- `Approval{id, kind(Write|JobStart|PolicyRelax|Unlock|TestPlan|RunPlan), risk, scope(Once|Session{max_uses,until}|TestSession|Run), digest, binding, stream_epoch, generation, policy_hash, profile_hash, env, requested_by, use_by(≤ 10분), uses_left, reserved, presence, state}`.
- digest = SHA-256(JCS{domain, method, target, args_normalized, bytes, binding_fields}) — 정규화를 명시하고 JSON 키 순서에 기대지 않는다.
- 사용 시 재검증: digest·epoch·generation·정책/프로파일 hash·기한·남은 횟수. 세션 패턴 승인은 **매 송신마다 실제 바이트·현재 상태로 `approval_binding`을 새로 만들어 `within_pattern` 검사**(uds C-7), 범위 밖·등급 상승·상태 변화 → 재승인.
- 토큰 재제출은 사람 동의의 증거가 아니다(A8) → `loom skills sync`가 Claude Code `Bash(loom * confirm:*)` `ask` 규칙을 **안내 출력**(설정 파일 자동 수정 없음). Codex 등 규칙이 없는 환경의 실HW는 `viewer` 권장.

### 10.5 기록 불능 (A12) — R2 수정 반영

| 상황 | health | agent·script 쓰기 | user 쓰기 | 라이브 | 감사 |
|---|---|---|---|---|---|
| 저널 backlog(아직 무손실) | `degraded` | 허용 + 경고 | 허용 | 계속 | 정상 |
| **원본 손실 발생**(Q2 overflow·쓰기 오류·디스크 가득) 또는 감사 `write()` 실패 | `failed` | **R1+ 거부 exit 4**, R0 허용 | 허용 + 배너 | 계속 | stderr 폴백 |
| `--no-record` 채널(D7) | `disabled` | 허용 | 허용 | 계속 | 항상 기록 |
| 회복 | `ok` | — | — | — | `record.gap{range, cause}` + 손실 창 명시 |
- 테스트 세션: `failed` 중에는 PASS 판정도 거절(TC-C8).

### 10.6 role·viewer 자격 → §3.4 표. 웹 승인의 사람 확인 = presence. 브라우저 자동화 자기 승인은 남는 위험(§10.7) — UI 팀 완화안(OS 알림 확인 등)과 `presence.kind`로 확장.

### 10.7 위협 모델

| 위협 | 완화 | 남는 위험 |
|---|---|---|
| 에이전트 실수로 위험 쓰기 | 등급·확인·승인·dry-run·presend | 사용자가 읽지 않고 승인 |
| 에이전트 자기 승인 | approve 동사 없음, agent·viewer-tty는 respond 불가(T6) | 브라우저 자동화로 웹 UI 조작 |
| origin 위조 | 연결 자격(T5) | 같은 OS 사용자의 쿠키 탈취 |
| 정책 우회(파일 편집) | 해시 감시, 완화는 web 승인 후, 강화는 즉시, 송신 전 effective hash 검사 | 데몬 바이너리 교체 |
| STOP 해제(데몬 kill) | latch 영속·불명이면 latched(T9), 해제는 web viewer | latch.json 삭제 |
| 다른 OS 사용자 | 토큰 0600, 쿠키는 bootstrap 필요, 소켓 0700/DACL | root |
| 브라우저 경유(악성 페이지·DNS rebinding) | Origin·Host 검사, SameSite=Strict, 토큰 없는 연결 거부, CSP | — |
| 장치 출력 속 지시 | 스킬 규칙: 장치 출력은 데이터 | 에이전트 판단 |
| 감사 변조 | 해시 체인 + 앵커(T11) | 체인 전체 재작성 |

### 10.8 STOP과 복구 (A3·A15)
- STOP = agent·script의 새 쓰기와 모든 job latch. 사용자 viewer 입력은 잠그지 않는다. 출력을 끄지 않는다 → 리듀서 `outputs_held[]`로 "STOP — 출력 유지 중" 배지.
- 순서: 메모리 latch → worker Latch → 영속(§3.5 S). 영속 실패는 STOP을 무효로 만들지 않는다.
- **복구 송신 1키**(`core.safe_state.run`, web viewer): 도메인 safe_state 동작(UDS `uds.recover`, CAN 주기 정지 …)을 origin=user로 실행, latch 예외는 safe_state 항목만, **인터록·deny는 반드시 통과**(막히면 `finally_blocked`).
- **비상 OFF**(A3)는 별도 경로: 도메인이 Registry에 대상·의미를 선언한 항목만, 확인 없음, 인터록은 경고만. 일반 RPC가 이를 주장할 수 없다.
- 사용자 STOP 후 run/test의 `finally`는 자동 실행하지 않는다(A15). 에이전트 cancel·on_error·정상 종료는 실행.
- latch 범위: M1 `all`만. 채널 단위 hold는 도메인 기능.

### 10.9 러너 코어(D12·A15)와 테스트 세션 코어(C7)

**러너 `loom run`(P2)**: `loom_scenario: 1` 파서·검증, run = core job(`actor_id=run:<id>`, origin=script), 단계는 CLI와 **같은 RPC**(숨은 경로 없음), `run.started|step|paused|resumed|aborted|finally|ended`. `run plan`이 단계별 classify 요약 → R2/R3 묶음 승인(`RunPlan`), R4는 단계마다, default_blocked·immutable_deny가 있으면 전체 거부(R4 포함 계획 거부와 R4 단계별 승인은 다른 흐름 — R4 단계는 그 단계 시작 시 개별 승인). 선언형 상한(timeout·반복·총 단계·총 시간), 임의 eval 없음.

**테스트 세션 코어** — 계약은 `_xteam/core-test.md`(TEST 요청 v0 + core 의견 O1–O9) + 아래 R2 수정. CORE가 `## core 답변`을 이 내용으로 갱신한다.
- 세션 상태기계 `PLANNED → WAITING_APPROVAL → RUNNING ⇄ PAUSED → STOPPED | DONE`. 이벤트(2토큰 규칙): `test.session_started/ended`, `test.step_started/ended`, `test.evidence_checked`, `test.verdict`, `test.note`, `test.ask/answer`. 세션에 묶인 client 이벤트에 허브가 `test_session`·`test_step`을 자동 부여(TC-C1). 장치 RX의 단계 귀속은 publish 순서가 아니라 **source fence**(단계 시작·끝에 worker별 source_seq 워터마크 기록)와 수집 시각으로.
- **M1 최소**: `core.mark`와 `test.step_started/ended` 마커 이벤트만(UI 테스트 화면 뼈대용). 검증·일괄 승인·보고는 P2.
- **기대 조건 결속**(Sol 리뷰 O-1): 판정은 등록된 plan의 `step.expect` id·hash에 결속된다. 인용 술어가 그 expect와 다르면 `E_PLAN_MISMATCH`. 계획 변경은 revision + 사용자 검토 + 승인 무효화.
- **증거 검증 알고리즘**: 인용 `{expect_ref, ref{stream_epoch, seq_range, source_refs}, pointer, complete}` → ① epoch·세션·단계 창(source fence) 확인 → ② 술어의 **coverage 요구** 분류: `Positive`(exists·count ≥ N — 부분 구간으로 긍정 증명 가능), `Complete`(absent·exact·max·hold·order — 창 전체 완전성 필요), `Measure`(within_ms·from/to — 두 끝점 + 시계 quality) → ③ `Complete`·`Measure`인데 창이 `record.gap`/`source.gap`/보존 삭제/`--no-record`/스캔 한도와 겹치면 `inconclusive` → ④ 저널 유한 조회 + **임시 pin lease**(검증 중 GC 방지) → ⑤ 코어 공통 술어(exists·absent·count·order·within_ms·hold — hold는 같은 source·신호·generation 연속 샘플, 간격 ≤ 150 ms)는 코어가, 의미 술어는 도메인 `EvidenceVerifier`가 원본으로 재평가 → ⑥ 결과 `{status: verified|mismatch|not_found|inconclusive|too_large, observed, digest}` 기록, verified면 임시 pin을 정식 pin으로 승격, 아니면 해제. `digest`는 일치 이벤트가 없어도 같아지지 않도록 expect·창·필터·coverage·DB/정책 버전·gap 상태를 포함.
- **판정 규칙**: PASS = 인용 ≥ 1 + 전부 verified + **증거 종류(witness)가 expect에 맞음** — 긍정 관측(device/외부 장치 이벤트), 완전 창의 부재 증명, 측정값, 사용자 assertion(별도 타입, 자동 PASS로 위장 금지) 중. 에이전트 자신의 송신 이벤트만으로는 DUT 동작의 긍정 증거가 될 수 없다. 증거 없는 PASS = exit 4 `E_EVIDENCE_MISSING`, 불일치 = `E_EVIDENCE_MISMATCH`(판정은 `pass_rejected`로 기록), 창 밖 = `E_OUT_OF_WINDOW`, inconclusive 포함 = `inconclusive`(exit 8), 기록 불능 중 PASS 거절. 누락 필드·타입 오류는 false가 아니라 오류.
- **평가 예산(TC-C9)**: 검사당 이벤트 ≤ 200k, CPU ≤ 200 ms(협력 budget token — 도메인 verify는 `EvalBudget`을 확인하며 진행, 강제 중단 아님), 스캔 I/O ≤ 64 MiB·2 s, AST 깊이·노드 수·정규식 입력 길이 상한. 초과 → `inconclusive{cause:"eval_budget"}`. 세션 동시 1(설정 ≤ 4), 단계 ≤ 500, REF ≤ 64.
- **일괄 사전 승인(TC-C3)**: R2·R3 묶음 1회 web 승인(plan hash·동작·대상·횟수 상한·env·프로파일 hash 결속), R4 제외(단계마다), 시작 ≤ 10분, 세션 `max_test_duration` 기본 1 h, 매 송신 `within_pattern` + 원자 예약. 계획 밖 R2+ 쓰기는 개별 승인 + 보고서 "계획 밖" 표시.
- **판정 수정**: web viewer만 `test.verdict{origin:user, overrides, reason}`(감사 `test.override`). 사용자 개입(STOP·입력)이 있었던 단계는 보고서에 표시하고 자동 판정의 원인 판단과 분리.
- **러너 공유·freeze(TC-C4)**: `loom run --test`는 같은 세션 엔진·검증기. `loom test freeze`는 과거 seq·토큰·시각을 복사하지 않고 새 세션에 재바인딩하는 `loom_scenario: 1`을 만든다.
- **TEST R4 추가 요청 반영**(`core-test.md` `## test 의견` 1–8):
  1. 검증기는 인용 witness만 보지 않고 **filter로 range 전체를 재조회**해 술어를 평가한다(count·absent·hold의 모수를 에이전트가 고르지 못함).
  2. 관측 증거 `origins` 기본값 = `["device"]`, 요청·응답은 `request_id`로 짝. 창 안에 **같은 domain·channel·target에 대한 사용자 쓰기**가 있으면 `inconclusive{cause:"user_intervention"}`.
  3. assertion 창은 대기 호출 시점이 아니라 **anchor 이벤트 기준**, 재시도마다 attempt 창 분리. 장비 시각 증거(LA)는 capture→host 오차 구간이 창 경계를 가로지르면 inconclusive.
  4. 위험 요약은 `test.plan.check`가 도메인 classify/`plan_summary`로 산출(finally·repeat·retry·분기 포함). 잔량은 원자 차감(사용자 retry 포함). **계획 변경 = 일괄 승인 전체 무효**, 바인딩에 stream_epoch 포함.
  5. 계획 밖 쓰기가 있었던 세션의 `freeze`는 `--allow-unplanned` 없이는 거부, 허용해도 `needs_review`.
  6. 세션 결과 `passed | failed | inconclusive | incomplete`. `test end`·`run --test` 종료 코드: 0 passed, **11 `E_TEST_FAILED`(신규, CR-C17)**, 8 inconclusive·incomplete·blocked, 6 STOP·`E_SESSION_PAUSED`.
  7. 개입 RPC `test.pause/resume/skip/retry/answer`·`test.plan.amend`. PAUSED 중 세션 client의 새 쓰기 = exit 6 `E_SESSION_PAUSED`. 사용자 abort는 finally 실행 여부를 묻고, 전역 STOP은 A15(자동 finally 없음).
  8. C2: test 제어 큐 1k(backpressure, 드롭 금지), 동시 검사 ≤ 2, 메모 ≤ 4 KiB·세션당 ≤ 1k, **세션 엔진 RSS ≤ 64 MiB**(gateway 장부의 job/test 8 + 증거 캐시 64 안에서), 보고서는 스트리밍 생성. JUnit은 코어가 quick-junit으로 생성(매핑 규칙은 TEST 소유), HTML은 TEST askama 템플릿 + 코어 데이터.
- **UI 테스트 화면 RPC**(`_xteam/ui-test.md`): `test.pause/resume/skip/retry/check`, `test.ask`(deadline 있는 비동기 질문)·`test.answer`(viewer), `test.plan.show`. UI 초안의 `approval.grant{plan_hash,bundle}`은 `core.approval.respond`(kind=TestPlan)로, `records.range`는 `core.events.query`·HTTP GET으로 이름을 맞춘다. 역할: 진행 조작은 agent·viewer, skip·override·answer는 viewer.

### 10.10 정책 파일 2층
- 프로젝트 `loom.toml`(도메인 섹션 + `[bench]` 한계·별칭·env) + 사용자 `~/.loom/config/policy.toml`. 병합 = 더 엄격한 쪽(deny 합집합, 한계 최솟값), 완화 항목(`r2="auto"`, `unlock`, 한계 상향)은 사용자 파일에만 유효.
- notify 감지 → diff → 강화 즉시 적용, 완화는 `Approval{PolicyRelax}` web 승인 후. 송신 전 effective hash/generation 검사(파일 이벤트만 믿지 않음). 파싱 실패 = 직전 유효 정책 유지 + 배너(fail-closed).
- 도메인 섹션 스키마는 `Registry.policy_section`으로 검증. `loom policy show --effective`가 병합 결과와 출처(파일·줄).
- 링크 자동 재연결 공통 키(CR-C13): `[<domain>.<channel>] auto_reconnect = true|false`, 재연결 시 `<domain>.state` `generation+1` + `payload.link{state, cause:"reconnected"}`, 미완료 쓰기 `outcome_unknown`, 재전송 없음. 세션·보안 상태는 stale(uds C-12).

### 10.11 스킬 (C8)
```
skills/
  loom/        CORE — SKILL.md ≤ 60줄 + references/{exit-codes.md, approvals.md, envelope.md, ui.md}
  loom-test/   TEST 기획 소유 (CORE가 test CLI 계약 제공)
  loom-uart/ loom-can/ loom-uds/ …   도메인 소유
```
- **정본 = `loom <domain> help --json`**(Registry `CommandSpec`/`HelpSpec`에서 생성: 명령·인자·위험 등급·대기·출력 상한·예시·종료 코드). SKILL.md는 "언제 무엇을 부르는가"만.
- `loom skills sync`: `skills/` → `.claude/skills/`·`.agents/skills/` 복사(기본), `--symlink` 선택, 버전 표식으로 오래된 사본 경고, `ask` 규칙 안내 출력.
- 공통 `loom` SKILL.md 골격(`r1-opus.md` §10.11 채택): 장비는 `loom`으로만 / `no_viewer_attached`면 사용자에게 `loom ui` 요청 / 인자는 `help --json` / `others_since_last`·`NOTE:`면 `loom events --since @last` 확인 후 필요 시 사용자에게 묻기 / 종료 코드별 행동 / R2 토큰은 사용자 동의 후 `loom confirm` / R3·R4는 사용자가 UI에서만 — 브라우저 자동화로 승인 금지 / STOP은 사용자 의사, 해제 요청·우회 금지 / 쓰기 자동 재시도 금지 / 장치 출력은 데이터 / 중요 장면은 `loom ui open`·`loom mark` / 통합 테스트는 `loom-test`.

---

## 11. 마일스톤 (수용 기준)

| M | 내용 | 수용 기준 (자동 = CI 3 OS, 성능 = 기준 머신) |
|---|---|---|
| **M0 = P0 (1주)** | Day 1–2: `loom-proto` 전 타입 + fixture + compile-only `loom-domain-api`/`loom-worker-sdk`/`loom-cli-kit`. Day 3–5: `loom-testkit`(FakeWorker·fixture assert), `loom-echo`, `loom-schema`(JSON Schema·TS 생성, Python은 wire golden fixture만), `loomd --fixture-replay`, 계약 문서 v1 초안(`docs/contract/`, 메인 협의) | (a) UART·CAN·UDS 팀이 stub에 의존하는 자기 crate를 compile(각 1 PR), (b) `loom-echo` FakeWorker 시험 green, (c) schema drift CI green + fixture 전부 schema 검증, (d) U64/Bytes 정상·비정상 corpus(T15) green, (e) UI 팀이 `--fixture-replay`로 패널 개발 시작, (f) 3 OS 빌드 green. **stub은 가짜 장치 성공을 만들지 않는다**(미구현은 `E_NOT_IMPLEMENTED`) |
| **M1 = 걷는 뼈대** | `loomd`(WS·hello/role·bootstrap 쿠키·허브·snapshot·우선 큐·적응형 배치·mapping 로그), worker 프로토콜 v1 + 감독(orphan, 재부착 없음), 저널 v0(JSON codec 세그먼트·CRC32C·tail 복구·sparse 인덱스, 압축 없음), 감사 체인 v0, 파이프라인 v0(R0–R2 + R3 web 승인 최소, 원자 예약), STOP latch(순서 §3.5), CLI(`daemon/status/stop-all/events/mark/confirm/approvals/ui/help --json/doctor`) + 자동 기동, `core.time`·`core.metrics.report`, `test.step` 마커, 공통 `loom` 스킬 v0 | **4요소 시연**(sim, 실제 브라우저 최소 웹 UI): UART·CAN·UDS 각각 에이전트 송신 → 장치 응답 → 웹 화면 → 사용자 입력·STOP·R2 확인(+ UDS R3 승인 1건). 자동: T1–T15 green; 코어 구간 p95 UART ≤ 25 ms·우선 ≤ 5 ms; **브라우저 화면 게이트 p95 UART/UDS < 100 ms·CAN < 200 ms**; CLI 왕복 p95 Linux ≤ 30 / Windows ≤ 60 ms; 늦은 합류 1,000회 누락·중복 0; worker kill -9 후 다른 스트림 중단 0·배너 < 1 s; 60 s CAN 5k fps + UART flood 중 정상 viewer 지연 목표 유지; 10분 soak 조각 RSS 기울기 관찰(관문 아님) |
| **M2** | 정책 2층·완화 승인·unlock, 승인 전체(R3 session·R4), 잡형 API·safe_state·`core.safe_state.run`, gateway 재부착(W6), 메모리 hard 상한(Job·cgroup), seal·zstd·GC·pin·`record verify`, 러너 v1, **테스트 세션 v1**(검증기·일괄 승인·보고 데이터), Python 어댑터 SDK(wire 호환), 바이너리 열 프레임(측정 후), `core.ui.layout.*`, MessagePack 저널 codec(CR-C14 승인 시) | **24 h soak 관문**(§9.6, Windows·Linux 기준 머신), 크래시 격리 전부(§9.7), 증거 규칙 시험(변조 digest·없는 seq·다른 epoch·창 밖·gap 겹침·보존 삭제·no-record·plan 불일치·부재 witness·동시 once → 증거 없는 PASS 0), 정책 우회 시험(파일 편집 → 미적용), Windows Job commit·Linux cgroup 유무 양쪽 실행 |
| **M3** | 재생, `test freeze`·JUnit/HTML 보고 데이터, 성능 최적화(할당자·simd-json 측정 기반), 릴리스 파이프라인(cargo-dist) | 재생 채널 쓰기 거부, freeze 시나리오가 에이전트 없이 같은 판정, 3 OS 아카이브 설치 스모크 |
| M4+ | 원격 LAN 보기(TLS·페어링), OS 서비스, 공유 메모리 IPC | 측정 근거가 있을 때만 |

---

## 12. 구현 단계 작업 분해

담당: **O** = Opus 워커(구조·API·UX·안전·스킬·계약), **S** = Sol 워커(프로토콜·저널·OS·성능·CI·측정). "기다리는 팀" = 이 작업이 끝나야 진행되는 타팀.

| ID | 작업 | 담당 | 의존 | 기다리는 팀 | 단계 |
|---|---|---|---|---|---|
| C00 | `loom-proto` 전 타입(Source/Published 레코드·봉투·오류·RPC·worker 메시지·U64/Bytes fallible) + fixture | O (+S 코덱 검토) | — | 전부 | P0 D1–2 |
| C01 | compile-only trait: `loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit` | O | C00 | UART·CAN·UDS·EXPLORE | P0 D1–2 |
| C02 | `loom-schema` exporter + TS 생성 + Python wire golden + drift CI | S | C00 | UI, (Python) CAN P4 | P0 D3–5 |
| C03 | `loom-testkit` FakeWorker·fixture assert + `loom-echo` | O | C01 | 전부 | P0 D3–5 |
| C04 | `loomd --fixture-replay` stub(WS hello + fixture 재생) | S | C00 | UI | P0 D5 |
| C05 | 계약 문서 v1(`docs/contract/`) + CHANGELOG 규칙. (`_xteam` `## core 답변` 7개 파일은 R3에서 작성 완료 — 이후 계약 변경 시 갱신) | O | C00 | 메인·전팀 | P0 |
| C10 | 허브(시퀀서·링·리듀서·구독·snapshot·우선 큐·적응형 배치·gap) + mapping 로그 | S | C00 | UI | P1 |
| C11 | gateway(axum WS·hello/role·bootstrap·dispatcher·HTTP GET) | O | C00 | UI | P1 |
| C12 | worker 프로토콜 v1(2 lane·msgpack·크레딧·covers) + 감독(백오프·quarantine·orphan) | S | C01 | UART·CAN worker | P1 |
| C13 | 저널 v0(세그먼트·CRC32C·복구·sparse 인덱스·index.db) + **M1 이중 인코딩 비용 측정**(worker가 IPC msgpack + 저널 JSON 두 번 인코딩 — CAN hot path 부담이 크면 CR-C14 앞당김 근거) | S | C00 | 모든 도메인 | P1 |
| C14 | 파이프라인 v0(원자 예약·idempotency pending) + 감사 체인 + 확인 토큰 + R3 최소 + STOP | O | C10, C11 | 도메인 쓰기 | P1 |
| C15 | CLI 프레임워크(동기 클라이언트·자동 기동·봉투·커서·`others_since_last`) + 코어 명령 + `doctor` | O | C11 | 도메인 CLI | P1 |
| C16 | `loom-bench`(코어 게이트 + 브라우저 게이트 수집) + CLI 왕복 | S | C10, C12 | 도메인 M1 수용, UI | P1 |
| C17 | 공통 `loom` 스킬 v0 + `help --json` 생성 + `skills sync` | O | C15 | 도메인 스킬 | P1 |
| C18 | 불변식 T1–T15 + 3 OS CI | S | C10–C15 | — | P1 |
| C19 | `test.step` 마커 + `core.mark` (M1 최소) | O | C10 | UI·TEST | P1 |
| C20 | 정책 2층·완화 승인·unlock | O | C14 | 도메인 M2 | P2 |
| C21 | 메모리 hard 상한(Job commit·cgroup)·자원 관측·전체 admission | S | C12 | — | P2 |
| C22 | seal·zstd·GC(세그먼트 단위)·pin·`record verify`·export job | S | C13 | CAN export, TEST | P2 |
| C23 | 잡형 API·safe_state·`core.safe_state.run`·비상 OFF 등록 | O | C14 | CAN 주기·UDS TP·EXPLORE | P2 |
| C24 | gateway 재부착(W6: discovery·lock·nonce) | S | C12 | CAN | P2 |
| C25 | Python 어댑터 SDK | S | C12, C02 | CAN(CANoe P4) | P2–P4 |
| C26 | 러너 `loom run` | O | C23 | EXPLORE/TEST | P2 |
| C27 | 테스트 세션 v1(검증기·coverage·pin lease·일괄 승인·보고 데이터·UI RPC) | O (+S 조회 검증) | C13, C14, C22 | TEST, UI | P2 |
| C28 | 24 h soak 장비·부하 스크립트·관문 자동 판정 | S | C16, C21 | 전부(관문) | P2 |
| C29 | 바이너리 열 프레임 + JS golden decoder(UI 팀과) | S + UI | C10 | UI·CAN Graphics | P2–P3 |
| C30 | 재생·freeze·JUnit·릴리스 | O/S | C22, C27 | TEST | P3 |
- 병렬: P0 {C00 → (C01 ∥ C02) → (C03 ∥ C04)}, C05 병행. P1 {C10 ∥ C11 ∥ C12 ∥ C13} → {C14, C15, C19} → {C16, C17, C18}.
- **임계 경로**: C00 → C01 → 도메인 compile. C00은 1–2일 안에 공개(타입만이라도). FakeWorker는 최소 기능부터, 제품 허브·정책을 가짜로 재현하지 않는다.

---

## 13. 리스크

| 리스크 | 영향 | 대응 |
|---|---|---|
| P0 stub 지연 → 세 도메인 정지 | 전체 일정 | C00·C01을 1–2일 compile-only로 먼저, `loom-echo` 복사 시작 |
| macOS 스파이크 수치의 과잉 일반화 | 잘못된 예산 | 모든 수치에 [실측-mac] 표기, Windows/Linux 수치는 M1–M2 관문에서 따로 |
| 허브 단일 스레드 병목 | 고부하 지연 | 허브는 seq·링·팬아웃만(직렬화 1회, 필터는 구독 태스크), full schema 4 클라이언트 측정(C16), 필요 시 도메인 샤딩(전역 seq 유지) |
| 두 lane 순서·publish 의미 혼동 | 거짓 순서 | covers 워터마크 + `E_PARTIAL` + T2 |
| gateway 재시작 시 수신 공백(M1, W6 미충족) | 기록 단절 | `source.gap{cause:"gateway_restart"}`로 정직 표시, M2 재부착 |
| Windows 자동 기동이 에이전트 Job에 묶임 | 세션 종료 시 데몬 사망 | breakaway 시도 + 경고 + `loom daemon start` 안내 |
| Windows fsync·Defender로 저널 지연 | degraded | 주기 설정화, `loom doctor` 예외 안내, M2 실측 |
| 메모리 증가(단편화·누수) | 1순위 품질 실패 | 바이트 장부·큐 표, soak 관문, 할당자 A/B, 핸들 관문 |
| 브라우저 자동화 자기 승인 | 안전 기대 불일치 | 위협 모델 명시, 스킬 규칙, 감사에 승인 연결 정보, presence 확장 |
| 증거 검증의 약한 술어로 거짓 PASS | C7 신뢰 상실 | expect 결속·coverage 분류·witness 규칙·T 시험(M2) |
| 저장량(30일 원본 보존 불가) | 사용자 기대 불일치 | 보존 가능 기간 추정 표시, pin, 질문 Q5 |
| 에이전트 샌드박스 안 자동 기동 | 장치 접근 실패 | `loom doctor` + 스킬 안내 |
| **개발 환경 파일 시스템 정지 재발**(2026-11-10 사고) | 전 에이전트 정지 | 스파이크·벤치 10분 이내, 빌드 산출물은 스파이크 폴더 안, 큰 다운로드는 프로젝트 밖 캐시, 30 s 이상 I/O 정지 시 BLOCKED 기록 |

---

## 14. 사용자에게 물어볼 질문 (답 없으면 기본값으로 진행)

| # | 질문 | 기본값 | 영향 |
|---|---|---|---|
| Q1 | 벤치 PC를 여러 OS 사용자(또는 여러 사람)가 같이 쓰나? | 한 사용자 | 소켓 권한·쿠키 정책 |
| Q2 | 다른 PC(macOS)에서 벤치 PC의 UI를 자주 봐야 하나? | 가끔 → SSH 포워딩 안내 | 원격 인증을 M5보다 앞당길지 |
| Q3 | 에이전트를 샌드박스·제한 모드로 돌리나? 데몬을 로그인 시 자동 실행하고 싶나? | 일반 모드, CLI 자동 기동 | 자동 기동 데몬의 권한 상속 |
| Q4 | 24 h soak 기준 머신 사양(Windows·Linux 벤치 PC CPU·RAM·디스크) | 4코어/16 GiB/SSD | 성능 관문 수치 |
| Q5 | 보존 30일/20 GiB 중 무엇이 우선인가? CAN 고부하면 하루에도 20 GiB를 넘을 수 있다. 테스트 증거로 인용된 구간은 자동 보호해도 되나? | 먼저 도달한 조건, 증거 구간 자동 보호 | 디스크·보존 |
| Q6 | R3/R4 승인 때 확인 코드 입력(2/4자)이 번거롭지 않나? | 코드 입력 | 승인 UX |

---

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 메인에 요청하는 계약 변경·확정

| # | 대상 | 요청 | 근거 |
|---|---|---|---|
| CR-C1 | D2 role | viewer 자격 = 웹 bootstrap 쿠키(M1). 필요 시 M2 `viewer-tty`(입력·hold·STOP만). 모든 확인·승인·해제·정책·재시작은 web viewer 전용 | 에이전트도 PTY에서 돈다, uart #5 |
| CR-C2 | D2 우선 큐 | "드롭 금지 = 상한 초과 시 연결 종료 + 재snapshot, STOP mailbox 1칸 보존"으로 명문화 | 무한 큐 금지(C2)와 양립 |
| CR-C3 | D5 커서 | `now` 추가(접수 시 high-water에 바인딩) | uart #7 |
| CR-C4 | D5 오류 코드 | `E_NOT_IMPLEMENTED`(1), `E_BUSY`(4), `E_STOPPED E_SESSION_ACTIVE`(6), `E_WORKER_DOWN E_WORKER_BUSY`(7), `E_EVIDENCE_MISSING E_EVIDENCE_MISMATCH E_OUT_OF_WINDOW E_PLAN_MISMATCH`(4), `E_PARTIAL`(8), `E_PROTO_MISMATCH`(9), `E_APPROVAL_DENIED E_APPROVAL_EXPIRED`(5) | 새 경로 |
| CR-C5 | D3 epoch | 최상위 `epoch` = stream_epoch, 장치·링크 세대는 `generation` | 세 문서 의미 충돌 |
| CR-C6 | D3 필드 | `env`(쓰기 필수), `external_actor`, `original_origin`, `test_session`·`test_step`, 원본/발행 레코드 구분, **payload = kind별 구조 객체(원시 바이트 `payload.bytes`, 쓰기 상태 `payload.write_state`)** | B4, A14, B3, 재생, TC-C1 |
| CR-C7 | D2 예약 domain | `core, record, display, source, run, test, ui, adapter` | 이름 충돌 방지 |
| CR-C8 | D10 → C3 | 도메인 등록 = Rust 컴파일 시 `DomainModule`, 외부 언어 = worker 프로토콜 + manifest, dylib 없음 | C3 |
| CR-C9 | 마스터 §3.2 | CORE P1 "공통 TUI 키트" 삭제 | C6 |
| CR-C10 | A1-③ | `input.batch` 감사 형식 확정(§6.3) | uart #4 |
| CR-C11 | A5 | worker → gateway 배치도 적응형(조용한 뒤 즉시, ≤ 10 ms) | uart #2 |
| CR-C12 | A6 종료 코드 3 | **3 = 조건 판정 불충족**으로 확장: 대기 timeout + 즉시 판정 불일치(`E_NO_MATCH`, `E_UNEXPECTED_MATCH`, `E_COUNT_OUT_OF_RANGE`). 증거 부족은 8 `inconclusive`. 장치 오류 7·NRC 10 구분 유지. **CORE 권고: 승인** — 확정 전 임시 규칙은 exit 0 + `data.matched=false` | uart #15, uds C-11 — `loom-test` 분기 하나로 |
| CR-C13 | 공통 재연결 | `auto_reconnect` 정책 키 + `generation+1` + `data.link` 이벤트 형식 | uds C-12, UART 재열거, CAN bus-off |
| CR-C14 | D7 저널 codec | 정본 세그먼트를 "길이 + CRC32C 프레임, codec id 헤더"로 정의하고 M1 codec = JSON(D7 내용 그대로), **M2부터 MessagePack codec 허용**(full schema 실측 후). JSONL은 export 형식 | 크기 581 B → 약 120–351 B [실측-mac, 축약], C2 디스크·CPU |
| CR-C15 | D7 보존 | 감사 무기한은 20 GiB quota와 별도 계정 + 감사 디스크 부족 시 agent 쓰기 차단을 명문화. "30일 원본 무손실 보존"은 부하에 따라 불가함을 문서에 명시 | 산술: CAN 15k fps JSON ≈ 750 GiB/일 |
| CR-C16 | D2 publish-before-reply | 의미 = "허브가 seq 부여 + 모든 viewer 우선 큐 enqueue 완료 후 응답". 렌더 보장 아님(렌더는 `core.metrics.report`). 미발행이면 성공 응답 대신 exit 8 `E_PARTIAL{published:false}` | Sol F4, 느린 뷰어가 RPC를 막지 않게 |
| CR-C17 | D5 종료 코드 | **11 `E_TEST_FAILED`** 신규(`test end`·`run --test`에서 판정 failed), 6에 `E_SESSION_PAUSED`. CI에서 실패 TC를 0이 아닌 코드로 구분하되 장치 오류 7·inconclusive 8과 섞지 않기 위함. **CORE 권고: 승인** | TEST R4 요청 6 |
| CR-C18 | A9 확장 | `TestPlan`·`RunPlan` 묶음 승인은 시작 ≤ 10분, 이후 **세션/run 종료·`max_test_duration`(기본 1 h, 설정 ≤ 8 h)·STOP·정책/epoch 변경 중 먼저 오는 때까지** 유효. 매 송신 `within_pattern` + 원자 예약 | A9는 승인 사용 기한 10분 + job renew만 정함, TEST TC-C3 |

### 15.2 타팀 요청 반영표 (`_xteam/*-core.md`, `core-ui.md`, `core-test.md`, `test-domains.md`, `ui-test.md` — 2026-11-10 판)

| 요청 | 답 | 위치 |
|---|---|---|
| uart #1 별도 worker | 수용(W3) | §3.1 |
| uart #2 적응형 배치 ≤ 10 ms + 원시 바이트 바이너리 | worker 프로토콜 수용(msgpack bin). UI WS 바이너리는 **M1 JSON(base64), M2 협상 binary** — 의도적 단계 차이, 완전 수용 아님 | §3.3, §3.4 |
| uart #3·#14 입력 빠른 경로·Ctrl-C 우선·STOP/기록 장애 중 열림 | 수용 — 연결 태스크 즉시 처리 + worker Ctrl-C mailbox, ctl 프레임 64 KiB로 대기 상한 | §0.2, §3.3, §10.3 |
| uart #4 입력 감사 묶음 | 수용 | §6.3, CR-C10 |
| uart #5 viewer = 브라우저 세션, 자기 승인 방지, 원격 | 수용: web bootstrap(fragment→POST)·presence, 원격은 SSH 터널(M5에 LAN·TLS·페어링) | §3.4, CR-C1 |
| uart #6 원시 바이트 무손실 저널 + seq 인덱스 | 수용 — worker 직접 세그먼트 + mapping, `chunks.jsonl` 불필요 | §6.2 |
| uart #7 `now` | 수용 | §4.5, CR-C3 |
| uart #8 안정 crate | 수용 — `loom-proto` 등 5개 | §3.9 |
| uart #9 증거 훅·원본 바이트 범위 API·스캔 예산 | 수용 — `EvidenceVerifier` + `JournalReader::scan(query, byte_limit)`(CallCtx), 예산은 검증기 공통 | §3.9, §10.9 |
| uart #10 `help --json` | 수용 | §10.11 |
| uart #11 soak 하네스 | 수용 | §9.6 |
| uart #12 `core.time`·`core.metrics.report` | 수용(이름 그대로) | §4.7 |
| uart #13 presence·`meta.viewers` | 수용 | §4.3, §4.7 |
| uart #15 exit 3 확장 | CR-C12(권고 승인) | §15.1 |
| uart #16 사전 승인 도메인 바인딩 | 수용 — `within_pattern` + 원자 예약, R4 제외 | §10.4, §10.9 |
| can W1–W4 | 수용 — 2 lane·바이트 크레딧·worker 직접 저널·원본 ID | §3.3, §6.2 |
| can W5 STOP ack | 수용 — 메모리 latch 먼저, worker ack p99 ≤ 10 ms, 미응답 표시 | §3.5 |
| can W6 재부착 | **M2**(M1은 orphan, 공백을 `source.gap{cause:gateway_restart}`로 표시) | §3.3 |
| can W7 hot path async 강제 안 함 | 수용 | §3.9 |
| can W8 지표·예산 단계 | 수용 | §3.3, §9.3 |
| can W9 DLL worker만, 재기동 시 수신만 | 수용 — `reopen_after_crash: RxOnly`(autostart 채널·identity 재검증 후) | §3.3 |
| can W10 Python 같은 프로토콜 | 수용(P0 wire golden, 실행 SDK P2–P4) | §3.3 |
| can §2 gateway 모듈 API | 수용 — CallCtx 링·저널 질의, `DisplaySummarizer`, 화면 드롭 후 `quality:stale` | §3.4, §3.9 |
| can §4 열 블록 바이너리 | M2 C29, CAN 필드 목록 반영(`core-ui.md`) | §3.4 |
| can §5 증거 훅·마커 | 수용 | §10.9 |
| can §6 soak·CI | 수용 — CORE 하네스, CAN 부하 생성기 | §9.6, §9.8 |
| uds C-1 훅 동기 ≤ 1 ms | 수용 — 세 훅 모두 동기(.await 없음) | §3.9 |
| uds C-2 digest·invalidate | 수용 | §10.4 |
| uds C-3 잡 API·idle gating·사용자 정지 | 수용 | §10.3, C23 |
| uds C-4 safe_state·`core.safe_state.run` | 수용 | §10.8 |
| uds C-5 snapshot provider·preview·커서 TTL | 수용 — 리듀서, viewer `preview`는 감사 제외·레이트 제한 | §3.4, §10.3 |
| uds C-6 이벤트 조회·마커·verify 훅·complete | 수용 | §4.7, §10.9 |
| uds C-7 패턴 승인·매 송신 재바인딩·원자 소비 | 수용 | §10.4 |
| uds C-8 seed-key worker·상한 | 수용 — helper worker, Q27 | §3.1, §9.2 |
| uds C-9 bootstrap TTY·데스크톱 셸만 | 수용 | §3.4 |
| uds C-10 기록 상태 판정 | 수용 | §10.5 |
| uds C-11 exit 3 | CR-C12 | §15.1 |
| uds C-12 재연결 정책 | CR-C13 | §10.10 |
| uds C-13 전용 런타임 | 수용 — `Dedicated` + 지터 관문, 실패 시 worker 이동 | §3.1 |
| core-ui CR-UI-01·03·07·08·10–16 (v0.2) | 수용 — binary M2 초반, `core.time` recv/send mono, rights 3단 + `presence`(M2 WebAuthn), `core.latch` payload + `POST /api/v1/stop`, localhost 오리진·헤더, `core.agent_cursor`·activity 범위, `core.safe_state.run{ids[]}`, `control{}` 뷰, WS 모드 fixture, soak UI 탭(M2), `core.ui.layout.*`·`core.replay.open` | §3.4, §4.7, §9.6 |
| core-ui U1–U8 | 수용(세부는 `core-ui.md` `## core 의견`): WS 1개 다중화, fragment→POST bootstrap, presence 필드, presence 2단 경고, 셸 전역 제어 구독, 패널 복원 snapshot, `coalesce_by`, binary opt-in(M2), `core.time`·metrics, HTTP NDJSON, `core.replay.open`(M3), `ui.focus_request`, layout RPC(M2), schema+TS 산출·해시 검사, 정적 임베드·dev proxy, **fixture 재생 stub P0** | §3.4, §4, §5 |
| TEST TC-C1–C9·판정 수정 | 수용 + R2 보강(expect 결속·coverage 분류·witness·pin lease·협력 예산). "device 증거 ≥ 1" 일괄 규칙은 witness 규칙으로 대체하되 **agent 자신의 tx만으로 PASS 불가는 유지**(TEST R-8) | §10.9 |
| TEST R4 추가 1–8(전수 재평가·역할 검사·anchor 창·plan_summary·freeze·집계/exit 11·개입 RPC·C2) | 전부 수용, exit 11은 CR-C17 | §10.9 |
| test-domains TC-C5/C7 | 수용 — 공통 `data.evidence[]` DTO, JSON Pointer 정본 = 발행 레코드 JSON | §4.3, §6.1 |
| ui-test | 수용 — pause/resume/skip/retry/ask/answer/plan.show RPC·역할, M1은 마커만 | §10.9 |

### 15.3 타팀 의존성

| 상대 | 우리가 필요 | 우리가 제공 |
|---|---|---|
| UART | `loom-console` Rust API(worker 안), 포트 예산 확인, flood·재연결 부하 생성기 | stub crate, worker SDK, InputLease/Input, `input.batch` |
| CAN | 2 lane 사용 확인, ISO-TP PDU를 ctl 우선 이벤트로, `can-hw/sim` 예산, CAN 부하 생성기, CANoe "왜 Rust 아님" 기록 | worker SDK·감독·저널, 잡형 API, `RateGuard`/`LatchFlag` |
| UDS | gateway 모듈(블로킹 금지·전용 런타임), `epoch→generation`(CR-C5), seed-key helper 규약 | 훅·승인 바인딩·잡형·safe_state·지터 측정 |
| EXPLORE/TEST | `core-test.md` `## test 의견`, 술어 AST 명세, 러너 단계 어댑터 | 세션 엔진·검증기·보고 데이터 |
| UI | `core-ui.md` `## ui 의견`(bootstrap 형식·presence UX·binary codec 스파이크 결과), 브라우저 지연 하네스 | WS 계약·스키마→TS·정적 임베드·`--fixture-replay`·`--sim-all` |
| 메인 | §15.1 CR 결정, `docs/contract/` 위치, core-sol 세션 재시작(STATUS 참조) | 계약 초안 C05 |

---

## 16. 결정 기록

| # | 쟁점 | Opus 안 | Sol 안 | 결정 | 이유 |
|---|---|---|---|---|---|
| 1 | v2.0 방향 전환 | — | — | Rust 코어, Python은 별도 worker 예외만, 웹 UI 1차, 통합 테스트 코어 | 마스터 C1–C8 |
| 2 | 프로세스 배치 | W1–W5 판정표, UDS gateway | DLL·CPU·지터 판정, 실패 시 worker 이동 | **Opus 판정표 + Sol 보조 기준·지터 관문** | 명확한 기준 + 측정 기반 탈출구 |
| 3 | worker 전송 | 로컬 소켓 2개 | 동일(interprocess) | **공통 채택** + ctl 안 STOP mailbox | HoL 분리, ctl 내부 대기도 상한 |
| 4 | 프레임 상한 | ctl 256 KiB, data 1 MiB | 전부 64 KiB + chunk | **64 KiB + chunk** | STOP 최악 대기 = 1프레임, 할당 상한 단순 |
| 5 | 크레딧 창·구독 큐 | 크레딧 4 MiB, 구독 2 MiB | 크레딧 512 KiB, 구독 512 KiB/8배치 | **크레딧 = 유입률 × 100 ms(기본 1 MiB), 구독 = 유입률 × 500 ms(1–4 MiB), 전체 32 MiB** | 512 KiB는 20k fps × 351 B에서 ≈ 73 ms뿐, 8배치 고정 큐는 Sol 첫 실행에서 손실(Opus E5) — 시간 기반 + 전체 상한 |
| 6 | 직렬화 | msgpack map(제어) + array(대량) | map + bin | **map + bin, array는 M2 후 옵션** | Python·진화 우선, array는 golden 후 |
| 7 | publish-before-reply | 50 ms 대기 후 `publish_lag` 경고로 응답 | 렌더 ACK 분리, 의미 정밀화 | **허브 enqueue 정의 + 미발행 시 `E_PARTIAL`** (CR-C16) | 경고는 순서 위반을 회복하지 못한다(Sol F4) |
| 8 | 저널 정본 형식 | msgpack LJ1 + SQLite | JSONL + raw + SQLite | **프레임 + CRC32C + codec id, M1 = JSON, M2 msgpack은 CR-C14** | D7 유지하면서 형식 진화 길을 열고, CR 누락 방지(Sol F8) |
| 9 | stream 인덱스 | 드롭 가능(재구축) | mapping 정본 분리 | **mapping 로그 = 정본, sparse 인덱스만 재구축 가능** | 원본만으로 전역 순서 복원 불가(Sol F3) |
| 10 | 저널 손실 시 정책 | degraded, agent 허용 | 손실 = 기록 불능 | **원본 손실 = failed → agent R1+ 차단, backlog만 = degraded** | A12 정신(Sol F5) |
| 11 | 승인 소비 | backend 후 consume | — | **presend 전 원자 예약, 제출 시 확정, 실패 시 해제** | 동시 once 이중 송신 방지(Sol F6) |
| 12 | STOP 순서 | latch.json fsync 후 worker 전달 | — | **메모리 latch → worker → 영속(실패해도 STOP 유효), 불명이면 latched** | 디스크 hang이 STOP을 막지 않게(Sol F7) |
| 13 | 메모리 수치 | gateway 256/512, ring 64, 64×32 구독 | gateway 256/384, ring 32, 전체 32 구독, 1 GiB admission | **Sol 전체 상한 체계 + 바이트 장부, UART 포트 예산식** | 합산 가능한 예산(Sol F1·F2) |
| 14 | soak 관문 | 기울기 ≤ 1 MiB/h + 범위 32/16 | ≤ 0.5 MiB/h + 중앙값 증가 max(16, 5 %) | **수명 구간별 ≤ 1 MiB/h AND 중앙값 증가 ≤ max(16 MiB, 5 %)**, 잠정 | 두 지표 결합, 재시작으로 누수가 숨지 않게 |
| 15 | CLI 클라이언트 | 동기 tungstenite, 30/60 ms | tokio 클라이언트, 50/500 ms | **동기, 30/60 ms, cold 1/1.5 s** | 실측 p95 3.3 ms [실측-mac] |
| 16 | JSON-RPC | 자체 라우터 | 제한 dispatcher, 규격 fixture | **자체 제한 dispatcher + 규격 fixture, jsonrpsee 대안 유지** | 구독·우선 큐 모델 독자적, 반증 없음 |
| 17 | 스키마 도구 | loom-proto + schemars → TS/Python | loom-schema 타입 crate, 생성기 버전 고정 | **이름은 Opus(proto=타입, schema=exporter), 도구·draft 고정은 Sol** | 혼동 제거 + 재현성 |
| 18 | P0 범위 | 6 stub + 동작 FakeGateway + echo, 1주 | 가벼운 stub + fixture gateway | **Day 1–2 compile-only, Day 3–5 FakeWorker·echo·schema·replay stub, 제품 허브 재현 안 함** | 첫 주 현실성(Sol §3) |
| 19 | 재부착 | M1 orphan, M2 재부착 | M1 재부착 | **M1 orphan + 공백 `source.gap` 표시, M2 재부착** | M1 범위 축소, W6 미충족을 명시(CAN 동의) |
| 20 | viewer-tty | 입력·STOP·R2 확인 | TTY 판정 회의적 | **M1 웹 viewer만. viewer-tty는 M2에 필요한 도메인이 있을 때 입력·hold·STOP만** | 에이전트 PTY 가능성, uart #5 브라우저 전환, 표·본문 불일치 해소 |
| 21 | 증거 PASS 규칙 | device 증거 ≥ 1 필수 | source ref·coverage·user assertion 분리 | **expect 결속 + coverage 분류 + witness 종류 규칙** | 정당한 부재 PASS 허용, 약한 술어 차단(Sol O-1–O-5) |
| 22 | 보존 GC 단위 | 세션 단위, 인용 세션 전체 보호 | 세그먼트·pin | **sealed 세그먼트 단위 + 증거 세그먼트 pin** | 24 h 열린 세션이 GC를 막지 않게(Sol F15) |
| 23 | 감사·데이터 sync | 감사 write + 그룹 100 ms, 데이터 1 s | 매 intent durable, 데이터 100 ms | **감사: R2+ intent 송신 전 sync, R1은 write + 그룹 100 ms. 데이터: 그룹 100 ms** — Windows 실측으로 재확인 | 전원 손실 시 '기록 없는 R2+ 쓰기'는 막을 가치, R1까지 sync하면 UART 지연 위협(양쪽 R2 리뷰 합의) |
| 24 | 메모리 hard 수단 | Job·RLIMIT_DATA를 RSS 상한처럼 | commit·가상 한계 구분 | **commit/가상 한계로 명시, RSS는 watchdog** | 사실 정정(Sol F10) |
| 25 | DoIP 경로 | VN5640 (b)면 Vector worker 안 smoltcp | 실측 대기 | **C4.1: OS TCP, UDS gateway 모듈 유지** | 메인 확정 |

---

## 핵심 원칙 자체 점검 (02-core-principle.md)
- [x] 맨 앞 핵심 루프 표 — §0.1
- [x] 종단 간 시나리오 — §0.2(UART, 홉별 예산·큐, 개입 3종), §0.3(CAN 고부하 + UDS R3)
- [x] 실시간성 목표·고부하 동작 — §0.4, §0.5, §9.2
- [x] M1 = 걷는 뼈대(sim 위 4요소 + 실제 브라우저 화면 게이트) — §0.6, §11
- [x] 사용자 화면이 에이전트 세션과 분리되지 않음 — §3.7 I1–I6, T1·T2·T7
- [x] 원칙에 기여하지 않는 기능은 후순위/비범위 — §1.3
