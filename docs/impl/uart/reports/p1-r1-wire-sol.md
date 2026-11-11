BLOCKED: CORE R1이 UART/main 의존성에 미반영. UART 연결 준비 변경은 미커밋 보존했으며, main 반영 후 재개한다. 일반 빌드·실 loomd·웹 E2E PASS 아님.

# P1 CORE R1 UART 연결 준비 — uart-sol

2026-11-11. 사용자 중간 지시로 추가 검사와 구현을 중단하고 현재 결과를 먼저 보존한다.

## 작업 공간과 상태 확인

- 코드: `/Users/ctmctm/workspace/Loom/.worktrees/uart`, `uart/main`, 시작 HEAD `4258f75`.
- 시작 시 UART `git status --short`는 깨끗했고 루트 UART STATUS에 uart-opus·uart-sol 대기가 기록돼 있었다. 프로세스 목록 확인은 sandbox에서 거부됐으며 우회하지 않았다. 작업 중 다른 worker의 UART 코드 변경은 관찰하지 않았다.
- 메인이 기존 UART 커밋을 `afd8894`로 병합한 사실은 루트 UART STATUS에서 확인했다. 이번 준비 변경은 그 이후의 별도 미커밋 변경이다.
- 다른 팀 브랜치 병합·커밋·stash 없음. CORE/루트 Cargo 파일과 타 팀 코드는 수정하지 않았다. 질문 창 없음. `CARGO_TARGET_DIR`를 지정하지 않았으며 UART worktree 기본 `target/`를 사용했다.
- 참고: 루트 `docs/impl/requests/core-answers-p1.md`, `docs/impl/core/reports/p1-r1-sol.md`, `p1-r1-opus.md`, UART→CORE 요청 §9. 정확한 시그니처는 `.worktrees/core`의 작업 중 소스를 읽어 확인했다.

## 준비한 변경

| 파일(UART worktree 기준) | 변경 |
|---|---|
| `crates/loom-uart/src/module.rs` | open/close/send/run에 `success_terminal(..., WriteState::Sent)`, `input_events(Owner::Domain)` 등록 |
| `crates/loom-uart/src/commands.rs` | channel → ch → alias 우선순위로 `worker_for`; 채널 없는 ports 등은 `worker(kind)` 유지 |
| `crates/loom-uart-worker/src/worker.rs` | 실제 `SharedWorker` 구현과 `pub fn run() -> !` 추가. 소유한 ctx·요청과 clone handle을 static Send future에 전달. serial `Worker` 어댑터는 기존 testkit용으로 유지 |
| 같은 파일 | open/close와 ports 열거를 `spawn_blocking`으로 이동. 마지막으로 lifecycle mutex를 추가하여 동시 close/open의 teardown·generation 이관을 직렬화 |
| 같은 파일 | Input의 conn_id·input_seq 사용, 다른 channel 거부. 식별자가 없으면 Usage 오류(lease/client 문자열로 viewer를 추정하거나 batch seq를 만들어내지 않음). viewer 제어 Call도 conn_id 사용 |
| 같은 파일 | `input_lease_end` 및 양쪽 trait callback을 `InputArbiter::viewer_left`에 연결. held keys 폐기·owner 해제, unfinished line dirty 유지 |
| `crates/loom-uart-worker/src/worker/port.rs` | RX 병합 후, TX chunk 처리 시 `reserve_seq`로 실제 producer seq를 확보해 링과 이벤트에 동일하게 사용. RX는 `rx_with_seq`, TX는 메타데이터를 보존하는 `data_with_seq` 사용 |
| 같은 파일 | worker 수명 동안 공유하는 publication mutex로 RX·TX·제어 이벤트의 예약→링 반영→발행 순서를 보호. reopen의 예전 port와 새 port도 같은 mutex 사용. 발행 실패 시 같은 예약 seq 재시도 없음. TX data 실패 카운터 추가 |
| `crates/loom-uart-worker/src/channel.rs` | `on_lost_unsequenced` 추가. 예약 전 bridge 손실은 producer_range 없이 offset·bytes·count를 보존. reader chunk id는 detail에만 기록. 기존 `on_lost` 시그니처 유지 |
| `crates/loom-uart-worker/src/clock.rs`, `bridge.rs` | reader/TX hand-off id와 발행 시 실제 source_seq의 구분을 주석에 명시 |
| `crates/loom-uart-worker/tests/worker_e2e.rs`, `worker_cancel.rs` | R1 Call/Input 리터럴 필드 보강 |
| `crates/loom-uart-worker/tests/r1_wire.rs` | 실제 SharedWorker/Emitter로 실행하는 R1 시험 4개 추가 |
| `crates/loom-uart/tests/registry.rs` | success_terminal과 input_events 소유자 등록 검증 추가 |

