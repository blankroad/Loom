# CAN P0 — 공통 규칙 (can-opus, can-sol)

## 어디서 일하나
- **worktree: `/Users/ctmctm/workspace/Loom/.worktrees/can`** (브랜치 `can/main`). 모든 파일은 여기서 만든다. 저장소 루트(`/Users/ctmctm/workspace/Loom`)의 코드는 건드리지 않는다.
- 배정받은 crate 폴더 **하나만** 수정한다. 다른 워커와 같은 worktree를 쓰므로 남의 폴더·`crates/loom-can-frame`·`docs/` 다른 파일은 고치지 않는다. `loom-can-frame`에 필요한 변경이 있으면 보고서에 요청으로 적는다(리드가 고침).
- 루트 `Cargo.toml`을 만들지 않는다(CORE 소유). 각 crate는 독립 crate로 `cargo test --manifest-path crates/<x>/Cargo.toml`. crate 폴더에 `.gitignore`로 `Cargo.lock` 무시(이미 `loom-can-frame` 참고).
- 커밋하지 않는다(리드가 검토 후 커밋).
- **대화형 질문 창을 열지 않는다**(선택지 질문 도구 등). 결정이 필요하면 가장 합리적인 기본값으로 진행하고, 무엇을 왜 골랐는지 보고서의 "리드 결정 요청"에 적는다. 리드 소유 파일 변경이 꼭 필요해 진행할 수 없으면 그 부분만 남기고 나머지를 끝낸 뒤 보고한다. (2026-11-11, 메인 지시)
- 경로는 항상 `/Users/ctmctm/workspace/Loom/...`(옛 Desktop 경로 금지).

## 먼저 읽을 것
- `docs/plan/02-core-principle.md`, `docs/impl/00-impl-playbook.md` §4 품질 기준
- `docs/plan/can/plan.md` v2.0: §3.3.1(스레드 모델), §3.6(ISO-TP), §7(라이브러리), §9.4(C2 상한), §9.5(증거 필드)
- 공통 타입: `crates/loom-can-frame/src/lib.rs` (`CanId`, `CanFrame`(고정 크기, CC/FD 검증), `len_to_dlc`·`dlc_to_len`·`fd_padded_len`, `parse_frame`, `TimedFrame`)

## 품질 기준 (전부 통과해야 보고)
- Rust stable 1.93, edition 2024, `rust-version = "1.93"`, `publish = false`.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과.
- `unsafe` 금지. Python 금지.
- **C2:** 무한 큐·무한 성장 금지. 모든 버퍼·컬렉션에 상한과 넘칠 때 동작(오류 반환)을 코드에 명시하고 시험한다.
- 공개 API에 짧은 doc 주석. 코드 주석은 짧고 쓸모 있게.
- 외부 crate는 필요한 것만(crates.io에서 받는 것은 허용, `cargo install` 금지). 추가한 crate와 이유를 보고서에 적는다.
- 장시간 시험·벤치는 10분 이내.

## 보고
끝나면 `docs/impl/can/reports/<작업 이름>.md`에 짧게: 만든 것, 공개 API 요약, 시험 목록과 결과(통과 수), 계획과 다르게 한 점과 이유, 남은 것·리드에게 요청. 그리고 멈춘다.
