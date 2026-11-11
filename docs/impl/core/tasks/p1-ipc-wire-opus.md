docs/plan/02-core-principle.md 준수

> **질문 창 금지**(playbook §6): 대화형 질문 창을 띄우지 않는다. 계약과 이 지시서로 판단해 진행하고 결정을 보고 파일에 적는다. 판단할 수 없으면 질문을 보고 파일에 적고 할 수 있는 부분까지 끝낸다. 경로는 항상 `/Users/ctmctm/workspace/Loom/...`.

# P1 M1 마무리 — core-opus: gateway에 IPC worker 연결

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`(기준 `2b261d9`). 근거: 계약 `05-worker-protocol.md`(1.1 포함)·`01` §1.1, 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-worker-ipc.md`(§4 WorkerHost 연결 스니펫, §8 worker 저널)·`p1-sol-a.md`.
**범위 경계**: `crates/loom-gateway`(주), 필요하면 `crates/loom-worker-host`·`crates/loom-worker-sdk` 소폭(보고에 명시). 동시에 core-sol이 `crates/loom-bench`를 고친다 — 건드리지 말 것. 루트 `Cargo.toml`·`loom-proto`는 리드만.

## W1. `IpcHost`를 gateway의 `WorkerHost`로
- `impl WorkerHost for loom_worker_host::IpcHost`(gateway 쪽, 보고 §4 스니펫). worker 이벤트는 `HubSink`로(우선/대량, `payload.final` 처리 그대로).
- 도메인 등록의 `WorkerSpec`에 따라 **인프로세스 / IPC 선택**: 설정 `[workers] mode = "ipc"|"inproc"`(기본 **ipc**, 시험 편의상 inproc 유지 가능). IPC일 때는 **worker가 저널을 직접 쓰므로 gateway는 그 producer를 다시 기록하지 않는다**(J1의 분기 사용). 저널 디렉터리는 `Welcome.config.journal_dir` = 현재 기록 세션의 `<producer_id>` 상위.
- `core.worker_state` 이벤트·`core.status` worker 표·`core.worker.restart`(viewer operate)·`core.worker.list` 연결. quarantined면 그 도메인 쓰기 `E_WORKER_DOWN{quarantined:true}`.
- **crash-tail**: worker 종료 이벤트의 `detail.journal_recovery.record_gaps`를 producer `gateway`로 `record.gap{cause:"crash_tail"}` 발행. 진행 중 쓰기는 `outcome_unknown` 행(final).

## W2. `loomd worker <kind>` 하위 명령
- `loomd worker <kind> --instance <id> --producer-epoch N --sock-dir … [--journal-dir …]` → 등록된 도메인의 worker 조각을 `loom_worker_sdk::run_worker`로 실행(지금은 `echo`). gateway가 자기 실행 파일 경로(`std::env::current_exe`)로 spawn. `WorkerSpec`에 별도 실행 파일(eth `loom-eth-worker`)이 있으면 그것을 쓴다.
- 도메인 crate가 worker를 제공하는 방법(`domains.rs`에 kind → worker 생성 함수 표)을 `loom-echo`로 보이고, 다른 도메인이 따라 할 수 있게 `crates/loom-gateway/README.md`에 5줄.

## W3. 시험 (echo를 IPC로)
- echo E2E가 **IPC 모드로도** 통과(행·rx가 응답보다 먼저, `others_since_last`).
- worker `kill -9`(시험에서 pid로) → 다른 스트림 영향 없음, 진행 중 쓰기 `outcome_unknown`, `core.worker_state` crashed → ready, crash-tail `record.gap`(있을 때), 쓰기 자동 재개 없음.
- STOP → IPC worker latch ack(`core.latch.acked`에 worker 포함), latch 중 agent 쓰기 거절.
- gateway 종료 → worker orphan latch 후 종료(짧은 orphan 시간 설정).
- 전체 시험 3분 이내, 각 시험 15 s 이내.

## 품질·보고
workspace 전체 fmt·clippy(`-D warnings`)·test — 단 **`loom-uart-worker`의 시험 리터럴과 `loom-eth`의 시험 1건은 다른 팀 요청 대기 중**(`docs/impl/requests/core-uart.md`, `core-eth.md`)이라 `--exclude loom-uart-worker`로 돌리고 eth 1건 실패는 무시해도 된다(보고에 명시). 커밋하지 않는다. 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-ipc-wire-opus.md`(W1–W3, 시험 수, 계약과 다른 점). 화면에는 경로와 3줄 요약만. 10분 넘는 실행 금지.