seq 예약은 reader에서 하지 않는다. reader의 예약을 오래 보유하면 RX 병합 대기·독립 TX pump·제어 이벤트가 더 큰 seq를 먼저 발행할 수 있기 때문이다. 링에 들어가는 chunk만 publication 구역에서 실제 이벤트 seq를 부여한다. bridge에서 사라진 bytes에는 아직 이벤트 seq가 없으므로 reader id를 실제 producer range로 표기하지 않는다. 실제 producer id/epoch는 Emitter getter로 설정한다.

## 상한·정책

| 항목 | 상한·정책 |
|---|---|
| publication | worker당 mutex 1개, 동시 예약 1개. 새로운 이벤트 대기 큐/정렬 버퍼 없음 |
| lifecycle | worker당 mutex 1개. open/close를 blocking 작업에서 직렬화 |
| 기존 RX 병합 | `RX_COALESCE` 10 ms, `RX_COALESCE_MAX_BYTES` 64 KiB 유지 |
| 기존 reader/bridge/TX/arbiter | 기존 상한 유지; 새 무한 큐·buffer 없음 |
| 시험 capture | `CAPTURE_RECORDS = 2048`, 초과 시 시험 실패 |
| 입력/latch 인프로세스 시험 | 각 100 ms 제한; 전체 대기 2 s 제한 |

CORE SharedWorker 런타임의 예약 슬롯·큐 상한은 SDK가 소유한다. 여기서 같은 슬롯 구현을 복제하지 않았다. 제어 RPC에서 생성하는 기존 내부 전송 식별자는 유지되며, 실제 Input batch ack의 seq는 전달받은 input_seq다.

## 검증 결과와 재현 방법

### 일반 UART 의존성: 예상된 차단, 반복 금지

한 번 실행한 명령:

```sh
cd /Users/ctmctm/workspace/Loom/.worktrees/uart
cargo check -p loom-uart-worker -p loom-uart --all-targets --locked --offline
```

exit 101. 현재 UART SDK에는 `InputLeaseEnd`, `SharedWorker`, `run_shared_worker`, `Worker::on_input_lease_end`가 없다. 이는 CORE R1 통합 전의 예상 의존성 차단이다. 같은 검사를 반복하지 않는다.

### 실제 CORE 작업 소스를 읽기 전용 경로로 연결한 제한적 검증

브랜치 병합이나 manifest 수정 없이 Cargo 일회성 `paths` override로 실제 CORE 구현을 사용했다. stub API나 cfg로 시험을 숨기지 않았다. 이 구성은 CORE의 미커밋 작업 소스에 의존하며 main 통합 빌드의 대체 증거가 아니다.

```sh
cd /Users/ctmctm/workspace/Loom/.worktrees/uart
cargo --config 'paths=["/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-proto", "/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-worker-sdk", "/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-domain-api", "/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-testkit"]' check -p loom-uart-worker -p loom-uart --all-targets --locked --offline
cargo --config 'paths=["/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-proto", "/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-worker-sdk", "/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-domain-api", "/Users/ctmctm/workspace/Loom/.worktrees/core/crates/loom-testkit"]' test -p loom-uart-worker --test r1_wire --locked --offline
```

- all-targets check: exit 0, `Finished dev profile ... in 1.05s`.
- R1 시험: exit 0, **4 passed; 0 failed; finished in 0.13s**.
- Cargo는 path override로 의존성 목록이 달라졌다는 경고를 출력했다. 이 임시 구성의 한계를 숨기지 않으며, 정식 검증은 CORE main 통합 후 override 없이 수행해야 한다.
- 마지막 lifecycle mutex와 설명 주석 추가 **이전** 결과다. 최종 변경 전체에 대한 재실행은 사용자 지시에 따라 보류한다.
- `cargo fmt -p loom-uart -p loom-uart-worker` 실행 완료. 별도 최종 `--check` 관문은 미수행.

시험 이름과 실제 확인 범위:

1. `input_uses_connection_and_client_sequence_and_lease_end_keeps_dirty_line`: opaque lease와 다른 client_id에 속지 않고 conn_id 사용; JS 정수 정밀도 밖 input_seq decimal 문자열 보존; 타 채널·누락 필드 거부; 종료 후 owner 해제·dirty 유지; conn_id 제어 Call.
2. `shared_input_and_latch_run_while_expect_is_pending`: 실제 SharedWorker future로 긴 expect 대기와 입력·latch 동시 실행, close로 expect 해제. **IPC reserved-slot 동작의 증거는 아님**.
3. `ring_sequences_equal_real_data_records_even_after_accepted_full_and_reopen`: 실제 Emitter의 reserved seq와 링 행 일치; 엄격히 증가하는 이벤트 순서; journal acceptance 뒤 Full 상황에서 중복 재시도 없음; TX data lane·origin·client/request/env 메타데이터; close/reopen.
4. `bridge_loss_before_reservation_has_no_invented_producer_range`: 예약 전 손실에는 가짜 producer seq 없음, bytes/count·reader 진단 좌표 보존.

