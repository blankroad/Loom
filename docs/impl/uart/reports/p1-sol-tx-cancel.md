DONE

# P1 uart-sol — TX actor 취소 API

2026-11-11. `02-core-principle.md`·계약 04 §4.4·UART plan §3.5/§3.7/§3.13 준수. 지정 worktree에서 배정 코드 2개만 변경했고 커밋·stash·질문 창을 사용하지 않았다. `CARGO_TARGET_DIR` 미지정, 기본 `target/` 사용. 실HW 없이 검증했다.

## 1. 변경 파일·새 공개 API

- `/Users/ctmctm/workspace/Loom/.worktrees/uart/crates/loom-uart-worker/src/tx.rs`
- `/Users/ctmctm/workspace/Loom/.worktrees/uart/crates/loom-uart-worker/tests/tx_cancel.rs`

기존 함수 시그니처·TxErrorCode/SubmitError variant는 유지했다. 추가 항목:

```rust
impl TxHandle {
    pub fn cancel(&self, request_id: u64, cause: &str) -> CancelResult;
    pub fn cancel_lane(&self, lane: TxLane, cause: &str) -> usize;
}

pub enum CancelResult {
    Queued,
    InProgress,
    AlreadyRequested,
    NotFound,
    QueueFull,
    InvalidCause,
}

// TxDone의 기존 필드에 추가
pub cause: Option<&'static str>;
```

허용 cause는 `interrupted_by_user` / `stopped` / `held_by_user` / `closed` 네 개다. 내부는 고정 크기 enum이므로 호출자의 문자열을 복사·보관하지 않는다. 다른 값은 InvalidCause, cancel_lane은 0으로 명시 거절한다.

| 결과 | 의미·종단 |
|---|---|
| Queued | runnable 큐에서 즉시 제거. actor가 비동기로 `Failed`, written=0, remaining=전체 길이, 지정 cause인 Done을 1개 발행 |
| InProgress | 활성 요청의 고정 cancellation slot에 기록. 현재 예약된 최대 1 KiB write가 돌아오는 청크 경계에서 중단. Done은 `OutcomeUnknown`, driver가 수용한 정확한 written_bytes |
| AlreadyRequested | 첫 취소가 이미 수용됨. 추가 메시지/Done을 만들지 않고 최초 cause 유지 |
| NotFound | 모르는 id·종단이 이미 확정된 요청·종료 중. 가짜 Done 없음 |
| QueueFull | bounded 취소 mailbox 예약 불가, 기존 요청 불변 |

`cancel_lane`은 **현재** 해당 lane의 대기·활성 요청 중 새로 취소 표시한 건수를 반환한다(기존 취소는 제외). Agent를 넘기면 User/CtrlC의 바이트·요청·permit은 건드리지 않는다. 종료·잘못된 cause·전체 mailbox 예약 불가 시 0이며 부분 적용하지 않는다. **영속 STOP latch API가 아니라 원자적 snapshot 취소**이므로 새 agent admission 차단은 호출자가 먼저 해야 한다.

활성 취소가 최종 write 완료와 경합하면 완료와 취소의 state lock 선후로 한 결과만 확정된다. 취소가 이기면 written=0 또는 전체 길이인 경우도 보수적으로 OutcomeUnknown이다. 이미 드라이버에 수용된 바이트는 취소·재송신하지 않는다. request id는 기존 worker의 `next_tx_id()`처럼 live 요청 간 유일한 u64를 써야 한다.

## 2. 상한·정책

| 상수/대상 | 상한 | 포화·처리 정책 |
|---|---|---|
| TX_CHUNK_MAX(기존) | 1,024 B/write | 취소는 다음 경계에서 반영. 현재 write deadline을 바꾸거나 flush하지 않음 |
| TX_CANCEL_QUEUE_ITEMS(신규) | 192(3 lane × 64) | queued 취소만 이동. 초과 예약은 거절, 취소 중복은 coalesce |
| TX_CANCEL_QUEUE_BYTES(신규) | 69,696 B(64 KiB + 4 KiB + 64 B) | 기존 lane payload permit을 공유하며 복사하지 않음. mailbox에 옮겨도 Done 확정까지 lane의 bytes/items에 계속 charge |
| 활성 cancellation slot(신규) | lane별 1개, 총 3개 | 첫 cause 유지, 큐/문자열 추가 할당 없음 |
| TX_CANCEL_CAUSE_BYTES(신규) | 19 B | 네 고정 문자열만 허용. 임의 길이 reason 큐 없음 |
| TX_CONTROL_QUEUE(기존) | 8건 | DTR/RTS/break 큐 유지; Ctrl-C가 준비됐으면 먼저 처리 |
| TX_EVENT_CHANNEL_CAP(기존) | 1,024건 | 기존 bounded sink 유지, Done 무음 drop 없음. 소비자가 멈추면 actor backpressure |

