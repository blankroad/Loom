# P1 M1 마무리 — gateway에 IPC worker 연결 (opus) 보고서

작업 지시: `docs/impl/core/tasks/p1-ipc-wire-opus.md` · 워크트리 `.worktrees/core`(기준 2b261d9) · 커밋하지 않음.
수정: `crates/loom-gateway`(주), `crates/loom-echo/src/worker.rs`(시험용 지연 인자 1곳 — 아래 W3). `loom-worker-host`·`loom-worker-sdk`·`loom-bench`·`loom-proto`·루트 `Cargo.toml`은 수정하지 않음.

## W1. `IpcHost` = gateway `WorkerHost` — 완료
- `worker_host.rs`: `impl WorkerHost for loom_worker_host::IpcHost`(보고 §4의 위임). trait에 추가한 메서드:
  - `journals_itself()` — IPC는 true, 인프로세스는 false.
  - `restart()` — IPC는 위임, 인프로세스는 no-op.
  - `pid()` — IPC는 위임, 인프로세스는 None.
- **배치 선택**: `Config.workers` > `loom.toml [workers] mode = "ipc"|"inproc"` > (`Config.clock`이 주입되면 inproc) > **ipc(기본)**.
  - 주입한 시계는 인프로세스 worker에만 닿기 때문에 그 경우만 inproc를 고른다. core-sol이 고치는 `loom-bench`(시계 주입)가 깨지지 않게 하려는 것이다.
- **worker 생성 시점 변경**: 이제 허브를 만든 **뒤에** worker를 띄운다(`DeferredSink` 제거).
  - 인프로세스 → `HubSink(journal=Some)`: gateway가 그 producer를 기록한다.
  - IPC → `HubSink(journal=None)`: worker가 직접 기록하므로 gateway는 다시 기록하지 않는다(J1 분기).
- IPC `HostConfig`에 채우는 값:
  - `program`: `WorkerDecl.program`(별도 실행 파일) > `Config.worker_program` > `current_exe()`.
  - `instance`: kind(M1은 kind당 1개, PerChannel 분할은 후속).
  - `session_id`: `s<epoch>`. `stream_epoch`: 데몬 epoch.
  - **`journal_dir` = 현재 기록 세션 루트**(`records/<date>/s<epoch>/`). worker는 `<root>/<instance>/<producer_epoch>/`에 쓰고, gateway의 mapping·조회가 같은 경로를 읽는다. `Welcome.config.journal_dir`도 같은 값.
  - `orphan`: `Config.worker_orphan`. `env`: `LOOM_HOME`.
  - 소켓 디렉터리 `$LOOM_HOME/run/w`. 경로가 Unix 소켓 길이 한도를 넘으면 `temp_dir/loom-w-<pid>-<n>`(데몬마다 고유).
- **기동 순서**: 데몬은 IPC worker가 Hello에 답할 때까지(≤ 5 s) 기다린 뒤에 `daemon.json`을 쓴다. 자동 기동한 CLI의 첫 쓰기가 worker `starting`과 경합해 `E_WORKER_DOWN`이 나던 것을 막는다.
- **상태 이벤트**: host 상태 콜백 → `core.worker_state{instance, kind, state, producer_epoch, pid, detail}`(코어 리듀서·snapshot에 반영). `core.status.workers` = 각 host의 `state()`(IPC: host·pid·starts·last_exit·journal·latch·writes).
- **RPC**: `core.worker.list`(전체 상태), `core.worker.restart{kind|instance}`(viewer operate만, 감사 `worker.restart`).
- **quarantine**: 그 도메인 worker가 `quarantined`면 쓰기가 `E_WORKER_DOWN{quarantined:true, worker}`(exit 7). 거절도 `requested → failed(final)` 행으로 보인다.
- **crash-tail**: 종료 상태 이벤트의 `detail.journal_recovery.record_gaps[]`를 producer `gateway`로 `record.gap`(cause 없으면 `crash_tail`)으로 발행한다.
- **진행 중 쓰기**: host가 돌려주는 `outcome_unknown`을 파이프라인이 그대로 종단 행(`write_state:"outcome_unknown"`, `final:true`)으로 발행한다. 응답은 `error.detail.write_state = "outcome_unknown"`이고 자동 재개는 없다.
- **이전 실행 orphan과의 충돌 방지**(결정): 데몬이 시작할 때 이전 세션의 crash-tail을 복구하는데, **최근 60 s 안에 쓰인 세션은 건너뛰고** 10분 주기 보존 작업 때 복구한다(`Journal::recover_earlier`). 재기동 직후에는 이전 데몬의 orphan worker가 아직 쓰고 있을 수 있어(orphan 30 s), 살아 있는 writer의 `.open`을 자르거나 rename하면 안 되기 때문이다.

## W2. `loomd worker <kind>` — 완료
- `main.rs`: `argv[1] == "worker"`이면 clap보다 먼저 `domains::worker_main(kind)`로 간다. SDK가 `--instance --producer-epoch --sock-dir --session --orphan-ms --health-ms --journal-dir`를 직접 읽는다. 모르는 kind는 exit 2.
- `domains.rs` 구조 변경: `Loaded{registry, workers: Vec<WorkerDecl{spec, inproc: fn(sink, clock) → host, program: Option<PathBuf>}>}`.
  - `decls(&registry, &[(kind, inproc_fn, program)])`가 등록된 `WorkerSpec`과 생성 함수를 kind로 짝짓는다.
  - `worker_main`은 kind → `loom_worker_sdk::run_worker(...)` 표(지금은 echo).
