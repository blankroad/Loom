# P1 M1 — worker IPC와 감독 보고 (`loom-worker-host`, `loom-worker-sdk::run_worker`)

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core` (브랜치 `core/main`). 커밋하지 않음.
지시서: `docs/impl/core/tasks/p1-worker-ipc-sol.md`(core-sol 대신 수행). 보고 파일 이름은 코디네이터 지시대로 `p1-worker-ipc.md`로 했다(지시서의 `p1-worker-ipc-sol.md`가 아님).

수정 범위
- 새 crate `crates/loom-worker-host/`
- `crates/loom-worker-sdk/`: `Cargo.toml`(tokio, dev tempfile 추가), `src/lib.rs`(stub `run_worker` 제거, 모듈 추가), `src/emit.rs`(`Emitter::display_gap`·`producer_id`·`producer_epoch` 추가), 새 파일 `src/runtime.rs`·`src/transport.rs`·`tests/transport.rs`
- loom-gateway·loom-proto·loom-journal·loom-cli·loom-cli-kit·skills·루트 `Cargo.toml`은 건드리지 않음. `Cargo.lock`에 새 crate 항목이 생김(workspace 자동).

## 1. 만든 것

### `loom-worker-sdk`
- **`transport`**: 소켓 이름과 연결, 비동기 프레임 I/O.
  - Unix: `<sock_dir>/<instance>.{ctl,data}.sock`. 기본 디렉터리는 `$LOOM_HOME/run/w`이고 `LOOM_HOME`의 기본값은 `~/.loom`. 경로가 100 B를 넘으면 오류.
  - Windows: `\\.\pipe\loom-<user FNV 8>-<instance>-{ctl,data}`.
  - `FrameReader::read(deadline)`: 8 B 헤더를 먼저 검사한다(≤ 64 KiB, 아는 type, flags 0, reserved 0). 그 뒤에야 버퍼를 늘린다. 첫 바이트 이후 프레임 전체에 read deadline을 건다. 프레임 중간 EOF는 `Truncated`, 경계에서의 EOF는 정상 종료다.
  - `is_request`: Reply를 받는 G→W 메시지인지 판별.
- **`runtime::run_worker` / `run_worker_with(worker, WorkerArgs, RunHooks)`**
  - 인자: `--instance --producer-epoch --sock-dir --session --orphan-ms --health-ms --journal-dir`. 그 밖의 토큰(`worker <kind>`, 도메인 플래그)은 무시한다.
  - **IPC 스레드**(호출 스레드, tokio current_thread)가 하는 일:
    - ctl·data 연결 → `Hello` → `Welcome`/`Reject`(Reject면 exit 3, 연결 실패면 exit 2).
    - ctl 수신:
      - `Latch`는 큐 없이 처리한다: 그 자리에서 `LatchFlag`를 set하고 `LatchAck`를 urgent 큐에 넣은 다음 dispatcher에 알린다.
      - 우선 `Input`은 1칸 mailbox로 보낸다.
      - 나머지 요청은 256칸 요청 큐로 보낸다. 큐가 넘치면 `E_WORKER_BUSY`로 답한다.
    - ctl 송신: 그때까지 쌓인 PriorityEvents를 먼저 보내고 Reply·Ack·Health를 보낸다(publish-before-reply).
    - data 송신: 바이트 크레딧 안에서 `Events` 배치를 보낸다. 조용했던 뒤에는 즉시 보내고, 아니면 최대 10 ms 모은다. 배치 상한은 64 KiB다.
    - `Health`를 1 s(설정 가능)마다 보낸다.
  - **dispatcher 스레드**(별도 current_thread): `Worker` 콜백을 하나씩 실행하고 heartbeat task를 돌린다. 콜백이 스레드를 막으면 heartbeat가 멈추고, IPC 스레드는 Health를 보내지 않는다. 그러면 gateway가 suspect → stalled → kill로 판정한다.
  - **IpcSink**(`EmitSink`):
    - `Lane::Ctl`은 우선 큐로 간다(Q6 256 KiB/4096, 넘치면 `Emitter`가 `Full`).
    - `Lane::Data`는 화면 미러로 간다(Q3 4 MiB/8192). 미러가 넘치면 화면 사본만 버리고 drop run(채널별 연속 구간)을 센다. 구간이 끝나거나 1 s가 지나면 `display.gap{producer_range, count 정확, cause:"ipc_backpressure", bytes}`를 ctl로 보낸다.
    - `Reply.covers` = 그 콜백 동안 우선 큐에 들어간 마지막 SourceRef. 없으면 None.
  - **gateway 사망**(ctl/data EOF·오류·잘못된 프레임): 즉시 latch on(flag와 `on_latch`). orphan 시간(기본 30 s) 뒤 `RunHooks::on_orphan`(저널 seal 자리)을 부르고 exit 0.
  - panic hook: 어디서든 panic이 나면 process abort(release의 `panic = "abort"`와 같은 의미).
  - RSS: Linux는 `/proc/self/status`. macOS·Windows는 FFI(unsafe)가 필요해 **0(모름)**으로 보낸다. `RunHooks::rss`로 바꿀 수 있다.
  - `encode_write_outcome`/`decode_write_outcome`: 쓰기 Reply 형식 `{write_state, detail, error}`.
- `Emitter::display_gap(range, count, cause, bytes)`: gap 레코드도 같은 producer seq를 받는다. domain·channel = 영향받은 스트림(계약 02).

### `loom-worker-host`
- **`listen::LaneListeners`**
  - Unix: 디렉터리 0700, 오래된 소켓 파일은 지우고 bind. accept할 때 peer uid가 같은지, 그리고 OS가 pid를 주면 spawn한 child pid와 같은지 확인하고 아니면 버린다. 두 lane이 연결되면 소켓 파일을 지운다.
  - Windows: `first_pipe_instance` + `reject_remote_clients`.
- **`host::IpcHost`**(`HostConfig` + `WorkerSpec`, `Arc<dyn EmitSink>`, 상태 콜백)
  - spawn: `<program> <args: 기본 ["worker", kind]> --instance --producer-epoch N --sock-dir --session --health-ms [--orphan-ms] <extra_args>`. 실행 파일 경로는 config에서 받는다(eth는 `loom-eth-worker`).
  - 핸드셰이크: Hello ≤ 5 s를 기다리되 child 조기 종료와 race한다.
    - 다음이면 `Reject` → `Incompatible`(재기동하지 않음): major 불일치, kind·instance·producer_epoch 불일치.
    - 통과하면 `Welcome{accepted_minor = min, stream_epoch, config, budgets, latch = 현재 latch}`를 보낸다.
  - 상태기계: `starting → ready → suspect(Health 3 s 누락) → stalled(5 s) → killed(10 s 또는 RSS > hard)`, `exited`, `restarting`, `quarantined`, `incompatible`, `stopped`. RSS가 soft를 넘으면 `ready` + `detail.mem = "pressure"` 이벤트를 낸다.
  - 재기동: 250 ms × 2ⁿ, 최대 30 s, ±20 % jitter. 60 s 넘게 ready였던 세션 뒤에는 백오프를 초기화한다. 60 s 안에 5회 종료하면 `quarantined`, `restart()`로 해제한다. producer epoch는 기동마다 +1.
  - 종료 처리: 소켓에 이미 쓴 write → `outcome_unknown`, 아직 큐에 있던 write → `failed{sent:false}`, call·job·input → `E_WORKER_DOWN`.
  - 큐:
    - Q5 ctl 송신 256건/256 KiB → `E_WORKER_BUSY`. 큐에 못 들어간 write는 `failed`.
    - latch 전용 16칸 lane은 먼저 쓴다.
    - 크레딧 grant 64칸(합쳐서 보냄).
    - 수신 측은 큐 없이 sink로 동기 push(소켓 backpressure).
  - 크레딧: 초기값 = `spec.budgets.credit`(설정 가능). 소비한 `Events` 프레임 바이트만큼 다시 준다(`regrant_credit`). `grant_credit()`도 공개.
- **`loom-test-worker`**(이 crate의 `[[bin]]`):
  - write → 우선 행 `sent`·`confirmed` + data lane `test.rx` echo.
  - call: `echo`, `rx{n}`(가짜 장치 생산자), `prio`, `latched`, `sleep`, `hang`, `panic`, `abort`, `flood`.
  - 플래그: `--fault panic|abort|hang|flood`, `--latch-file`, `--fake-rss`.

## 2. 시험 — 27개 통과 (두 crate, 전체 약 2.7 s, 3회 반복 모두 통과)

| 묶음 | 수 | 내용 |
|---|---|---|
| `loom-worker-host/tests/ipc.rs` | 8 | ① 왕복: call·오류·write(우선 행 2개가 write 완료 **전에** sink에 있음, `covers` = 마지막 행)·echo rx·job start/stop·입력(일반/우선)·동시 call 50개 id 매칭 ② latch ack 200회 + 300 ms 걸리는 call 중에도 ack < 100 ms + latch 중 write `failed/E_STOPPED` ③ 크레딧 0: 가짜 생산자 1만 건이 막힘 없이 전부 Ok, `display.gap` 합계가 정확히 1808(= 10000 − 미러 8192), 범위가 생산 범위 안, 크레딧을 주면 미러 8192건 도착, 받은 수 + gap = 1만 ③ kill -9 → 진행 중 write `outcome_unknown`·재기동(producer_epoch 2)·다른 worker 영향 없음 ⑤ crash-loop(`--fault abort`) → quarantined, 그 뒤 재기동 없음, call `quarantined:true` / `--fault panic` → exited ⑥ hang → suspect(이때 write는 미전송 `failed`) → stalled → killed(`no_health`) → 재기동 ⑦ RSS > hard → killed(`mem_hard`) ⑧ 호스트 drop → worker latch 후 orphan 종료 |
| `loom-worker-host/tests/frames.rs` | 3 | 가짜 gateway가 실제 worker에 64 KiB 초과 헤더·모르는 type·잘린 프레임을 보냄 → 연결 오류 → latch → orphan 정상 종료 |
| `loom-worker-sdk/tests/transport.rs` | 7 | 왕복·경계 EOF, 초과 헤더(할당 전 거부), 모르는 type·flags, 잘린 프레임(헤더·본문), read deadline, 깨진 payload, 64 KiB 넘는 메시지는 쓰지 않음 |
| `loom-worker-sdk` 단위 + 기존 `tests/sdk.rs` | 3 + 6 | 인자 파싱, write outcome 왕복, drop run 정확·상한 / 기존 시험 |

품질 검사:
- `cargo fmt --all --check` 통과.
- `cargo clippy --workspace --all-targets -- -D warnings` 통과(확인 시점, 다른 작업자 변경 포함).
- `cargo test -p loom-worker-host -p loom-worker-sdk` 통과.
- Windows: `cargo check`와 `clippy -D warnings`를 `--target x86_64-pc-windows-gnu`로 두 crate에 대해 통과. msvc 타깃은 설치돼 있지 않아 gnu로 확인했고, 실행 시험은 하지 않았다.
- `unsafe` 없음.

## 3. 지연 측정값 (macOS, debug 빌드, 시험 안)
- latch → LatchAck 왕복 200회: **p50 78 µs, p99 178 µs, max 347 µs**(기준 ≤ 10 ms).
- 크레딧 0에서 가짜 생산자 1만 건 emit: 44 ms(장치 수신이 막히지 않음).
- 호스트가 사라진 뒤 worker latch: 7.8 ms. orphan 종료: 303 ms(설정 300 ms).
- hang → kill: 1.85 s(시험 설정 suspect 0.4 / stall 0.7 / kill 1.2 s, health 100 ms. 기본값이면 10 s).

## 4. `WorkerHost` trait와 맞춘 방식
trait는 `loom-gateway`에 있다. gateway가 이 crate에 의존해야 하므로 여기서 trait를 구현하면 순환 의존이 생긴다. 그래서 `IpcHost`에 **같은 이름·같은 시그니처의 메서드**(`kind/call/write/job/input/latch/state`, `BoxFuture` 반환)를 두었다. 리드가 gateway에 아래 위임 impl만 넣으면 된다(trait는 gateway 로컬이라 orphan 규칙 문제 없음).

```rust
impl WorkerHost for loom_worker_host::IpcHost {
    fn kind(&self) -> &str { loom_worker_host::IpcHost::kind(self) }
    fn call(&self, c: Call) -> BoxFuture<'_, Result<Value, WorkerError>> { loom_worker_host::IpcHost::call(self, c) }
    fn write(&self, w: Write) -> BoxFuture<'_, WriteOutcome> { loom_worker_host::IpcHost::write(self, w) }
    fn job(&self, j: JobCmd) -> BoxFuture<'_, Result<Value, WorkerError>> { loom_worker_host::IpcHost::job(self, j) }
    fn input(&self, i: Input) -> BoxFuture<'_, Result<(), WorkerError>> { loom_worker_host::IpcHost::input(self, i) }
    fn latch(&self, l: Latch) -> BoxFuture<'_, Result<(), WorkerError>> { loom_worker_host::IpcHost::latch(self, l) }
    fn state(&self) -> Value { loom_worker_host::IpcHost::state(self) }
}
```

의미는 InProcessHost와 같다:
- 이벤트는 생성 때 받은 `EmitSink`(gateway에서는 `HubSink`)로만 나간다.
- `write`는 우선 행이 sink에 들어간 뒤에 끝난다. 같은 ctl FIFO에서 PriorityEvents가 Reply보다 먼저 오고, reader가 그것을 sink에 넣은 다음 Reply를 처리하기 때문이다. watermark 대기용으로 `write_with_covers()`가 `covers`도 돌려준다.
- `latch`는 ack를 받으면 Ok다. 200 ms 안에 ack가 없으면 `E_TIMEOUT{missing:true, instance}`, worker가 없으면 `E_WORKER_DOWN`.
- `state()`는 `{kind, instance, host:"ipc", state, producer_epoch, pid, starts, last_exit, rss_bytes, mem_pressure, latch, writes}`.

## 5. 결정한 것과 계약·지시와 다른 점
1. **job·input의 Reply 대응**: `JobStart/Update/Renew/Stop`, `InputLease`, `Input`에는 proto에 `call_id`가 없다. 그래서 양쪽 모두 G→W 요청 프레임을 소켓 순서대로 1, 2, …로 센다. `Call`/`Write`는 그 번호를 `call_id`로 싣고, 나머지는 `Reply{call_id: n}`으로 답한다. 소켓이 FIFO라 처리 순서가 달라도(우선 입력) 번호가 맞는다. **proto v1.x에서 이 메시지들에 `call_id`를 넣는 것을 권한다.**
2. **스레드 분리**: 지시서는 "IPC 스레드가 콜백 디스패치"라고 했다. 콜백이 스레드를 막아도 latch·ack가 막히지 않도록 콜백은 별도 dispatcher 스레드에서 돈다. 그 스레드의 heartbeat로 "콜백 hang"을 Health 중단으로 바꾼다.
3. **data lane 화면 드롭**: 미러가 차도 `Emitter::rx`는 Ok를 돌려준다(저널이 원본이고 화면 사본만 버리기 때문). 손실은 `display.gap`으로 보고한다. 단 **저널이 아직 연결되지 않아** 지금은 그 레코드가 실제로 잃어진다(아래 6-3).
4. **`display.gap` 범위**: `producer_range`는 버린 seq들의 최소~최대다. 그 사이에 우선 lane으로 나간 seq가 끼어 있을 수 있다. `count`는 정확하다.
5. **크레딧 단위** = 프레임 전체 바이트(헤더 포함). gateway는 받은 프레임 바이트만큼 즉시 다시 준다(창 일정).
6. **상태별 거절**:
   - write는 `ready`일 때만 받는다(suspect부터 거절, 계약 "신규 쓰기 거절").
   - call·input은 ready·suspect일 때 받는다.
   - job은 ready일 때만 받는다. 단 `JobStop`은 suspect에서도 받는다.
   - 모든 종료를 크래시로 센다(worker는 스스로 끝나지 않으므로).
7. **`reopen_after_crash` 전달**: `Welcome.config`에 `reopen_after_crash: "none"|"rx_only"`와 `restart: bool`(두 번째 기동부터 true)을 넣는다. 실제 재open 판단은 도메인 worker 몫이다.
8. **Health의 journal 기본값** = `disabled`(저널 미연결 상태를 그대로 보고). 리드가 `RunHooks::journal`로 실제 값을 연결한다.
9. **orphan 종료 코드 0**. Reject는 3, 설정·연결 실패는 2.
10. **호스트 drop** = gateway 사망과 같다(worker를 죽이지 않고 소켓만 닫음 → worker orphan). `shutdown(true)`는 kill한다.
11. 시험 worker는 lib crate의 `[[bin]]`이다(`CARGO_BIN_EXE_*`가 bin에만 있음). 배포에서 빼려면 리드가 별도 crate로 옮겨도 된다.
12. 지시서는 `interprocess` 2.x 언급(plan §3.3) 대신 tokio를 직접 썼다(workspace에 이미 있음, 의존 추가 없음).

## 6. 리드에게 필요한 것
1. **gateway 연결**:
   - 위 `impl WorkerHost for IpcHost`.
   - `IpcHost::start(HostConfig::new(spec, instance, current_exe), HubSink, 상태 콜백)`. 상태 콜백 → `core.worker_state`.
   - `HostConfig.stream_epoch`, `session_id`, `config`를 채운다.
2. **`loomd worker <kind>` 서브커맨드**: 현재 gateway `main.rs`에는 없다. `loomd worker <kind> …`이면 해당 도메인 `Worker`로 `loom_worker_sdk::run_worker(...)`를 부르게 해야 한다.
3. **저널 연결**(`RunHooks`):
   - `on_record` → `JournalHandle::try_append`.
   - `journal` → `JournalStatus::to_proto()`.
   - `on_orphan` → `JournalWriter::close()`.
   - `Segment` 메시지(sealed 보고)는 host가 지금 무시한다.
4. **의존성**:
   - `loom-worker-host`는 자기 Cargo.toml에서 `tokio = { workspace = true, features = ["process"] }`로 `process` 기능을 **추가만** 했다(루트는 그대로). 루트 workspace 항목에 `process`를 넣어도 된다.
   - `loom-worker-sdk`가 이제 tokio에 의존한다.
5. **P2로 미룬 것 (unsafe 또는 FFI 필요)**:
   - Windows named pipe DACL = 현재 사용자(현재는 기본 DACL + 원격 거부).
   - Windows peer pid 확인.
   - macOS·Windows RSS(지금 0 = 모름). 그래서 **soft/hard 메모리 감시는 Linux에서만 실제로 동작**하고, 나머지에서는 hook이 있어야 동작한다.
   - Windows Job/Linux cgroup hard 집행.
6. **계약 보강 제안**:
   - (a) job·input 메시지에 `call_id`.
   - (b) 기존 `Emitter::source_gap`은 domain이 `"source"`인데, 계약 02는 "영향받은 스트림의 domain"이라 어긋난다. 내 범위 밖이라 고치지 않았다.

## 7. 기타
- 파일 읽기가 멈춘 일은 없었다. 대화형 질문은 띄우지 않았다.
- 시험은 `/tmp/lw*` 임시 디렉터리를 쓴다(Unix 소켓 경로 길이 제한 때문).

---

## 8. 후속 작업 (237c9c6 이후): worker 쪽 저널과 `source.gap` 수정

수정 범위는 이번에도 `crates/loom-worker-sdk`와 `crates/loom-worker-host`뿐이다(두 Cargo.toml에 `loom-journal.workspace = true` 추가). gateway·proto·journal·루트 `Cargo.toml`은 그대로다. 커밋하지 않음.

### 8.1 만든 것
- **worker 안 저널**(계약 06 §6.1, can-core W3)
  - `run_worker`가 `Welcome`을 받은 직후 `loom_journal::JournalWriter`를 띄운다.
    - 위치: `--journal-dir`가 우선이고, 없으면 `Welcome.config.journal_dir`. 경로는 `<root>/<instance>/<producer_epoch>/`.
    - codec `json/1`, 큐 16 MiB/32k(Q2 worker 합), 재시도 간격 `--journal-retry-ms`(기본 5 s).
    - 둘 다 없으면 저널을 끄고 Health는 `disabled`로 보고한다.
  - `IpcSink::try_push`가 **모든 레코드를 lane·크레딧과 무관하게** 먼저 `try_append`한다. 그래서 크레딧이 없어 생기는 화면 드롭은 이제 화면에서만의 손실이고, `display.gap`은 screen-only라는 의미 그대로다.
  - **Health**: `journal = {written_through, durable_through, health}`가 실제 값이다. `queues.journal`에는 깊이·바이트·drops를 싣는다.
  - **저널 감시**(IPC 스레드, 50 ms마다):
    - health가 바뀌면 ctl로 `record.health{health, previous, written_through, durable_through, error}`를 보내고 Health를 즉시 한 번 더 보낸다.
    - 끝난 손실(`journal_queue_full`, 회복 후 `disk_error`/`disk_full`, `sync_failed`)은 `record.gap`(GapData, producer_range 포함)으로 보낸다.
    - 우선 큐가 차서 못 보낸 gap은 최대 64개까지 보관해 다음 주기에 다시 보낸다. 넘치면 마지막 gap에 합치고 count는 unknown이 된다.
  - **orphan**: orphan 시간이 끝나면 `JournalWriter::close()`(drain → fsync → rename seal)를 부르고, 그다음 `RunHooks::on_orphan`을 부르고 종료한다.
  - `RunHooks`는 확장용으로 남겼다. `on_record`는 저널과 함께 호출되고, `journal` hook이 있으면 Health 출처를 덮어쓴다.
- **seq 순서 보장**: 저널은 producer별로 `source_seq`가 엄격히 증가해야 한다. 여러 스레드가 동시에 emit하면 seq 할당과 push 순서가 엇갈릴 수 있었다. 그래서 `Emitter`가 producer별 mutex 안에서 **seq 할당과 sink push를 함께** 한다(짧은 임계 구역 — 저널 enqueue와 크기 계산).
- **`source.gap` 수정**(계약 02 §2.4, UI Q13): domain·channel이 이제 영향받은 스트림(그 emitter)의 것이다(이전에는 domain `"source"`). 그 밖의 변경:
  - 새 `Emitter::system(kind, payload)`를 추가했다(record.gap·record.health 등 producer 수준 ctl 이벤트용).
  - channel이 `""`인 emitter는 이제 `channel` 필드를 생략한다.
- **host**
  - `HostConfig.journal_dir`를 추가했다. 그 값은 `--journal-dir`로 넘어간다.
  - worker가 종료하거나 죽으면 host가 그 epoch 디렉터리를 `recover_producer_dir`로 복구한다(torn tail 절단 + seal). 결과는 종료 상태 이벤트의 `detail.journal_recovery = {segments, record_gaps:[GapData]}`에 담긴다.
  - `state()`에 마지막 Health의 `journal{health, written_through, durable_through}`가 들어간다.

### 8.2 시험 — 두 crate 합계 31개 통과(+4, 3회 반복 모두 통과, 실행 약 5 s)

| 시험 | 내용 |
|---|---|
| `journal.rs::credit_starved_flood_is_fully_journaled_and_orphan_seals` | 크레딧 0으로 1만 건을 흘린다. 화면 쪽 `display.gap` 합은 정확히 1808이다. Health의 `durable_through`가 마지막 seq까지 올라간다. gateway가 사라지면 orphan으로 종료하고 **모든 세그먼트가 sealed**다. 디스크의 `test.rx`가 **정확히 10000건**이고 첫·끝 seq가 일치하며 seq가 엄격히 증가한다. 디스크의 display.gap 수 = ctl로 보낸 수 |
| `journal.rs::unwritable_journal_dir_reports_failed_then_gap_on_ctl` | 저널 디렉터리 자리에 파일을 둔다. rx 100건 → ctl `record.health{failed, error}`(domain = 영향받은 스트림), host `state().journal.health = failed`. 파일을 지우면 → ctl `record.gap{cause: disk_error, producer_range ⊇ rx 범위, count ≥ 100}` → `record.health ok` → 다시 기록되어 디스크에 rx 10건 |
| `journal.rs::crashed_worker_journal_is_recovered_by_host` | written_through 확인 뒤 kill -9 → 종료 이벤트에 `journal_recovery.segments ≥ 1`, 죽은 epoch가 전부 sealed, 1000건 모두 남음, 재기동은 epoch 2 |
| `sdk.rs::gap_events_use_the_stream_domain_and_order_is_kept` | `source.gap`이 domain `can`·channel `ch1`. `system`은 channel 없음. 4 스레드에서 8000건을 emit해도 sink에 seq가 엄격 증가 순서로 들어옴 |
| `sdk.rs::emitter_is_non_blocking_and_counts_seq`(기존) | 예전 기대값(domain `"source"`)을 계약대로 `"uart"`로 고침 |

품질 검사: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings` 통과. `--target x86_64-pc-windows-gnu` clippy(두 crate)도 통과.

