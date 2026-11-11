docs/plan/02-core-principle.md 준수.
질문 창 금지: 대화형 질문을 띄우지 말고, 스스로 정해 보고서에 적거나 보고서에 질문을 쓰고 끝낸다(playbook §6, p0-common.md).

# P1 uart-opus: `UartWorker` — worker 조립 (T08 + T09 연결 + T10 중재 연결)

공통 규칙: `docs/impl/uart/tasks/p0-common.md`. CARGO_TARGET_DIR = `$HOME/.cache/loom-target/uart-opus`.
보고서: `/Users/ctmctm/workspace/Loom/docs/impl/uart/reports/p1-opus-worker.md`.

## 수정 가능한 파일 (이것만)
- `crates/loom-uart-worker/src/worker.rs` (하위 `src/worker/` 허용)
- `crates/loom-uart-worker/tests/worker_*.rs`

`src/channel.rs`(ChannelCore)는 **다른 작업자가 동시에** 만든다 — API 계약은 `docs/impl/uart/tasks/p1-channel-core.md` "공개 API". 먼저 그 API를 가정하고 짜되, 그쪽이 끝나기 전에는 컴파일이 안 될 수 있다(그동안 worker 쪽 구조·TX·arbiter 연결부터). 다른 모듈(transport·reader·bridge·tx·sim)·`loom-console`·`loom-uart`는 읽기만 — 고칠 점은 보고서에. Cargo.toml에는 `loom-worker-sdk`, `tokio`, `loom-console`, `base64`, `hex`, dev `loom-testkit`이 이미 있다.

## 목표
`loom_worker_sdk::Worker`를 구현한 `UartWorker` — **채널(포트) 하나 = worker 하나**(PerChannel, plan §3.13). CORE 런타임(`run_worker`)은 아직 stub이므로 **`loom_testkit::FakeWorker`로 시험**한다(견본: `crates/loom-echo/src/worker.rs`, `crates/loom-echo/tests/echo.rs`). gateway 쪽 명령·kind·라우팅 정본: `crates/loom-uart/src/{commands,kinds}.rs`(Request 쓰기 = `open/close/send/run`이 `on_write`로 옴, `op.method`에 메서드명 — `commands.rs` `Forward` 참고; 그 밖은 `on_call`).

