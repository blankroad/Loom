# can → core 요청 (v2.0, can-lead 작성, 2026-11-10)

근거: 마스터 §2.3 C1–C3·C7, CAN `plan.md`(v1.2 §3·§6·§10·§12, v2.0 개정 중), 실측 `spikes/can/r1-results.json`(같은 프로세스 CPU 경쟁 시 ISO-TP FF→FC p99 1.49 → 51.9 ms). 아래는 CAN worker·CAN gateway 모듈이 Rust 코어에 기대는 것. 이름·형식은 CORE가 정하고 `## core 답변`에 적어 주면 CAN이 따른다.

## 1. worker 프로토콜 (C1·C2)
| # | 요구 | 이유 |
|---|---|---|
| W1 | **두 레인**: 대량 레인(프레임 배치) + 우선 제어 레인(STOP latch, ISO-TP PDU·`rx_start`, 채널 상태, 승인 결과). 대량 레인이 밀려도 제어 레인은 막히지 않는다 | v1.2 A5·R-TP-15: UDS PDU 전달 p99 ≤ 5 ms 목표, STOP은 즉시 |
| W2 | 대량 레인 흐름 제어 = gateway가 주는 크레딧. 크레딧이 없으면 worker는 **드라이버 수신을 막지 않고** 화면 전달분만 버리고 `display.gap`(구간·개수)을 보낸다 | CAN은 backpressure 불가. 드라이버 큐를 막으면 `source.gap`이 생긴다 |
| W3 | **기록은 gateway를 거치지 않는다**: CORE 저널 writer를 worker 프로세스 안에서 라이브러리로 돌리거나(1안), 저널 전용 경로를 둔다(2안). gateway가 죽거나 느려도 기록은 계속 | v1.2 D2/§3.2 "gateway가 죽어도 저널은 계속", A12 |
| W4 | 메시지 헤더에 원본 ID `(producer_id, producer_epoch, source_seq)`. `stream_seq`는 gateway가 붙인다(D25 두 층 유지) | 프로세스 분리 후 단일 순번 주체 |
| W5 | STOP latch 전달: 제어 레인, worker ack, 목표 지연 p99 < 10 ms(목표, 미측정). ack가 없으면 gateway가 worker를 `unresponsive`로 표시 | A3 |
| W6 | gateway 재시작 시 살아 있는 worker에 **다시 붙음**(discovery/lock, 장치 이중 open 방지) | v1.2 §3.2 |
| W7 | worker SDK가 hot path 스레드에 async 런타임을 강제하지 않는다. CAN worker는 RX·FC(ISO-TP)·주기 스케줄러를 **전용 OS 스레드**(우선순위 설정 가능)로 돌리고 SDK와는 상한 채널로만 만난다 | FC·주기 타이밍(R1 실측) |
| W8 | worker 상태 지표 이벤트(RSS, 큐 깊이·최대치, 드롭 카운터)를 주기 보고하는 공통 형식. 메모리 예산 초과 시 동작(경고 → 화면 구독 축소 → 재기동)을 CORE 감독자가 집행 | C2 |
| W9 | 벤더 DLL(`vxlapi64.dll`, `PCANBasic.dll`)은 worker에서만 로드. 감독자는 worker 크래시 시 재기동하되 **주기 송신·ISO-TP 송신은 자동 재개하지 않는다**(v1.2 원칙 유지), 수신 재개만 | C2, v1.2 §10.1-5 |
| W10 | Python 어댑터 worker(CANoe COM, 선택 P4)도 같은 프로토콜 + 생성된 Python 타입. 메모리 상한·재기동은 감독자 | C1 |

## 2. gateway 쪽 도메인 모듈 (C1, 마스터 §3.2 "CAN gateway 측 모듈")
- `loomd`에 정적으로 링크되는 도메인 crate 등록 방식(trait): CAN 질의(trace 필터·`--around`·Fixed 테이블·커서 `(client, channel)`·others_since_last 집계), DBC 디코더·SignalCache, Graphics용 시계열 질의(min/max 다운샘플).
- 필요한 코어 API: 링 미러 읽기(seq 범위·시간 범위), **저널 인덱스 질의**(seq/시간 → 레코드, 기록 탐색·재생·테스트 증거용), 구독자별 화면 예산 훅.

## 3. 쓰기 파이프라인 (A1·A2·A14)
- 요청형/잡형 구분, 도메인 훅을 Rust trait로: `classify`, `approval_binding`, `presend_check`. 잡의 start/update/renew/stop만 코어가 게이트하고 프레임 단위 송신은 worker가 집행(v1.2 D30).
- Registry에 `safe_state` 동작(`can.periodic.stop`, `can.stop_all`) 선언.

## 4. 스키마 (C3)
- CAN 이벤트 kind(`can.frame`, `can.isotp_*`, `can.periodic.*` 상태, `can.channel_state`, `source.gap` 등)를 serde 타입으로 정본화 → JSON Schema → UI TS 타입.
- 고속 프레임의 UI 전송용 **열 블록 바이너리**(id·ts·dlc·data 오프셋 열) — UI 팀과 `core-ui.md`에서 정할 때 CAN 프레임 필드 목록을 반영해 달라.

## 5. 테스트 세션 (C7)
- 증거 인용 검증에서 도메인 훅이 필요: 코어는 인용(`{domain, seq 범위, 조건}`)을 받아 해당 도메인 crate의 `verify_evidence(citation) -> {ok, matched, detail}`을 부른다. CAN은 "이 seq 범위에 ID X가 있고 신호 Y가 조건을 만족" 같은 검증을 제공한다.
- 테스트 단계 마커가 CAN 저널·Trace에 같은 `mark` 이벤트로 찍히는 공통 경로(`--since mark:X`와 같은 것).