- `crates/loom-gateway/README.md`에 "도메인 worker 추가 5단계"를 적었다(도메인 crate의 Worker·WorkerSpec, `builtin` 표, `worker_main` 표, 세션 저널 위치, cargo feature).

## W3. 시험 — 완료
- **새 `tests/ipc.rs` 4개**(Unix, 각 ≤ 3 s, 합계 약 4.7 s):
  1. 진행 중 쓰기(`delay_ms` 1.5 s) 중 `kill -9`:
     - 응답 `write_state:"outcome_unknown"`. viewer가 받은 그 요청의 마지막 행이 `outcome_unknown`·`final:true`이고 `sent` 행은 없다(자동 재개 없음).
     - worker가 다운된 동안에도 `core.mark`가 발행·전달된다.
     - `core.worker_state`가 exited/killed → … → ready로 가고, 새 pid, `producer_epoch` 2. 그 뒤 쓰기 성공.
     - `record.gap`이 있으면 cause `crash_tail`.
  2. STOP:
     - `core.stop` 결과 `acked:["echo"]`, `missing:[]`, worker state `latch:true`. latch 중 agent 쓰기 `E_STOPPED`.
     - viewer 해제 → worker latch false → 쓰기 성공. `core.worker.list`도 확인.
  3. gateway 종료(orphan 400 ms) → worker 프로세스 종료 확인. 그 세션의 `echo/1/` 세그먼트가 전부 sealed(`.lj`만, `.open` 없음 = orphan seal).
  4. crash-loop: 5회 kill → `quarantined` → 쓰기 `E_WORKER_DOWN{quarantined:true}` → agent의 `core.worker.restart`는 거절, viewer는 허용 → ready → 쓰기 성공.
- **기존 시험도 IPC 기본으로 돌린다**: `tests/daemon.rs` 9개(echo E2E: 행·rx가 응답보다 먼저, `others_since_last` 포함)와 `tests/journal.rs` 4개. `Config.worker_program = CARGO_BIN_EXE_loomd`, orphan 500 ms. 모두 통과.
  - journal 재기동 시험 한 곳을 고쳤다. IPC에서 rx(`echo.data`)는 data lane으로 따로 오므로(응답 전에 보장되는 것은 쓰기 행뿐 — 계약 05 §5.3), 질의 전에 rx 3건이 도착할 때까지 기다리게 했다.
  - 이 시험은 **이전 실행의 worker가 직접 쓴 저널을 mapping으로 다시 읽는 경로**도 확인한다.
- `loom-cli` 시험 5개는 실제 `loomd` 바이너리를 쓰므로 이제 IPC worker로 돈다. 모두 통과.
- `loom-echo` 변경: `echo.send` params `delay_ms`(≤ 2 s, 장치 지연 흉내)를 worker가 `std::thread::sleep`으로 반영한다. 진행 중 쓰기를 잡기 위한 시험 장치이고 기본 동작은 그대로다.

## 품질
- fmt: `cargo fmt -p loom-gateway -p loom-echo --check` 통과. **workspace 전체 fmt는 `loom-bench`가 core-sol 작업 중(`mod soak;`는 있는데 `soak.rs`가 아직 없음)이라 실패**한다. 그 crate는 건드리지 않았다.
- clippy: `cargo clippy --workspace --all-targets --exclude loom-uart-worker --exclude loom-bench -- -D warnings` 통과.
- test: `cargo test --workspace --exclude loom-uart-worker --exclude loom-bench --no-fail-fast` → **646 통과, 1 실패** = `loom-eth` `worker::tests::unknown_stats_gap_and_stop_latency`(지시서대로 무시). 약 70 s.
  - 출력의 `loom-test-worker.rs:265` panic 줄은 worker-host 시험이 일부러 일으키는 것이다(그 시험은 통과).
  - 시험 뒤 남은 `loomd worker` 프로세스는 0개.
- `loom-gateway` 시험: 단위 6 + daemon 9 + hub 5 + **ipc 4** + journal 4 + replay 8 = 36.

## 계약과 다른 점 / 결정 / 리드에게
1. **정상 종료(`loom daemon stop`)에도 worker는 orphan 경로로 끝난다**(latch → 저널 seal → orphan 시간, 기본 30 s 뒤 종료). 프로토콜에 "정상 종료" 메시지가 없어서다. 그 30 s 동안 장치를 쥐고 있으므로, M2에 `Shutdown` 메시지(또는 latch 후 `IpcHost::shutdown`)를 제안한다.
2. 이전 세션 crash-tail 복구를 60 s 미룬다(위 W1).
3. `[workers] mode`는 기동 시에만 읽는다(재로드 대상 아님).
4. IPC 인스턴스는 kind당 1개(UART 포트당 프로세스 등 PerChannel 분할은 채널 설정이 생기면).
5. `WorkerSpec`에는 별도 실행 파일 필드가 없어 `WorkerDecl.program`(gateway `domains.rs` 표)으로 지정한다. eth `loom-eth-worker`를 붙일 때 그 표에 경로를 넣으면 된다. Registry에 넣으려면 `loom-domain-api` 변경이 필요하다.
6. Windows: IPC 경로(named pipe)는 `loom-worker-host`가 담당한다. gateway 쪽 코드는 OS 무관이지만 Windows에서 실행 검증은 하지 않았다. `tests/ipc.rs`는 `cfg(unix)`(kill/pid 사용).
7. 의존성: gateway에 `loom-worker-host`(workspace에 있음) 추가. 그 밖에는 없음.
