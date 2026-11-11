# CAN v2.0 초안 — can-opus (구조 · UI 패널 명세 · 통합 테스트 · 스킬 · M1)

작성: can-opus, 2026-11-10. 입력: 마스터 §2.3 C1–C8, `04-v2-tasks.md`, `can/plan.md` v1.2, `core|ui|test/brief.md`, `_xteam/can-uds-isotp.md`·`canoe-bridge.md`·`viz-requirements.md`.
이 문서는 **v1.2에서 바뀌는 절만** 다룬다. C3에 따라 D2·D3·D4/A14·D5·D6·D7·A1–A16·B1·B4와 ISO-TP 합의는 v1.2 그대로 쓴다.
표기: **[실측]** spike 측정, **[추정]** 산술, **[목표]** 수용 기준, **[미확인]**. crate 이름·버전, C2 큐·메모리 수치, FFI 범위는 **can-sol(v2-sol.md)** 몫이라 `⟨Sol⟩` 자리 표시만 한다. CORE·UI 팀 초안은 아직 없어(2026-11-10 확인) 그쪽 계약은 **요구**로 적는다.

---

## 0. 핵심 루프 (v2판)

**에이전트가 CAN 인터페이스(PCAN-USB FD, VN5640, SocketCAN, sim)를 `loom can`으로 직접 열어 읽고 쓰며, 사용자는 같은 프레임·신호를 같은 순간 Loom UI 앱의 CAN 패널에서 보고, 같은 패널에서 송신·IG·STOP·승인으로 개입한다.**

| 요소 | 에이전트 명령 | Rust 구성요소 | UI 앱 패널 (사용자) | 첫 M |
|---|---|---|---|---|
| ① 직접 접근 | `loom can status`, `loom can open ch0 --iface sim\|pcan\|vxl\|socketcan …` (데몬 자동 기동, 사람 손 불필요) | **CAN worker 프로세스**(백엔드별: `can-sim`, `can-pcan`, `can-socketcan`, `vxl`)만 드라이버·벤더 DLL을 로드·소유. gateway의 **CAN 모듈**이 채널 레지스트리·worker 라우팅 | **채널 상태 바**: `ch0 PCAN-USB FD 500k/2M ● load 23% err 0 ACTIVE · REC ok · env BENCH`, 접속 클라이언트, LIVE/LAG | M1 sim, M2 PCAN FD(Win+Linux), M3 VN5640 |
| ② 읽기 | `trace --since @last`, `last`, `expect`, (M2) `sig get`, `sig wait-for`, `check`, `stats` | worker: 수신 스레드·`source_seq`·단기 링·저널 writer. gateway CAN 모듈: 링 미러·Fixed 테이블·trace 질의·DBC 디코더·SignalCache·신호 시계열 저장·증거 질의 | **Trace**(Fixed/Chrono), **Data/State**, **Graphics**. 에이전트가 읽고 기다리는 것은 **Activity**에 (`agent:claude-1 waiting EngRPM > 3000`) | M1 raw, M2 신호, M2~M3 Graphics |
| ③ 쓰기 | `send ID#DATA [--dry-run]`, (M2) `send --msg`, `periodic start`, `sig set`, `isotp *` | core 쓰기 파이프라인(정책→확인/승인→감사) → worker **TX 중재기**(latch·예약 ID·rate 재집행) / **주기 스케줄러 스레드** / **ISO-TP 스레드**. 모든 TX는 `bus` 전에 이벤트로 발행(publish-before-reply) | Trace에 `AGT` 배지 행(state `requested→sent→confirmed`), **IG**에 job 소유자·rev·lease, 승인 대기열(셸) | M1 raw 단발, M2 주기·IG·승인 |
| ④ 사용자 실시간 표시 + 개입 | (보는 쪽) 에이전트가 사용자에게 **UI URL**을 건넴: `loom ui open can/trace?ch=ch0` → `http://127.0.0.1:<port>/#/can/trace?ch=ch0` | gateway 이벤트 허브(CORE): 원자적 snapshot+live, 우선 제어 큐, 적응형 배치. 사용자 입력은 viewer role 연결 → `origin=user` | **Trace 하단 송신 입력줄**(M1), **IG**(M2), **STOP**(셸, M1), 승인(셸, M2). 에이전트 다음 결과에 `others_since_last` / `NOTE:` | M1 |

루프 보장 장치(v1.2 유지): 가시성 불변식 I1~I6(§3.7), 실시간 목표·고부하 정직성(§3.8, 아래 §5.1에서 UI 예산으로 재분배), 걷는 뼈대 M1(§11, 아래 §7).

---

## 3. 구조 (v2판)

### 3.1 결론
1. 사용자에게는 여전히 **`loomd` 하나**(포트·URL·승인·감사 하나, D1).
2. CAN은 두 부분: **gateway 안 CAN 모듈**(Rust crate, `loomd`에 링크 — 데이터 질의·디코드·정책 훅) + **CAN worker 프로세스**(Rust 실행 파일 — 장치·타이밍·기록). 경계 기준은 "**장치/벤더 DLL과 타이밍 스레드는 worker, 사람과 에이전트가 묻는 질의는 gateway**".
3. **worker는 백엔드별로 나눈다**(권고, §3.3): `can-sim` / `can-pcan`(Windows, `PCANBasic.dll`) / `can-socketcan`(Linux, PCAN `peak_usb` 포함) / `vxl`(Windows, `vxlapi64.dll` — VN5640 CAN FD + 이더넷 원시 프레임). 실행 파일은 하나(`loom-can-worker --backend …`), 프로세스만 백엔드마다 따로 띄운다.
4. **분리 근거가 바뀌었다.** v1.2의 근거는 Python GIL(같은 프로세스 CPU 경쟁 → FC p99 1.5→52 ms [실측, `spikes/can/r1-results.json`])였다. Rust에는 GIL이 없으므로 이 근거는 약해진다. v2의 근거는 **C2**: ① 벤더 DLL 크래시가 데몬·다른 장치·저널을 죽이지 않음, ② worker별 메모리 예산 집행(Job Object/rlimit — CORE), ③ 타이밍 스레드(주기·FC)를 다른 도메인 부하와 OS 수준에서 분리(우선순위·코어). Rust에서 FC·주기 지연을 다시 재는 것은 ⟨Sol §5⟩.
5. Python은 **CANoe 브리지(P4, 선택)** 하나만 — 별도 Python 어댑터 worker, 같은 worker 프로토콜(CORE), 메모리 상한, 죽어도 데몬은 산다(C1). "왜 Rust로 안 되는가"는 ⟨Sol §6⟩.

### 3.2 gateway CAN 모듈 vs CAN worker — 책임 표

| 책임 | gateway CAN 모듈 (loomd 안) | CAN worker (별도 프로세스) | 이유 |
|---|---|---|---|
| 장치 열기·드라이버·벤더 DLL | – | ✓ (DLL은 worker에서만 로드, C2) | 크래시 격리 |
| 채널 레지스트리·라우팅(ch → worker) | ✓ | – | 사용자에게 채널 이름 하나 |
| 수신 스레드, `source_seq`, HW 타임스탬프 매핑 | – | ✓ | 타이밍 |
| 단기 링(저널·ISO-TP용) | – | ✓ 고정 크기 ⟨Sol C2⟩ | |
| **정본 저널 쓰기** | – | ✓ (CORE 저널 라이브러리를 worker에서 사용) | gateway가 죽어도 기록 계속 |
| TX 중재기·TX latch·프레임별 예약 ID/rate/하드 차단 재집행 | – | ✓ (A1 잡형 단위 송신) | 마지막 방어선이 장치 바로 앞 |
| 주기 스케줄러, ISO-TP FC/CF | – | ✓ 전용 스레드 | 타이밍 |
| SimNode·가짜 ECU·결함 주입 | – | ✓ `can-sim`만 | 실HW worker에 시뮬 부하 금지 |
| 버스 통계 원천(부하 추정·에러 카운터·버스 상태) | – | ✓ 1 s 집계 | 드라이버 정보 |
| 링 미러, Fixed 테이블, trace 질의(필터·`--around`·`mark:` 범위) | ✓ | – | 질의는 사람·에이전트 쪽 |
| 커서 `(client, channel)`, `--peek`, `others_since_last` CAN 집계 | ✓ (CORE 커서 위) | – | |
| DBC 저장·디코드·인코드, SignalCache, `wait-for`/`check` 평가 | ✓ | 인코드 결과(바이트)만 받음 | worker를 작게, DB는 백엔드 공통 |
| **신호 시계열 저장(Graphics 질의용)** — 신호별 링 + min/max 피라미드 | ✓ | – | §5.4 |
| 증거 생성·재검증(`can.evidence.verify`) | ✓ (저널 재조회) | – | §6 |
| IG 인코드 미리보기(바이트·범위 검사) | ✓ R0 | – | 사용자가 슬라이더를 움직일 때 |
| 정책 도메인 훅 `classify / approval_binding / presend_check` + 위험 표·`safe_state` 등록 | ✓ | presend의 장치 상태 질의에 응답 | core 파이프라인(D10/A2) |
| Activity(에이전트 읽기·대기) 생성 | ✓ | – | I4 |

