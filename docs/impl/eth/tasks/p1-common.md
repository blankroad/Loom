# ETH P1 공통 규칙 (두 워커 모두 읽기)

P0 규칙(`p0-common.md`)은 그대로다. 아래만 바뀌거나 추가된다.

## 바뀐 것
- CORE workspace가 main에 들어왔고 eth/main에 병합되었다. eth crate는 이제 workspace member다(`[lints] workspace = true`, 루트 `rustfmt.toml`). **루트 `Cargo.toml`은 CORE 소유라 고치지 않는다.** 외부 crate 추가가 필요하면 보고서에 적는다(리드가 `docs/impl/requests/eth-core.md`에 올림).
- 빌드 환경: `CARGO_HOME=$HOME/.cache/loom-eth-cargo CARGO_TARGET_DIR=$HOME/.cache/loom-target/eth-ws`.
- 관문(worktree 루트에서):
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo clippy -p loom-eth --features live --all-targets -- -D warnings`
  - `cargo test --workspace`
- 새 도메인 crate: **`crates/loom-eth`**(gateway·worker·CLI 세 조각, CORE 계약 §8.1).
  - 템플릿: `crates/loom-echo`.
  - **공유 모델**: `crates/loom-eth/src/model.rs` = 스키마 v1이다. 요약은 `docs/impl/eth/schema-v1.md`, UI 합의는 `docs/impl/requests/ui-eth.md`의 `## eth 답변`.
  - model.rs·lib.rs는 **리드 소유**다. 바꿔야 하면 고치지 말고 보고서에 적는다. 급하면 리드에게 남긴 메모를 보고서 맨 위에 둔다.

## 참고할 CORE 계약·코드
- 계약: `docs/contract/`.
  - 02-events: 이벤트 필드·gap kind
  - 03-cli: 봉투·종료 코드·커서
  - 04-safety: 위험·F1 자동
  - 05-worker-protocol
  - 06-journal
  - 07-test-session: 증거 검증
  - 08-domain-integration
- 안정 API(이것만 의존): `loom-proto`, `loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit`, `loom-testkit`.
- **아직 없는 것(CORE P1 진행 중)**:
  - 실제 gateway/허브, `run_worker` IPC 루프, `loom-journal` writer, `loom`·`loomd` 바이너리의 도메인 등록
  - 그래서 이번 시험은 **testkit**(`RegistryProbe`, `HookHarness`, `FakeWorker`, fixtures)과 worker-sdk의 `MemorySink`/`SimClock`으로 한다.
  - 빠진 CORE 기능은 흉내 내지 말고 보고서의 "CORE 요청" 목록에 적는다(가짜 성공 금지 — 미구현은 `E_NOT_IMPLEMENTED`).
- 이벤트 kind·RPC 이름·필드는 `model.rs`와 `schema-v1.md`를 그대로 따른다. UI 팀이 이 모양으로 패널을 이미 만들었다(`ui/src/panels/eth/ethModel.ts` 읽기만).

## 핵심 원칙 확인 (관문)
- 에이전트 CLI가 보는 패킷과 UI가 보는 패킷은 같은 `eth.pkt` 레코드다. 별도 경로를 만들지 않는다.
- 사용자 개입(캡처 정지·송신·STOP)은 같은 RPC로 들어오고 `others_since_last`로 에이전트가 본다(CORE 처리).
- 증거 없는 PASS 금지: eth 증거 검증기는 `attributed_origin`이 있는 패킷을 device 관측으로 세지 않는다(TEST R-8).

## 추가 (2026-11-11, 플레이북 §6)
- 옛 Desktop 경로를 쓰지 않는다. 항상 `/Users/ctmctm/workspace/Loom/...`(eth worktree는 `/Users/ctmctm/workspace/Loom/.worktrees/eth`).
- **explore-sol(Codex, 쓰기는 작업 폴더 안만)**: `~/.cache`에 쓸 수 없으므로 빌드 환경을 worktree 안(둘 다 gitignore 대상)으로 바꾼다:
  `CARGO_HOME=/Users/ctmctm/workspace/Loom/.worktrees/eth/.cache/cargo CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/.worktrees/eth/target`
  첫 빌드에 의존성 다운로드가 있다(정상). 네트워크가 막혀 있으면 실패 내용을 보고서에 적고 멈춘다 — 리드가 대신 빌드한다.
- explore-opus는 기존 `~/.cache` 경로를 그대로 써도 된다.

## 질문 대화상자 금지 (플레이북 §6, 2026-11-11)
- 작업 중 **대화형 질문 창(선택지를 묻는 UI·AskUserQuestion 등)을 열지 않는다.** 리드나 사람이 답할 때까지 멈추게 된다.
- 판단이 필요하면 계약(`docs/contract/`)·계획(`docs/plan/eth/plan.md`)·스키마(`docs/impl/eth/schema-v1.md`)에 따라 **스스로 정하고**, 그 결정을 보고서의 "결정" 목록에 적는다.
- 계약으로 정할 수 없으면 보고서의 "질문" 목록에 쓰고, 그 부분은 안전한 쪽(미구현은 `E_NOT_IMPLEMENTED`, 가짜 성공 금지)으로 두고 나머지를 끝낸다.
