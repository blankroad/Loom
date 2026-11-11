docs/plan/02-core-principle.md 준수
# TEST 실제 CORE session runtime hooks · 증거 수용

- 기능 / 상태 / 담당: TEST session runtime hooks(source fence·recording health·STOP·session write gate·finally) / **READY_TO_INTEGRATE** / Herdr Claude opus 포어그라운드 직접 수행, 자체 리뷰만(독립 리뷰 없음). 하위·내장 agent 없음.
- 기준 main `6d41c24` + 실제 작업 트리(메인·UDS 병렬 미커밋 변경 포함). index/commit/push/reset 없음. Cargo manifest/lock 변경 없음(요청할 것도 없음).
- 빌드: `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=$HOME/.cache/loom-main-target`(공유 target, 신규 target 생성 없음).
- 보존 확인: `loom-domain-api/src/hooks.rs`(on_latch) 미수정, `daemon::stop`의 `hooks.on_latch(on)` 루프 그대로(TEST 호출은 그 **뒤**에 추가), 메인의 RSS(`memory.rs`, `rss_default`, host `state()`, status `process_rss_bytes`)·`hello_capabilities()`·`conn.rs` 미수정. UDS/CAN/PSU/UI 소스 미수정.

## 결과 요약

`NotWiredHooks`를 실제 CORE runtime(`TestRuntime`)으로 교체했다. 실제 daemon(in-process sim) + journal에서 **실제 장치 증거로 PASS·`passed`/exit 0**까지 확인했고, 사전 증거·이전 attempt·지어낸 seq·다른 stream epoch·증거 없음·STOP·기록 실패·IPC unflushed는 모두 fail-closed로 확인했다. `finally`는 등록 safe-state(UDS `uds.session default`, R1)를 정상 write pipeline으로 실행했다.

핵심 설계 결정: **창 소속은 per-producer source fence(커밋된 `producer_epoch/source_seq`)로만 판정**한다. 조사 중 발견: hub clock(`loomd-<pid>`), in-process worker clock(`proc-<pid>`, 원점 다름), IPC worker clock(프로세스별)이 서로 다른 시간축이라 기존 verifier의 `captured_mono_ns` vs `ts.mono_ns` 비교는 근거가 없었다. 시간 조건(anchor/within)은 같은 `clock_id` 안에서만 비교하고, 다르면 `uncertain`.

## 변경 계약 (통합 시 확인)

### loom-test (공개 API)
| 항목 | 변경 |
|---|---|
| `Fence` | `clock_id: String`, `unflushed_domains: Vec<String>` 추가(serde default, 빈 값 생략). 의미 문서화: producer별 최신 epoch + 커밋된 최고 source_seq, 없는 producer = 그 시점 커밋 없음 |
| `SessionHooks::fence_async(session, step, attempt)` | 신규 async hook(기본 = sync `fence`). 엔진은 이것만 호출하며 **state lock 밖에서** 호출. `Err` = 전이 거부(fail closed), `Ok(None)` = 미연결 |
| `RecordingHealth::Degraded` | 신규(degraded/disabled). PASS/done 거부 `E_RECORDING_FAILED` |
| `MAX_FENCE_SOURCES=128`, `MAX_FENCE_DOMAINS=64` | fence 상한, 초과 시 `E_PARTIAL`(절단 없음). 128 = attempt당 2 fence × 500 step이 `CAP_SESSION_BASE_BYTES` 안 |
| 엔진 전이 | `test.step/step.begin/step.end/retry/skip/end/plan.amend`는 fence를 lock 전에 1회 캡처 → 같은 경계로 이전 attempt 종료·새 attempt 시작. 늦게 캡처된(더 오래된) fence로 닫으려는 경합은 `E_LOCKED` |
| 열린 attempt 검증 | verify 직전 check fence를 캡처해 verifier 사본에만 `end`로 사용(세션 창은 열린 채, 커밋 비교는 원본) |
| 시간축 | `host_started_ns/host_ended_ns/CheckCtx.now_ns` = `CallCtx::now_mono_ns()`(hub clock). 모듈 자체 Instant 원점 제거 |
| verifier(`wire.rs`) | 소속 = `(start, end]` per producer + 같은 stream epoch. 시간 조건 = 같은 clock_id만. unflushed 도메인 → `uncertain`(`fence_unflushed(<d>)`). 스캔 도메인 = `[d, source, record, display]` 명시 후 channel 사후 필터(이전엔 channel만 주면 gateway가 `test` 도메인으로 기본값 → 항상 빈 창이던 결함). `display.gap`도 gap. end fence 없으면 coverage incomplete. REF는 도메인까지 일치해야 함. 재조회가 불완전한데 REF를 못 찾으면 `uncertain` `ref_unchecked_incomplete_scan`(지어낸 REF로 단정하지 않음, 완전하면 기존대로 `ref_not_in_journal`). **stimulus(`done`) 인용 write도 attempt 창 안이어야 함**(이전엔 창 검사 없음) |