**주의:** DBC 디코드는 gateway CPU를 쓴다. gateway에는 UDS 세션 관리자(D1)도 있다. 디코드는 **구독·wait-for·Graphics가 요구한 메시지만**, 상한 있는 작업 큐에서 한다(넘치면 `can.decode_backlog` 경고 + 해당 신호 `quality=lagging`). 수치 ⟨Sol C2⟩.

### 3.3 worker 분리안 비교 (C2 크래시 격리 관점)

| 안 | 구성 | 크래시 반경 | 메모리 예산 | 다채널 시각 일관성 | 프로세스 수(전형 벤치) | 판정 |
|---|---|---|---|---|---|---|
| A. 단일 `can-hw` + `can-sim` | 모든 벤더 DLL을 한 프로세스에 | `vxlapi64.dll` 크래시가 PCAN 채널까지 끊음 | 벤더별 구분 불가 | 좋음 | 2 | ✗ — C2 위반 소지 |
| **B. 백엔드별 worker** | `can-sim`, `can-pcan`, `can-socketcan`, `vxl` (+P4 `canoe-bridge` Python) | 한 벤더만 재기동 | 벤더별 예산 | 같은 벤더 안 채널끼리는 같은 포트·시계 | 3~4 | **권고** |
| C. 채널별 worker | ch마다 프로세스 | 가장 작음 | 가장 세밀 | **나쁨** — XL은 한 앱 포트에 여러 채널을 묶어야 같은 HW 시계를 쓴다. 같은 채널을 두 프로세스가 열면 init access 경합 | 채널 수만큼 | ✗ (필요하면 B 안에서 예외로) |

B의 세부 규칙:
- worker는 **필요할 때 띄운다**(그 백엔드의 첫 채널을 열 때, 또는 `loom.toml autostart`). 마지막 채널을 닫으면 유휴 30 s 뒤 종료(⟨CORE⟩ 감독 규칙과 맞춤).
- `vxl` worker는 VN5640의 **CAN FD 채널과 이더넷 원시 프레임 포트를 함께** 소유한다(같은 XL 드라이버·같은 앱 이름·같은 시계). 이름을 `can-vxl`이 아니라 `vxl`로 한 이유: 이더넷은 CAN이 아니고, DoIP 경로 (b)(사용자 공간 TCP/IP)를 고르면 TCP 스택도 이 프로세스에 붙어야 하기 때문. DoIP 경로 (a)/(b)/(c) 결정은 `_xteam/vn5640-eth.md`(UDS와 합의, 사실 조사 ⟨Sol §4⟩).
- 같은 PC에서 Windows는 `can-pcan` + `vxl`, Linux는 `can-socketcan`만(Vector·PCAN-Basic 경로 없음, B1). sim은 어디서나 `can-sim`.
- worker 크래시 → 감독자 재기동(지수 백오프, 크래시 루프 차단 ⟨CORE⟩). **재기동 후 주기 job·ISO-TP 채널·replay는 자동 재개하지 않음**(v1.2 원칙 유지). 채널은 `loom.toml autostart` 채널만 다시 열고, UI 상태 바에 `ch0 RESTARTED (worker crash 14:02:11) — jobs stopped` 배지 + `source.gap{cause:worker_restart}`.
- `canoe-bridge`(P4, Python): Windows 전용, 장치를 열지 않음. CANoe가 같은 VN5640을 쓰는 동안 `vxl` worker는 init access 없이 공존한다(실측 gate는 v1.2 K13a 유지).

### 3.4 프로세스 구성도 (v2)
```text
  사용자: Loom UI 앱 (브라우저, 127.0.0.1)            에이전트: Claude Code / Codex
   [셸: 채널 바·STOP·승인·Activity·env]                  └ loom can … (Rust 단일 바이너리, 단발)
   [CAN 패널: Trace · Graphics · IG · Data/State]                │
            │ HTTP(정적) + WS (viewer role)                         │ WS JSON-RPC (agent role)
            ▼                                                       ▼
 ┌──────────────────────────── loomd (Rust, CORE 소유) ─────────────────────────────────┐
 │ IPC·role · 이벤트 허브(stream_seq·snapshot·우선 큐·적응형 배치) · 정책/승인/감사 · 감독자 │
 │ 테스트 세션 코어(C7) · 저널 색인/조회                                                 │
 │ ┌───────── CAN 모듈 (CAN 팀 crate) ──────────┐  ┌ UDS 모듈(세션 관리자) ┐ ┌ UART … ┐ │
 │ │ 채널 레지스트리 · 링 미러 · Fixed 테이블    │  │  can.isotp.* RPC ────┼─┐          │
 │ │ trace/expect/check 질의 · 커서·others 집계  │  └──────────────────────┘ │          │
 │ │ DBC·디코더·SignalCache · 신호 시계열 저장   │                            │          │
 │ │ 증거 생성/재검증 · IG 미리보기 · 정책 훅    │                            │          │
 │ └────────────────────────────────────────────┘                            │          │
 └───────────┬───────────────┬────────────────┬──────────────────┬───────────┴──────────┘
   worker 프로토콜(CORE: 로컬 소켓/named pipe, 바이너리, 흐름 제어, 하트비트)
             ▼               ▼                ▼                  ▼
      ┌ can-sim ─────┐ ┌ can-pcan(Win) ┐ ┌ can-socketcan(Linux) ┐ ┌ vxl (Win) ──────────────────┐
      │ sim 버스      │ │ PCANBasic.dll │ │ SocketCAN FD          │ │ vxlapi64.dll (자체 FFI)      │
      │ SimNode·가짜ECU│ │ 수신·TX 중재  │ │ 수신·TX 중재          │ │ CAN FD ch · Eth 원시 프레임   │
      │ 결함 주입     │ │ 주기·ISO-TP   │ │ 주기(BCM?⟨Sol⟩)·ISO-TP│ │ 주기·ISO-TP · (b)면 TCP/IP   │
      │ 저널 writer   │ │ 저널 writer   │ │ 저널 writer           │ │ 저널 writer                  │
      └──────────────┘ └───────────────┘ └───────────────────────┘ └─────────────────────────────┘
                                         (P4, Win) canoe-bridge: Python 어댑터 worker ──COM──► CANoe
```
공통 worker 내부 스레드(초안, 수치·큐는 ⟨Sol C2⟩): 수신(드라이버 이벤트 대기) · TX 중재 · 주기 스케줄러(높은 우선순위) · ISO-TP(FC/CF 전용) · 저널 writer · IPC 송수신. 화면 경로와 기록 경로는 큐가 분리되고, 저널 큐가 넘치면 `record.gap`, 화면 쪽은 gateway가 `display.gap`(v1.2 §3.8 규칙 그대로).

