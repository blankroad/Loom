docs/plan/02-core-principle.md 준수.

# UI 팀 R1 초안 — ui-opus (Loom UI 앱: 셸·패널 프레임워크·사용자 개입 UX·안전 UX)

작성: ui-opus, 2026-11-10 · 입력: `02-core-principle.md`, `ui/brief.md`, `00-master-plan.md`(C1–C8 > B > A > D), `00-brief.md` §4·§5, `_xteam/viz-requirements.md`, `_xteam/can-core.md`, `can/plan.md` §3.7–3.8·§4.4–4.5·§5.3·§6.4·§10.3·§10.6, `uart/plan.md` §3.7·§3.11–3.12·§5, `uds/plan.md` §3.5–3.6·§5·§6.1, `explore/plan.md` §5.5 L·§6.6·§7.1–7.3, `test/brief.md`.
미존재(읽지 못함): `core/*.md` 초안, `test/plan.md`, `_xteam/core-ui.md`, `_xteam/ui-test.md`, `_xteam/*-ui.md`. 이 문서의 CORE·TEST 계약은 **요청안**이다(§15).
관점: 아키텍처 · 사용자/에이전트 UX · 패널 프레임워크 · 안전 UX. 라이브러리 벤치마크·전송 형식 측정은 ui-sol(`r1-sol.md`)이 맡으므로 §7은 판단 기준과 후보만 둔다.
표기: **[결정안]** = 이 초안의 제안, **[미확인]** = 확인 못 한 사실(검증 담당 표시), **M1** = 걷는 뼈대.

---

## 0. 핵심 루프

UI는 장치를 직접 열지 않는다. UI의 책임은 "에이전트가 장치에 쓰고 읽는 바로 그 데이터를 **같은 순간·같은 스트림**으로 사용자에게 보여 주고, 사용자 개입을 **같은 게이트**로 장치에 보내는 것"이다.

| 요소 | UI 쪽 구현 | UI가 보장하는 것 | 근거·검증 |
|---|---|---|---|
| 1 직접 접근 | 없음 — 장치는 `loomd` worker만 연다(D1, C1). 브라우저는 `loomd`의 WS 클라이언트일 뿐 | 브라우저가 장치를 **열 수 없음**: 패널 API에 소켓·fetch 없음(§4.2), `Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()`로 WebSerial/WebUSB 차단, CSP `connect-src 'self'` | §4.3 불변식 I-UI1, 시험 UI-T07 |
| 2 읽기 | `core.subscribe`(원자적 snapshot + `after_seq` live) 한 WS 다중화 → Web Worker 디코드 → 도메인 스토어(상한 있는 링) → 패널 | 에이전트가 읽는 스트림과 **같은 seq**를 본다. 에이전트의 읽기 행위(`trace`, `read`, `sig get`)도 Activity로 보인다(CAN I4) | §3.3, 시험 UI-T02 |
| 3 쓰기 | 패널 `ctx.act()` → 셸 ActionGate(자격·확인) → 도메인 쓰기 RPC(`uart.input`, `can.tx.send`, `uds.request` …). **origin=user는 서버가 연결 자격으로 부여**(D2) | 사용자 쓰기도 에이전트와 같은 정책·감사·이벤트 경로를 지나고, 결과는 D4/A14 쓰기 단계로 같은 행에서 갱신 | §5.2, 시험 UI-T04 |
| 4 사용자 실시간 표시 | 출처 배지(AGT/USR/DEV/SCR/EXT), 쓰기 단계 칩, `LIVE/LAG/STALLED/LIVE-DEGRADED`, `display.gap` 생략 행, 브라우저 측 `lag` 계측 | 장치→화면 p95: **UART 100 ms, CAN 200 ms(웹 Trace 목표 150 ms), UDS 100 ms, 플롯 250 ms**. 고부하에서도 멈춤·조용한 드롭 없음 | §3.5 예산, §3.6 정직성, 시험 UI-T03·T05 |

**비분리 보장**: UI는 데몬 하나에 붙는 일반 viewer 클라이언트다. 에이전트 전용 숨은 연결·토픽은 없다(CAN I2). UI를 언제 열어도 snapshot + live로 이어진다(늦은 합류 첫 화면 < 1 s). 뷰어 0개면 에이전트 결과에 `no_viewer_attached`, 탭이 숨겨져 있으면 `viewer_hidden`(신규 요청, §15)을 띄운다.

### 0.1 종단 간 시나리오 E-UI-1 (UART, M1 수용 시험 골격)

환경: `loomd` + `uart-sim`(pty 가짜 장치 `bootshell`), UI 탭 1개(쿠키 자격 operate), 에이전트 Claude Code. 모든 시각은 데몬 mono 기준(브라우저 값은 §3.5 시계 정렬로 환산).

| 시각 | 누가 | 동작 (명령·입력) | 이벤트 흐름 | 사용자 화면 (목표) |
|---|---|---|---|---|
| T0 | 사용자 | 브라우저에서 `http://localhost:7420/` (이미 1시간째 에이전트가 쓰는 중이어도 됨) | `core.subscribe{topics:[uart.dut.*, core.*], snapshot:true, max_batch_ms:10}` → `snapshot_part 1/3..3/3` → `snapshot_end{stream_seq:4409}` → live | < 1 s: 터미널에 최근 백로그(줄 경계부터) + `── 백로그 / 라이브 ──` 표식, 상태줄 `입력권: 비어 있음`, `LIVE lag 9ms` |
| T1 | 에이전트 | `loom uart send dut 'uname -a' --expect 'aarch64' --timeout 5s` | gateway 정책 R1(sim auto) → `uart.tx{request_id:r-88, write_state:requested}` seq 4410 → `accepted` 4411 → worker write → `sent` 4412 (**publish-before-reply**) | ≤ 40 ms: 상태줄 `A▶ "uname -a⏎" ◑보냄`, Activity 행, 트레이스 모드면 `A▶` 행 |
| T1+3 ms | 장치(sim) | 에코 + `Linux dut 6.6.21 … aarch64` | `uart.rx` 청크 seq 4413–4415 → 배치(적응형: 조용한 구간 뒤 첫 이벤트 즉시) → Worker 디코드 → xterm write | **p95 < 100 ms** 화면 반영. 에이전트 결과 `matched`, `others_since_last.count=0` |
| T2 | 사용자 | 터미널 클릭 후 `dmesg -w⏎` 입력 | 첫 키 → `uart.input`(쓰기 클래스 ③, A1) → worker InputArbiter `input_owner=user` → `uart.lease{owner:user, viewer:tab-1}` | 칩 `입력권: 나 (입력 중)` 노랑. 키 → 장치 에코 p95 < 100 ms |
| T3 | 에이전트 | `loom uart send dut 'reboot'` | InputArbiter: 사용자 입력권 중 → `deferred` 이벤트 | 칩 `⏳ 에이전트 대기 "reboot⏎" 0.0/10 s` 카운트다운 + [에이전트에게 반환 Ctrl-] y] |
| T4 | 사용자 | `Ctrl-C` (터미널 포커스 → 0x03 그대로 장치로) | 0x03 송신, 줄 종결 → 프롬프트 재출현 → 입력권 자동 반환(§3.7-3 UART) → 대기 send `sent` | 칩 `입력권: 비어 있음`, 상태줄 `A▶ "reboot⏎"`. 사용자 Ctrl-C 행에 `⇢ 에이전트 전달 대기` |
| T5 | 에이전트 | (T3 결과 수신) | 결과 `state: sent, deferred_ms: 5100, others_since_last{count:2, origins:[user]}` + `NOTE: user sent 2 inputs (dmesg -w⏎, ^C) since your last call` | 사용자 입력 행 표식이 `⇢ 에이전트에게 전달됨 (12:05:16)`로 바뀜 — 사용자는 **에이전트가 내 개입을 알았다는 사실**을 본다(§5.2.3) |
| T6 | 사용자 | 상단 **[■ STOP]** 클릭 (또는 어디서든 `Ctrl-] x`) | `core.stop_all` → 전 도메인 agent·script 쓰기·잡 latch (A3), 우선 제어 큐로 `core.stop_state` | ≤ 1 프레임 안에 STOP 띠 + STOP 상태 패널(§5.1.7). 사용자 키 입력은 계속 가능 |

UDS 변형(E-UI-2, M1): 에이전트 `loom uds read-did bcm F190` → UDS 콘솔에 같은 `request_id` 행이 `○요청 → ◔수락 → ◑보냄 → ●확정 F190 VIN=WVW…`로 **한 행에서** 진행(p95 ≤ 100 ms, 응답 수신→렌더). 사용자는 하단 명령줄 `22 F190⏎`로 직접 요청(USR 행), 또는 [제어 가져오기]로 lease 회수 → 에이전트 다음 R1+ 요청 exit 6, 결과 `others_since_last`에 `lease → user`.
CAN 변형(E-UI-3, M1): `can/plan.md` §2.0과 같은 표. 사용자 송신은 Trace 패널 하단 `0x300#00⏎`.

---

## 1. 범위 / 비범위

### 1.1 범위 (UI 팀 소유)
| 항목 | M | 핵심 루프 기여 |
|---|---|---|
| 웹 SPA (정적 자산 `loomd` 내장, 오프라인) | M1 | 4 |
| 인증 bootstrap·자격(watch/operate/approve)·원격 보기(SSH 포워딩) | M1(자격 분리) / M2(approve 존재 확인) | 3·4 |
| 공통 셸: 상단 바(연결·LIVE·REC·env·보는 사람·에이전트), **전역 STOP**, Activity, 레이아웃(M1 고정 프리셋 → M2 도킹·저장) | M1 | 3·4 |
| 우선 제어 경로(승인·STOP·오류·최종 결과) | M1 | 4 |
| 패널 프레임워크 `registerPanel` + 선언형 spec 패널 + 패널 명세 템플릿 | M1(코드 패널) / M3(spec 패널) | 전부 |
| 1차 패널: UART 터미널(xterm.js), CAN Trace(Chrono), UDS 콘솔 | **M1** | 전부 |
| 승인 대기열·승인 모달·자기 승인 차단 | M2 | 3(안전) |
| CAN Fixed·IG·Data, UDS DTC/세션, UART 트레이스 모드·hex | M2 | 3·4 |
| CAN Graphics, `loom.viz` 범용 렌더(파일·라이브), 통합 타임라인, 테스트 실행 화면, LA 레인·디코드 표, DLT | M3 | 4 |
| 에이전트 "가리키기"(`loom ui open/show`) | M1(최소: URL 열기) / M2(present 토스트) | 4 |
| 브라우저 측 지연 계측·`render_ack`·UI 포함 soak | M1 | 4 |

### 1.2 후순위
기록 탐색·재생 UI(M3 탐색, M5 Replay 블록), Statistics(M4), Tauri 셸(M4 — §3.8), 다국어(한국어 기본, 영어 M4), 테마 편집, 패널 팝아웃 창(M4), PNG export(M5).

### 1.3 비범위
- UI가 장치·드라이버·파일 포맷을 직접 다루는 모든 것(BLF/MF4 파싱은 CORE/CAN 질의 결과만 받음).
- 제3자 동적 패널 플러그인(임의 JS 로드). 필요해지면 sandbox iframe 설계부터(§4.5).
- TUI 구현. UART `loom uart attach`(터미널 모드)는 UART 팀 소유로 남고, CAN `monitor`·UDS `view` Textual TUI는 C1(Rust)·C6로 **폐기 권고**(§5.10).
- 인터넷 노출·다중 OS 계정 보안 경계(D6 위협 모델 밖).

---

## 2. 사용 시나리오

### 2.1 에이전트 관점
| # | 상황 | 에이전트 행동 | UI에 보이는 것 |
|---|---|---|---|
| A1 | 세션 시작, 뷰어 없음 | `loom uart send …` 결과에 `no_viewer_attached` → `loom ui open --layout m1-default` | 데몬이 OS 기본 브라우저를 직접 연다(부트스트랩 비밀은 에이전트 출력에 없음, §10.3). 에이전트 출력: `opened http://localhost:7420/#/l/m1-default viewers: 1 visible` |
| A2 | 사용자에게 특정 구간을 보여 주고 싶음 | `loom ui show can.trace --channel ch0 --filter 'id in 0x300,0x301' --at seq:5012 --reason "0x301 무응답 구간"` | 모든 탭에 "가리키기" 토스트(§5.8). 사용자가 [열기]를 누를 때만 레이아웃 변경(기본) |
| A3 | R3 동작(주기 송신)이 필요 | `loom can periodic start …` → exit 5 + `approval_id a17` → 채팅에 "UI에서 a17(코드 ··7K) 승인 부탁" | 승인 대기열 배지 `⚠ 승인 1`, 모달. 에이전트는 UI를 조작하지 않는다(스킬 규칙 + §10.3) |
| A4 | 테스트 TC 실행 | `loom test start tc.md` → 단계마다 `loom test step/verdict --evidence seq:…` | 테스트 실행 화면(§5.9): 현재 단계·증거 검증·사람 판정 요청 |
| A5 | 사용자가 개입한 뒤 | 결과 `others_since_last{count:3, origins:[user]}` → 다음 행동 전에 `--since @last`로 읽기 | 사용자 개입 행에 `⇢ 에이전트에게 전달됨` 표식 |

