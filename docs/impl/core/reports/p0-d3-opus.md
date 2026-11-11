# P0 D3–5 보고 — core-opus (A proto 보강 · B fixture-replay · C testkit · D echo)

작업 위치: `.worktrees/core`(기준 `f01221a`). **커밋하지 않음.** `crates/loom-schema`·`docs/contract`·루트 `Cargo.toml`은 건드리지 않음.

## A. `loom-proto` 보강 — **완료**
- 새 모듈 `stream.rs`: `CONTROL_SUB = "control"`(Q1: viewer 연결마다 서버가 자동 생성, `core.events{sub:"control"}`), 알림 이름 상수(`core.events/snapshot_part/snapshot_end/heartbeat/closed`), `SnapshotItem{domain, channel?, kind:"<domain>.snapshot", state}`, `SnapshotPart{sub, n, total, items, events}`, `SnapshotEnd{sub, stream_seq}`, `Heartbeat{stream_seq_hw, daemon_mono_ns, daemon_wall_ns, record_health, latch, sub_stats[]}`, `SubStats`, `Closed{reason, detail?}` + `CloseReason{slow_consumer, proto_mismatch, shutdown, auth_expired, protocol_error}`, `TimeResult{recv_mono_ns(d1), send_mono_ns(d2), wall_ns, clock_id}`, `ViewerPresence`, `MetricsReport{kind, channel?, p50, p95, p99, n, skew_err_ms, rendered_through_seq?, detail}`.
- 새 모듈 `core_events.rs`: `LatchPayload{on, by, origin, acked, missing, jobs_stopped, approvals_cancelled, uncancellable[{channel,count}], outputs_held[{domain,channel,description}], persisted}`, `AgentCursorPayload`, `ActivityPayload{…, range?, waiting?}`, `ApprovalRequestedPayload`, `ApprovalResolvedPayload{outcome}`, `ApprovalRespond{approval_id, decision, scope?, presence?}`, `SafeStateItem`, `SafeStateRun{ids}`, `SafeStateRunResult{items[{id, outcome, finally_blocked, reason?, request_id?}]}`, **`is_priority_kind(kind, payload)`**.
- `rpc.rs`: `SubFilter.ext: Option<Value>`(F2 푸시다운 자리), `Subscribe.stream_epoch`(재연결), `EventsBatch.hub_publish_mono_ns`(입력 없으면 0)·`record_health?`. lib.rs에 모듈 2개 추가.
- `schema::export_all` 16 → 32개(새 타입 16개). fixture 3개 추가(`event_core_latch`, `stream_snapshot_part`, `stream_heartbeat`).
- 시험: workspace(`loom-schema` 제외) **47개 통과**(+2: latch payload·우선 분류, 스트림 타입 JSON/MessagePack 왕복). fmt·clippy 통과.

### A의 결정·계약과 다른 점
1. **우선 분류 규칙**(UI·허브 공용): CORE kind 목록(latch·approval·policy·worker_state·job_state/progress·activity·agent_cursor·mark·epoch_changed·write_recovered·session_closed·env·gap 3종·record.health·ui.focus_request) + `test.*`·`run.*` 전부 + **`payload.write_state`가 있는 모든 쓰기 행**(최종 단계만이 아니라 `requested`부터 — 한 경로에 있어야 순서가 유지됨) + 도메인 `state/error/alert/timeout/late`·`*_state`·`*_error`. 잘못된 write_state도 우선(조용히 버리지 않게). bulk(`uart.data`, `can.frame`)와 `core.heartbeat`는 아님.
2. gap payload는 계약대로 `seq_range{from, to}`(+ `producer_range`, `count: "<n>"|"unknown"`, `cause`). **UI 임시 타입 `{first_seq, last_seq}`는 UI가 맞출 것.**
3. `SnapshotItem`에 계약 01 §1.4의 `kind:"<domain>.snapshot"`을 넣음(지시서 목록에는 없던 필드).
4. Q6 답(구독 상한 초과): `core.subscribe`가 JSON-RPC 오류로 응답, `error.data` = `ErrorBody{code:"E_BUSY", detail:{limit:8}}`(exit 4 묶음).
5. **`crates/loom-schema/tests/drift.rs` 실패**: 커밋된 `schema/v1/`이 없거나 A의 새 타입이 반영되지 않음. 리드가 `cargo run -p loom-schema -- export`로 재생성해야 함(제 범위 밖이라 실행하지 않음).