### loom-gateway
| 파일 | 변경 |
|---|---|
| 신규 `src/daemon/fence.rs` | `Commits`(hub observer, producer별 최신 epoch·최고 source_seq, 4096 producer 상한, 초과 시 fence 거부), `SourceFence`, `HubClockSource`(in-process worker용 hub clock). `daemon.rs`의 `pub mod fence;`로 선언 — **`lib.rs` 변경 없음** |
| `src/hub/mod.rs` (리뷰 수정) | ring별 축출 watermark `(seq, publish instant)` 추적, `RangeResult.evicted_through/evicted_at` = 필터가 선택 가능한 ring(domain/channel)의 최댓값 |
| `src/journal.rs`, `src/daemon/core_rpc.rs` (리뷰 수정) | `JournalRead.unreadable`: mapping은 있으나 producer journal 디렉터리가 없는 stream 구간(조용한 skip 제거). `core.events.query` journal 결과에 비어 있지 않을 때 `unreadable:[{from,to,cause:"producer_not_recorded"}]` |
| `Services::scan` (리뷰 수정) | seq cursor가 해당 필터 ring의 축출 seq 이하이면 durable journal로 재조회(기존 전역 oldest 비교 보완). 시간/Last/Mark cursor 범위가 축출과 겹치면 `complete:false` + gap `ring_evicted`. journal 경로의 unreadable 구간은 gap `producer_not_recorded` + `complete:false` |
| `daemon.rs` | observer에 commit 추적, 주입 clock 없으면 in-process worker clock = hub clock, `Daemon::source_fence()`(clock → hub barrier ≤1 s → 커밋 표 + IPC 도메인 unflushed), `domain_ctx()`, STOP 시 TEST 세션 STOPPED(메인 scaffold 뒤), `write_inner`에 session-bound gate(agent/script, `detail.test_session_gate:true`), TEST 로드 시 `test.step.begin/end`를 legacy marker 대신 TEST 엔진으로 라우팅 |
| `domains.rs` | `Loaded.test: Option<TestWiring>`(feature test), TEST는 `test_wiring()`으로 생성·등록 |
| `test_services.rs` | `TestWiring`, `TestRuntime`(SessionHooks): fence/recording health/STOP latch/approval(명시 NotImplemented)/finally dispatcher. 미바인딩 시 fence 없음·health Unknown·**STOP 가정**. `test_module()` → `test_wiring()` |

## Hook 상태