### 3.5 v1.2 → v2 변화 요약
| 항목 | v1.2 | v2.0 |
|---|---|---|
| 언어 | Python(python-can, can-isotp, cantools, asammdf) | Rust 전부. Python은 P4 CANoe 브리지만 |
| worker | `can-hw` / `can-sim` 2종 | 백엔드별 `can-sim` / `can-pcan` / `can-socketcan` / `vxl` |
| 분리 근거 | GIL 실측 | C2 크래시·메모리 격리 + 타이밍 스레드 (GIL 근거는 Rust에서 재측정 대상) |
| Vector | python-can `vector`, 공존 실측 | **자체 XL FFI crate(CAN 팀 소유)**, VN5640 CAN FD + 이더넷 원시 프레임 |
| PCAN | M2 CC 위주 | **M2부터 CAN FD**(Win PCAN-Basic FFI, Linux SocketCAN FD) |
| 사용자 화면 | M1 Textual TUI → M3 웹(CAN 팀 소유) | **M1부터 웹 UI**(UI 팀 소유 셸·렌더). CAN은 패널 명세 + 데이터 질의. TUI는 보조·후순위 |
| viz | CAN 팀 소유 | UI 팀 소유. CAN은 CAN 데이터셋 어댑터 요구만 |
| 통합 테스트 | 없음 | `expect`·`check`·`stats --expect-period`·증거 객체·`mark:` 범위 (§6) |
| CANoe 브리지 | M2 정식(B3) | **P4 선택**(C4). 진단 기본 경로는 자체 스택 |

---

## 5. UI 패널 명세 (C6) — `_xteam/can-ui.md` 초안 재료

### 5.0 소유 경계
- **CAN 팀:** 패널의 *내용* — 무엇을 보여 주고, 어떤 데이터(이벤트 kind·RPC)로, 어떤 지연·고부하 규칙으로, 사용자가 어떤 입력으로 개입하는지. gateway CAN 모듈의 질의 RPC.
- **UI 팀:** 셸, 패널 프레임워크, 렌더링(가상 스크롤·플롯 엔진), 상태 관리, 레이아웃, 공통 요소(STOP·승인 대기열·env 배지·LIVE/LAG·Activity 타임라인·마커·테스트 실행 화면).
- **CORE:** 이벤트 허브·구독·바이너리 전송 형식(`_xteam/core-ui.md`), 스키마 → TS 타입.

### 5.1 CAN 패널 공통 규칙
| 규칙 | 내용 |
|---|---|
| 출처 배지 | 색 + **문자**(색맹 대비): `AGT` 주황 / `USR` 파랑 / `DEV` 회색 / `EXT` 보라(`external_actor`: `canoe`·`host`·`?`) / 보조 태그 `sim:BCM`, `UDS tp1`, `job p3`, `test s2`. v1.2 D50 유지 |
| 쓰기 상태 | TX 행은 같은 `request_id` 행을 갱신: `requested → accepted → sent → confirmed`, 또는 `failed`·`outcome_unknown`. `sent`는 "드라이버 접수"라고 툴팁에 명시(D4/A14) |
| env | 채널별 `SIM`/`BENCH`/`VEHICLE` 배지 상시(B4). VEHICLE은 반전색 |
| 생략·손실 | `display.gap` = Trace에 합성 행 `⋯ N frames not shown (seq a..b) — recording ok`. `record.gap`·`source.gap` = 빨간 행 + 채널 바 배지 `RECORDING INCOMPLETE` / `SOURCE LOSS (driver overrun)`. 세 가지 문구를 섞지 않는다(A13) |
| 신선도 | 채널 바 `LIVE lag 40 ms` / `LAG 620 ms`(노랑) / `STALLED`(heartbeat 1 s 끊김) / `LIVE-DEGRADED`(생략·다운샘플 중) — D9 |
| 지연 측정 정의 | 장치 프레임의 worker ingest `ts.mono_ns` → 브라우저 paint 완료. 브라우저↔데몬 시계 오프셋은 WS ping 왕복으로 추정. 표본 프레임의 paint 시각을 UI가 `ui.latency_sample`로 되돌려 보내 gateway가 p50/p95를 집계(§5.9 U-CAN-7) |
| STOP 중 | 셸 STOP latch 동안 CAN 패널 상단에 `STOP — agent/script TX latched`. 사용자 송신 입력은 **계속 동작**(A3) |

**지연 예산 (웹, 장치→paint p95)** [목표]: worker 수신 → gateway ≤ 10 ms(내부 배치) + gateway 처리 ≤ 10 ms + WS 배치 ≤ 40 ms(적응형, 조용한 뒤 첫 이벤트 즉시) + 브라우저 디코드·상태 반영 ≤ 20 ms + paint ≤ 16 ms ≈ **96 ms 명목, 게이트 p95 < 150 ms / p99 < 300 ms**(UI 브리프의 CAN 200 ms보다 엄격하게 잡아 여유 확보). Graphics 표시 age p95 < 250 ms.

### 5.2 채널 상태 바 (셸 상단 띠의 CAN 부분)
| 항목 | 내용 |
|---|---|
| 목적 | 지금 어떤 버스에, 어떤 장치로, 어떤 상태로 붙어 있는지 한눈에. 에이전트가 장치를 열거나 잃으면 즉시 보임 |
| 요소 | 채널명 · 드라이버 종류(`PCAN-USB FD` / `VN5640 ch3` / `SocketCAN can0` / `sim`) · CC/FD + 비트레이트(`500k/2M`) · 모드(`configure`/`shared`) · 버스 상태(`ACTIVE`/`PASSIVE`/`BUS-OFF`) · 부하 %(추정/실측 표시) · 에러 프레임 수(1 s) · TEC/REC(지원 시, 아니면 `n/a`) · env 배지 · REC 상태 · 신선도 배지 · `EXT canoe` 존재 · CANoe 측정 배지(P4 브리지 부착 시) · worker 상태(`RESTARTED`) |
| 데이터 | 이벤트 `can.channel_state`(열기·닫기·버스 상태·worker 재시작, **우선 제어 큐**), `can.stats`(1 Hz: fps, load, err, tec/rec), `record.health`, `source.gap`. snapshot에 현재 값 포함 |
| 지연 | 버스 상태 변화 → 배지 p95 < 200 ms. 통계 1 Hz |
| 고부하 | 통계는 집계값이므로 드롭 없음. 우선 큐라 프레임 폭주에도 묻히지 않음 |
| 사용자 개입 | 채널 클릭 → 채널 상세(설정·capability·최근 오류). **bus-off 복구 버튼**(user, R2, 자동 복구 반복 금지) · 채널 닫기(R0) · 열기는 `loom.toml` 채널 목록에서 선택(R0, configure는 사람 설정 그대로) |
| 에이전트 표시 | 에이전트가 open/close/복구를 하면 Activity + 바에 2 s 강조 |
| M | M1(sim, 최소: 이름·sim·REC·LIVE·env), M2(PCAN 전부), M3(VN5640·shared·EXT) |

