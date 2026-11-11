docs/plan/02-core-principle.md 준수

> **질문 창 금지**(playbook §6): 대화형 질문 창을 띄우지 않는다. 계약과 이 지시서로 판단해 진행하고 결정을 보고 파일에 적는다. 판단할 수 없으면 질문을 보고 파일에 적고 할 수 있는 부분까지 끝낸다.
> **경로**: 항상 `/Users/ctmctm/workspace/Loom/...`. 보고서는 **저장소 루트** `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/`에 쓴다(worktree 안 아님 — 지난번 보고는 리드가 옮겼다).

# P1 관문 준비 — core-sol: soak 하네스와 기준 측정

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`(기준 `2b261d9`). 근거: 마스터 §3.0 P1 관문("30분 연속 RSS 증가 ≤ 10 MiB"), `docs/plan/core/plan.md` §9.3·§9.4·§9.6, 하네스 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-journal-wire-opus.md` J3.
**범위 경계**: `crates/loom-bench`만. core-opus가 동시에 `crates/loom-gateway`(IPC 연결)를 고친다 — 건드리지 말 것. 다른 crate·루트 `Cargo.toml` 수정 금지(필요하면 보고).

## B1. `loom-bench soak`
- `loom-bench soak --minutes N [--profile mixed] [--out <json>]`: 임시 `LOOM_HOME`으로 실제 `loomd`(빌드된 바이너리, `CARGO_BIN_EXE` 또는 target 경로)를 띄우고 부하를 건다:
  - echo 쓰기 루프(agent 클라이언트 초당 20건, 가끔 idempotency 키),
  - viewer 2개: 하나는 정상 구독, 하나는 **2 s마다 읽기를 멈추는 느린 구독**(gap 발생 확인), 5분마다 재연결,
  - `core.events.query` 반복(링 밖 질의 포함), `core.mark`, 1분마다 STOP→release 1회,
  - CLI 프로세스 `loom echo send`를 10 s마다 1회(자동 기동 경로 제외 — 데몬은 이미 떠 있음).
- 1 s마다 `loomd`(그리고 IPC 모드면 worker 프로세스들)의 RSS·열린 FD/핸들 수·스레드 수를 **외부 명령으로** 표본(macOS·Linux `ps -o rss= -p <pid>`, FD는 Linux `/proc/<pid>/fd`, macOS `lsof -p` 1분마다 — `unsafe` 금지). Windows는 P2(보고).
- 결과 JSON: 시간열 + 판정. 판정(plan §9.6 축약): 워밍업 2분 제외, **PID 수명 구간별** 선형 기울기(MiB/h)와 "마지막 5분 중앙값 − 처음 5분 중앙값", FD·스레드 증가 추세, 정상 viewer의 수신 누락(gap 카운터와 정합), 쓰기 실패 수. P1 관문 판정 = 30분 실행에서 RSS 증가 ≤ 10 MiB.
- 개발 실행은 **10분 이내**(playbook). 30분 관문은 리드가 별도로 돌린다 — 그래서 `--minutes`를 받고, 10분 실행 결과로 추세를 보고.

## B2. 기준 측정 재실행
- J3 하네스(코어 게이트 idle/burst, CLI 왕복)를 **release 빌드로** 3회씩 돌려 범위를 보고(머신 부하가 있으면 그 사실도). 결과 파일은 `bench/results/`에(커밋 여부는 리드 판단).

## 품질·보고
`cargo fmt --all --check`, `cargo clippy -p loom-bench --all-targets -- -D warnings`, `cargo test -p loom-bench`. 커밋하지 않는다. 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-sol-b.md`(B1 설계·10분 실행 결과 표(RSS 기울기·증가량, FD, 스레드, gap, 실패)·B2 수치·한계). 화면에는 경로와 3줄 요약만.