## 구성 (plan §3.1, §3.5, §3.7)
- `uart.open`(on_write): `transport::open`(sim:// 또는 경로, `sim_pty`는 sim이 발급한 pty만) → reader 스레드 + bridge → **펌프 스레드**(bridge pop_batch → `ChannelCore::on_rx`/`on_lost` → emitter `rx(uart.data)` / gap 보고, `Ingest.hits`로 expect 대기자 깨움, `prompt_seen`/`alt_screen`을 arbiter에 전달) + TX actor(이벤트 채널 → `on_tx` → `uart.data dir=tx` + `TxDone` → 대기 중인 쓰기 완료). env: sim://·pty = `sim`, 그 밖 `bench`(인자 `env`가 있으면 그것). 결과 `{channel, generation, path, config, profile, env}` + `uart.state{state:"open"}`·`uart.config` 이벤트(우선). 같은 채널 재open은 멱등(설정 다르면 `E_CONFIG`).
- `uart.send`/`uart.run`(on_write): ① `InputArbiter::agent_send` → Now / Deferred(대기 중 `uart.agent_activity{deferred_send}` + tick으로 재판정) / Reject(코드 그대로) ② `send --expect`면 송신 **직전** `expect_add(ExpectFrom::Now)` ③ TX actor agent lane 요청 → `TxDone` → `uart.write` 행(`requested→accepted→sent|failed|outcome_unknown`, 같은 `request_id`, 종단 행 `final:true`, `bytes`(secret이면 생략 + `secret{len,sha256}`), `text` escape 미리보기, `written_bytes`, `remaining_bytes`, `deferred_ms`) — **우선 이벤트로 먼저 발행하고 나서** 결과 반환(publish-before-reply) ④ expect 대기(timeout은 tokio 시간, deadline은 `Write.deadline_mono_ns`도 존중) → `expect_result` 이벤트 + 결과 data(`sent_escaped`, `state`, `written_bytes`, `deferred_ms`, `expect_result`/`output`). STOP latch(`cx.latch()`) 중 agent/script 쓰기 = `E_STOPPED`, 행은 `requested→failed`로 보이게.
- `uart.read`/`uart.lines`/`uart.status`/`uart.expect`(on_call): ChannelCore로. expect는 async 대기(대기 중 `uart.agent_activity{expect}`), timeout = exit 3 성격 오류(`ErrorCode::Timeout`, detail에 `ChannelCore::expect_timeout` data).
- 사용자 입력(`on_input`, worker `Input{lease_id, bytes, priority}`): CORE에 `conn_id`·`input_seq` 추가를 요청해 둠(`requests/uart-core.md` 2). **그때까지 `lease_id`를 viewer id로, `input_seq`는 내부 카운터**로 쓰고 `TODO(core#2)` 주석. `InputArbiter::user_input` → send 바이트를 TX user/Ctrl-C lane으로, `preempt_agent`면 TX actor가 agent 요청 취소(Ctrl-C), ack 이벤트 발행. viewer 중재 RPC `uart.input.yield/clear/takeover`, `uart.hold/unhold`는 on_call(`Call.client_id`를 viewer id로 임시 사용).
- arbiter tick: 50 ms 주기(tokio interval 또는 펌프 스레드), `TickAction`을 대기 중 send에 전달. `ArbiterEvent`는 모두 우선 이벤트(`emitter.priority`)로.
- 오류 코드: transport/tx 자리표시(`E_TIMEOUT`, `E_INTERRUPTED`, `E_IO`, `E_SHUTDOWN`)를 `loom_proto::error::ErrorCode`로 매핑(`Timeout`, `InterruptedByUser`, `Disconnected`/`NotOpen` 등 — 실제 enum에서 고를 것). 포트 열기 실패 `E_PORT_BUSY`/`E_PERMISSION`/`E_NOT_FOUND`, 미지원 설정은 `ErrorCode::Unsupported`.
- `uart.close`: reader·펌프·TX 정지(각 ≤ 100 ms), 미완료 쓰기 `outcome_unknown`, `uart.state{closed}`.
- C2: 새로 만드는 큐·채널은 전부 상한(tokio `mpsc` bounded 등)과 넘칠 때 정책 주석.

## 시험 (`tests/worker_*.rs`, FakeWorker + `sim://bootshell`, 필요하면 pty)
1. 핵심 루프: open → expect `login:` → send `root` --expect `# ` → run `uname -a` 출력 확인 → read. 발행 이벤트(`drain_published`)에 `uart.data` rx/tx, `uart.write` 행 순서, `final:true`, testkit `assert_write_rows`·`assert_no_gap_lies` 통과.
2. 사용자 개입: viewer 입력 `ifcon`(미완결) 중 agent send → Deferred → 줄 완결 + 프롬프트 뒤 송신 / 미완결 유지 시 `E_USER_ACTIVE`(defer timeout 짧게). Ctrl-C가 agent 대용량 send를 끊음(`outcome_unknown` + `written_bytes`). hold 중 `E_HELD_BY_USER`. STOP latch 중 agent `E_STOPPED`이지만 viewer 입력은 송신.
3. publish-before-reply: send 결과를 받은 시점에 해당 `uart.write` 종단 행과 tx `uart.data`가 이미 sink에 있다.
4. close 후 스레드 누수 없음, 재open generation +1.
전체 `cargo test -p loom-uart-worker` 60 s 이내.

## 끝나면
보고서 첫 줄 `DONE`(또는 ChannelCore 대기 중이면 `BLOCKED: channel` + 진행 상태). 대기.