### 2.2 사용자 관점
| # | 상황 | 사용자 행동 | 결과 |
|---|---|---|---|
| U1 | 에이전트가 UART로 부팅 로그를 보고 있음 | UI 터미널에서 직접 `printenv⏎` | 입력권 → 나, 에이전트 send 대기, 에이전트에게 `others_since_last` |
| U2 | 에이전트의 CAN 주기 송신이 이상함 | 상단 [■ STOP] 또는 `Ctrl-] x` | 전 도메인 agent 쓰기 latch, STOP 패널에서 복구 송신 1키 |
| U3 | UDS RoutineControl이 0x78 pending으로 오래 걸림 | UDS 콘솔에서 행을 봄(카운터 증가·경과/한도), 필요하면 [중단 !] | 같은 행이 `pending ×12 3.1s/5.0s`로 계속 갱신, 중단 시 `⌛? 불확정` |
| U4 | 실차(`env=vehicle`)에서 DID 쓰기 승인 요청 | 모달에서 내용·hex·env 확인 → 코드 2자 + 존재 확인(Touch ID 등, M2) | 승인 1회, 감사에 승인 연결·방법 기록 |
| U5 | 벤치 PC(Windows)의 화면을 macOS에서 보고 싶음 | `ssh -L 7420:localhost:7420 bench` → `loom ui pair`(벤치 TTY에서) 코드 입력 | 원격 탭도 같은 스트림, `보는 중: 원격 1` 표시 |
| U6 | 버스 100% 부하 | Trace를 보며 Fixed로 전환 제안 배너 수락 | `LIVE-DEGRADED`, 생략 행 `⋯ 3,812 프레임 화면 생략 — 기록 정상` |

---

## 3. 아키텍처

### 3.1 결정 요약 [결정안]
1. **웹 SPA 정적 자산을 `loomd` 바이너리에 내장**(빌드 시 번들, 런타임 CDN·네트워크 0). 같은 오리진 `http://localhost:<port>/`에서 정적 파일 + `/rpc`(WS) + `/data/<id>`(대용량 GET) 제공(D2·A5). 호스트 이름은 `localhost`를 정본으로(WebAuthn RP ID 때문, §10.3 [미확인]), `127.0.0.1` 접근은 `localhost`로 리디렉트. 바인딩은 여전히 127.0.0.1.
2. **브라우저 탭 1개 = WS 1개**(다중화). WS·디코드는 전용 Web Worker, 메인 스레드는 스토어·렌더만.
3. **두 경로**: 제어 경로(승인·STOP·상태·오류·쓰기 최종 결과 — 즉시, 집계 금지) / 데이터 경로(프레임·바이트·트랜잭션 — 배치·예산·생략 표시).
4. **패널은 코어 데이터에만 접근**: 패널에 주어지는 것은 `PanelContext` 하나. 소켓·fetch·RPC 클라이언트 없음.
5. 데스크톱 셸(Tauri)은 M4 선택 — 브라우저가 가로채지 못하는 키(Ctrl-W 등)·OS 알림·창 관리가 필요해질 때(§3.8).

### 3.2 프로세스 구성도
```text
 벤치 PC (Windows x64 / Linux x64)                                           (선택) macOS
┌──────────────────────────────────────────────────────────────────────┐   ┌───────────────────┐
│ loomd (Rust)                                                          │   │ 브라우저 탭       │
│  ├ HTTP: / (내장 SPA) · /boot · /pair · /data/<id>   ┐ 127.0.0.1:7420 │◄──┤ ssh -L 7420:...   │
│  ├ WS /rpc: JSON-RPC 2.0 (+ 바이너리 블록, §6.2)      ┘               │   └───────────────────┘
│  ├ 이벤트 허브: seq · snapshot · 우선 제어 큐 · 구독자별 화면 예산     │
│  ├ 정책·승인·감사 · STOP latch · 쓰기 파이프라인                       │
│  └ worker 감독 ── uart-worker ── can-worker ── (uds in gateway) ...    │
│        ▲ 장치는 여기서만 열림                                          │
└────────┼──────────────────────────────────────────────────────────────┘
         │ WS (role=agent)            WS (role=viewer, 쿠키)        WS (role=viewer)
   ┌─────┴──────┐                ┌──────────────────────┐      ┌────────────────────┐
   │ loom CLI   │ ← 에이전트      │ 브라우저 탭 1 (operate)│      │ 탭 2 (watch, 관전) │
   └────────────┘                └──────────────────────┘      └────────────────────┘
```

### 3.3 브라우저 내부 구성도
```text
            WS /rpc (1개)
                │  text: JSON-RPC (제어·RPC·저속 이벤트)   binary: 열 블록 (대량 레인, §6.2)
┌───────────────▼──────────────────────────────────────────────────────────────┐
│ Net Worker (Web Worker)                                                       │
│  RpcClient(id 매칭, 64 KiB 상한 검사) · SubscriptionMux(sub_id → topic)        │
│  Decoder(JSON / 열 블록 → TypedArray) · ClockSync(core.time, 10 s 주기)        │
│  분류기: kind ∈ CONTROL? ──yes──► postMessage(즉시, 단건)        ─────────────┐ │
│                          └─no──► 배치 누적 → postMessage(transfer, ≤ 1/프레임) │ │
└───────────────┬─────────────────────────────────────────────────────────────┼─┘
                │ data batches (Transferable)                     control msgs  │
┌───────────────▼──────────────────────────────────────────┐  ┌────────────────▼──────────┐
│ Main thread                                              │  │ ControlStore (우선)        │
│  DomainStores (상한 링)                                   │  │  approvals · stop_state ·  │
│   uart: 바이트 링 + 줄 인덱스   can: 열 링(ts,id,dlc,..)  │  │  health · leases · errors  │
│   uds: request_id → 행 맵 + 순서 링   core: activity 링  │  │  → 셸 바 즉시 렌더          │
│  WriteTracker(request_id → 단계, 단조 갱신)               │  │   (rAF 1프레임, 스로틀 없음)│
│  RenderScheduler: rAF, 패널별 ≤ 20 Hz(터미널 60 Hz),      │  └────────────────────────────┘
│   프레임당 데이터 렌더 예산 8 ms, 남으면 다음 프레임       │
│  Panels ← PanelContext(구독 뷰 · 질의 · act · scheduler)  │
│  ActionGate: 자격 검사 · 확인 UI · RPC 위임 (Worker 경유)  │
└──────────────────────────────────────────────────────────┘
```
- **구독 관리자**: 패널이 같은 토픽을 요청하면 서버 구독 1개를 공유(참조 카운트). 필터는 가능하면 서버 측(`core.subscribe{filter}`)으로 내려 대역을 줄이고, 패널 고유 필터는 클라이언트 뷰로 처리.
- **도메인 스토어**: 탭당 1개, 패널 간 공유. 모든 링은 고정 상한(§6.4) — 넘치면 오래된 것부터 버리고 "화면 보관 범위 밖 → 질의로 조회" 표식(C2).
- **렌더 스케줄러**: 데이터가 와도 즉시 그리지 않고 rAF에 모은다. 패널별 상한(트레이스·표 20 Hz, 터미널 60 Hz, 플롯 20 Hz). 프레임 예산 초과 시 해당 패널 `LIVE-DEGRADED` 후보.
- **우선 제어 경로**: Worker에서 `CONTROL` kind(`core.approval_*`, `core.stop_state`, `*.state`, `record.health`, `display.gap`, 쓰기 최종 단계 `confirmed|failed|outcome_unknown|effective_*`, `core.error`, heartbeat)는 배치에 섞지 않고 단건 즉시 전달. 서버도 우선 제어 큐로 보낸다(D2). 메인 스레드가 데이터 렌더로 막히지 않도록 프레임 예산이 제어 렌더 시간을 남긴다.
- **숨은 탭**: `document.hidden`이면 렌더 중단, 스토어 반영만(링 상한 그대로). 보이면 최신 상태로 한 번에 그림 + `⋯ 숨김 중 12.4 s (51k 프레임) — 라이브로 이동`. 숨김 상태는 서버에 알림(`core.viewer_state{visible:false}`, 요청) → 에이전트 경고 `viewer_hidden`.

### 3.4 RX/TX 흐름 (UI 구간)
- RX: `core.events` 배치 도착(Worker) → 디코드 → (제어면 즉시) → 메인 스토어 append → 패널 invalidate → 다음 rAF 렌더 → 렌더 완료 시각 기록(지연 계측 표본, 1 Hz `render_ack`).
- TX(사용자): 패널 `ctx.act(id, params)` → ActionGate(자격·위험 등급별 확인) → Worker → `<domain>.<verb>` RPC → 서버가 `origin=user, client_id=viewer:<tab>` 부여 → 이벤트 `requested` … → 같은 행 진행. RPC 응답보다 이벤트가 먼저 온다(publish-before-reply) → UI는 **이벤트로만 행을 그린다**(응답은 오류 표시용).
- TX(키 스트림, UART): xterm `onData` → 5 ms 묶음(붙여넣기 대비) → `uart.input{channel, bytes_hex, client_seq}` — 확인 UI 없음(입력권 획득 시 1회 정책 판정, A1 ③). Ctrl-C(0x03)는 묶음 대기 없이 즉시.

### 3.5 지연 예산 (브라우저 쪽 분해)과 계측

**구간 정의**: `t_dev`(worker host ingest `ts.mono_ns`) → `t_send`(gateway WS write) → `t_recv`(Worker onmessage) → `t_dec`(디코드 완료) → `t_apply`(스토어 반영) → `t_paint`(렌더 커밋 후 다음 rAF 시작 = 화면 반영 근사).
UI 책임 구간 = `t_recv → t_paint`. CORE 책임 = `t_dev → t_send`(배치 대기 포함).

| 경로 (p95 목표) | CORE: ingest→hub | 배치 대기 | WS 전송(로컬) | Worker 디코드+전달 | 스토어 반영 | 렌더 대기(스케줄러) | 렌더·페인트 | 합계 예산 | 여유 |
|---|---|---|---|---|---|---|---|---|---|
| UART 터미널 100 ms | ≤ 10 | ≤ 10 (`max_batch_ms:10`, 적응형) | ≤ 2 | ≤ 5 | ≤ 2 | ≤ 17 (60 Hz) | ≤ 20 (xterm parse+WebGL) | 66 | 34 |
| UDS 콘솔 100 ms | ≤ 10 | ≤ 40 (적응형: 조용한 구간 뒤 즉시) | ≤ 2 | ≤ 3 | ≤ 2 | ≤ 17 (최종 단계 = 제어 경로) | ≤ 10 | 84 | 16 |
| CAN 웹 Trace 150 ms (200 ms 상한) | ≤ 20 | ≤ 40 | ≤ 5 | ≤ 10 | ≤ 5 | ≤ 50 (20 Hz) | ≤ 15 | 145 | 5 / 55 |
| 플롯(신호·DID·추출값) 250 ms | ≤ 20 | ≤ 40 | ≤ 5 | ≤ 10 | ≤ 10 (피라미드 갱신) | ≤ 50 | ≤ 30 | 165 | 85 |

**UI 측 하위 목표**(`t_recv → t_paint` p95): UART ≤ 45 ms, UDS ≤ 35 ms, CAN ≤ 85 ms, 플롯 ≤ 100 ms. 이 값이 UI 수용 기준이고, 전 구간 목표는 CORE와 합동 판정.
UDS 행이 20 Hz 스로틀에 묶이지 않도록 **쓰기 최종 단계 이벤트는 제어 경로**로 보낸다(위 표의 렌더 대기 17 ms).

**시계 정렬 (데몬 mono ↔ 브라우저 `performance.now()`)**
1. Worker가 `core.time{}` RPC를 연결 직후 8회, 이후 10 s마다 1회 호출. 요청 직전 `c0`, 응답 수신 `c1`(둘 다 `performance.timeOrigin + performance.now()` 기반 ms), 응답 `server_mono_ns`(요청 처리 시점).
2. 표본별 `offset = server_mono − (c0+c1)/2`, `err = (c1−c0)/2`. 최근 60 s 창에서 **RTT 최소 표본**을 채택(NTP식). 같은 호스트면 err < 1 ms 예상 [미확인 — ui-sol S1 측정]. 원격(SSH 포워딩)이면 err를 화면 툴팁에 표시.
3. 브라우저 시각을 데몬 mono로 환산: `mono_est(now) = now + offset`. `lag(ev) = mono_est(t_paint) − ev.ts.mono_ns`.
4. 단조 보정: offset 변화가 한 번에 5 ms 넘으면 표시값을 1 s에 걸쳐 수렴(배지 깜빡임 방지). macOS sleep·탭 throttle 복귀 후에는 즉시 재동기화(8회).
5. **`lag` 표시 정의(UART §3.11-6 준용)**: 패널 lag = 아직 그리지 못한 가장 오래된 이벤트의 나이. 받은 게 없으면 lag 0 + `idle 5.2s`. 셸 배지는 보이는 패널 lag의 최댓값.
6. **render_ack(요청)**: 탭이 1 Hz로 `core.render_ack{sub, last_rendered_seq, paint_mono_est, err_ms, visible}` 송신 → 데몬이 뷰어별 지연을 알고 에이전트 `status`에 `viewers[{lag_ms, visible}]` 표시, 시험 하네스가 같은 값으로 p95 계산(브라우저 내부 값만으로 판정하지 않고 서버 수신 시각과 교차 확인).

