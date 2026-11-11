docs/plan/02-core-principle.md 준수

# P0 Day 3–5 — core-opus: proto 보강 · `loomd --fixture-replay` · testkit · echo

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`(기준 `67680e0`, 태그 `core-p0-d2`). 근거: `docs/contract/`(특히 01 §1.3–1.6, 08 §8.4), `docs/plan/core/plan.md` §3.4·§3.9·§9.5, **UI 요청 `/Users/ctmctm/workspace/Loom/docs/impl/requests/ui-core.md`(전부 읽을 것)**.
리드가 루트 `Cargo.toml`에 tokio 1.54.1·axum 0.8.9(ws)·futures-util·tungstenite 0.29·tracing·tracing-subscriber를 추가해 두었다. 리드는 동시에 `crates/loom-schema`(JSON Schema 내보내기)와 계약 문서를 고친다 — **`crates/loom-schema`·`docs/contract`·루트 `Cargo.toml`은 건드리지 말 것**(필요하면 보고).

**순서가 중요하다(UI가 기다림): A → B → C → D.** 각 단계가 끝날 때마다 workspace 전체 fmt·clippy·test를 통과시키고 보고 파일에 진행을 적는다(리드가 중간 커밋할 수 있게).

## A. `loom-proto` 보강 (ui-core.md 1–13, Q1–Q2)
- 리드 판정: **Q1** 셸 전역 제어 구독은 서버가 viewer 연결마다 자동 생성하고 `core.events{sub:"control", …}`로 보낸다(예약 sub id 상수 `CONTROL_SUB = "control"`). **Q2** 다음 타입 추가(`rpc.rs` 또는 새 모듈 `stream.rs` — lib.rs 수정 허용):
  - `SnapshotPart{sub, n: u32, total: u32, items: Vec<SnapshotItem{domain, channel: Option, state: Value}>, events: Vec<PublishedRecord>(backlog, 비면 생략)}`, `SnapshotEnd{sub, stream_seq: U64}`.
  - `Heartbeat{stream_seq_hw, daemon_mono_ns, daemon_wall_ns, record_health, latch: bool, sub_stats: Vec<SubStats{sub, depth, drops, last_flush_mono_ns}>}`, `Closed{reason: CloseReason{SlowConsumer, ProtoMismatch, Shutdown, AuthExpired, …}}`.
  - `EventsBatch`에 `hub_publish_mono_ns`, `record_health: Option` 추가(없으면 추가), `Subscribe`에 `filter.ext: Option<Value>`(도메인 확장 푸시다운 자리, eth F2)와 재연결용 `after_seq` + `stream_epoch`.
  - `TimeResult{recv_mono_ns(d1), send_mono_ns(d2), wall_ns, clock_id}`, `ViewerPresence{visible, rendered_through_seq: Option, oldest_pending_age_ms: Option}`, `MetricsReport{kind: String("ui.render" 등), channel?, p50, p95, p99, n, skew_err_ms, …}`.
  - payload 타입(이벤트 kind `core.*` 전용, `event` 모듈 또는 `core_events.rs`): `LatchPayload{on, by, origin, acked, missing, jobs_stopped, approvals_cancelled, uncancellable, outputs_held, persisted}`, `AgentCursorPayload`, `ActivityPayload{client_id, method, summary, range: Option<SeqRange>, waiting: Option<Value>}`, `ApprovalRequestedPayload`/`ApprovalResolvedPayload`, `ApprovalRespond{approval_id, decision, scope?, presence?}`, `SafeStateItem`·`SafeStateRun{ids}`·`SafeStateRunResult{items:[{id, outcome, finally_blocked?, reason?}]}`.
  - 우선 제어 kind 목록 함수 `is_priority_kind(kind, payload) -> bool`(응답·쓰기 최종 단계·승인·STOP·상태·오류·gap — 계약 01 §1.5). UI·허브가 같은 함수를 쓴다.
  - gap payload는 UI가 `{first_seq, last_seq, …}`로 적었지만 계약은 `seq_range{first,last}` — **계약 유지**, 보고에 "UI가 맞출 것"으로 적기.
  - 새 타입은 `schema::export_all`에 추가하고 fixture(`snapshot_part`, `heartbeat`, `latch`)와 왕복 시험.

## B. `crates/loom-gateway` + bin `loomd` — `--fixture-replay` 모드 (UI P0 1순위)
- 새 crate `crates/loom-gateway`(lib + `[[bin]] name = "loomd"`). P1에서 진짜 gateway가 들어올 자리이므로 replay는 `src/replay/` 모듈로 격리.
- `loomd --fixture-replay <dir|builtin> [--port N] [--speed 1.0] [--scenario normal|highload|gaps|late|saturate|all] [--echo-writes]`. 기본 bind `127.0.0.1`, 오리진 정본 안내는 `http://localhost:<port>`. Origin/Host allowlist(`localhost:<port>`, `127.0.0.1:<port>`, 그리고 `--allow-origin http://localhost:5173` 같은 Vite dev 오리진 옵션).
- WS `/rpc`: `core.hello` → `HelloResult{role:viewer, rights: operate(기본, --rights로 변경), env:"sim", stream_epoch}`(replay 모드는 인증 없음 — 시작 시 stderr에 "fixture replay: no auth, 127.0.0.1 only" 경고). `core.subscribe` → snapshot_part/end → `core.events` 배치(적응형: 조용한 뒤 즉시, 바쁠 때 ≤ max_batch_ms·200건·64 KiB). 연결마다 `sub:"control"` 자동. heartbeat 1 s. `core.time`. `core.viewer.presence`·`core.metrics.report`는 받아서 로그만.
- 시나리오(내장 생성기 + `tests/fixtures` 재사용): **normal**(UART rx 줄·CAN 프레임 수백 fps·UDS 요청/응답 행 갱신), **highload**(CAN 10k fps 합성 — 구독 큐 상한을 실제로 적용해 `display.gap{cause:"slow_client"}`가 정직하게 나와야 함), **gaps**(display/record/source.gap 각 1회 + `count:"unknown"`), **late**(1,000 이벤트를 먼저 흘린 뒤 접속하면 snapshot에 backlog), **saturate**(제어 큐를 일부러 채워 `core.closed{reason:"slow_consumer"}` 후 재연결 → snapshot), `outcome_unknown` 쓰기 행 포함.
- `--echo-writes`: viewer의 도메인 쓰기 RPC(`<domain>.<verb>`, 쓰기로 보이는 것 — 목록은 `uart.send`, `uart.input`, `can.send`, `uds.request` 정도의 고정 표 + 나머지는 `E_NOT_IMPLEMENTED`)를 받으면 `requested → accepted → sent → confirmed` 행을 origin=user, `quality:["simulated"]`로 발행한 **뒤** 응답(publish-before-reply). `core.stop`은 `core.latch` 이벤트 발행, `core.safe_state.list/run`은 고정 항목 1–2개로 응답.
- C2: 구독 큐·우선 큐·연결 수 상한을 계약 값으로 코드에 상수로(무한 큐 금지). JSON-RPC 오류 코드 규격대로, batch 배열 거절, 메시지 64 KiB 상한.
- 시험(tokio, 실제 WS 클라이언트 = tungstenite 또는 tokio-tungstenite가 필요하면 보고 — 우선 axum 시험 클라이언트나 `tokio::net` + tungstenite로): hello→subscribe→snapshot_end→events 수신, echo 쓰기 행 순서 + 응답이 마지막 행 이후, saturate에서 closed 후 재연결 snapshot, 64 KiB 초과·batch 거절. 시험 하나 10 s 이내.
- 실행 안내를 `crates/loom-gateway/README.md`에 5–10줄(UI 팀이 그대로 따라 할 수 있게).