### 5.3 Trace
| 항목 | 내용 |
|---|---|
| 목적 | CANoe Trace와 같은 작업 감각. 에이전트·사용자·장치 프레임을 같은 시간축에 출처와 함께 |
| 모드 | **Chrono**(시간순 스크롤, 일시정지 시 위치 고정) / **Fixed**(ID별 1행 덮어쓰기, 바뀐 바이트 하이라이트, count·주기). 고부하에서 Fixed 권장 배너 |
| 열 | 시간(상대 s / 절대, HW ts면 `◆` 표시) · Δt(같은 ID 직전 대비) · ch · dir · 출처 배지 · ID(11/29bit 표기 `x`) · 이름(DB 있으면) · FD/BRS/ESI 플래그 · DLC/len · data hex · 상태(TX 행) · 주석(ISO-TP/UDS·job·test) |
| 행 펼침 | 신호 이름=값 단위(raw), mux 분기, 범위 밖 경고 |
| **ISO-TP 묶음** | FF/CF/FC 행을 `ISO-TP tp1 FF len=20 → 22 F1 90` 한 줄로 접고 펼침. 주석 열에 UDS 팀 이벤트(`request_id`)가 있으면 `UDS ReadDataByIdentifier F190 → +62` (해석은 UDS payload 그대로 표시, V-UDS-5) |
| **EXT 배지** | Loom이 보내지 않은 프레임 중 외부 송신자로 판정된 것. 판정 근거(공존 포트 표시·`MSG_DONTROUTE`)를 툴팁에, 근거 없으면 `EXT ?`. 관찰 경로가 없으면 패널 상단 `bus not observed by Loom` |
| 필터 | 식 입력 `id in 0x100..0x1FF & org != dev & sig.EngRPM > 3000`, 출처/방향 토글. **관측 필터일 뿐** — 기록·ISO-TP에 영향 없음(v1.2 규칙 3) |
| 데이터 | 구독 `can.frame`(배치, 바이너리 열 블록 — U-CAN-2), `can.tx`(상태 갱신), `can.isotp_*`, `can.job_state`, `display.gap`/`record.gap`/`source.gap`, `test.step`·`core.mark`. 질의: `can.trace.query{channel, filter, since|around, until, max, cursor}`(일시정지 후 과거 스크롤·검색), `can.fixed.snapshot{channel}` |
| 지연 | 장치 → 행 paint p95 < 150 ms(§5.1), TX 요청 → `requested` 행 p95 < 100 ms |
| 고부하 | Chrono 예산 2,000 행/s(U-CAN-1) 초과분은 합성 생략 행 + 헤더 누적 카운터. Fixed는 모든 프레임 반영(ID 수에 비례), gap 이후 count·주기에 `?`. Pause는 화면만 고정(`⋯ paused 12.4 s (51k frames)`) |
| **사용자 개입** | 하단 **송신 입력줄**: `0x300#01`, `0x18DAF110#…`(29bit), `fd brs 0x123#…` / (M2) `MSG Sig=V …`. Enter → `can.tx.send`(viewer → origin=user). 자동완성: DB 메시지·신호. 행 우클릭: "이 프레임 다시 보내기", "IG로 보내기", "이 ID 필터", "마커 추가"(`core.mark`), "Graphics에 신호 추가" |
| 에이전트 표시 | `AGT` 행, 에이전트가 trace 질의한 범위를 왼쪽 여백 막대로 잠깐 표시(“agent read seq a..b”) + Activity |
| M | M1: Chrono + raw 열 + 송신 입력(raw) + 생략 행 + 출처 배지. M2: 신호 펼침·Fixed·필터식·ISO-TP 묶음. M3: EXT·UDS 주석·질의 기반 과거 탐색 |

### 5.4 Graphics
| 항목 | 내용 |
|---|---|
| 목적 | 다중 신호 시계열, 커서로 값·Δ 확인. 에이전트가 "그래프 보세요"라고 한 신호를 사용자가 바로 봄 |
| 요소 | 신호 트리(DB) → 드래그로 추가 · 단위별 y축 그룹(최대 4) · 패널 분할 · 커서 2개(값·Δt·Δvalue, 원본 샘플로 확정) · 줌/팬 · 라이브 따라가기/정지 · enum/bool은 step·state 레인 · gap에서 선 끊기(보간 금지) · `downsampled N→M` 표시 · 마커·테스트 단계 수직선 |
| 데이터 | 라이브: 구독 `can.signal_sample{signals[]}`(gateway가 디코드한 값, 신호별 배치). 과거/줌: **`can.signal.query{signals, t0, t1, px, mode:minmax}`** → 신호별 픽셀 버킷(first/last/min/max) 열 블록. 64 KiB 초과는 HTTP GET `raw_ref`(A5). 커서 값: `can.signal.at{signal, t}` → 원본 샘플 |
| 지연 | 신호 변화 → 선 갱신 표시 age p95 < 250 ms(10~20 Hz 갱신) |
| 고부하 | 서버측 min/max 다운샘플(극값 보존), series당 ≤ min(4×px, 20,000)점. 디코드 backlog면 해당 신호 `lagging` 배지. 알람·wait-for·check는 원본으로 계산(화면 근사값 사용 금지) |
| 사용자 개입 | 신호 추가/제거, 커서 위치에 **마커 추가**, 구간 선택 → "이 구간 trace 열기"(Trace `around`), "이 구간 CSV 내보내기"(R0) |
| 에이전트 표시 | 에이전트가 `sig wait-for`/`check` 중인 신호에 임계선(점선) + 라벨 `agent waiting > 3000`. 에이전트가 `loom ui open can/graphics?sig=EngRPM,VehicleSpeed`로 사용자에게 보여 줄 신호를 지정 가능 |
| M | M2: 데이터 질의 RPC(`signal.query/at/sample`) 완성 + UI 기본 플롯. M3: 커서·state 레인·마커·분할 |

### 5.5 IG (Interactive Generator)
| 항목 | 내용 |
|---|---|
| 목적 | DB 메시지를 골라 신호 값으로 1회·주기 송신, **에이전트가 돌리는 주기 job도 같은 화면에서 보고 바꾸거나 넘겨받기** |
| 요소 | 메시지 목록(채널별) · 메시지 행: ID·이름·주기 입력·[1x Send]·[▶ Start/■ Stop] · 신호 위젯(숫자/슬라이더 = DBC min..max, enum 드롭다운 = 값 테이블, raw 토글) · **인코드 바이트 미리보기**(입력할 때마다) · 위험 등급·사유(`R3 periodic on bench`) · **소유자**(`agent:claude-1 job p3 rev 4 · lease 8m12s · approved range EngRPM 0..6000`) · 버튼 [patch] [takeover] |
| 데이터 | `can.db.catalog{channel}`(메시지·신호 메타·범위·값 테이블), `can.encode.preview{msg, values}` → `{bytes, range_errors, mux, risk}`(R0, ≤ 20 ms [목표]), 구독 `can.job_state`(job 시작/갱신/정지/lease/소유자), 쓰기 `can.tx.send`, `can.periodic.start/update/stop`, `can.job.takeover{job_id}` |
| 지연 | 값 변경 → 미리보기 < 50 ms(입력 반응). job 상태 변화 → IG 반영 p95 < 200 ms |
| 고부하 | job 상태는 우선 큐. 주기 프레임 자체는 Trace가 담당(IG에 프레임을 흘리지 않음) |
| **사용자 개입** | ① 1회 송신(user, 승인 불필요 — rate·하드 차단은 적용) ② 주기 시작(user, R3도 **본인이 viewer이므로 셸 확인창 1회**로 처리 ⟨UI·CORE 합의 필요⟩) ③ **에이전트 job에 patch**: 승인된 신호 범위 안이면 즉시, revision CAS(경합 시 "에이전트가 방금 rev 5로 바꿈 — 다시 적용?") ④ **takeover**: 에이전트 job을 사용자 소유로 이전(에이전트 결과에 `others_since_last` + `E_INTERRUPTED_BY_USER`가 다음 job 조작 시) ⑤ Stop |
| 에이전트 표시 | job 소유자 배지 `AGT`, 에이전트가 `sig set`할 때 해당 위젯이 깜빡이고 값 옆 `agent rev 5` |
| M | M2(데이터 RPC + 최소 IG: 1회·주기·patch·stop), M3(takeover·mux UI·여러 메시지 패널) |

### 5.6 신호 Data / State
| 항목 | 내용 |
|---|---|
| 목적 | 고른 신호들의 현재값 표(Data), enum/bool의 시간 레인(State) |
| 열 | 이름 · 값 · 단위 · raw · enum 라벨 · age(ms) · 출처 메시지·origin · quality(`ok`/`stale`/`lagging`/`polled`(CANoe)) · 최근 변경 시각 |
| 데이터 | `can.signal_sample`(구독 신호), snapshot에 최신값. stale 기준 = 주기×3 또는 1 s |
| 지연 | 값 → 표 p95 < 200 ms |
| 고부하 | 표는 최신값 덮어쓰기(드롭 없음). State 레인은 Graphics와 같은 질의 |
| 사용자 개입 | 신호 클릭 → Graphics에 추가 / IG에서 열기 / 임계값 표시 추가 |
| 에이전트 표시 | 에이전트가 `sig get`한 신호는 행 옆 `read by agent 3s ago` |
| M | M2 |

### 5.7 Statistics (후순위, M4)
채널별 부하 그래프(추정/실측 구분), fps, 에러 프레임·버스 상태 타임라인, ID별 실제 주기·지터 vs DBC `GenMsgCycleTime`(±20% 하이라이트). 데이터 `can.stats`, `can.stats.query{by:id, window}`. 지원 안 되는 값은 `n/a`(0 아님). 사용자 개입 없음(보기 전용), 테스트 중에는 `stats --expect-period` 결과(§6)를 행에 표시.

### 5.8 와이어프레임