### 3.6 고부하 정직성 (UI 규칙)
| 상황 | UI 동작 | 표시 |
|---|---|---|
| 서버가 이 구독자 몫을 드롭 | `display.gap{first_seq,last_seq,count,cause}` → 데이터 패널에 합성 행/터미널 장식(스트림에 바이트 삽입 금지) | `⋯ 3,812 프레임 화면 생략 (seq 9001–12812) — 기록 정상` (`기록 정상`은 `record.health`가 그 구간 ok일 때만) |
| UI 자체 렌더 예산 초과 | 패널이 행 렌더를 건너뛰고 생략 카운트 | 패널 헤더 `LIVE-DEGRADED · 화면 41k fps → 2k 행/s` |
| lag > 목표 | 노랑 `LAG 620ms`; > 2 s 빨강 + 해당 패널 backlog 버리고 최신으로 점프 | `⋯ 2.3 s 건너뛰고 라이브로 이동` |
| heartbeat 3 s 미수신 | 전 패널 회색 오버레이, 입력 비활성(쓰기 버튼 잠금) | `STALLED 3.4 s — 데몬 응답 없음`; 재연결 시 `after_seq`로 이음, 못 이으면 `gap` 행 + 질의로 채움 시도 |
| WS 끊김 | 지수 백오프 재연결(0.5→8 s), 재연결 동안 **쓰기 버튼 전부 잠금**(STOP 제외 — STOP은 HTTP POST `/stop` 대체 경로, §15 요청) | `DISCONNECTED 재연결 중 3s` |
| 다운샘플 | 플롯 축 옆 표기, 커서·통계는 원본 질의 | `downsampled 1,000,000→4,000` |
| 손실 3층 | `source.gap`·`record.gap`은 빨강 배지, `display.gap`은 회색 | `SOURCE LOSS`, `RECORDING INCOMPLETE`, `화면 생략` 문구 분리(A13) |
| 일시정지 | 수집·스토어는 계속, 렌더만 정지 | `PAUSED +3,214행 버퍼` |
| **우선 이벤트** | 승인·STOP·상태·오류·최종 결과·gap은 어떤 스로틀·집계·숨김 탭에서도 반영(숨김 탭은 복귀 즉시 + 승인은 OS 알림 선택) | — |

### 3.7 인증·자격 흐름
```text
loom ui open (에이전트·사용자 공용 CLI)
  └► RPC ui.open{layout?, panel?} (agent role 허용)
       └► loomd: 1회용 부트스트랩 코드 c(128 bit, 60 s, 1회) 생성
            └► loomd가 직접 OS 기본 브라우저 실행: http://localhost:7420/boot#c   ← 코드는 CLI 출력에 없음
                 └► /boot: c 검증 → Set-Cookie loom_v=<세션>; HttpOnly; SameSite=Strict; Path=/
                      → 자격 = operate (기본), 이후 approve는 존재 확인 등록 후 (M2)
CLI 출력(에이전트가 보는 것): url(비밀 없음), opened:true, viewers:{n, visible}

원격·헤드리스: 벤치에서 사람이 `loom ui pair` (TTY 필수, 6자리 코드, 120 s) → 원격 브라우저 /pair에 입력 → 쿠키
WS /rpc 업그레이드: Origin=localhost:7420 · Host 검사 · 쿠키 → hello{want_role:viewer} → 서버가 role·rights 부여
```
자격 3단(§10.2): **watch**(보기 + STOP·비상 OFF·거부 — 안전 방향만), **operate**(+ 사용자 쓰기·입력·take-control·hold·복구 송신·마커), **approve**(+ R2 뷰어 확인·R3/R4 승인·정책 완화·`tx resume`·`unlock`).

### 3.8 Tauri 셸 — 언제·왜 [결정안: M4 선택]
| 이유 | 웹으로 해결? |
|---|---|
| 터미널에서 Ctrl-W/Ctrl-T/Ctrl-N이 브라우저 단축키로 먹힘(셸에서 Ctrl-W = 단어 지우기) | 부분: `beforeunload` 경고 + `Ctrl-] w`로 0x17 송신 대체, Keyboard Lock API는 전체화면에서만 [미확인 — ui-sol] |
| OS 알림·트레이에서 승인 대기 알림 | 웹 Notification API로 대부분 가능 |
| 승인 존재 확인 | WebAuthn으로 웹에서 가능(§10.3) |
| WebView2(Windows)·WebKitGTK(Linux) 렌더 성능 차이 | Tauri의 **위험** — ui-sol 측정 |
결론: M1–M3는 브라우저(Chromium 계열 1급). Tauri는 키 가로채기 불만이 실측되면 M4에 얇은 셸로(같은 SPA, 같은 WS — 별도 경로 금지).

---

## 4. CLI / API 명세 초안

### 4.1 UI 관련 CLI (CORE `loom` CLI에 추가 요청)
| 명령 | 역할 | 위험 | M |
|---|---|---|---|
| `loom ui open [--layout NAME] [--panel ID --<prop> V …] [--no-browser]` | 데몬이 브라우저를 열고 URL(비밀 없음) 반환. `--no-browser`면 URL만(이미 쿠키 있는 브라우저에서 열기) | R0 | M1 |
| `loom ui show PANEL [--channel] [--filter EXPR] [--at seq:N\|mark:X\|-5s] [--reason TEXT]` | 열린 탭들에 "가리키기"(present) 요청. 사용자가 수락할 때만 화면 변경 | R0, 2 s당 1회 | M2 |
| `loom ui status --json` | 탭 수·visible·lag·자격·레이아웃 | R0 | M1 |
| `loom ui pair` | 사람 전용(TTY 필수), 원격 브라우저 페어링 코드 | — | M1 |
| `loom ui layout list\|get\|put NAME -f file.json` | `~/.loom/layouts/` 관리. put은 사용자 레이아웃 덮어쓰기 금지(새 이름 강제) | R0 | M2 |

출력 예(`--json`, D5 봉투):
```json
{"schema_version":1,"ok":true,"cmd":"ui.open",
 "data":{"url":"http://localhost:7420/#/l/m1-default","opened":true,"viewers":{"n":1,"visible":1,"max_lag_ms":38}},
 "meta":{},"warnings":[],"others_since_last":{"count":0,"origins":[],"from_cursor":"seq:4409"}}
```

### 4.2 패널 프레임워크 API (TS 초안)
```ts
// @loom/panel-sdk — 패널이 import할 수 있는 유일한 패키지
export type Right = 'watch' | 'operate' | 'approve';
export type Risk = 'R0' | 'R1' | 'R2' | 'R3' | 'R4';

export interface TopicSpec {
  key: string;                       // 'rx' — 패널 내부 이름
  topic: string;                     // 'uart.{channel}.*' — props로 치환
  kinds?: string[];                  // ['uart.rx','uart.tx','uart.lease','display.gap']
  serverFilter?: (props: any) => object;  // core.subscribe{filter}로 내려감
  encoding?: 'json' | 'columnar';    // 대량 레인은 columnar(§6.2)
  snapshot: 'required' | 'none';
  maxBatchMs?: number;               // 터미널 10
}
export interface ActionSpec {
  id: string;                        // 'input' | 'send' | 'takeControl' | 'abort' ...
  rpc: `${string}.${string}`;        // 'uart.input'
  class: 'request' | 'job' | 'stream';   // A1 쓰기 클래스
  right: Right;                      // 최소 자격
  risk: Risk | 'by_server';          // 서버 classify 결과를 따르면 by_server
  confirm: 'none' | 'enter_twice' | 'modal';  // UI 측 실수 방지(서버 정책과 별개)
  safetyDirection?: 'reduce';        // STOP·abort·비상 OFF: watch 자격으로도 허용, 확인 없음
}
export interface PanelManifest {
  id: string;                        // 'uart.terminal'
  domain: string; title: string; version: 1;
  milestone: 'M1' | 'M2' | 'M3' | 'M4';
  props: JSONSchema;                 // 인스턴스 인자(channel, filter, mode) — 레이아웃·URL 직렬화 대상
  topics: TopicSpec[];
  queries: string[];                 // 읽기 RPC 허용 목록: ['uart.trace.query']
  actions: ActionSpec[];
  memory: { maxRows?: number; maxBytes: number };   // 탭 메모리 예산 할당(§6.4)
  render: { maxHz: number; budgetMs: number };       // 스케줄러 한도
  layout: { minW: number; minH: number; prefer: 'main' | 'side' | 'bottom' };
}
export interface StoreView<T> {      // 읽기 전용, 변경 불가
  snapshotDone: boolean; headSeq: bigint; gaps: GapMark[];
  range(fromSeq: bigint, toSeq: bigint): Iterable<T>;
  tail(n: number): Iterable<T>;
  onAppend(cb: (fromIdx: number, toIdx: number) => void): Unsubscribe;
}
export interface WriteHandle {       // D4/A14 단계를 구독
  requestId: string;
  state(): WriteState;               // requested|accepted|sent|confirmed|effective_verified|effect_unverified|failed|outcome_unknown
  onChange(cb: (s: WriteState, ev: LoomEvent) => void): Unsubscribe;
}
export interface PanelContext {
  readonly props: Readonly<Record<string, unknown>>;
  readonly rights: ReadonlySet<Right>;
  readonly env: 'sim' | 'bench' | 'vehicle';
  store<T>(topicKey: string): StoreView<T>;                 // manifest.topics에 없는 key → throw
  query<R>(rpc: string, params: object): Promise<R>;       // manifest.queries에 없는 rpc → throw, 64 KiB 초과는 셸이 /data GET으로 대신
  act(actionId: string, params: object): Promise<WriteHandle>;  // ActionGate 경유(자격·확인·감사 표시)
  scheduler: { invalidate(): void; onFrame(cb: (budgetMs: number) => void): Unsubscribe };
  clock: { monoEst(): bigint; errMs(): number };
  cursor: { link(group: string): void; set(seq: bigint): void; on(cb: (seq: bigint) => void): Unsubscribe };
  ui: { openPanel(id: string, props: object): void; mark(label: string): Promise<void>; toast(msg: string): void };
}
export interface PanelInstance { mount(el: HTMLElement): void; resize(w: number, h: number): void; dispose(): void; }
export function registerPanel(m: PanelManifest, factory: (ctx: PanelContext) => PanelInstance): void;
```

### 4.3 "패널은 장치를 열 수 없다" 불변식의 강제 (I-UI1)
| 층 | 수단 | 막는 것 |
|---|---|---|
| API | 패널은 `PanelContext`만 받는다. RpcClient·WS·쿠키 접근 없음. `store/query/act`는 manifest 선언분만 허용(런타임 throw) | 선언 안 된 토픽·쓰기 |
| 빌드 | 패널 패키지(`ui/panels/**`)에 ESLint `no-restricted-globals`·`no-restricted-imports`: `WebSocket, fetch, XMLHttpRequest, EventSource, WebTransport, navigator.serial/usb/hid/bluetooth`, `@loom/net` import 금지. CI 실패 = 머지 금지 | 우회 코드 유입 |
| 브라우저 | 응답 헤더 `Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()` [미확인: 디렉티브 이름·브라우저 지원 — ui-sol], CSP `default-src 'self'; connect-src 'self'; script-src 'self'; frame-ancestors 'none'` | WebSerial로 COM 직접 열기, 외부 송신 |
| 서버 | WS 업그레이드에 셸이 받은 **1회용 WS 티켓** 요구(쿠키만으로 두 번째 WS 불가, 탭당 1연결) | 패널이 쿠키로 별도 WS 생성 |
| 리뷰 | 도메인 팀 패널 PR은 UI 팀 리뷰 필수, manifest diff 자동 표시 | — |

### 4.4 선언형 spec 패널 (탐색 §6.6 PanelSpec 수용, M3)
코드 없이 도메인이 Rust Registry에 JSON spec을 올리면 데몬 `core.ui.panels`로 내려오고, UI의 범용 렌더러가 그린다. 타입: `table`, `trace_table`, `timeseries`, `state_lamps`, `gauge`, `console`(UART 터미널 재사용), `logic_lanes`(전이 보존, LTTB 금지), `decode_table`, `timeline_lane`, `run_panel`. 규칙: payload에 실행 코드·HTML 금지, 텍스트는 항상 textContent, actions는 `ActionSpec` 같은 형식(서버가 RPC 이름을 Registry로 검증).
```json
{"id":"psu.bench","type":"gauge","title":"PSU bench","source":{"kinds":["psu.meas","psu.state"],"channel":"bench"},
 "fields":[{"path":"payload.volt","unit":"V"},{"path":"payload.curr","unit":"A"}],
 "actions":[{"id":"off","rpc":"psu.out","params":{"state":"off"},"right":"watch","safetyDirection":"reduce","label":"비상 OFF"}]}
```