| Hook | 구현 | 근거 |
|---|---|---|
| source fence | ✅ hub barrier 후 커밋 표 | gateway·in-process producer는 정확: SDK `Emitter::push/push_reserved`가 producer order lock을 **`sink.try_push`까지** 유지하고 in-process `HubSink`는 hub 큐에 동기 enqueue → producer별 커밋 순서 = emit 순서, barrier 이전 emit은 모두 커밋. (`ts`는 도메인이 push 전에 찍으므로 fence 직전 캡처·직후 push된 레코드는 source 순서상 다음 창에 속한다 — 정의상 정상.) IPC는 data lane flush 수단이 없어 `unflushed` |
| recording health | ✅ daemon `record_health()` | ok→Healthy, failed→Failed, degraded/disabled→Degraded |
| STOP | ✅ memory latch 즉시 반영 + 세션 STOPPED 이벤트 | release는 재개·cleanup 없음 |
| session write gate | ✅ CORE write pipeline | paused/승인 전/ending 세션의 client 쓰기 거부, viewer 유지 |
| finally | ✅ `Daemon::call` 정상 write pipeline(STOP·record·classify·policy·presend) | role Script, client `<owner>#test-finally`, `RegistryCommands::rpc_params`로 plan 검사와 동일 params, deadline 제한, 실패 시 `outcome:unknown` + 세션 paused 유지(엔진 기존 정책) |
| approval | ❌ 명시 `E_NOT_IMPLEMENTED` | loomd는 F1 자동, 승인 큐 없음. 세션은 `ApprovalMode::Auto`. 정책 완화 없음 |

## 검증 증거

로그: `/private/tmp/claude-501/-Users-ctmctm-workspace-Loom/48746cca-f87b-4f53-983d-6fb2e2fb36a5/scratchpad/` (`final-test.log`, `final-clippy.log`, `final-clippy-echo.log`, `final-check-test.log`, `e2e.log`)

| 명령 | 결과 |
|---|---|
| `cargo test -p loom-gateway -p loom-test --offline` (1차) | exit 0, 115 passed / 0 failed |
| 리뷰 수정 후 `cargo test -p loom-gateway -p loom-test --offline --no-fail-fast` | **118 passed / 1 failed**. 실패 1 = 메인 작업 중 신규 `tests/tc1_live_smoke.rs`(untracked): `uart.open` → `E_NOT_OPEN "/dev/ttys008: Not a typewriter"`(PTY 환경, TEST 호출 이전 단계) — 본 변경과 무관. 나머지: gateway lib 17(**ring 축출·unreadable 회귀 2 신규**), **test_runtime_smoke 6**(absence/축출 1 신규), preview_policy 2, 기존 전부, loom-test wire 17 등. 로그 `r2-test.log` |
| `cargo clippy -p loom-gateway -p loom-test --all-targets -- -D warnings` | PASS (`r2-clippy.log`; 한 번은 타 팀 작업 중 `loom-can/src/signals.rs:674` private `from_str`로 실패 → 재실행 PASS) |
| `cargo clippy -p loom-gateway --no-default-features --features echo -- -D warnings` | PASS |
| `cargo check -p loom-gateway --no-default-features --features test` | PASS (기존 `inproc` 미사용 매크로 경고 1, 무관) |
| `cargo fmt -p loom-gateway -p loom-test --check` | 내 파일 PASS. 남은 diff 1건은 메인 소유 `daemon.rs` `hello_capabilities()`(손대지 않음) |
| `git diff --check` | PASS |

### 실제 daemon 종단(`tests/test_runtime_smoke.rs`, 격리 HOME, WebSocket agent+viewer)