## 6. 시험·soak (C2)
- CAN은 sim 부하 생성기(혼합 fps, FD 64 B, 주기 job, ISO-TP 왕복, bus-off 주입)를 제공. CORE는 24 h soak 하네스·지표 수집(RSS 추이, 지연 p99, 손실 카운터)을 제공.
- CI에 Windows x64 러너(벤더 DLL 없이 빌드·가짜 백엔드 시험)와 Linux vcan 가능 러너.

## v2.0 갱신 (can-lead, 2026-11-10 — core R2 리뷰 확인 후)
- core R2 리뷰가 W1–W10을 수용한 것을 확인했다. **W9 "재기동 후 수신만 재개"는 도메인 선언**으로 일반화하는 데 동의(UART는 자동 open 안 함 — CAN은 `autostart` 채널만 장치 identity 재검증 후 RX 재개, 송신 잡·ISO-TP·replay는 재개 안 함).
- W6 재부착 시점(M1 vs M2)은 CORE 결정을 따른다. M2로 가면 CAN M1은 "gateway 사망 시 worker latch + 저널 계속 + orphan 종료"로 정직하게 표시(`source.gap{cause:gateway_restart}`).
- 증거(§5): CAN은 `EvidenceVerifier`에 도메인 술어 `can.frame_match`·`can.signal_range`·`can.id_period`와 신호 샘플 추출기를 제공한다(형식은 `_xteam/test-domains.md` `## can 의견`). witness 유형별 판정 규칙(긍정 관측·부재 coverage·측정·user assertion)으로 바꾸는 데 동의, "agent 자신의 tx만으로는 불가" 유지.
- 도메인 Emitter(RX·FC 스레드)는 block하지 않는다(`try_send` + 카운터). block 허용은 IPC 스레드만(core Sol 리뷰 Q6 지적 수용). CAN 큐 상한 표는 CAN `plan.md` §9.4.

## core 답변
core-lead, 2026-11-10 (R3 통합) — 정본 `docs/plan/core/plan.md` v2.0. 아래는 요약이며 차이가 나면 plan.md가 우선.

| 요청 | 답 | plan.md |
|---|---|---|
| W1 두 레인 | 수용 — ctl·data 별도 로컬 소켓(UDS / named pipe). ctl 안에서도 `Latch`·우선 `Input`은 SDK IPC 스레드가 큐 없이 즉시 처리, 프레임 ≤ 64 KiB(최악 대기 1프레임) | §3.3 |
| W2 크레딧 | 수용 — 바이트 크레딧, 창 = 유입률 × 100 ms(기본 1 MiB, worker별 설정). 크레딧 0이면 화면 미러(4 MiB/8192건)만 드롭 + `display.gap{producer_range}`, 장치 수신·저널은 그대로 | §3.3, Q3–Q4 |
| W3 기록이 gateway를 거치지 않음 | 수용(1안) — `loom-journal` writer가 worker 프로세스 안 스레드. 세그먼트 = 길이 + CRC32C 프레임, M1 codec JSON(D7), M2 msgpack은 CR-C14 | §6.2 |
| W4 원본 ID | 수용 — 원본(`SourceRecord`)에는 stream seq 없음, 허브가 붙이고 gateway **mapping 로그(정본)**에 기록 | §3.4, §6.1 |
| W5 STOP ack | 수용 — 메모리 latch → worker `Latch` → ack(p99 ≤ 10 ms 목표) → 영속은 그 뒤. 미응답 = `stalled` + `core.latch{missing}` | §3.5 |
| W6 재부착 | **M2**. M1 = gateway 사망 시 worker 즉시 TX latch + 저널 계속 + orphan 30 s 후 seal·종료, 공백은 `source.gap{cause:"gateway_restart"}`(CAN 제안 그대로) | §3.3 |
| W7 hot path async 강제 안 함 | 수용 — 도메인 전용 OS 스레드 ↔ `Emitter`(동기·non-blocking, 실패 반환). block은 SDK IPC 스레드만 | §3.9 |
| W8 지표·예산 단계 | 수용 — `Health` 1 s(RSS·commit·큐·드롭·journal written/durable). soft 10 s → 축소 경고, 60 s 또는 hard → latch·seal·재기동. CAN worker(2채널) 기본 96/192 MiB | §3.3, §9.3 |
| W9 DLL은 worker만, 재기동 시 수신만 | 수용 — 도메인 선언 `reopen_after_crash: RxOnly`(identity 재검증은 CAN 쪽), job·ISO-TP·replay 자동 재개 금지 | §3.3 |
| W10 Python 같은 프로토콜 | 수용 — P0는 Rust↔Python wire golden fixture, 실행 SDK는 P2–P4 | §3.3 |
| §2 gateway 모듈 API | 수용 — `CallCtx`의 링·저널 질의(`JournalReader::scan(query, byte_limit)`), `DisplaySummarizer`, 리듀서(Fixed·신호, 화면 드롭 후 `quality:stale`) | §3.4, §3.9 |
| §3 쓰기 파이프라인 | 수용 — 요청형/잡형/대화형, 훅 3개 동기 ≤ 1 ms, `safe_state` 등록(`can.periodic.stop` 등) | §10.3 |
| §4 스키마·열 블록 | 수용 — serde 정본. UI binary `loom.columns.v1`은 M2(C29), CAN 필드 목록(id·ts·dlc·flags·data 오프셋) 반영 | §3.4 |
| §5 증거·마커 | 수용 — `EvidenceVerifier`(coverage_need·EvalBudget), `test.step_*` 마커는 M1 | §10.9 |
| §6 soak·CI | 수용 — CORE 하네스(§9.4·§9.6), CAN 부하 생성기 요청. CI Windows(DLL 없이 fake)·Linux vcan job | §9.6, §9.8 |
