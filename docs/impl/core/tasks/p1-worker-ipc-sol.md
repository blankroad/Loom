docs/plan/02-core-principle.md 준수


> **질문 창 금지**(playbook §6): 작업 중 사용자·리드에게 대화형 질문 창(선택지 묻기 등)을 띄우지 않는다. 계약(`docs/contract/`)과 이 지시서로 판단해 진행하고 그 결정을 보고 파일에 적는다. 판단할 수 없으면 질문을 보고 파일에 적고 할 수 있는 부분까지 끝낸다.

# P1 M1 — core-sol: worker IPC와 감독 (`crates/loom-worker-host`, `crates/loom-worker-sdk`의 `run_worker`)

작업 위치: **`/Users/ctmctm/workspace/Loom/.worktrees/core`**(브랜치 `core/main`). 옛 Desktop 경로는 절대 쓰지 말 것(playbook §6). 저장소 루트(main 체크아웃)에서는 코드를 고치지 않는다.
먼저 읽기: `/Users/ctmctm/workspace/Loom/docs/impl/00-impl-playbook.md`, 워크트리의 `docs/contract/05-worker-protocol.md`, `01-processes-ipc.md` §1.1, `docs/plan/core/plan.md` §3.3·§9.2(Q3–Q8)·§9.7, 그리고 `crates/loom-proto/src/worker.rs`, `crates/loom-worker-sdk/src/`(프레임 코덱·Emitter·LatchFlag 이미 있음).

## 범위 경계 (같은 worktree를 셋이 쓴다)
- **너**: 새 crate `crates/loom-worker-host`(gateway 쪽: 소켓 서버·worker 프로세스 spawn·감독), 그리고 `crates/loom-worker-sdk`(worker 쪽: `run_worker` 실제 구현, IPC 스레드).
- core-opus: `crates/loom-gateway`(허브·WS·파이프라인). gateway는 `src/worker_host.rs`의 **`WorkerHost` trait**(인프로세스 구현)으로 worker를 부른다 — 너는 같은 trait를 만족하는 **IPC 구현을 네 crate에** 만든다(trait 정의는 gateway에 있으니 읽기만 하고, 맞추기 어려우면 보고. gateway 연결은 리드가 한다).
- 리드 하위 작업자: `crates/loom-journal`.
- `crates/loom-proto`·루트 `Cargo.toml`은 수정 금지(필요 의존성은 보고). workspace에 있는 것: tokio(net·time·sync·io-util·signal·fs·rt-multi-thread·macros), serde/json, rmp-serde, thiserror, tracing, rand, tempfile.