로그는 worktree의 `target/p1-r1-check.log`, `target/p1-r1-wire.log`, `target/p1-r1-clippy.log`에 보존했다. target은 빌드 산출물이며 git 추적 대상이 아니다.

`clippy` 첫 시도는 cargo 외부 subcommand 앞의 `--config`가 실제 clippy 검사에 적용되지 않아 구 SDK 타입 누락에서 종료됐다. config를 clippy 뒤에 지정한 최종 시도는 실제 R1 구현까지 연결됐으나 **exit 101**이다. 새 `r1_wire.rs`의 146·307행에서 `clippy::cmp_owned` 2건이 발견됐다. JSON의 decimal 문자열과 `.to_string()`을 직접 비교하는 시험 코드다. 이것은 UART 소유 시험의 미해결 lint이며 CORE 미반영 차단과 구별한다. 사용자 보존·대기 지시에 따라 추가 수정/재검사는 하지 않았다. 재개 시 예상 문자열을 비교 바깥에서 준비하고 `as_str()`로 비교하여 문자열 계약 assertion을 유지해야 한다(숫자 JSON 비교로 바꾸면 안 됨).

```text
error: this creates an owned instance just for comparison
  --> crates/loom-uart-worker/tests/r1_wire.rs:146:85
  --> crates/loom-uart-worker/tests/r1_wire.rs:307:52
error: could not compile `loom-uart-worker` (test "r1_wire") due to 2 previous errors
```

## 필요한 CORE API와 연결

- `loom_worker_sdk::{SharedWorker, run_shared_worker}`. callback은 `&self`, owned WorkerCtx/request, `BoxFuture<'static, ...>`; latch·lease-end callback 포함.
- `loom_proto::worker::{InputLeaseEnd, InputLeaseEndCause}`. `Input{call_id, conn_id, channel, input_seq}` 및 `Call{conn_id}`.
- `Emitter::{reserve_seq, rx_with_seq, data_with_seq, producer_id, producer_epoch}`. 낮은 예약 seq부터 발행, Full 뒤 재시도 금지 계약.
- `Registry::{success_terminal, input_events}`, `Owner::Domain`.
- `CallCtx::worker_for(kind, channel) -> Result<Box<dyn WorkerHandle + '_>, DomainError>`. 기존 worker는 borrowed handle이므로 두 반환형을 구분해 처리했다.
- **CORE 소유 `crates/loom-gateway/src/domains.rs`의 UART 실행 분기를 `loom_uart_worker::worker::run()` 또는 `run_shared_worker(Arc::new(UartWorker::new()))`로 전환해야 한다.** 관찰한 CORE R1 소스도 여전히 UART에 `run_worker(UartWorker::new())`를 사용한다. 여기서는 수정하지 않았다. serial 어댑터가 남아 있으므로 빌드 성공만으로 Shared 런타임 연결 완료로 판단하면 안 된다.
- CORE gateway가 입력 conn_id/channel/input_seq를 전달하고 WS 종료·release 때 InputLeaseEnd를 보내야 한다. `input_events(Owner::Domain)`에서 core의 가짜 uart.input sent 이벤트가 없어야 한다. 실제 worker TX와 input_ack/held/discarded는 도메인 소유다.

## main R1 후 재개할 검증 — 아직 미검증

1. 위 UART 시험 lint 2건을 수정하고, 정식 CORE R1 반영 뒤 override 없이 최종 diff의 fmt check·all-target clippy `-D warnings`·UART 두 crate 전체 시험. 기존 E2E/cancel/분류 회귀와 새 등록 assertion 실행.
2. 데몬 UART worker 실행 분기의 SharedWorker 연결 확인. 실제 IPC에서 긴 expect/send 중 Ctrl-C·STOP·lease 종료의 지연과 취소 결과 검증.
3. gateway PerChannel 인스턴스 `<kind>.<channel>` 라우팅, channel/ch/alias 우선순위, 다중 채널 격리와 ports의 `_enum` fallback.
4. 실 gateway write 행의 final=true/sent 성공 terminal 및 중복 uart.input sent 부재. 정상/held/discarded/실제 TX 이벤트 대조.
5. 실제 worker journal·hub에서 RX/TX/control seq 순서, source_seq↔stream seq 매핑, reader bridge overflow·표시 Full·reopen·동시 close/open의 정합성.
6. UI 팀과 웹 패널 E2E. 실제 HW는 사용하지 않았으며 sim 인프로세스 시험만 확인했다.

전체 UART/워크스페이스·실 데몬·웹 연결은 PASS로 보고하지 않는다. 새 API를 사용하는 UART 준비 변경은 기존 구 SDK에서는 컴파일되지 않는 의도적인 통합 대기 diff다. 임의 compatibility feature나 stub, 타 팀 소스 변경으로 차단을 숨기지 않았다.

질문/승인 요청 없음. CORE R1 main 반영 지시가 오면 재개한다.