**M1 최소 CAN 패널** (UI 셸 안, sim)
```text
┌ Loom ─ [■ STOP]  approvals 0  ───────────────────────────────────────────────────────────────┐
│ ch0 sim 500k  ● LIVE lag 38ms  REC ok  env SIM   clients: agent:claude-1 · user:web          │
├ CAN Trace ─ ch0 ─ [Chrono] ─ filter: ___________________________ ─ [Pause] ─ shown 1,204 ────┤
│ 812.004   tx AGT  0x300   1  01                        sent         agent:claude-1            │
│ 812.009   rx DEV  0x301   1  01                                     sim:BCM                    │
│ 812.050   rx DEV  0x310   8  01 00 00 00 00 00 00 00                                           │
│ ⋯ 3,812 frames not shown (seq 9001..12812) — recording ok                                       │
│ 818.731   tx USR  0x300   1  00                        sent         user:web                   │
│ 818.736   rx DEV  0x301   1  00                                     sim:BCM                    │
├────────────────────────────────────────────────────────────────────────────────────────────────┤
│ send ▸ 0x300#00                                                         [Send ⏎]  (as user)   │
├ Activity ──────────────────────────────────────────────────────────────────────────────────────┤
│ 818.90 agent:claude-1 read ch0 trace (2 rows) · 818.73 user:web sent 0x300                     │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
```

**M3 CAN 작업 공간** (Trace · Graphics · IG · Data, 테스트 실행 중)
```text
┌ Loom ─ TEST TC-012 step 3/5 "RPM 3000 유지" ● running ─ [■ STOP] approvals 1 ─ env BENCH ──────┐
│ ch0 PCAN-USB FD 500k/2M ACTIVE load 23% err 0 REC ok LIVE 54ms │ ch1 VN5640 ch3 500k/2M shared EXT canoe │
├ Trace [Fixed] ─────────────────────────────┬ Graphics ─ decimated 1:40 ───────────────────────────┤
│ 0x100 EngineData 8 0B B8 5A.. 10.0ms ×812 AGT p3 │ EngRPM  ──╱‾‾‾‾‾‾‾╲─── rpm   ┊step3     ┊   │
│ 0x0FD ESP_01     8 10 0E 00.. 10.0ms ×41k  DEV   │ - - - - - agent waiting > 2900 - - - - - -  │
│ 0x7E8 ISO-TP tp1 FF len=20 → UDS 62 F1 90  DEV   │ GearPos [P][   D        ][N]                 │
│ ▸ EngRPM=3000 rpm  EngTemp=90 °C                 │ cursor Δ 4.20 s  EngRPM 2998 → 3001          │
├ IG ─ EngineData 0x100 ch0 ────────────────┼ Data ───────────────────────────────────────────────┤
│ period [10] ms  [■ Stop]  owner agent:claude-1  │ EngRPM   3001 rpm  4ms  AGT p3   ok            │
│ EngRPM [=====|=====] 3000  EngTemp [90]          │ VehicleSpeed 37.2 km/h 8ms ESP_01 ok          │
│ bytes 0B B8 5A 00 …  rev 4 · lease 8m · range 0..6000 [patch][takeover] │ GearPos D (4)  40ms TCU_02 ok │
├ Activity ─────────────────────────────────────────────────────────────────────────────────────┤
│ agent check "EngRPM between 2900 3100" mark:s3..now · step 3 marker · user approved test plan │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### 5.9 패널 ↔ 코어 데이터 계약

**CAN이 제공 (gateway CAN 모듈, RPC/이벤트 이름은 CORE 계약 v1 표기를 따름)**
| 종류 | 이름 | 요지 | M |
|---|---|---|---|
| 이벤트 | `can.frame` | 프레임 배치. UI용은 **열 블록**(ts_mono_ns u64, ch u8, dir/flags u8, id u32, len u8, data offset/bytes, origin u8, external u8, source id) — 형식은 `core-ui.md` | M1 |
| 이벤트 | `can.tx` | TX 행 상태 갱신(request_id 기준) | M1 |
| 이벤트 | `can.channel_state` (우선) | 열기/닫기/버스 상태/worker 재시작/capability | M1 |
| 이벤트 | `can.stats` | 1 Hz 채널 통계 | M1(sim 최소) |
| 이벤트 | `can.activity` | 에이전트 읽기·대기·job 조작 요약 | M1 |
| 이벤트 | `can.job_state` (우선) | 주기 job 시작/patch/lease/정지/소유자 | M2 |
| 이벤트 | `can.signal_sample` | 구독 신호의 디코드 값(신호별 열 블록) | M2 |
| 이벤트 | `can.isotp_*` | v1.2 §15.2 그대로 | M2 |
| RPC | `can.trace.query`, `can.fixed.snapshot` | 과거 탐색·검색, Fixed 초기 상태 | M1 |
| RPC | `can.tx.send` | 1회 송신(user/agent 공용, role로 origin) | M1 |
| RPC | `can.db.catalog`, `can.encode.preview` | IG·자동완성 | M2 |
| RPC | `can.signal.query`, `can.signal.at` | Graphics 줌·커서 | M2 |
| RPC | `can.periodic.start/update/stop`, `can.job.takeover` | IG | M2 |
| RPC | `can.channel.recover` | bus-off 복구(R2) | M2 |
| RPC | `can.stats.query` | Statistics | M4 |
| RPC | `can.evidence.verify` | 테스트 증거 재검증(§6, CORE 테스트 세션이 호출) | M2 |

**UI 팀에 요구 (U-CAN-*)**
| # | 요구 | 근거 |
|---|---|---|
| U-CAN-1 | Trace 가상 스크롤: 클라이언트 보존 ≥ 100k 행(초과분은 `can.trace.query`로 페이지), 유입 **2,000 행/s** Chrono에서 프레임 드롭 없는 렌더, Fixed ≤ 2,000 ID를 10 Hz 갱신 | 정상 프로파일 5k fps에서 화면 예산 |
| U-CAN-2 | `can.frame`·`can.signal_sample`을 **바이너리 열 블록**으로 받기(TypedArray 직행). M1은 JSON 허용, M2 전 전환 | 현 레코드 JSON ≈ 2.7 MB/s/구독 [추정, v1.2 §4.5] |
| U-CAN-3 | Graphics: ≥ 16 신호, y축 그룹 4, pan/zoom 중 ≥ 30 fps, step/state 렌더러, gap 끊기, 서버 다운샘플 결과를 그대로 그림(클라이언트 재샘플 금지) | CANoe 수준 |
| U-CAN-4 | 패널 간 연결: 공통 시간 커서·마커 동기, Trace 행 → Graphics/IG 열기, 구간 → Trace `around` | CANoe 작업 흐름 |
| U-CAN-5 | 셸 공통 요소에 CAN 데이터 연결: STOP(`core.stop_all`), 승인 대기열(`core.approval_*` — CAN plan 요약·바이트·신호 범위 표시 슬롯), env 배지, LIVE/LAG 계산 | A3, D6 |
| U-CAN-6 | 입력 위젯: hex 입력 검증(11/29bit, CC 0–8, FD 0–8/12/16/20/24/32/48/64), DB 기반 슬라이더·드롭다운, 단축키(송신 ⏎, Pause Space, STOP 셸 공통) | 오입력 방지 |
| U-CAN-7 | **지연 계측**: 표본 프레임(예: 1/50) paint 완료 시각을 `ui.latency_sample{source_id, paint_mono_est}`로 회신 | M1 수용 기준 자동 측정 |
| U-CAN-8 | **딥링크**: `/#/can/trace?ch=ch0&since=mark:s2`, `/#/can/graphics?sig=A,B&t0=…`. `loom ui open <path>`가 URL을 출력·열기 | 에이전트가 "UI의 여기 보세요"를 정확히 지시 |
| U-CAN-9 | 테스트 마커 렌더: `test.step` 이벤트를 Trace 전폭 행, Graphics 수직선, Activity로 | C7 |
| U-CAN-10 | 브라우저 메모리 상한: 패널별 링 고정 크기, 24 h 켜 둔 탭 RSS 증가 상한 ⟨UI C2⟩ | C2 |

---

## 6. 통합 테스트 제공 동작 (C7) — TEST 기획(`test/brief.md`)에 줄 것