### 4.5 등록 방식 결정 [결정안]
- **정적 번들**: 모든 코드 패널은 저장소 `ui/panels/<domain>/`에서 함께 빌드되어 `loomd`에 내장. 버전 불일치·보안 검토·오프라인을 한 번에 해결.
- 도메인 팀 기여 방식: (1) 패널 명세(`_xteam/<comp>-ui.md`, 아래 템플릿) → UI 팀 구현(1차 패널), 또는 (2) 도메인 팀이 `ui/panels/<domain>/`에 PR(UI 리뷰 필수), 또는 (3) spec 패널(JSON, 코드 없음).
- 동적 로드(임의 JS)는 비범위. 언젠가 필요하면 sandbox iframe(`allow-scripts`만) + postMessage로 `PanelContext` 프록시.

### 4.6 패널 명세 템플릿 (`_xteam/<comp>-ui.md`에 도메인 팀이 채울 양식)
```markdown
## 패널 <id> (예: uds.console) — 소유: <팀> 내용 / UI 팀 구현 — M<n>
### 1. 목적·핵심 루프 기여
- 사용자가 이 패널로 보는 것 / 개입하는 것 (각 1–3줄)
### 2. 데이터 (구독)
| topic | kinds | 필드(스키마 경로, 단위, 표시 형식) | snapshot에 필요한 것 | 예상 레이트(정상/최대) |
### 3. 질의 (읽기 RPC)
| rpc | 언제(스크롤 백·행 펼침·재디코드) | 최대 결과 | 64 KiB 초과 시 /data 사용 여부 |
### 4. 동작 (쓰기)
| action id | rpc | 쓰기 클래스(request/job/stream) | 위험(R0–R4 또는 서버) | 최소 자격 | UI 확인 | 키·버튼 | safetyDirection |
### 5. 화면
- ASCII 와이어프레임(정상·좁은 폭 80열·빈 상태·오류 상태)
- 열 정의, 출처 배지 규칙, 행 상태 기호(D4/A14 단계 → 기호 대응)
### 6. 실시간·고부하 규칙
- 지연 목표(장치→화면 p95/p99), 렌더 Hz 상한
- 집계·생략 규칙(무엇을 묶는가, 절대 묶지 않는 이벤트), display.gap 표시 위치
- Fixed/최신값 모드 여부
### 7. 늦은 합류·재접속
- snapshot 내용, 재접속 시 이음 실패 처리
### 8. 메모리 상한
- 보관 행/바이트, 넘칠 때 동작(오래된 것 버림 + 질의 링크)
### 9. 시험 fixture
- 이벤트 JSONL 경로(정상·고부하·gap·쓰기 실패·outcome_unknown), 기대 화면 상태 목록
### 10. 미결·질문
```

### 4.7 UI가 쓰는 코어 RPC (요약, 정확한 이름은 CORE 계약 v1)
`core.hello` · `core.subscribe/unsubscribe` · `core.time`(신규) · `core.render_ack`(신규) · `core.viewer_state`(신규) · `core.approval.list/respond`(viewer 전용) · `core.approval.presence.challenge`(신규, M2) · `core.stop_all` · `core.tx_resume` · `core.recover.list/run`(A15 복구 송신, 신규 이름) · `core.mark` · `core.activity.query` · `ui.open/show/layout.*`(신규) · `core.ui.panels`(spec 패널, M3) · 도메인 `<domain>.<verb>`.

---

## 5. 사용자 뷰어 UX

### 5.1 화면 설계 (와이어프레임)

#### 5.1.1 공통 셸 전체 — **M1**
```text
┌ Loom · bench-pc · [SIM] ─────────────────────────────────────────────────────────────────────────────────┐
│ ● LIVE lag 38ms │ REC ok │ 보는 중: 이 탭 + 원격 1 │ 에이전트 claude-1 활동 2s 전 │ ⚠ 승인 1 │ [■ STOP 전체] │
├ 레이아웃: m1-default ▾ ──────────────────────────────────────────────────────────────── 자격: operate ─┤
│ ┌ UART dut ─────────────────────────────────────┐ ┌ CAN ch0 Trace ──────────────────────────────────────┐ │
│ │ (터미널 패널 §5.1.2)                          │ │ (트레이스 패널 §5.1.3)                              │ │
│ │                                               │ │                                                     │ │
│ └───────────────────────────────────────────────┘ └─────────────────────────────────────────────────────┘ │
│ ┌ UDS 콘솔 ─────────────────────────────────────────────────────────────────────────────────────────────┐ │
│ │ (UDS 패널 §5.1.4)                                                                                      │ │
│ └────────────────────────────────────────────────────────────────────────────────────────────────────────┘ │
├ Activity ──────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 12:05:11.020 AGT claude-1 uart.send dut "uname -a⏎" ◑보냄 │ 12:05:11.300 AGT claude-1 can.trace 읽음 3행     │
│ 12:05:16.410 USR 나 uart ^C  ⇢ 에이전트에게 전달됨          │ 12:05:20.402 AGT claude-1 uds.read-did F190 ●확정 │
└ Ctrl-] x STOP · Ctrl-] A 승인 · Ctrl-] / 패널 찾기 · Ctrl-] ? 도움말 ─────────────────────── ● 12:05:21 ┘
```
- **env 배지**는 생략 금지(B4): `[SIM]` 회색, `[BENCH]` 파랑, `[VEHICLE]` 반전 주황 + 상단 바 테두리 색.
- 맨 오른쪽 아래 `● 12:05:21` = 1 Hz 하트비트 시계(데이터가 없어도 갱신 — 화면이 멈추면 사용자가 안다).
- **[■ STOP 전체]**는 모든 레이아웃·모든 자격(watch 포함)에서 항상 같은 자리, 확인 없음(안전 방향).
- M1 레이아웃은 고정 프리셋 3개(`m1-default`, `uart-only`, `can-uds`) + 패널 최대화. 드래그 도킹·저장은 M2.

#### 5.1.2 UART 터미널 패널 — **M1**
```text
┌ UART dut ● OPEN 115200 gen1 [SIM] ─ 입력권: 비어 있음 ─ [터미널|트레이스] [HOLD] [⋯] ───────── LIVE 12ms ┐
│[    3.112] systemd[1]: Started Journal Service.                                                          │
│dut login: root                                                                                           │
│── 백로그 / 라이브 ── (장식, 스트림에 삽입하지 않음)                                                        │
│root@dut:~# uname -a                                                                                      │
│Linux dut 6.6.21 #1 SMP PREEMPT aarch64 GNU/Linux                                                         │
│▒ 화면 생략 1.2 MB (seq 81102–93220) — 기록 정상 ▒   ← xterm decoration(행 장식), 이후 SGR 리셋·파서 재동기 │
│root@dut:~# █                                                                                             │
├──────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ A▶ "uname -a⏎" ◑보냄 2s 전 │ 에이전트: expect "aarch64" 3.1/5s │ rx 1.2 KB/s │ 차단 OSC52×0 │ REC ok     │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
입력권 칩 상태(§5.2.2): `비어 있음`(회색) · `나 (입력 중)`(노랑, [에이전트에게 반환 Ctrl-] y]) · `다른 뷰어 tab-3`(회색, 내 키는 보류) · `🔒 에이전트 잠금 "u-boot env 수정" 87s`([가져오기 Ctrl-] o]) · `⏳ 에이전트 대기 "reboot⏎" 3.2/10s` · `미완결 줄 남음`(주황, [지우기 Ctrl-] c]) · `HOLD`(빨강) · `PAUSED — 키 송신 안 됨`(Ctrl-C만 통과).
- 포커스 규칙: 터미널에 포커스가 있을 때만 원시 키 전달. 포커스 테두리 색으로 "지금 키가 장치로 간다"를 표시.
- Ctrl-C: 선택 영역이 있으면 복사, 없으면 0x03 [결정안 — xterm 기본 동작 확인 필요, ui-sol]. 복사는 `Ctrl-Shift-C`도 제공.
- 장치 출력 보안(UART §5.2의 웹 대응): xterm.js가 화면 모델을 가지므로 상태줄 보호 문제는 사라진다. 남는 것: **OSC 52(클립보드)·OSC 8(링크) 차단, 제목 변경 무시, 단말 질의(DA/DSR) 응답 억제** — xterm은 질의 응답을 `onData`로 내보내 사용자 키처럼 장치로 갈 수 있으므로 CSI 핸들러로 삼켜야 한다 [미확인: xterm.js 동작 — ui-sol].
- 트레이스 모드(M1 최소): 행 = RX 줄/TX 이벤트, 열 = 시각·출처(`D/A▶/U▶/S▶`)·내용(보이지 않는 문자 기호화).

#### 5.1.3 CAN Trace (M1 Chrono) + Graphics (M3)
```text
┌ CAN ch0 Trace · virtual 500k [SIM] load 3% ─ [Chrono|Fixed M2] [Sym|Raw M2] filter [id in 0x300,0x301 ] ─ LIVE 54ms ┐
│ t(s)      dir 출처        id     dlc data                     상태      비고                                    │
│ 812.004   tx  AGT         0x300  1   01                       ●확정     claude-1 r-311                          │
│ 812.009   rx  DEV         0x301  1   01                                 sim:BCM                                 │
│ 812.050   rx  DEV         0x310  8   01 00 00 00 00 00 00 00                                                    │
│ ⋯ 3,812 프레임 화면 생략 (seq 9001–12812; 0x0FD×1200 …) — 기록 정상                                             │
│ 818.731   tx  USR(나)     0x300  1   00                       ●확정     ⇢ 에이전트에게 전달됨                    │
│ 818.736   rx  DEV         0x301  1   00                                 sim:BCM                                 │
├────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 송신 [0x300#00           ] [보내기 ⏎] R1(sim) │ ⏸ 일시정지 │ 보이는 5,000 / 탭 링 200k │ 수신 4.1k fps · 화면 2k 행/s │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘

(M3) ┌ GRAPHICS [+신호] [커서1|2] [맞춤] [동기: 그룹 A] ───────────────── downsampled 1,000,000→4,000 ─ LIVE 120ms ┐
     │ VehicleSpeed ── km/h   ╱‾‾‾╲___╱‾‾┊‾‾‾‾‾‾‾┊╲                                                          │
     │ EngRPM       ── rpm    ___╱‾‾‾‾‾‾‾┊‾‾ ‖ ‾‾┊‾  (‖ = gap, 선 끊김 — NaN 보간 금지)                        │
     │ GearPos(state) [P][   D   ┊     ][N]                                                                    │
     │ 커서 Δ 11.2 s  VehicleSpeed 12.0 → 37.25 (원본 값)   AGT 송신 마커 ▼ 812.004                              │
     └────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
- 출처 배지 통일 [결정안]: **AGT 청록 · USR 노랑 · DEV 회색 · SCR 보라 · EXT 보라 점선**(UART·UDS·탐색 다수안, CAN 계획의 AGT 주황·USR 파랑을 맞춤 — CAN 팀 동의 필요). 색 + 문자 병기(색각 이상 대비). 내 입력은 `USR(나)`, 다른 탭은 `USR(tab-3)`.
- Chrono 화면 예산 2,000행/s, 보이는 행 5,000, 탭 링 200k(§6.4). 고부하 시 "Fixed로 전환" 배너(M2).

#### 5.1.4 UDS 콘솔 — **M1**
```text
┌ UDS ─ bcm ● DoIP 0x1001 sess=extended✓ sec=L1🔓 TP ✓×143 P2 50/5000 [SIM] ─ 제어: AGT claude-1 [제어 가져오기] ┐
│        ecm ● CAN 7E0/7E8 sess=default? sec=🔒 TP off                         ─ 제어: 비어 있음                │
├──────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 시각          출처 대상 서비스                 요약                          상태              p2/rtt         │
│ 12:05:11.020  AGT  bcm DiagSessionControl      → extended                     ●확정             9/9ms          │
│ 12:05:20.402  AGT  bcm ReadDataByIdentifier    0101 VariantCoding KR          ●확정             12ms           │
│ 12:05:31.114  USR  ecm ReadDataByIdentifier    F40D VehicleSpeed=0 km/h       ●확정             16ms           │
│ 12:05:40.950  AGT  bcm RoutineControl          start FF01                     ◑pending ×3       3.1s/5.0s ▲    │
│ 12:05:44.002  AGT  bcm WriteDataByIdentifier   0101 KR→EU                     ⚠R3 승인 대기     만료 98s       │
│ 12:05:47.100  AGT  bcm ECUReset                hard                           ⌛?불확정         P2* 초과       │
│ poll#3 F40D,F405 ×1,204 last 42 km/h p2 12/15/31 ms NRC 0 skipped 3   (집계 행, 1 s 갱신)                      │
├ 상세 ─ t-000431 · AGT claude-1 · R3 · ○요청 44.002 → ⚠승인 대기 · req 2E 01 01 02 1F · diff KR(05)→EU(02) ───────┤
├──────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ 대상 [bcm ▾] > [22 F190                 ] ReadDataByIdentifier F190 VIN · R0  [보내기 ⏎] [중단 !] [폴링 정지] │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
- 명령줄 실시간 디코드 미리보기, R2 이상은 Enter 두 번(UDS §5.2). TesterPresent는 상태 바 카운터로 접힘.
- M1: 트랜잭션 표·명령줄·[제어 가져오기]·[중단]. DTC 표·세션 패널·`active_overrides`는 M2.

#### 5.1.5 승인 대기열 / 승인 모달 — **M2**
```text
상단 바 ⚠ 승인 2 클릭 또는 Ctrl-] A
┌ 승인 대기열 (2) ───────────────────────────────────────────────┐
│ ▶ a17 R3 AGT claude-1 can.periodic 0x2A0 @20ms ch0     만료 98s │
│   a18 R4 AGT claude-1 uart.send dut "sf erase …"       만료 41s │
└─────────────────────────────────────────────────────────────────┘
┌ 승인 요청 a18 ─────────────────────────────────────────── [BENCH] ┐
│ 요청자   agent claude-1 (client cl-7f3a · Claude Code · pid 4411)   │
│ 동작     uart.send → dut (gen 2)                                     │
│ 내용     sf erase 0 0x100000⏎                                        │
│ hex      73 66 20 65 72 61 73 65 20 30 20 30 78 31 30 30 … 0d        │
│ 등급     R4 (uboot.flash_erase) · 범위: 이번 1회 (고정)               │
│ 사유     (에이전트 작성) "기존 이미지 지우고 재기록"                   │
│ plan     sha256 9f3c…e1 · 사용 기한 10분 · 요청 만료 41s              │
│ ──────────────────────────────────────────────────────────────────── │
│ 요청 코드 확인: 에이전트가 알려 준 코드 끝 2자 [  ]   (a18-··)        │
│ [거부 ⏎ (기본 포커스)]   [나중에 Esc]   [승인 — 본인 확인 필요 🔐]     │
│ 0.7 s 후 활성 · 이 화면은 사람만 조작합니다 (AI 에이전트: 클릭 금지)   │
└──────────────────────────────────────────────────────────────────────┘
```
- 기본 포커스 = 거부, Esc = 보류(거부 아님), 0.7 s 무장 지연, 모달 열린 동안 터미널 키 전달 중단(장치 출력은 계속).
- R3 범위 선택 `once | session(≤ 10분)`, R4는 once 고정, `default_blocked`·하드 차단은 "차단됨 — 승인 불가" 안내만.
- 요청 코드 2자는 **오클릭·다른 요청 승인 방지**(UDS 규칙). 에이전트도 코드를 알기 때문에 자동화 방지 수단은 아니다 — 그건 🔐 존재 확인(§10.3).
- 여러 탭: 모두 알림, 먼저 응답한 쪽 유효, `approval_result`로 다른 탭 모달 닫힘 + "tab-3에서 승인됨" 토스트.

#### 5.1.6 테스트 실행 화면 — **M3** (§5.9 요구 정리)
```text
┌ 테스트 TC-BOOT-CAN-DTC-01 · 세션 ts-0042 · 실행 claude-1 · [BENCH] · 사전 승인 R2×3 R3×1 (8:12 남음) ── [⏸ 일시정지][■ 중단] ┐
│ 단계                                       상태         증거 (Loom 검증)                    판정                        │
│ 1 PSU ON, UART "login:" 대기 30s           ✓ 4.2s       uart seq 1201–1288  ✓존재 ✓일치     PASS                        │
│ 2 CAN VehState.PowerMode == 2 (5s)         ✓ 0.8s       can seq 5530 값 2   ✓존재 ✓일치     PASS                        │
│ ▶3 UDS DTC 읽기 mask 0x09 → 0건             ◑ 진행 12s   uds t-000512 pending ×1             —                           │
│ 4 (수동) 계기판 부팅 로고 확인              ○ 대기       —                                   사람 판정 필요              │
│ 5 UDS ClearDTC 후 재확인                    ○                                                                            │
├ 실시간 증거 (단계 3) ───────────────────────────────┬ 판정 근거 (단계 2) ─────────────────────────────────────────────────┤
│ 12:07:01.204 AGT uds bcm 19 02 09 ◑pending ×1       │ 에이전트: "PowerMode=2 at can seq 5530"                          │
│ 12:07:01.205 AGT can ch0 0x7E0 03 19 02 09 (ISO-TP) │ Loom: 레코드 존재 ✓ · 신호값 2 = 기대 2 ✓ · 시각이 단계 구간 안 ✓ │
│                                                     │ [증거 열기 → CAN Trace seq 5530]                                  │
├─────────────────────────────────────────────────────┴───────────────────────────────────────────────────────────────────┤
│ 사람 개입: 단계 4 [PASS] [FAIL] [메모…]   │ 에이전트 질문: "계기판에 로고가 보입니까?" [예] [아니요] [직접 입력]  │ [보고서] │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