| 시나리오 | 실제 결과 |
|---|---|
| in-process sim, PSU on(S1) → `done` 인용 = 최종 write row | `effective: done`, `step_started.fence_available:true` |
| S2 증거 없음 | `E_EVIDENCE_MISSING` |
| S2에 세션 전 실제 `psu.meas` | `E_OUT_OF_WINDOW`(check `cause: out_of_window`, candidates 0) |
| S2에 S1 write row(이전 attempt) | `E_OUT_OF_WINDOW` |
| 지어낸 seq / 이전 stream epoch REF | `E_EVIDENCE_MISMATCH` ×2 |
| S2에 새 device `psu.meas` | `effective: pass`, check `verified`·`witness: device`·`complete: true` → `test.end` **`passed`, exit 0** |
| agent `test.pause` 후 agent `psu.out` | `E_SESSION_PAUSED`, `test_session_gate:true`; viewer `psu.out` 성공 |
| agent `core.stop` | viewer가 `test.session_state STOPPED` 라이브 수신, status STOPPED, verdict 거부, agent 쓰기 `E_STOPPED`; viewer release 후에도 STOPPED, `test.end` → `incomplete`, exit 6 |
| journal 장애 주입 → `record_health=failed` 중 유효 REF PASS | `E_RECORDING_FAILED` |
| 기본 IPC 배치, PSU done/PASS | 거부, check `cause: fence_unflushed(psu)`, status `uncertain` |
| absence(`absent`) 깨끗한 창 vs 같은 ring(psu/pb)에 ~5.6 MiB 유입으로 실제 4 MiB 채널 ring 축출 | 깨끗한 창: `pass`, witness `complete_absence`. 축출: PASS 거부(`E_EVIDENCE_MISSING`), check `uncertain`, `complete:false`, problems `journal_gap(1)`·`journal_incomplete`, 인용 REF는 `ref_unchecked_incomplete_scan` |
| `Services::scan` 단위(실제 daemon): 축출된 seq를 seq cursor로 | durable journal에서 그 이벤트 반환, complete. 시간 cursor는 gap `ring_evicted`, 무관 채널 ring은 complete 유지. producer 디렉터리 삭제 시 journal read `unreadable=[(seq,seq)]`, scan gap `producer_not_recorded`, complete false |
| finally `uds.session {default, recovery:true}`(R1 safe state) | `test.session_ended.finally = {complete:true, actions:[{method:"uds.session", request_id:"w2", state:"confirmed"}]}`, 해당 write row `origin: script`(viewer 라이브) |

### loom-test 반례(`tests/wire.rs`, 기존 9 유지 + 신규 8)
start fence 이전 커밋 레코드(시간상 within 안이어도) → OUT_OF_WINDOW · end fence 이후 늦은 응답 → OUT_OF_WINDOW · 이전 epoch REF → `ref_not_in_journal` · attempt 이전 confirmed write로 `done` → OUT_OF_WINDOW(창 안 write는 수용) · unflushed 도메인 → uncertain · 다른 clock_id 증거 → `clock_domain_mismatch` uncertain · `display.gap` → complete absence 거부 · 오래된 fence로 전이 → `E_LOCKED` 후 상태 불변.

## 완료 기준 판정

| 기준 | 판정 |
|---|---|
| 실제 daemon+sim+journal에서 연결된 hook 기능 검증 | ✅ (in-process 배치) |
| 기록 실패 / STOP / 옛 epoch·attempt / 잘못된 REF / 증거 없는 PASS fail-closed | ✅ 위 표 |
| 실제 유효 증거는 PASS 가능 | ✅ **Inproc 배치**(실제 도메인 backend·journal, worker in-process)와 gateway producer(UDS 등). ❌ **IPC 배치의 worker 도메인은 불가**(fail-closed `uncertain`) — 아래 "P1 판정" |
| 위험/승인 정책 완화 없음 | ✅ classify·policy·STOP·record gate 그대로, approval은 명시 미구현 |
| fixture 오류/finally R3 충돌 변경 요청 | ✅ 아래 요청 4·5 |

## P1 판정과 배치 제약 (메인 리뷰 반영)