### 8.3 결정·한계
1. **채널별 8 MiB 상한**(Q2 "채널 8 MiB")은 따로 집행하지 않는다. 저널은 seq 순서 때문에 producer당 1개이고 큐는 worker 합(16 MiB/32k)만 적용된다. 채널별 상한이 필요하면 `loom-journal`에 클래스별 계정(예: `try_append_in(class)`)을 추가해야 한다. **리드에게 요청할 사항.**
2. 저널 `Full`이어도 화면·ctl 전달은 계속하고 `Emitter`는 Ok를 돌려준다. 원본 손실은 저널 gap → `record.gap` + `record.health failed`로만 알린다(Q2: 원본 버림 = record.gap + health failed).
3. `record.health`는 상태가 바뀔 때마다 보낸다. 손실 gap을 가져가면(`take_gaps`) 저널은 다시 `ok`가 되므로 failed → ok 순서로 두 번 나온다. failed를 얼마나 유지할지(에이전트 쓰기 차단 기간)는 gateway가 정한다.
4. 저널 큐 overflow(`journal_queue_full`) 경로는 `loom-journal` 시험이 다루고, worker 쪽은 같은 `take_gaps` 경로로 보낸다. worker 통합 시험에서 overflow 자체를 일으키지는 않았다(디스크 오류 경로로 같은 보고 경로를 검증).
5. `Emitter`의 producer mutex는 gateway `InProcessHost`의 emitter에도 적용된다. `HubSink` ctl publish가 비정상적으로 막히는 경우에는 같은 producer의 다른 emit도 그동안 기다린다(정상 경로에서는 짧다).
6. crash 복구 gap은 host가 SourceRecord로 만들지 않는다. 죽은 producer의 seq를 쓸 수 없기 때문이다. 대신 상태 이벤트 detail(GapData 목록)로 넘긴다. gateway가 producer `gateway`로 `record.gap{cause: crash_tail}`을 발행해야 한다. **리드에게 요청할 사항.**