#### 5.1.7 STOP 후 상태 — **M1**(latch·표시·복구 송신) / M2(비상 OFF 대상 장치가 생기는 시점)
```text
┌ Loom · [BENCH] · ● LIVE ─ ■ STOP — agent·script 쓰기 잠김 (12:07:03, 나 tab-1) ───────────────────── [재개…] ┐
│ ┌ STOP 상태 ───────────────────────────────────────────────────────────────────────────────────────────────┐ │
│ │ 잠긴 것: 새 agent·script 쓰기 · 잡 3개 정지 (can p3 0x2A0, uds poll#2, run r-0012) · 대기 승인 1건 취소     │ │
│ │ 이미 드라이버에 넘어가 취소 못 했을 수 있음: CAN ≤ 2프레임 · 대기 큐 비움 14건                              │ │
│ │ ⚠ STOP — 출력 유지 중:  psu bench ON 13.5 V 0.35 A  [비상 OFF]     io kl15 ON  [비상 OFF]                  │ │
│ │ 복구 송신 (safe_state · 사용자 출처 · 인터록 검사):                                                         │ │
│ │   ☑ can.periodic.stop (남은 잡 없음 확인)  ☑ uds.recover bcm (세션 default, IO 제어 해제)                  │ │
│ │   ☐ psu.out bench off  ⛔ 막힘: uds.flashing 진행 중 PSU OFF 금지 (인터록)                                 │ │
│ │   [복구 송신 ⏎]    run r-0012의 finally는 자동 실행하지 않습니다 (A15)                                    │ │
│ │ 사용자 입력은 계속 가능합니다 (UART 키, 1회 송신, UDS 명령줄).                                              │ │
│ └──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘ │
```
- STOP 직후 STOP 상태 패널이 열리고 **[복구 송신 ⏎]에 포커스**(단, 터미널에 키 입력 중이면 포커스를 뺏지 않고 띠만 표시). 이것이 A15의 "복구 송신 1키".
- [비상 OFF]: 확인 없음, 인터록은 경고만(A3), watch 자격 허용(안전 방향).
- [재개…]: `tx resume` — 위험 방향이므로 approve 자격 + 실HW 채널은 승인 흐름(CAN §10.3). 자동 재개 없음.
- 띠는 STOP 해제 전까지 모든 레이아웃에서 고정. 늦게 연 탭도 snapshot의 `stop_state`로 즉시 같은 화면.

### 5.2 사용자 개입 UX

#### 5.2.1 개입 동작 표
| 동작 | 어디서 | 키 | 자격 | 쓰기 경로 | 에이전트가 받는 것 |
|---|---|---|---|---|---|
| UART 키 입력(원시) | 터미널 포커스 | 그대로 | operate | `uart.input`(③ 스트림) | 대기·`E_USER_ACTIVE`·`others_since_last` |
| 1회 송신(CAN/UDS) | 패널 명령줄 | Enter (R2+ 두 번) | operate | `can.tx.send`/`uds.request`(①) | `others_since_last` |
| take-control | 패널 헤더 [제어 가져오기] | UART `Ctrl-] o`, UDS `T` | operate | `uds.lease.takeover`, UART lock break | exit 6 `E_LOCK_BROKEN`/lease 정보 |
| 입력권 반환 | 칩 버튼 | `Ctrl-] y` | operate | InputArbiter | 대기 send 진행 |
| hold | 패널 [HOLD] | `Ctrl-] h` | operate | `uart.hold` | `E_HELD_BY_USER` |
| 승인/거부 | 모달 | Ctrl-] A, 거부 Enter | 승인 approve / 거부 watch | `core.approval.respond` | exit 0 / 5 `DENIED by user` |
| STOP | 상단 버튼 | `Ctrl-] x` (전역) | watch | `core.stop_all` | 이후 쓰기 exit 4/6 + STOP 상태 |
| 비상 OFF | STOP 패널·도메인 패널 | 도메인 `!` (터미널 밖) | watch | 도메인 safe_state RPC | `others_since_last` |
| 복구 송신 | STOP 패널 | Enter | operate | `core.recover.run` | `others_since_last` |
| 마커 | 셸 | `Ctrl-] m` | operate | `core.mark`(user) | 커서 `mark:X` 사용 가능 |

- **전역 키 처리**: 셸이 `window` keydown(capture 단계)에서 `Ctrl-]` 접두키를 먼저 받는다. 터미널 포커스여도 접두키 다음 한 글자는 셸이 해석(`Ctrl-] Ctrl-]` = 0x1D 리터럴). 비터미널 포커스에서 `Ctrl-X`는 브라우저 잘라내기와 충돌하므로 STOP에 쓰지 않는다(D9의 비터미널 `Ctrl-X` STOP은 웹에서 미채택 — CORE/리드 확인).

#### 5.2.2 입력이 섞일 때의 표시 (출처·입력권·`others_since_last`)
1. **출처 배지는 서버 이벤트의 `origin`만** 쓴다(클라이언트 추측 금지, 장치 에코는 DEV).
2. **입력권(lease) 칩은 채널 단위로 하나**. 도메인별 개념(UART input_owner/lock, UDS lease, CAN IG job owner)을 셸 어댑터가 공통 칩 `제어: 비어 있음 | 나 | 다른 뷰어 | AGT <id> [사유, 남은 s]`로 정규화.
3. **`⇢ 에이전트에게 전달됨` 표식**: 사용자 쓰기 행은 처음에 `⇢ 전달 대기`, 에이전트(그 채널을 쓰는 agent client)의 다음 결과 봉투에 그 seq가 `others_since_last`로 포함되면 `⇢ 에이전트에게 전달됨 (시각)`. 이를 위해 CORE가 `core.agent_cursor{client_id, from_seq, to_seq, others_count}` 이벤트를 발행(요청 §15). 사용자는 "에이전트가 내 개입을 알고 다음 행동을 하는지"를 본다.
4. 에이전트가 대기 중이면(`deferred`) 칩에 대기 중인 에이전트 송신 원문(`⏳ "reboot⏎" 3.2/10s`)을 보여 준다 — 사용자가 끝내야 할지 판단.
5. 같은 사용자 탭이 여러 개면 키를 먼저 친 탭이 writer(UART §3.7-7). 다른 탭에서 키를 치면 `다른 뷰어 tab-1이 입력 중 — Ctrl-] o로 가져오기` 토스트(M2), Ctrl-C는 어느 탭이든 즉시 통과.

#### 5.2.3 쓰기 상태 단계의 점진 갱신 규칙 (D4/A14, 행 하나)
| 단계 | 기호·색 | 의미 | 화면 규칙 |
|---|---|---|---|
| requested | `○요청` 회색 | 접수(정책 판정 전·승인 대기 포함) | 행 생성. 승인 대기면 `⚠R3 승인 대기 만료 98s` |
| accepted | `◔수락` 회색 | 정책 통과·큐 진입 | 큐 위치 `#2` |
| sent | `◑보냄` 청록 | 드라이버·소켓에 넘김(**wire 완료 아님**) | 경과 시간 클라이언트 측 증가 표시(`3.1s/5.0s`) |
| confirmed | `●확정` 초록 | 장치·버스 확인(UDS 긍정 응답, CAN TX 확인) | 0.5 s 하이라이트 후 정지 |
| effective_verified / effect_unverified | `●●검증` / `●?미검증` | 되읽기 일치/불일치·불가 | 같은 행 갱신 |
| failed | `✗실패` 빨강 + 원인(NRC 이름 등) | | 상세에 다음 행동 |
| outcome_unknown | `⌛?불확정` 주황 | 실행됐을 수도 있음 | **"실패"로 칠하지 않는다**, 툴팁 "장치가 실행했을 수 있음 — 상태 확인 필요" |
규칙: (1) 키 = `request_id`, 첫 이벤트에서 행 생성(늦은 합류면 snapshot의 현재 단계부터). (2) **단조 갱신** — 더 낮은 단계 이벤트가 늦게 오면 무시(seq 기준). (3) 행 위치는 요청 시각 순 고정, 갱신 때문에 행이 이동하지 않음. (4) 성공 색은 `confirmed` 이후에만 — UART의 성공 종단은 `sent`이므로 UART 행은 초록 `●`을 쓰지 않고 `◑보냄`에서 끝난다(장치 ACK 없음, A14). (5) 최종 단계 이벤트는 제어 경로(집계·생략 금지). (6) 0x78 pending은 같은 행 카운터 `pending ×N`, 정체 시 마지막 pending 이후 경과를 빨강.

