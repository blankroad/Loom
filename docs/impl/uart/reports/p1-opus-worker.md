DONE

# P1 uart-opus 보고 — `UartWorker` (T08 조립 + T09 연결 + T10 중재 연결)

- 작성: uart-opus, 2026-11-11. docs/plan/02-core-principle.md 준수. 커밋하지 않았다.
- CARGO_TARGET_DIR는 `~/.cache/loom-target/uart-opus`다.
- ChannelCore가 오기 전에는 crate를 스크래치에 복사하고 계약 스텁을 붙여 형 검사만 했다(대상 디렉터리 `…/uart-opus-scratch`). 실제 시험은 완성된 ChannelCore로 돌렸다.

## 0. 먼저 알릴 것
1. **SDK `Worker` 콜백 직렬화**(`requests/uart-core.md` 6, 리드가 올림).
   - UART 쪽 대응: `UartWorker`를 `Clone` 핸들로 만들었다. 모든 동작을 `&self`(`call`/`write`/`input`)로 제공하고, 트레이트 구현은 이것을 부르기만 한다.
   - 사용자 개입 시험은 핸들 복제본을 두 번째 스레드에서 써서 동시성을 재현한다.
   - 실제 `run_worker`가 콜백을 직렬화하는 동안에는 두 가지가 막힌다:
     - 대기 중인 에이전트 send에 대해 사용자가 줄을 끝내는 키.
     - expect 대기 중의 Ctrl-C.
2. **Emitter `source_seq` ≠ reader·TX `SeqSource`**(`requests/uart-core.md` 7).
   - SDK에 producer id·epoch를 얻는 API가 없다. 그래서 `ChannelCore::set_producer`에는 reader 순번 공간을 그대로 이름 붙였다: `("uart-reader:<channel>", generation)`.
   - Emitter 레코드의 producer와 같다고 꾸미지 않았다. 덕분에 gap payload는 `GapData`로 역직렬화된다.
   - CORE가 seq를 공유하거나 accessor를 주면 이 한 줄(`worker.rs` open)만 바꾸면 된다.
3. **TX actor에 요청 id 취소 API가 없다**(리드·P0 `tx.rs` 요청).
   - arbiter의 `preempt_agent`(일반 사용자 키가 에이전트 전송을 선점)는 지금 이렇게 처리한다: 사용자 바이트가 64 B 이하이면 Ctrl-C 차선으로 보낸다. 이 차선은 진행 중인 에이전트 요청을 취소한다.
   - 64 B를 넘으면 사용자 차선으로 가서 에이전트 청크 사이에 끼어들 수 있다.
   - STOP latch로 이미 큐에 있는 에이전트 바이트를 거둘 수 없다. 새 쓰기는 `E_STOPPED`로 막힌다.
   - `TxHandle::cancel(request_id)`가 필요하다.
4. **비밀값.** kinds 계약은 `secret{len, sha256}`인데 이 crate에 해시 crate가 없어 `secret{len}`만 싣는다. 약한 해시는 짧은 비밀번호 대입에 쓰일 수 있어 넣지 않았다. `sha2` 의존성을 둘지 리드가 정해 달라.
5. **리드 지적 (5) `ctrl_c_cuts_a_large_agent_send` 실패.**
   - 원인은 시험 쪽 타이밍이었다. 0x03은 에이전트 요청이 취소된 **뒤에** 쓰이므로, 그 `uart.data` tx는 에이전트 응답보다 늦게 발행될 수 있다. 시험이 한 번만 drain해서 놓쳤다.
   - 이벤트 초안의 origin은 처음부터 `user`였다. 바이트를 낼 때 등록하는 `TxWait.origin`에서 정한다.
   - 시험을 `drain_until`로 바꿨고, 연속 3회 통과했다.

## 1. 바꾼 파일
| 파일 | 내용 |
|---|---|
| `src/worker.rs` | `UartWorker` 핸들, `Worker` 트레이트 구현, call 라우팅, open/close, viewer 중재 RPC, status, config 파싱 |
| `src/worker/port.rs` | `Port`(열린 포트 하나): 스레드 시작·종료, RX 펌프(10 ms 묶음 + arbiter tick), TX 이벤트 펌프, 대기자 표, 발행 헬퍼 |
| `src/worker/send.rs` | `uart.send`/`uart.run`, expect 대기(`ExpectWait`), admission, 사용자 입력 → TX 차선 |
| `src/worker/util.rs` | 오류 매핑, Ts·이벤트 초안, escape 미리보기, SDK 시계 어댑터 |
| `tests/worker_e2e.rs` | FakeWorker + `sim://bootshell` 시험 10개 |