### 6.1 대기·검증 명령
| 명령 | 의미 | 결과·종료 코드 | M |
|---|---|---|---|
| `loom can expect --id 0x301 [--data 01 --mask FF] [--origin device] [--after mark:s2\|seq:N\|now] --within 200ms` | 조건에 맞는 **프레임이 기한 안에 나타나는가** | 0 + 증거(첫 일치 프레임) / 3 `E_TIMEOUT` + 기한 내 근접 후보 3개 | M1 |
| `loom can send 0x300#01 --expect 0x301 --within 100ms` | 송신과 응답 확인을 한 번에(v1.2) | 결과 state `effective_verified`/`effect_unverified` + 증거 | M1 |
| `loom can trace --since mark:s2 --until mark:s3 [--ids …]` | 단계 구간 원본 조회 | 행 + `range` 메타 | M1 |
| `loom can sig wait-for "EngRPM > 2900" --timeout 5s` | 조건이 **처음 참이 되는 순간** 대기 | 0 + 증거 샘플 / 3 | M2 |
| `loom can check "EngRPM between 2900 3100" --from mark:s3 --to mark:s4 --mode always\|never\|eventually` | 구간 전체에 대한 **단언**(대기 아님, 이미 기록된 구간 평가) | `result: holds\|violated\|insufficient_data` + 위반 첫 샘플·통계, 종료 0(평가 성공, 결과와 무관) / 8(gap 포함 구간) | M2 |
| `loom can stats --id 0x310 --from mark:s3 --to mark:s4 --expect-period 100ms --tol 10%` | **주기 측정 검증** | 간격 n·min·mean·p99·max, 허용 밖 개수, `ts.source`(hardware/host) + `result: within\|outside` | M2 |
| `loom can last --id 0x310` | 현재 스냅샷 | 행 + 증거 | M1 |

설계 규칙:
- **검증 명령은 판정하지 않는다.** `result`는 사실 평가일 뿐, TC 판정(pass/fail/inconclusive)은 에이전트가 `loom test verdict …`로 증거를 인용해 내린다(판정 책임은 테스트 세션).
- `check`·`stats`의 시간 기준은 **`ts.mono_ns`**(같은 호스트의 단조 시계)다. `mark:`는 마커의 `mono_ns`로 풀린다. `stream_seq`로 자르면 worker 배치 지연(≤ 10 ms) 때문에 마커 직전 프레임이 뒤로 밀릴 수 있어 쓰지 않는다. HW 타임스탬프가 있으면 주기 측정은 HW 시각을 쓰고 `ts.source=hardware`로 밝힌다.
- 구간에 `record.gap`/`source.gap`이 있으면 `result: insufficient_data` + 종료 8 — **gap을 넘긴 PASS가 나오지 않게**.
- 원시 바이트 비교는 `--data/--mask`(hex), 신호 비교는 물리값 + 허용오차(`--tol`, 기본 DBC factor/2).

### 6.2 증거 객체 (인용 가능한 이벤트)
모든 검증 명령의 `--json` 결과 `data.evidence`:
```json
{"evidence_id":"ev-can-7f3a","domain":"can","kind":"can.expect",
 "query":{"cmd":"expect","channel":"ch0","id":"0x301","data":"01","mask":"FF","after":{"mark":"s2","mono_ns":"812003990000"},"within_ms":200},
 "items":[{"src":{"producer":"can-sim","epoch":"3","source_seq":"5013"},"stream_seq":"5013",
           "ts":{"mono_ns":"812009120000","source":"host"},"id":"0x301","dir":"rx","origin":"device","data_hex":"01"}],
 "range":{"from_mono_ns":"812003990000","to_mono_ns":"812203990000"},
 "gaps":[],"digest":"sha256:…"}
```
| 필드 | 쓰임 |
|---|---|
| `src{producer, epoch, source_seq}` | **영구 인용 키**(저널에 보존, D3). `stream_seq`는 gateway 재시작마다 바뀌므로 UI 링크용 보조 |
| `query` | 정규화된 질의 — Loom이 같은 질의를 저널에 다시 돌려 재현 |
| `digest` | items 정규 직렬화 해시 — 인용 위·변조 감지 |
| `gaps` | 구간 안 손실(있으면 판정에 쓸 수 없음을 표시) |

**재검증:** 테스트 세션 코어(CORE)가 `loom test verdict` 시 인용된 `evidence_id`(또는 객체)를 받아 **`can.evidence.verify`** 를 호출 → CAN 모듈이 저널에서 `query`를 재실행해 같은 items·digest인지, 인용한 `src`가 실제 존재하는지, 기대값(`--data`, 범위)과 일치하는지 확인 → `verified | mismatch | missing | gap`. 증거 없는 PASS는 코어가 거부(C7). 에이전트가 trace 행을 눈으로 읽고 "응답 있었음"이라 쓰는 것은 증거가 아니다(스킬 규칙).

### 6.3 세션 일괄 사전 승인과 CAN 잡
- `loom test start TC-012.yaml` 시 코어가 TC의 **장치 동작 계획**을 모아 한 번에 승인 요청한다. CAN이 계획 항목으로 내는 것: `{channel, kind: send|periodic|ig|isotp, ids, signal ranges, period range, count/duration 상한}`. CAN의 `approval_binding` 훅이 이 항목들의 digest를 만든다.
- 승인된 세션 안에서 에이전트의 R2/R3 CAN 동작은 **계획 범위 안이면 추가 확인 없이** 실행되고, 감사에 `approval=test:TC-012#item3`이 남는다. 범위 밖이면 일반 규칙(viewer 승인).
- **R4는 사전 승인 불가**(매번 once, D6). 하드 차단은 그대로.
- 세션 소유 job: 테스트 세션이 시작한 주기 job은 `owner=test:TC-012`. `loom test end`·abort 시 코어가 `safe_state` 동작(`can.periodic.stop`)을 finally로 실행(A15). 사용자 STOP이면 finally 자동 실행 없음 + UI "복구 송신" 1키(A15).
- lease: 세션 job의 lease = 세션 수명(최대 `max_job_duration`, A9). 세션이 끝나면 job도 끝난다.

### 6.4 테스트 단계 마커가 CAN에 찍히는 모양
- 코어가 `loom test step 3 "RPM 3000 유지"` 시 `test.step{tc, step, title, mono_ns}` 이벤트를 모든 도메인 타임라인에 발행(이름은 `core-test.md`).
- CAN Trace: 전폭 행 `━━ TEST TC-012 · step 3 · RPM 3000 유지 ━━`(Chrono), Fixed 모드에서는 상단 띠에 현재 단계. Graphics: 수직선 + 라벨. `--since mark:s3`와 같은 이름 체계(`s<step>` 별칭 ⟨core-test 합의⟩).
- 단계 판정이 나면 마커 행에 `PASS ✓ ev-can-7f3a`(증거 클릭 → 해당 프레임으로 스크롤).

### 6.5 예시 TC (자연어 → 명령 → 증거)
> **TC-012** "sim BCM에 Light_Req(0x300)=01을 보내면 100 ms 안에 0x301 응답이 오고, 이후 3초 동안 0x310이 100 ms ±10% 주기로 byte0=01을 유지한다."

```text
loom test start TC-012 --title "BCM light request"                # 코어: 세션·사전 승인(sim이라 R1, 승인창 없음)
loom test step 1 "Light_Req 송신과 응답"
loom can send 0x300#01 --channel ch0 --expect 0x301 --within 100ms --json      # → evidence ev-can-a1 (0x301 01 @+5.1ms)
loom test verdict 1 pass --evidence ev-can-a1
loom test step 2 "0x310 주기·값 유지 3 s"
loom can expect --id 0x310 --data 01 --mask FF --after mark:s2 --within 200ms --json   # 첫 갱신 프레임 → ev-can-a2
loom test wait 3s                                                 # 3초 유지 구간: 셸 sleep 대신 코어 대기(마커 기록) ⟨core-test 합의⟩
loom can stats --id 0x310 --from mark:s2 --to now --expect-period 100ms --tol 10% --json   # → ev-can-a3 (n=30, p99 101.8ms, within)
loom can check "byte(0x310,0) == 0x01" --from mark:s2 --to now --mode always --json      # → ev-can-a4 holds  (raw 바이트 식, DB 없어도 됨)
loom test verdict 2 pass --evidence ev-can-a2,ev-can-a3,ev-can-a4
loom test end                                                     # 코어가 can.evidence.verify로 4건 재검증 후 보고서
```
UI: 테스트 실행 화면(UI 팀)의 단계 3개 + CAN Trace의 마커 행 + 증거 클릭 이동. 사용자가 단계 2 도중 IG에서 0x300=00을 보내면 → 0x310 byte0이 바뀌어 `check` = `violated`, 에이전트 결과에 `others_since_last{count:1, origins:[user]}` → 스킬 규칙상 **판정 전에 사용자 개입을 보고**하고 `inconclusive` 또는 재실행을 묻는다.