## 할 일
1. **전송**: Unix(`cfg(unix)`) = Unix domain socket(디렉터리 0700, `~/.loom/run/w/<instance>.{ctl,data}.sock` — 홈은 `LOOM_HOME` 환경변수로 바꿀 수 있게), Windows(`cfg(windows)`) = tokio named pipe(이름 `\\.\pipe\loom-<user 해시 8>-<instance>-{ctl,data}`, 원격 pipe 거부 플래그). Windows 코드는 macOS에서 `cargo check --target x86_64-pc-windows-msvc`가 가능하면 확인(타깃이 없으면 보고만 — rustup 설치 금지). **물리 연결 2개(ctl, data).**
2. **프레임**: `loom-worker-sdk`의 `encode_frame/decode_frame` 사용, 64 KiB 상한, 할당 전 길이 검사, read deadline, 잘린 프레임·모르는 tag → 그 연결 오류.
3. **worker 쪽 `run_worker`**: IPC 스레드(tokio current_thread)가 연결·`Hello` 송신(`producer_epoch`는 기동마다 증가 — gateway가 `--producer-epoch`로 넘김), `Welcome`/`Reject` 처리, ctl 수신 → `Worker` 콜백 디스패치, **`Latch`·우선 `Input`은 큐 없이 즉시 처리**(LatchFlag set, 우선 mailbox), `Health` 1 s(RSS: macOS `task_info`·Linux `/proc/self/status`·Windows는 P2로 미루고 None 허용 — unsafe가 필요하면 그 부분은 보고하고 None), `Emitter` sink를 IPC로 연결(data lane = `Events` 배치, 적응형: 조용한 뒤 즉시·≤ 10 ms·64 KiB, **바이트 크레딧**(기본 1 MiB) 없으면 화면 미러(4 MiB/8192건)에 쌓다가 넘치면 드롭 + `display.gap{producer_range, cause:"ipc_backpressure"}`를 ctl로), 우선 이벤트는 ctl `PriorityEvents`, `Reply{covers}`는 그 요청의 마지막 우선 이벤트 SourceRef. ctl EOF(gateway 사망) → **즉시 latch**, orphan 30 s 후 종료(저널 seal 훅 호출 자리 — 저널 연결은 리드가 함).
4. **gateway 쪽 `loom-worker-host`**: `WorkerSupervisor` — `WorkerSpec`(loom-domain-api)대로 spawn(기본 `loomd worker <kind> --instance <id> --producer-epoch N --sock-dir …`, **실행 파일 경로를 spec에서 받을 수 있게**(eth는 별도 `loom-eth-worker`)), Hello 대기 ≤ 5 s, 상태기계 starting → ready → suspect(Health 3 s 누락) → stalled(5 s) → killed(10 s 또는 hard 메모리), 재기동 백오프 250 ms → … ≤ 30 s ±20 %, 60 s 내 5회 → quarantined, `reopen_after_crash` 전달, 크래시 시 진행 중 호출 = `outcome_unknown`. 상태 변화는 콜백/채널로 gateway에(→ `core.worker_state`). 큐 상한 Q5(ctl 송신 256 KiB/256건 → `E_WORKER_BUSY`), Q6(ctl 수신 256 KiB/4096건), Q4(크레딧).
   - `WorkerHost` IPC 구현: `call`·`write`(→ `Reply`의 covers 반환)·`job`·`input`·`latch`(ack 수집, worker당 타임아웃 200 ms → missing 보고)·이벤트 스트림(우선/대량 구분).
   - 메모리 hard 집행(Job·cgroup)은 P2 — 지금은 `Health.rss`로 soft/hard 초과 감지 → 경고/kill만.
5. **시험 바이너리**: `crates/loom-worker-host/tests/`에서 쓸 수 있는 시험용 worker 실행 파일(예: `[[bin]] name = "loom-test-worker"` 또는 `examples/`) — echo처럼 send를 rx로 되돌림, `--fault panic|abort|hang|flood` 옵션.
6. **시험**: 왕복(call·write·covers 순서: 우선 이벤트가 Reply보다 먼저), latch ack ≤ 10 ms(p99는 측정만), data 크레딧 0일 때 장치 수신(가짜 생산자) 계속 + display.gap 정확, worker kill → 재기동·outcome_unknown·다른 worker 영향 없음, crash-loop → quarantined, hang → stalled → kill, gateway(호스트) drop → worker latch 후 orphan 종료(시험에서는 orphan 시간을 짧게 설정), 잘린 프레임·64 KiB 초과 → 연결 오류. 각 시험 10 s 이내, 전체 1–2분.

## 품질·보고
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`(다른 작업자 crate가 잠시 깨져 있으면 `-p loom-worker-host -p loom-worker-sdk`로 하고 보고), `cargo test -p loom-worker-host -p loom-worker-sdk`. `unsafe` 금지(workspace lint) — RSS 등에 꼭 필요하면 쓰지 말고 보고. 무한 큐 금지. 커밋하지 않는다. 보고: `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-worker-ipc-sol.md`(만든 것·시험 수·지연 측정값·`WorkerHost` trait와 맞춘 방식·리드에게 필요한 것). 화면에는 경로와 3줄 요약만. 10분 넘는 실행 금지.