## 2. 구성 (요지)
**open (`on_write`, `uart.open`)**
- 흐름: config(`baud/data_bits/parity/stop_bits/flow`) → `validate`(실패는 `E_UNSUPPORTED`) → `transport::open`.
- `sim_pty`는 `register_sim_pty`로 등록된 경로만 켠다.
- 띄우는 스레드: reader, `TxActor`, pump, txev.
- env: `sim://`·등록된 pty = `sim`, 그 밖 `bench`. 인자 `env`가 있으면 그것을 쓴다.
- 결과는 `{channel, generation, path, config, profile, env}`이고, `uart.state{open}`·`uart.config` 우선 이벤트를 낸다.
- 같은 설정으로 다시 열면 멱등이고, 설정이 다르면 `E_CONFIG`다.
- 닫은 뒤 같은 채널을 다시 열면 링·커서를 유지하고 **generation + 1**(`ChannelCore::new_generation`), reader는 `rx_end`부터 이어 받는다.

**RX 펌프**
- `pop_batch` → 이어지는 청크를 **최대 10 ms·64 KiB**로 합쳐 `on_rx`에 넣는다(리드 지시 3). 손실·끊김 항목이 오면 먼저 비우고 순서를 지킨다.
- 이어서: `uart.data` rx(data lane) → expect 대기자 깨우기 → `prompt_seen`/`alt_screen`을 arbiter로.
- 50 ms마다 `arbiter.tick` → `TickAction`을 대기 중인 send로 넘긴다. 아무도 기다리지 않는 `AgentReady`는 즉시 `agent_done`으로 입력권을 푼다.
- `Lost` → `on_lost` payload로 `record.gap` + `display.gap`(DR-V14).
- `Disconnected` → `uart.state{disconnected}`, 이후 send는 `E_DISCONNECTED`(재연결은 M2).

**TX 이벤트 펌프**
- `Chunk` → `on_tx` → `uart.data` tx(우선, origin·request_id는 `TxWait`에서).
- `Done` → 대기 중인 write를 깨우고 `agent_done`·`interrupt_written`을 부른다.

**send / run (`on_write`)**
1. `requested` 행을 낸다(`text` ≤ 256자, `bytes` 또는 `secret{len}`).
2. STOP latch이고 origin이 agent/script이면 `failed{E_STOPPED}`.
3. `InputArbiter::agent_send` → Now / Deferred(대기자를 arbiter 잠금 안에서 등록해 tick 경쟁이 없다) / Reject(코드 그대로).
4. `accepted{deferred_ms}`.
5. expect가 있으면 송신 **직전**에 `expect_add(ExpectFrom::Now)`.
6. TX agent 차선. deadline = `Write.deadline_mono_ns` − SDK 시계.
7. `TxDone` → 종단 행(`sent|failed|outcome_unknown`, `written_bytes`, `remaining_bytes`, `deferred_ms`, `final:true`). **응답 전에 발행한다.**
8. expect 대기(`uart.agent_activity{expect}`) → `uart.expect_result` 이벤트 → 성공이면 `commit_cursor(client, 매치 끝)`(리드 지시 2) → `detail.expect_result` 또는 `detail.output`.
   - expect timeout: state는 `sent`(바이트는 나갔다) + error `E_TIMEOUT`(exit 3, A6), detail에 `expect_timeout`.

**expect / read / lines / status (`on_call`)**
- 모두 ChannelCore(`from_params`·`from_spec`)로 간다.
- read의 `scan_limit`은 결과 필드로 그대로 전달한다(리드 지시 4).
- status에는 포트·입력권(owner/dirty/hold/lock/대기 send)·발행 실패 수를 붙인다.
- 포트를 닫은 뒤에도 남긴 링으로 read·lines·status를 할 수 있다.