## B. `loomd --fixture-replay` — **완료**
- 새 crate `crates/loom-gateway`(lib + bin `loomd`). replay는 `src/replay/{mod,hub,conn,scenario}.rs`로 격리(P1 gateway 자리 보존). 의존: workspace의 tokio·axum(ws)·futures-util·tracing·clap·base64, 시험 전용 tungstenite.
- **UI 실행법**(`crates/loom-gateway/README.md`):
  `cargo run -p loom-gateway --bin loomd -- --fixture-replay builtin --port 4790 --scenario all --echo-writes --allow-origin http://localhost:5173` → Vite 프록시 `/rpc → ws://localhost:4790/rpc`. 시작 시 stderr에 "NO AUTH, 127.0.0.1 only" 경고와 URL 출력.
- 동작: `core.hello`(viewer, rights 기본 operate, env sim, stream_epoch 1) → `core.subscribe`(응답 → `core.snapshot_part`(200건씩, 첫 part에 `core.snapshot` 상태) → `core.snapshot_end{stream_seq:H}` → live 배치), `after_seq`+`stream_epoch`가 링 안이면 resume. 우선 이벤트는 `core.events{sub:"control"}`(연결마다 자동). 적응형 배치(조용한 뒤 즉시, ≤ `max_batch_ms`·200건·60 KiB 페이로드). heartbeat 1 s(`sub_stats` 포함). `core.time`(d1/d2), `core.viewer.presence`/`core.metrics.report`(로그만), `core.unsubscribe`, `core.stop`/`core.stop.release`(→ `core.latch`), `core.safe_state.list/run`, `--echo-writes`(uart.send·uart.input·can.send·uds.request → 4행 후 응답, 나머지 domain 메서드 `E_NOT_IMPLEMENTED`), watch 권한은 쓰기·STOP `E_DENIED`.
- 시나리오: normal(UART 5줄/s, CAN 300 fps, UDS 2 s마다 요청/응답 — 5번째마다 `outcome_unknown`), highload(CAN 10k fps), gaps(3 s마다 display/record/source.gap, record·source는 `count:"unknown"`), late(1,000건 선발행), saturate(1 s 뒤부터 5 s마다 우선 이벤트 300건 한 턴 → `core.closed{slow_consumer}`), all, 그리고 `--fixture-replay <dir>`(event_*.json 반복, seq·시각 새로 부여).
- C2 상수(`hub.rs`): 제어 큐 256(넘치면 연결 종료), 구독 큐 8,192건(넘치면 오래된 것 드롭 + **그 연결에만** 실제 seq를 가진 `display.gap{cause:"slow_client"}` + 배치 `display_gap`), 링 20,000, 연결당 구독 8·전체 32(초과 `E_BUSY`), 연결 64. Origin/Host allowlist(`localhost:<port>`, `127.0.0.1:<port>`, `--allow-origin`), 위반 403. JSON-RPC: 파싱 −32700, batch 배열·64 KiB 초과·hello 전 호출 −32600, 없는 메서드 −32601, 파라미터 −32602, Loom 오류는 −32000 + `data`=ErrorBody.
- 시험 `tests/replay.rs` **8개 통과(3.2 s)**: hello→subscribe→snapshot_end→live(seq > H)·control·heartbeat·time / echo 4행이 응답보다 먼저 + latch + safe_state / watch 거절 / saturate closed → 재연결 snapshot / late backlog ≥ 1,000 / 프로토콜 오류 5종 + 구독 상한 E_BUSY / Origin 거절·허용 / highload에서 3 s 안 읽는 클라이언트가 `slow_client` gap을 받음.
- workspace(`loom-schema` 제외) 55개 통과, fmt·clippy 통과.

### B의 계약과 다른 점·주의
1. replay는 **인증 없음**(쿠키·토큰 미검사). 모든 연결 = viewer.
2. `display.gap`이 연결 하나에만 전달되므로 다른 연결은 그 seq를 못 본다(필터로 빠진 seq와 같음 — 계약상 손실 아님).
3. 쓰기 행은 우선 분류(A-1) 때문에 **control 구독으로** 온다. 데이터 구독에는 bulk만 온다(UI는 kind로 라우팅).
4. `lag > 2 s → jump_to_live`, snapshot 중 live 1 MiB/2 s 상한, binary encoding은 replay에 없음(P1/M2).
5. 오리진 정본 `http://localhost:<port>` 리디렉트·정적 `/ui/*`·보안 헤더는 P1(UI 요청 문서의 "정적 자산" 절).
## C. `loom-testkit` — **완료**
- `FakeWorker<W: Worker>`: 인프로세스 구동(`SimClock`, 수집 sink, current-thread tokio로 콜백 실행). `call/write/job/input/latch`, `drain()`(lane 포함), `drain_published()`(시험용 seq 부여).
- `RegistryProbe::run/assert_ok`: `register` 실행 → `ProbeReport{commands, write_commands, event_kinds, workers, errors, warnings}`. 오류: API_VERSION 불일치·명령 0개·help 없음·쓰기 명령인데 hooks/risk table 없음. 경고: 쓰기 메서드의 risk 행 없음·이벤트 kind 없음·포인터 없는 kind.
- `HookHarness` + `OwnedWriteRequest`(빌더): 4개 훅 호출 + 시간 측정(`Timed{value, elapsed, slow}`, 1 ms 초과 `slow`, **10 ms 초과는 panic**).
- `fixtures`: `loom-proto/tests/fixtures`를 **`include_str!`로 임베드**(복사 없음, 정본 하나). `ALL`, `json(name)`, `events(name)`.
- `asserts`: `check_/assert_write_rows`(같은 request_id·write_state 필수·seq 증가·단계 전진·종단 뒤 행 금지), `check_/assert_publish_before_reply(&[TraceItem])`(응답 전 행 ≥ 1, 응답 뒤 같은 요청 행 금지), `check_/assert_no_gap_lies`(GapData 파싱·cause·범위 순서·exact count ≤ 범위 크기·display.gap 범위 필수·seq 증가).
- 제품 허브·정책은 재현하지 않음(문서 주석 명시). 시험 **4개 통과**(fixture 로드, 각 assert의 정상·위반 사례).