## C. `crates/loom-testkit` (도메인 첫 주 단위 시험용)
- `FakeWorker` 하네스: `loom-worker-sdk::Worker`를 인프로세스로 구동(가짜 `EmitSink`로 이벤트 수집, `on_call/on_write/on_job/on_input/on_latch` 호출 헬퍼, `SimClock`).
- `RegistryProbe`: `DomainModule::register` 호출 → 등록 내용 요약·검사(메서드 이름·이벤트 kind·포인터·worker spec·help 존재). 실패를 읽기 쉬운 메시지로.
- `HookHarness`: fixture `WriteRequest`로 `classify/approval_binding/presend_check/within_pattern` 호출, 호출 시간 측정(1 ms 초과 경고, 10 ms 초과 실패).
- fixture 로더(`loom-proto/tests/fixtures`를 crate에 복사하지 말고 `include_str!` 경로 또는 `fixtures/` 하위로 공유 — 방법은 판단해서 보고)와 assert: `assert_write_rows(&[PublishedRecord])`(같은 request_id 단계 순서), `assert_publish_before_reply(trace)` 형식 검사, `assert_no_gap_lies`(gap count·범위 정합).
- **제품 허브·정책을 재현하지 않는다.**

## D. `crates/loom-echo` (견본 도메인 — 도메인 팀 복사 시작점)
- sim 루프백 장치: gateway 조각(`DomainModule` — 명령 `echo.send`(쓰기, R1), `echo.read`(R0), 이벤트 kind `echo.data`, risk table, hooks, help, reducer), worker 조각(`Worker` 구현 — 보낸 바이트를 rx로 되돌림), cli 조각(`DomainCli` — `loom echo send <text>` / `read --tail N`).
- testkit으로 시험: RegistryProbe 통과, HookHarness, FakeWorker로 send→rx 왕복과 write 행.
- 주석으로 "도메인 팀이 바꿔야 할 곳"을 표시. 50–300줄 정도로 짧게.

## 품질·보고
workspace 전체 `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`(전체 2분 이내). 커밋하지 않는다. 보고: `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p0-d3-opus.md` — 단계별(A–D) 완료 여부·시험 수·UI가 알아야 할 실행법·계약과 다른 점. **A, B가 끝나면 보고 파일에 먼저 적고 계속 진행**(리드가 중간 커밋). 화면에는 경로와 3줄 요약만.