신규 상수 옆에 §3.13 근거·포화 정책을 적었다. queued 취소는 원래 TxRequest의 소유권만 이동한다(client_id/payload 복제 없음). 종단 확정 시 payload를 먼저 해제하고 permit을 돌려주므로 Done sink에서 막힌 동안 불필요한 TX payload를 붙잡지 않는다.

Ctrl-C는 대기 취소 완료 batch보다 **물리 write가 먼저**다. queued 취소 완료는 한 루프에 최대 1개이고 Ctrl-C가 준비됐으면 뒤로 미룬다. 취소된 활성 agent는 일반 사용자 lane이 계속 준비돼 있어도 다음 경계에서 완료한다. 단, 기존 transport write 자체가 반환하기까지의 시간과 기존 event sink backpressure는 우회하지 않는다. 바이트 경계 보장은 OS write의 hard preemption이나 wire 회수 보장이 아니다.

## 3. 시험·품질 관문

macOS arm64, Rust 1.93.1. 신규 fake transport의 제어/진입/release 채널은 모두 bounded(4건), recv 대기는 최대 2 s다. sleep·실HW는 사용하지 않았다.

| 명령 | 결과 |
|---|---|
| `cargo fmt -p loom-uart-worker -- --check` | PASS |
| `cargo clippy -p loom-uart-worker --all-targets -- -D warnings` | PASS (`Finished dev ... in 6.49s`) |
| `cargo test -p loom-uart-worker` | **107 passed, 0 failed**, doc-test 0. 시험 target 시간 합 약 3.79 s, 마지막 빌드 3.31 s 별도 |
| `git diff --check` | PASS |

전체 시험의 끝 부분:

```text
test result: ok. 44 passed; 0 failed ... finished in 0.08s  # lib
test result: ok. 20 passed; 0 failed ... finished in 0.29s  # channel_core
test result: ok. 1 passed; 0 failed ... finished in 1.37s   # channel_perf
test result: ok. 6 passed; 0 failed ... finished in 1.26s   # pty
test result: ok. 5 passed; 0 failed ... finished in 0.15s   # sim_transport
test result: ok. 9 passed; 0 failed ... finished in 0.16s   # existing tx
test result: ok. 12 passed; 0 failed ... finished in 0.01s  # tx_cancel
test result: ok. 10 passed; 0 failed ... finished in 0.47s  # worker_e2e
test result: ok. 0 passed; 0 failed ... finished in 0.00s   # doc-tests
```

신규 12개 시험:

1. queued 요청을 네 cause 각각으로 취소·0바이트 Failed·다른 agent FIFO 보존.
2. 느린 fake의 첫 1,024 B + 다음 7 B 부분 수용에서 정확히 **1,031 B**/remaining/Chunk 합계와 일치, 다음 write 없음.
3. 활성 취소의 0 B/전량 수용 경합 모두 정확한 계수와 OutcomeUnknown.
4. unknown·완료 id=NotFound, invalid cause·긴 문자열이 요청/이벤트를 바꾸지 않음.
5. cancel_lane Agent가 대기/활성 agent만 취소하고 user/Ctrl-C를 Sent로 유지.
6. user 3청크 전송 중에도 취소된 agent의 완료가 먼저 진행됨.
7. queued 취소 64개가 밀려도 Ctrl-C write 앞에 그 완료 batch가 나오지 않음.
8. 동시 취소 2개+Ctrl-C, **32회**: 한 claim만 수용·최초 cause·Done 1개/요청.
9. 마지막 write 완료 vs 취소, **64회**: Sent/NotFound 또는 OutcomeUnknown/InProgress의 단일 결과.
10. Ctrl-C 경계 vs 명시 취소, **64회**: 자동 선점/명시 취소 중 하나만 종단 발행.
11. 192건·69,696 B까지 mailbox를 채워 기존 permit 유지, 중복 100회 coalesce·추가 submit 거절·최종 전량 해제.
12. shutdown과 경합해도 이미 수용한 closed/held_by_user cause 보존·중복 Done 없음.

각 시험은 shutdown까지 남은 이벤트도 합쳐 Done 중복을 검사하고, TxChunk의 실제 바이트 합과 Done.written_bytes를 대조한다. 기존 Ctrl-C·FIFO·timeout·shutdown·worker E2E 회귀도 모두 녹색이다.

## 4. worker 연결 예시 — 리드/uart-opus 소유 파일은 수정하지 않음