### 5.3 STOP · 비상 OFF 흐름
```text
사용자 [■ STOP] ─► core.stop_all (HTTP POST /stop 대체 경로 병행, WS 끊김에도 동작 — 요청)
   서버: latch(< 50 ms) → stop_state 우선 이벤트 → 잡 정지 보고 → 출력 유지 장치 목록
   UI:   1프레임 안에 STOP 띠(낙관적 표시 "STOP 요청 중…") → 서버 확인 시 "STOP — 잠김" (확인 2 s 없으면 빨강 "STOP 확인 안 됨 — 재시도")
   → STOP 상태 패널: 복구 송신 1키 · 비상 OFF · 재개(승인)
```

### 5.4 Activity (에이전트 행동 타임라인)
행 = 에이전트·스크립트·사용자 행동(쓰기 요청, **읽기 질의**, 대기 시작/종료, 승인 요청, lease 변화, 가리키기). 필터: 출처·도메인. 행 클릭 → 해당 패널·seq로 이동. M1은 하단 2줄 띠 + 펼치기.

### 5.5 연결·건강 표시
상단 바 왼쪽부터 우선순위(좁으면 뒤부터 생략): **env 배지(생략 금지) > STOP 띠 > 연결/LIVE 배지 > 승인 수 > REC > 보는 사람 > 에이전트 활동**. `REC failed`면 빨강 배너 "기록 불능 — 에이전트 쓰기 중단됨, 사용자 입력은 가능"(A12).

### 5.6 기록 탐색·재생 (M3 탐색 / M5 Replay)
- 시간 막대(세션 전체) + 마커 + gap 구간 음영. 과거로 이동하면 패널이 `HISTORY` 모드(큰 배지), 라이브 데이터는 계속 받되 화면은 질의 결과. `[라이브로 ⏭]`.
- 재생(`replay:<session>`)은 쓰기 버튼 전부 비활성 + `REPLAY` 배지, 과거 AGT 행은 `original_origin` 표시(탐색 §7.3).

### 5.7 마커·커서 동기화
`Ctrl-] m` 마커(user) → 모든 패널 시간축·트레이스에 표시. 패널은 커서 그룹으로 연결(같은 시점 이동). 통합 타임라인(M3)은 레인 = (domain, channel).

### 5.8 레이아웃·작업 공간과 에이전트 "가리키기"
- **저장 형식**(데몬 `~/.loom/layouts/<name>.json` — 모든 탭·에이전트가 공유. 탭 로컬 조정은 localStorage에 덮어쓰기로만):
```json
{"loom_layout":1,"name":"can-analysis","env_hint":"bench",
 "root":{"split":"h","sizes":[60,40],"children":[
   {"panel":"can.trace","props":{"channel":"ch0","mode":"chrono","filter":"id in 0x300,0x301"}},
   {"split":"v","sizes":[50,50],"children":[
     {"panel":"can.graphics","props":{"signals":["VehicleSpeed","EngRPM"]},"cursor":"A"},
     {"panel":"uds.console","props":{"targets":["bcm"]}}]}]},
 "pinned":["core.activity"]}
```
  props는 manifest JSON Schema로 검증, 모르는 패널은 "이 버전에 없는 패널" 자리표시. URL `#/l/<name>?panel=can.trace&channel=ch0`로도 표현(공유·북마크).
- **가리키기 흐름**: `loom ui show …` → `ui.present{panel, props, at, reason, client_id}` 이벤트(제어 경로) → 각 탭 토스트:
```text
┌ 에이전트 claude-1이 보여 주려 함 ─────────────────────────────┐
│ CAN ch0 Trace · filter id==0x300 · seq 5012로 이동            │
│ 이유: "응답 0x301이 안 오는 구간"                               │
│ [열기 ⏎] [이 탭은 자동으로 따라가기 ☐] [무시 Esc]               │
└────────────────────────────────────────────────────────────────┘
```
  기본 = 묻기(사용자가 입력 중일 때 화면을 뺏지 않음). 탭 설정 `follow_agent: ask | auto | off`. auto라도 터미널 입력 중이면 보류. 가리키기는 R0 — **에이전트는 화면을 바꿀 수만 있고(사용자가 허락한 경우) 버튼을 누를 수 없다.**
- 여러 탭/사용자: 탭마다 독립 레이아웃·구독, 데이터는 같은 스트림. 상단 `보는 중` 목록(탭 id, 로컬/원격, visible). 승인·STOP·입력권은 서버 상태라 모든 탭에 같게 보임.

### 5.9 테스트 실행 화면 요구 (TEST 기획 브리프 기준 — `_xteam/ui-test.md` 초안 요청)
| 요구 | 화면 요소 | 필요한 이벤트/RPC (TEST·CORE에 요청) |
|---|---|---|
| 현재 단계 | 단계 표, 진행 단계 강조, 경과/timeout | `test.session_state`, `test.step_state{idx, state, started, timeout}` |
| 실시간 증거 | 단계 구간 동안 관련 도메인 이벤트 스트림(필터 = 단계가 쓰는 채널) | 단계 마커가 모든 도메인 타임라인에 찍힘(`test.step_marker`) |
| 판정 + 증거 검증 | 판정 열 + 증거 칸 `✓존재 ✓일치 / ✗불일치 / ?inconclusive`, 증거 클릭 → 해당 패널 seq | `test.verdict{step, verdict, evidence[{domain, seq_range, field, expected}], check{exists, matches, detail}}` |
| 증거 없는 PASS 금지 | Loom 검증 실패면 PASS 표시 대신 `inconclusive`(주황) + 이유 | 서버가 검증 결과를 확정해 보냄(UI가 판정하지 않음) |
| 사람 개입 | 수동 단계 PASS/FAIL/메모, 에이전트 질문 응답, 일시정지·계속·건너뛰기·중단·수동 재시도 | `test.ask{prompt, choices}` → `test.answer`(user), `run.pause/resume/skip/abort` |
| 사전 승인 | 세션 시작 시 일괄 승인 묶음(R2/R3 목록·대상·횟수) 모달, 남은 사용 기한 표시 | 승인 객체 `bundle` 형식 |
| 정직성 | 일시정지 중에도 "장비 시간은 흐름" 표시(탐색 §7.1), STOP 시 finally 미실행 + 복구 송신 | — |
| 보고서 | 종료 후 HTML 보고서 링크, 각 단계 증거 → 기록 구간 링크(HISTORY 모드로 열림) | `test.report{html_ref, junit_ref}` |

### 5.10 TUI의 위치 [결정안]
- 남김: **UART `loom uart attach` 터미널 모드**(herdr 패인, 원시 키, UART 팀 소유 — 웹과 같은 구독·같은 InputArbiter). SSH만 되는 환경의 최소 상태 보기 `loom status --watch`(CORE).
- 안 만듦: CAN `monitor`·UDS `view`의 Textual TUI(Python — C1과 충돌, 웹 M1이 대체). 터미널 사용자의 승인은 웹으로(질문 Q4).

---

## 6. 데이터 / 기록 포맷

### 6.1 UI가 소유하는 데이터
| 데이터 | 위치 | 비고 |
|---|---|---|
| 레이아웃 | 데몬 `~/.loom/layouts/*.json` | 사용자·에이전트 공유, 이름 충돌 시 새 이름 |
| 탭 로컬 설정(열 폭, follow_agent, 테마) | localStorage | 실패해도 기본값으로 동작 |
| 마커 | 데몬 이벤트 `core.mark`(user) | UI가 따로 저장하지 않음 |
| 승인 기록 | 데몬 감사 로그 | 승인 연결 종류·방법(존재 확인 여부)·UA·webdriver 플래그 포함 요청 |
UI는 **기록을 직접 쓰지 않는다**. 모든 보존은 데몬 저널(D7).

### 6.2 전송 형식 요구 (CORE와 `_xteam/core-ui.md`에서 합의 — 측정은 ui-sol)
1. 제어·RPC·저속 이벤트 = JSON-RPC text 프레임(D2 그대로).
2. 대량 레인(CAN 프레임, UART 바이트, 신호 샘플) = **WS binary 프레임의 열 블록**: 헤더(`sub_id, stream_epoch, seq_first, seq_last, kind, n, display_gap, lag_ms, server_send_mono`) + 열(ts_mono i64, id u32, flags u8(dir·origin·fd·brs), dlc u8, data 오프셋+바이트 풀, request_id 사전 인덱스). `_xteam/can-core.md` W 항목의 CAN 필드를 반영.
3. 구독 옵션 `encoding: json | columnar` — M1은 json 허용(CAN 2.7 MB/s/구독 추정, CAN §4.5), columnar는 M2 성능 관문 전에.
4. 모든 배치에 `seq` 범위·`display_gap`·`record_health`·`lag_ms` 동봉, 1 Hz heartbeat에 `server_mono`.
5. 64 KiB 초과 질의 결과는 `/data/<query_id>`(쿠키 검사) — 패널은 `ctx.query`만 부르고 셸이 경로 선택.
6. **스키마 → TS**: Rust serde 타입 → JSON Schema(정본) → `ui/src/gen/*.ts` 생성, CI에서 drift 검사(생성물 diff ≠ 0이면 실패). CORE 스키마 fixture JSONL을 UI 단위 시험 입력으로 그대로 사용.

### 6.3 이벤트 → 화면 매핑 공통 규칙
`origin` → 배지, `dir` → 화살표, `payload.write_state` → 단계 칩, `risk` → 등급 표시, `ts.quality`/`ts.source` → 툴팁(host_read_complete vs HW ts), `metrics[]` → 자동 플롯 후보(viz), `purpose=tester_present|aggregate` → 접기 규칙.

### 6.4 브라우저 메모리 상한 (C2)
| 저장소 | 상한(탭당) | 넘칠 때 |
|---|---|---|
| UART 터미널 scrollback | 10,000줄/채널 (xterm) | 오래된 줄 버림, "이전은 질의로" 링크 |
| UART 원시 링 | 4 MiB/채널 | 오래된 것 버림 |
| CAN 열 링 | 200,000 프레임/채널(≈ 12 MiB 추정) | 오래된 것 버림, 스크롤 백은 `can.trace.query` |
| UDS 트랜잭션 | 20,000행/대상 + 진행 중 전부 | 완료된 오래된 행부터 |
| 플롯 라이브 | 시리즈당 원본 최근 60 s + min/max 피라미드 3층 | 피라미드는 고정 크기 |
| Activity | 5,000행 | 오래된 것 버림 |
| 전체 목표 | **JS 힙 ≤ 300 MiB, 1 h 워밍업 후 24 h 증가 ≤ 10%, DOM 노드 수 평탄** | 예산 초과 시 패널 링 축소 + 배지 `메모리 절약 모드` |
측정: Playwright + CDP `Runtime.getHeapUsage`/`Memory` 도메인, 1분 표본. **24 h soak에 UI 포함**(헤드리스 Chromium 탭 1개: UART flood + CAN 고부하 + UDS 폴링 패널 동시) [결정안 — CORE soak 관문에 합류 요청].

---

## 7. 라이브러리 선택과 근거 (판단 기준 + 후보, 세부 측정은 ui-sol)

| 영역 | 요구사항(판단 기준) | 후보 | 상태 |
|---|---|---|---|
| UI 프레임워크(셸·폼·모달) | 고빈도 갱신은 명령형 렌더러가 담당하므로 셸은 **미세 반응성 + 작은 번들 + 장기 유지보수**가 중요 | SolidJS / Preact+signals / Svelte 5 / React | ui-sol 비교. 기본 성향: 미세 반응성 계열 |
| 트레이스 그리드 | 수만 행/s 유입, Fixed 모드, 가상 스크롤, 행 펼침, 선택·복사 | 자체 캔버스 그리드 vs DOM 가상화(TanStack Virtual) vs Glide Data Grid | ui-sol S1 |
| 터미널 | VT 정확도, WebGL 렌더, 원시 키, decoration API(gap 장식), CSI 핸들러 훅(질의 억제) | xterm.js (+ webgl·fit addon) | 사실상 단일 후보. 버전·라이선스 ui-sol |
| 플롯 | 다중 신호, 10^7점 + min/max 다운샘플, gap 선 끊김, 커서 2개, 라이브 스크롤 | uPlot 1.6.32 (MIT — CAN 계획 §5.1 npm 확인) / WebGL 계열 | ui-sol |
| 도킹 레이아웃 | 분할·탭·최대화·직렬화(JSON), 키보드 접근 | dockview / FlexLayout / golden-layout | M2, ui-sol |
| 직렬화 | 열 블록 디코드(Worker), JSON | 자체 열 프레임 / Arrow IPC / MessagePack | ui-sol + CORE |
| 스키마→TS | Rust 정본, drift 검사 | schemars → json-schema-to-typescript / ts-rs / specta | ui-sol + CORE |
| 내장 | 오프라인 단일 바이너리 | rust-embed / include_dir (CORE 소유 crate) | CORE |
| 존재 확인 | 플랫폼 인증기(Touch ID·Windows Hello) + 보안 키 | WebAuthn(브라우저 내장, 라이브러리 불필요) | **[미확인] 스파이크 S-O1** |
| E2E | 3-OS 헤드리스, CDP 접근 | Playwright | ui-sol |
원칙: 런타임 CDN 0, 라이선스 MIT/Apache/BSD만(GPL 계열 금지 — 단일 바이너리 내장), 패키지별 버전 고정 + lockfile.