---

## 7. 마일스톤 (v2판)

### 7.1 M1 — 걷는 뼈대 (Rust + 웹 UI, sim)
**내용:** `loomd`(CORE M1: IPC·허브·감독·정책 v0·저널) + **`can-sim` Rust worker**(sim 버스, 수신·TX 중재·latch, 저널 writer, raw 규칙 SimNode) + **gateway CAN 모듈 G1**(채널 레지스트리, 링 미러, trace/last/expect 질의, 커서·others 집계, Activity) + **`loom can` CLI**(`status / open / close / send(--dry-run, --expect) / trace(--since @last|mark:, --until, --peek) / last / expect / stop-all / tx resume / demo / node / record status·mark`, `loom ui open`) + **웹 UI 최소 CAN 패널**(UI 팀 구현: 채널 상태 바 최소, Trace Chrono + 송신 입력줄 + 생략 행 + 출처 배지, 셸 STOP, Activity) + `loom-can` 스킬 v0.

**수용 기준** (성능은 기준 머신 [목표], 논리 시험은 CI 3 OS)
| # | 기준 |
|---|---|
| ① | §2.0 E2E를 **브라우저 E2E(Playwright 등)** 로 자동 통과: 에이전트 `send 0x300#01` → sim BCM `0x301` → UI Trace에 `AGT`/`DEV` 행 → 브라우저에서 사용자가 `0x300#00` 송신 → 에이전트 다음 `trace --since @last` 결과에 USR 행 + **`others_since_last.count = 1`, origins=[user]** + 사람용 `NOTE:` |
| ② | 장치 → UI paint **p95 < 150 ms / p99 < 300 ms**(U-CAN-7 계측), TX 요청 → `requested` 행 p95 < 100 ms, trace 가시성 p95 < 50 ms |
| ③ | 늦은 참가: 에이전트가 10분 송신 중일 때 새 탭 → snapshot + live **< 1 s**, stream_seq 중복·누락 0 |
| ④ | 고부하: `can-sim` 20k fps 60 s → UI 멈춤 없음(메인 스레드 long task < 200 ms), **생략 행·누적 카운터·`LIVE-DEGRADED` 배지가 실제 생략 수와 일치**, 저널 프레임 수 = 생성 수(무손실) |
| ⑤ | 느린 뷰어 격리: 탭 2개 중 하나를 throttle → 그 탭만 `display.gap`, 다른 탭·저널·송신 정상 |
| ⑥ | STOP: UI STOP → agent TX 신규 submit 0까지 < 50 ms, 사용자 송신 입력은 계속 동작, `tx resume` 전 자동 재개 없음 |
| ⑦ | 크래시 격리(C2 맛보기): `can-sim` kill → 데몬·UI 살아 있음, 상태 바 `RESTARTED`, `source.gap{cause:worker_restart}`, job 자동 재개 없음 |
| ⑧ | 1 h soak(5k fps + UI 탭 2 + 에이전트 1 s마다 trace): `loomd`·`can-sim` RSS 증가 < ⟨Sol/CORE C2 수치⟩. 24 h soak는 M2 관문 |
| ⑨ | RPC 스키마에 숨은/비공개 구독 옵션 없음, 모든 TX가 publish-before-reply |

**M1에서 뺀 것:** DBC·신호·Graphics·IG·Data(루프는 raw로 성립), 승인 흐름(sim만 송신 → R1), 주기 송신, 실HW, ISO-TP, export, **TUI**(C6: 보조 — 후순위, 필요 시 UI 팀 공통 TUI 킷 위에서), CANoe.

### 7.2 이후 (한 줄씩)
| M | 내용 |
|---|---|
| **M2** | **PCAN-USB FD 첫 실HW**: Windows(`can-pcan`, PCAN-Basic FFI ⟨Sol: 공식 Rust FFI 존재 여부⟩) + Linux(`can-socketcan`, FD, 사람 사전 설정 검증 D47) · DBC·신호·`wait-for`/`check`/`stats --expect-period`·증거 재검증 · 주기 job·IG·승인(R2 viewer·R3) · Graphics/IG/Data 데이터 RPC + UI 기본 패널 · **ISO-TP 최소판(= UDS M1b 전제, D11)** · BLF/ASC export · **24 h soak 관문** |
| **M3** | **VN5640 CAN FD**: 자체 XL FFI crate + `vxl` worker, configure/shared 모드, **CANoe 측정 중 공존 실측**(v1.2 K13a), `EXT canoe` 판정 · Trace EXT/UDS 주석·Graphics 커서/마커·IG takeover 완성 |
| **M4** | **ISO-TP 완성**(주소 7종·FD·functional group, = UDS M3 전제) · **VN5640 이더넷 원시 프레임**(DoIP 경로는 UDS와 `_xteam/vn5640-eth.md`에서 결정한 (a)/(b)/(c)) · Statistics 패널 · MF4 export |
| **M5** | **CANoe 브리지 P4**(Python 어댑터 worker, 측정·신호·sysvar, UDS `canoe` 백엔드는 UDS 소유) · Replay · ARXML · TUI 보조 뷰 |

---

## 12. 작업 분해 (v2 초안)

담당: **O** = Opus 워커, **S** = Sol 워커. CORE·UI·TEST 작업은 "요구"로만 표시. 웹·viz 구현(v1.2 W1–W5, V1–V3)은 **UI 팀으로 이관**.

| ID | 작업 | 담당 | 의존 | M |
|---|---|---|---|---|
| P1 | **`_xteam/can-ui.md`** 패널 명세(§5) + U-CAN 요구 확정, UI 팀 회신 반영 | O | UI 팀 셸·프레임워크 초안 | M1 전 |
| P2 | `_xteam/can-core.md`: CAN이 CORE에 요구(worker 프로토콜에 CAN 메시지 종류·우선 큐, gateway 도메인 모듈 trait, 저널 라이브러리 worker 사용, 열 블록 `can.frame`, 테스트 세션 훅 `evidence.verify`) | O | CORE P0 | M1 전 |
| P3 | `_xteam/can-test.md`(또는 `core-test.md`에 CAN 절): §6 명령·증거 객체·사전 승인 항목·마커 | O | TEST 기획 초안 | M1 전 |
| K1 | `loom-can-worker` 골격 + `can-sim` 백엔드(sim 버스, 수신·TX 중재·latch, source_seq, 단기 링, 우선 제어 메시지) | S | CORE worker 프로토콜 stub | M1 |
| K2 | worker 저널 writer(CORE 저널 lib 사용), `record.gap`/`source.gap` | S | K1, CORE 저널 | M1 |
| K4 | SimNode raw 규칙(YAML) + `node`/`demo` + 결함 주입(크래시·drop) | S | K1 | M1 |
| G1 | gateway CAN 모듈: 채널 레지스트리·worker 라우팅, 링 미러·Fixed, `trace.query`/`fixed.snapshot`/`expect`, 커서·others, Activity | O | CORE gateway 도메인 trait | M1 |
| K3 | `loom can` CLI(M1 명령) + Registry(위험 표·훅·safe_state) + `loom ui open` 딥링크 | O | G1, CORE CLI | M1 |
| Q1 | M1 수용 시험 하네스: 브라우저 E2E·지연 계측(U-CAN-7)·고부하·느린 탭·STOP·크래시·1 h soak | S (+UI 팀) | K1–K4, G1, K3, UI M1 패널 | M1 |
| D0 | `loom-can` 스킬 v0(§8) + `loom can help --json` 정본 | O | K3 | M1 |
| K5a | `can-pcan`(Windows, PCAN-Basic FFI, CC/FD) | S | K1 | M2 |
| K5b | `can-socketcan`(Linux FD, netdev 검증·`setup-hint`, `EXT host`) | S | K1 | M2 |
| K6 | DBC 파서·디코더·인코더(G1 안), SignalCache, 신호 시계열 저장·min/max 피라미드 | S | G1 | M2 |
| G2 | 신호·IG 데이터 RPC: `signal_sample` 구독, `signal.query/at`, `db.catalog`, `encode.preview` | O | K6 | M2 |
| K7 | `sig get/wait-for`, `check`, `stats --expect-period`, 증거 객체·`evidence.verify` | O | K6, G1, CORE 테스트 세션 | M2 |
| K8 | CAN 정책 매핑(위험 표, R2 viewer, approval_binding 신호 범위·테스트 계획 항목, presend, takeover/override) | O | CORE 정책 | M2 |
| K9 | 주기 스케줄러 스레드(job, rev CAS, lease, 프레임별 재집행) | S | K1, K8 | M2 |
| K10 | ISO-TP 최소판(Rust, FC 전용 스레드, v1.2 §15.2 계약) | S (+UDS) | K1, K8 | M2 |
| K11 | 버스 통계 원천(worker) + `can.stats` | S | K1 | M2 |
| K12 | BLF/ASC export(worker 밖 job) | S | K2 | M2 |
| Q2 | 24 h soak(CAN 고부하 + UART flood + UDS 폴링, 마스터 C2) CAN 부하 패턴 제공·판정 | S (+CORE) | K5a/b, K9, K10 | M2 |
| K13 | `vxl` worker: XL FFI crate(CAN FD), configure/shared, 공존 실측, `EXT canoe` | S | K1 | M3 |
| K14 | ISO-TP 완성 | S | K10 | M4 |
| K15 | XL 이더넷 원시 프레임 포트(+ (b)면 TCP/IP 위치) — UDS와 | S (+UDS) | K13, `vn5640-eth.md` | M4 |
| K16 | CANoe 브리지 P4(Python 어댑터 worker) | S(COM) + O(CLI·정책) | CORE 어댑터 worker 프로토콜 | M5 |
| D1 | `loom-can` 스킬 완성 + references(`help --json`에서 생성) | O | K7, K8 | M2~M3 |

