# TEST P1 공통 규칙 (두 워커 모두 읽기)

정본: `docs/plan/test/plan.md`(특히 §3.1 증거 검증 모델, §4 CLI, §6 형식, 결정 T0–T14)와 CORE 계약 `docs/contract/07-test-session.md`. 최우선 원칙(`docs/plan/02-core-principle.md`): 사용자는 테스트 실행을 UI에서 실시간으로 보고 개입하며, **증거 없는 PASS는 없다**.

## 작업 위치·소유
- 코드는 **`/Users/ctmctm/workspace/Loom/.worktrees/test`(브랜치 `test/main`)**에서만 고친다. 옛 Desktop 경로는 금지(플레이북 §6).
- TEST 소유 crate는 `crates/loom-test-*`와 `skills/loom-test/`다. 나머지는 읽기만 한다. 루트 `Cargo.toml`·CORE crate는 고치지 않는다.
- **`test`는 CORE 예약 domain**이다(`loom_proto::rpc::RESERVED_DOMAINS`). 세션 RPC 모듈은 `Registry::for_core("test")`로 만들고, `loomd` 등록은 CORE가 한다(리드가 요청).
- 커밋은 리드가 한다. 보고서: `/Users/ctmctm/workspace/Loom/docs/impl/test/reports/<task>.md`.

## crate 규칙
- workspace member: `version/edition/rust-version/publish.workspace = true`, `[lints] workspace = true`(unsafe 금지, unwrap 금지, todo 금지).
- 의존은 workspace에 있으면 `.workspace = true`, 없으면 crate에 **정확한 버전**(`=x.y.z`)으로 둔다. 새 외부 crate는 보고서의 "CORE workspace.dependencies 요청"에 적는다.
- **모든 상한은 코드 상수**로 둔다(C2, 계약 07 §7.1·§7.3-7): AST 깊이·노드, 정규식 크기, 평가 이벤트 ≤ 200k, CPU ≤ 200 ms(`EvalBudget`), 파일 크기, 단계 ≤ 500, REF ≤ 64/판정 등. 넘칠 때 동작을 doc 주석에 쓴다.
- 입력 파서(.tc.md, plan YAML, 술어 JSON)는 어떤 입력에도 panic 금지. 무작위 입력 10만 회 시험을 둔다(결정적 시드, 외부 fuzz crate 없이).
- **누락 필드·타입 불일치는 false가 아니라 오류**(계약 07 §7.4).
- Python 금지.

## 빌드·관문
- explore-opus: `CARGO_HOME=$HOME/.cache/loom-eth-cargo CARGO_TARGET_DIR=$HOME/.cache/loom-target/test-ws`
- explore-sol(Codex, 쓰기는 저장소 안만): `CARGO_HOME=/Users/ctmctm/workspace/Loom/.worktrees/test/.cache/cargo CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/.worktrees/test/target`
- 관문(worktree 루트): `cargo fmt --all --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo test --workspace`. 명령 1회 10분 이내.

## 질문 대화상자 금지 (플레이북 §6)
대화형 질문 창을 열지 않는다. 계약·계획대로 스스로 정하고 보고서 "결정"에 적는다. 정할 수 없으면 보고서 "질문"에 쓰고, 그 부분은 안전한 쪽(미구현 = `E_NOT_IMPLEMENTED`, 가짜 성공 금지)으로 두고 나머지를 끝낸다.
