# can → ui: CAN 패널 명세 (v2.0, can-lead 작성, 2026-11-10)

근거: 마스터 §2.3 **C6**(UI 1급, UI 팀이 셸·패널 프레임워크·렌더링 소유, CAN은 패널 내용 명세와 데이터 질의 소유), `ui/brief.md`, CAN `plan.md` v2.0 §5. v1.2 웹 패널 표(§5.3)·TUI(§5.2)에서 출발했다.

**요약 — UI 팀에 부탁하는 것:** U-CAN-1~10(§5.9 아래 표). 특히 ① Trace 유입 2,000 행/s 가상 스크롤과 클라이언트 보존 100k 행, ② `can.frame`·`can.signal_sample` 바이너리 열 블록(M2 전), ③ 지연 계측 회신 `ui.latency_sample`(M1 수용 기준 자동 측정), ④ 딥링크(`loom ui open can/trace?…`), ⑤ 테스트 마커 렌더.
**CAN이 제공하는 것:** §5.9 "CAN이 제공" 표의 이벤트·RPC(이름은 CORE 계약 v1 표기를 따름).
**합의가 필요한 열린 점:** (a) 사용자 본인이 시작하는 R3(IG 주기 송신)를 셸 확인창 1회로 처리할지(CORE·UI), (b) 지연 게이트 p95 < 150 ms(UI 브리프 200 ms보다 엄격) 수용 여부, (c) 열 블록 형식은 `core-ui.md` 결정을 따름.

## 패널 명세 본문 (CAN `plan.md` v2.0 §5와 같은 내용)

### 5.0 소유 경계
- **CAN 팀:** 패널의 *내용* — 무엇을 보여 주고, 어떤 데이터(이벤트 kind·RPC)로, 어떤 지연·고부하 규칙으로, 사용자가 어떤 입력으로 개입하는지. gateway CAN 모듈의 질의 RPC.
- **UI 팀:** 셸, 패널 프레임워크, 렌더링(가상 스크롤·플롯 엔진), 상태 관리, 레이아웃, 공통 요소(STOP·승인 대기열·env 배지·LIVE/LAG·Activity 타임라인·마커·테스트 실행 화면).
- **CORE:** 이벤트 허브·구독·바이너리 전송 형식(`_xteam/core-ui.md`), 스키마 → TS 타입.

### 5.1 CAN 패널 공통 규칙
| 규칙 | 내용 |
|---|---|
| 출처 배지 | 색 + **문자**(색맹 대비): `AGT` 청록 / `USR` 노랑 / `DEV` 회색 / `SCR` 보라 / `EXT` 보라 점선(`external_actor`: `canoe`·`host`·`?`) — 마스터 E8 통일안 / 보조 태그 `sim:BCM`, `UDS tp1`, `job p3`, `test s2`. v1.2 D50 유지 |
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


## v2.0 갱신 (can-lead, 2026-11-10)
- 마스터 C4.1: VN5640은 CAN FD 전용(이더넷 포트는 Loom이 열지 않음). 패널 명세 영향 없음 — 채널 바의 `VN5640 ch3`는 CAN 채널.
- U-CAN-2(바이너리 열 블록): UI r1b의 "M1 JSON, 바이너리는 M2–M3" 일정을 받아들인다. 다만 **CAN M2a(PCAN FD 실HW·24 h soak) 시작 전**에 `can.frame` 열 블록이 있어야 정상 프로파일 5k fps에서 화면 예산을 맞출 수 있다 — CORE M2 초반 고정 요청에 동의.
- 증거 표시(§5.8 M3 와이어프레임의 `ev-can-…`)는 TEST 정본 ref(`can:ch0@seq:…`)로 읽는다. 클릭 → 해당 프레임으로 스크롤(U-CAN-8 딥링크).

## ui 의견

ui-lead, 2026-11-10 (UI `plan.md` v2.0 기준). 대부분 수용한다.
- **수용**: U-CAN-1(보관 하한 100k·상한 2M — 탭 데이터 예산 256 MiB 안, 2k행/s Chrono, Fixed 10–20 Hz), U-CAN-3(16신호·4축·≥ 30 fps — UI-M2 관문), U-CAN-4, U-CAN-6, U-CAN-8(딥링크 형식만 `/ui/#/p/can/trace?channel=ch0&at=mark:s2`로 통일), U-CAN-10. 지연 게이트 **p95 < 150 / p99 < 300 ms** 수용 — 그래서 정상 부하는 rAF마다 렌더하고 20 Hz는 예산 초과 시만.
- **M 배정**: 채널 바 M1(셸 `channel_bar` 슬롯), Trace M1(Chrono·raw·송신줄·생략 행·에이전트 읽은 범위 막대), Fixed·신호 펼침·필터식·IG·Data·Graphics 기본 M2, Graphics 고급 M3.
- **U-CAN-2**: CORE에 `loom.columns.v1` M2 **초반** 확정을 요청했다(CAN M2a 전 조건 동의). 열 필드(ts_mono_ns u64, ch, dir/flags, id, len, data offsets/bytes, origin, external, source id)는 `core-ui.md`에서 CORE와 확정.
- **이름 변경 요청**: U-CAN-5 STOP은 CORE가 확정할 이름을 따른다(`core.stop_all` 또는 `core.stop{scope}`). U-CAN-7 `ui.latency_sample` → **`core.metrics.report{kind:"ui.render.sample"}`**(CORE 단일 스키마). U-CAN-9 `test.step` → TEST/CORE 합의 `test.step_started/ended`.
- **출처 색 변경 통보**: 렌더는 UI 소유라 전 팀 통일안 **AGT 청록 · USR 노랑 · DEV 회색 · SCR 보라 · EXT 보라 점선**(문자 병기)으로 확정한다(CAN D50의 AGT 주황·USR 파랑 대체). 이견 있으면 이 절 아래에.
- 사용자 시작 R3(IG 주기 송신): "요청+승인" 합친 본인 확인 모달 1회 + 승인과 같은 존재 확인(M2). 서버가 승인 객체를 만들고 deny·rate 검사는 그대로.
- 에이전트 표시(Trace 읽은 범위 막대, Graphics `wait-for` 임계 점선, Data `read by agent 3s ago`) 수용 — CORE에 `core.activity` 범위·술어 필드 요청함.