`preempt_agent`는 원래 agent의 **문자열 request id**, TX actor API는 **내부 u64 tx_id**다. `port.waiters().tx`의 `TxWait.agent_request`로 찾고 잠금을 놓은 뒤 cancel한다. 연결용 의사 코드:

```rust
if let Some(agent_request) = decision.preempt_agent.as_deref() {
    let tx_id = {
        let waiters = port.waiters();
        waiters.tx.iter().find_map(|(id, wait)| {
            (wait.agent_request.as_deref() == Some(agent_request)).then_some(*id)
        })
    };
    if let Some(tx_id) = tx_id {
        let result = port.tx.cancel(tx_id, "interrupted_by_user");
        // Queued/InProgress/AlreadyRequested: 최종 상태는 TxDone으로 받는다.
        // NotFound: 이미 종단이 확정됐을 수 있음; 자동 재송신하지 않는다.
        // QueueFull/InvalidCause: 명시 오류로 보고한다.
    }
}
let lane = if decision.interrupt { TxLane::CtrlC } else { TxLane::User };
// 일반 선점 키를 64 B 이하라는 이유로 Ctrl-C lane에 넣던 우회를 제거한다.
// submit_user 쪽은 위 lane으로 원래 바이트를 submit한다.
```

STOP/HOLD/close 연결:

```rust
// agent의 gate 검사 + TX submit과 같은 admission 동기화 구간에서 수행한다.
// 먼저 새 admission을 막아야 cancel_lane 직후 들어오는 요청이 빠져나가지 않는다.
set_stop_latch_or_hold_or_closing(); // 해당 worker 소유 상태 갱신
let count = port.tx.cancel_lane(TxLane::Agent, "stopped");
// HOLD이면 "held_by_user", close이면 "closed".
// TxDone 소비/발행 펌프는 계속 돌린다. User/CtrlC 차선은 STOP/HOLD로 막지 않는다.
```

일반 사용자 선점에서도 agent submit과 중재 결정을 같은 admission 규칙으로 직렬화해야 한다. 아직 tx waiter만 등록되고 submit 직전인 요청은 외부 중재/STOP 상태를 송신 직전에 재검사한다. `cancel_lane` 자체는 미래 submit을 영구 latch하지 않는다. close는 이후 기존 actor.shutdown을 호출하면 된다. 취소 완료를 기다린다고 TX 이벤트 소비자를 먼저 멈추면 기존 bounded sink에 막힐 수 있으므로 소비/발행은 끝까지 유지한다.

**사유·코드 매핑**: 기존 `TxErrorCode`를 worker/util.rs가 exhaustive match하므로 variant 추가로 다른 소유 파일을 깨지 않았다. interrupted_by_user는 기존 InterruptedByUser, 나머지 세 취소는 호환용 Shutdown이다. 새 `TxDone.cause`가 정확한 정본 사유이므로 worker 결과 detail/오류/uart.write에 다음처럼 반영해야 한다.

| done.cause | CORE ErrorCode |
|---|---|
| interrupted_by_user | InterruptedByUser |
| stopped | Stopped |
| held_by_user | HeldByUser |
| closed | NotOpen |

그 밖에는 기존 `done.error` 매핑을 사용한다. 기존 매핑만 사용하면 stopped/held도 NotOpen이 되므로 이 연결 때 함께 보강해야 한다.

## 5. 계획과 다르게 한 점·리드에게 남기는 것

- 추가된 취소 상태/결과는 enum과 고정 mailbox로 구현했다. producer 호출자가 transport/event sink를 기다리지 않도록 비동기 수용 결과와 최종 Done을 구분한다. queued 취소의 완료도 actor가 현 write 경계를 지난 뒤 발행한다.
- 일반 user 입력의 **기존 interleave 동작은 그대로** 유지한다. 실제 선점은 새 cancel API를 호출한 후 User lane에 보내야 한다. 작업 범위가 TX API이므로 worker/send.rs·worker.rs·channel 등에는 연결 코드를 넣지 않았다.
- TxErrorCode 시그니처·variant 유지와 정확한 cause 전달을 함께 만족하기 위해 TxDone에 cause 필드를 추가했다. 모든 기존 생성·호출 코드는 전체 관문으로 확인했다.
- live tx_id 유일성은 기존 next_tx_id 호출 계약이다. 완료 id 이력을 쌓지 않으므로 완료·unknown 조회는 NotFound이며 메모리 누적이 없다.
- 추가 의존성·모듈 등록·다른 파일 수정 요청 없음. 리드/uart-opus에게 필요한 것은 위 worker 연결과 cause→CORE 코드 반영이다. API/시험 배정 범위는 완료, 커밋 없이 대기한다.
