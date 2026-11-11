docs/plan/02-core-principle.md 준수

> **질문 창 금지**(playbook §6). 경로는 항상 `/Users/ctmctm/workspace/Loom/...`.

# P1 M1 — core-opus: 정상 종료 시 worker 즉시 해제 (작은 작업)

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`(기준 `7c5f90f`). 근거: 네 보고 `p1-ipc-wire-opus.md` "리드에게 1"(정상 종료에도 worker가 orphan 30 s 동안 장치를 쥠).
**왜 지금**: 사용자가 `loom daemon stop` 후 바로 다시 띄우면 UART 포트가 30 s 동안 busy다 — M1 시연에서 바로 보인다.
**범위**: `crates/loom-gateway`, `crates/loom-worker-sdk`, `crates/loom-worker-host`. core-sol이 `crates/loom-bench`를 고치는 중 — 건드리지 말 것. **proto 변경 없이** 한다.

## 할 일
- 정상 종료(`loom daemon stop`, SIGTERM/Ctrl-C, `core.shutdown`이 있다면 그것)에서 gateway는: 새 요청 거절 → 모든 worker에 `Latch{on:true, reason:"shutdown"}` → ack 수집(≤ 200 ms) → ctl·data 연결을 닫는다 → worker 종료 대기(≤ 2 s, 넘으면 kill) → `daemon.json` 삭제.
- worker(SDK): `reason == "shutdown"`인 latch를 받은 뒤 ctl EOF를 보면 **orphan 대기 없이** 저널 seal → 장치 close → 즉시 종료(exit 0). 그 밖의 EOF(gateway 크래시)는 기존 orphan 경로 그대로.
- `latch.json`: 정상 종료의 shutdown latch는 사용자 STOP이 아니므로 **영속하지 않는다**(재기동 후 latched로 시작하면 안 됨). 사용자 STOP 상태였으면 그대로 유지.
- 계약 문서 반영은 리드가 한다 — 보고에 "05 §5.5에 넣을 문장" 1–2줄.
- 시험: `loom daemon stop` 후 1 s 안에 worker 프로세스가 사라지고 같은 `LOOM_HOME`으로 즉시 재기동 → echo 송신 성공; 사용자 STOP 중 정상 종료 → 재기동 후 여전히 latched; gateway kill -9는 기존 orphan 경로(이미 있는 시험 유지).

## 품질·보고
`cargo fmt -p` 3개 crate `--check`, `cargo clippy --workspace --exclude loom-uart-worker --exclude loom-bench --all-targets -- -D warnings`, `cargo test -p loom-gateway -p loom-worker-sdk -p loom-worker-host -p loom-cli --no-fail-fast`(3회 반복 통과 확인 — 타이밍 시험이 부하에 약한지 함께 보고). 커밋하지 않는다. 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-shutdown-opus.md`. 화면에는 경로와 3줄 요약만.
