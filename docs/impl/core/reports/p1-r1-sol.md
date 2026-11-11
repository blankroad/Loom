proto 완료: worker minor 2, Call.conn_id/Input.conn_id·channel·input_seq/InputLease.conn_id(선택), InputLeaseEnd{lease_id:String, conn_id:Option<String>, cause:InputLeaseEndCause::{ConnClosed,Released}}(msg_type=20, ctl·응답 없는 통지), ErrorCode::Limit(E_LIMIT, exit4), ActivityPayload.action·domain·channel(선택), SafeStateKind::{Recover(기본),EmergencyOff}/SafeStateList{items}, canonical::{canonical_json,sha256_hex} 추가. proto 시험 통과·schema/v1 33개 재생성 완료.

# P1 R1 core-sol 보고

R1 배정분을 완료했다. 코드는 `/Users/ctmctm/workspace/Loom/.worktrees/core`의 loom-proto·loom-worker-sdk·loom-worker-host와 schema/v1에서만 변경했다. 보고는 지시서와 기존 보고 위치 지시대로 저장소 루트에 둔다. 커밋·스테이징하지 않았다. 기존 bench 결과와 동시 작업자의 변경을 보존했다.

## opus 연동 위치

- 새 선택 필드를 쓰지 않는 Rust literal도 None을 명시해야 한다. proto 완료 시 알린 범위 밖 위치는 loom-testkit/src/fake_worker.rs의 Call·Input, loom-gateway/src/daemon.rs의 ActivityPayload·SafeStateItem·Input·Call, loom-gateway/src/replay/conn.rs의 SafeStateItem이다. 최종 소스 확인에서는 소유자의 보강이 반영되어 있었다. 이 작업자는 해당 파일을 수정하지 않았다. workspace 전체의 통합 검증은 opus·리드가 수행해야 한다.
- InputLeaseEnd는 새 v1.2 통지이며 소켓 요청 순번을 소비하거나 Reply를 요구하지 않는다. host API `IpcHost::input_lease_end(InputLeaseEnd) -> BoxFuture<Result<(), WorkerError>>`와 `input_lease(InputLease)`를 추가했다. 후자는 기존 순번 Reply를 기다리고 전자는 ctl 큐 수락까지만 확인한다. minor<2의 종료 통지는 NotImplemented로 거절한다. minor<2의 Call/Input/InputLease는 v1.2 필드를 보내지 않는다.
- canonical_json은 serde_json 기본 Map의 키 정렬을 사용하며 중첩·공백·키 순서와 SHA-256 표준 벡터를 시험한다. sha2는 기존 workspace 의존성을 상속한다.

## 1. proto

- 모든 새 선택 필드는 serde default·None 생략을 적용했다. Input.input_seq는 U64이고 JSON에서는 십진 문자열, MessagePack에서는 정수다. 새 lease 종료 cause는 conn_closed/released이며 lease_id·cause는 필수다.
- 기존 오류 variant 순서를 유지하고 ErrorCode::Limit을 마지막에 추가했다. E_LIMIT의 CLI 종료 코드는 4다.
- SafeStateItem.kind는 이전 JSON에서 생략하면 Recover다. 결과를 SafeStateList{items}로 표현하고 recover/emergency_off 문자열을 사용한다.
- canonical_json(&Value)->String, sha256_hex(&[u8])->String을 추가했다. 공백 없는 중첩 JSON·키 정렬·serde_json 기본 Map의 BTreeMap 순서와 표준 SHA-256 빈 입력/abc 벡터를 고정했다. RFC 8785의 별도 숫자 정규화까지 구현한 API는 아니다.
- 새 wire 필드의 생략 호환, full u64 input_seq, 두 종료 cause와 msg_type/lane, 활동·안전 상태 결과·E_LIMIT의 JSON/MessagePack golden을 보강했다. schema/v1은 33개 export 후 drift check를 통과했다.

## 2. SharedWorker·런타임

기존 Worker·run_worker·run_worker_with의 서명과 직렬 실행은 유지한다. Worker에는 기본 no-op의 on_input_lease_end만 추가했다.

새 SharedWorker는 Send+Sync이고 콜백 receiver는 &self다. on_call/on_write/on_job/on_input은 소유한 WorkerCtx와 요청을 받고 BoxFuture<'static, 결과>를 반환한다. on_latch(on)은 BoxFuture<'static, ()>, on_tick(WorkerCtx)·on_input_lease_end(WorkerCtx, InputLeaseEnd)는 기본 no-op의 같은 future 타입이다. 모든 future는 Send다.

실행 API는 run_shared_worker(Arc<W>)->! 및 명시 인자·훅을 받는 run_shared_worker_with(Arc<W>, WorkerArgs, RunHooks)->!다. 기존 IPC·저널·health·credit·orphan 경로를 공유하고 디스패처만 교체한다. 공유 디스패처는 두 Tokio worker thread에서 콜백을 실행한다. 콜백의 동기 부분·poll에서 블로킹하지 않아야 하며, 장비의 블로킹 I/O는 도메인 전용 스레드로 보내야 한다.