- **이 반환은 TEST runtime hook 연결의 READY_TO_INTEGRATE이며 P1 완료가 아니다.** 기본 IPC 배치에서 실제 TC-1 PASS는 불가능하다: UART/CAN/ETH/PSU worker 도메인은 fence에 `unflushed`로 기록되고 TEST는 그 기대값을 `uncertain`(`fence_unflushed(<domain>)`)으로 판정한다. 이것은 의도된 fail-closed이며 유지한다(메인 결정: IPC flush는 별도 범위로 보류, source_fence 수정 없음).
- TC-1 PASS 경로는 **Inproc 배치**(`[workers] mode = "inproc"` 또는 `Config.workers = Inproc`)에서 실제 도메인 backend·실제 journal로 검증한다. 그 경우 worker `ts`도 gateway clock(`loomd-<pid>`)을 쓰므로 anchor/within 비교가 성립한다.
- **기존 IPC 개별 도메인 smoke와 구별**: `tests/ipc.rs`, `r1.rs`, `can_eth_smoke.rs`, `uds_*_smoke.rs` 등은 IPC worker의 RPC/이벤트/STOP/재시작 경로를 검증하며 TEST 판정(PASS)을 주장하지 않는다. 이번 `test_runtime_smoke.rs`의 IPC 시험은 "IPC에서는 PASS가 거부된다"는 fail-closed만 검증한다.

### 후속 IPC fence 설계용 참고 제약 (현재 코드 확인)

- **시간축**: IPC worker `ts.mono_ns`는 worker 프로세스의 `SystemClock`(원점 = 프로세스 시작 `Instant`, `clock_id = proc-<pid>`)이라 hub clock(`loomd-<pid>`, 원점 = hub 시작)과 비교 불가. 현 `Fence`는 시계 하나(`clock_id`, `captured_mono_ns`)만 담는다 → worker 시계 대응을 넣으려면 `Fence`에 clock별 캡처 시각(예: `clocks: [{clock_id, mono_ns, uncertainty_ns}]`, 왕복 측정)을 추가하는 TEST 계약 확장이 필요(TEST 소유, 요청 시 반영). host 공통 monotonic(CLOCK_MONOTONIC/mach)로 SDK가 바뀌면 불필요.
- **seq 순서**: SDK `Emitter`는 producer당 하나의 seq 카운터를 lock 아래에서 할당·journal push(journal은 엄격 증가). 그 뒤 ctl(priority)과 data(mirror, 10 ms batch, credit) lane이 갈라져 **hub 도착 순서는 seq 단조가 아니다**. 현재 `Commits`는 producer별 **최댓값**만 추적한다 → IPC에 쓰려면 "contiguous-through"(모든 seq ≤ S가 커밋됐거나 gap으로 보고됨) 추적이 필요(gateway/TEST 소유 영역, 해당 proto가 오면 반영).
- **journal 조회 한계**: TEST 재조회(`Services::scan`)는 live 구간을 hub ring(전체 32 MiB, 채널당 4 MiB, 채널별 축출 미감지)에서, 오래된/다른 epoch seq cursor는 `Journal::read_range`(hub mapping log의 stream seq ↔ producer source range로 producer 디렉터리 스캔, 2 s·scan_bytes 한도)에서 읽는다. 따라서 **hub에 커밋되지 않은 레코드(mirror drop → `display.gap{producer_range}`)는 worker journal에는 있어도 stream seq·REF가 없어 TEST가 읽을 수 없다.** worker journal flush 증거는 내구성을 증명할 뿐 재조회 가능성을 증명하지 않는다. 또 `read_range`는 producer 디렉터리가 없으면 조용히 건너뛴다(손실 표시 없음).

## 변경 요청 (타 소유)