**viewer 입력 (`on_input`)**
- `lease_id`를 viewer로 쓰고, `input_seq`는 내부 카운터다(`TODO(core#2)`).
- `user_input` → Ctrl-C 차선(인터럽트, 또는 64 B 이하 선점) / 사용자 차선. 제출이 실패하면 `input_ack{discarded, queue_full}`로 바로잡는다.
- 중재 RPC(`uart.input.yield/clear/takeover`, `uart.hold/unhold`)는 `on_call`로 오고, `Call.client_id`를 viewer로 쓴다(TODO core#2).

**close**
- reader stop → TX actor shutdown(미완료 요청은 `E_SHUTDOWN` → write가 `outcome_unknown`/`failed`) → txev·pump join.
- 대기 중인 send·expect는 `E_NOT_OPEN`으로 끝낸다. `uart.state{closed}`를 낸다.
- 결과에 `threads_left`·`refs_left`를 실어 누수를 점검할 수 있다(시험에서 0).

## 3. 상수 (상한·정책)
| 상수 | 값 | 넘칠 때 | 근거 |
|---|---|---|---|
| `ARB_TICK` | 50 ms | — (arbiter tick·브리지 대기) | 과제 |
| `RX_COALESCE` / `RX_COALESCE_MAX_BYTES` | 10 ms / 64 KiB | 창이나 크기를 넘으면 즉시 `on_rx` | 리드 지시 3, §2.0② |
| `PUMP_BATCH` | 64 | 다음 루프에서 계속 | §3.13 |
| `MAX_EXPECT_WAITERS` | 8 | `E_RATE_LIMIT` | §3.13 expect 행 |
| `MAX_ADMIT_WAITERS` | 64(= arbiter 상한) | arbiter가 `E_RATE_LIMIT` | §3.13 TX 행 |
| `MAX_TX_WAITERS` | 192(3 차선 × 64) | `E_RATE_LIMIT` | §3.13 TX 행 |
| `MAX_SIM_PTYS` | 16 | `E_RATE_LIMIT` | C2 |
| `MAX_KEPT_CHANNELS` | 1 | 이전 것을 교체 | 포트당 worker 1개 |
| `SEND_EXPECT_TIMEOUT_MS` / `RUN_TIMEOUT_MS` / `DEFER_TIMEOUT_MS` / `MAX_WAIT_MS` | 10 s / 30 s / 10 s / 3600 s | 상한으로 자름 | §4.1, A6 |
| 사용자 입력 TX deadline | 5 s | `outcome_unknown`/`failed` | §3.5 |

새로 만든 채널은 tokio `oneshot`(값 하나)뿐이다. 나머지 큐는 P0 상한(bridge·TX 차선·TX 이벤트 1024)을 그대로 쓴다.

## 4. 시험
```
cargo fmt -p loom-uart-worker -- --check   → ok
cargo clippy -p loom-uart-worker --all-targets -- -D warnings → ok
cargo test -p loom-uart-worker             → 100 passed (전체 4.7 s)
  worker_e2e 10 passed (0.5 s, 3회 연속 green) · 기존 lib 44 · channel_core 20 · channel_perf 1 · pty 6 · sim_transport 5 · tx 9
```
| 시험 | 확인 |
|---|---|
| `core_loop_open_expect_send_run_read` | open(env sim, gen 1) → expect `login: ` → send root --expect `Password: ` → secret 비밀번호 --expect prompt → run `uname -a`(출력에 "Linux sim", 명령 에코 제거) → read. rx/tx `uart.data`, 각 요청의 `requested→accepted→sent` + `final:true`(`assert_write_rows`), 비밀값 행에 `bytes`가 없고 `secret.len=7`, `expect_result`, `assert_no_gap_lies`. **run 뒤 summary read에 출력이 다시 나오지 않음**(커서 commit) |
| `agent_send_waits_for_the_users_line_then_goes` | 사용자 `ifcon` 타이핑 중 agent send → 300 ms 뒤 다른 스레드가 `fig\r` → 프롬프트 → 송신. `deferred_ms ≥ 200`, `deferred_send` 활동, 장치 TX 순서상 사용자 줄이 에이전트 바이트보다 앞섬 |
| `unfinished_user_line_makes_the_agent_fail_user_active` | 미완결 줄 + defer 300 ms → `E_USER_ACTIVE`, 행 `requested→failed`, 에이전트 바이트 0 |
| `hold_refuses_agent_writes` | hold → `E_HELD_BY_USER`, unhold 뒤 송신 |
| `stop_latch_blocks_agent_but_not_the_viewer` | latch → agent `E_STOPPED`(`requested→failed`), viewer 입력은 TX됨 |
| `ctrl_c_cuts_a_large_agent_send` | 느린 transport(2 ms당 16 B)에 4 KiB agent send, 100 ms 뒤 Ctrl-C → `outcome_unknown` + `E_INTERRUPTED_BY_USER`, `0 < written_bytes < 4096`, 사용자 0x03 tx |
| `rows_and_tx_bytes_are_published_before_the_reply` | 응답 시점에 그 요청의 tx `uart.data`와 종단 행(`sent`, `final:true`)이 이미 sink에 있고, tx가 종단 행보다 앞섬 |
| `close_joins_threads_and_reopen_bumps_generation` | close → `threads_left 0`·`refs_left 0`·`live_threads 0`, 닫힌 뒤 read, 재open gen 2, `uart.state{closed}` |
| `open_is_idempotent_and_refuses_other_settings` | 같은 open → 같은 결과, baud가 다르면 `E_CONFIG` |
| `unsupported_line_settings_and_bad_targets_map_to_codes` | stop 1.5 → `E_UNSUPPORTED`, `sim://nope` → `E_USAGE`, 없는 경로 → `E_NOT_FOUND` |

## 5. 과제와 다르게 한 점
1. **test seam `UartWorker::open_with_transport`(pub).**
   - Ctrl-C 선점을 결정적으로 시험하려면 느린 transport가 필요하다. sim transport는 버퍼가 1 MiB라 send가 즉시 끝난다.
   - 어떤 RPC에도 연결하지 않았다. worker 바이너리는 `transport::open`만 쓴다.
   - `register_sim_pty`도 같은 성격이다(sim이 발급한 pty만 baud ioctl을 건너뛴다).
2. **open/close의 `WriteOutcome.state`는 `sent`이고 쓰기 행을 내지 않는다.**
   - 포트 수명 동작은 바이트 송신이 아니라서, 패널의 "최근 송신"에 섞이지 않게 했다.
   - 대신 `uart.state`/`uart.config` 우선 이벤트를 낸다.
   - gateway가 모든 쓰기에 행을 요구하면 알려 달라.
3. **상태 이벤트(`uart.input_owner` 등·`uart.state`)의 origin은 `device`다.** 주체 없는 채널 상태라는 뜻이고, payload가 소유자를 말한다. 계약에 정해진 값이 없다.
4. **arbiter `idle` 활동 이벤트.** expect가 끝날 때 대기 중인 deferred send가 없을 때만 `idle`을 낸다. arbiter의 deferred 표시를 지우지 않으려는 것이다.
5. **expect의 `from`에 `seq:N`·`mark:`가 오면 `E_USAGE`다.** gateway 모듈이 `src:N`/`off:N`으로 바꿔서 보내야 한다(ChannelCore `PosSpec`과 같은 규칙).

## 6. 리드에게
1. 0절 2·3·4(`set_producer` 표기, TX cancel API, `sha2`) 판단.
2. gateway 모듈(`loom-uart`)이 할 것:
   - read·expect 위치 지정자 `seq:N`/`mark:`/시각을 `src:N`·`off:N`으로 바꾸기.
   - 결과의 `source_seq`를 stream seq로 바꾸기(0절 2가 풀린 뒤).
3. SDK 런타임이 들어오면 `Call`/`Input`에 `conn_id`·`input_seq`가 생길 것이다(core#2). 그때 `TODO(core#2)` 두 곳(`input`, 중재 RPC)을 바꾼다.

## 추가: 취소 연결 (task p1-opus-cancel-wire, TX API 168ac1c)

DONE. 바꾼 파일: `src/worker.rs`, `src/worker/{port,send,util}.rs`, 새 시험 `tests/worker_cancel.rs`. 커밋하지 않았다.

### 바뀐 동작
| 항목 | 이전 | 지금 |
|---|---|---|
| 사용자 선점(arbiter `preempt_agent`) | 사용자 바이트가 64 B 이하이면 Ctrl-C 차선으로 보내 에이전트를 끊고, 넘으면 사용자 차선으로 가서 섞일 수 있었다 | **`cancel(진행 중 agent tx id, "interrupted_by_user")`를 먼저** 부르고, 사용자 바이트는 크기와 무관하게 사용자 차선으로 보낸다. 0x03이 있으면 Ctrl-C 차선이다(64 B를 넘는 Ctrl-C 묶음은 사용자 차선으로 순서대로; 에이전트는 이미 취소됐고 사용자 차선이 에이전트 차선보다 먼저 나간다). takeover도 같다 |
| STOP(`on_latch(true)`) | 새 쓰기만 `E_STOPPED`, 큐에 있던 에이전트 바이트는 그대로 나갔다 | `Port::cancel_agents("stopped", E_STOPPED)`: TX agent 차선을 `cancel_lane`하고, admission을 기다리던 send는 즉시 `E_STOPPED`. 진행 중이던 send는 `outcome_unknown` + 정확한 `written_bytes`. 사용자 입력은 계속 나간다(계약 04 §4.4) |
| HOLD(`uart.hold`) | arbiter가 대기 send만 거절 | 위에 더해 `cancel_agents("held_by_user", E_HELD_BY_USER)`로 진행 중·큐에 있는 요청도 끊는다. 결과에 `cancelled_agent_requests` |
| close | 바로 shutdown(`E_SHUTDOWN`은 `E_NOT_OPEN`으로 매핑) | `cancel_agents("closed", E_NOT_OPEN)` 뒤 shutdown. 미완료 요청은 cause `closed`로 끝난다 |
| 종단 행 | `error.code`를 `TxErrorCode`에서 매핑(stopped·held·closed가 모두 `Shutdown` → `E_NOT_OPEN`이라 거짓 이유가 될 수 있었다) | **`TxDone.cause`를 우선**: `stopped→E_STOPPED`, `held_by_user→E_HELD_BY_USER`, `closed→E_NOT_OPEN`, `interrupted_by_user→E_INTERRUPTED_BY_USER`. 종단 행에 `cause`와 `error{code,message}`, 정확한 `written_bytes`/`remaining_bytes`를 싣는다 |
| 경쟁 구간 | — | lane cancel은 그 순간의 스냅샷이다. 그래서 admission 뒤 submit **직전**에 STOP·HOLD를 다시 확인하고(차단이면 입력권을 돌려주고 `failed`), submit **직후**에도 확인해 걸렸으면 자기 tx id를 `cancel`한다 |

### 시험 (`tests/worker_cancel.rs`, 느린 transport 16 B / 2 ms)
| 시험 | 확인 |
|---|---|
| `a_4_kib_user_paste_cuts_a_large_agent_send` | 4 KiB agent send 도중(100 ms) **4 KiB 사용자 붙여넣기**. 에이전트는 `outcome_unknown` + `E_INTERRUPTED_BY_USER`, `0 < written < 4096`. 장치에 붙여넣기 4096 B가 모두 도착하고, **에이전트 마지막 바이트가 사용자 첫 바이트보다 앞선다**(섞임 없음, 64 B 우회 제거 확인). transport가 받은 에이전트 바이트 수 = `written_bytes`(정확). 종단 행 `cause:"interrupted_by_user"` |
| `stop_cuts_the_running_send_fails_the_waiting_one_and_user_keys_still_go` | A(4 KiB) 진행 중 B(64 B)가 admission을 기다린다. 120 ms에 STOP. A는 `outcome_unknown`·`E_STOPPED`·`cause:stopped`, B는 `failed`·`E_STOPPED`이고 B의 바이트는 장치에 0. STOP 중 사용자 키 `u`는 장치에 도착 |
| `hold_cuts_the_running_agent_send` | 진행 중 HOLD → `outcome_unknown`·`E_HELD_BY_USER`·`cause:held_by_user`. transport가 받은 바이트 = `written_bytes`(끊긴 뒤 0). HOLD 중 새 send는 즉시 `E_HELD_BY_USER` |
| `close_ends_an_unfinished_agent_send_outcome_unknown` | 진행 중 close → `outcome_unknown`·`E_NOT_OPEN`·`cause:closed`, `uart.state{closed}` |

기존 `worker_e2e` 10개도 그대로 통과한다(Ctrl-C 시험은 이제 명시 cancel 경로로 `E_INTERRUPTED_BY_USER`).

```
cargo fmt -p loom-uart-worker -- --check            → ok
cargo clippy -p loom-uart-worker --all-targets -D warnings → ok
cargo test -p loom-uart-worker  ×3 연속            → 111 passed 매번
  (lib 44 · channel_core 20 · channel_perf 1 · pty 6 · sim_transport 5 · tx 9 · tx_cancel 12 · worker_cancel 4 · worker_e2e 10)
```

### 남은 것·판단 요청
1. 큐에서 취소된 요청은 `failed`(written 0)로 끝난다. 그런데 worker에서는 agent가 한 번에 하나만 TX에 들어가므로(나머지는 arbiter admission 대기), TX **큐 안**에서 취소되는 agent 요청은 사실상 submit 직후 경쟁에서만 생긴다. "대기 중 요청 `failed`"는 admission 대기자 거절로 충족했다.
2. `on_latch(false)`(STOP 해제)는 아무것도 재개하지 않는다(A1-②: 자동 재개 금지).
3. 0절의 다른 항목(SDK 직렬화 6, source_seq 7, `sha2`)은 그대로다.