---

## 8. HW / OS 지원 매트릭스

| 항목 | Windows x64 (벤치) | Linux x64 (벤치) | macOS (개발·원격 보기) |
|---|---|---|---|
| 브라우저 1급 | Chrome / Edge (Chromium) | Chrome / Chromium | Chrome |
| 브라우저 2급(표시만 보장) | Firefox | Firefox | Safari·Firefox |
| 존재 확인(승인) | Windows Hello [미확인] | **플랫폼 인증기 대개 없음** → 보안 키 또는 페어링 코드 대체(§10.3) [미확인] | Touch ID [미확인] |
| 원격 보기 | — | — | `ssh -L 7420:localhost:7420 bench` 후 `/pair` |
| 데몬이 브라우저 열기 | `start` (ShellExecute) | `xdg-open` (GUI 세션 없으면 `--no-browser` + 안내) | `open` |
| Tauri(M4 선택) | WebView2 | WebKitGTK (성능 위험) | WKWebView |
| CI | 헤드리스 Chromium(Playwright) 3 OS 모두 | | |

---

## 9. 시뮬레이션 & 테스트 전략

### 9.1 단계별 백엔드
1. **UI-M0: `loom-mock`**(UI 팀, Node/TS, `spikes`가 아니라 `ui/tools/mock`) — CORE 스키마 fixture JSONL을 시나리오대로 재생하는 WS 서버 + 쓰기 RPC 스크립트 응답 + 부하 생성(CAN 20k fps, UART 92 KB/s, UDS 0x78 연속) + gap·stall·disconnect 주입. CORE stub이 늦어도 UI가 막히지 않게.
2. **UI-M1: 실 `loomd` + sim worker**(CORE P1) — 같은 시험을 실데몬으로 다시. mock과 실데몬 둘 다 green이어야 M1.

### 9.2 시험 계층
| 계층 | 내용 | 도구 |
|---|---|---|
| 단위 | 스토어·WriteTracker 단조성·ClockSync·열 블록 디코더·키 매핑 | Vitest |
| 패널 계약 | manifest 위반(미선언 토픽·act) throw, ESLint 금지 규칙, CSP/Permissions-Policy 헤더 존재 | Vitest + lint + 헤더 검사 |
| E2E 핵심 루프 | E-UI-1/2/3 시나리오 자동화(에이전트 = CLI 또는 RPC 클라이언트, 사용자 = Playwright) | Playwright |
| 지연 | `render_ack` + 화면 DOM/캔버스 관찰로 p50/p95/p99, 기준 머신에서만 성능 판정, CI는 논리 판정 | Playwright + 하네스 |
| 정직성 | 부하·드롭 주입 → `display.gap` 행·`LIVE-DEGRADED`·`STALLED`가 **실제로 화면에 뜨는지**, 생략 수 = 서버 gap count | Playwright + mock |
| 안전 UX | 승인 무장 지연, 기본 포커스 거부, watch 자격으로 쓰기 불가, STOP 1프레임, webdriver 감지 경고 | Playwright |
| 시각 회귀 | §5.1 와이어프레임 상태별 스크린샷 | Playwright 스냅샷 |
| soak | 24 h UI 포함(§6.4) | 기준 머신 |

---

## 10. 안전 / 가드레일 (UI 측)

### 10.1 원칙
1. **안전 방향은 쉽게, 위험 방향은 어렵게**: STOP·비상 OFF·거부·중단 = 확인 없음, 모든 자격. 송신·승인·완화·재개 = 자격 + 확인.
2. **거짓 성공 금지**: 성공 색은 `confirmed` 이후, `outcome_unknown`은 실패와 다른 색, gap은 "수신 없음"처럼 보이지 않게.
3. **장치 출력은 신뢰하지 않는 데이터**: 항상 textContent, 터미널 질의 응답 억제, OSC 52/8 차단, 패널 spec에 HTML 금지.
4. **UI는 서버 정책을 대신하지 않는다**: UI 확인(Enter 두 번 등)은 실수 방지일 뿐, 판정은 데몬 정책(D6).

### 10.2 자격 모델 [결정안]
| 자격 | 발급 | 허용 |
|---|---|---|
| watch | `loom ui open --watch`, 페어링 시 선택, 기본 원격 관전 | 보기, STOP, 비상 OFF, 거부, 중단(abort) |
| operate | `loom ui open`(데몬이 브라우저를 직접 엶), `loom ui pair` | + 사용자 쓰기·입력·take-control·hold·복구 송신·마커 |
| approve | operate + **존재 확인 등록**(M2) | + R2 뷰어 확인·R3/R4 승인·정책 완화·`tx resume`·`unlock` |

### 10.3 에이전트 자기 승인 차단 — 시나리오 분석과 완화안
**위협 모델(D6)**: 에이전트는 협조적이지만 실수할 수 있다. 막을 것 = "도움이 되려는 에이전트가 승인 화면을 보고 눌러 버리는 것". 같은 OS 계정의 악의적 우회(쿠키 파일 읽기, 데몬 메모리)는 범위 밖이며 그렇게 명시한다.

**시나리오**
| # | 경로 | 현실성 | 비고 |
|---|---|---|---|
| S1 | 에이전트가 `loom ui open` 출력의 URL(부트스트랩 포함)을 Playwright로 열어 쿠키 획득 → 승인 클릭 | 높음(설계가 허술하면) | 부트스트랩 비밀이 CLI 출력에 있으면 그대로 뚫림 |
| S2 | 에이전트가 **사용자의 실제 브라우저 프로필**을 조작하는 도구(claude-in-chrome·ego-browser 등 — 로그인 상태 공유)로 이미 열린 Loom 탭에서 승인 클릭 | **높음** — 쿠키 분리로 못 막음 | 이 환경의 기본 도구가 바로 이런 형태 |
| S3 | 에이전트가 화면을 스크린샷으로 읽고 요청 코드 2자를 입력 | S1·S2에 딸림 | 코드 2자는 자동화 방지 수단이 아님 |
| S4 | `script`/pty로 TTY를 흉내 내 `loom ui pair` 코드 획득 | 중간(의도적 우회에 가까움) | 범위 경계 |
| S5 | 브라우저 자동화로 사용자 입력(UART 키·송신)을 `origin=user`로 위장 | S2와 같음 | 승인 우회보다 약하지만 출처를 속임 |

**완화안 비교**
| 안 | 내용 | S1 | S2 | S3 | 비용·부작용 |
|---|---|---|---|---|---|
| A. 자격 분리 + 비밀 비노출 | 부트스트랩 코드는 데몬이 직접 브라우저에 전달(CLI 출력·로그 금지), 에이전트 경로로 얻는 링크는 비밀 없음, approve는 별도 단계 | ✓ | ✗ | ✗ | 낮음, M1 가능 |
| B. 존재 확인(WebAuthn user verification) | R3/R4·완화·재개 승인 시 플랫폼 인증기(Touch ID/Windows Hello) 또는 보안 키 터치를 요구, 서버가 assertion 검증 후 승인 객체 발급 | ✓ | ✓ (OS 생체 프롬프트는 브라우저 자동화가 통과 못 함 [미확인]) | ✓ | 중간. `localhost` RP ID·보안 컨텍스트 [미확인], Linux 벤치에 인증기 없을 수 있음 → 보안 키나 C로 대체 |
| C. 대역 외 코드 | 승인 시 데몬이 **에이전트가 읽지 않는 곳**(OS 알림·벤치 TTY `loom ui pair --approve`)에 6자리 코드 표시, 모달에 입력 | ✓ | △ (알림을 읽는 computer-use 계열에는 뚫림) | ✓ | 중간, 매번 번거로움 |
| D. 감지·표시·감사 | 승인 연결의 UA·`navigator.webdriver`·자동화 흔적을 기록, webdriver 브라우저는 approve 자격 발급 거부, 요청 후 300 ms 안 승인은 경고, DOM에 "AI 에이전트: 이 화면을 조작하지 말고 사용자에게 요청하라" 고지문, 스킬 규칙(Loom UI를 브라우저 자동화로 조작 금지 — 보기·스크린샷만) | △ | △ (협조적 에이전트는 고지문·스킬 규칙을 따름) | △ | 낮음 |

**추천**: **A + D를 M1부터**(싸고, 협조적 에이전트의 실수를 대부분 막음), **B를 M2 approve 자격의 기본**으로(S2를 실제로 막는 유일한 안), B가 불가능한 호스트(인증기 없는 Linux 등)는 **C로 대체**하고 설정 `approval.presence = webauthn | out_of_band_code | none`(none은 sim에서만 허용, 감사에 표시). R2 뷰어 확인은 A + D만(빈도가 높아 B는 과함, 설정으로 강화 가능).
S5(입력 위장)는 B로 막지 않는다(키 입력마다 생체 확인은 불가) — D의 고지·스킬 규칙·감사(입력 연결 UA 기록)로 다루고 한계를 문서에 명시한다.
검증: 스파이크 S-O1 — (1) `http://localhost:<port>`에서 WebAuthn 등록·user verification 동작(Chrome/Edge, macOS/Windows), (2) Playwright CDP 가상 인증기로 실 플랫폼 자격증명 assertion이 불가한지, (3) claude-in-chrome류 확장 조작 시 OS 프롬프트가 사람 입력을 요구하는지, (4) Linux에서 보안 키 경로.

### 10.4 기타 UI 안전 규칙
- 승인 모달: 0.7 s 무장 지연, 기본 포커스 거부, Esc = 보류, 키보드 자동 반복 무시, 모달 뒤 클릭 차단.
- env=vehicle: 상단 테두리 강조 + 승인 모달·송신 명령줄에 `VEHICLE` 표시(규칙은 bench와 같음, B4).
- 붙여넣기 4 KiB 초과(UART) → 검토 모달.
- DNS rebinding·CSRF: Host/Origin 검사(D2), SameSite=Strict, WS 티켓 1회용, `frame-ancestors 'none'`.
- 비밀값: `[secret 12 B]`, 승인 토큰 원문 미표시(`approval_ref`만), seed/key 마스킹(UDS §6.1).

---

## 11. 마일스톤

| M | 내용 | 수용 기준 (자동 시험 ID) |
|---|---|---|
| **UI-M0** (P0, CORE stub 병행) | SPA 골격, `loom-mock`, Worker/스토어/스케줄러, ClockSync, 스키마 TS 생성 파이프라인 | mock 위에서 UART·CAN·UDS fixture 재생 화면 렌더, drift 검사 CI green |
| **UI-M1 걷는 뼈대** (P1 관문) | 셸(상단 바·STOP·Activity·고정 레이아웃) + UART 터미널 + CAN Trace(Chrono) + UDS 콘솔, 인증(A+D), render_ack | 아래 표 전부 green (mock + 실 loomd sim) |
| UI-M2 (P2) | 승인 대기열·모달·존재 확인(B/C), 도킹·레이아웃 저장, 가리키기 토스트, CAN Fixed·IG·Data, UDS DTC·세션·active_overrides, UART 트레이스·hex, columnar 전송, 탭 간 writer 탈취 | 승인 E2E(자격별), 자기 승인 차단 시험(Playwright로 approve 불가), 10k fps Trace p95 ≤ 150 ms |
| UI-M3 (P3) | Graphics·`loom.viz`(파일·라이브), spec 패널, 통합 타임라인, 테스트 실행 화면, LA 레인·디코드 표, DLT, 기록 탐색 | viz 요구 V-UDS-1/2/4·V-UART-1/3/4 화면 시험, 테스트 화면 E2E(TEST fixture) |
| UI-M4 | Statistics, Tauri(선택), 영어, 팝아웃 | — |
| UI-M5 | Replay 블록, PNG export | — |