1. **(메인 결정으로 보류) CORE proto/SDK/worker-host — worker flush fence**: IPC data lane(screen mirror, 10 ms batch)은 flush 수단이 없다. 제안: ctl `Fence{fence_id}` → SDK가 mirror를 해당 시점 source_seq까지 data lane으로 보낸 뒤 `FenceAck{fence_id, through: SourceRef}` 응답, host는 data reader가 `through`까지 sink에 push(또는 gap 보고)한 뒤 resolve. 이후 gateway가 IPC 도메인을 `unflushed`에서 뺀다. 그 전까지 기본(IPC) 배치에서 UART/CAN/ETH/PSU 기대값은 PASS 불가.
2. **(메인 진행 예정) CORE/UDS — gateway 도메인 RX origin**: `Services::publish`가 caller origin을 써서 `uds.rx`가 `origin: agent`였다. 메인이 `dir == Rx`이면 device로 바꾸기로 함(UDS는 실제 장치 응답만 RX). 반영 후 UDS 증거는 device witness가 된다.
3. **UART/UDS evidence verifier 부재**: `uart.text_contains`, `uds.dtc_absent`를 등록한 도메인이 없다(CAN/ETH/PSU만 등록). TC-1 S2/S4 그대로는 `no domain verifier registered` → PASS 불가. 각 도메인 소유.
4. **TC-1 fixture(loom-test-format 소유)**: 이전 보고 그대로 — `can.sig.wait-for`→`can.sig.wait_for{expr,within_ms}`, `uart.expect{channel,patterns}`, `psu.out{ch}`, `uds.dtc.clear` 미등록.
5. **finally R3 충돌(format 또는 PSU)**: `psu.out off`=R3 safe state, 포맷은 finally에 R0–R1 요구. R1 safe state(UDS)는 이번에 정상 실행 확인. 결정 없이 위험을 낮추지 않았다.
6. **in-process clock 통일 확인 요청(메인)**: 주입 clock이 없으면 in-process worker `ts.clock_id`가 `proc-<pid>` → `loomd-<pid>`(hub clock)로 바뀐다. 기존 테스트 영향 없음 확인. IPC worker clock은 여전히 별도(P0 한계) → IPC 증거의 anchor/within은 어차피 uncertain.

## 잔여 위험 · 후속 (이번 범위 밖, 기능 확대 안 함)

- ~~hub ring 채널 축출 미감지~~ → **리뷰 수정 완료**(위 hub/scan/journal 행). 남은 한계: 시간 cursor 재조회는 축출 구간을 journal로 대신 읽지 않고 incomplete로 표시한다(시각→seq 색인 없음). absence/measure는 `uncertain`, 긍정(exists)은 남은 ring으로 판정 가능. TEST 재조회를 seq cursor로 바꾸면 journal로 완전 재조회가 가능하나 fence에 stream 위치가 없어 이번 범위(기능 확대 금지) 밖.
- ~~in-process seq/push 경합(서브 µs)~~ → **정정**: `Emitter`가 producer order lock을 `sink.try_push`까지 유지하므로 경합 없음(위 Hook 표).
- `Commits`는 죽은 producer id를 잊지 않는다(4096 상한). 장기 실행에서 producer가 128을 넘으면 TEST 전이가 `E_PARTIAL`로 막힌다(fail closed).
- fence bytes는 엔진 byte budget에 별도 과금하지 않는다(상한 128으로 base charge 안에 둠).
- plan `env: sim` vs daemon env(`bench`) 비교 없음(기존 지적 유지).
- `test.step.begin/end`는 TEST 로드 시 legacy marker 대신 엔진으로 간다(동작 변경; 기존 테스트·CLI·UI 사용처 없음 확인).
- 세션 client의 일반 쓰기에 `test_session/test_step` 자동 태깅(TC-C1)은 미구현.

## 자체 리뷰 메모

- 거짓 PASS 경로 점검: fence 없음/미바인딩/hub stall/producer 초과/unflushed/clock 불일치/gap/end fence 없음 → 모두 uncertain 또는 거부. stream seq를 fence로 쓰지 않음.
- lock 순서: 엔진 state lock 안에서 daemon lock을 잡지 않음(STOP 경로는 latch_lock → TEST lock → hub 채널 send만).
- 경계: barrier 1 s, finally는 `test.end` deadline 및 액션별 `max_wait`, fence 128 refs, producer 4096, ring 축출 표는 ring key 수만큼.