| 슬롯·큐 | 상한과 포화 정책 |
| --- | --- |
| 일반 call/write/job | tick 없으면 56개, tick 있으면 55개; 초과 요청은 E_WORKER_BUSY |
| 입력 | 6개 전용 슬롯; 초과 요청은 E_WORKER_BUSY |
| lease 종료 | 1개 전용 슬롯 + 대기 16개; 유실할 수 없는 통지의 큐 포화는 worker 종료 |
| latch | 1개 전용 슬롯; 실행 중 추가 변경은 최신 상태로 합침 |
| tick | 설정 시 1개 전용 슬롯; 이전 tick이 실행 중이면 새 tick 생략 |
| 전체 도메인 콜백 | 최대 64개; 일반 요청이 포화되어도 제어·tick 슬롯을 사용 가능 |
| 응답 큐 | 기존 512개; 포화 시 응답을 조용히 버리지 않고 worker 종료, 닫힌 큐는 종료 경로로 처리 |

Welcome.config.tick_ms가 양의 정수이면 그 주기로 on_tick을 실행한다. 누락·0·표현 불가능한 기간은 tick을 켜지 않는다. 밀린 주기를 한꺼번에 실행하지 않는다. tick·lease 종료는 Reply를 만들지 않는다.

Input의 conn_id/channel/input_seq/call_id를 그대로 콜백에 전달한다. InputLeaseEnd는 요청 순번을 소비하지 않고 콜백으로 전달한다. ctl reader는 기존 직렬 Worker가 긴 콜백을 실행하는 동안에도 멈추지 않도록 종료 통지를 non-blocking 유한 큐에 넣는다. 이 큐도 포화되면 worker를 종료하여 revocation 유실을 숨기지 않는다. IPC에서 latch flag·ack는 콜백보다 먼저 처리한다.

## 3. Emitter

- data(EventDraft), data_with_seq(U64, EventDraft)를 추가했다. data lane에서 dir/origin/purpose/quality/metrics/request_id와 기존 draft의 나머지 메타데이터를 보존한다.
- reserve_seq()->Result<U64, EmitError>, rx_with_seq(U64, &str, Ts, Value)->Result<U64, EmitError>를 추가했다. clone·채널 전체에서 seq를 공유하며 예약·순서 계정은 고정 크기다. 예약하지 않은 seq, 중복·역순 발행, u64 소진을 각각 오류로 반환하고 wrap하지 않는다.
- sink가 저널에 수락한 뒤 화면 Full을 반환할 수 있으므로 실패한 발행 시도도 순서를 소비한다. 이후 낮은 예약 seq의 재시도는 거절하여 저널 역순을 막는다. 낮은 예약 seq부터 발행할 책임은 도메인에 있다.
- 기존 source_gap(cause,count)와 display_gap(range,count,cause,bytes)는 유지했다. source_gap_with(cause,count,Option<ProducerRange>,detail), display_gap_with(Option<ProducerRange>,count,cause,bytes,detail), record_gap(Option<ProducerRange>,count,cause,bytes,detail)를 추가했다. 세 표준 gap 모두 영향받은 emitter의 domain/channel로 ctl에 발행한다.

## 4. 시험과 품질

| 검사 | 결과 |
| --- | --- |
| cargo fmt --all --check | 통과 |
| cargo clippy -p loom-proto -p loom-worker-sdk -p loom-worker-host -p loom-schema --all-targets -- -D warnings | 통과 |
| 위 4개 crate cargo test | **85개 통과**, 실패·무시 0 |
| loom-schema export / check | 33개 생성, drift 없음 |
| git diff --check | 통과 |

crate별 통과 수는 proto 38, SDK 26, host 19, schema 2다. 이번 배정에서 새로 추가한 시험은 15개(proto 4, SDK 9, host 2)다.

- 디스패처 시험: on_call이 실제 5초 기다리는 동안 on_input과 on_latch가 각각 10 ms 안에 처리된다. 일반 요청 상한 초과 시 WorkerBusy, 포화 상태의 입력·latch·lease 종료, 긴 요청이 일반 슬롯을 채운 상태의 tick 진행, tick opt-in, lease 종료 전달·Reply 부재를 확인했다.
- 실제 프로세스 시험: 새 loom-test-shared-worker로 run_shared_worker 진입부터 IPC를 왕복했다. 5초 호출 중 입력과 latch 처리, conn_id/channel/input_seq 보존, tick, 종료 cause, 종료 통지 다음 생략 ID Input의 응답 순번을 확인했다. 실제 IPC 입력 왕복 시험의 허용치는 100 ms이며, 10 ms 판정은 같은 프로세스 디스패처 시험이다.
- 기존 직렬 worker 시험: 400 ms 호출 중 종료 통지 3개가 들어와도 ctl reader가 STOP을 읽고 100 ms 안에 ack한다. 콜백 종료 후 다음 Call의 응답 순번도 맞는다.
- Emitter 시험: 모든 draft 필드 보존, 채널 간 예약·중복·역순·미예약 오류, u64 소진, 저널 수락 후 Full의 역순 차단, 세 gap의 domain/channel/range/detail을 확인했다.
- proto 시험: 기존 map의 선택 필드 생략 호환과 JSON/MessagePack golden, canonical JSON과 SHA 벡터를 확인했다.

검증 환경은 Rust 1.93.1 / macOS aarch64다. 실제 IPC 시험은 cfg(unix)로 분리했으며 Windows 런타임은 이번 지시 범위에서 실행하지 않았다. workspace 전체 통과로 확대하지 않는다. 새 외부 의존성은 기존 workspace sha2를 proto에서 상속한 것뿐이며 루트 Cargo.toml은 수정하지 않았다. unsafe와 무한 큐는 추가하지 않았다.

로그는 워크트리 target/p1-r1-proto-test.log, p1-r1-selected-test.log, p1-r1-clippy.log, p1-r1-schema-export.log, p1-r1-fmt.log에 있다. schema check도 실제 실행해 일치함을 확인했다.
