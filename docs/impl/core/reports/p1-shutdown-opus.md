# P1 M1 — 정상 종료 시 worker 즉시 해제 (opus) 보고서

작업 지시: `docs/impl/core/tasks/p1-shutdown-opus.md` · 워크트리 `.worktrees/core`(기준 7c5f90f) · 커밋하지 않음.
수정: `loom-gateway`(`daemon.rs`, `main.rs`, `worker_host.rs`, `tests/ipc.rs`), `loom-worker-sdk`(`runtime.rs`), `loom-worker-host`(`host.rs`, `tests/ipc.rs`), `loom-cli`(`tests/cli.rs`만). **proto 변경 없음.** `loom-bench`는 건드리지 않았다(작업 트리의 `M crates/loom-bench/src/main.rs`는 core-sol 작업분).

## 한 일
- **gateway** `Daemon::shutdown_gracefully()`: `core.shutdown`(= `loom daemon stop`), Ctrl-C, **SIGTERM**(Unix, 새로 처리)에서 실행한다.
  1. 새 요청 거절: `closing` 플래그. `core.status`를 뺀 모든 RPC가 `E_DAEMON "the daemon is shutting down"`.
  2. 모든 worker에 `Latch{on:true, scope:"all", reason:"shutdown"}`을 보내고 ack를 모은다(≤ 200 ms, 병렬).
  3. worker 해제: `WorkerHost::shutdown()`(새 trait 메서드, 인프로세스는 no-op). IPC는 `IpcHost::shutdown_graceful(2 s)` = ctl·data lane을 닫고 프로세스 종료를 ≤ 2 s 기다린 뒤 넘으면 kill.
  4. 저널 close(seal).
  5. `main`이 `daemon.json`을 삭제(자기 pid일 때만).
  - `latch.json`은 **쓰지 않는다**. shutdown latch는 worker로만 가고 `Daemon::stop`(사용자 STOP 경로)을 거치지 않는다. 그래서 사용자 STOP 상태였으면 파일의 `on:true`가 그대로 남아 재기동 후 latched로 시작하고, 아니면 latched로 시작하지 않는다.
- **worker SDK** `run_worker`: `Latch{on:true, reason:"shutdown"}`를 받으면 표시해 둔다(`SHUTDOWN_REASON` 상수). 그 뒤 ctl EOF가 오면 **orphan 대기 없이** latch → 저널 seal(`JournalWriter::close`) → `on_orphan` hook(장치 close 자리) → exit 0. 그 밖의 EOF(gateway 크래시)는 기존 orphan 경로(latch, 저널 계속, orphan 시간 후 seal·종료) 그대로.
- **worker host** `IpcHost::shutdown_graceful(grace)`: 기존 `shutdown(false)`(orphan에 맡김)·`shutdown(true)`(kill)에 더해 "lane을 닫고 grace만큼 기다린 뒤 kill"을 추가했다. 정상 종료에서는 crash 복구를 하지 않는다(worker가 스스로 seal).

## 시험 (새 4개 + 기존 유지)
| 시험 | 내용 |
|---|---|
| gateway `ipc::graceful_shutdown_releases_worker_now` | orphan 10 s로 설정해도 `shutdown_gracefully` 후 **약 145 ms**에 worker 프로세스가 사라짐(≤ 1 s 확인), ack `["echo"]`, 이후 요청 `E_DAEMON`, `latch.json` 없음, 세그먼트 전부 sealed |
| gateway `ipc::graceful_shutdown_keeps_user_stop_only` | 사용자 STOP → 정상 종료 → 재기동하면 latched. viewer 해제 → 정상 종료 → 재기동하면 latched 아님 + echo 송신 성공 |
| worker-host `ipc::graceful_shutdown_skips_orphan_wait` | 시험 worker(orphan 10 s)에 shutdown latch → `shutdown_graceful` → **약 1.4 ms**에 종료, kill 없이(< 1 s) |
| cli `daemon_stop_releases_worker_and_restarts` | `loom daemon stop`이 돌아온 뒤 **1 s 안에 worker pid가 사라짐** → 같은 `LOOM_HOME`으로 바로 `echo send` 성공(자동 재기동), 재기동 후 latch false. `stop-all` → `daemon stop` → 재기동 후 `status.latch == true` |
- gateway kill -9 / 소켓만 닫는 경로: 기존 `ipc::gateway_exit_orphans_worker`(orphan 400 ms 후 종료 + seal)와 worker-host `host_gone_worker_latches_then_orphan_exits`가 그대로 통과한다(orphan 경로 유지 확인).

## 품질
- `cargo fmt -p loom-gateway -p loom-worker-sdk -p loom-worker-host -p loom-cli --check` 통과.
- `cargo clippy --workspace --exclude loom-uart-worker --exclude loom-bench --all-targets -- -D warnings` 통과.
- `cargo test -p loom-gateway -p loom-worker-sdk -p loom-worker-host -p loom-cli --no-fail-fast`: **3회 연속 78/78 통과**(각 27–29 s, 부하 평균 약 9–10).
- 시험 뒤 남은 `loomd worker` 프로세스는 0개.

### 타이밍 시험과 부하
- 처음 3회 중 1회(부하 평균 약 21)에서 `loom-cli::events_bounded_and_cursors`가 실패했다(최신 3건이 `m1..m3`).
- 원인은 이번 변경이 아니다. **직전 작업에서 기본값을 IPC로 바꾼 뒤**에는 rx(`echo.data`)가 data lane으로 와서 CLI 응답보다 늦을 수 있는데(계약 05: 응답 전 보장은 쓰기 행뿐), 시험은 즉시 읽었다.
- 시험이 rx 도착을 기다리도록(`--peek` 폴링 ≤ 3 s) 고쳤고, 그 뒤 CLI 단독 3회와 4개 crate 3회 모두 통과했다.
- 남은 타이밍 민감 시험: gateway T2(최종 행이 응답보다 50 ms 이상 늦지 않음), 1 s 해제 판정. 부하 평균 20 이상에서는 여유가 줄어든다.

## 계약 05 §5.5에 넣을 문장 (리드)
- "정상 종료: gateway는 새 요청을 거절하고 모든 worker에 `Latch{on:true, reason:"shutdown"}`을 보낸 뒤(ack ≤ 200 ms) 두 연결을 닫는다. 이 latch 뒤의 EOF를 받은 worker는 orphan 대기 없이 저널을 seal하고 장치를 닫은 다음 즉시 종료한다(gateway는 ≤ 2 s 기다린 뒤 kill). shutdown latch는 사용자 STOP이 아니므로 영속하지 않는다."
- "그 밖의 연결 끊김(gateway 크래시)은 기존대로 latch → 저널 계속 → orphan 30 s 후 seal·종료."

## 메모
- 정상 종료 순서에서 gateway 자신의 메모리 latch나 `core.latch` 이벤트는 내지 않는다(이미 새 요청을 거절하므로 불필요하고, UI에 STOP처럼 보이면 안 됨).
- Windows: SIGTERM 처리는 Unix 전용이다(Windows는 Ctrl-C 경로). named pipe 쪽 graceful 경로는 같은 코드를 쓰지만 실행 검증은 하지 않았다.