## 소유 반환 (메인 요청)

리뷰 수정을 끝내고 `crates/loom-test`의 `lib.rs`·`engine.rs`·`wire.rs`(및 `seams.rs`·`model.rs`·tests) 소유를 메인에 반환한다. 이후 이 파일들은 수정하지 않는다. runtime/cli feature 분리용 참고(현재 소스 기준):

- tokio 사용: `engine.rs`(`tokio::sync::{Notify, Semaphore, OwnedSemaphorePermit}`, `task::spawn_blocking` 3곳, `select!`+`time::sleep` in `wait`, `#[tokio::test]` 1), `wire.rs`(`block()`: `runtime::Handle::try_current`/`Builder::new_current_thread`). → runtime cfg 대상.
- `seams.rs`: tokio 없음(`BoxFuture`는 `loom-domain-api`). 단 `VerifierEngine`/`SessionHooks`가 `CallCtx`·`EvalBudget`에 의존 → runtime 쪽에 두는 편이 자연스럽다.
- `model.rs`: 순수(serde). 이번에 `Fence.clock_id`, `Fence.unflushed_domains`(serde default) 추가.
- `lib.rs`: 상수만 추가(`MAX_FENCE_SOURCES=128`, `MAX_FENCE_DOMAINS=64`), 그 외 모듈 선언 변경 없음.
- gateway 쪽(`test_services.rs`의 `TestRuntime`/`TestWiring`)은 `loom_test::{TestModule, SessionHooks, Fence, RecordingHealth, ApprovalStatus, PredicateEngine, SessionConfig}`를 쓴다 → gateway는 runtime feature 필요.

## 부록: `wire.rs` 도메인 EvidenceVerifier 호출 계약 (UART/UDS verifier 구현용, 현재 코드 기준)

대상: `PredicateEngine::one` → `domain_check` (`crates/loom-test/src/wire.rs`). 공통 술어(`exists`, `absent`, `count`, `order`, `within_ms`, `hold` 등 `loom_test_predicate::pred::is_common`)는 도메인 verifier로 가지 않는다.

1. **선택**: 술어 이름 = plan `pred`의 `op` 문자열(또는 키 하나짜리 `{<name>: …}`). `PredicateEngine::from_registries`가 모은 verifier 중 `predicates()`에 그 이름이 있는 **첫 번째**가 호출된다. 없으면 check `error`, cause `no domain verifier registered for "<name>"`.
2. **`Predicate` 인자**: format은 `pred`를 정규화하지 않고 그대로 넘긴다.
   - `name` = 술어 이름, `expect_id` = `"<step_id>#<expect index>"`.
   - `args` = plan `pred` 객체에서 `op` 키만 제거 + `filter` 추가 + (plan filter에 channel이 있고 `ch`가 없으면) `ch` 추가.
   - `filter` = `{"domain", "kinds"(배열, 빈 배열 가능), "channel"?, "request_id"?, "origins"?(비어 있으면 생략)}` — plan 값 그대로(문자열).
   - **`op` 형식 권장**: `{op: uart.text_contains, pattern: "login:"}` → `args = {"pattern":"login:","filter":{…},"ch":"dut"}`. 키 형식 `{uart.text_contains: {pattern: …}}`이면 `args = {"uart.text_contains": {"pattern": …}, "filter": …, "ch": …}`로 **중첩된 채** 전달된다(평탄화 없음).
   - `pred` 안의 다른 키(`mask`, `hold_ms`, `max_gap_ms` 등)는 그대로 전달된다.