**UI 팀으로 넘기는 것:** v1.2 W1(웹 골격), W2(Trace 렌더), W3(Data/IG/승인/STOP), W4(Graphics), W5(Statistics), V1–V3(viz 데이터셋·`loom viz`·바이너리 WS). CAN은 각 패널의 명세(P1)와 데이터 RPC(G1·G2·K7)를 제공한다.

---

## 8. 스킬 개정 (C8) — `loom-can` v2 골자

변경점:
1. **UI 안내:** "monitor를 열어 달라" → **"Loom UI를 열어 달라"** + URL(`loom ui open can/trace?ch=…`이 출력). 결과의 `viewers: 0`/`no_viewer_attached`이면 첫 송신 전에 안내.
2. **정본 = `loom can help --json`**(명령·인자·위험 등급·종료 코드·예시). SKILL.md는 워크플로와 규칙만, references는 `help --json`에서 생성(손으로 중복 작성 금지).
3. **테스트 세션 규칙:** 판정은 검증 명령(`expect`·`check`·`stats --expect-period`·`wait-for`)의 **증거를 인용**해서만. trace를 눈으로 읽은 결론은 증거가 아님. gap(종료 8)·`insufficient_data`·사용자 개입(`others_since_last`)이 있으면 PASS 금지 → `inconclusive` 또는 사람에게 묻기.
4. python-can 언급 제거 → "Loom이 장치를 소유하는 동안 다른 도구·스크립트(python-can, 벤더 툴, CANoe 송신 노드 추가)로 같은 장치를 열거나 송신하지 않는다."
5. **`loom-viz`는 UI 팀 소유로 이관** — CAN 스킬은 "추이·대량 데이터는 `loom viz show …` 또는 `loom ui open can/graphics?sig=…`"로 안내만.

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
5. Send: `--dry-run` for R2+ → one line to the user → run. Periodic: note job id, `periodic stop --mine` when done.
6. Trends → `loom ui open can/graphics?sig=…`; don't dump numbers.

## Rules
- exit 4 denied → report, don't retry. exit 5 → approval/confirm needed in the UI; tell the user and wait. exit 6 → the user intervened or a lock; tell the user. exit 8 / data-loss warning → report the gap before concluding. exit 9 → `loom daemon status`.
- `others_since_last` / `NOTE:` = the user acted. Mention it and adapt; never silently overwrite.
- Never edit loom.toml to relax policy, never approve your own request or drive the UI's approval buttons.
- env=vehicle follows bench rules; say "vehicle" when you report a send.
- In a test session: verdicts must cite evidence ids from expect/check/stats/wait-for. No evidence, a gap, or user intervention → not PASS (inconclusive or ask).
- Emergency: `loom can stop-all`.
```
(약 30줄 — D13의 ≤ 90줄 안. 공통 규칙(종료 코드·승인·others)은 `loom` 공통 스킬과 겹치므로 리드가 공통 스킬 확정 후 줄인다. `loom-test` 스킬은 TEST 기획 소유, CAN 스킬은 증거 규칙 한 줄만 둔다.)

---

## 9. 결정 기록 제안 (v2.0 방향 전환 행)

| # | 쟁점 | 결정 제안 | 이유 |
|---|---|---|---|
| D54 | v2.0 방향 전환 (C1·C6) | Rust 코어·worker, 웹 UI 1급, CAN 웹/viz 소유 → UI 팀 이관, CAN은 패널 명세·데이터 질의 | 마스터 C1–C8 |
| D55 | worker 분리 단위 | **백엔드별 worker**(`can-sim`/`can-pcan`/`can-socketcan`/`vxl`), 실행 파일 하나 | C2 크래시·메모리 격리, XL 다채널 시계 일관성(채널별 분리 거부) |
| D56 | 분리 근거 갱신 | GIL 근거 → C2 격리 + 타이밍 스레드. Rust FC·주기 지연 재측정 ⟨Sol⟩ | 정직성: Python 실측을 Rust 근거로 쓰지 않음 |
| D57 | `vxl` worker 범위 | VN5640 CAN FD + 이더넷 원시 프레임 한 프로세스, DoIP 경로는 `vn5640-eth.md` | 같은 드라이버·포트·시계 |
| D58 | 디코드 위치 | gateway CAN 모듈(요구된 메시지만, 상한 큐) | worker를 작게, DB 공통 |
| D59 | 검증 명령 | `expect`·`check`·`stats --expect-period`는 사실만 평가, 판정은 테스트 세션 | 환각 방지·책임 분리 |
| D60 | 증거 인용 키 | `src{producer, epoch, source_seq}` + 정규 질의 + digest, 시간 구간은 `mono_ns` | stream_seq는 재시작마다 바뀜, 배치 지연 |
| D61 | M1 뷰어 | 웹 UI 최소 CAN 패널(UI 팀 구현), TUI는 후순위 | C6 |
| D62 | UI 지연 게이트 | 장치→paint p95 < 150 ms(브라우저 계측 회신) | UI 브리프 200 ms보다 여유 |

---

## 핵심 원칙 체크리스트 (02) — 이 초안 기준
- [x] 핵심 루프 표 — §0 (에이전트 명령 / Rust 구성요소 / UI 패널 / 첫 M)
- [x] 종단 간 시나리오 — §7.1 ①(브라우저 E2E로 수용), §5.8 M1 와이어프레임, §6.5 TC 예시(사용자 개입 포함)
- [x] 실시간성 목표 + 고부하 — §5.1 웹 지연 예산·측정 정의, 패널별 지연·고부하 행, §7.1 ②④⑤
- [x] M1 = 걷는 뼈대 — §7.1 (Rust sim worker + loomd + CLI + 웹 최소 패널), 뺀 것 명시
- [x] 사용자 화면이 에이전트와 분리되지 않음 — §0 ④, §5.3 사용자 송신·에이전트 읽기 표시, §7.1 ③⑨, v1.2 I1~I6 유지
- [x] 후순위/비범위 — §7.1 "뺀 것", §7.2 M5, §5.7 Statistics 후순위