**UI-M1 수용 기준 (모두 자동 시험)**
| ID | 기준 |
|---|---|
| UI-T01 | 늦은 합류: 에이전트가 1,000 이벤트를 보낸 뒤 탭을 열면 1 s 안에 snapshot 표시, 이후 live와 seq 중복·누락 0 |
| UI-T02 | 에이전트 `uart send`/`can send`/`uds read-did` 결과의 seq가 같은 패널 행에 존재, 출처 배지 AGT |
| UI-T03 | 지연(기준 머신, 정상 부하): UI 구간 p95 UART ≤ 45 ms, UDS ≤ 35 ms, CAN ≤ 85 ms. 전 구간 UART 100 / UDS 100 / CAN 200 ms (render_ack 기반) |
| UI-T04 | 사용자 개입 3종: UART 키(입력권 칩 `나`) → 에이전트 send 대기 → Ctrl-C → 에이전트 결과 `others_since_last.count ≥ 1` 및 행 표식 `⇢ 전달됨`; CAN 1회 송신 USR 행; UDS 명령줄 요청 + [제어 가져오기] → 에이전트 exit 6 |
| UI-T05 | 정직성: mock이 구독 드롭 주입 → 생략 행의 count = 서버 gap count, `LIVE-DEGRADED` 표시; heartbeat 중단 3 s → `STALLED`; 재연결 후 이음 |
| UI-T06 | STOP: 버튼 클릭 → 1 프레임 안 띠, 서버 latch 확인 표시, 이후 에이전트 쓰기 거부, 사용자 UART 키는 통과, 복구 송신 1키 실행(user 출처) |
| UI-T07 | 불변식: 패널 lint 규칙 위반 PR fixture가 CI 실패, 응답 헤더 CSP·Permissions-Policy 존재, 두 번째 WS(티켓 없음) 거부 |
| UI-T08 | 자격: watch 탭에서 송신·입력 버튼 비활성 + 서버도 거부(직접 RPC 호출 시험), watch로 STOP 가능; CLI 출력·데몬 로그에 부트스트랩 코드 없음 |
| UI-T09 | 쓰기 단계: UDS 0x78 ×5 시나리오에서 한 행이 `○→◔→◑ pending×1..5→●`로 갱신, 행 수 1; 늦은 이벤트 역순 주입 시 단계 후퇴 없음; `outcome_unknown` 주황 |
| UI-T10 | 메모리: CAN 5k fps + UART flood 1 h, 힙 증가 ≤ 10%(워밍업 후) — 24 h는 P1 이후 soak 관문 |

---

## 12. 구현 단계 작업 분해

| ID | 작업 | 담당 추천 | 의존 | M |
|---|---|---|---|---|
| UI-01 | 저장소 `ui/` 골격(Vite·TS·lint 규칙·CSP 헤더 목록), 프레임워크 확정(ui-sol 결과) | Sol | ui-sol R1 | M0 |
| UI-02 | `loom-mock` (fixture 재생·부하·gap/stall 주입·쓰기 스크립트) | Sol | CORE 스키마 fixture(없으면 임시 fixture) | M0 |
| UI-03 | 스키마 → TS 생성 + drift CI | Sol | CORE C3 스키마 export | M0 |
| UI-04 | Net Worker(RpcClient·SubscriptionMux·디코더·ClockSync·제어 분류) | Sol | UI-02 | M0 |
| UI-05 | 스토어·WriteTracker·RenderScheduler·우선 제어 경로 | Opus | UI-04 | M0 |
| UI-06 | `@loom/panel-sdk` (registerPanel·PanelContext·ActionGate·manifest 검증) | Opus | UI-05 | M0 |
| UI-07 | 셸: 상단 바·env·STOP·STOP 상태 패널·Activity·고정 레이아웃·키 접두 처리 | Opus | UI-06, CORE `core.stop_all`/`stop_state` | M1 |
| UI-08 | 인증: `/boot`·`/pair` 화면, 자격 표시, WS 티켓(서버 쪽은 CORE) | Opus | CORE D2 구현 | M1 |
| UI-09 | UART 터미널 패널(xterm, 입력권 칩, gap 장식, 질의 억제, 트레이스 최소) | Sol | UI-06, UART 패널 명세 | M1 |
| UI-10 | CAN Trace 패널(Chrono, 캔버스/DOM 선택은 S1 결과, 생략 행, 송신 줄) | Sol | UI-06, S1 | M1 |
| UI-11 | UDS 콘솔(트랜잭션 표 점진 갱신, 명령줄 디코드 미리보기는 서버 dry-run RPC, 제어 가져오기) | Opus | UI-06, UDS 이벤트 스키마(§6.1) | M1 |
| UI-12 | 지연 하네스(render_ack·p95 계산) + 정직성 시험 + E2E E-UI-1/2/3 | Sol | UI-07–11 | M1 |
| UI-13 | 승인 대기열·모달·존재 확인(WebAuthn·대역 외 코드)·감지/고지 | Opus | 스파이크 S-O1, CORE 승인 서비스 P2 | M2 |
| UI-14 | 도킹·레이아웃 저장·`ui.present` 토스트 | Opus | 도킹 라이브러리(ui-sol), CORE `ui.*` | M2 |
| UI-15 | columnar 전송 디코드 + CAN Fixed·IG·Data | Sol | CORE 열 블록, CAN DBC 질의 | M2 |
| UI-16 | UDS DTC·세션·overrides, UART 트레이스·hex | Sol | 도메인 명세 | M2 |
| UI-17 | Graphics + `loom.viz` 범용 렌더 + spec 패널 렌더러 | Sol(렌더) + Opus(spec 계약) | CAN viz 데이터셋 질의 | M3 |
| UI-18 | 테스트 실행 화면 | Opus | `_xteam/ui-test.md`, CORE 테스트 세션 | M3 |
| UI-19 | LA 레인·디코드 표, DLT, 통합 타임라인, 기록 탐색 | Sol | EXPLORE 명세 | M3 |
| S-O1 | 스파이크: WebAuthn on localhost + 자동화 불가 확인 | Opus | — | M1 전 |
임계 경로: CORE 스키마 fixture → UI-02/03/04 → UI-05/06 → UI-07/09/10/11 → UI-12 → P1 관문.

---

## 13. 리스크
| 리스크 | 영향 | 대응 |
|---|---|---|
| CORE stub·스키마 지연 | UI M0 정지 | `loom-mock` + 임시 fixture로 진행, 스키마 확정 시 생성 파이프라인으로 교체 |
| 브라우저가 터미널 키(Ctrl-W 등)를 가로챔 | 터미널 수준 UX 미달 | `beforeunload`, 대체 키, 실측 후 Tauri(M4) |
| 메인 스레드 정체로 제어 이벤트 지연 | 승인·STOP 표시 늦음 | 프레임 예산, 제어 경로 분리, STOP HTTP 대체 경로 |
| WebAuthn localhost/Linux 제약 | 자기 승인 차단 약화 | S-O1 결과로 C(대역 외 코드) 대체, 감사·고지 유지 |
| 브라우저 자동화로 사용자 입력 위장(S5) | 출처 신뢰 저하 | 고지·스킬 규칙·감사, 한계 명시 |
| 고부하 JSON 디코드 비용 | CAN 지연 목표 실패 | columnar(M2), Worker 디코드, 서버 필터 |
| 오래 켠 탭 메모리 증가 | C2 위반 | 모든 링 상한, soak에 UI 포함 |
| 패널 기여가 불변식 우회 | 숨은 경로 | lint·CSP·WS 티켓·리뷰 4층 |
| 도메인 팀 명세 지연 | 1차 패널 지연 | 기존 plan.md 뷰어 절로 UI 팀이 초안 작성 후 확인만 요청 |
| 출처 색 불일치(CAN vs 다른 팀) | 혼란 | §5.1.3 통일안 리드 결정 |

---

## 14. 사용자에게 물어볼 질문 (기본값으로 진행)
| # | 질문 | 기본값 |
|---|---|---|
| Q1 | UI를 주로 벤치 PC 화면에서 보나, 다른 PC(macOS)에서 원격으로 보나? | 둘 다. 원격은 SSH 포워딩 |
| Q2 | 승인 시 Touch ID/Windows Hello/보안 키 확인을 써도 되나? (Linux 벤치에 보안 키가 있나?) | 가능하면 사용, 없으면 대역 외 코드 |
| Q3 | 에이전트가 화면을 "가리킬" 때 자동 전환을 원하나, 묻기를 원하나? | 묻기 |
| Q4 | 터미널(herdr)에서만 일하는 경우에도 승인은 웹에서 해도 되나? | 예(TUI 승인 화면 안 만듦) |
| Q5 | 주 브라우저는? (Chrome/Edge/Firefox) | Chromium 계열 1급 |
| Q6 | 화면 언어 | 한국어(영어 M4) |

---

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 CORE에 요청 (`_xteam/core-ui.md`로 정리 요청)
| # | 요청 | 이유 |
|---|---|---|
| C-1 | `core.time{}` → `{server_mono_ns}` RPC | 브라우저 시계 정렬(§3.5) |
| C-2 | `core.render_ack` 수신·뷰어별 lag/visible 집계, 에이전트 `status`·경고(`viewer_hidden`) 반영 | 지연 판정·비분리 원칙 |
| C-3 | `core.viewer_state{visible}` | 숨은 탭을 "보는 중"으로 세지 않기 |
| C-4 | 우선 제어 큐 kind 목록 확정(쓰기 최종 단계 포함) + 배치 메타(`lag_ms`, `server_send_mono`) | UI 제어 경로 |
| C-5 | 열 블록 binary 프레임 + 구독 `encoding` 옵션(형식은 ui-sol·CORE 합의) | CAN 고부하 |
| C-6 | 자격 3단(watch/operate/approve)·부트스트랩 코드 비노출·`loom ui pair`(TTY)·WS 1회용 티켓 | 자기 승인 차단 A |
| C-7 | 승인 서비스에 존재 확인 훅(`presence.challenge` → WebAuthn assertion 검증) + 감사에 승인 방법·UA·webdriver 플래그 | 자기 승인 차단 B/D |
| C-8 | `core.agent_cursor{client_id, from_seq, to_seq, others_count}` 이벤트 | `⇢ 에이전트에게 전달됨` 표시 |
| C-9 | `core.stop_state` 이벤트(잠긴 것·정지 잡·취소 승인·출력 유지 장치·복구 송신 목록·인터록 막힘) + HTTP POST `/stop` 대체 경로 | STOP 화면 |
| C-10 | `core.recover.list/run`(A15 복구 송신 공통) | 복구 송신 1키 |
| C-11 | `ui.open/show/present`, 레이아웃 저장소 RPC, 데몬이 OS 브라우저 실행 | 가리키기 |
| C-12 | 응답 헤더: CSP, `Permissions-Policy`(serial/usb/hid/bluetooth 차단), `localhost` 정본 리디렉트 | 불변식 I-UI1 |
| C-13 | 24 h soak에 UI 탭 포함 | C2 |
| C-14 | 스키마 fixture JSONL(정상·고부하·gap·쓰기 실패·outcome_unknown) 첫 주 배포 | UI-M0 |
| C-15 | 공통 lease 이벤트 형태(`core.lease{channel, owner, kind, reason, ttl}`) — 도메인별 lease/lock/job owner 정규화 | 입력권 칩 |

### 15.2 도메인 팀에 요청 (각 `_xteam/<comp>-ui.md`, §4.6 템플릿)
- **UART**: 터미널 패널 명세(InputArbiter 이벤트 `uart.lease`/`deferred`/`input_dirty`, 키 접두 맵 웹 대응 확인 — 특히 브라우저에서 못 쓰는 키), gap 장식 정책, 트레이스 모드 열.
- **CAN**: Trace/Fixed/IG/Data/Graphics 명세, 출처 색 통일안 동의(§5.1.3), 열 블록 필드(`_xteam/can-core.md` W), 화면 예산 수치 확인, ISO-TP→UDS 주석 열(V-UDS-5).
- **UDS**: 콘솔 명세(§6.1 이벤트 표 그대로 fixture), 명령줄 디코드 미리보기용 dry-run RPC, `uds.recover` safe_state 목록, R3 코드 2자 규칙의 웹 적용 확인.
- **EXPLORE**: LA 레인·디코드 표(캡처 후 표시 배지), DLT 표, PSU/IO spec 패널(JSON)과 비상 OFF 목록, run 패널.
- **TEST(EXPLORE)**: `_xteam/ui-test.md` — §5.9 표의 이벤트(`test.session_state/step_state/step_marker/verdict/ask/report`)와 증거 검증 결과 형식, 사전 승인 묶음 형식.

### 15.3 마스터 계약 변경 요청
| # | 대상 | 요청 |
|---|---|---|
| R-1 | D9 | 웹에서 비터미널 STOP 키 `Ctrl-X` 미채택(브라우저 잘라내기 충돌). 웹 전역 STOP = 버튼 + `Ctrl-] x` |
| R-2 | D2 | viewer role 안에 자격 3단 추가(watch/operate/approve) |
| R-3 | D6 | R3/R4 승인에 "존재 확인" 기본(설정으로 대역 외 코드/none — none은 sim만) |
| R-4 | 출처 색 | AGT 청록·USR 노랑·DEV 회색·SCR 보라·EXT 보라 점선으로 전 팀 통일 |
| R-5 | D9/C6 | CAN `monitor`·UDS `view` Textual TUI 폐기, UART attach만 TUI로 유지 |