3. **verifier 호출 전에 TEST가 이미 결정하는 것**(verify 미호출, check `uncertain`):
   - start fence 없음 → `window_unknown: no CORE source fence for this attempt`
   - 도메인이 fence의 `unflushed_domains`에 있음 → `fence_unflushed(<domain>)` (**현재 IPC 배치의 UART 등 worker 도메인은 항상 여기서 끝남**; UDS는 gateway producer라 해당 없음)
   - anchor 지정인데 없음 → `anchor_not_found`; anchor/within 시간과 창 레코드 `ts.clock_id`가 다름 → `clock_domain_mismatch`
   - 창 안에 같은 도메인의 `origin:user` + `payload.write_state` 레코드 → `user_intervention`
4. **`events`(window)**: 스캔은 도메인 `[d, source, record, display]`, 이후 `(domain == d && (plan channel 없음 || record.channel == plan channel)) || gap kind`인 레코드 중 attempt 창(per-producer source fence `(start, end]`, 같은 stream epoch) ∩ 시간 조건(anchor/within, 같은 clock)만, **stream seq 순서**.
   - `filter.kinds`/`filter.origins`/`request_id`로는 **걸러지지 않는다** → verifier가 `args.filter`를 보고 직접 거른다.
   - gap 레코드(`source.gap`/`record.gap`/`display.gap`, domain `source`/`record`/`display`, channel 없을 수 있음)가 섞여 들어온다 → kind로 무시하거나 손실 표시에 쓴다.
5. **`Coverage`**: `window` = 창 레코드의 첫·마지막 stream seq(비면 0..0). `complete = 스캔 완료 && 창 안 gap 없음 && end fence 있음`(열린 attempt는 verify 직전 check fence가 end로 들어가므로 보통 있음). `causes` 문자열: `scan_limit(time|events|bytes)`, `journal_gap(n)`, `journal_incomplete`, `gap_in_window`, `window_open_no_end_fence`. `coverage_need`: `Positive`는 incomplete여도 긍정 증명 가능, `Complete`/`Measure`는 incomplete면 Inconclusive를 돌려야 한다.
6. **`EvalBudget`**: 세션 check 하나의 공유 예산. 소진 시 TEST가 PASS를 inconclusive로 강등.
7. **결과 매핑**:
   - `Match{matched, observed}` → `verified`. `matched` EventRef는 `(seq, epoch)`로 window에서 찾는다 — window에 없는 ref가 하나도 못 찾히면 `mismatch`/`out_of_window`. witness: 찾은 레코드 중 하나라도 `payload.attributed_origin`(있으면 우선) 또는 `origin`이 `device` → `device`; 아니면 `own_tx_only`(**PASS 거부**). `matched` 비어 있으면 `need==Measure` → `measurement`, 아니면 `complete_absence`.
   - `Mismatch{observed, reason}` → `mismatch`, cause = reason.
   - `Inconclusive{reason}` → reason이 `error:`로 시작하면 `error`, 아니면 `uncertain`.
   - `observed`는 `pinned:false`가 추가된 뒤 2048 B 초과 시 `{"truncated":true}`로 대체. cause 4096 B 초과는 미리보기로 절단. `matched_refs` ≤ 64.
   - digest = canonical sha256(verification, args, name, coverage.window, complete, causes).
8. **REF(agent 인용)**: 후보일 뿐. `<domain>:<ch>@e<epoch>:seq:<n>` 파싱 실패/journal에 없음/도메인 불일치 → `mismatch` `ref_not_in_journal`(재조회가 불완전하면 대신 `uncertain` `ref_unchecked_incomplete_scan`); 있으나 창 밖 → `mismatch` `out_of_window`. 판정은 재조회 결과로만. 재조회 gap(`ring_evicted`, `producer_not_recorded` 등)은 coverage cause `journal_gap(n)`/`journal_incomplete`로 나타난다.
9. **UDS 주의**: 현재 `uds.rx`는 caller origin(`agent`)으로 발행돼 device witness가 될 수 없다(변경 요청 2). verifier가 `matched`로 돌려도 `own_tx_only`가 된다.