## D. `loom-echo` — **완료**
- `gateway`: `Echo: DomainModule` — `echo.send`(쓰기 R1 → `ctx.worker("echo").write`), `echo.read`(R0, 저널 bounded scan), 이벤트 kind `echo.data`(포인터 `/payload/text`)·`echo.write`(`/payload/write_state`), risk table, `EchoHooks`(classify R1, presend: generation 0이면 `EpochChanged`), `LastLines` 리듀서(최근 100줄), worker spec(Shared, 16/32 MiB), capability, help.
- `worker`: `EchoWorker: Worker` — latch 중 agent/script 쓰기 `failed{E_STOPPED}`, 아니면 `echo.write{write_state:"sent"}`(priority) 후 같은 텍스트를 `echo.data` rx로 되돌림, `on_input`도 rx, `echo.status` 호출.
- `cli`: `EchoCli` — `loom echo send <text>`, `loom echo read --tail N(1–1000) [--since CURSOR]`.
- `DOMAIN:` 주석으로 바꿀 곳 표시. 분량 396줄(rustfmt 후 — 지시 50–300줄보다 조금 김, 세 조각 + 주석).
- 시험(testkit 사용) **5개 통과**: RegistryProbe 경고 0, HookHarness(R1·1 ms 이내·presend 거절), FakeWorker send→rx 왕복 + write 행 + gap 정직성, STOP latch 중 agent 거절·user input 통과, CLI 조각(`--tail 5000` exit 2, `--since -5s` 허용).

## 최종 점검
- `cargo fmt --all --check` ✓, `cargo clippy --workspace --all-targets -- -D warnings` ✓, `RUSTDOCFLAGS=-D warnings cargo doc --workspace` ✓.
- `cargo test --workspace`: **65 통과 / 1 실패, 전체 8초.** 실패는 `loom-schema/tests/drift.rs`(리드 crate) 하나 — A로 proto 타입이 늘어서 `schema/v1/`을 다시 내보내야 함(`cargo run -p loom-schema -- export`). 제 crate 시험: proto 33, domain-api 3, worker-sdk 6, cli-kit 5, gateway 8, testkit 4, echo 5 = 64.

## 리드에게 필요한 것
1. `cargo run -p loom-schema -- export`로 `schema/v1/` 재생성(drift 시험 통과).
2. 루트 `[workspace.dependencies]`에 `loom-testkit`, `loom-gateway`, `loom-echo` path 항목 추가. 지금 `loom-echo`는 dev-dep `loom-testkit = { path = "../loom-testkit" }`로 씀(TODO 주석).
3. 새 crate 4개(`loom-gateway`, `loom-testkit`, `loom-echo`)와 proto 변경 커밋 → `core-p0-d5` 태그.

## UI 팀이 알아야 할 것
- 실행: `cargo run -p loom-gateway --bin loomd -- --fixture-replay builtin --port 4790 --scenario all --echo-writes --allow-origin http://localhost:5173`(`crates/loom-gateway/README.md`).
- 우선 이벤트(쓰기 행 **모든 단계**, latch, gap, 상태, test/run)는 `core.events{sub:"control"}`로만 온다. 데이터 구독에는 bulk만.
- gap payload는 `seq_range{from,to}` / `producer_range` / `count:"<n>"|"unknown"` / `cause` — UI 임시 `{first_seq,last_seq}`를 맞춰 주세요.
- 배치에 `hub_publish_mono_ns`·`record_health`, `core.time`에 `recv_mono_ns`(d1)·`send_mono_ns`(d2).
- 구독 상한 초과 = JSON-RPC 오류 −32000, `data.code = "E_BUSY"`, `data.detail = {per_connection: 8, total: 32}`.
